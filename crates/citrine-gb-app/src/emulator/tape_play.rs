use super::Emulator;
use crate::frame_log::TapeTimes;
use citrine_gb::console::Console as _;
use citrine_gb::error::GbResult;
use citrine_gb::machine::Machine;
use citrine_gb::rom::Rom;
use citrine_gb_replay::{Diverged, Player, Replay, SeekPoints};
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
const SEEK_CAP: usize = 96 << 20;
#[cfg(not(target_arch = "wasm32"))]
const SEEK_CAP: usize = 256 << 20;

pub(crate) const SEEK_BUDGET: std::time::Duration = std::time::Duration::from_millis(12);

pub(crate) struct TapePlay {
    player: Player<Arc<Replay>>,
    target: Option<u64>,
    diverged: Option<Diverged>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TapeFacts {
    pub played: u64,
    pub frames: u64,
    pub frame_secs: f64,
    pub seeking: Option<u64>,
    pub running: bool,
    pub diverged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TapeSpot {
    played: u64,
    frames: u64,
    seeking: Option<u64>,
    diverged: bool,
}

impl TapePlay {
    pub(crate) fn spot(&self) -> TapeSpot {
        TapeSpot {
            played: self.player.played(),
            frames: self.player.frames(),
            seeking: self.target,
            diverged: self.diverged.is_some(),
        }
    }

    pub(crate) fn new(replay: Arc<Replay>, points: SeekPoints) -> Self {
        Self {
            player: Player::new(replay).seekable_from(SEEK_CAP, points),
            target: None,
            diverged: None,
        }
    }

    pub(crate) fn wanted(&self) -> usize {
        self.target
            .map_or(0, |to| to.saturating_sub(self.player.played()) as usize)
    }

    pub(crate) fn is_done(&self) -> bool {
        self.player.is_done() || self.diverged.is_some()
    }

    pub(crate) fn keeps_next_frame(&self) -> bool {
        self.player.keeps_next_frame()
    }

    pub(crate) fn step(&mut self, machine: &mut Machine, run: impl FnOnce(&mut Machine)) {
        if let Err(diverged) = self.player.step_with(machine, run) {
            log::warn!("the replay went another way: {diverged}");
            self.diverged = Some(diverged);
        }
        if self
            .target
            .is_some_and(|to| self.player.played() >= to || self.is_done())
        {
            self.target = None;
        }
    }

    fn seek(&mut self, machine: &mut Machine, to: u64) -> GbResult<()> {
        let to = to.clamp(1, self.player.frames().max(1));
        self.player.seek(machine, to)?;
        self.diverged = None;
        self.target = (self.player.played() < to).then_some(to);
        Ok(())
    }
}

impl Emulator {
    pub fn for_replay(&self, replay: Replay, rom: &Rom, points: SeekPoints) -> GbResult<Emulator> {
        let mut machine =
            Machine::from_state(&replay.start, rom, replay.header.start_unix, &self.firmware)?;
        machine.set_sample_rate(self.sample_rate());
        let mut show = Emulator {
            store: self.store.clone(),
            sound: self.sound,
            match_screen: self.match_screen,
            firmware: self.firmware.clone(),
            ..Emulator::default()
        };
        show.replace_machine(machine);
        show.game_name = replay.header.game.clone();
        show.tape_times = replay
            .perf
            .as_ref()
            .and_then(|perf| std::str::from_utf8(&perf.frames).ok())
            .and_then(TapeTimes::read);
        show.seat.core_mut().tape_play = Some(TapePlay::new(Arc::new(replay), points));
        show.playing_tape = true;
        show.running = true;
        Ok(show)
    }

    pub fn rom_with_sha1(&self, sha1: &str) -> Option<Rom> {
        if !self.has_game() {
            return None;
        }
        let machine = &self.seat.core().machine;
        let hex: String = machine
            .rom_id()
            .sha1
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        (hex == sha1).then(|| machine.rom())
    }

    pub fn hand_audio_to(&mut self, other: &mut Emulator) {
        let feed = self.seat.core_mut().audio_feed.take();
        if feed.is_some() {
            other.seat.core_mut().audio_feed = feed;
        }
    }

    pub fn tape_source(&self) -> Option<(Arc<Replay>, SeekPoints)> {
        let play = self.seat.core().tape_play.as_ref()?;
        Some((Arc::clone(play.player.replay()), play.player.seek_points()))
    }

    pub fn tape_times(&self) -> Option<&TapeTimes> {
        self.tape_times.as_ref().filter(|_| self.playing_tape)
    }

    pub fn is_playing_tape(&self) -> bool {
        self.playing_tape
    }

    pub fn tape_facts(&self) -> Option<TapeFacts> {
        if !self.playing_tape {
            return None;
        }
        let facts = self.seat.facts();
        let spot = facts.tape?;
        Some(TapeFacts {
            played: spot.played,
            frames: spot.frames,
            frame_secs: facts.timing.frame_secs(),
            seeking: spot.seeking,
            running: self.tape_resume.unwrap_or(self.running),
            diverged: spot.diverged,
        })
    }

    pub fn seek_tape(&mut self, to: u64) {
        let core = self.seat.core_mut();
        let Some(play) = core.tape_play.as_mut() else {
            return;
        };
        if let Err(err) = play.seek(&mut core.machine, to) {
            log::error!("could not seek the replay: {err:?}");
            return;
        }
        if play.target.is_some() && self.tape_resume.is_none() {
            self.tape_resume = Some(self.running);
            self.running = true;
        }
    }

    pub fn toggle_tape(&mut self) {
        let Some(facts) = self.tape_facts() else {
            return;
        };
        if facts.running {
            self.tape_resume = None;
            self.running = false;
            if let Some(play) = self.seat.core_mut().tape_play.as_mut() {
                play.target = None;
            }
        } else if facts.played >= facts.frames || facts.diverged {
            self.seek_tape(1);
            self.tape_resume = None;
            self.running = true;
        } else if self.tape_resume.is_some() {
            self.tape_resume = Some(true);
        } else {
            self.running = true;
        }
    }

    pub(super) fn follow_tape_play(&mut self) {
        if !self.playing_tape {
            return;
        }
        let Some(play) = self.seat.core().tape_play.as_ref() else {
            return;
        };
        let done = play.is_done();
        let seeking = play.target.is_some();
        if !seeking && let Some(resume) = self.tape_resume.take() {
            self.running = resume;
        }
        if done {
            self.running = false;
        }
    }
}
