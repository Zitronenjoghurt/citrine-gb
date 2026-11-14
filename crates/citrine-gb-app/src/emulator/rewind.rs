mod delta;

use super::Emulator;
use citrine_gb::console::{Console, ConsoleKind};
use citrine_gb::error::GbResult;
use citrine_gb::machine::Machine;
use std::collections::VecDeque;
use std::fmt::Display;
use strum_macros::EnumIter;

const MIB: usize = 1024 * 1024;

pub const REWIND_NOTE: &str = "Hold the rewind key or button to play the game backwards. Rewind is off while you play together or with a link.";

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum RewindLength {
    Off,
    TenSeconds,
    #[default]
    ThirtySeconds,
    OneMinute,
    TwoMinutes,
}

impl RewindLength {
    pub fn seconds(self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::TenSeconds => Some(10),
            Self::ThirtySeconds => Some(30),
            Self::OneMinute => Some(60),
            Self::TwoMinutes => Some(120),
        }
    }
}

impl Display for RewindLength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.seconds() {
            None => f.write_str("Off"),
            Some(seconds) if seconds < 60 => write!(f, "{seconds} s"),
            Some(seconds) => write!(f, "{} min", seconds / 60),
        }
    }
}

pub(crate) fn every(kind: ConsoleKind) -> u32 {
    match kind {
        ConsoleKind::Gb | ConsoleKind::Gbc => 1,
        ConsoleKind::Gba => 2,
        ConsoleKind::Nds => 4,
    }
}

const SMALL_DEVICE: usize = if cfg!(any(target_arch = "wasm32", target_os = "android")) {
    2
} else {
    1
};

