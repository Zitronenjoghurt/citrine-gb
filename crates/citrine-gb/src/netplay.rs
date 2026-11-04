mod input;
mod local;
pub(crate) mod pair;
mod peer;
pub(crate) mod state;

pub use input::{CableEvent, Input, MAX_UNITS, Player};
pub use local::LocalCable;
pub use pair::LinkPort;
pub use peer::{LinkPeer, PeerCable};

use crate::console::{ConsoleKind, Rollback};
use crate::error::{GbError, GbResult};
use crate::gb::ppu::types::color::{ColorCorrection, DmgColors};
use crate::gb::ppu::types::theme::DmgTheme;
use crate::gb::ram_init::RamInit;
use crate::gb::{GameBoy, GbModel};
#[cfg(feature = "gba")]
use crate::gba::machine::Gba;
use crate::persistence::sram_dump::SramDump;
use crate::rom::Rom;
use input::{InputLog, LOG_LEN};
use pair::Cable;
use state::Snapshot;
use std::collections::VecDeque;

pub const MAX_ROLLBACK_FRAMES: u32 = 30;
pub const DEFAULT_CHECK_INTERVAL: u32 = 60;
pub const NEVER: u32 = u32::MAX;
const LOCAL_AHEAD: u32 = 128;
const REMOTE_AHEAD: u32 = 256;
const KEPT_CHECKS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sync {
    pub budget: u32,
    pub delay: u32,
}

impl Default for Sync {
    fn default() -> Self {
        Self::new(MAX_ROLLBACK_FRAMES, 0)
    }
}

impl Sync {
    pub const fn new(budget: u32, delay: u32) -> Self {
        Self { budget, delay }
    }

    pub const fn lockstep(delay: u32) -> Self {
        Self::new(0, delay)
    }

    pub fn budget(self) -> u32 {
        self.budget.min(MAX_ROLLBACK_FRAMES)
    }

