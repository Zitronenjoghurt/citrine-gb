mod input;
mod local;
mod pace;
pub(crate) mod pair;
mod peer;
pub(crate) mod state;

pub use input::{CableEvent, CablePace, Input, MAX_UNITS, Player};
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
#[cfg(feature = "nds")]
use crate::nds::machine::{Nds, NdsFirmware};
use crate::persistence::sram_dump::SramDump;
use crate::rom::Rom;
use citrine_gb_threads::{Split, Turnout};
use input::{InputLog, LOG_LEN};
use pace::Pacer;
use pair::Cable;
use state::Snapshot;
use std::collections::VecDeque;

pub const MAX_ROLLBACK_FRAMES: u32 = 30;
pub const DEFAULT_CHECK_INTERVAL: u32 = 60;
pub const NEVER: u32 = u32::MAX;
const LOCAL_AHEAD: u32 = 128;
const REMOTE_AHEAD: u32 = 256;
const KEPT_CHECKS: usize = 8;
#[cfg(feature = "gba")]
pub const DEFAULT_CABLE_WINDOW: u32 = crate::gba::serial::CABLE_WINDOW;
#[cfg(not(feature = "gba"))]
pub const DEFAULT_CABLE_WINDOW: u32 = 0;

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
    pub cable_window: u32,
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
            cable_window: DEFAULT_CABLE_WINDOW,
        }
    }
}

pub struct ConsoleSetup<'a> {
    pub rom: &'a Rom,
    pub kind: ConsoleKind,
    pub save: Option<SramDump>,
    pub state: Option<&'a [u8]>,
}

#[cfg(feature = "nds")]
pub enum NdsBoot<'a> {
    Card(ConsoleSetup<'a>),
    DownloadPlay,
}

#[cfg(feature = "nds")]
pub struct NdsSetup<'a> {
    pub boot: NdsBoot<'a>,
    pub firmware: NdsFirmware,
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
    most_budget: u32,
    next_check: u32,
    check_lag: u32,
    outbox: VecDeque<Check<N>>,
    local_checks: VecDeque<Check<N>>,
    remote_checks: VecDeque<Check<N>>,
    compared: Option<u32>,
    desync: Option<Desync<N>>,
    checkpoints: Box<[Snapshot<C, N>; 2]>,
    scratches: Box<[C; N]>,
    saved: Option<Vec<u8>>,
    watching: bool,
    present: [bool; N],
    pacer: Pacer,
    turnout: Option<Turnout>,
    local_from_peer: bool,
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
        ppu.settle();
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

