mod input;
mod local;
pub(crate) mod pair;
mod peer;
pub(crate) mod state;

pub use input::{CableEvent, Input, Player};
pub use local::LocalCable;
pub use peer::{LinkPeer, PeerCable};

use crate::error::{GbError, GbResult};
use crate::gb::ppu::types::color::{ColorCorrection, DmgColors};
use crate::gb::ppu::types::theme::DmgTheme;
use crate::gb::ram_init::RamInit;
use crate::gb::{GameBoy, GbModel};
use crate::persistence::rtc_footer::RTC_FOOTER_LEN;
use crate::persistence::sram_dump::SramDump;
use crate::rom::Rom;
use input::{InputLog, LOG_LEN};
use pair::Cable;
use state::Snapshot;
use std::collections::VecDeque;

pub const MAX_ROLLBACK_FRAMES: u32 = 30;
pub const DEFAULT_CHECK_INTERVAL: u32 = 60;
pub const NEVER: u32 = u32::MAX;
const RING_LEN: usize = MAX_ROLLBACK_FRAMES as usize;
const LOCAL_AHEAD: u32 = 128;
const REMOTE_AHEAD: u32 = 256;
const KEPT_CHECKS: usize = 8;
const CYCLES_PER_FRAME: u64 = 70_224;
const CYCLES_PER_SECOND: u64 = 4_194_304;

#[derive(Debug, Clone, Copy)]
pub struct SessionConfig {
    pub local: Player,
    pub seed: u64,
    pub start_time: u64,
    pub check_interval: u32,
    pub sample_rate: u32,
    pub power_on: [u32; 2],
}

impl SessionConfig {
    pub fn new(local: Player, seed: u64, start_time: u64) -> Self {
        Self {
            local,
            seed,
            start_time,
            check_interval: DEFAULT_CHECK_INTERVAL,
            sample_rate: 44_100,
            power_on: [0; 2],
        }
    }
}

pub struct ConsoleSetup<'a> {
    pub rom: &'a Rom,
    pub model: GbModel,
    pub save: Option<SramDump>,
    pub state: Option<&'a [u8]>,
}