pub(crate) fn cap(kind: ConsoleKind) -> usize {
    let cap = match kind {
        ConsoleKind::Gb | ConsoleKind::Gbc => 64 * MIB,
        ConsoleKind::Gba => 128 * MIB,
        ConsoleKind::Nds => 256 * MIB,
    };
    cap / SMALL_DEVICE
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RewindPlan {
    pub(crate) keep: Option<u32>,
    pub(crate) back: bool,
}

impl RewindPlan {
    pub(crate) const OFF: Self = Self {
        keep: None,
        back: false,
    };
}

impl Emulator {
    pub fn can_rewind(&self) -> bool {
        let facts = self.seat.facts();
        #[cfg(feature = "nds")]
        if facts.walker {
            return false;
        }
        #[cfg(ds_wifi)]
        if facts.wifi {
            return false;
        }
        self.rewind_length.seconds().is_some()
            && facts.has_game
            && !facts.linked
            && !facts.partnered
            && !self.together
            && self.replay.is_none()
            && !self.recorder.is_active()
    }

    pub fn is_rewinding(&self) -> bool {
        self.rewinding
    }

    pub(super) fn rewind_plan(&self) -> RewindPlan {
        if !self.can_rewind() {
            return RewindPlan::OFF;
        }
        RewindPlan {
            keep: self.rewind_length.seconds(),
            back: self.rewind_held,
        }
    }
}

struct Step {
    packed: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct Rewind {
    newest: Vec<u8>,
    parts: Vec<delta::Part>,
    steps: VecDeque<Step>,
    packed_bytes: usize,
    since: u32,
    every: u32,
    keep: usize,
    cap: usize,
    scratch: Vec<u8>,
    fresh: Vec<u8>,
    spare: Vec<u8>,
    #[cfg(feature = "nds")]
    copy: Option<Box<citrine_gb::nds::machine::StateCopy>>,
    #[cfg(feature = "nds")]
    copied: bool,
}

impl Rewind {
    pub(crate) fn clear(&mut self) {
        self.newest.clear();
        self.fresh.clear();
        self.parts.clear();
        self.steps.clear();
        self.packed_bytes = 0;
        self.since = 0;
        #[cfg(feature = "nds")]
        {
            self.copy = None;
            self.copied = false;
        }
    }

    pub(crate) fn follow(&mut self, kind: ConsoleKind, seconds: u32, fps: f64) {
        self.every = every(kind);
        self.cap = cap(kind);
        self.keep = (seconds as f64 * fps / self.every as f64).ceil() as usize;
        self.trim();
    }

    #[cfg(all(test, feature = "nds", not(target_arch = "wasm32")))]
    pub(crate) fn kept(&self) -> (usize, usize, usize) {
        (self.newest.len(), self.steps.len(), self.packed_bytes)
    }

    pub(crate) fn bytes(&self) -> usize {
        self.newest.len() + self.packed_bytes
    }

    pub(crate) fn captures_next(&self) -> bool {
        (self.newest.is_empty() && self.is_settled()) || self.since + 1 >= self.every
    }

    pub(crate) fn after_frame(&mut self, machine: &mut Machine, beside: bool) {
        let due = self.captures_next();
        self.since += 1;
        if due && let Err(err) = self.capture(machine, beside) {
            log::error!("could not keep a rewind state: {err:?}");
            self.clear();
        }
    }

    pub(crate) fn is_settled(&self) -> bool {
        #[cfg(feature = "nds")]
        if self.copied {
            return false;
        }
        self.fresh.is_empty()
    }

    fn capture(&mut self, machine: &mut Machine, beside: bool) -> GbResult<()> {
        crate::profile::scope!("rewind capture");
        self.settle();
        machine.finish_picture();
        #[cfg(feature = "nds")]
        if beside && let Machine::Nds(nds) = machine {
            nds.copy_state_into(&mut self.copy);
            self.copied = true;
            self.since = 0;
            return Ok(());
        }
        #[cfg(not(feature = "nds"))]
        let _ = beside;
        let mut state = std::mem::take(&mut self.spare);
        let encoded = machine.rewind_state_into(&mut state);
        self.fresh = state;
        if encoded.is_err() {
            self.fresh.clear();
        }
        self.since = 0;
        encoded
    }

    pub(crate) fn settle(&mut self) {
        #[cfg(feature = "nds")]
        self.encode_copy();
        if self.fresh.is_empty() {
            return;
        }
        crate::profile::scope!("rewind settle");
        let parts = delta::layout(&self.fresh);
        if !self.newest.is_empty() {
            delta::diff(
                &self.newest,
                &self.parts,
                &self.fresh,
                &parts,
                &mut self.scratch,
            );
            let packed = lz4_flex::compress_prepend_size(&self.scratch);
            self.packed_bytes += packed.len();
            self.steps.push_back(Step { packed });
        }
        std::mem::swap(&mut self.newest, &mut self.fresh);
        self.spare = std::mem::take(&mut self.fresh);
        self.spare.clear();
        #[cfg(feature = "nds")]
        if self.copy.is_some() {
            self.spare = Vec::new();
        }
        self.parts = parts;
        self.trim();
    }

    #[cfg(feature = "nds")]
    fn encode_copy(&mut self) {
        if !std::mem::take(&mut self.copied) {
            return;
        }
        let Some(copy) = self.copy.as_deref() else {
            return;
        };
        crate::profile::scope!("rewind encode");
        let mut state = std::mem::take(&mut self.spare);
        state.clear();
        state.reserve(self.newest.len());
        match copy.encode_into(&mut state) {
            Ok(()) => self.fresh = state,
            Err(err) => {
                log::error!("could not keep a rewind state: {err:?}");
                self.clear();
            }
        }
    }

    fn trim(&mut self) {
        while self.steps.len() > self.keep || (!self.steps.is_empty() && self.bytes() > self.cap) {
            if let Some(step) = self.steps.pop_front() {
                self.packed_bytes -= step.packed.len();
            }
        }
    }

    pub(crate) fn step_back(&mut self, machine: &mut Machine) -> bool {
        crate::profile::scope!("rewind step");
        match self.try_step_back(machine) {
            Ok(moved) => moved,
            Err(err) => {
                log::error!("could not rewind: {err:?}");
                self.clear();
                false
            }
        }
    }

    fn try_step_back(&mut self, machine: &mut Machine) -> GbResult<bool> {
        self.settle();
        if self.newest.is_empty() {
            return Ok(false);
        }
        if self.since == 0 {
            let Some(step) = self.steps.pop_back() else {
                return Ok(false);
            };
            self.packed_bytes -= step.packed.len();
            let broken = |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, what);
            let delta = lz4_flex::decompress_size_prepended(&step.packed)
                .map_err(|_| broken("rewind step does not unpack"))?;
            delta::undo(&self.newest, &self.parts, &delta, &mut self.spare)
                .ok_or_else(|| broken("rewind step does not fit the newer state"))?;
            std::mem::swap(&mut self.newest, &mut self.spare);
            self.spare.clear();
            self.parts = delta::layout(&self.newest);
        }
        let sample_rate = machine.sample_rate();
        machine.rewind_to(&self.newest)?;
        machine.set_sample_rate(sample_rate);
        self.since = 0;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb::console::Keys;
    use citrine_gb::gb::{GameBoy, GbModel};
    use citrine_gb::rom::Rom;

    const SNAKE: &[u8] = include_bytes!("../../homebrew/snake.gb");
    #[cfg(feature = "gba")]
    const PENTRIS: &[u8] = include_bytes!("../../homebrew/gba/pentris.gba.br");
    #[cfg(feature = "nds")]
    const DS: &[u8] = include_bytes!("../../fixtures/nds/blocksds-lighting_cube.nds");

    fn gb() -> Machine {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&Rom::new(SNAKE)).unwrap();
        Machine::Gb(Box::new(gb))
    }

    #[cfg(feature = "gba")]
    fn gba() -> Machine {
        let mut rom = Vec::new();
        std::io::Read::read_to_end(&mut brotli::Decompressor::new(PENTRIS, 4096), &mut rom)
            .unwrap();
        Machine::Gba(Box::new(citrine_gb::gba::machine::Gba::new(&rom)))
    }

    #[cfg(feature = "nds")]
    fn ds() -> Machine {
        let firmware = citrine_gb::nds::machine::NdsFirmware::default();
        Machine::Nds(Box::new(
            citrine_gb::nds::machine::Nds::new(DS.into(), firmware).unwrap(),
        ))
    }

    fn game_of(machine: &Machine) -> (u64, Vec<u8>) {
        use citrine_gb::console::Rollback;
        let gb = machine.as_gb().unwrap();
        let mut scratch = gb.clone_state();
        (gb.state_hash(&mut scratch), machine.frame().to_vec())
    }

    fn played(machine: &mut Machine, rewind: &mut Rewind, frames: usize) {
        played_beside(machine, rewind, frames, true);
    }

    fn played_beside(machine: &mut Machine, rewind: &mut Rewind, frames: usize, beside: bool) {
        for frame in 0..frames {
            let keys = if frame % 20 < 5 {
                Keys::A
            } else {
                Keys::empty()
            };
            machine.set_keys(keys);
            machine.run_frame();
            machine.take_audio(&mut Vec::new());
            rewind.after_frame(machine, beside);
        }
    }

    fn rewinds_to_the_exact_state(machine: Machine) {
        rewinds_to_the_exact_state_beside(machine, true);
    }

    fn rewinds_to_the_exact_state_beside(mut machine: Machine, beside: bool) {
        let fps = 1.0 / machine.timing().frame_secs();
        let mut rewind = Rewind::default();
        rewind.follow(machine.kind(), 10, fps);
        let every = every(machine.kind()) as usize;
        played_beside(&mut machine, &mut rewind, 1 + 15 * every, beside);
        let then = machine.rewind_state().unwrap();
        let screen = machine.frame().to_vec();
        let seconds = 2;
        let later = (seconds as f64 * fps).round() as usize / every * every;
        played_beside(&mut machine, &mut rewind, later, beside);
        assert_ne!(machine.rewind_state().unwrap(), then);

        for _ in 0..later / every {
            assert!(rewind.step_back(&mut machine));
        }

        assert!(
            machine.rewind_state().unwrap() == then,
            "{:?} came back to another state",
            machine.kind()
        );
        assert!(machine.frame() == screen.as_slice());
    }

    #[test]
    fn rewinding_two_seconds_brings_back_the_game_boy_state_from_two_seconds_ago() {
        rewinds_to_the_exact_state(gb());
    }

    #[test]
    #[cfg(feature = "gba")]
    fn rewinding_two_seconds_brings_back_the_gba_state_from_two_seconds_ago() {
        rewinds_to_the_exact_state(gba());
    }

    #[test]
    #[cfg(feature = "nds")]
    fn rewinding_two_seconds_brings_back_the_ds_state_from_two_seconds_ago() {
        rewinds_to_the_exact_state(ds());
    }

    #[test]
    #[cfg(feature = "nds")]
    fn rewinding_a_ds_without_a_pool_brings_back_the_state_from_two_seconds_ago() {
        rewinds_to_the_exact_state_beside(ds(), false);
    }

    #[test]
    #[cfg(feature = "nds")]
    fn a_copied_ds_state_keeps_the_bytes_of_an_encoded_one() {
        let mut machine = ds();
        let mut copied = Rewind::default();
        let mut encoded = Rewind::default();
        copied.follow(machine.kind(), 10, 60.0);
        encoded.follow(machine.kind(), 10, 60.0);
        for frame in 0..120 {
            machine.set_keys(if frame % 20 < 5 {
                Keys::A
            } else {
                Keys::empty()
            });
            machine.run_frame();
            machine.take_audio(&mut Vec::new());
            copied.after_frame(&mut machine, true);
            encoded.after_frame(&mut machine, false);
            copied.settle();
            encoded.settle();
            assert!(copied.newest == encoded.newest, "frame {frame}");
        }
        assert_eq!(copied.steps.len(), encoded.steps.len());
        assert_eq!(copied.packed_bytes, encoded.packed_bytes);
    }

    #[test]
    fn a_rewound_game_plays_on_the_way_it_did_the_first_time() {
        let mut machine = gb();
        let mut rewind = Rewind::default();
        rewind.follow(machine.kind(), 10, 60.0);
        played(&mut machine, &mut rewind, 30);
        played(&mut machine, &mut rewind, 30);
        let ahead = game_of(&machine);
        for _ in 0..30 {
            assert!(rewind.step_back(&mut machine));
        }
        let mut again = Rewind::default();
        played(&mut machine, &mut again, 30);
        assert!(game_of(&machine) == ahead);
    }

    #[test]
    #[cfg(feature = "nds")]
    fn a_step_back_between_captures_first_returns_to_the_last_capture() {
        let mut machine = ds();
        let mut rewind = Rewind::default();
        rewind.follow(machine.kind(), 10, 60.0);
        played(&mut machine, &mut rewind, 9);
        let captured = machine.rewind_state().unwrap();
        played(&mut machine, &mut rewind, 2);
        assert!(rewind.step_back(&mut machine));
        assert!(machine.rewind_state().unwrap() == captured);
    }

    #[test]
    fn the_length_caps_how_far_back_it_goes() {
        let mut machine = gb();
        let mut rewind = Rewind::default();
        rewind.follow(machine.kind(), 1, 60.0);
        played(&mut machine, &mut rewind, 300);
        let mut steps = 0;
        while rewind.step_back(&mut machine) {
            steps += 1;
        }
        assert_eq!(steps, 60);
        assert!(!rewind.step_back(&mut machine), "the oldest state stays");
    }

    #[test]
    #[cfg(feature = "nds")]
    fn the_memory_cap_holds() {
        let mut machine = ds();
        let mut rewind = Rewind::default();
        rewind.follow(machine.kind(), 120, 60.0);
        played(&mut machine, &mut rewind, 1);
        rewind.settle();
        rewind.cap = rewind.bytes() + MIB / 4;
        played(&mut machine, &mut rewind, 400);
        assert!(rewind.bytes() <= rewind.cap, "{} bytes", rewind.bytes());
        assert!(rewind.steps.len() > 1);
    }

    #[test]
    #[cfg(feature = "gba")]
    fn a_cap_smaller_than_one_state_keeps_only_the_newest() {
        let mut machine = gba();
        let mut rewind = Rewind::default();
        rewind.follow(machine.kind(), 60, 60.0);
        rewind.cap = 1;
        played(&mut machine, &mut rewind, 20);
        assert!(rewind.steps.is_empty());
        assert!(!rewind.newest.is_empty());
    }

    #[test]
    fn lengths_read_in_seconds_and_minutes() {
        assert_eq!(RewindLength::Off.to_string(), "Off");
        assert_eq!(RewindLength::TenSeconds.to_string(), "10 s");
        assert_eq!(RewindLength::TwoMinutes.to_string(), "2 min");
    }
}