    pub fn with_delay(self, delay: u32) -> Self {
        Self { delay, ..self }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SessionConfig<const N: usize = 2> {
    pub local: Player,
    pub seed: u64,
    pub start_time: u64,
    pub start_frame: u32,
    pub cable_plugged: bool,
    pub check_interval: u32,
    pub sample_rate: u32,
    pub power_on: [u32; N],
    pub sync: Sync,
}

impl<const N: usize> SessionConfig<N> {
    pub fn new(local: Player, seed: u64, start_time: u64) -> Self {
        Self {
            local,
            seed,
            start_time,
            start_frame: 0,
            cable_plugged: false,
            check_interval: DEFAULT_CHECK_INTERVAL,
            sample_rate: 44_100,
            power_on: [0; N],
            sync: Sync::default(),
        }
    }
}

pub struct ConsoleSetup<'a> {
    pub rom: &'a Rom,
    pub kind: ConsoleKind,
    pub save: Option<SramDump>,
    pub state: Option<&'a [u8]>,
}

pub fn encode_state<C: Rollback>(console: &C) -> GbResult<Vec<u8>> {
    console.encode_state()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advance {
    Ran,
    AwaitingLocal,
    AwaitingRemote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Check<const N: usize = 2> {
    pub frame: u32,
    pub hashes: [u64; N],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Desync<const N: usize = 2> {
    pub frame: u32,
    pub consoles: [bool; N],
}

impl<const N: usize> Desync<N> {
    pub fn differs(&self, player: Player) -> bool {
        self.consoles[player.index()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    #[error("frame {0} already ran")]
    Past(u32),
    #[error("frame {0} is too far ahead")]
    TooFarAhead(u32),
    #[error("frame {0} already has a different input")]
    Conflict(u32),
}

#[derive(Debug, thiserror::Error)]
pub enum ResyncError {
    #[error("no checkpoint for frame {0}")]
    NoCheckpoint(u32),
    #[error(transparent)]
    Decode(#[from] GbError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MemberError {
    #[error("frame {0} is already confirmed")]
    Past(u32),
    #[error("player {0} is already in the session")]
    Present(usize),
    #[error("player {0} is not in the session")]
    Absent(usize),
    #[error("player {0} is this session's own player")]
    Local(usize),
}

struct Join<C> {
    at: u32,
    unit: usize,
    console: Box<C>,
}

pub struct Session<C: Rollback + LinkPort, const N: usize = 2> {
    config: SessionConfig<N>,
    frame: u32,
    consoles: Box<[C; N]>,
    cable: Cable<C::Message, N>,
    inputs: [InputLog; N],
    leave_at: [u32; N],
    joins: Vec<Join<C>>,
    used: Box<[[Input; N]]>,
    ring: Box<[Snapshot<C, N>]>,
    rollback_from: Option<u32>,
    last_rollback: u32,
    next_check: u32,
    outbox: VecDeque<Check<N>>,
    local_checks: VecDeque<Check<N>>,
    remote_checks: VecDeque<Check<N>>,
    compared: Option<u32>,
    desync: Option<Desync<N>>,
    checkpoints: Box<[Snapshot<C, N>; 2]>,
    scratch: Box<C>,
    saved: Option<Vec<u8>>,
    watching: bool,
    present: [bool; N],
}

impl<const N: usize> Session<GameBoy, N> {
    pub fn new(config: SessionConfig<N>, setups: [Option<ConsoleSetup>; N]) -> GbResult<Self> {
        let present = setups.each_ref().map(Option::is_some);
        let mut consoles = Vec::with_capacity(N);
        for (setup, player) in setups.into_iter().zip(Player::ALL) {
            consoles.push(match setup {
                Some(setup) => new_console(setup, &config, player)?,
                None => GameBoy::new_empty(GbModel::Dmg),
            });
        }
        let Ok(consoles) = consoles.try_into() else {
            unreachable!("one console per setup")
        };
        Ok(Self::with_consoles(config, consoles, present))
    }

    pub fn set_display(
        &mut self,
        player: Player,
        theme: DmgTheme,
        colors: DmgColors,
        correction: ColorCorrection,
    ) {
        let ppu = &mut self.consoles[player.index()].ppu;
        ppu.dmg_theme = theme;
        ppu.dmg_colors = colors;
        ppu.color_correction = correction;
    }
}

#[cfg(feature = "gba")]
impl<const N: usize> Session<Gba, N> {
    pub fn new_gba(config: SessionConfig<N>, setups: [Option<ConsoleSetup>; N]) -> GbResult<Self> {
        let present = setups.each_ref().map(Option::is_some);
        let mut consoles = Vec::with_capacity(N);
        for setup in setups {
            consoles.push(match setup {
                Some(setup) => new_gba(setup, &config)?,
                None => Gba::new(&[]),
            });
        }
        let Ok(consoles) = consoles.try_into() else {
            unreachable!("one console per setup")
        };
        Ok(Self::with_consoles(config, consoles, present))
    }
}

impl<C: Rollback + LinkPort, const N: usize> Session<C, N> {
    pub fn with_consoles(
        config: SessionConfig<N>,
        mut consoles: [C; N],
        present: [bool; N],
    ) -> Self {
        const { assert!(N >= 2 && N <= MAX_UNITS) };
        let config = settled_config(config, present);
        for (console, here) in consoles.iter_mut().zip(present) {
            if here {
                console.set_sample_rate(config.sample_rate);
            }
        }
        let start = config.start_frame;
        let interval = config.check_interval;
        let ring_len = config.sync.budget().max(1) as usize;
        let cable = Cable {
            plugged: config.cable_plugged,
            ..Cable::default()
        };
        let mut session = Self {
            config,
            frame: start,
            cable,
            inputs: std::array::from_fn(|_| InputLog::starting_at(start)),
            leave_at: [NEVER; N],
            joins: Vec::new(),
            used: vec![[Input::default(); N]; LOG_LEN].into_boxed_slice(),
            ring: (0..ring_len).map(|_| Snapshot::new(&consoles)).collect(),
            rollback_from: None,
            last_rollback: 0,
            next_check: start.div_ceil(interval) * interval,
            outbox: VecDeque::new(),
            local_checks: VecDeque::new(),
            remote_checks: VecDeque::new(),
            compared: None,
            desync: None,
            checkpoints: Box::new([Snapshot::new(&consoles), Snapshot::new(&consoles)]),
            scratch: Box::new(consoles[0].clone_state()),
            saved: None,
            watching: false,
            present,
            consoles: Box::new(consoles),
        };
        session.set_catch_up(false);
        session
    }

    pub fn frame(&self) -> u32 {
        self.frame
    }

    pub fn local(&self) -> Player {
        self.config.local
    }

    pub fn sync(&self) -> Sync {
        self.config.sync
    }

    pub fn console(&self, player: Player) -> &C {
        &self.consoles[player.index()]
    }

    pub fn into_console(self, player: Player) -> C {
        let mut consoles = (self.consoles as Box<[C]>).into_vec();
        consoles.swap_remove(player.index())
    }

    pub fn into_confirmed_console(mut self, player: Player) -> C {
        let frame = self.confirmed_frame();
        let slot = &self.ring[self.slot(frame)];
        if frame < self.frame && slot.frame == Some(frame) {
            self.consoles[player.index()].copy_state_from(&slot.consoles[player.index()]);
        }
        self.into_console(player)
    }

    pub fn keeping_confirmed(mut self, frames: u32) -> Self {
        let len = self.config.sync.budget().max(1) + frames;
        self.ring = (0..len).map(|_| Snapshot::new(&self.consoles)).collect();
        self
    }

    pub fn confirmed_states(&self, player: Player) -> Vec<(u32, C)> {
        let confirmed = self.confirmed_frame();
        let unit = player.index();
        let fresh = |frame: u32| self.rollback_from.is_none_or(|from| frame <= from);
        let mut states: Vec<(u32, C)> = self
            .ring
            .iter()
            .filter_map(|slot| slot.frame.map(|frame| (frame, slot)))
            .filter(|&(frame, _)| frame <= confirmed && frame < self.frame && fresh(frame))
            .map(|(frame, slot)| (frame, slot.consoles[unit].clone_state()))
            .collect();
        if confirmed == self.frame && self.rollback_from.is_none() {
            states.push((confirmed, self.consoles[unit].clone_state()));
        }
        states.sort_by_key(|&(frame, _)| frame);
        states
    }

    pub fn present(&self, player: Player) -> bool {
        self.present[player.index()]
    }

    pub fn left_by(&self, player: Player, frame: u32) -> bool {
        self.leave_at[player.index()] <= frame
    }

    pub fn powered(&self, player: Player) -> bool {
        self.on(player.index(), self.frame)
    }

    pub fn cable_plugged(&self) -> bool {
        self.cable.plugged
    }

    pub fn cable_transfers(&self) -> [u64; N] {
        self.cable.transfers
    }

    pub fn confirmed_frame(&self) -> u32 {
        self.remote_frontier().min(self.frame)
    }

    pub fn frames_ahead(&self) -> u32 {
        self.frame - self.confirmed_frame()
    }

    pub fn remote_frame(&self) -> u32 {
        self.remotes()
            .filter(|&i| self.leave_at[i] == NEVER)
            .map(|i| self.inputs[i].latest().map_or(0, |frame| frame + 1))
            .min()
            .unwrap_or(0)
    }

    pub fn waiting_for(&self) -> Option<Player> {
        let frontier = self.remote_frontier();
        (frontier <= self.frame)
            .then(|| self.remotes().find(|&i| self.unit_frontier(i) == frontier))
            .flatten()
            .map(|i| Player::ALL[i])
    }

    pub fn last_rollback(&self) -> u32 {
        self.last_rollback
    }

    pub fn desync(&self) -> Option<Desync<N>> {
        self.desync
    }

    pub fn watch_remote(&mut self, on: bool) {
        self.watching = on;
        self.set_catch_up(false);
    }

    pub fn watching(&self) -> bool {
        self.watching
    }

    pub fn add_local_input(&mut self, frame: u32, input: Input) -> Result<(), InputError> {
        if frame < self.frame {
            return Err(InputError::Past(frame));
        }
        if frame >= self.frame + LOCAL_AHEAD {
            return Err(InputError::TooFarAhead(frame));
        }
        let log = &mut self.inputs[self.config.local.index()];
        match log.get(frame) {
            Some(existing) if existing == input => Ok(()),
            Some(_) => Err(InputError::Conflict(frame)),
            None => {
                log.set(frame, input);
                Ok(())
            }
        }
    }

    pub fn add_remote_input(&mut self, frame: u32, input: Input) {
        self.add_player_input(self.config.local.other(), frame, input);
    }

    pub fn add_player_input(&mut self, player: Player, frame: u32, input: Input) {
        let unit = player.index();
        if unit >= N
            || player == self.config.local
            || !self.present[unit]
            || frame >= self.leave_at[unit]
        {
            return;
        }
        let log = &mut self.inputs[unit];
        if !log.accepts(frame, REMOTE_AHEAD) || log.get(frame).is_some() {
            return;
        }
        log.set(frame, input);
        if frame < self.frame && self.used[frame as usize % LOG_LEN][unit] != input {
            self.rewind(frame);
        }
    }

    pub fn join(&mut self, player: Player, at: u32, console: C) -> Result<(), MemberError> {
        let unit = player.index();
        if self.present[unit] {
            return Err(MemberError::Present(unit));
        }
        self.reschedule(at)?;
        let mut console = Box::new(console);
        console.set_sample_rate(self.config.sample_rate);
        self.present[unit] = true;
        self.config.power_on[unit] = at;
        self.inputs[unit] = InputLog::starting_at(at);
        self.joins.push(Join { at, unit, console });
        self.set_catch_up(false);
        Ok(())
    }

    pub fn leave(&mut self, player: Player, at: u32) -> Result<(), MemberError> {
        let unit = player.index();
        if player == self.config.local {
            return Err(MemberError::Local(unit));
        }
        if !self.present[unit] || self.leave_at[unit] != NEVER {
            return Err(MemberError::Absent(unit));
        }
        self.reschedule(at)?;
        self.leave_at[unit] = at;
        Ok(())
    }

    pub fn advance(&mut self) -> Advance {
        self.last_rollback = 0;
        self.settle();
        let local = self.config.local.index();
        if self.present[local] && self.inputs[local].get(self.frame).is_none() {
            return Advance::AwaitingLocal;
        }
        let budget = if self.present[local] {
            self.config.sync.budget()
        } else {
            0
        };
        let next = self.frame + 1;
        if next - self.remote_frontier().min(next) > budget {
            return Advance::AwaitingRemote;
        }
        self.run(self.frame);
        self.frame = next;
        let confirmed = self.confirmed_frame();
        self.joins.retain(|join| join.at >= confirmed);
        Advance::Ran
    }

    pub fn drain_audio(&mut self, out: &mut Vec<f32>) {
        let heard = self.heard();
        self.consoles[heard].take_audio(out);
    }

    pub fn drain_remote_audio(&mut self, out: &mut Vec<f32>) {
        if let Some(friend) = self.friend() {
            self.consoles[friend].take_audio(out);
        }
    }

    pub fn take_check(&mut self) -> Option<Check<N>> {
        self.outbox.pop_front()
    }

    pub fn add_remote_check(&mut self, check: Check<N>) {
        keep(&mut self.remote_checks, check);
        self.compare(check.frame);
    }

    pub fn checkpoint(&self, frame: u32, player: Player) -> Option<Vec<u8>> {
        let checkpoint = self.checkpoints.iter().find(|c| c.frame == Some(frame))?;
        checkpoint.consoles[player.index()].encode_state().ok()
    }

    pub fn latest_checkpoint(&self) -> Option<u32> {
        self.checkpoints.iter().filter_map(|c| c.frame).max()
    }

    pub fn checkpoint_cable(&self, frame: u32) -> Option<Vec<u8>> {
        let checkpoint = self.checkpoints.iter().find(|c| c.frame == Some(frame))?;
        checkpoint.cable.encode().ok()
    }

    pub fn restore_cable(&mut self, data: &[u8]) -> GbResult<()> {
        self.cable.decode(data)
    }

    pub fn resync(&mut self, frame: u32, player: Player, data: &[u8]) -> Result<(), ResyncError> {
        let index = self
            .checkpoints
            .iter()
            .position(|c| c.frame == Some(frame))
            .filter(|_| self.frame - frame < LOG_LEN as u32 - REMOTE_AHEAD)
            .ok_or(ResyncError::NoCheckpoint(frame))?;
        let checkpoint = &mut self.checkpoints[index];
        let console = checkpoint.consoles[player.index()].decode_state(data)?;
        checkpoint.consoles[player.index()].copy_state_from(&console);
        checkpoint.load(&mut self.consoles, &mut self.cable);
        self.rollback_from = None;
        self.next_check = frame;
        self.local_checks.retain(|c| c.frame < frame);
        self.desync = None;
        self.replay(frame);
        self.emit_checks(self.frame);
        Ok(())
    }

    pub fn local_save(&mut self, force: bool) -> Option<SramDump> {
        if !self.present[self.config.local.index()] {
            return None;
        }
        self.settle();
        let frame = self.confirmed_frame();
        let local = self.config.local.index();
        let timing = self.consoles[local].timing();
        let now = self.config.start_time
            + frame as u64 * u64::from(timing.cycles_per_frame) / u64::from(timing.clock_hz);
        let console = if frame == self.frame {
            &mut self.consoles[local]
        } else {
            let slot = &mut self.ring[self.slot(frame)];
            if slot.frame != Some(frame) {
                return None;
            }
            &mut slot.consoles[local]
        };
        let save = console.battery(true, now)?;
        if !force && self.saved.as_ref() == Some(&save.ram) {
            return None;
        }
        let dump = SramDump::from_slice(&save.to_file());
        self.saved = Some(save.ram);
        Some(dump)
    }

    fn remotes(&self) -> impl Iterator<Item = usize> + '_ {
        let local = self.config.local.index();
        (0..N).filter(move |&i| i != local && self.present[i])
    }

    fn unit_frontier(&self, unit: usize) -> u32 {
        let frontier = self.inputs[unit].frontier();
        if frontier >= self.leave_at[unit] {
            NEVER
        } else {
            frontier
        }
    }

    fn remote_frontier(&self) -> u32 {
        self.remotes()
            .map(|i| self.unit_frontier(i))
            .min()
            .unwrap_or(NEVER)
    }

    fn on(&self, unit: usize, frame: u32) -> bool {
        frame >= self.config.power_on[unit] && frame < self.leave_at[unit]
    }

    fn slot(&self, frame: u32) -> usize {
        frame as usize % self.ring.len()
    }

    fn rewind(&mut self, frame: u32) {
        self.rollback_from = Some(self.rollback_from.map_or(frame, |from| from.min(frame)));
    }

    fn reschedule(&mut self, at: u32) -> Result<(), MemberError> {
        if at < self.confirmed_frame() {
            return Err(MemberError::Past(at));
        }
        if at < self.frame {
            self.rewind(at);
        }
        Ok(())
    }

    fn heard(&self) -> usize {
        heard_index(&self.config, self.present)
    }

    fn friend(&self) -> Option<usize> {
        let heard = self.heard();
        self.remotes().find(|&i| i != heard)
    }

    fn settle(&mut self) {
        if let Some(from) = self.rollback_from.take() {
            self.emit_checks(from);
            let slot = &self.ring[self.slot(from)];
            if slot.frame == Some(from) && !self.replay_remote(from) {
                let slot = &self.ring[self.slot(from)];
                slot.load(&mut self.consoles, &mut self.cable);
                self.replay(from);
            }
        }
        self.emit_checks(self.frame);
    }

    fn replay(&mut self, from: u32) {
        self.set_catch_up(true);
        for frame in from..self.frame {
            if frame + 1 == self.frame {
                self.set_catch_up(false);
                self.mute_all();
            }
            self.run(frame);
        }
        self.set_catch_up(false);
        self.last_rollback = self.frame - from;
    }

    fn set_catch_up(&mut self, on: bool) {
        let local = self.config.local.index();
        let heard = self.heard();
        let friend = self.friend();
        for (i, console) in self.consoles.iter_mut().enumerate() {
            let hidden = i != local && !self.watching;
            let audible = i == heard || (i != local && Some(i) == friend);
            console.set_blind(on || hidden);
            console.set_muted(on || hidden || !audible);
        }
    }

    fn mute_all(&mut self) {
        for console in self.consoles.iter_mut() {
            console.set_muted(true);
        }
    }

    fn run(&mut self, frame: u32) {
        for join in self.joins.iter().filter(|join| join.at == frame) {
            self.consoles[join.unit].copy_state_from(&join.console);
            self.cable.reseat();
        }
        let frontier = self.remote_frontier();
        let slot = self.slot(frame);
        self.ring[slot].save(frame, &self.consoles, &self.cable);
        if frame == self.next_check && frontier >= frame {
            let hashes = state::hash_consoles(&mut *self.scratch, &self.consoles, &self.cable);
            let index = self.checkpoint_index(frame);
            self.checkpoints[index].save(frame, &self.consoles, &self.cable);
            self.record_check(Check { frame, hashes });
        }

        let inputs = self.inputs_for(frame);
        let on = std::array::from_fn(|i| self.on(i, frame));
        pair::run_frame(&mut self.consoles, &mut self.cable, &inputs, on);
    }

    fn inputs_for(&mut self, frame: u32) -> [Input; N] {
        let local = self.config.local.index();
        let inputs = std::array::from_fn(|i| {
            let log = &self.inputs[i];
            if frame >= self.leave_at[i] {
                Input::default()
            } else if i == local {
                log.get(frame).unwrap_or_default()
            } else {
                log.get(frame).unwrap_or_else(|| log.predict(frame))
            }
        });
        self.used[frame as usize % LOG_LEN] = inputs;
        inputs
    }

    fn quiet_since(&self, from: u32) -> bool {
        let local = self.config.local.index();
        let slot = &self.ring[self.slot(from)];
        let outside = |&at: &u32| at < from || at >= self.frame;
        slot.cable.messages == self.cable.messages
            && slot.cable.stalls[local] == self.cable.stalls[local]
            && slot.cable.quiet()
            && self.config.power_on.iter().all(outside)
            && self.leave_at.iter().all(outside)
            && (from..self.frame).all(|frame| {
                self.ring[self.slot(frame)].frame == Some(frame)
                    && self
                        .inputs
                        .iter()
                        .all(|log| log.get(frame).is_none_or(|input| input.cable.is_none()))
            })
    }

    fn replay_remote(&mut self, from: u32) -> bool {
        if !self.quiet_since(from) {
            return false;
        }
        let local = self.config.local.index();
        let start = &self.ring[self.slot(from)];
        for remote in (0..N).filter(|&i| i != local) {
            self.consoles[remote].copy_state_from(&start.consoles[remote]);
            self.cable.clocks[remote] = start.cable.clocks[remote];
        }
        self.set_catch_up(true);
        for frame in from..self.frame {
            let index = self.slot(frame);
            let slot = &mut self.ring[index];
            for remote in (0..N).filter(|&i| i != local) {
                slot.consoles[remote].copy_state_from(&self.consoles[remote]);
                slot.cable.clocks[remote] = self.cable.clocks[remote];
            }
            if frame == self.next_check && self.remote_frontier() >= frame {
                let slot = &self.ring[index];
                let hashes = state::hash_consoles(&mut *self.scratch, &slot.consoles, &slot.cable);
                let checkpoint = self.checkpoint_index(frame);
                self.checkpoints[checkpoint].save(frame, &slot.consoles, &slot.cable);
                self.record_check(Check { frame, hashes });
            }
            if frame + 1 == self.frame {
                self.set_catch_up(false);
                self.mute_all();
            }
            let inputs = self.inputs_for(frame);
            for remote in (0..N).filter(|&i| i != local) {
                let on = self.on(remote, frame);
                let input = inputs[remote];
                if !pair::run_solo(&mut self.consoles, &mut self.cable, remote, input, on) {
                    self.set_catch_up(false);
                    return false;
                }
            }
        }
        self.set_catch_up(false);
        self.last_rollback = self.frame - from;
        true
    }

    fn emit_checks(&mut self, until: u32) {
        while self.next_check <= until
            && self.next_check < self.frame
            && self.remote_frontier() >= self.next_check
        {
            let frame = self.next_check;
            let slot = &self.ring[self.slot(frame)];
            if slot.frame != Some(frame) {
                self.next_check += self.config.check_interval;
                continue;
            }
            let hashes = state::hash_consoles(&mut *self.scratch, &slot.consoles, &slot.cable);
            let index = self.checkpoint_index(frame);
            self.checkpoints[index].save(frame, &slot.consoles, &slot.cable);
            self.record_check(Check { frame, hashes });
        }
    }

    fn checkpoint_index(&self, frame: u32) -> usize {
        (frame / self.config.check_interval) as usize % 2
    }

    fn record_check(&mut self, check: Check<N>) {
        self.next_check = check.frame + self.config.check_interval;
        self.outbox.push_back(check);
        keep(&mut self.local_checks, check);
        self.compare(check.frame);
    }

    fn compare(&mut self, frame: u32) {
        if self.compared.is_some_and(|compared| compared > frame) {
            return;
        }
        let find = |checks: &VecDeque<Check<N>>| checks.iter().find(|c| c.frame == frame).copied();
        let (Some(local), Some(remote)) = (find(&self.local_checks), find(&self.remote_checks))
        else {
            return;
        };
        let consoles = std::array::from_fn(|i| local.hashes[i] != remote.hashes[i]);
        self.compared = Some(frame);
        self.desync = consoles
            .contains(&true)
            .then_some(Desync { frame, consoles });
    }

    #[cfg(test)]
    pub(crate) fn link_messages(&self) -> ([u64; N], [u64; N]) {
        (self.cable.messages, self.cable.transfers)
    }

    #[cfg(test)]
    pub(crate) fn rewind_to(&mut self, frame: u32) {
        let from = frame.max(self.remote_frontier());
        if from < self.frame {
            self.rewind(from);
        }
    }

    #[cfg(test)]
    pub(crate) fn corrupt(&mut self, player: Player, change: impl Fn(&mut C)) {
        change(&mut self.consoles[player.index()]);
        for slot in self.ring.iter_mut() {
            change(&mut slot.consoles[player.index()]);
        }
    }
}

fn settled_config<const N: usize>(
    config: SessionConfig<N>,
    present: [bool; N],
) -> SessionConfig<N> {
    let mut config = SessionConfig {
        check_interval: config.check_interval.max(1),
        ..config
    };
    for (at, here) in config.power_on.iter_mut().zip(present) {
        if !here {
            *at = NEVER;
        }
    }
    config
}

fn heard_index<const N: usize>(config: &SessionConfig<N>, present: [bool; N]) -> usize {
    let local = config.local.index();
    if present[local] {
        return local;
    }
    (0..N)
        .find(|&i| i != local && present[i])
        .unwrap_or(local ^ 1)
}

fn keep<const N: usize>(checks: &mut VecDeque<Check<N>>, check: Check<N>) {
    checks.retain(|c| c.frame != check.frame);
    checks.push_back(check);
    while checks.len() > KEPT_CHECKS {
        checks.pop_front();
    }
}

pub(crate) fn new_console<const N: usize>(
    setup: ConsoleSetup,
    config: &SessionConfig<N>,
    player: Player,
) -> GbResult<GameBoy> {
    let seed = mix(config.seed ^ player.index() as u64);
    let model = GbModel::try_from(setup.kind)?;
    let mut gb = GameBoy::new_empty_with_ram_init(model, RamInit::Random { seed });
    gb.load_rom(setup.rom)?;
    if let Some(data) = setup.state {
        return gb.decode_state(data);
    }
    if let Some(save) = setup.save {
        gb.put_sram_dump(save, config.start_time);
    }
    Ok(gb)
}

#[cfg(feature = "gba")]
pub(crate) fn new_gba<const N: usize>(
    setup: ConsoleSetup,
    config: &SessionConfig<N>,
) -> GbResult<Gba> {
    use crate::console::Console;
    if setup.kind != ConsoleKind::Gba {
        return Err(GbError::NotGba);
    }
    let mut gba = Gba::new(&setup.rom.data);
    if let Some(data) = setup.state {
        return gba.decode_state(data);
    }
    if let Some(save) = setup.save {
        let battery = gba.battery_from_file(save.as_slice());
        gba.put_battery(battery, config.start_time);
    }
    Ok(gba)
}

fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