pub fn encode_state(gb: &GameBoy) -> GbResult<Vec<u8>> {
    state::encode_console(gb)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advance {
    Ran,
    AwaitingLocal,
    AwaitingRemote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Check {
    pub frame: u32,
    pub hashes: [u64; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Desync {
    pub frame: u32,
    pub consoles: [bool; 2],
}

impl Desync {
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

pub struct Session {
    config: SessionConfig,
    frame: u32,
    consoles: Box<[GameBoy; 2]>,
    cable: Cable,
    inputs: [InputLog; 2],
    used: Box<[Input]>,
    ring: Box<[Snapshot]>,
    rollback_from: Option<u32>,
    last_rollback: u32,
    next_check: u32,
    outbox: VecDeque<Check>,
    local_checks: VecDeque<Check>,
    remote_checks: VecDeque<Check>,
    compared: Option<u32>,
    desync: Option<Desync>,
    checkpoints: Box<[Snapshot; 2]>,
    scratch: Box<GameBoy>,
    saved: Option<Vec<u8>>,
    watching: bool,
    present: [bool; 2],
}

impl Session {
    pub fn new(config: SessionConfig, setups: [Option<ConsoleSetup>; 2]) -> GbResult<Self> {
        let present = [setups[0].is_some(), setups[1].is_some()];
        let mut config = SessionConfig {
            check_interval: config.check_interval.max(1),
            ..config
        };
        for (at, here) in config.power_on.iter_mut().zip(present) {
            if !here {
                *at = NEVER;
            }
        }
        let [one, two] = setups;
        let console = |setup: Option<ConsoleSetup>, player| match setup {
            Some(setup) => new_console(setup, &config, player),
            None => Ok(GameBoy::new_empty(GbModel::Dmg)),
        };
        let mut consoles = [console(one, Player::One)?, console(two, Player::Two)?];
        for (gb, here) in consoles.iter_mut().zip(present) {
            if here {
                gb.apu.set_sample_rate(config.sample_rate);
            }
        }
        let mut session = Self {
            config,
            frame: 0,
            cable: Cable::default(),
            inputs: [InputLog::new(), InputLog::new()],
            used: vec![Input::default(); LOG_LEN].into_boxed_slice(),
            ring: (0..RING_LEN).map(|_| Snapshot::new(&consoles)).collect(),
            rollback_from: None,
            last_rollback: 0,
            next_check: 0,
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
        Ok(session)
    }

    pub fn frame(&self) -> u32 {
        self.frame
    }

    pub fn local(&self) -> Player {
        self.config.local
    }

    pub fn console(&self, player: Player) -> &GameBoy {
        &self.consoles[player.index()]
    }

    pub fn into_console(self, player: Player) -> GameBoy {
        let mut consoles = (self.consoles as Box<[GameBoy]>).into_vec();
        consoles.swap_remove(player.index())
    }

    pub fn present(&self, player: Player) -> bool {
        self.present[player.index()]
    }

    pub fn powered(&self, player: Player) -> bool {
        self.frame >= self.config.power_on[player.index()]
    }

    pub fn cable_plugged(&self) -> bool {
        self.cable.plugged
    }

    pub fn confirmed_frame(&self) -> u32 {
        self.remote_frontier().min(self.frame)
    }

    pub fn frames_ahead(&self) -> u32 {
        self.frame - self.confirmed_frame()
    }

    pub fn remote_frame(&self) -> u32 {
        self.remote_log().latest().map_or(0, |frame| frame + 1)
    }

    pub fn last_rollback(&self) -> u32 {
        self.last_rollback
    }

    pub fn desync(&self) -> Option<Desync> {
        self.desync
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

    pub fn watch_remote(&mut self, on: bool) {
        self.watching = on;
        self.set_catch_up(false);
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
        if !self.present[self.config.local.other().index()] {
            return;
        }
        let log = &mut self.inputs[self.config.local.other().index()];
        if !log.accepts(frame, REMOTE_AHEAD) || log.get(frame).is_some() {
            return;
        }
        log.set(frame, input);
        if frame < self.frame && self.used[frame as usize % LOG_LEN] != input {
            self.rollback_from = Some(self.rollback_from.map_or(frame, |from| from.min(frame)));
        }
    }

    pub fn advance(&mut self) -> Advance {
        self.last_rollback = 0;
        self.settle();
        let local = self.config.local.index();
        if self.present[local] && self.inputs[local].get(self.frame).is_none() {
            return Advance::AwaitingLocal;
        }
        let waiting = if self.present[local] {
            self.frames_ahead() >= MAX_ROLLBACK_FRAMES
        } else {
            self.remote_frontier() <= self.frame
        };
        if waiting {
            return Advance::AwaitingRemote;
        }
        self.run(self.frame);
        self.frame += 1;
        Advance::Ran
    }

    pub fn drain_audio(&mut self, out: &mut Vec<f32>) {
        let heard = self.heard();
        out.append(&mut self.consoles[heard].apu.audio_buffer);
    }

    pub fn drain_remote_audio(&mut self, out: &mut Vec<f32>) {
        let remote = self.config.local.other().index();
        if self.present[remote] && self.heard() != remote {
            out.append(&mut self.consoles[remote].apu.audio_buffer);
        }
    }

    pub fn take_check(&mut self) -> Option<Check> {
        self.outbox.pop_front()
    }

    pub fn add_remote_check(&mut self, check: Check) {
        keep(&mut self.remote_checks, check);
        self.compare(check.frame);
    }

    pub fn checkpoint(&self, frame: u32, player: Player) -> Option<Vec<u8>> {
        let checkpoint = self.checkpoints.iter().find(|c| c.frame == Some(frame))?;
        state::encode_console(&checkpoint.consoles[player.index()]).ok()
    }

    pub fn resync(&mut self, frame: u32, player: Player, data: &[u8]) -> Result<(), ResyncError> {
        let index = self
            .checkpoints
            .iter()
            .position(|c| c.frame == Some(frame))
            .filter(|_| self.frame - frame < LOG_LEN as u32 - REMOTE_AHEAD)
            .ok_or(ResyncError::NoCheckpoint(frame))?;
        let checkpoint = &mut self.checkpoints[index];
        let gb = state::decode_console(data, &checkpoint.consoles[player.index()])?;
        checkpoint.consoles[player.index()].copy_state_from(&gb);
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
        let now = self.config.start_time + frame as u64 * CYCLES_PER_FRAME / CYCLES_PER_SECOND;
        let local = self.config.local.index();
        let gb = if frame == self.frame {
            &mut self.consoles[local]
        } else {
            let slot = &mut self.ring[frame as usize % RING_LEN];
            if slot.frame != Some(frame) {
                return None;
            }
            &mut slot.consoles[local]
        };
        let dump = gb.poll_sram_dump(true, now)?;
        let data = dump.as_slice();
        let body = if gb.cartridge.rtc().is_some() {
            &data[..data.len().saturating_sub(RTC_FOOTER_LEN)]
        } else {
            data
        };
        if !force && self.saved.as_deref() == Some(body) {
            return None;
        }
        self.saved = Some(body.to_vec());
        Some(dump)
    }

    fn remote_log(&self) -> &InputLog {
        &self.inputs[self.config.local.other().index()]
    }

    fn remote_frontier(&self) -> u32 {
        if self.present[self.config.local.other().index()] {
            self.remote_log().frontier()
        } else {
            NEVER
        }
    }

    fn heard(&self) -> usize {
        heard_index(&self.config, self.present)
    }

    fn settle(&mut self) {
        if let Some(from) = self.rollback_from.take() {
            self.emit_checks(from);
            let slot = &self.ring[from as usize % RING_LEN];
            if slot.frame == Some(from) && !self.replay_remote(from) {
                let slot = &self.ring[from as usize % RING_LEN];
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
        for (i, gb) in self.consoles.iter_mut().enumerate() {
            let hidden = i != local && !self.watching;
            gb.ppu.blind = on || hidden;
            gb.apu.muted = on || hidden || (i == local && i != heard);
        }
    }

    fn mute_all(&mut self) {
        for gb in self.consoles.iter_mut() {
            gb.apu.muted = true;
        }
    }

    fn run(&mut self, frame: u32) {
        let frontier = self.remote_frontier();
        if frame >= frontier {
            self.ring[frame as usize % RING_LEN].save(frame, &self.consoles, &self.cable);
        }
        if frame == self.next_check && frontier >= frame {
            let hashes = state::hash_consoles(&mut self.scratch, &self.consoles, &self.cable);
            let index = self.checkpoint_index(frame);
            self.checkpoints[index].save(frame, &self.consoles, &self.cable);
            self.record_check(Check { frame, hashes });
        }

        let inputs = self.inputs_for(frame);
        let on = self.config.power_on.map(|at| frame >= at);
        pair::run_frame(&mut self.consoles, &mut self.cable, &inputs, on);
    }

    fn inputs_for(&mut self, frame: u32) -> [Input; 2] {
        let local = self.config.local.index();
        let remote = self.config.local.other().index();
        let mut inputs = [self.inputs[local].get(frame).unwrap_or_default(); 2];
        inputs[remote] = self.inputs[remote]
            .get(frame)
            .unwrap_or_else(|| self.inputs[remote].predict(frame));
        self.used[frame as usize % LOG_LEN] = inputs[remote];
        inputs
    }

    fn quiet_since(&self, from: u32) -> bool {
        let local = self.config.local.index();
        let slot = &self.ring[from as usize % RING_LEN];
        slot.cable.messages == self.cable.messages
            && slot.cable.stalls[local] == self.cable.stalls[local]
            && slot.cable.in_flight.iter().all(VecDeque::is_empty)
            && self
                .config
                .power_on
                .iter()
                .all(|&at| at < from || at >= self.frame)
            && (from..self.frame).all(|frame| {
                self.ring[frame as usize % RING_LEN].frame == Some(frame)
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
        let remote = self.config.local.other().index();
        let slot = &self.ring[from as usize % RING_LEN];
        self.consoles[remote].copy_state_from(&slot.consoles[remote]);
        self.cable.clocks[remote] = slot.cable.clocks[remote];
        self.set_catch_up(true);
        for frame in from..self.frame {
            let index = frame as usize % RING_LEN;
            let slot = &mut self.ring[index];
            slot.consoles[remote].copy_state_from(&self.consoles[remote]);
            slot.cable.clocks[remote] = self.cable.clocks[remote];
            if frame == self.next_check && self.remote_frontier() >= frame {
                let slot = &self.ring[index];
                let hashes = state::hash_consoles(&mut self.scratch, &slot.consoles, &slot.cable);
                let checkpoint = self.checkpoint_index(frame);
                self.checkpoints[checkpoint].save(frame, &slot.consoles, &slot.cable);
                self.record_check(Check { frame, hashes });
            }
            if frame + 1 == self.frame {
                self.set_catch_up(false);
                self.mute_all();
            }
            let input = self.inputs_for(frame)[remote];
            let on = frame >= self.config.power_on[remote];
            if !pair::run_solo(&mut self.consoles, &mut self.cable, remote, input, on) {
                self.set_catch_up(false);
                return false;
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
            let slot = &self.ring[frame as usize % RING_LEN];
            if slot.frame != Some(frame) {
                self.next_check += self.config.check_interval;
                continue;
            }
            let hashes = state::hash_consoles(&mut self.scratch, &slot.consoles, &slot.cable);
            let index = self.checkpoint_index(frame);
            self.checkpoints[index].save(frame, &slot.consoles, &slot.cable);
            self.record_check(Check { frame, hashes });
        }
    }

    fn checkpoint_index(&self, frame: u32) -> usize {
        (frame / self.config.check_interval) as usize % 2
    }

    fn record_check(&mut self, check: Check) {
        self.next_check = check.frame + self.config.check_interval;
        self.outbox.push_back(check);
        keep(&mut self.local_checks, check);
        self.compare(check.frame);
    }

    fn compare(&mut self, frame: u32) {
        if self.compared.is_some_and(|compared| compared > frame) {
            return;
        }
        let find = |checks: &VecDeque<Check>| checks.iter().find(|c| c.frame == frame).copied();
        let (Some(local), Some(remote)) = (find(&self.local_checks), find(&self.remote_checks))
        else {
            return;
        };
        let consoles = [0, 1].map(|i| local.hashes[i] != remote.hashes[i]);
        self.compared = Some(frame);
        self.desync = consoles
            .contains(&true)
            .then_some(Desync { frame, consoles });
    }

    #[cfg(test)]
    pub(crate) fn link_messages(&self) -> ([u64; 2], [u64; 2]) {
        (self.cable.messages, self.cable.transfers)
    }

    #[cfg(test)]
    pub(crate) fn rewind_to(&mut self, frame: u32) {
        let from = frame.max(self.remote_frontier());
        if from < self.frame {
            self.rollback_from = Some(self.rollback_from.map_or(from, |at| at.min(from)));
        }
    }

    #[cfg(test)]
    pub(crate) fn corrupt(&mut self, player: Player, change: impl Fn(&mut GameBoy)) {
        change(&mut self.consoles[player.index()]);
        for slot in self.ring.iter_mut() {
            change(&mut slot.consoles[player.index()]);
        }
    }
}

fn heard_index(config: &SessionConfig, present: [bool; 2]) -> usize {
    let local = config.local.index();
    if present[local] { local } else { 1 - local }
}

fn keep(checks: &mut VecDeque<Check>, check: Check) {
    checks.retain(|c| c.frame != check.frame);
    checks.push_back(check);
    while checks.len() > KEPT_CHECKS {
        checks.pop_front();
    }
}

pub(crate) fn new_console(
    setup: ConsoleSetup,
    config: &SessionConfig,
    player: Player,
) -> GbResult<GameBoy> {
    let seed = mix(config.seed ^ player.index() as u64);
    let mut gb = GameBoy::new_empty_with_ram_init(setup.model, RamInit::Random { seed });
    gb.load_rom(setup.rom)?;
    if let Some(data) = setup.state {
        return state::decode_console(data, &gb);
    }
    if let Some(save) = setup.save {
        gb.put_sram_dump(save, config.start_time);
    }
    Ok(gb)
}

fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