#[cfg(feature = "nds")]
impl<const N: usize> Session<Nds, N> {
    pub fn new_nds(config: SessionConfig<N>, setups: [Option<NdsSetup>; N]) -> GbResult<Self> {
        let present = setups.each_ref().map(Option::is_some);
        let follows = setups.each_ref().map(|setup| {
            matches!(
                setup,
                Some(NdsSetup {
                    boot: NdsBoot::DownloadPlay,
                    ..
                })
            )
        });
        let mut first = None;
        let mut consoles = Vec::with_capacity(N);
        for setup in setups {
            match setup {
                Some(setup) => {
                    let nds = new_nds(setup, &config)?;
                    first.get_or_insert_with(|| nds.clone_state());
                    consoles.push(Some(nds));
                }
                None => consoles.push(None),
            }
        }
        let first = first.ok_or(GbError::NotNds)?;
        let mut consoles: Vec<Nds> = consoles
            .into_iter()
            .map(|nds| nds.unwrap_or_else(|| first.clone_state()))
            .collect();
        let clocks: Vec<u64> = consoles
            .iter()
            .filter(|nds| nds.on_the_air())
            .map(Nds::air_us)
            .collect();
        if clocks.windows(2).any(|pair| pair[0] != pair[1]) {
            consoles.iter_mut().for_each(Nds::leave_the_air);
        }
        let Ok(consoles) = consoles.try_into() else {
            unreachable!("one console per setup")
        };
        let mut session = Self::with_consoles(
            SessionConfig {
                cable_plugged: true,
                ..config
            },
            consoles,
            present,
        );
        session.cable.follows = follows;
        Ok(session)
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
            window: u64::from(config.cable_window),
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
            ring: (0..ring_len)
                .map(|_| Snapshot::new(&mut consoles))
                .collect(),
            rollback_from: None,
            last_rollback: 0,
            most_budget: config.sync.budget(),
            next_check: start.div_ceil(interval) * interval,
            check_lag: 0,
            outbox: VecDeque::new(),
            local_checks: VecDeque::new(),
            remote_checks: VecDeque::new(),
            compared: None,
            desync: None,
            checkpoints: Box::new([Snapshot::new(&mut consoles), Snapshot::new(&mut consoles)]),
            scratches: Box::new(consoles.each_ref().map(Rollback::scratch)),
            saved: None,
            watching: false,
            present,
            pacer: Pacer::default(),
            turnout: None,
            local_from_peer: false,
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

    pub fn local_from_peer(&self) -> bool {
        self.local_from_peer
    }

    pub fn sync(&self) -> Sync {
        self.config.sync
    }

    pub fn console(&self, player: Player) -> &C {
        &self.consoles[player.index()]
    }

    pub fn console_mut(&mut self, player: Player) -> &mut C {
        &mut self.consoles[player.index()]
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
        self.ring = (0..len)
            .map(|_| Snapshot::new(&mut self.consoles))
            .collect();
        self
    }

    pub fn trailing_checks(mut self, frames: u32) -> Self {
        self.check_lag = frames;
        self.keeping_confirmed(frames)
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

    pub fn input(&self, player: Player, frame: u32) -> Option<Input> {
        self.inputs.get(player.index())?.get(frame)
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

    pub fn cable_pace(&self) -> Option<CablePace> {
        let local = self.config.local.index();
        if !C::CABLE_WINDOWS || !self.present[local] || self.pacer.waiting_on(self.frame) {
            return None;
        }
        let agreed = if self.cable.exact[local] {
            CablePace::Exact
        } else {
            CablePace::Windows
        };
        let wish = self.pacer.wish();
        (wish != agreed).then_some(wish)
    }

    pub fn cable_turnout(&self) -> Option<Turnout> {
        self.turnout
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

    pub fn set_budget(&mut self, budget: u32) {
        self.config.sync.budget = budget.min(self.most_budget);
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
                if input.pace.is_some() {
                    self.pacer.asked(frame);
                }
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

    pub fn leave_from(&mut self, player: Player, at: u32) -> Result<(), MemberError> {
        let unit = player.index();
        if player == self.config.local {
            return Err(MemberError::Local(unit));
        }
        if !self.present[unit] || self.leave_at[unit] != NEVER {
            return Err(MemberError::Absent(unit));
        }
        if at < self.confirmed_frame()
            && (self.ring[self.slot(at)].frame != Some(at)
                || self.local_checks.iter().any(|c| c.frame >= at))
        {
            return Err(MemberError::Past(at));
        }
        self.remote_checks.retain(|c| c.frame < at);
        if at < self.frame {
            self.rewind(at);
        }
        self.leave_at[unit] = at;
        Ok(())
    }

    pub fn advance(&mut self) -> Advance {
        self.advance_on(&mut Split::inline())
    }

    pub fn advance_on(&mut self, split: &mut Split<'_>) -> Advance {
        self.last_rollback = 0;
        self.settle(split);
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
        self.run(self.frame, split);
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
        self.resync_with_cable(frame, player, data, None)
    }

    pub fn resync_with_cable(
        &mut self,
        frame: u32,
        player: Player,
        data: &[u8],
        cable: Option<&[u8]>,
    ) -> Result<(), ResyncError> {
        let index = self
            .checkpoints
            .iter()
            .position(|c| c.frame == Some(frame))
            .filter(|_| self.frame - frame < LOG_LEN as u32 - REMOTE_AHEAD)
            .ok_or(ResyncError::NoCheckpoint(frame))?;
        let checkpoint = &mut self.checkpoints[index];
        let console = checkpoint.consoles[player.index()].decode_state(data)?;
        if let Some(cable) = cable {
            checkpoint.cable.decode(cable)?;
        }
        checkpoint.consoles[player.index()].copy_state_from(&console);
        checkpoint.load(&mut self.consoles, &mut self.cable);
        self.rollback_from = None;
        self.next_check = frame;
        self.local_checks.retain(|c| c.frame < frame);
        self.desync = None;
        if player == self.config.local {
            self.local_from_peer = true;
        }
        let split = &mut Split::inline();
        self.replay(frame, split);
        self.emit_checks(self.frame, split);
        Ok(())
    }

    pub fn local_save(&mut self, force: bool) -> Option<SramDump> {
        if !self.present[self.config.local.index()] {
            return None;
        }
        self.settle(&mut Split::inline());
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

    fn settle(&mut self, split: &mut Split<'_>) {
        if let Some(from) = self.rollback_from.take() {
            self.emit_checks(from, split);
            let slot = &self.ring[self.slot(from)];
            if slot.frame == Some(from) && !self.replay_remote(from, split) {
                let slot = &self.ring[self.slot(from)];
                slot.load(&mut self.consoles, &mut self.cable);
                self.replay(from, split);
            }
        }
        self.emit_checks(self.frame, split);
    }

    fn replay(&mut self, from: u32, split: &mut Split<'_>) {
        self.set_catch_up(true);
        for frame in from..self.frame {
            if frame + 1 == self.frame {
                self.set_catch_up(false);
                self.mute_all();
            }
            self.run(frame, split);
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

    fn run(&mut self, frame: u32, split: &mut Split<'_>) {
        for join in self.joins.iter().filter(|join| join.at == frame) {
            self.consoles[join.unit].copy_state_from(&join.console);
            self.cable.reseat();
        }
        let frontier = self.remote_frontier();
        let slot = self.slot(frame);
        self.ring[slot].take(frame, &mut self.consoles, &self.cable, split);
        if frame == self.next_check && frontier >= frame + self.check_lag {
            let hashes = state::hash_consoles_on(
                &mut self.scratches,
                &mut self.consoles,
                &self.cable,
                split,
            );
            let index = self.checkpoint_index(frame);
            self.checkpoints[index].save(frame, &self.consoles, &self.cable);
            self.record_check(Check { frame, hashes });
        }

        let inputs = self.inputs_for(frame);
        let on = std::array::from_fn(|i| self.on(i, frame));
        self.turnout = pair::run_frame_on(&mut self.consoles, &mut self.cable, &inputs, on, split);
        let local = self.config.local.index();
        if let Some(turnout) = self.turnout {
            self.pacer.windows_ran(turnout);
        } else if C::CABLE_WINDOWS
            && let Some(lanes) = self.cable.held_back_by(on, local)
        {
            self.pacer.exact_ran(lanes, split);
        }
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

    fn replay_remote(&mut self, from: u32, split: &mut Split<'_>) -> bool {
        if !self.quiet_since(from) {
            return false;
        }
        let local = self.config.local.index();
        let end = self.frame;
        let start = &self.ring[self.slot(from)];
        for remote in (0..N).filter(|&i| i != local) {
            self.consoles[remote].copy_state_from(&start.consoles[remote]);
            self.cable.clocks[remote] = start.cable.clocks[remote];
        }
        self.set_catch_up(true);
        let inputs: Vec<[Input; N]> = (from..end).map(|frame| self.inputs_for(frame)).collect();
        let on: Vec<[bool; N]> = (from..end)
            .map(|frame| std::array::from_fn(|i| self.on(i, frame)))
            .collect();
        let blind = !self.watching;
        let frames = (end - from) as usize;
        let len = self.ring.len();
        let mut columns: [Vec<Option<(&mut C, &mut u64)>>; N] =
            std::array::from_fn(|_| (0..frames).map(|_| None).collect());
        for (index, slot) in self.ring.iter_mut().enumerate() {
            let offset = (index + len - from as usize % len) % len;
            if offset >= frames {
                continue;
            }
            let cells = slot.consoles.iter_mut().zip(slot.cable.clocks.iter_mut());
            for (column, cell) in columns.iter_mut().zip(cells) {
                column[offset] = Some(cell);
            }
        }
        let mut failed = [None; N];
        split.scope("friend catch-up", |scope| {
            let tasks = self
                .consoles
                .iter_mut()
                .zip(self.cable.clocks.iter_mut())
                .zip(columns)
                .zip(failed.iter_mut())
                .enumerate()
                .filter(|&(i, _)| i != local);
            for (i, (((gb, clock), column), failed)) in tasks {
                let (inputs, on) = (&inputs, &on);
                scope.spawn(move || {
                    for (k, cell) in column.into_iter().enumerate() {
                        let frame = from + k as u32;
                        let (slot, slot_clock) = cell.expect("a quiet replay keeps every frame");
                        slot.copy_state_from(gb);
                        *slot_clock = *clock;
                        if frame + 1 == end {
                            gb.set_blind(blind);
                            gb.set_muted(true);
                        }
                        if !pair::run_alone(gb, clock, inputs[k][i], on[k][i]) {
                            *failed = Some(frame);
                            return;
                        }
                    }
                });
            }
        });
        self.set_catch_up(false);
        let failed = failed.into_iter().flatten().min();
        let frontier = self.remote_frontier();
        for frame in from..failed.map_or(end, |frame| frame + 1) {
            if frame == self.next_check && frontier >= frame + self.check_lag {
                self.check_slot(frame, split);
            }
        }
        if failed.is_some() {
            return false;
        }
        self.last_rollback = end - from;
        true
    }

    fn emit_checks(&mut self, until: u32, split: &mut Split<'_>) {
        while self.next_check <= until
            && self.next_check < self.frame
            && self.remote_frontier() >= self.next_check + self.check_lag
        {
            let frame = self.next_check;
            if self.ring[self.slot(frame)].frame != Some(frame) {
                self.next_check += self.config.check_interval;
                continue;
            }
            self.check_slot(frame, split);
        }
    }

    fn check_slot(&mut self, frame: u32, split: &mut Split<'_>) {
        let index = self.slot(frame);
        let checkpoint = self.checkpoint_index(frame);
        let slot = &mut self.ring[index];
        let hashes =
            state::hash_consoles_on(&mut self.scratches, &mut slot.consoles, &slot.cable, split);
        self.checkpoints[checkpoint].save(frame, &slot.consoles, &slot.cable);
        self.record_check(Check { frame, hashes });
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
    let mut gba = Gba::from_rom(
        setup.rom,
        crate::gba::bios::Bios::built_in(),
        crate::gba::bios::Boot::Skip,
    );
    if let Some(data) = setup.state {
        return gba.decode_state(data);
    }
    if let Some(save) = setup.save {
        let battery = gba.battery_from_file(save.as_slice());
        gba.put_battery(battery, config.start_time);
    }
    Ok(gba)
}

#[cfg(feature = "nds")]
pub(crate) fn new_nds<const N: usize>(setup: NdsSetup, config: &SessionConfig<N>) -> GbResult<Nds> {
    use crate::console::Console;
    let NdsSetup { boot, firmware } = setup;
    let setup = match boot {
        NdsBoot::Card(setup) => setup,
        NdsBoot::DownloadPlay => {
            let mut nds = Nds::download_play(firmware)?;
            nds.set_clock(config.start_time as i64);
            return Ok(nds);
        }
    };
    if setup.kind != ConsoleKind::Nds {
        return Err(GbError::NotNds);
    }
    let mut nds = Nds::from_rom(setup.rom, firmware)?;
    if let Some(data) = setup.state {
        return nds.decode_state(data);
    }
    nds.set_clock(config.start_time as i64);
    if let Some(save) = setup.save {
        let battery = nds.battery_from_file(save.as_slice());
        nds.put_battery(battery, config.start_time);
    }
    Ok(nds)
}

fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const _: fn() = || {
    fn moves_between_threads<T: Send>() {}
    moves_between_threads::<Session<GameBoy>>();
    #[cfg(feature = "gba")]
    moves_between_threads::<Session<Gba, 4>>();
    #[cfg(feature = "nds")]
    moves_between_threads::<Session<Nds, 4>>();
};

#[cfg(all(test, feature = "persistence"))]
mod tests {
    use crate::ReadMemory;
    use crate::console::{Console, Timing, VideoSpec};
    use crate::console::{ConsoleKind, Keys, Rollback};
    use crate::error::{GbError, GbResult};
    use crate::gb::serial::LinkMessage;
    use crate::gb::{GameBoy, GbModel};
    use crate::netplay::pair::{self, Cable};
    use crate::netplay::state::{self, Snapshot};
    use crate::netplay::{
        Advance, CableEvent, Check, ConsoleSetup, Input, LinkPort, MAX_ROLLBACK_FRAMES, Player,
        Session, SessionConfig, Sync, new_console,
    };
    use crate::netplay::{CablePace, DEFAULT_CABLE_WINDOW, MemberError, NEVER};
    use crate::persistence::battery_save::BatterySave;
    use crate::persistence::rtc_footer::RtcFooter;
    use crate::persistence::sram_dump::SramDump;
    use crate::persistence::state::StateBody;
    use crate::rom::Rom;
    #[cfg(feature = "rom-id")]
    use crate::rom::id::RomId;
    use crate::test_support::Step;
    #[cfg(feature = "gba")]
    use citrine_gb_threads::Split;
    use std::collections::VecDeque;
    use std::marker::PhantomData;

    const SNAKE: &[u8] = include_bytes!("../../citrine-gb-app/homebrew/snake.gb");

    const PETRIS: &[u8] = include_bytes!("../../citrine-gb-app/homebrew/petris.gbc");

    const TOBU: &[u8] = include_bytes!("../../citrine-gb-app/homebrew/tobu.gb");

    const LIBBET: &[u8] = include_bytes!("../../citrine-gb-app/homebrew/libbet.gb");

    const UCITY: &[u8] = include_bytes!("../../citrine-gb-app/homebrew/ucity.gbc");

    const POSTIE: &[u8] = include_bytes!("../../citrine-gb-app/homebrew/postie.gbc");

    const FRAME_MS: f64 = 70_224.0 * 1000.0 / 4_194_304.0;

    const START_TIME: u64 = 1_790_000_000;

    const PLAYERS: [Player; 2] = [Player::One, Player::Two];

    const IDLE: [Input; 2] = [Input::new(Keys::empty()); 2];

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        fn unit(&mut self) -> f64 {
            (self.next() >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum Delay {
        Fixed(f64),
        Jitter(f64, f64),
    }

    struct Wire<T> {
        delay: Delay,
        rng: Rng,
        queue: VecDeque<(f64, T)>,
        last: f64,
    }

    impl<T> Wire<T> {
        fn new(delay: Delay, seed: u64) -> Self {
            Self {
                delay,
                rng: Rng(seed),
                queue: VecDeque::new(),
                last: 0.0,
            }
        }

        fn send(&mut self, now: f64, item: T) {
            let delay = match self.delay {
                Delay::Fixed(ms) => ms,
                Delay::Jitter(mean, spread) => mean + (self.rng.unit() * 2.0 - 1.0) * spread,
            };
            self.last = (now + delay).max(self.last);
            self.queue.push_back((self.last, item));
        }

        fn arrived(&mut self, now: f64) -> Option<T> {
            if self.queue.front()?.0 <= now {
                self.queue.pop_front().map(|(_, item)| item)
            } else {
                None
            }
        }
    }

    fn roms(one: &[u8], two: &[u8]) -> [Rom; 2] {
        [Rom::new(one), Rom::new(two)]
    }

    fn setups(roms: &[Rom; 2]) -> [Option<ConsoleSetup<'_>>; 2] {
        [0, 1].map(|i| {
            Some(ConsoleSetup {
                rom: &roms[i],
                kind: GbModel::for_rom(&roms[i]).unwrap().into(),
                save: None,
                state: None,
            })
        })
    }

    fn config(local: Player, check_interval: u32) -> SessionConfig {
        SessionConfig {
            check_interval,
            ..SessionConfig::new(local, 0x0C17_2026, START_TIME)
        }
    }

    fn session(roms: &[Rom; 2], local: Player, check_interval: u32) -> Session<GameBoy> {
        let mut session = Session::new(config(local, check_interval), setups(roms)).unwrap();
        session.watch_remote(true);
        session
    }

    const PAD: [Keys; 8] = [
        Keys::empty(),
        Keys::LEFT,
        Keys::RIGHT,
        Keys::UP,
        Keys::DOWN,
        Keys::A,
        Keys::B,
        Keys::A.union(Keys::RIGHT),
    ];

    fn random_inputs(seed: u64, frames: usize, longest: u64) -> Vec<Input> {
        let mut rng = Rng(seed);
        let mut inputs = Vec::with_capacity(frames);
        while inputs.len() < frames {
            let held = Input::new(PAD[rng.below(PAD.len() as u64) as usize]);
            let length = 1 + rng.below(longest) as usize;
            inputs.extend(std::iter::repeat_n(held, length));
        }
        inputs.truncate(frames);
        inputs
    }

    fn press(inputs: &mut [Input], at: usize, buttons: Keys, frames: usize) {
        for input in &mut inputs[at..at + frames] {
            input.buttons = buttons;
        }
    }

    fn cable(inputs: &mut [Input], at: usize, event: CableEvent) {
        inputs[at].cable = Some(event);
    }

    struct Online<C: Rollback + LinkPort = GameBoy> {
        sessions: [Session<C>; 2],
        checks: [Vec<Check>; 2],
        deepest: u32,
        rollbacks: u32,
        resyncs: u32,
        resynced: Vec<u32>,
        advance_ms: Vec<f64>,
    }

    fn play_online<C: Rollback + LinkPort>(
        new_session: impl Fn(Player) -> Session<C>,
        inputs: &[Vec<Input>; 2],
        delay: Delay,
        corrupt: impl FnMut(u32, &mut Session<C>),
    ) -> Online<C> {
        play_online_ahead(new_session, inputs, delay, 0, corrupt)
    }

    fn play_online_ahead<C: Rollback + LinkPort>(
        new_session: impl Fn(Player) -> Session<C>,
        inputs: &[Vec<Input>; 2],
        delay: Delay,
        lead: u32,
        mut corrupt: impl FnMut(u32, &mut Session<C>),
    ) -> Online<C> {
        let frames = inputs[0].len() as u32;
        let mut online = Online {
            sessions: PLAYERS.map(new_session),
            checks: [Vec::new(), Vec::new()],
            deepest: 0,
            rollbacks: 0,
            resyncs: 0,
            resynced: Vec::new(),
            advance_ms: Vec::new(),
        };
        let mut wires = [1, 2].map(|seed| Wire::new(delay, seed));
        let mut check_wires = [3, 4].map(|seed| Wire::<Check>::new(delay, seed));
        let mut resync_wire = Wire::<(u32, Player, Vec<u8>)>::new(delay, 5);
        let mut answered = None;
        let mut sent = [0u32; 2];
        let limit = frames * 4 + 400 + lead * 4;
        for tick in 0..limit {
            let now = tick as f64 * FRAME_MS;
            for side in 0..2 {
                let session = &mut online.sessions[side];
                while let Some((frame, input)) = wires[1 - side].arrived(now) {
                    session.add_remote_input(frame, input);
                }
                while let Some(check) = check_wires[1 - side].arrived(now) {
                    session.add_remote_check(check);
                }
                if side == 1 {
                    while let Some((frame, player, data)) = resync_wire.arrived(now) {
                        session.resync(frame, player, &data).unwrap();
                        online.resyncs += 1;
                        online.resynced.push(frame);
                    }
                }
                let frame = session.frame();
                while sent[side] <= frame + lead && sent[side] < frames {
                    let at = sent[side];
                    let input = inputs[side][at as usize];
                    session.add_local_input(at, input).unwrap();
                    wires[side].send(now, (at, input));
                    sent[side] += 1;
                }
                corrupt(frame, session);
                let mut advanced = Advance::AwaitingLocal;
                let ms = time_ms(|| advanced = session.step());
                if side == 0 {
                    online.advance_ms.push(ms);
                }
                if advanced == Advance::Ran && session.last_rollback() > 0 {
                    online.rollbacks += 1;
                }
                online.deepest = online.deepest.max(session.last_rollback());
                while let Some(check) = session.take_check() {
                    online.checks[side].push(check);
                    check_wires[side].send(now, check);
                }
                if side == 0
                    && let Some(desync) = session.desync()
                    && answered != Some(desync.frame)
                {
                    answered = Some(desync.frame);
                    for player in PLAYERS.into_iter().filter(|&p| desync.differs(p)) {
                        let data = session.checkpoint(desync.frame, player).unwrap();
                        resync_wire.send(now, (desync.frame, player, data));
                    }
                }
            }
            let settled = online
                .sessions
                .iter()
                .all(|s| s.frame() == frames && s.confirmed_frame() == frames);
            let quiet = wires.iter().all(|w| w.queue.is_empty())
                && check_wires.iter().all(|w| w.queue.is_empty())
                && resync_wire.queue.is_empty();
            if settled && quiet {
                return online;
            }
        }
        panic!("sessions did not settle within {limit} ticks");
    }

    fn play_reference(
        session: Session<GameBoy>,
        inputs: &[Vec<Input>; 2],
    ) -> (Session<GameBoy>, Vec<Check>) {
        let mut session = session;
        let mut checks = Vec::new();
        for (frame, (&local, &remote)) in inputs[0].iter().zip(&inputs[1]).enumerate() {
            session.add_remote_input(frame as u32, remote);
            session.add_local_input(frame as u32, local).unwrap();
            assert_eq!(session.step(), Advance::Ran);
            assert_eq!(session.last_rollback(), 0);
            checks.extend(std::iter::from_fn(|| session.take_check()));
        }
        (session, checks)
    }

    struct Plain {
        consoles: [GameBoy; 2],
        cable: Cable<LinkMessage>,
        hashes: Vec<[u64; 2]>,
    }

    fn play_plain(roms: &[Rom; 2], inputs: &[Vec<Input>; 2], power_on: [u32; 2]) -> Plain {
        let config = config(Player::One, 1);
        let [one, two] = setups(roms);
        let mut consoles = [
            new_console(one.unwrap(), &config, Player::One).unwrap(),
            new_console(two.unwrap(), &config, Player::Two).unwrap(),
        ];
        let mut cable = Cable::default();
        let mut scratch = consoles[0].clone_state();
        let mut hashes = Vec::new();
        for (frame, (&one, &two)) in inputs[0].iter().zip(&inputs[1]).enumerate() {
            hashes.push(state::hash_consoles(&mut scratch, &consoles, &cable));
            let on = power_on.map(|at| frame as u32 >= at);
            pair::run_frame(&mut consoles, &mut cable, &[one, two], on);
        }
        Plain {
            consoles,
            cable,
            hashes,
        }
    }

    fn by_frame(checks: &[Check]) -> Vec<[u64; 2]> {
        let mut sorted = checks.to_vec();
        sorted.sort_by_key(|c| c.frame);
        sorted.dedup_by_key(|c| c.frame);
        for (frame, check) in sorted.iter().enumerate() {
            assert_eq!(check.frame, frame as u32, "check frames have a gap");
        }
        sorted.iter().map(|c| c.hashes).collect()
    }

    fn assert_same_consoles(a: &GameBoy, b: &GameBoy, what: &str) {
        assert!(
            a.clone_state().encode_state().unwrap() == b.clone_state().encode_state().unwrap(),
            "{what}: console state differs"
        );
        assert!(
            a.frame().as_slice() == b.frame().as_slice(),
            "{what}: screen differs"
        );
    }

    fn snake_inputs(frames: usize) -> [Vec<Input>; 2] {
        let mut one = random_inputs(11, frames, 24);
        let mut two = random_inputs(29, frames, 24);
        for at in (60..frames - 10).step_by(90) {
            press(&mut two, at, Keys::START, 5);
        }
        cable(&mut one, 20, CableEvent::Plug);
        cable(&mut two, frames / 2, CableEvent::Pull);
        cable(&mut one, frames / 2 + 40, CableEvent::Plug);
        [one, two]
    }

    #[test]
    fn delayed_sessions_match_a_run_without_delay_every_frame() {
        let roms = roms(SNAKE, SNAKE);
        let inputs = snake_inputs(540);
        let (_, reference) = play_reference(session(&roms, Player::One, 1), &inputs);
        let reference = by_frame(&reference);
        assert_eq!(reference.len(), inputs[0].len());

        for delay in [
            Delay::Fixed(0.0),
            Delay::Fixed(50.0),
            Delay::Fixed(125.0),
            Delay::Fixed(250.0),
            Delay::Jitter(100.0, 40.0),
        ] {
            let online = play_online(|local| session(&roms, local, 1), &inputs, delay, |_, _| {});
            for side in 0..2 {
                let hashes = by_frame(&online.checks[side]);
                assert_eq!(hashes.len(), reference.len(), "{delay:?}");
                if let Some(frame) = (0..hashes.len()).find(|&f| hashes[f] != reference[f]) {
                    panic!("{delay:?}: side {side} differs from the reference at frame {frame}");
                }
                assert!(online.sessions[side].desync().is_none(), "{delay:?}");
            }
            if !matches!(delay, Delay::Fixed(ms) if ms == 0.0) {
                assert!(
                    online.rollbacks > 20,
                    "{delay:?}: {} rollbacks",
                    online.rollbacks
                );
            }
            assert!(online.deepest <= MAX_ROLLBACK_FRAMES, "{delay:?}");
        }
    }

    #[test]
    fn a_resync_of_the_local_player_marks_its_state_as_from_the_peer() {
        let roms = roms(SNAKE, SNAKE);
        let inputs = [vec![Input::default(); 60], vec![Input::default(); 60]];
        let (mut session, _) = play_reference(session(&roms, Player::One, 1), &inputs);
        assert!(!session.local_from_peer());
        let at = session.latest_checkpoint().unwrap();
        let state = session.checkpoint(at, Player::One).unwrap();
        session.resync(at, Player::One, &state).unwrap();
        assert!(session.local_from_peer());
        let at = session.latest_checkpoint().unwrap();
        let state = session.checkpoint(at, Player::Two).unwrap();
        session.resync(at, Player::Two, &state).unwrap();
        assert!(session.local_from_peer());
    }

    #[test]
    fn a_resync_of_a_remote_player_leaves_the_local_state_its_own() {
        let roms = roms(SNAKE, SNAKE);
        let inputs = [vec![Input::default(); 60], vec![Input::default(); 60]];
        let (mut session, _) = play_reference(session(&roms, Player::One, 1), &inputs);
        let at = session.latest_checkpoint().unwrap();
        let state = session.checkpoint(at, Player::Two).unwrap();
        session.resync(at, Player::Two, &state).unwrap();
        assert!(!session.local_from_peer());
    }

    #[test]
    fn a_cgb_game_stays_in_step_under_jitter() {
        let roms = roms(PETRIS, PETRIS);
        let mut inputs = [random_inputs(5, 360, 12), random_inputs(6, 360, 12)];
        cable(&mut inputs[1], 0, CableEvent::Plug);
        let (_, reference) = play_reference(session(&roms, Player::One, 1), &inputs);
        let online = play_online(
            |local| session(&roms, local, 1),
            &inputs,
            Delay::Jitter(100.0, 40.0),
            |_, _| {},
        );
        for (side, checks) in online.checks.iter().enumerate().take(2) {
            assert!(by_frame(checks) == by_frame(&reference), "side {side}");
        }
        assert!(online.rollbacks > 20);
    }

    fn linked_round(
        roms: &[Rom; 2],
        inputs: &[Vec<Input>; 2],
        power_on: [u32; 2],
        least_messages: u64,
    ) -> Cable<LinkMessage> {
        let plain = play_plain(roms, inputs, power_on);
        assert!(
            plain.cable.messages.iter().all(|&m| m >= least_messages),
            "the round sent only {:?} link messages, {:?} of them transfers",
            plain.cable.messages,
            plain.cable.transfers
        );
        let online = play_online(
            |local| {
                let config = SessionConfig {
                    power_on,
                    ..config(local, 1)
                };
                let mut session = Session::new(config, setups(roms)).unwrap();
                session.watch_remote(true);
                session
            },
            inputs,
            Delay::Jitter(100.0, 40.0),
            |_, _| {},
        );
        assert!(online.rollbacks > 10, "{} rollbacks", online.rollbacks);
        for (side, session) in online.sessions.iter().enumerate() {
            assert!(
                by_frame(&online.checks[side]) == plain.hashes,
                "side {side}"
            );
            assert_eq!(
                session.link_messages(),
                (plain.cable.messages, plain.cable.transfers)
            );
            for player in PLAYERS {
                assert_same_consoles(
                    session.console(player),
                    &plain.consoles[player.index()],
                    &format!("side {side}, {player:?}"),
                );
            }
        }
        plain.cable
    }

    #[test]
    fn a_linked_petris_match_through_sessions_matches_a_local_pair() {
        let frames = 1500;
        let mut inputs = [
            vec![Input::default(); frames],
            vec![Input::default(); frames],
        ];
        cable(&mut inputs[0], 0, CableEvent::Plug);
        press(&mut inputs[0], 450, Keys::START, 5);
        press(&mut inputs[1], 450, Keys::START, 5);
        press(&mut inputs[0], 600, Keys::DOWN, 5);
        press(&mut inputs[0], 640, Keys::RIGHT, 5);
        press(&mut inputs[0], 700, Keys::START, 5);
        let pad = [Keys::LEFT, Keys::RIGHT, Keys::A, Keys::DOWN, Keys::empty()];
        let mut rng = Rng(9);
        for side in &mut inputs {
            let mut at = 800;
            while at < frames {
                let length = (4 + rng.below(20) as usize).min(frames - at);
                press(side, at, pad[rng.below(pad.len() as u64) as usize], length);
                at += length;
            }
        }
        linked_round(&roms(PETRIS, PETRIS), &inputs, [0; 2], 20);
    }

    #[test]
    fn snake_links_through_sessions_when_one_console_starts_later() {
        let frames = 1200;
        let mut inputs = [
            vec![Input::default(); frames],
            vec![Input::default(); frames],
        ];
        cable(&mut inputs[0], 0, CableEvent::Plug);
        let mut rng = Rng(3);
        for side in &mut inputs {
            let mut at = 300;
            while at < frames {
                let length = (6 + rng.below(30) as usize).min(frames - at);
                press(side, at, PAD[1 + rng.below(4) as usize], length);
                at += length;
            }
        }
        for at in (120..frames - 10).step_by(60) {
            press(&mut inputs[1], at, Keys::START, 5);
        }
        let cable = linked_round(&roms(SNAKE, SNAKE), &inputs, [0, 60], 20);
        let [one, two] = cable.transfers;
        assert!(one > 0 && two > 0, "{:?}", cable.transfers);
        assert_eq!(cable.messages, [one + two; 2]);
    }

    #[test]
    fn a_console_handed_back_comes_from_the_last_confirmed_frame() {
        let roms = roms(SNAKE, SNAKE);
        let inputs = snake_inputs(120);
        let mut ahead = session(&roms, Player::One, 1);
        for frame in 0..120 {
            if frame < 90 {
                ahead.add_remote_input(frame, inputs[1][frame as usize]);
            }
            ahead
                .add_local_input(frame, inputs[0][frame as usize])
                .unwrap();
            assert_eq!(ahead.step(), Advance::Ran);
        }
        assert_eq!(ahead.confirmed_frame(), 90);
        let shared = [0, 1].map(|side| inputs[side][..90].to_vec());
        let (reference, _) = play_reference(session(&roms, Player::One, 1), &shared);
        let handed = ahead.into_confirmed_console(Player::One);
        assert_same_consoles(&handed, reference.console(Player::One), "confirmed frame");
    }

    #[test]
    fn the_kept_states_up_to_the_confirmed_frame_match_a_replay() {
        let roms = roms(SNAKE, SNAKE);
        let inputs = snake_inputs(120);
        let mut late = session(&roms, Player::One, 1);
        let mut heard = 0;
        for frame in 0..100u32 {
            if frame % 16 == 0 {
                while heard + 6 < frame {
                    late.add_remote_input(heard, inputs[1][heard as usize]);
                    heard += 1;
                }
            }
            late.add_local_input(frame, inputs[0][frame as usize])
                .unwrap();
            assert_eq!(late.step(), Advance::Ran);
        }
        late.step();
        let confirmed = late.confirmed_frame();
        assert_eq!(confirmed, heard);
        for player in [Player::One, Player::Two] {
            let states = late.confirmed_states(player);
            assert!(states.len() >= 10, "{} states", states.len());
            let frames: Vec<u32> = states.iter().map(|&(frame, _)| frame).collect();
            let first = frames[0];
            assert_eq!(frames, (first..=confirmed).collect::<Vec<_>>());
            for &index in &[0, states.len() / 2, states.len() - 1] {
                let (frame, state) = &states[index];
                let shared = [0, 1].map(|side| inputs[side][..*frame as usize].to_vec());
                let (reference, _) = play_reference(session(&roms, Player::One, 1), &shared);
                assert_same_consoles(state, reference.console(player), &format!("frame {frame}"));
            }
        }
    }

    #[test]
    fn a_session_at_its_budget_keeps_confirmed_states_only_when_asked() {
        let roms = roms(SNAKE, SNAKE);
        let inputs = snake_inputs(120);
        let budget = Sync::new(4, 0);
        let run = |keep: u32| {
            let config = SessionConfig {
                sync: budget,
                ..config(Player::One, 1)
            };
            let mut late = Session::new(config, setups(&roms))
                .unwrap()
                .keeping_confirmed(keep);
            late.watch_remote(true);
            for frame in 0..60u32 {
                for heard in frame.saturating_sub(4)..frame.saturating_sub(3) {
                    late.add_remote_input(heard, inputs[1][heard as usize]);
                }
                late.add_local_input(frame, inputs[0][frame as usize])
                    .unwrap();
                assert_eq!(late.step(), Advance::Ran, "frame {frame}");
            }
            late.step();
            (late.confirmed_frame(), late.confirmed_states(Player::Two))
        };
        assert!(run(0).1.len() <= 2);
        let (confirmed, states) = run(20);
        assert!(states.len() > 20, "{} states", states.len());
        let frames: Vec<u32> = states.iter().map(|&(frame, _)| frame).collect();
        assert_eq!(frames, (frames[0]..=confirmed).collect::<Vec<_>>());
        let (frame, state) = &states[0];
        let shared = [0, 1].map(|side| inputs[side][..*frame as usize].to_vec());
        let (reference, _) = play_reference(session(&roms, Player::One, 1), &shared);
        assert_same_consoles(
            state,
            reference.console(Player::Two),
            &format!("frame {frame}"),
        );
    }

    #[test]
    fn a_lowered_budget_waits_for_the_friend_and_never_rises_past_the_start() {
        let roms = roms(SNAKE, SNAKE);
        let inputs = snake_inputs(120);
        let config = SessionConfig {
            sync: Sync::new(4, 0),
            ..config(Player::One, 1)
        };
        let mut late = Session::new(config, setups(&roms)).unwrap();
        late.set_budget(9);
        assert_eq!(late.sync().budget(), 4);
        for frame in 0..4u32 {
            late.add_local_input(frame, inputs[0][frame as usize])
                .unwrap();
            assert_eq!(late.step(), Advance::Ran, "frame {frame}");
        }
        late.set_budget(1);
        late.add_local_input(4, inputs[0][4]).unwrap();
        assert_eq!(late.step(), Advance::AwaitingRemote);
        for heard in 0..4u32 {
            late.add_remote_input(heard, inputs[1][heard as usize]);
        }
        assert_eq!(late.step(), Advance::Ran);
        assert_eq!(late.frame(), 5);
        late.set_budget(0);
        late.add_local_input(5, inputs[0][5]).unwrap();
        assert_eq!(late.step(), Advance::AwaitingRemote);
        late.add_remote_input(4, inputs[1][4]);
        late.add_remote_input(5, inputs[1][5]);
        assert_eq!(late.step(), Advance::Ran);
    }

    fn exchange_program(sc: u8, first: u8, step: u8, count: u16) -> Vec<u8> {
        let [low, high] = count.to_le_bytes();
        let mut code = vec![0xAF, 0xEA, 0x00, 0xDF, 0x21, 0x00, 0xC0];
        code.extend_from_slice(&[0x11, low, high, 0x06, first]);
        let top = code.len();
        code.extend_from_slice(&[0x78, 0xE0, 0x01]);
        if sc == 0x81 {
            code.extend_from_slice(&[0x0E, 0x28, 0x0D, 0x20, 0xFD]);
        }
        code.extend_from_slice(&[0x3E, sc, 0xE0, 0x02, 0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA]);
        code.extend_from_slice(&[0xF0, 0x01, 0x22, 0x78, 0xC6, step, 0x47]);
        code.extend_from_slice(&[0x1B, 0x7A, 0xB3, 0x20]);
        let back = top as i32 - (code.len() as i32 + 1);
        code.push(back as i8 as u8);
        code.extend_from_slice(&[0x3E, 0xAA, 0xEA, 0x00, 0xDF]);
        code
    }

    fn exchanged(gb: &GameBoy, count: u16) -> Vec<u8> {
        (0..count)
            .map(|i| gb.memory.read_naive(0xC000 + i))
            .collect()
    }

    #[test]
    fn a_scripted_trade_exchanges_the_same_bytes_as_a_local_link() {
        let count = 420u16;
        let roms = [
            crate::test_support::rom(0x00, &exchange_program(0x81, 0x10, 1, count)),
            crate::test_support::rom(0x00, &exchange_program(0x80, 0x40, 3, count)),
        ];
        let mut inputs = [random_inputs(7, 240, 6), random_inputs(8, 240, 6)];
        cable(&mut inputs[0], 0, CableEvent::Plug);

        let plain = play_plain(&roms, &inputs, [0; 2]);
        let master: Vec<u8> = (0..count).map(|i| 0x10u8.wrapping_add(i as u8)).collect();
        let slave: Vec<u8> = (0..count)
            .map(|i| 0x40u8.wrapping_add((i as u8).wrapping_mul(3)))
            .collect();
        for gb in &plain.consoles {
            assert_eq!(
                gb.memory.read_naive(0xDF00),
                0xAA,
                "the local trade did not finish"
            );
        }
        assert_eq!(exchanged(&plain.consoles[0], count), slave);
        assert_eq!(exchanged(&plain.consoles[1], count), master);

        for delay in [Delay::Fixed(125.0), Delay::Jitter(100.0, 40.0)] {
            let online = play_online(|local| session(&roms, local, 1), &inputs, delay, |_, _| {});
            assert!(online.rollbacks > 10, "{delay:?}");
            for session in &online.sessions {
                assert_eq!(exchanged(session.console(Player::One), count), slave);
                assert_eq!(exchanged(session.console(Player::Two), count), master);
                for player in PLAYERS {
                    assert_same_consoles(
                        session.console(player),
                        &plain.consoles[player.index()],
                        &format!("{delay:?}, {player:?}"),
                    );
                }
            }
        }
    }

    #[test]
    fn a_desync_is_detected_and_resync_recovers() {
        let roms = roms(SNAKE, SNAKE);
        let inputs = snake_inputs(600);
        let mut corrupted = false;
        let online = play_online(
            |local| session(&roms, local, 30),
            &inputs,
            Delay::Fixed(50.0),
            |frame, session| {
                if frame >= 200 && !corrupted && session.local() == Player::Two {
                    corrupted = true;
                    session.corrupt(Player::One, |gb| {
                        for addr in 0xC000..0xC100u16 {
                            let value = gb.memory.read_naive(addr);
                            crate::WriteMemory::write_naive(&mut gb.memory, addr, value ^ 0x5A);
                        }
                    });
                }
            },
        );
        assert!(corrupted);
        assert!(online.resyncs >= 1, "the difference was never detected");
        let (reference, _) = play_reference(session(&roms, Player::One, 30), &inputs);
        for session in &online.sessions {
            assert!(session.desync().is_none());
            for player in PLAYERS {
                assert_same_consoles(
                    session.console(player),
                    reference.console(player),
                    &format!("{player:?} after the resync"),
                );
            }
        }
    }

    #[test]
    fn a_resync_in_the_middle_of_a_trade_keeps_the_trade() {
        let count = 420u16;
        let roms = [
            crate::test_support::rom(0x00, &exchange_program(0x81, 0x10, 1, count)),
            crate::test_support::rom(0x00, &exchange_program(0x80, 0x40, 3, count)),
        ];
        let mut inputs = [random_inputs(7, 240, 6), random_inputs(8, 240, 6)];
        cable(&mut inputs[0], 0, CableEvent::Plug);
        let plain = play_plain(&roms, &inputs, [0; 2]);

        let config = config(Player::One, 1);
        let [one, two] = setups(&roms);
        let mut consoles = [
            new_console(one.unwrap(), &config, Player::One).unwrap(),
            new_console(two.unwrap(), &config, Player::Two).unwrap(),
        ];
        let mut cable = Cable::default();
        let mut trading = Vec::new();
        for (&one, &two) in inputs[0].iter().zip(&inputs[1]) {
            trading.push(consoles[0].serial.transfer_active());
            pair::run_frame(&mut consoles, &mut cable, &[one, two], [true; 2]);
        }
        let interval = 4;
        let checks: Vec<usize> = (0..trading.len())
            .step_by(interval as usize)
            .filter(|&frame| trading[frame])
            .collect();
        let during: Vec<usize> = (0..trading.len()).filter(|&f| trading[f]).collect();
        assert!(checks.len() > 2, "the trade runs in frames {during:?}");
        let at = checks[checks.len() / 2] as u32;

        let mut corrupted = false;
        let online = play_online(
            |local| session(&roms, local, interval),
            &inputs,
            Delay::Fixed(50.0),
            |frame, session| {
                if frame >= at && !corrupted && session.local() == Player::Two {
                    corrupted = true;
                    session.corrupt(Player::One, |gb| {
                        for addr in 0xD000..0xD100u16 {
                            let value = gb.memory.read_naive(addr);
                            crate::WriteMemory::write_naive(&mut gb.memory, addr, value ^ 0x5A);
                        }
                    });
                }
            },
        );
        assert!(corrupted);
        assert!(online.resyncs >= 1, "the difference was never detected");
        let first = online.resynced[0];
        assert!(
            trading[first as usize],
            "the first resync, frame {first}, is not mid-trade"
        );
        for session in &online.sessions {
            assert!(session.desync().is_none());
            assert_eq!(
                session.link_messages(),
                (plain.cable.messages, plain.cable.transfers)
            );
            for player in PLAYERS {
                assert_same_consoles(
                    session.console(player),
                    &plain.consoles[player.index()],
                    &format!("{player:?} after the resync"),
                );
            }
        }
    }

    fn rtc_program() -> Vec<u8> {
        let mut code = vec![0x3E, 0x0A, 0xEA, 0x00, 0x00];
        let top = code.len();
        code.extend_from_slice(&[0xAF, 0xEA, 0x00, 0x60, 0x3C, 0xEA, 0x00, 0x60]);
        for (register, target) in [(0x08u8, 0x00u8), (0x09, 0x01), (0x0A, 0x02), (0x0B, 0x03)] {
            code.extend_from_slice(&[0x3E, register, 0xEA, 0x00, 0x40, 0xFA, 0x00, 0xA0]);
            code.extend_from_slice(&[0xEA, target, 0xC0]);
        }
        code.extend_from_slice(&[
            0x3E, 0x00, 0xEA, 0x00, 0x40, 0xFA, 0x00, 0xA0, 0x3C, 0xEA, 0x00, 0xA0,
        ]);
        let back = top as i32 - (code.len() as i32 + 2);
        code.extend_from_slice(&[0x18, back as i8 as u8]);
        code
    }

    fn rtc_rom() -> Rom {
        let mut data = crate::test_support::rom(0x00, &rtc_program()).data.to_vec();
        data[0x0147] = 0x10;
        data[0x0149] = 0x02;
        Rom::new(&data)
    }

    fn rtc_save(ram: u8, timestamp: u64) -> SramDump {
        let mut data = vec![ram; 0x2000];
        let footer = RtcFooter {
            real: crate::gb::cartridge::RtcRegisters::from_bytes([10, 20, 3, 4, 0]),
            latched: crate::gb::cartridge::RtcRegisters::default(),
            timestamp,
        };
        data.extend_from_slice(&footer.encode());
        SramDump::from_slice(&data)
    }

    fn rtc_session(rom: &Rom, local: Player, start_time: u64) -> Session<GameBoy> {
        let saved = START_TIME - 3_600;
        let setups = [0x11, 0x22].map(|ram| {
            Some(ConsoleSetup {
                rom,
                kind: ConsoleKind::Gb,
                save: Some(rtc_save(ram, saved)),
                state: None,
            })
        });
        Session::new(
            SessionConfig {
                check_interval: 1,
                ..SessionConfig::new(local, 7, start_time)
            },
            setups,
        )
        .unwrap()
    }

    fn clock(gb: &GameBoy) -> [u8; 4] {
        [0, 1, 2, 3].map(|i| gb.memory.read_naive(0xC000 + i))
    }

    #[test]
    fn rtc_carts_run_on_session_time() {
        let rom = rtc_rom();
        let frames = 400usize;
        let inputs = [random_inputs(1, frames, 10), random_inputs(2, frames, 10)];
        let (_, reference) = play_reference(rtc_session(&rom, Player::One, START_TIME), &inputs);
        let mut online = play_online(
            |local| rtc_session(&rom, local, START_TIME),
            &inputs,
            Delay::Jitter(100.0, 40.0),
            |_, _| {},
        );
        assert!(online.rollbacks > 10);
        for side in 0..2 {
            assert!(
                by_frame(&online.checks[side]) == by_frame(&reference),
                "side {side}"
            );
            for player in PLAYERS {
                assert_eq!(clock(online.sessions[side].console(player)), [16, 20, 4, 4]);
            }
        }

        let elapsed = frames as u64 * 70_224 / 4_194_304;
        for (side, ram) in [(0, 0x11), (1, 0x22)] {
            let session = &mut online.sessions[side];
            let saved = session.local_save(false).unwrap();
            let data = saved.as_slice();
            let footer_at = data.len() - crate::persistence::rtc_footer::RTC_FOOTER_LEN;
            let footer = RtcFooter::decode(&data[footer_at..]).unwrap();
            assert_eq!(footer.timestamp, START_TIME + elapsed);
            assert_eq!(data[1], ram);
            assert!(session.local_save(false).is_none());
            assert!(session.local_save(true).is_some());
        }

        let mut later = rtc_session(&rom, Player::One, START_TIME + 86_400);
        later.add_local_input(0, inputs[0][0]).unwrap();
        later.add_remote_input(0, inputs[1][0]);
        later.step();
        assert_ne!(later.take_check().unwrap().hashes, reference[0].hashes);
    }

    #[test]
    fn inputs_arrive_late_out_of_order_and_twice() {
        let roms = roms(SNAKE, SNAKE);
        let frames = 160u32;
        let inputs = snake_inputs(frames as usize);
        let (_, reference) = play_reference(session(&roms, Player::One, 1), &inputs);
        let mut session = session(&roms, Player::One, 1);
        let mut checks = Vec::new();
        let mut rng = Rng(99);
        let mut pending: Vec<u32> = Vec::new();
        let mut delivered: Vec<u32> = Vec::new();
        let deliver = |session: &mut Session<GameBoy>, frame: u32| {
            session.add_remote_input(frame, inputs[1][frame as usize]);
        };
        for frame in 0..frames {
            session
                .add_local_input(frame, inputs[0][frame as usize])
                .unwrap();
            pending.push(frame);
            for i in (1..pending.len()).rev() {
                pending.swap(i, rng.below(i as u64 + 1) as usize);
            }
            let mut kept = Vec::new();
            for remote in pending.drain(..) {
                if frame - remote >= 20 || rng.below(4) == 0 {
                    deliver(&mut session, remote);
                    deliver(&mut session, remote);
                    delivered.push(remote);
                } else {
                    kept.push(remote);
                }
            }
            pending = kept;
            if !delivered.is_empty() && rng.below(3) == 0 {
                let again = delivered[rng.below(delivered.len() as u64) as usize];
                deliver(&mut session, again);
            }
            assert_eq!(session.step(), Advance::Ran);
            checks.extend(std::iter::from_fn(|| session.take_check()));
        }
        for remote in pending.drain(..) {
            deliver(&mut session, remote);
        }
        assert_eq!(session.step(), Advance::AwaitingLocal);
        checks.extend(std::iter::from_fn(|| session.take_check()));
        assert!(by_frame(&checks) == by_frame(&reference));
        assert_eq!(
            session.add_local_input(3, Input::default()),
            Err(crate::netplay::InputError::Past(3))
        );
    }

    #[test]
    fn a_session_waits_when_the_remote_falls_too_far_behind() {
        let roms = roms(SNAKE, SNAKE);
        let mut session = session(&roms, Player::One, 60);
        for frame in 0..MAX_ROLLBACK_FRAMES {
            session.add_local_input(frame, Input::default()).unwrap();
            assert_eq!(session.step(), Advance::Ran);
        }
        session
            .add_local_input(MAX_ROLLBACK_FRAMES, Input::default())
            .unwrap();
        assert_eq!(session.step(), Advance::AwaitingRemote);
        session.add_remote_input(0, Input::new(Keys::A));
        assert_eq!(session.step(), Advance::Ran);
        assert_eq!(session.last_rollback(), MAX_ROLLBACK_FRAMES);
        assert_eq!(session.step(), Advance::AwaitingLocal);
    }

    #[test]
    fn a_rewind_never_replays_sound_already_heard() {
        let roms = roms(SNAKE, SNAKE);
        let frames = 300u32;
        let inputs = snake_inputs(frames as usize);
        let mut delayed = session(&roms, Player::One, 60);
        let mut direct = session(&roms, Player::One, 60);
        let (mut heard, mut expected) = (Vec::new(), Vec::new());
        let mut most = 0;
        let mut rollbacks = 0;
        for frame in 0..frames {
            for session in [&mut delayed, &mut direct] {
                session
                    .add_local_input(frame, inputs[0][frame as usize])
                    .unwrap();
            }
            direct.add_remote_input(frame, inputs[1][frame as usize]);
            if frame >= 10 {
                delayed.add_remote_input(frame - 10, inputs[1][frame as usize - 10]);
            }
            assert_eq!(delayed.step(), Advance::Ran);
            assert_eq!(direct.step(), Advance::Ran);
            rollbacks += u32::from(delayed.last_rollback() > 0);
            let before = heard.len();
            delayed.drain_audio(&mut heard);
            most = most.max(heard.len() - before);
            direct.drain_audio(&mut expected);
        }
        assert!(rollbacks > 10);
        let (heard, expected) = (heard.len() as f64, expected.len() as f64);
        assert!(
            (heard - expected).abs() < expected * 0.01,
            "heard {heard} samples, a run without rewinds makes {expected}"
        );
        assert!(most < 4_000, "one frame gave {most} samples");
    }

    #[test]
    fn a_rewind_never_replays_the_friends_sound() {
        let roms = roms(SNAKE, SNAKE);
        let frames = 300u32;
        let inputs = snake_inputs(frames as usize);
        let mut delayed = session(&roms, Player::One, 60);
        let mut direct = session(&roms, Player::One, 60);
        let (mut heard, mut expected) = (Vec::new(), Vec::new());
        let mut rollbacks = 0;
        for frame in 0..frames {
            for session in [&mut delayed, &mut direct] {
                session
                    .add_local_input(frame, inputs[0][frame as usize])
                    .unwrap();
            }
            direct.add_remote_input(frame, inputs[1][frame as usize]);
            if frame >= 10 {
                delayed.add_remote_input(frame - 10, inputs[1][frame as usize - 10]);
            }
            assert_eq!(delayed.step(), Advance::Ran);
            assert_eq!(direct.step(), Advance::Ran);
            rollbacks += u32::from(delayed.last_rollback() > 0);
            delayed.drain_remote_audio(&mut heard);
            direct.drain_remote_audio(&mut expected);
        }
        assert!(rollbacks > 10);
        assert!(!expected.is_empty());
        let (heard, expected) = (heard.len() as f64, expected.len() as f64);
        assert!(
            (heard - expected).abs() < expected * 0.01,
            "heard {heard} of the friend's samples, a run without rewinds makes {expected}"
        );
    }

    #[test]
    fn the_friends_sound_plays_only_while_watching() {
        let roms = roms(SNAKE, SNAKE);
        let inputs = snake_inputs(240);
        let mut session = session(&roms, Player::One, 60);
        session.watch_remote(false);
        let mut heard = Vec::new();
        for frame in 0..240u32 {
            if frame == 120 {
                session.drain_remote_audio(&mut heard);
                assert!(heard.is_empty(), "the friend's game made sound unwatched");
                session.watch_remote(true);
            }
            session
                .add_local_input(frame, inputs[0][frame as usize])
                .unwrap();
            session.add_remote_input(frame, inputs[1][frame as usize]);
            assert_eq!(session.step(), Advance::Ran);
        }
        session.drain_remote_audio(&mut heard);
        assert!(
            !heard.is_empty(),
            "the friend's game made no sound while watched"
        );
        let mut mine = Vec::new();
        session.drain_audio(&mut mine);
        assert!(!mine.is_empty());
    }

    #[test]
    fn snapshots_round_trip_the_whole_pair() {
        let roms = roms(SNAKE, SNAKE);
        let inputs = snake_inputs(200);
        let mut plain = play_plain(
            &roms,
            &[inputs[0][..150].to_vec(), inputs[1][..150].to_vec()],
            [0; 2],
        );
        let mut snapshot = Snapshot::new(&mut plain.consoles);
        snapshot.save(150, &plain.consoles, &plain.cable);
        let mut scratch = plain.consoles[0].clone_state();
        let saved = state::hash_consoles(&mut scratch, &plain.consoles, &plain.cable);
        for _ in 0..30 {
            pair::run_frame(&mut plain.consoles, &mut plain.cable, &IDLE, [true; 2]);
        }
        let moved = state::hash_consoles(&mut scratch, &plain.consoles, &plain.cable);
        assert_ne!(saved, moved);
        snapshot.load(&mut plain.consoles, &mut plain.cable);
        assert_eq!(
            state::hash_consoles(&mut scratch, &plain.consoles, &plain.cable),
            saved
        );
    }

    #[test]
    fn a_console_taken_out_of_a_session_runs_on_like_the_session_copy() {
        let roms = roms(SNAKE, SNAKE);
        let inputs = [random_inputs(3, 400, 20), random_inputs(4, 400, 20)];
        let play = |frames: usize| {
            let mut session = session(&roms, Player::One, 60);
            for (frame, (&local, &remote)) in
                inputs[0].iter().zip(&inputs[1]).take(frames).enumerate()
            {
                session.add_remote_input(frame as u32, remote);
                session.add_local_input(frame as u32, local).unwrap();
                assert_eq!(session.step(), Advance::Ran);
            }
            session
        };
        let whole = play(400);
        for player in PLAYERS {
            let mut gb = play(250).into_console(player);
            for input in &inputs[player.index()][250..] {
                gb.set_keys(input.buttons);
                gb.run_frame();
            }
            assert_same_consoles(&gb, whole.console(player), &format!("{player:?}"));
        }
    }

    fn play_rewinding(
        roms: &[Rom; 2],
        config: SessionConfig,
        inputs: &[Vec<Input>; 2],
        lag: usize,
        rewind_to: impl Fn(usize) -> Option<u32>,
    ) -> (Session<GameBoy>, Vec<Check>) {
        let local = config.local;
        let mut session = Session::new(config, setups(roms)).unwrap();
        session.watch_remote(true);
        let remote = local.other().index();
        let frames = inputs[0].len();
        let mut checks = Vec::new();
        for frame in 0..frames {
            if frame >= lag {
                session.add_remote_input((frame - lag) as u32, inputs[remote][frame - lag]);
            }
            session
                .add_local_input(frame as u32, inputs[local.index()][frame])
                .unwrap();
            if let Some(to) = rewind_to(frame) {
                session.rewind_to(to);
            }
            assert_eq!(session.step(), Advance::Ran);
            checks.extend(std::iter::from_fn(|| session.take_check()));
        }
        let start = frames.saturating_sub(lag);
        for (frame, &input) in inputs[remote].iter().enumerate().skip(start) {
            session.add_remote_input(frame as u32, input);
        }
        assert_eq!(session.step(), Advance::AwaitingLocal);
        checks.extend(std::iter::from_fn(|| session.take_check()));
        (session, checks)
    }

    fn sorted_checks(checks: &[Check]) -> Vec<Check> {
        let mut sorted = checks.to_vec();
        sorted.sort_by_key(|c| c.frame);
        sorted.dedup_by_key(|c| c.frame);
        sorted
    }

    fn differing_parts(a: &GameBoy, b: &GameBoy) -> Vec<&'static str> {
        fn bytes<T: serde::Serialize>(part: &T) -> Vec<u8> {
            rmp_serde::to_vec(part).unwrap()
        }
        let parts = [
            ("cpu", bytes(&a.cpu) != bytes(&b.cpu)),
            ("cartridge", bytes(&a.cartridge) != bytes(&b.cartridge)),
            ("cgb", bytes(&a.cgb) != bytes(&b.cgb)),
            ("dma", bytes(&a.dma) != bytes(&b.dma)),
            ("hdma", bytes(&a.hdma) != bytes(&b.hdma)),
            ("ic", bytes(&a.ic) != bytes(&b.ic)),
            ("memory", bytes(&a.memory) != bytes(&b.memory)),
            ("timer", bytes(&a.timer) != bytes(&b.timer)),
            ("ppu", bytes(&a.ppu) != bytes(&b.ppu)),
            ("apu", bytes(&a.apu) != bytes(&b.apu)),
            ("joypad", bytes(&a.joypad) != bytes(&b.joypad)),
            ("serial", bytes(&a.serial) != bytes(&b.serial)),
            ("cycles", a.cycle_counter != b.cycle_counter),
        ];
        parts
            .into_iter()
            .filter(|(_, differs)| *differs)
            .map(|(name, _)| name)
            .collect()
    }

    #[test]
    fn rewinds_replay_to_the_same_state_as_a_straight_run() {
        let roms = roms(SNAKE, SNAKE);
        let inputs = snake_inputs(600);
        let power_on = 60;
        let rewind_to = |frame: usize| match frame {
            f if f > power_on as usize && f <= power_on as usize + 12 => Some(power_on),
            f if f % 7 == 0 => Some(f.saturating_sub(3 + f / 7 % 6) as u32),
            _ => None,
        };
        for local in PLAYERS {
            let mapped = match local {
                Player::One => inputs.clone(),
                Player::Two => [inputs[1].clone(), inputs[0].clone()],
                _ => unreachable!(),
            };
            for (interval, power_on) in [(1, [0, 0]), (60, [0, 0]), (1, [0, power_on])] {
                let config = SessionConfig {
                    power_on,
                    ..config(local, interval)
                };
                let mut reference = Session::new(config, setups(&roms)).unwrap();
                reference.watch_remote(true);
                let (reference, straight) = play_reference(reference, &mapped);
                for lag in [3, 8, 12] {
                    let (session, rewound) = play_rewinding(&roms, config, &inputs, lag, rewind_to);
                    let ours = sorted_checks(&rewound);
                    let theirs = sorted_checks(&straight);
                    let first = theirs
                        .iter()
                        .enumerate()
                        .find(|&(i, check)| ours.get(i) != Some(check))
                        .map(|(_, check)| check.frame);
                    let parts =
                        PLAYERS.map(|p| differing_parts(session.console(p), reference.console(p)));
                    let case = format!(
                        "{local:?} local, interval {interval}, power on {power_on:?}, lag {lag}"
                    );
                    assert!(
                        first.is_none(),
                        "{case}: first differing check at frame {first:?}, \
                     differing parts at the end {parts:?}"
                    );
                    for player in PLAYERS {
                        assert_same_consoles(
                            session.console(player),
                            reference.console(player),
                            &format!("{case}, {player:?}"),
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_session_keeps_its_consoles_off_the_stack() {
        let size = std::mem::size_of::<Session<GameBoy>>();
        assert!(size < 4096, "a session is {size} bytes");
    }

    fn median_ms(mut samples: Vec<f64>) -> f64 {
        samples.sort_by(f64::total_cmp);
        samples[samples.len() / 2]
    }

    fn p95_ms(mut samples: Vec<f64>) -> f64 {
        samples.sort_by(f64::total_cmp);
        samples[samples.len() * 95 / 100]
    }

    fn time_ms(mut f: impl FnMut()) -> f64 {
        let start = std::time::Instant::now();
        f();
        start.elapsed().as_secs_f64() * 1000.0
    }

    fn measure(name: &str, roms: &[Rom; 2], plug: bool) {
        let frames = 600;
        let mut inputs = [random_inputs(1, frames, 20), random_inputs(2, frames, 20)];
        if plug {
            cable(&mut inputs[0], 0, CableEvent::Plug);
        }
        let mut plain = play_plain(roms, &inputs, [0; 2]);
        let mut snapshot = Snapshot::new(&mut plain.consoles);
        let mut scratch = plain.consoles[0].clone_state();
        let (mut save, mut load, mut rerun, mut frame, mut hash) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for _ in 0..200 {
            save.push(time_ms(|| snapshot.save(0, &plain.consoles, &plain.cable)));
            frame.push(time_ms(|| {
                pair::run_frame(&mut plain.consoles, &mut plain.cable, &IDLE, [true; 2]);
            }));
            load.push(time_ms(|| {
                snapshot.load(&mut plain.consoles, &mut plain.cable)
            }));
            rerun.push(time_ms(|| {
                for _ in 0..8 {
                    pair::run_frame(&mut plain.consoles, &mut plain.cable, &IDLE, [true; 2]);
                }
            }));
            hash.push(time_ms(|| {
                std::hint::black_box(state::hash_consoles(
                    &mut scratch,
                    &plain.consoles,
                    &plain.cable,
                ));
            }));
        }
        let mut catch_up = Vec::new();
        for _ in 0..200 {
            snapshot.load(&mut plain.consoles, &mut plain.cable);
            for gb in &mut plain.consoles {
                gb.ppu.blind = true;
                gb.apu.muted = true;
            }
            catch_up.push(time_ms(|| {
                for _ in 0..8 {
                    pair::run_frame(&mut plain.consoles, &mut plain.cable, &IDLE, [true; 2]);
                }
            }));
            for gb in &mut plain.consoles {
                gb.ppu.blind = false;
                gb.apu.muted = false;
            }
        }
        let (catch_up, catch_up_p95) = (median_ms(catch_up.clone()), p95_ms(catch_up));
        let rerun_p95 = p95_ms(rerun.clone());
        let (save, load, rerun, frame, hash) = (
            median_ms(save),
            median_ms(load),
            median_ms(rerun),
            median_ms(frame),
            median_ms(hash),
        );

        let mut session = Session::new(config(Player::One, 60), setups(roms)).unwrap();
        let mut worst = Vec::new();
        for f in 0..frames as u32 {
            session.add_local_input(f, inputs[0][f as usize]).unwrap();
            if f >= 8 {
                let flipped = Input::new(Keys::from_bits_truncate(u16::from(f as u8)));
                session.add_remote_input(f - 8, flipped);
            }
            let ms = time_ms(|| {
                session.step();
            });
            if f >= 60 {
                worst.push(ms);
            }
        }
        worst.sort_by(f64::total_cmp);
        let online = play_online(
            |player| Session::new(config(player, 60), setups(roms)).unwrap(),
            &inputs,
            Delay::Jitter(75.0, 20.0),
            |_, _| {},
        );
        let mut played = online.advance_ms;
        played.sort_by(f64::total_cmp);
        eprintln!(
            "{name}: snapshot {save:.3} ms, restore {load:.3} ms, 8 frames of two consoles {rerun:.3} ms (p95 {rerun_p95:.3} ms), \
         total {:.3} ms; 8 catch-up frames without picture or sound {catch_up:.3} ms (p95 {catch_up_p95:.3} ms); one frame of two consoles {frame:.3} ms; state hash {hash:.3} ms; \
         advance with an 8-frame rewind every frame: median {:.3} ms, p95 {:.3} ms, max {:.3} ms; \
         online at 150 ms RTT: advance median {:.3} ms, p95 {:.3} ms, max {:.3} ms, {} rewinds, deepest {} frames",
            save + load + rerun,
            worst[worst.len() / 2],
            worst[worst.len() * 95 / 100],
            worst[worst.len() - 1],
            played[played.len() / 2],
            played[played.len() * 95 / 100],
            played[played.len() - 1],
            online.rollbacks,
            online.deepest,
        );
    }

    #[test]
    #[ignore]
    #[expect(
        clippy::disallowed_methods,
        reason = "the rollback bench needs a 64 MiB stack"
    )]
    fn rollback_cost() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                measure("DMG Snake, unplugged", &roms(SNAKE, SNAKE), false);
                measure("DMG Snake, linked", &roms(SNAKE, SNAKE), true);
                measure("CGB Petris, unplugged", &roms(PETRIS, PETRIS), false);
                measure("CGB Petris, linked", &roms(PETRIS, PETRIS), true);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    fn long_run(roms: &[Rom; 2], frames: usize, plug: bool, blind: bool) -> ([u64; 2], [u64; 2]) {
        use std::io::Write;
        let mut inputs = [random_inputs(5, frames, 30), random_inputs(6, frames, 30)];
        for side in &mut inputs {
            for at in (100..frames.saturating_sub(10)).step_by(240) {
                press(side, at, Keys::START, 5);
            }
        }
        if plug {
            cable(&mut inputs[0], 0, CableEvent::Plug);
        }
        let config = config(Player::One, 1);
        let [one, two] = setups(roms);
        let mut consoles = [
            new_console(one.unwrap(), &config, Player::One).unwrap(),
            new_console(two.unwrap(), &config, Player::Two).unwrap(),
        ];
        for gb in &mut consoles {
            gb.ppu.blind = blind;
            gb.apu.muted = blind;
        }
        let mut cable = Cable::default();
        let mut scratch = consoles[0].clone_state();
        let mut states = [state::CheckHash::new(), state::CheckHash::new()];
        let mut pictures = [state::CheckHash::new(), state::CheckHash::new()];
        for (&one, &two) in inputs[0].iter().zip(&inputs[1]) {
            pair::run_frame(&mut consoles, &mut cable, &[one, two], [true; 2]);
            let hashes = state::hash_consoles(&mut scratch, &consoles, &cable);
            for i in 0..2 {
                states[i].write_all(&hashes[i].to_le_bytes()).unwrap();
                pictures[i]
                    .write_all(consoles[i].frame().as_slice())
                    .unwrap();
            }
        }
        (states.map(|f| f.finish()), pictures.map(|f| f.finish()))
    }

    const LONG_RUNS: [(&str, &[u8], &[u8], bool); 4] = [
        ("DMG Snake, linked", SNAKE, SNAKE, true),
        ("CGB Petris, linked", PETRIS, PETRIS, true),
        ("DMG Tobu and CGB uCity", TOBU, UCITY, false),
        ("DMG libbet and CGB Postie", LIBBET, POSTIE, false),
    ];

    #[test]
    fn catch_up_frames_end_in_the_same_state() {
        for (name, one, two, plug) in LONG_RUNS {
            let roms = roms(one, two);
            assert_eq!(
                long_run(&roms, 300, plug, true).0,
                long_run(&roms, 300, plug, false).0,
                "{name}"
            );
        }
    }

    #[test]
    #[ignore]
    fn long_run_hashes() {
        for (name, one, two, plug) in LONG_RUNS {
            let roms = roms(one, two);
            let (states, pictures) = long_run(&roms, 6000, plug, false);
            let (blind, _) = long_run(&roms, 6000, plug, true);
            assert_eq!(states, blind, "{name}");
            eprintln!("{name}: states {states:016x?}, pictures {pictures:016x?}");
        }
    }

    fn alone<'a>(rom: &'a Rom, state: Option<&'a [u8]>) -> [Option<ConsoleSetup<'a>>; 2] {
        [
            Some(ConsoleSetup {
                rom,
                kind: GbModel::for_rom(rom).unwrap().into(),
                save: None,
                state,
            }),
            None,
        ]
    }

    fn watched(rom: &Rom, local: Player, state: Option<&[u8]>) -> Session<GameBoy> {
        let mut session = Session::new(config(local, 60), alone(rom, state)).unwrap();
        session.watch_remote(true);
        session
    }

    fn run_alone(gb: &mut GameBoy, inputs: &[Input]) {
        for input in inputs {
            gb.set_keys(input.buttons);
            gb.run_frame();
        }
    }

    #[test]
    fn a_player_never_waits_for_a_watcher() {
        let rom = Rom::new(SNAKE);
        let inputs = random_inputs(7, 400, 20);
        let mut player = watched(&rom, Player::One, None);
        assert!(player.present(Player::One));
        assert!(!player.present(Player::Two));
        for (frame, &input) in inputs.iter().enumerate() {
            player.add_local_input(frame as u32, input).unwrap();
            player.add_remote_input(frame as u32, Input::new(Keys::A));
            assert_eq!(player.step(), Advance::Ran);
            assert_eq!(player.confirmed_frame(), player.frame());
            assert_eq!(player.last_rollback(), 0);
        }
        let config = config(Player::One, 60);
        let [setup, _] = alone(&rom, None);
        let mut plain = new_console(setup.unwrap(), &config, Player::One).unwrap();
        run_alone(&mut plain, &inputs);
        assert_same_consoles(player.console(Player::One), &plain, "player");
        let mut sound = Vec::new();
        player.drain_audio(&mut sound);
        assert!(!sound.is_empty());
    }

    #[test]
    fn a_watcher_runs_the_players_console_only_on_inputs_that_arrived() {
        let rom = Rom::new(SNAKE);
        let frames = 600u32;
        let inputs = random_inputs(9, frames as usize, 20);
        let mut player = watched(&rom, Player::One, None);
        let mut watcher = watched(&rom, Player::Two, None);
        let mut pads = Wire::new(Delay::Jitter(90.0, 40.0), 1);
        let mut checks = Wire::<Check>::new(Delay::Jitter(90.0, 40.0), 2);
        let (mut arrived, mut heard) = (0u32, Vec::new());
        for tick in 0..frames * 3 {
            let now = tick as f64 * FRAME_MS;
            if player.frame() < frames {
                let frame = player.frame();
                player
                    .add_local_input(frame, inputs[frame as usize])
                    .unwrap();
                assert_eq!(player.step(), Advance::Ran);
                pads.send(now, (frame, inputs[frame as usize]));
                while let Some(check) = player.take_check() {
                    checks.send(now, check);
                }
            }
            while let Some((frame, input)) = pads.arrived(now) {
                watcher.add_remote_input(frame, input);
                arrived = arrived.max(frame + 1);
            }
            while let Some(check) = checks.arrived(now) {
                watcher.add_remote_check(check);
            }
            let before = watcher.frame();
            let advanced = watcher.step();
            assert_eq!(advanced == Advance::Ran, before < arrived);
            assert!(watcher.frame() <= arrived);
            assert_eq!(watcher.last_rollback(), 0);
            while watcher.take_check().is_some() {}
            watcher.drain_audio(&mut heard);
        }
        assert_eq!(watcher.frame(), frames);
        assert_eq!(watcher.desync(), None);
        assert_same_consoles(
            watcher.console(Player::One),
            player.console(Player::One),
            "watcher",
        );
        assert!(!heard.is_empty(), "the watcher hears the player's game");
        assert!(watcher.local_save(true).is_none());
    }

    #[test]
    fn a_console_resumes_from_its_running_state() {
        let rom = Rom::new(SNAKE);
        let inputs = random_inputs(5, 500, 20);
        let config = config(Player::One, 60);
        let [setup, _] = alone(&rom, None);
        let mut plain = new_console(setup.unwrap(), &config, Player::One).unwrap();
        run_alone(&mut plain, &inputs[..250]);
        let state = crate::netplay::encode_state(&plain).unwrap();
        run_alone(&mut plain, &inputs[250..]);
        for local in PLAYERS {
            let mut session = watched(&rom, local, Some(&state));
            for (frame, &input) in inputs[250..].iter().enumerate() {
                match local {
                    Player::One => session.add_local_input(frame as u32, input).unwrap(),
                    Player::Two => session.add_remote_input(frame as u32, input),
                    _ => unreachable!(),
                }
                assert_eq!(session.step(), Advance::Ran);
            }
            assert_same_consoles(session.console(Player::One), &plain, &format!("{local:?}"));
        }
    }

    #[test]
    fn the_state_hash_for_a_fixed_seed_and_script_is_pinned() {
        let roms = roms(TOBU, PETRIS);
        let mut inputs = [random_inputs(21, 300, 20), random_inputs(22, 300, 20)];
        cable(&mut inputs[1], 0, CableEvent::Plug);
        let (_, checks) = play_reference(session(&roms, Player::One, 60), &inputs);
        let last = checks.last().unwrap();
        assert_eq!(last.frame, 240);
        assert_eq!(
            last.hashes,
            [5_248_057_994_921_852_960, 5_139_518_631_026_255_222]
        );
    }

    #[test]
    fn the_check_value_is_xxh3_64_whatever_the_writes() {
        use std::io::Write;
        assert_eq!(state::CheckHash::new().finish(), 0x2D06_8005_38D3_94C2);
        let bytes: Vec<u8> = (0..5000u32).map(|i| (i * 31 % 251) as u8).collect();
        let mut whole = state::CheckHash::new();
        whole.write_all(&bytes).unwrap();
        let mut pieces = state::CheckHash::new();
        for piece in bytes.chunks(7) {
            pieces.write_all(piece).unwrap();
        }
        assert_eq!(whole.finish(), pieces.finish());
        assert_eq!(
            whole.finish(),
            xxhash_rust::xxh3::xxh3_64_with_seed(&bytes, 0)
        );
    }

    #[test]
    fn consoles_in_the_same_state_give_the_same_check_value() {
        let roms = roms(TOBU, PETRIS);
        let inputs = [random_inputs(31, 120, 20), random_inputs(32, 120, 20)];
        let config = config(Player::One, 1);
        let [one, two] = setups(&roms);
        let mut consoles = [
            new_console(one.unwrap(), &config, Player::One).unwrap(),
            new_console(two.unwrap(), &config, Player::Two).unwrap(),
        ];
        let mut cable = Cable::default();
        for (&one, &two) in inputs[0].iter().zip(&inputs[1]) {
            pair::run_frame(&mut consoles, &mut cable, &[one, two], [true; 2]);
        }
        let mut scratch = consoles[0].clone_state();
        let mut twins = consoles.each_ref().map(|gb| gb.clone_state());
        let hashes = state::hash_consoles(&mut scratch, &consoles, &cable);
        assert_eq!(state::hash_consoles(&mut scratch, &twins, &cable), hashes);
        assert_ne!(hashes[0], hashes[1]);
        let held = Input::new(Keys::A | Keys::RIGHT);
        pair::run_frame(&mut twins, &mut cable, &[held, held], [true; 2]);
        let moved = state::hash_consoles(&mut scratch, &twins, &cable);
        assert_ne!(moved[0], hashes[0]);
        assert_ne!(moved[1], hashes[1]);
    }

    #[test]
    fn inputs_encode_like_the_joypad_state_they_replaced() {
        #[derive(serde::Serialize)]
        struct Before {
            buttons: crate::gb::joypad::JoypadState,
            cable: Option<CableEvent>,
        }
        for bits in 0..=u8::MAX {
            for cable in [None, Some(CableEvent::Plug), Some(CableEvent::Pull)] {
                let before = Before {
                    buttons: crate::gb::joypad::JoypadState::from_bits_retain(bits),
                    cable,
                };
                let input = Input {
                    buttons: Keys::from_bits_retain(u16::from(bits)),
                    cable,
                    pace: None,
                    touch: None,
                };
                assert_eq!(
                    rmp_serde::to_vec(&input).unwrap(),
                    rmp_serde::to_vec(&before).unwrap()
                );
                assert_eq!(
                    rmp_serde::to_vec_named(&input).unwrap(),
                    rmp_serde::to_vec_named(&before).unwrap()
                );
                assert_eq!(
                    serde_json::to_string(&input).unwrap(),
                    serde_json::to_string(&before).unwrap()
                );
            }
        }
    }

    #[cfg(feature = "nds")]
    mod ds {
        use super::*;
        use crate::console::Touch;
        use crate::nds::machine::{Nds, NdsFirmware};
        use crate::nds::spi::firmware::UserSettings;
        use crate::nds::spi::firmware::online::Online as DsOnline;
        use crate::netplay::{NEVER, NdsBoot, NdsSetup, encode_state};
        use citrine_gb_threads::{Mode, Threads};

        const NICKNAME: &[u8] = include_bytes!("../../citrine-gb-app/fixtures/nds/nickname.nds");
        const LEAD: u32 = 6;

        fn firmware(unit: u8) -> NdsFirmware {
            NdsFirmware {
                user_settings: Some(UserSettings {
                    nickname: format!("Unit {unit}"),
                    ..UserSettings::default()
                }),
                online: DsOnline {
                    mac: Some([0x00, 0x09, 0xBF, 0x12, 0x34, unit]),
                    ..DsOnline::default()
                },
                ..NdsFirmware::default()
            }
        }

        pub(super) fn ds_session(rom: &Rom, local: Player) -> Session<Nds> {
            let setups = [0, 1].map(|unit| {
                Some(NdsSetup {
                    boot: NdsBoot::Card(ConsoleSetup {
                        rom,
                        kind: ConsoleKind::Nds,
                        save: None,
                        state: None,
                    }),
                    firmware: firmware(unit),
                })
            });
            let config = SessionConfig {
                check_interval: 1,
                sync: Sync::lockstep(0),
                ..SessionConfig::new(local, 0x0D5_2026, START_TIME)
            };
            let mut session = Session::new_nds(config, setups).unwrap();
            session.watch_remote(true);
            session
        }

        #[test]
        #[ignore = "times a DS pair's frames with rollback; set CITRINE_DS_ROM, run in release"]
        fn ds_rollback_frame_time() {
            use std::time::{Duration, Instant};
            let var = |name: &str| std::env::var_os(name).map(std::path::PathBuf::from);
            let Some(path) = var("CITRINE_DS_ROM") else {
                return;
            };
            let rom = Rom::new(&std::fs::read(path).unwrap());
            let state = var("CITRINE_DS_STATE").map(|path| std::fs::read(path).unwrap());
            let num = |name: &str, or: u32| std::env::var(name).map_or(or, |v| v.parse().unwrap());
            let budget = num("ROLLBACK_BUDGET", 2);
            let late = num("REMOTE_LATE", budget);
            let change = num("REMOTE_CHANGE", 7).max(1);
            let frames = num("ROLLBACK_FRAMES", 1800);
            let mode = match num("ROLLBACK_LANES", 2) {
                1 => Mode::Inline,
                _ => Mode::Threads,
            };
            let threads = Threads::new(mode);
            let setups = std::array::from_fn(|unit| {
                Some(NdsSetup {
                    boot: NdsBoot::Card(ConsoleSetup {
                        rom: &rom,
                        kind: ConsoleKind::Nds,
                        save: None,
                        state: state.as_deref(),
                    }),
                    firmware: firmware(unit as u8),
                })
            });
            let config = SessionConfig {
                check_interval: NEVER,
                sync: Sync::new(budget, 0),
                ..SessionConfig::<2>::new(Player::One, 0x0D5_2026, START_TIME)
            };
            let mut session = Session::<Nds, 2>::new_nds(config, setups)
                .unwrap()
                .keeping_confirmed(num("KEPT_FRAMES", 20));
            #[cfg(feature = "jit")]
            for nds in session.consoles.iter_mut() {
                nds.set_jit(true);
            }
            let remote = |frame: u32| pad(1, frame * 7 / change);
            let mut times: Vec<Duration> = Vec::with_capacity(frames as usize);
            let mut replayed = 0u64;
            let mut deepest = 0;
            let mut sent = 0;
            let mut waits = 0;
            let mut frame = 0;
            session.add_local_input(0, pad(0, 0)).unwrap();
            while frame < frames {
                while sent + late <= frame {
                    session.add_player_input(Player::Two, sent, remote(sent));
                    sent += 1;
                }
                let t = Instant::now();
                let ran = threads.split(|split| session.advance_on(split));
                let spent = t.elapsed();
                if ran != Advance::Ran {
                    waits += 1;
                    session.add_player_input(Player::Two, sent, remote(sent));
                    sent += 1;
                    continue;
                }
                times.push(spent);
                replayed += u64::from(session.last_rollback());
                deepest = deepest.max(session.last_rollback());
                frame += 1;
                session.add_local_input(frame, pad(0, frame)).unwrap();
            }
            let warm = times.len() / 10;
            let mut kept: Vec<f64> = times[warm..]
                .iter()
                .map(|t| t.as_secs_f64() * 1e3)
                .collect();
            kept.sort_by(f64::total_cmp);
            let at = |q: f64| kept[((kept.len() - 1) as f64 * q).round() as usize];
            let mean = kept.iter().sum::<f64>() / kept.len() as f64;
            let over = kept.iter().filter(|&&ms| ms > 12.7).count();
            println!(
                "DSRB budget {budget} late {late} change {change} lanes {}: frame mean {mean:.2} p50 {:.2} p90 {:.2} p99 {:.2} max {:.2} ms, over 12.7 ms {over} of {}, replayed {:.2} a frame, deepest {deepest}, waits {waits}",
                threads.pool_size(),
                at(0.5),
                at(0.9),
                at(0.99),
                at(1.0),
                kept.len(),
                replayed as f64 / f64::from(frames),
            );
        }

        pub(super) fn ds_inputs(seed: u64, frames: usize) -> Vec<Input> {
            let mut rng = Rng(seed);
            let mut inputs = random_inputs(seed, frames, 30);
            for input in &mut inputs {
                if rng.below(4) == 0 {
                    *input = input.with_touch(Some(Touch {
                        x: rng.below(256) as u8,
                        y: rng.below(192) as u8,
                    }));
                }
            }
            inputs
        }

        pub(super) fn assert_in_step(online: &Online<Nds>, frames: usize) {
            for (side, session) in online.sessions.iter().enumerate().take(2) {
                assert!(session.desync().is_none(), "side {side}");
            }
            assert_eq!(online.resyncs, 0);
            let [mine, theirs] = [0, 1].map(|side| by_frame(&online.checks[side]));
            assert!(mine.len() >= frames - 1, "{} checks", mine.len());
            assert_eq!(mine, theirs);
        }

        #[test]
        fn two_dses_stay_in_step_at_150_ms() {
            let rom = Rom::new(NICKNAME);
            let frames = 600;
            let inputs = [ds_inputs(1, frames), ds_inputs(2, frames)];
            let online = play_online_ahead(
                |local| ds_session(&rom, local),
                &inputs,
                Delay::Jitter(75.0, 15.0),
                LEAD,
                |_, _| {},
            );
            assert_in_step(&online, frames);
            let reference = play_online_ahead(
                |local| ds_session(&rom, local),
                &inputs,
                Delay::Fixed(0.0),
                LEAD,
                |_, _| {},
            );
            assert_eq!(by_frame(&online.checks[0]), by_frame(&reference.checks[0]));
        }

        const AIRTALK: &[u8] = include_bytes!("../../citrine-gb-app/fixtures/nds/airtalk.nds");

        fn waiting_session(rom: &Rom, local: Player) -> Session<Nds> {
            let setups = [0, 1].map(|unit| {
                let boot = match unit {
                    0 => NdsBoot::Card(ConsoleSetup {
                        rom,
                        kind: ConsoleKind::Nds,
                        save: None,
                        state: None,
                    }),
                    _ => NdsBoot::DownloadPlay,
                };
                Some(NdsSetup {
                    boot,
                    firmware: firmware(unit),
                })
            });
            let config = SessionConfig {
                check_interval: 1,
                sync: Sync::lockstep(0),
                ..SessionConfig::new(local, 0x0D5_2026, START_TIME)
            };
            let mut session = Session::new_nds(config, setups).unwrap();
            session.watch_remote(true);
            session
        }

        #[test]
        fn a_console_waiting_for_download_play_stays_in_step() {
            let rom = Rom::new(AIRTALK);
            let frames = 300;
            let inputs = [ds_inputs(5, frames), ds_inputs(6, frames)];
            let online = play_online_ahead(
                |local| waiting_session(&rom, local),
                &inputs,
                Delay::Jitter(75.0, 15.0),
                LEAD,
                |_, _| {},
            );
            assert_in_step(&online, frames);
            for session in &online.sessions {
                let [host, waiting] = PLAYERS.map(|player| session.console(player));
                assert!(host.on_the_air() && waiting.on_the_air());
                assert!(waiting.download().is_some());
                assert_eq!(host.air_us(), waiting.air_us());
            }
        }

        fn late_guest_session(rom: &Rom, local: Player) -> Session<Nds> {
            let setups = [0, 1].map(|unit| {
                Some(NdsSetup {
                    boot: NdsBoot::Card(ConsoleSetup {
                        rom,
                        kind: ConsoleKind::Nds,
                        save: None,
                        state: None,
                    }),
                    firmware: firmware(unit),
                })
            });
            let config = SessionConfig {
                check_interval: 1,
                sync: Sync::new(2, 0),
                power_on: [0, 1],
                ..SessionConfig::new(local, 0x0D5_2026, START_TIME)
            };
            let mut session = Session::new_nds(config, setups).unwrap();
            session.watch_remote(true);
            session
        }

        #[test]
        fn a_guest_powered_on_late_replays_the_air_in_step() {
            let rom = Rom::new(AIRTALK);
            let frames = 60;
            for seed in 0..2 {
                let inputs = [ds_inputs(3 + seed, frames), ds_inputs(4 + seed, frames)];
                let delay = Delay::Jitter(20.0 + seed as f64 * 15.0, seed as f64 * 8.0);
                let online = play_online_ahead(
                    |local| late_guest_session(&rom, local),
                    &inputs,
                    delay,
                    1,
                    |_, _| {},
                );
                assert_in_step(&online, frames);
            }
        }

        const GUEST_AHEAD: u16 = 100;

        fn guest_mid_frame(rom: &Rom, local: Player) -> Session<Nds> {
            let mut session = waiting_session(rom, local);
            let mut ahead = session.console(Player::One).clone_state();
            let line = (ahead.buses().lcd().line() + GUEST_AHEAD) % crate::nds::lcd::LINES;
            while ahead.buses().lcd().line() != line {
                ahead.step_on_air();
            }
            let guest = session.console_mut(Player::Two);
            guest.buses_mut().adopt_beam(ahead.buses());
            session
        }

        fn guest_line(session: &Session<Nds>) -> u16 {
            session.console(Player::Two).buses().lcd().line()
        }

        fn run_lockstep(
            session: &mut Session<Nds>,
            inputs: &[Input],
            frames: std::ops::Range<usize>,
        ) {
            for frame in frames {
                session
                    .add_local_input(frame as u32, inputs[frame])
                    .unwrap();
                session.add_remote_input(frame as u32, inputs[frame]);
                assert_eq!(session.advance(), Advance::Ran);
            }
        }

        #[test]
        fn a_download_guest_ends_its_frames_at_the_hosts_clock() {
            let rom = Rom::new(AIRTALK);
            let mut session = guest_mid_frame(&rom, Player::One);
            let inputs = ds_inputs(7, 120);
            for frame in 0..inputs.len() {
                run_lockstep(&mut session, &inputs, frame..frame + 1);
                let [host, guest] = session.cable.clocks;
                assert!(
                    host.abs_diff(guest) < u64::from(crate::nds::lcd::LINE_CYCLES),
                    "frame {frame}: the host is at {host}, the guest at {guest}"
                );
                let line = guest_line(&session);
                assert!(
                    (1..crate::nds::lcd::VISIBLE_LINES).contains(&line),
                    "frame {frame}: the guest stopped on line {line}"
                );
            }
            let [host, guest] = PLAYERS.map(|player| session.console(player).frames());
            assert!(
                host.abs_diff(guest) <= 1,
                "{host} host frames, {guest} guest frames"
            );
        }

        #[test]
        fn a_download_guest_takes_its_input_at_the_cut() {
            let rom = Rom::new(AIRTALK);
            let mut session = guest_mid_frame(&rom, Player::One);
            let mut inputs = vec![Input::default(); 40];
            inputs[30] = Input::new(Keys::A);
            run_lockstep(&mut session, &inputs, 0..30);
            assert!(!session.console(Player::Two).keys().contains(Keys::A));
            run_lockstep(&mut session, &inputs, 30..31);
            assert!(session.console(Player::Two).keys().contains(Keys::A));
            assert!((1..crate::nds::lcd::VISIBLE_LINES).contains(&guest_line(&session)));
            run_lockstep(&mut session, &inputs, 31..32);
            assert!(!session.console(Player::Two).keys().contains(Keys::A));
        }

        #[test]
        fn a_download_guest_cut_mid_frame_stays_in_step_with_its_friend() {
            let rom = Rom::new(AIRTALK);
            let frames = 300;
            let inputs = [ds_inputs(5, frames), ds_inputs(6, frames)];
            let online = play_online_ahead(
                |local| guest_mid_frame(&rom, local),
                &inputs,
                Delay::Jitter(75.0, 15.0),
                LEAD,
                |_, _| {},
            );
            assert_in_step(&online, frames);
            for session in &online.sessions {
                assert!((1..crate::nds::lcd::VISIBLE_LINES).contains(&guest_line(session)));
            }
        }

        #[test]
        fn a_download_guest_saved_mid_frame_loads_to_the_same_run() {
            let rom = Rom::new(AIRTALK);
            let inputs = ds_inputs(8, 180);
            let mut straight = guest_mid_frame(&rom, Player::One);
            let mut loaded = guest_mid_frame(&rom, Player::One);
            run_lockstep(&mut straight, &inputs, 0..60);
            run_lockstep(&mut loaded, &inputs, 0..60);
            assert!((1..crate::nds::lcd::VISIBLE_LINES).contains(&guest_line(&loaded)));
            let guest = loaded.console_mut(Player::Two);
            let state = guest.encode_state().unwrap();
            *guest = Rollback::decode_state(&*guest, &state).unwrap();
            let hashes = |session: &Session<Nds>| {
                PLAYERS.map(|player| {
                    let nds = session.console(player);
                    nds.state_hash(&mut nds.scratch())
                })
            };
            assert_eq!(hashes(&loaded), hashes(&straight));
            for frame in 60..inputs.len() {
                run_lockstep(&mut straight, &inputs, frame..frame + 1);
                run_lockstep(&mut loaded, &inputs, frame..frame + 1);
                assert_eq!(hashes(&loaded), hashes(&straight), "frame {frame}");
                assert_eq!(loaded.cable.clocks, straight.cable.clocks, "frame {frame}");
            }
            assert!(loaded.console(Player::Two).frame() == straight.console(Player::Two).frame());
        }

        #[test]
        fn a_download_guest_asleep_at_a_cut_loads_to_the_same_run() {
            use crate::nds::machine::test_rom::{BRANCH_TO_SELF, SLEEP_ON_LID, rom};
            let host = Rom::new(AIRTALK);
            let session = |delay: u32| {
                let mut session = waiting_session(&host, Player::One);
                let mut sleeper =
                    Nds::new(rom(&[BRANCH_TO_SELF], &SLEEP_ON_LID), firmware(1)).unwrap();
                for _ in 0..delay {
                    sleeper.step_on_air();
                }
                *session.console_mut(Player::Two) = sleeper;
                session
            };
            let mut inputs = vec![Input::default(); 160];
            for input in &mut inputs[40..70] {
                *input = Input::new(Keys::LID);
            }
            let mut straight = session(37);
            let mut loaded = session(37);
            run_lockstep(&mut straight, &inputs, 0..20);
            run_lockstep(&mut loaded, &inputs, 0..20);
            assert!(loaded.console(Player::Two).buses().sleeping());
            let guest = loaded.console_mut(Player::Two);
            let state = guest.encode_state().unwrap();
            *guest = Rollback::decode_state(&*guest, &state).unwrap();
            let hashes = |session: &Session<Nds>| {
                PLAYERS.map(|player| {
                    let nds = session.console(player);
                    nds.state_hash(&mut nds.scratch())
                })
            };
            for frame in 20..inputs.len() {
                run_lockstep(&mut straight, &inputs, frame..frame + 1);
                run_lockstep(&mut loaded, &inputs, frame..frame + 1);
                assert_eq!(hashes(&loaded), hashes(&straight), "frame {frame}");
                assert_eq!(loaded.cable.clocks, straight.cable.clocks, "frame {frame}");
            }
            assert!(!straight.console(Player::Two).buses().sleeping());
            let [host, guest] = straight.cable.clocks;
            assert!(host.abs_diff(guest) < u64::from(crate::nds::lcd::LINE_CYCLES));
        }

        #[derive(Debug)]
        struct Report {
            role: u32,
            linked: u32,
            sent: u32,
            heard: u32,
            checksum: u32,
        }

        fn report(nds: &Nds) -> Report {
            let ram = nds.buses().main_ram();
            let at = ram
                .windows(8)
                .position(|w| w == b"AIRTALK\0")
                .expect("the report is in main RAM");
            let word = |i: usize| {
                let from = at + 8 + i * 4;
                u32::from_le_bytes(ram[from..from + 4].try_into().unwrap())
            };
            Report {
                role: word(0),
                linked: word(1),
                sent: word(2),
                heard: word(3),
                checksum: word(6),
            }
        }

        fn talk_over_the_air(frames: usize) {
            let rom = Rom::new(AIRTALK);
            let inputs = [ds_inputs(3, frames), ds_inputs(4, frames)];
            let online = play_online_ahead(
                |local| ds_session(&rom, local),
                &inputs,
                Delay::Jitter(75.0, 15.0),
                LEAD,
                |_, _| {},
            );
            assert_in_step(&online, frames);
            let session = &online.sessions[0];
            let [host, client] = PLAYERS.map(|player| report(session.console(player)));
            assert_eq!((host.role, client.role), (1, 2), "{host:?} {client:?}");
            assert_eq!((host.linked, client.linked), (1, 1), "{host:?} {client:?}");
            let talking = frames as u32 / 2;
            assert!(host.heard > talking, "{host:?} {client:?}");
            assert!(client.heard > talking, "{host:?} {client:?}");
            assert!(host.sent.abs_diff(client.heard) <= 2, "{host:?} {client:?}");
            assert!(client.sent.abs_diff(host.heard) <= 2, "{host:?} {client:?}");
            for player in PLAYERS {
                let theirs = report(online.sessions[1].console(player));
                let mine = report(session.console(player));
                assert_eq!(theirs.checksum, mine.checksum);
            }
        }

        fn air_session(rom: &Rom, states: [Option<&[u8]>; 2]) -> Session<Nds> {
            let setups = [0, 1].map(|unit| {
                Some(NdsSetup {
                    boot: NdsBoot::Card(ConsoleSetup {
                        rom,
                        kind: ConsoleKind::Nds,
                        save: None,
                        state: states[unit],
                    }),
                    firmware: firmware(unit as u8),
                })
            });
            let config = SessionConfig {
                check_interval: NEVER,
                sync: Sync::lockstep(0),
                ..SessionConfig::new(Player::One, 0x0D5_2026, START_TIME)
            };
            let mut session = Session::new_nds(config, setups).unwrap();
            session.watch_remote(true);
            session
        }

        fn run_idle(session: &mut Session<Nds>, frames: std::ops::Range<u32>) {
            for frame in frames {
                session.add_local_input(frame, IDLE[0]).unwrap();
                session.add_remote_input(frame, IDLE[1]);
                assert!(matches!(session.advance(), Advance::Ran));
            }
        }

        #[test]
        fn two_dses_resumed_from_states_saved_apart_share_one_air_clock() {
            let rom = Rom::new(AIRTALK);
            let mut first = air_session(&rom, [None, None]);
            run_idle(&mut first, 0..300);
            let host = encode_state(first.console(Player::One)).unwrap();
            run_idle(&mut first, 300..520);
            let client = encode_state(first.console(Player::Two)).unwrap();
            let mut resumed = air_session(&rom, [Some(host.as_slice()), Some(client.as_slice())]);
            let before = PLAYERS.map(|player| report(resumed.console(player)).heard);
            run_idle(&mut resumed, 0..600);
            let [one, two] = PLAYERS.map(|player| resumed.console(player).air_us());
            assert_eq!(one, two);
            let after = PLAYERS.map(|player| report(resumed.console(player)).heard);
            assert!(
                after[0] > before[0] && after[1] > before[1],
                "{before:?} {after:?}"
            );
        }

        #[test]
        fn two_dses_resumed_from_states_saved_apart_drop_frames_heard_on_the_old_air() {
            use crate::nds::wifi::frame::{Frame, Rate};
            let rom = Rom::new(AIRTALK);
            let mut first = air_session(&rom, [None, None]);
            run_idle(&mut first, 0..300);
            let host = encode_state(first.console(Player::One)).unwrap();
            run_idle(&mut first, 300..520);
            let mut client = first.console(Player::Two).clone_state();
            let channel = client.buses().wifi().channel();
            client.hear_on_air(Frame {
                start: client.air_us() + 3_000_000,
                channel,
                rate: Rate::Mbit2,
                short_preamble: false,
                bytes: vec![0x80, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
            });
            assert_eq!(client.buses().wifi().frames_due(), 1);
            let client = encode_state(&client).unwrap();
            let resumed = air_session(&rom, [Some(host.as_slice()), Some(client.as_slice())]);
            assert_eq!(resumed.console(Player::Two).buses().wifi().frames_due(), 0);
        }

        #[test]
        fn two_dses_talk_over_the_air_at_150_ms() {
            talk_over_the_air(1_200);
        }

        #[test]
        #[ignore = "ten minutes of two DSes on two machines; run in release"]
        fn two_dses_talk_over_the_air_for_ten_minutes() {
            talk_over_the_air(36_000);
        }

        fn linked_hashes<const N: usize>(threads: &Threads, frames: u32) -> Vec<[u64; N]> {
            let rom = Rom::new(AIRTALK);
            let setups = std::array::from_fn(|unit| {
                Some(NdsSetup {
                    boot: NdsBoot::Card(ConsoleSetup {
                        rom: &rom,
                        kind: ConsoleKind::Nds,
                        save: None,
                        state: None,
                    }),
                    firmware: firmware(unit as u8),
                })
            });
            let config = SessionConfig {
                check_interval: NEVER,
                sync: Sync::lockstep(0),
                ..SessionConfig::<N>::new(Player::One, 0x0D5_2026, START_TIME)
            };
            let mut session = Session::<Nds, N>::new_nds(config, setups).unwrap();
            session.watch_remote(true);
            let inputs: [Vec<Input>; N] =
                std::array::from_fn(|unit| ds_inputs(11 + unit as u64, frames as usize));
            let mut scratch = session.consoles[0].clone_state();
            let mut hashes = Vec::new();
            for frame in 0..frames {
                for (unit, inputs) in inputs.iter().enumerate() {
                    let input = inputs[frame as usize];
                    match Player::ALL[unit] {
                        Player::One => session.add_local_input(frame, input).unwrap(),
                        player => session.add_player_input(player, frame, input),
                    }
                }
                let ran = threads.split(|split| session.advance_on(split));
                assert_eq!(ran, Advance::Ran);
                hashes.push(state::hash_consoles(
                    &mut scratch,
                    &session.consoles,
                    &session.cable,
                ));
            }
            let heard = session.consoles.each_ref().map(|nds| report(nds).heard);
            let talking = heard.iter().filter(|&&heard| heard > frames / 4).count();
            assert!(talking >= 2, "{heard:?}");
            hashes
        }

        fn assert_linked_hashes_match<const N: usize>(frames: u32) {
            let inline = linked_hashes::<N>(&Threads::new(Mode::Inline), frames);
            assert_eq!(
                linked_hashes::<N>(&Threads::new(Mode::Threads), frames),
                inline
            );
            for seed in [1, 77] {
                let shuffled = linked_hashes::<N>(&Threads::new(Mode::Shuffled(seed)), frames);
                assert_eq!(shuffled, inline, "seed {seed}");
            }
        }

        #[test]
        fn a_linked_ds_pair_hashes_the_same_every_frame_on_one_thread_and_on_threads() {
            assert_linked_hashes_match::<2>(600);
        }

        #[test]
        fn four_linked_dses_hash_the_same_every_frame_on_one_thread_and_on_threads() {
            assert_linked_hashes_match::<4>(600);
        }
    }

    const LOCKSTEP: Sync = Sync::lockstep(2);

    const ROLLBACK: Sync = Sync::new(30, 0);

    const CHECK_EVERY: u32 = 10;

    fn mix(seed: u64) -> u64 {
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    #[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct Unit {
        state: u64,
        keys: Keys,
        frames: u32,
        rank: Option<usize>,
        steps: u32,
        outbox: Vec<u64>,
        heard: u32,
    }

    impl Unit {
        fn new(seed: u64) -> Self {
            Self {
                state: mix(seed),
                ..Self::default()
            }
        }

        fn tick(&mut self) {
            self.state = mix(self.state ^ u64::from(self.keys.bits()));
        }
    }

    impl Console for Unit {
        fn kind(&self) -> ConsoleKind {
            ConsoleKind::Gb
        }

        fn video(&self) -> VideoSpec {
            VideoSpec::of(&[])
        }

        fn timing(&self) -> Timing {
            Timing {
                clock_hz: 4_194_304,
                cycles_per_frame: 70_224,
            }
        }

        fn run_frame(&mut self) {
            self.tick();
            self.frames += 1;
        }

        fn frame(&self) -> &[u8] {
            &[]
        }

        fn take_audio(&mut self, _out: &mut Vec<f32>) {}

        fn sample_rate(&self) -> u32 {
            44_100
        }

        fn set_sample_rate(&mut self, _rate: u32) {}

        fn keys(&self) -> Keys {
            self.keys
        }

        fn set_keys(&mut self, keys: Keys) {
            self.keys = keys;
        }

        fn soft_reset(&mut self) {
            unimplemented!()
        }

        fn audio_channels(&self) -> &'static [&'static str] {
            &[]
        }

        fn set_channel_enabled(&mut self, _index: usize, _enabled: bool) {}

        fn rom_bytes(&self) -> std::sync::Arc<[u8]> {
            std::sync::Arc::from([])
        }

        fn title(&self) -> String {
            String::new()
        }

        fn has_battery(&self) -> bool {
            false
        }

        fn has_clock(&self) -> bool {
            false
        }

        fn battery(&mut self, _force: bool, _now: u64) -> Option<BatterySave> {
            None
        }

        fn battery_from_file(&self, _data: &[u8]) -> BatterySave {
            unimplemented!()
        }

        fn put_battery(&mut self, _save: BatterySave, _now: u64) {}

        fn save_state(&mut self, _now: u64) -> GbResult<Vec<u8>> {
            unimplemented!()
        }

        fn state_body(&mut self, _now: u64) -> GbResult<StateBody> {
            unimplemented!()
        }

        #[cfg(feature = "rom-id")]
        fn rom_id(&self) -> RomId {
            unimplemented!()
        }
    }

    impl Rollback for Unit {
        fn clone_state(&self) -> Self {
            self.clone()
        }

        fn copy_state_from(&mut self, other: &Self) {
            self.clone_from(other);
        }

        fn state_hash(&self, _scratch: &mut Self) -> u64 {
            let bytes = rmp_serde::to_vec(self).unwrap();
            bytes
                .iter()
                .fold(0, |hash, &byte| mix(hash ^ u64::from(byte)))
        }

        fn encode_state(&self) -> GbResult<Vec<u8>> {
            rmp_serde::to_vec(self).map_err(|error| GbError::IO(std::io::Error::other(error)))
        }

        fn decode_state(&self, data: &[u8]) -> GbResult<Self> {
            rmp_serde::from_slice(data).map_err(|error| GbError::IO(std::io::Error::other(error)))
        }

        fn set_blind(&mut self, _blind: bool) {}

        fn set_muted(&mut self, _muted: bool) {}
    }

    impl LinkPort for Unit {
        type Message = u64;

        fn plug_link(&mut self) {
            self.plug_link_as(0, 2);
        }

        fn plug_link_as(&mut self, unit: usize, _units: usize) {
            self.rank = Some(unit);
        }

        fn unplug_link(&mut self) {
            self.rank = None;
        }

        fn link_plugged(&self) -> bool {
            self.rank.is_some()
        }

        fn link_waiting(&self) -> bool {
            false
        }

        fn take_link_messages(&mut self) -> Vec<u64> {
            std::mem::take(&mut self.outbox)
        }

        fn receive_link_message(&mut self, message: u64) {
            self.state = mix(self.state ^ message);
            self.heard += 1;
        }

        fn is_transfer(_message: &u64) -> bool {
            true
        }

        fn begin_linked_frame(&mut self) -> u64 {
            self.steps = 0;
            0
        }

        fn step_linked(&mut self) -> u64 {
            self.tick();
            self.steps += 1;
            if self.steps.is_multiple_of(3) {
                let rank = self.rank.unwrap_or_default() as u64;
                self.outbox.push(mix(self.state ^ rank));
            }
            1_000 + self.state % 512
        }

        fn linked_frame_done(&self) -> bool {
            self.steps >= 9
        }

        fn end_linked_frame(&mut self) {
            self.frames += 1;
        }
    }

    fn pad(unit: usize, frame: u32) -> Input {
        let held = mix(((unit as u64) << 32) | u64::from(frame / 7));
        let input = Input::new(Keys::from_bits_truncate(held as u16));
        if unit == 0 && frame == 0 {
            input.with_cable(CableEvent::Plug)
        } else {
            input
        }
    }

    fn units<const N: usize>() -> [Unit; N] {
        std::array::from_fn(|i| Unit::new(i as u64 + 1))
    }

    fn joiner() -> Unit {
        Unit::new(99)
    }

    trait Units: Rollback + LinkPort + Sized {
        fn units<const N: usize>() -> [Self; N];
        fn joiner() -> Self;
    }

    impl Units for Unit {
        fn units<const N: usize>() -> [Self; N] {
            units()
        }

        fn joiner() -> Self {
            joiner()
        }
    }

    struct Straight<const N: usize, C = Unit> {
        power_on: [u32; N],
        leave_at: [u32; N],
        join: Option<(u32, usize)>,
        window: u32,
        units: PhantomData<C>,
    }

    impl<const N: usize, C: Units> Straight<N, C> {
        fn new() -> Self {
            Self {
                power_on: [0; N],
                leave_at: [NEVER; N],
                join: None,
                window: DEFAULT_CABLE_WINDOW,
                units: PhantomData,
            }
        }

        fn run(
            &self,
            frames: u32,
            paces: &[(u32, usize, CablePace)],
        ) -> (Vec<[u64; N]>, Vec<bool>) {
            let mut consoles = C::units::<N>();
            let mut cable = Cable::<C::Message, N> {
                window: u64::from(self.window),
                ..Cable::default()
            };
            let mut scratch = C::joiner();
            let mut hashes = Vec::new();
            let mut windows = Vec::new();
            for frame in 0..frames {
                if let Some((at, unit)) = self.join
                    && at == frame
                {
                    consoles[unit] = C::joiner();
                    cable.reseat();
                }
                hashes.push(state::hash_consoles(&mut scratch, &consoles, &cable));
                let on: [bool; N] =
                    std::array::from_fn(|i| frame >= self.power_on[i] && frame < self.leave_at[i]);
                let inputs = std::array::from_fn(|i| {
                    if frame < self.power_on[i] || frame >= self.leave_at[i] {
                        Input::default()
                    } else {
                        paced(pad(i, frame), paces, i, frame)
                    }
                });
                windows.push(pair::run_frame(&mut consoles, &mut cable, &inputs, on).is_some());
            }
            (hashes, windows)
        }
    }

    enum Payload<const N: usize> {
        Pad(u32, Input),
        Check(Check<N>),
    }

    struct Packet<const N: usize> {
        due: u32,
        from: usize,
        to: usize,
        payload: Payload<N>,
    }

    fn paced(input: Input, paces: &[(u32, usize, CablePace)], unit: usize, frame: u32) -> Input {
        paces
            .iter()
            .find(|&&(at, from, _)| at == frame && from == unit)
            .map_or(input, |&(_, _, pace)| input.with_pace(pace))
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Lanes {
        FromEnv,
        #[cfg(feature = "gba")]
        Inline,
        #[cfg(feature = "gba")]
        Starved,
    }

    struct Room<const N: usize, C: Rollback + LinkPort = Unit> {
        sessions: [Option<Session<C, N>>; N],
        lanes: [Lanes; N],
        paces: Vec<(u32, usize, CablePace)>,
        windows: [Vec<(u32, bool)>; N],
        sent: [u32; N],
        silent: [bool; N],
        wire: Vec<Packet<N>>,
        checks: Vec<Check<N>>,
        tick: u32,
        rng: u64,
        lag: u32,
        jitter: u32,
        rollbacks: u32,
        deepest: u32,
        furthest: u32,
        waited: u32,
    }

    impl<const N: usize> Room<N> {
        fn new(sync: Sync, present: [bool; N], lag: u32, jitter: u32) -> Self {
            Self::with_units(sync, present, lag, jitter)
        }
    }

    impl<const N: usize, C: Units> Room<N, C> {
        fn with_units(sync: Sync, present: [bool; N], lag: u32, jitter: u32) -> Self {
            Self::windowed(sync, present, lag, jitter, DEFAULT_CABLE_WINDOW)
        }

        fn windowed(sync: Sync, present: [bool; N], lag: u32, jitter: u32, window: u32) -> Self {
            let sessions = std::array::from_fn(|i| {
                present[i].then(|| {
                    let config = SessionConfig {
                        check_interval: CHECK_EVERY,
                        sync,
                        cable_window: window,
                        ..SessionConfig::new(Player::ALL[i], 7, 0)
                    };
                    let mut session = Session::with_consoles(config, C::units::<N>(), present);
                    session.watch_remote(true);
                    session
                })
            });
            Self {
                sessions,
                lanes: [Lanes::FromEnv; N],
                paces: Vec::new(),
                windows: std::array::from_fn(|_| Vec::new()),
                sent: [0; N],
                silent: [false; N],
                wire: Vec::new(),
                checks: Vec::new(),
                tick: 0,
                rng: 0x5EED,
                lag,
                jitter,
                rollbacks: 0,
                deepest: 0,
                furthest: 0,
                waited: 0,
            }
        }

        fn session(&mut self, player: Player) -> &mut Session<C, N> {
            self.sessions[player.index()].as_mut().unwrap()
        }

        fn send(&mut self, from: usize, payload: impl Fn() -> Payload<N>) {
            for to in (0..N).filter(|&to| to != from) {
                self.rng = mix(self.rng);
                let due = self.tick + self.lag + (self.rng % u64::from(self.jitter + 1)) as u32;
                self.wire.push(Packet {
                    due,
                    from,
                    to,
                    payload: payload(),
                });
            }
        }

        fn step(&mut self) {
            self.tick += 1;
            let tick = self.tick;
            let (due, wire) = std::mem::take(&mut self.wire)
                .into_iter()
                .partition::<Vec<_>, _>(|packet| packet.due <= tick);
            self.wire = wire;
            for packet in due {
                let Some(session) = self.sessions[packet.to].as_mut() else {
                    continue;
                };
                match packet.payload {
                    Payload::Pad(frame, input) => {
                        session.add_player_input(Player::ALL[packet.from], frame, input)
                    }
                    Payload::Check(check) => session.add_remote_check(check),
                }
            }
            for unit in 0..N {
                if self.silent[unit] {
                    continue;
                }
                let Some(session) = self.sessions[unit].as_mut() else {
                    continue;
                };
                let ahead = session.frame() + session.sync().delay;
                let mut pads = Vec::new();
                while self.sent[unit] <= ahead {
                    let frame = self.sent[unit];
                    let mut input = pad(unit, frame);
                    if let Some(pace) = session.cable_pace() {
                        input = input.with_pace(pace);
                        self.paces.push((frame, unit, pace));
                    }
                    session.add_local_input(frame, input).unwrap();
                    pads.push((frame, input));
                    self.sent[unit] += 1;
                }
                let advance = match self.lanes[unit] {
                    Lanes::FromEnv => session.step(),
                    #[cfg(feature = "gba")]
                    Lanes::Inline => session.advance_on(&mut Split::inline()),
                    #[cfg(feature = "gba")]
                    Lanes::Starved => session.advance_on(&mut Split::starved()),
                };
                if advance == crate::netplay::Advance::Ran {
                    let ran = session.frame() - 1;
                    self.windows[unit].push((ran, session.cable_turnout().is_some()));
                } else {
                    self.waited += 1;
                }
                self.rollbacks += session.last_rollback();
                self.deepest = self.deepest.max(session.last_rollback());
                self.furthest = self.furthest.max(session.frames_ahead());
                assert_eq!(session.desync(), None, "unit {unit} at tick {tick}");
                let mut checks = Vec::new();
                while let Some(check) = session.take_check() {
                    checks.push(check);
                }
                for (frame, input) in pads {
                    self.send(unit, || Payload::Pad(frame, input));
                }
                for check in checks {
                    self.checks.push(check);
                    self.send(unit, || Payload::Check(check));
                }
            }
        }

        fn run_until(&mut self, frame: u32) {
            let limit = self.tick + frame * 4 + 200;
            while (0..N).any(|i| {
                !self.silent[i] && self.sessions[i].as_ref().is_some_and(|s| s.frame() < frame)
            }) {
                assert!(self.tick < limit, "stuck at tick {}", self.tick);
                self.step();
            }
        }

        fn frames(&self) -> Vec<u32> {
            (0..N)
                .filter(|&i| !self.silent[i])
                .filter_map(|i| self.sessions[i].as_ref().map(Session::frame))
                .collect()
        }

        fn assert_matches(&self, straight: &Straight<N, C>, frames: u32) {
            let (reference, _) = straight.run(frames, &self.paces);
            let mut compared = 0;
            for check in self.checks.iter().filter(|c| c.frame < frames) {
                assert_eq!(
                    check.hashes, reference[check.frame as usize],
                    "check of frame {}",
                    check.frame
                );
                compared += 1;
            }
            assert!(
                compared >= (frames / CHECK_EVERY) as usize,
                "{compared} checks"
            );
        }
    }

    #[test]
    fn three_units_in_lockstep_match_a_straight_run() {
        let mut room = Room::<3>::new(LOCKSTEP, [true; 3], 1, 2);
        room.run_until(300);
        assert_eq!(room.rollbacks, 0);
        room.assert_matches(&Straight::new(), 300);
        let session = room.session(Player::Two);
        assert!(session.cable_plugged());
        assert!(session.console(Player::Three).heard > 0);
    }

    #[test]
    fn four_units_with_rollback_match_a_straight_run() {
        let mut room = Room::<4>::new(ROLLBACK, [true; 4], 3, 4);
        room.run_until(300);
        assert!(room.rollbacks > 0);
        room.assert_matches(&Straight::new(), 300);
        let session = room.session(Player::One);
        assert!((0..4).all(|i| session.console(Player::ALL[i]).heard > 0));
    }

    #[test]
    fn lockstep_never_runs_a_frame_it_has_not_heard() {
        let mut room = Room::<3>::new(LOCKSTEP, [true; 3], 6, 0);
        for _ in 0..200 {
            room.step();
            for i in 0..3 {
                let session = room.session(Player::ALL[i]);
                assert_eq!(session.frames_ahead(), 0);
            }
        }
        assert_eq!(room.rollbacks, 0);
    }

    #[test]
    fn a_peer_late_past_the_budget_makes_the_others_wait_instead_of_rolling_back_deeper() {
        let budget = Sync::new(4, 0);
        let mut room = Room::<2>::new(budget, [true; 2], 12, 3);
        room.run_until(300);
        assert!(room.rollbacks > 0);
        assert_eq!(room.furthest, 4);
        assert!(room.deepest <= 4, "{}", room.deepest);
        room.assert_matches(&Straight::new(), 300);
    }

    #[test]
    fn a_waiting_session_runs_again_once_the_late_input_arrives() {
        let mut room = Room::<2>::new(Sync::new(4, 0), [true; 2], 12, 0);
        for _ in 0..40 {
            room.step();
        }
        let session = room.session(Player::One);
        assert_eq!(session.frames_ahead(), 4);
        let frame = session.frame();
        session.add_local_input(frame, pad(0, frame)).unwrap();
        assert_eq!(session.step(), crate::netplay::Advance::AwaitingRemote);
        assert_eq!(session.frame(), frame);
        assert_eq!(session.waiting_for(), Some(Player::Two));
        room.run_until(200);
        assert!(room.session(Player::One).frame() >= 200);
    }

    #[test]
    fn rollback_within_the_budget_gives_the_frames_lockstep_gives() {
        let mut rolled = Room::<2>::new(Sync::new(8, 1), [true; 2], 3, 4);
        rolled.run_until(300);
        let mut stepped = Room::<2>::new(Sync::lockstep(1), [true; 2], 3, 4);
        stepped.run_until(300);
        assert!(rolled.rollbacks > 0);
        assert_eq!(stepped.rollbacks, 0);
        assert!(rolled.deepest <= 8);
        let checks = |room: &Room<2>| {
            let mut checks: Vec<_> = room
                .checks
                .iter()
                .filter(|c| c.frame < 300)
                .copied()
                .collect();
            checks.sort_by_key(|c| c.frame);
            checks.dedup();
            checks
        };
        assert_eq!(checks(&rolled), checks(&stepped));
        rolled.assert_matches(&Straight::new(), 300);
    }

    fn link_stats(sync: Sync, lag: u32, jitter: u32) -> (u32, u32) {
        let mut room = Room::<2>::new(sync, [true; 2], lag, jitter);
        room.run_until(600);
        room.assert_matches(&Straight::new(), 600);
        (room.waited, room.deepest)
    }

    #[test]
    fn game_boy_links_at_80_and_150_ms_wait_less_with_the_full_window() {
        for (lag, jitter, delay) in [(2, 2, 0), (4, 3, 2)] {
            let full = link_stats(Sync::new(MAX_ROLLBACK_FRAMES, delay), lag, jitter);
            let tight = link_stats(Sync::new(2, delay), lag, jitter);
            assert!(full.0 <= tight.0, "{full:?} {tight:?}");
            assert!(full.1 <= MAX_ROLLBACK_FRAMES);
        }
    }

    fn drop_mid_session<const N: usize>(sync: Sync, lag: u32, jitter: u32) {
        let mut room = Room::<N>::new(sync, [true; N], lag, jitter);
        room.run_until(120);
        room.silent[2] = true;
        for _ in 0..80 {
            room.step();
        }
        let stalled = room.frames();
        for _ in 0..10 {
            room.step();
        }
        assert_eq!(room.frames(), stalled);
        let at = room.sent[2];
        for i in (0..N).filter(|&i| i != 2) {
            let session = room.session(Player::ALL[i]);
            assert_eq!(session.waiting_for(), Some(Player::Three));
            assert_eq!(session.confirmed_frame(), at);
            session.leave(Player::Three, at).unwrap();
            assert_eq!(
                session.leave(Player::Three, at),
                Err(MemberError::Absent(2))
            );
        }
        room.run_until(400);
        let mut straight = Straight::new();
        straight.leave_at[2] = at;
        room.assert_matches(&straight, 400);
        for i in (0..N).filter(|&i| i != 2) {
            let session = room.session(Player::ALL[i]);
            assert_eq!(session.console(Player::Three).frames, at);
            assert!(session.console(Player::One).heard > 0);
        }
    }

    #[test]
    fn a_dropped_unit_stops_and_the_others_play_on_in_lockstep() {
        drop_mid_session::<3>(LOCKSTEP, 1, 2);
    }

    #[test]
    fn a_dropped_unit_stops_and_the_others_play_on_with_rollback() {
        drop_mid_session::<4>(ROLLBACK, 3, 4);
    }

    fn late_joiner(sync: Sync) {
        const JOIN: u32 = 120;
        let mut room = Room::<3>::new(sync, [true, true, false], 2, 2);
        room.run_until(60);
        for i in 0..2 {
            room.session(Player::ALL[i])
                .join(Player::Three, JOIN, joiner())
                .unwrap();
        }
        assert_eq!(
            room.session(Player::One)
                .join(Player::Three, JOIN, joiner()),
            Err(MemberError::Present(2))
        );
        room.run_until(JOIN);
        let states = loop {
            let source = room.session(Player::One);
            let states: Option<Vec<Vec<u8>>> = (0..3)
                .map(|i| {
                    let player = Player::ALL[i];
                    if let Some(state) = source.checkpoint(JOIN, player) {
                        return Some(state);
                    }
                    let here = source.frame() == JOIN && source.confirmed_frame() == JOIN;
                    let console = if i == 2 {
                        joiner()
                    } else {
                        source.console(player).clone()
                    };
                    here.then(|| crate::netplay::encode_state(&console).unwrap())
                })
                .collect();
            if let Some(states) = states {
                break states;
            }
            assert!(room.tick < 1_000, "no state at frame {JOIN}");
            room.step();
        };
        let source = room.session(Player::One);
        let plugged = source.cable_plugged();
        let consoles = std::array::from_fn(|i| Unit::default().decode_state(&states[i]).unwrap());
        let config = SessionConfig {
            check_interval: CHECK_EVERY,
            sync,
            start_frame: JOIN,
            cable_plugged: plugged,
            power_on: [0, 0, JOIN],
            ..SessionConfig::new(Player::Three, 7, 0)
        };
        let mut session = Session::with_consoles(config, consoles, [true; 3]);
        session.watch_remote(true);
        room.sessions[2] = Some(session);
        room.sent[2] = JOIN;
        for unit in 0..2 {
            for frame in JOIN..room.sent[unit] {
                room.session(Player::Three).add_player_input(
                    Player::ALL[unit],
                    frame,
                    pad(unit, frame),
                );
            }
        }
        room.run_until(400);
        let mut straight = Straight::new();
        straight.power_on[2] = JOIN;
        straight.join = Some((JOIN, 2));
        room.assert_matches(&straight, 400);
        for i in 0..3 {
            let session = room.session(Player::ALL[i]);
            let joined = session.console(Player::Three);
            assert!(joined.heard > 0);
            assert!(joined.frames >= 400 - JOIN - 40);
        }
    }

    #[test]
    fn a_late_joiner_links_in_and_stays_in_step_in_lockstep() {
        late_joiner(LOCKSTEP);
    }

    #[test]
    fn a_late_joiner_links_in_and_stays_in_step_with_rollback() {
        late_joiner(ROLLBACK);
    }

    #[test]
    fn a_member_change_in_a_confirmed_frame_is_refused() {
        let mut room = Room::<3>::new(LOCKSTEP, [true; 3], 1, 0);
        room.run_until(50);
        let session = room.session(Player::One);
        assert_eq!(session.leave(Player::Two, 10), Err(MemberError::Past(10)));
        assert_eq!(session.leave(Player::One, 60), Err(MemberError::Local(0)));
    }

    fn leave_behind_confirmed(sync: Sync) {
        let mut room = Room::<3>::new(sync, [true; 3], 1, 2);
        room.sessions = room.sessions.map(|s| s.map(|s| s.trailing_checks(8)));
        room.run_until(120);
        room.silent[2] = true;
        for _ in 0..80 {
            room.step();
        }
        let at = room.sent[2] - 5;
        for i in 0..2 {
            let session = room.session(Player::ALL[i]);
            assert!(session.confirmed_frame() > at);
            assert_eq!(session.leave(Player::Three, at), Err(MemberError::Past(at)));
            session.leave_from(Player::Three, at).unwrap();
        }
        room.run_until(400);
        let mut straight = Straight::new();
        straight.leave_at[2] = at;
        room.assert_matches(&straight, 400);
    }

    #[test]
    fn a_leave_behind_the_confirmed_frame_replays_in_lockstep() {
        leave_behind_confirmed(LOCKSTEP);
    }

    #[test]
    fn a_leave_behind_the_confirmed_frame_replays_with_rollback() {
        leave_behind_confirmed(ROLLBACK);
    }

    #[cfg(feature = "gba")]
    mod gba {
        use super::{CHECK_EVERY, LOCKSTEP, Lanes, Room, Straight, Units, pad};
        use crate::console::{Console, Rollback};
        use crate::gba::machine::Gba;
        use crate::gba::serial::{CABLE_WINDOW, LinkMessage};
        use crate::netplay::Sync;
        use crate::netplay::pair::{self, Cable};
        use crate::netplay::state;
        use crate::netplay::{LinkPort, Player, Session, SessionConfig};
        use crate::test_support::Step;
        use citrine_gb_threads::{Mode, Threads};

        const EWRAM: u32 = 0x0200_0000;
        const MISSING: u16 = 0xFFFF;
        const FRAMES: u32 = 240;
        const CHANGE: u32 = 120;

        fn program() -> Vec<u8> {
            [
                0xE3A0_0404,
                0xE280_2C01,
                0xE280_3C02,
                0xE3A0_4000,
                0xE1C2_43B4,
                0xE3A0_5A06,
                0xE385_5083,
                0xE3A0_8402,
                0xE3A0_9080,
                0xE1D2_13B0,
                0xE021_1004,
                0xE1C2_12BA,
                0xE1C3_90B2,
                0xE1C2_52B8,
                0xE1D3_60B2,
                0xE316_0080,
                0x0AFF_FFFC,
                0xE592_6020,
                0xE592_7024,
                0xE8A8_00C0,
                0xE3C8_8701,
                0xE284_4001,
                0xEAFF_FFF1u32,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect()
        }

        impl Units for Gba {
            fn units<const N: usize>() -> [Self; N] {
                std::array::from_fn(|_| Gba::new(&program()))
            }

            fn joiner() -> Self {
                Gba::new(&program())
            }
        }

        fn transfers(gba: &Gba) -> Vec<[u16; 4]> {
            let count = gba.cpu().reg(4);
            (0..count)
                .map(|i| {
                    let low = gba.bus().read32(EWRAM + 8 * i);
                    let high = gba.bus().read32(EWRAM + 8 * i + 4);
                    [
                        low as u16,
                        (low >> 16) as u16,
                        high as u16,
                        (high >> 16) as u16,
                    ]
                })
                .collect()
        }

        fn assert_same_transfers(logs: &[Vec<[u16; 4]>], linked: usize) {
            let longest = logs.iter().max_by_key(|log| log.len()).unwrap();
            assert!(longest.len() > 100, "{} transfers", longest.len());
            for log in logs {
                assert!(!log.is_empty());
                let fits = (0..=1).any(|behind| {
                    let end = longest.len() - behind;
                    end >= log.len() && longest[end - log.len()..end] == log[..]
                });
                assert!(fits, "a unit saw other words");
            }
            let last = longest.last().unwrap();
            for (id, word) in last.iter().enumerate() {
                assert_eq!(*word == MISSING, id >= linked, "word {id} of {last:04X?}");
            }
        }

        fn session_logs<const N: usize>(
            session: &Session<Gba, N>,
            units: &[usize],
        ) -> Vec<Vec<[u16; 4]>> {
            units
                .iter()
                .map(|&i| transfers(session.console(Player::ALL[i])))
                .collect()
        }

        fn linked<const N: usize>(sync: Sync, lag: u32, jitter: u32) {
            let mut room = Room::<N, Gba>::with_units(sync, [true; N], lag, jitter);
            room.run_until(FRAMES);
            if sync.budget == 0 {
                assert_eq!(room.rollbacks, 0);
            } else {
                assert!(room.rollbacks > 0);
                assert!(room.furthest <= sync.budget);
                assert!(room.deepest <= sync.budget);
            }
            room.assert_matches(&Straight::new(), FRAMES);
            let all: Vec<usize> = (0..N).collect();
            for i in 0..N {
                let session = room.session(Player::ALL[i]);
                assert!(session.cable_plugged());
                assert_same_transfers(&session_logs(session, &all), N);
            }
        }

        #[test]
        fn two_gbas_trade_multiplay_words_in_lockstep() {
            linked::<2>(LOCKSTEP, 1, 2);
        }

        #[test]
        fn three_gbas_trade_multiplay_words_in_lockstep() {
            linked::<3>(LOCKSTEP, 1, 2);
        }

        #[test]
        fn four_gbas_trade_multiplay_words_in_lockstep() {
            linked::<4>(LOCKSTEP, 1, 2);
        }

        #[test]
        fn two_gbas_trade_multiplay_words_with_rollback_inside_the_budget() {
            linked::<2>(Sync::new(4, 0), 6, 3);
        }

        #[test]
        fn four_gbas_trade_multiplay_words_with_rollback_inside_the_budget() {
            linked::<4>(Sync::new(2, 1), 4, 2);
        }

        fn exact<const N: usize>(sync: Sync, lag: u32, jitter: u32) {
            let mut room = Room::<N, Gba>::windowed(sync, [true; N], lag, jitter, 0);
            room.run_until(FRAMES);
            let mut straight = Straight::new();
            straight.window = 0;
            room.assert_matches(&straight, FRAMES);
            let all: Vec<usize> = (0..N).collect();
            for i in 0..N {
                assert!(room.windows[i].iter().all(|&(_, windows)| !windows));
                assert_same_transfers(&session_logs(room.session(Player::ALL[i]), &all), N);
            }
        }

        #[test]
        fn four_gbas_trade_multiplay_words_exactly_in_lockstep() {
            exact::<4>(LOCKSTEP, 1, 2);
        }

        #[test]
        fn four_gbas_trade_multiplay_words_exactly_with_rollback_inside_the_budget() {
            exact::<4>(Sync::new(2, 1), 4, 2);
        }

        #[test]
        fn a_group_of_four_runs_in_windows_and_a_pair_runs_exactly() {
            let mut four = Room::<4, Gba>::with_units(LOCKSTEP, [true; 4], 1, 2);
            four.lanes = [Lanes::Inline; 4];
            four.run_until(FRAMES);
            assert!(
                four.windows[0]
                    .iter()
                    .filter(|&&(_, windows)| windows)
                    .count()
                    > 200
            );
            let mut two = Room::<2, Gba>::with_units(LOCKSTEP, [true; 2], 1, 2);
            two.run_until(FRAMES);
            assert!(two.windows[0].iter().all(|&(_, windows)| !windows));
        }

        const STARVED_UNTIL: u32 = 150;
        const PACED_FRAMES: u32 = 360;

        #[test]
        fn late_lanes_on_one_peer_take_the_group_to_exact_and_back_on_the_same_frames() {
            let mut room = Room::<4, Gba>::with_units(Sync::new(2, 1), [true; 4], 4, 2);
            room.lanes = [Lanes::Starved, Lanes::Inline, Lanes::Inline, Lanes::Inline];
            room.run_until(STARVED_UNTIL);
            room.lanes[0] = Lanes::Inline;
            room.run_until(PACED_FRAMES);
            room.assert_matches(&Straight::new(), PACED_FRAMES);
            let (_, windows) = Straight::<4, Gba>::new().run(PACED_FRAMES, &room.paces);
            let plugged = windows.iter().position(|&on| on).unwrap();
            let switches: Vec<usize> = (plugged + 1..windows.len())
                .filter(|&f| windows[f - 1] != windows[f])
                .collect();
            assert_eq!(switches.len(), 2, "switches at {switches:?}");
            assert!(switches[0] < 60, "exact from {}", switches[0]);
            assert!(
                switches[1] > STARVED_UNTIL as usize,
                "windows again at {}",
                switches[1]
            );
            let asked: Vec<_> = room
                .paces
                .iter()
                .map(|&(_, unit, pace)| (unit, pace))
                .collect();
            assert_eq!(
                asked,
                [
                    (0, crate::netplay::CablePace::Exact),
                    (0, crate::netplay::CablePace::Windows)
                ]
            );
        }

        fn windowed_hashes(threads: &Threads) -> Vec<[u64; 4]> {
            let mut consoles = Gba::units::<4>();
            let mut cable = Cable::<LinkMessage, 4> {
                plugged: true,
                window: u64::from(CABLE_WINDOW),
                ..Cable::default()
            };
            let mut scratch = Gba::joiner();
            let mut hashes = Vec::new();
            for frame in 0..FRAMES / 2 {
                let inputs = std::array::from_fn(|i| pad(i, frame));
                threads.split(|split| {
                    pair::run_frame_on(&mut consoles, &mut cable, &inputs, [true; 4], split)
                });
                hashes.push(state::hash_consoles(&mut scratch, &consoles, &cable));
            }
            let longest = consoles.iter().map(|gba| transfers(gba).len()).max();
            assert!(longest.unwrap_or(0) > 50, "{longest:?} transfers");
            hashes
        }

        #[test]
        fn a_windowed_group_of_four_hashes_the_same_inline_threaded_and_shuffled() {
            let inline = windowed_hashes(&Threads::new(Mode::Inline));
            assert_eq!(windowed_hashes(&Threads::new(Mode::Threads)), inline);
            for seed in [1, 77, 0x5EED] {
                assert_eq!(windowed_hashes(&Threads::new(Mode::Shuffled(seed))), inline);
            }
        }

        #[test]
        fn a_gba_that_leaves_at_a_frame_unplugs_and_the_others_trade_on() {
            let mut room = Room::<4, Gba>::with_units(LOCKSTEP, [true; 4], 1, 2);
            room.run_until(CHANGE / 2);
            for i in 0..3 {
                room.session(Player::ALL[i])
                    .leave(Player::Four, CHANGE)
                    .unwrap();
            }
            room.run_until(CHANGE);
            room.silent[3] = true;
            room.run_until(FRAMES);
            let mut straight = Straight::new();
            straight.leave_at[3] = CHANGE;
            room.assert_matches(&straight, FRAMES);
            for i in 0..3 {
                let session = room.session(Player::ALL[i]);
                assert_same_transfers(&session_logs(session, &[0, 1, 2]), 3);
            }
        }

        #[test]
        fn a_gba_that_joins_at_a_frame_links_in_from_a_state() {
            let mut room = Room::<4, Gba>::with_units(LOCKSTEP, [true, true, true, false], 1, 2);
            room.lanes = [Lanes::Inline; 4];
            room.run_until(CHANGE / 2);
            for i in 0..3 {
                room.session(Player::ALL[i])
                    .join(Player::Four, CHANGE, Gba::joiner())
                    .unwrap();
            }
            room.run_until(CHANGE);
            let states = loop {
                let source = room.session(Player::One);
                let states: Option<Vec<Vec<u8>>> = (0..4)
                    .map(|i| {
                        let player = Player::ALL[i];
                        if let Some(state) = source.checkpoint(CHANGE, player) {
                            return Some(state);
                        }
                        let here = source.frame() == CHANGE && source.confirmed_frame() == CHANGE;
                        let console = if i == 3 {
                            Gba::joiner()
                        } else {
                            source.console(player).clone_state()
                        };
                        here.then(|| crate::netplay::encode_state(&console).unwrap())
                    })
                    .collect();
                if let Some(states) = states {
                    break states;
                }
                assert!(room.tick < 1_000, "no state at frame {CHANGE}");
                room.step();
            };
            let template = Gba::joiner();
            let consoles = std::array::from_fn(|i| template.decode_state(&states[i]).unwrap());
            let config = SessionConfig {
                check_interval: CHECK_EVERY,
                sync: LOCKSTEP,
                start_frame: CHANGE,
                cable_plugged: room.session(Player::One).cable_plugged(),
                power_on: [0, 0, 0, CHANGE],
                ..SessionConfig::new(Player::Four, 7, 0)
            };
            let mut session = Session::with_consoles(config, consoles, [true; 4]);
            session.watch_remote(true);
            room.sessions[3] = Some(session);
            room.sent[3] = CHANGE;
            for unit in 0..3 {
                for frame in CHANGE..room.sent[unit] {
                    room.session(Player::Four).add_player_input(
                        Player::ALL[unit],
                        frame,
                        pad(unit, frame),
                    );
                }
            }
            room.run_until(FRAMES);
            let mut straight = Straight::new();
            straight.power_on[3] = CHANGE;
            straight.join = Some((CHANGE, 3));
            room.assert_matches(&straight, FRAMES);
            for i in 0..4 {
                let session = room.session(Player::ALL[i]);
                assert_same_transfers(&session_logs(session, &[0, 1, 2, 3]), 4);
            }
        }

        fn watcher_checks(
            room: &mut Room<4, Gba>,
            at: u32,
            states: &[Vec<u8>],
            cable: Option<&[u8]>,
        ) -> Vec<crate::netplay::Check<4>> {
            let template = Gba::joiner();
            let consoles = std::array::from_fn(|i| match states.get(i) {
                Some(state) => template.decode_state(state).unwrap(),
                None => Gba::joiner(),
            });
            let config = SessionConfig {
                check_interval: CHECK_EVERY,
                sync: LOCKSTEP,
                start_frame: at,
                ..SessionConfig::new(Player::Four, 7, 0)
            };
            let mut watcher = Session::with_consoles(config, consoles, [true, true, true, false]);
            if let Some(cable) = cable {
                watcher.restore_cable(cable).unwrap();
            }
            watcher.watch_remote(true);
            room.run_until(FRAMES);
            let mut checks = Vec::new();
            for frame in at..FRAMES {
                for unit in 0..3 {
                    watcher.add_player_input(Player::ALL[unit], frame, pad(unit, frame));
                }
                while watcher.step() == crate::netplay::Advance::Ran {}
                while let Some(check) = watcher.take_check() {
                    checks.push(check);
                }
            }
            checks
        }

        #[test]
        fn a_watcher_follows_a_running_group_from_a_checkpoint_and_its_cable() {
            let mut room = Room::<4, Gba>::with_units(LOCKSTEP, [true, true, true, false], 1, 2);
            room.run_until(CHANGE);
            let source = room.session(Player::One);
            let at = source.latest_checkpoint().unwrap();
            let cable = source.checkpoint_cable(at).unwrap();
            let states: Vec<Vec<u8>> = (0..3)
                .map(|i| source.checkpoint(at, Player::ALL[i]).unwrap())
                .collect();
            let compare = |room: &Room<4, Gba>, checks: &[crate::netplay::Check<4>]| {
                checks
                    .iter()
                    .filter_map(|check| {
                        let theirs = room.checks.iter().find(|c| c.frame == check.frame)?;
                        Some(theirs.hashes[..3] == check.hashes[..3])
                    })
                    .collect::<Vec<bool>>()
            };
            let followed = watcher_checks(&mut room, at, &states, Some(&cable));
            let matched = compare(&room, &followed);
            assert!(matched.len() >= ((FRAMES - at) / CHECK_EVERY - 1) as usize);
            assert!(matched.iter().all(|same| *same));
            let blind = watcher_checks(&mut room, at, &states, None);
            assert!(compare(&room, &blind).iter().any(|same| !same));
        }

        fn linked_pair(frames: u32) -> [Gba; 2] {
            let mut consoles = Gba::units::<2>();
            let mut cable = Cable::default();
            for frame in 0..frames {
                let inputs = [pad(0, frame), pad(1, frame)];
                pair::run_frame(&mut consoles, &mut cable, &inputs, [true; 2]);
            }
            consoles
        }

        #[test]
        fn a_gba_state_round_trip_keeps_the_hash_and_the_run() {
            let [mut one, _] = linked_pair(30);
            one.unplug_link();
            let mut scratch = Gba::joiner();
            let mut two = Gba::joiner()
                .decode_state(&one.encode_state().unwrap())
                .unwrap();
            assert_eq!(two.state_hash(&mut scratch), one.state_hash(&mut scratch));
            let mut three = Gba::joiner();
            three.copy_state_from(&one);
            for frame in 0..20 {
                for gba in [&mut one, &mut two, &mut three] {
                    gba.set_keys(pad(0, frame).buttons);
                    gba.run_frame();
                }
            }
            let hash = one.state_hash(&mut scratch);
            assert_eq!(two.state_hash(&mut scratch), hash);
            assert_eq!(three.state_hash(&mut scratch), hash);
            assert_eq!(two.frame(), one.frame());
            assert_eq!(three.frame(), one.frame());
        }

        #[test]
        fn a_gba_state_leaves_the_game_out() {
            let mut rom = program();
            rom.resize(4 << 20, 0);
            let gba = Gba::new(&rom);
            let state = gba.encode_state().unwrap();
            assert!(state.len() < 1 << 20, "{} bytes", state.len());
            let back = Gba::new(&rom).decode_state(&state).unwrap();
            assert_eq!(back.bus().rom().len(), rom.len());
        }

        #[test]
        fn a_blind_and_muted_gba_hashes_like_a_seen_one() {
            let mut seen = Gba::joiner();
            let mut blind = Gba::joiner();
            blind.set_blind(true);
            blind.set_muted(true);
            let mut audio = Vec::new();
            for frame in 0..10 {
                for gba in [&mut seen, &mut blind] {
                    gba.set_keys(pad(1, frame).buttons);
                    gba.run_frame();
                }
                blind.take_audio(&mut audio);
            }
            let mut scratch = Gba::joiner();
            assert_eq!(
                blind.state_hash(&mut scratch),
                seen.state_hash(&mut scratch)
            );
            assert!(audio.is_empty());
        }

        #[cfg(feature = "brotli")]
        mod real_game {
            use super::pad;
            use crate::console::ConsoleKind;
            use crate::gba::machine::Gba;
            use crate::netplay::{Advance, Check, ConsoleSetup, Player, Session, SessionConfig};
            use crate::rom::Rom;
            use crate::test_support::Step;

            const CASCADE: &[u8] =
                include_bytes!("../../citrine-gb-app/homebrew/gba/cascade7.gba.br");
            const FRAMES: u32 = 240;
            const LATE: u32 = 5;

            fn rom() -> Rom {
                let mut data = Vec::new();
                brotli::BrotliDecompress(&mut &CASCADE[..], &mut data).unwrap();
                Rom::new(&data)
            }

            fn session(rom: &Rom) -> Session<Gba> {
                let setup = || {
                    Some(ConsoleSetup {
                        rom,
                        kind: ConsoleKind::Gba,
                        save: None,
                        state: None,
                    })
                };
                let config = SessionConfig {
                    check_interval: 10,
                    ..SessionConfig::new(Player::One, 0x0C17_2026, 1_700_000_000)
                };
                let mut session = Session::new_gba(config, [setup(), setup()]).unwrap();
                session.watch_remote(true);
                session
            }

            fn play(late: u32) -> (Vec<Check>, u32) {
                let mut session = session(&rom());
                let mut checks = Vec::new();
                let mut rollbacks = 0;
                for frame in 0..FRAMES + late {
                    if frame < FRAMES {
                        session.add_local_input(frame, pad(0, frame)).unwrap();
                    }
                    if let Some(heard) = frame.checked_sub(late) {
                        session.add_remote_input(heard, pad(1, heard));
                    }
                    if frame < FRAMES {
                        assert_eq!(session.step(), Advance::Ran);
                    }
                    rollbacks += session.last_rollback().min(1);
                    checks.extend(std::iter::from_fn(|| session.take_check()));
                }
                checks.extend(std::iter::from_fn(|| session.take_check()));
                (checks, rollbacks)
            }

            #[test]
            fn a_gba_game_rolled_back_hashes_like_one_played_on_time() {
                let (on_time, none) = play(0);
                let (late, rollbacks) = play(LATE);
                assert_eq!(none, 0);
                assert!(rollbacks > 10, "{rollbacks} rollbacks");
                let shared: Vec<_> = late
                    .iter()
                    .filter_map(|check| {
                        on_time
                            .iter()
                            .find(|c| c.frame == check.frame)
                            .map(|c| (c, check))
                    })
                    .collect();
                assert!(shared.len() >= 20, "{} checks in common", shared.len());
                for (expected, got) in shared {
                    assert_eq!(got.hashes, expected.hashes, "frame {}", got.frame);
                }
            }

            #[test]
            fn the_gba_state_hash_for_a_fixed_seed_and_script_is_pinned() {
                let (checks, _) = play(LATE);
                let last = checks.iter().max_by_key(|check| check.frame).unwrap();
                assert_eq!(last.frame, 230);
                assert_eq!(
                    last.hashes,
                    [9_079_662_873_613_382_230, 16_208_275_486_166_875_730]
                );
            }
        }
    }

    #[cfg(all(feature = "nds", feature = "brotli"))]
    mod nds {
        use super::pad;
        use crate::console::{ConsoleKind, Rollback};
        use crate::nds::machine::{Nds, NdsFirmware};
        use crate::netplay::{
            Advance, Check, ConsoleSetup, NdsBoot, NdsSetup, Player, Session, SessionConfig,
        };
        use crate::rom::Rom;

        const PONG: &[u8] = include_bytes!("../../citrine-gb-app/homebrew/nds/pong_nds.nds.br");
        const FRAMES: u32 = 240;
        const LATE: u32 = 6;

        fn pong() -> Vec<u8> {
            let mut data = Vec::new();
            brotli::BrotliDecompress(&mut &PONG[..], &mut data).unwrap();
            data
        }

        fn played(nds: &mut Nds, frames: std::ops::Range<u32>) {
            for frame in frames {
                nds.set_keys(pad(0, frame).buttons);
                nds.run_frame();
                nds.take_audio(&mut Vec::new());
            }
        }

        fn hashed(nds: &Nds) -> (u64, Vec<u8>) {
            let mut scratch = nds.scratch();
            (nds.state_hash(&mut scratch), nds.encode_state().unwrap())
        }

        fn restores_like_full_clones(jit: bool) {
            let mut nds = Nds::new(pong().into(), NdsFirmware::default()).unwrap();
            nds.set_jit(jit);
            played(&mut nds, 0..90);
            let mut slots: Vec<Nds> = (0..3).map(|_| nds.snapshot()).collect();
            let mut fulls = Vec::new();
            for (index, slot) in slots.iter_mut().enumerate() {
                let start = 90 + index as u32 * 20;
                nds.snapshot_into(slot);
                fulls.push(nds.clone_state());
                played(&mut nds, start..start + 20);
            }
            for index in [1, 0, 2, 2, 0] {
                nds.copy_state_from(&slots[index]);
                assert_eq!(
                    hashed(&nds),
                    hashed(&fulls[index]),
                    "jit {jit}, slot {index}"
                );
                let start = 90 + index as u32 * 20;
                let mut full = fulls[index].clone_state();
                played(&mut nds, start..start + 15);
                played(&mut full, start..start + 15);
                assert_eq!(hashed(&nds), hashed(&full), "jit {jit}, after slot {index}");
                nds.snapshot_into(&mut slots[(index + 1) % 3]);
                fulls[(index + 1) % 3] = full;
            }
        }

        #[test]
        fn a_ds_restored_from_a_shared_snapshot_matches_a_full_clone() {
            restores_like_full_clones(false);
        }

        #[cfg(jit)]
        #[test]
        fn a_ds_restored_from_a_shared_snapshot_matches_a_full_clone_with_the_jit() {
            restores_like_full_clones(true);
        }

        #[test]
        fn a_ds_snapshot_slot_hashes_like_a_full_clone() {
            let mut nds = Nds::new(pong().into(), NdsFirmware::default()).unwrap();
            played(&mut nds, 0..90);
            let mut slot = nds.snapshot();
            played(&mut nds, 90..110);
            nds.snapshot_into(&mut slot);
            let full = nds.clone_state();
            played(&mut nds, 110..115);
            assert_eq!(hashed(&slot), hashed(&full));
        }

        fn session(rom: &Rom) -> Session<Nds> {
            let setup = || {
                Some(NdsSetup {
                    boot: NdsBoot::Card(ConsoleSetup {
                        rom,
                        kind: ConsoleKind::Nds,
                        save: None,
                        state: None,
                    }),
                    firmware: NdsFirmware::default(),
                })
            };
            let config = SessionConfig {
                check_interval: 10,
                ..SessionConfig::new(Player::One, 0x0D5_2026, 1_700_000_000)
            };
            let mut session = Session::new_nds(config, [setup(), setup()]).unwrap();
            session.watch_remote(true);
            session
        }

        fn play(late: u32) -> (Vec<Check>, u32) {
            let mut session = session(&Rom::new(&pong()));
            let mut checks = Vec::new();
            let mut rollbacks = 0;
            for frame in 0..FRAMES + late {
                if frame < FRAMES {
                    session.add_local_input(frame, pad(0, frame)).unwrap();
                }
                if let Some(heard) = frame.checked_sub(late) {
                    session.add_remote_input(heard, pad(1, heard));
                }
                if frame < FRAMES {
                    assert_eq!(session.advance(), Advance::Ran);
                }
                rollbacks += session.last_rollback().min(1);
                checks.extend(std::iter::from_fn(|| session.take_check()));
            }
            checks.extend(std::iter::from_fn(|| session.take_check()));
            (checks, rollbacks)
        }

        #[test]
        fn a_ds_game_rolled_back_hashes_like_one_played_on_time() {
            let (on_time, none) = play(0);
            let (late, rollbacks) = play(LATE);
            assert_eq!(none, 0);
            assert!(rollbacks > 10, "{rollbacks} rollbacks");
            let shared: Vec<_> = late
                .iter()
                .filter_map(|check| {
                    on_time
                        .iter()
                        .find(|c| c.frame == check.frame)
                        .map(|c| (c, check))
                })
                .collect();
            assert!(shared.len() >= 20, "{} checks in common", shared.len());
            for (expected, got) in shared {
                assert_eq!(got.hashes, expected.hashes, "frame {}", got.frame);
            }
        }

        #[test]
        fn the_ds_state_hash_for_a_fixed_seed_and_script_is_pinned() {
            let (checks, _) = play(LATE);
            let last = checks.iter().max_by_key(|check| check.frame).unwrap();
            assert_eq!(last.frame, 230);
            assert_eq!(last.hashes, [7076099957967274382, 11928561717309766812]);
        }
    }
}
