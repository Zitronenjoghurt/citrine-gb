use super::Emulator;
use citrine_gb::console::{Console, ConsoleKind, VideoSpec};
use citrine_gb::error::GbResult;
use citrine_gb::machine::Machine;
use citrine_gb_replay::{Clip, Gap, Held, Input, Limits, Mark, Replay, Tape};
use citrine_gb_threads::Spawner;
use std::collections::VecDeque;
use std::fmt::Display;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use strum::IntoEnumIterator;
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
        write_length(f, self.seconds())
    }
}

fn write_length(f: &mut std::fmt::Formatter<'_>, seconds: Option<u32>) -> std::fmt::Result {
    match seconds {
        None => f.write_str("Off"),
        Some(seconds) if seconds < 60 => write!(f, "{seconds} s"),
        Some(seconds) => write!(f, "{} min", seconds / 60),
    }
}

pub const CLIP_LENGTH_NOTE: &str =
    "How far back Save a clip goes. It doesn't change how far Rewind goes.";

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum ClipLength {
    Off,
    TenSeconds,
    #[default]
    ThirtySeconds,
    OneMinute,
    TwoMinutes,
}

impl ClipLength {
    pub fn seconds(self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::TenSeconds => Some(10),
            Self::ThirtySeconds => Some(30),
            Self::OneMinute => Some(60),
            Self::TwoMinutes => Some(120),
        }
    }

    pub fn reaching(rewind: RewindLength) -> Self {
        let Some(wanted) = rewind.seconds() else {
            return Self::default();
        };
        Self::iter()
            .find(|clip| clip.seconds().is_some_and(|seconds| seconds >= wanted))
            .or_else(|| Self::iter().max_by_key(|clip| clip.seconds()))
            .unwrap_or_default()
    }
}

impl Display for ClipLength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_length(f, self.seconds())
    }
}

fn frames_in(seconds: u32, fps: f64) -> u64 {
    (f64::from(seconds) * fps).ceil() as u64
}

pub(crate) fn keyframe_every(kind: ConsoleKind) -> u64 {
    match kind {
        ConsoleKind::Nds => 30,
        _ => 60,
    }
}

pub(crate) fn twin_of(live: &Machine) -> Machine {
    let mut twin = live.twin();
    twin.defer_3d(false);
    twin
}

fn pictures_of(
    gap: &Gap,
    twin: &mut Machine,
    every: u64,
    below: u64,
    spare: &mut Vec<Vec<u8>>,
) -> GbResult<Vec<Picture>> {
    let every = every.max(1);
    let mut fresh = Vec::new();
    gap.run(twin, |frame, twin| {
        if frame.is_multiple_of(every) && frame < below {
            let mut pixels = spare.pop().unwrap_or_default();
            pixels.clear();
            pixels.extend_from_slice(twin.frame());
            fresh.push(Picture {
                frame,
                pixels,
                video: twin.video(),
            });
        }
    })?;
    Ok(fresh)
}

fn every(kind: ConsoleKind) -> u32 {
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
    pub(crate) reach: u32,
    pub(crate) back: bool,
}

#[cfg(test)]
impl RewindPlan {
    pub(crate) const OFF: Self = Self {
        keep: None,
        reach: 0,
        back: false,
    };
}

const RECORDING_KEEP: u32 = 2;

impl Emulator {
    pub fn can_tape(&self) -> bool {
        self.seat.facts().has_game && self.tape_blocker().is_none()
    }

    pub fn tape_blocker(&self) -> Option<&'static str> {
        let facts = self.seat.facts();
        #[cfg(feature = "nds")]
        if facts.walker {
            return Some("Pokéwalker");
        }
        #[cfg(ds_wifi)]
        if facts.wifi {
            return Some("DS wireless");
        }
        if facts.linked {
            Some("link cable")
        } else if facts.partnered {
            Some("Game Boy partner")
        } else if self.together {
            Some("Play together")
        } else if self.replay.is_some() || self.recorder.is_active() {
            Some("input replay")
        } else if self.playing_tape {
            Some("replay")
        } else {
            None
        }
    }

    pub fn video_header(&self) -> Option<citrine_gb_replay::Header> {
        if !self.has_game() {
            return None;
        }
        let machine = &self.seat.core().machine;
        Some(citrine_gb_replay::Header {
            no_inputs: Some(self.tape_blocker().unwrap_or("video").to_string()),
            ..citrine_gb_replay::Header::of(machine, crate::storage::now_unix())
        })
    }

    pub fn can_rewind(&self) -> bool {
        self.rewind_length.seconds().is_some() && !self.hardcore && self.can_tape()
    }

    pub fn is_rewinding(&self) -> bool {
        self.rewinding
    }

    pub(super) fn rewind_plan(&self) -> RewindPlan {
        let reach = self.rewind_length.seconds().filter(|_| self.can_rewind());
        let clip = self.clip_length.seconds().filter(|_| self.can_clip());
        let recording = (self.taping && self.can_tape()).then_some(RECORDING_KEEP);
        RewindPlan {
            keep: reach.max(clip).or(recording),
            reach: reach.unwrap_or(0),
            back: self.rewind_held && reach.is_some(),
        }
    }

    pub fn start_tape_recording(&mut self) -> bool {
        if self.taping || !self.can_tape() {
            return false;
        }
        self.taping = true;
        let plan = self.rewind_plan();
        let now = crate::storage::now_unix();
        self.taping = self.seat.core_mut().start_recording(now, plan);
        self.taping
    }

    pub fn restart_tape_recording(&mut self) -> bool {
        self.taping && !self.hardcore && self.seat.core_mut().restart_recording()
    }

    pub fn stop_tape_recording(&mut self) -> Option<Replay> {
        if !std::mem::take(&mut self.taping) {
            return None;
        }
        self.seat.core_mut().stop_recording()
    }

    pub fn is_taping(&self) -> bool {
        self.taping
    }

    pub fn is_recording(&self) -> bool {
        self.taping || self.screen_recorder.is_recording()
    }

    pub fn recording_label(&self) -> String {
        match self.tape_recording_secs() {
            Some(secs) => crate::media::format_length((secs * 1000.0) as u64),
            None => self.screen_recorder.elapsed_label(),
        }
    }

    pub fn tape_frame(&self) -> Option<u64> {
        if !self.taping {
            return None;
        }
        self.seat.facts().recording_frames
    }

    pub fn tape_recording_secs(&self) -> Option<f64> {
        if !self.taping {
            return None;
        }
        let frames = self.seat.facts().recording_frames?;
        Some(frames as f64 * self.timing().frame_secs())
    }

    pub fn can_clip(&self) -> bool {
        self.clip_length.seconds().is_some() && !self.hardcore && self.can_tape()
    }

    pub fn take_clip(&mut self) -> Option<(Clip, Machine)> {
        let seconds = self.clip_length.seconds().filter(|_| self.can_clip())?;
        let core = self.seat.core_mut();
        core.land_rewind();
        let clip = core.rewind.clip(&core.machine, seconds)?;
        Some((clip, twin_of(&core.machine)))
    }

    pub fn take_cut_recording(&mut self) -> Option<Replay> {
        self.seat.core_mut().cut_recording.take()
    }

    pub(super) fn follow_taping(&mut self) {
        if !self.taping || (self.can_tape() && self.seat.core().rewind.recording_frames().is_some())
        {
            return;
        }
        self.taping = false;
        let core = self.seat.core_mut();
        if let Some(replay) = core.stop_recording() {
            core.cut_recording = Some(replay);
        }
    }
}

struct Picture {
    frame: u64,
    pixels: Vec<u8>,
    video: VideoSpec,
}

struct Ahead {
    from: u64,
    to: u64,
    done: Receiver<GbResult<Filled>>,
}

struct Filled {
    twin: Machine,
    pictures: Vec<Picture>,
}

enum Ready {
    Filled,
    Waiting,
    Missing,
}

#[derive(Default)]
pub(crate) struct Rewind {
    held: Held,
    tape: Option<Tape>,
    limits: Option<Limits>,
    every: u64,
    reach: u64,
    pictures: VecDeque<Picture>,
    spare: Vec<Vec<u8>>,
    progress: VecDeque<(u64, Option<Vec<u8>>)>,
    shown: Option<u64>,
    twin: Option<Machine>,
    spawner: Option<Spawner>,
    ahead: Option<Ahead>,
    recording: Option<Recording>,
}

struct Recording {
    mark: Mark,
    progress: Option<Vec<u8>>,
}

impl Rewind {
    pub(crate) fn clear(&mut self) {
        self.tape = None;
        while let Some(picture) = self.pictures.pop_front() {
            self.spare.push(picture.pixels);
        }
        self.progress.clear();
        self.shown = None;
        self.twin = None;
        self.ahead = None;
        self.recording = None;
    }

    #[cfg(any(test, threaded))]
    pub(crate) fn run_gaps_on(&mut self, spawner: Spawner) {
        self.spawner = Some(spawner);
    }

    pub(crate) fn follow(&mut self, kind: ConsoleKind, seconds: u32, reach: u32, fps: f64) {
        self.every = u64::from(every(kind));
        self.reach = frames_in(reach, fps);
        let limits = Limits {
            keyframe_every: keyframe_every(kind),
            keep_frames: frames_in(seconds, fps),
            cap: cap(kind),
        };
        if self.limits == Some(limits) {
            return;
        }
        self.limits = Some(limits);
        if let Some(tape) = &mut self.tape {
            tape.set_limits(limits);
            let oldest = tape.oldest();
            self.progress.retain(|(frame, _)| *frame >= oldest);
        }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn kept(&self) -> (u64, usize) {
        self.tape
            .as_ref()
            .map_or((0, 0), |tape| (tape.frame() - tape.oldest(), tape.bytes()))
    }

    pub(crate) fn feed(&mut self, machine: &mut Machine, input: Input) {
        citrine_gb_replay::apply(machine, &input);
        self.record(input);
    }

    pub(crate) fn record(&mut self, input: Input) {
        self.held.take(&input);
        if let Some(tape) = &mut self.tape {
            tape.record(input);
        }
    }

    pub(crate) fn captures_next(&self) -> bool {
        match &self.tape {
            Some(tape) => (tape.frame() + 1).is_multiple_of(self.every.max(1)),
            None => true,
        }
    }

    pub(crate) fn is_rewound(&self) -> bool {
        self.shown.is_some()
    }

    pub(crate) fn after_frame(
        &mut self,
        machine: &mut Machine,
        progress: impl FnOnce() -> Option<Vec<u8>>,
    ) {
        crate::profile::scope!("rewind keep");
        if let Err(err) = self.keep(machine, progress) {
            log::error!("could not keep a rewind state: {err:?}");
            self.clear();
        }
    }

    fn keep(
        &mut self,
        machine: &mut Machine,
        progress: impl FnOnce() -> Option<Vec<u8>>,
    ) -> GbResult<()> {
        let Some(limits) = self.limits else {
            return Ok(());
        };
        if let Some(tape) = &mut self.tape {
            tape.after_frame(machine)?;
        } else {
            self.tape = Some(Tape::start(machine, limits, self.held.clone())?);
        }
        let Some(tape) = &self.tape else {
            return Ok(());
        };
        let frame = tape.frame();
        let oldest = tape.oldest();
        let floor = tape.landing(frame).unwrap_or(frame);
        if frame.is_multiple_of(limits.keyframe_every) {
            self.progress.push_back((frame, progress()));
        }
        while self.progress.front().is_some_and(|(at, _)| *at < oldest) {
            self.progress.pop_front();
        }
        while self
            .pictures
            .front()
            .is_some_and(|picture| picture.frame < floor)
        {
            if let Some(picture) = self.pictures.pop_front() {
                self.spare.push(picture.pixels);
            }
        }
        if frame.is_multiple_of(self.every.max(1)) {
            let picture = self.picture_of(frame, machine);
            self.pictures.push_back(picture);
        }
        Ok(())
    }

    fn picture_of(&mut self, frame: u64, machine: &Machine) -> Picture {
        let mut pixels = self.spare.pop().unwrap_or_default();
        pixels.clear();
        pixels.extend_from_slice(machine.frame());
        Picture {
            frame,
            pixels,
            video: machine.video(),
        }
    }

    pub(crate) fn picture(&self) -> Option<(&[u8], VideoSpec)> {
        let shown = self.shown?;
        self.pictures
            .back()
            .filter(|picture| picture.frame == shown)
            .map(|picture| (picture.pixels.as_slice(), picture.video))
    }

    pub(crate) fn step_back(&mut self, machine: &Machine) -> bool {
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

    fn floor(&self, tape: &Tape) -> u64 {
        let oldest = tape.oldest();
        tape.landing(tape.frame().saturating_sub(self.reach))
            .map_or(oldest, |landing| landing.max(oldest))
    }

    fn try_step_back(&mut self, machine: &Machine) -> GbResult<bool> {
        let Some(tape) = &self.tape else {
            return Ok(false);
        };
        let every = self.every.max(1);
        let shown = self.shown.unwrap_or(tape.frame());
        let Some(target) = shown.checked_sub(1).map(|frame| frame / every * every) else {
            return Ok(false);
        };
        if target <= self.floor(tape)
            || self
                .recording
                .as_ref()
                .is_some_and(|recording| target < recording.mark.frame())
        {
            return Ok(false);
        }
        if !self.pictures.iter().any(|picture| picture.frame == target) {
            match self.collect_ahead(target)? {
                Ready::Filled => {}
                Ready::Waiting => return Ok(false),
                Ready::Missing => self.fill(machine, target)?,
            }
        }
        while self
            .pictures
            .back()
            .is_some_and(|picture| picture.frame > target)
        {
            if let Some(picture) = self.pictures.pop_back() {
                self.spare.push(picture.pixels);
            }
        }
        if self
            .pictures
            .back()
            .is_none_or(|picture| picture.frame != target)
        {
            return Ok(false);
        }
        self.shown = Some(target);
        self.start_ahead(machine);
        Ok(true)
    }

    fn below(&self) -> u64 {
        self.pictures
            .front()
            .map_or(u64::MAX, |picture| picture.frame)
    }

    fn put_front(&mut self, pictures: Vec<Picture>) {
        let below = self.below();
        for picture in pictures.into_iter().rev() {
            if picture.frame < below {
                self.pictures.push_front(picture);
            } else {
                self.spare.push(picture.pixels);
            }
        }
    }

    fn fill(&mut self, live: &Machine, target: u64) -> GbResult<()> {
        crate::profile::scope!("rewind fill");
        let Some(gap) = self.tape.as_ref().and_then(|tape| tape.gap(target)) else {
            return Ok(());
        };
        let below = self.below();
        let twin = self.twin.get_or_insert_with(|| twin_of(live));
        let fresh = pictures_of(&gap, twin, self.every, below, &mut self.spare)?;
        self.put_front(fresh);
        Ok(())
    }

    fn collect_ahead(&mut self, target: u64) -> GbResult<Ready> {
        let Some(ahead) = &self.ahead else {
            return Ok(Ready::Missing);
        };
        if target <= ahead.from || target > ahead.to {
            self.ahead = None;
            return Ok(Ready::Missing);
        }
        match ahead.done.try_recv() {
            Ok(filled) => {
                self.ahead = None;
                let filled = filled?;
                self.twin = Some(filled.twin);
                self.put_front(filled.pictures);
                Ok(Ready::Filled)
            }
            Err(TryRecvError::Empty) => Ok(Ready::Waiting),
            Err(TryRecvError::Disconnected) => {
                self.ahead = None;
                Ok(Ready::Missing)
            }
        }
    }

    fn start_ahead(&mut self, live: &Machine) {
        let (Some(spawner), Some(tape), None) = (&self.spawner, &self.tape, &self.ahead) else {
            return;
        };
        let every = self.every.max(1);
        let below = self.below();
        let Some(next) = below.checked_sub(1).map(|frame| frame / every * every) else {
            return;
        };
        if next <= self.floor(tape) {
            return;
        }
        let Some(gap) = tape.gap(next) else {
            return;
        };
        let mut twin = self.twin.take().unwrap_or_else(|| twin_of(live));
        let (from, to) = (gap.from(), gap.to());
        let (sent, done) = mpsc::channel();
        spawner.run("rewind gap", move || {
            let filled = pictures_of(&gap, &mut twin, every, below, &mut Vec::new())
                .map(|pictures| Filled { twin, pictures });
            let _ = sent.send(filled);
        });
        self.ahead = Some(Ahead { from, to, done });
    }

    #[cfg(any(test, not(target_arch = "wasm32")))]
    pub(crate) fn landing_progress(&self) -> Option<Option<&[u8]>> {
        let landing = self.tape.as_ref()?.landing(self.shown?)?;
        self.progress
            .iter()
            .find(|(frame, _)| *frame == landing)
            .map(|(_, progress)| progress.as_deref())
    }

    pub(crate) fn land(&mut self, machine: &mut Machine, each: impl FnMut(u64, &mut Machine)) {
        let Some(shown) = self.shown.take() else {
            return;
        };
        self.ahead = None;
        let Some(tape) = self.tape.as_mut() else {
            return;
        };
        crate::profile::scope!("rewind land");
        let sample_rate = machine.sample_rate();
        let landed = tape.land(machine, shown, each);
        machine.set_sample_rate(sample_rate);
        match landed {
            Ok(true) => tape.hold(machine, &self.held),
            Ok(false) => return,
            Err(err) => {
                log::error!("could not land the rewind: {err:?}");
                self.clear();
                return;
            }
        }
        self.drop_after(shown);
    }

    fn drop_after(&mut self, frame: u64) {
        while self
            .pictures
            .back()
            .is_some_and(|picture| picture.frame > frame)
        {
            if let Some(picture) = self.pictures.pop_back() {
                self.spare.push(picture.pixels);
            }
        }
        self.progress.retain(|(at, _)| *at <= frame);
    }

    pub(crate) fn start_recording(
        &mut self,
        machine: &mut Machine,
        now: u64,
        progress: Option<Vec<u8>>,
    ) -> GbResult<bool> {
        self.stop_recording(machine);
        if self.tape.is_none() {
            let Some(limits) = self.limits else {
                return Ok(false);
            };
            self.tape = Some(Tape::start(machine, limits, self.held.clone())?);
        }
        let Some(tape) = &mut self.tape else {
            return Ok(false);
        };
        let mark = tape.mark(machine, now)?;
        self.recording = Some(Recording { mark, progress });
        Ok(true)
    }

    pub(crate) fn clip(&mut self, machine: &Machine, seconds: u32) -> Option<Clip> {
        let back = frames_in(seconds, 1.0 / machine.timing().frame_secs());
        self.record(Input::Keys(machine.keys().bits()));
        self.tape
            .as_ref()?
            .clip(machine, back)
            .filter(|clip| clip.frames() > 0)
    }

    pub(crate) fn recording_frames(&self) -> Option<u64> {
        let mark = self.recording.as_ref()?.mark;
        self.tape.as_ref()?.frame().checked_sub(mark.frame())
    }

    pub(crate) fn restart_recording(
        &mut self,
        machine: &mut Machine,
        restore: impl FnOnce(Option<&[u8]>),
    ) -> GbResult<bool> {
        let (Some(recording), Some(tape)) = (&self.recording, self.tape.as_mut()) else {
            return Ok(false);
        };
        let mark = recording.mark;
        self.shown = None;
        self.ahead = None;
        let sample_rate = machine.sample_rate();
        let restarted = tape.restart(machine, mark);
        machine.set_sample_rate(sample_rate);
        if !restarted? {
            return Ok(false);
        }
        restore(recording.progress.as_deref());
        tape.hold(machine, &self.held);
        self.drop_after(mark.frame());
        Ok(true)
    }

    pub(crate) fn stop_recording(&mut self, machine: &Machine) -> Option<Replay> {
        let mark = self.recording.take()?.mark;
        self.record(Input::Keys(machine.keys().bits()));
        let tape = self.tape.as_mut()?;
        let replay = tape.cut(mark, machine);
        tape.unmark(mark);
        replay
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use citrine_gb::console::Keys;
    use citrine_gb::gb::{GameBoy, GbModel};
    use citrine_gb::machine::Firmware;
    use citrine_gb::rom::Rom;
    use citrine_gb_replay::Player;

    const NOW: u64 = 1_780_000_000;

    const SNAKE: &[u8] = include_bytes!("../../homebrew/snake.gb");
    #[cfg(feature = "gba")]
    const PENTRIS: &[u8] = include_bytes!("../../homebrew/gba/pentris.gba.br");
    #[cfg(feature = "nds")]
    const DS: &[u8] = include_bytes!("../../fixtures/nds/blocksds-lighting_cube.nds");

    pub(crate) const RECORDED_ROM: &[u8] = SNAKE;

    pub(crate) fn recorded_replay(frames: usize) -> Replay {
        let mut machine = gb();
        let mut rewind = following(&machine, 10);
        played(&mut machine, &mut rewind, 37);
        assert!(rewind.start_recording(&mut machine, NOW, None).unwrap());
        played(&mut machine, &mut rewind, frames);
        rewind.stop_recording(&machine).unwrap()
    }

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

    fn keys_at(frame: usize) -> Keys {
        if frame % 20 < 5 {
            Keys::A
        } else {
            Keys::empty()
        }
    }

    fn played(machine: &mut Machine, rewind: &mut Rewind, frames: usize) {
        for frame in 0..frames {
            rewind.feed(machine, Input::Keys(keys_at(frame).bits()));
            #[cfg(feature = "nds")]
            if matches!(machine, Machine::Nds(_)) {
                rewind.feed(machine, Input::Mic(Vec::new()));
            }
            machine.run_frame();
            machine.take_audio(&mut Vec::new());
            rewind.after_frame(machine, || None);
        }
    }

    fn following(machine: &Machine, seconds: u32) -> Rewind {
        let mut rewind = Rewind::default();
        let fps = 1.0 / machine.timing().frame_secs();
        rewind.follow(machine.kind(), seconds, seconds, fps);
        rewind
    }

    fn keeping(machine: &Machine, seconds: u32, reach: u32) -> Rewind {
        let mut rewind = Rewind::default();
        let fps = 1.0 / machine.timing().frame_secs();
        rewind.follow(machine.kind(), seconds, reach, fps);
        rewind
    }

    fn rewinds_to_the_exact_state(mut machine: Machine) {
        let mut rewind = following(&machine, 10);
        let every = every(machine.kind()) as usize;
        played(&mut machine, &mut rewind, 1 + 15 * every);
        let held_then = keys_at(15 * every);
        let then = machine.state_hash();
        let screen = machine.frame().to_vec();
        let later = 120 / every * every;
        played(&mut machine, &mut rewind, later);
        let now = machine.state_hash();
        assert_ne!(now, then);

        for _ in 0..later / every {
            assert!(rewind.step_back(&machine));
        }
        assert_eq!(machine.state_hash(), now, "the live game waits");
        let (shown, _) = rewind.picture().unwrap();
        assert!(
            shown == screen.as_slice(),
            "{:?} shows another picture",
            machine.kind()
        );

        rewind.feed(&mut machine, Input::Keys(held_then.bits()));
        rewind.land(&mut machine, |_, _| {});
        assert_eq!(
            machine.state_hash(),
            then,
            "{:?} came back to another state",
            machine.kind()
        );
        assert!(rewind.picture().is_none());
    }

    fn played_back(replay: &Replay, rom: &[u8]) -> Machine {
        let replay = Replay::read(&replay.write().unwrap()).unwrap();
        let mut machine =
            Machine::from_state(&replay.start, &Rom::new(rom), NOW, &Firmware::default()).unwrap();
        let mut player = Player::new(&replay);
        while !player.is_done() {
            player.step(&mut machine).unwrap();
            machine.take_audio(&mut Vec::new());
        }
        machine
    }

    #[test]
    fn a_recording_plays_back_to_where_it_stopped() {
        let mut machine = gb();
        let mut rewind = following(&machine, 10);
        played(&mut machine, &mut rewind, 37);
        assert!(rewind.start_recording(&mut machine, NOW, None).unwrap());
        played(&mut machine, &mut rewind, 250);
        assert_eq!(rewind.recording_frames(), Some(250));
        let replay = rewind.stop_recording(&machine).unwrap();
        assert_eq!(replay.header.frames, 250);
        assert_eq!(rewind.recording_frames(), None);
        assert_eq!(
            played_back(&replay, SNAKE).state_hash(),
            machine.state_hash()
        );
    }

    fn starting_over_brings_back_the_marked_state(mut machine: Machine) {
        let mut rewind = following(&machine, 10);
        played(&mut machine, &mut rewind, 37);
        rewind.start_recording(&mut machine, NOW, None).unwrap();
        let then = machine.state_hash();
        played(&mut machine, &mut rewind, 100);
        rewind.feed(&mut machine, Input::Keys(keys_at(36).bits()));
        assert!(rewind.restart_recording(&mut machine, |_| {}).unwrap());
        assert_eq!(
            machine.state_hash(),
            then,
            "{:?} started over from another state",
            machine.kind()
        );
        assert_eq!(rewind.recording_frames(), Some(0));
        played(&mut machine, &mut rewind, 50);
        let replay = rewind.stop_recording(&machine).unwrap();
        assert_eq!(replay.header.frames, 50);
    }

    #[test]
    fn starting_over_brings_back_the_game_boy_state_of_the_mark() {
        starting_over_brings_back_the_marked_state(gb());
    }

    #[test]
    #[cfg(feature = "nds")]
    fn starting_over_brings_back_the_ds_state_of_the_mark() {
        starting_over_brings_back_the_marked_state(ds());
    }

    #[test]
    fn starting_over_brings_back_the_progress_kept_at_the_mark() {
        let mut machine = gb();
        let mut rewind = following(&machine, 10);
        played(&mut machine, &mut rewind, 20);
        rewind
            .start_recording(&mut machine, NOW, Some(vec![7, 9]))
            .unwrap();
        played(&mut machine, &mut rewind, 70);
        let mut restored = None;
        rewind
            .restart_recording(&mut machine, |progress| {
                restored = progress.map(<[u8]>::to_vec)
            })
            .unwrap();
        assert_eq!(restored, Some(vec![7, 9]));
    }

    #[test]
    fn rewinding_inside_a_recording_stops_at_its_start_and_records_on() {
        let mut machine = gb();
        let mut rewind = following(&machine, 10);
        played(&mut machine, &mut rewind, 100);
        rewind.start_recording(&mut machine, NOW, None).unwrap();
        played(&mut machine, &mut rewind, 30);
        let mut steps = 0;
        while rewind.step_back(&machine) {
            steps += 1;
        }
        assert_eq!(steps, 30);
        rewind.land(&mut machine, |_, _| {});
        assert_eq!(rewind.recording_frames(), Some(0));
        played(&mut machine, &mut rewind, 80);
        let replay = rewind.stop_recording(&machine).unwrap();
        assert_eq!(replay.header.frames, 80);
        assert_eq!(
            played_back(&replay, SNAKE).state_hash(),
            machine.state_hash()
        );
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
    fn the_first_step_back_shows_a_kept_picture_without_running_the_game() {
        let mut machine = gb();
        let mut rewind = following(&machine, 10);
        let mut screens = Vec::new();
        for _ in 0..100 {
            played(&mut machine, &mut rewind, 1);
            screens.push(machine.frame().to_vec());
        }
        assert!(rewind.step_back(&machine));
        assert!(rewind.twin.is_none());
        let (shown, _) = rewind.picture().unwrap();
        assert!(shown == screens[98].as_slice());
    }

    #[test]
    fn landing_brings_back_the_progress_of_its_keyframe_and_runs_on_from_it() {
        let mut machine = gb();
        let mut rewind = following(&machine, 10);
        for frame in 0u32..200 {
            machine.run_frame();
            machine.take_audio(&mut Vec::new());
            rewind.after_frame(&mut machine, || Some(frame.to_le_bytes().to_vec()));
        }
        for _ in 0..99 {
            assert!(rewind.step_back(&machine));
        }
        assert_eq!(
            rewind.landing_progress(),
            Some(Some(60u32.to_le_bytes().as_slice()))
        );
        let mut caught_up = Vec::new();
        rewind.land(&mut machine, |frame, _| caught_up.push(frame));
        assert_eq!(caught_up, (61..=100).collect::<Vec<u64>>());
        assert_eq!(rewind.landing_progress(), None);
    }

    #[test]
    fn a_rewound_game_plays_on_the_way_it_did_the_first_time() {
        let mut machine = gb();
        let mut rewind = following(&machine, 10);
        played(&mut machine, &mut rewind, 30);
        played(&mut machine, &mut rewind, 30);
        let ahead = machine.state_hash();
        for _ in 0..30 {
            assert!(rewind.step_back(&machine));
        }
        rewind.land(&mut machine, |_, _| {});
        played(&mut machine, &mut rewind, 30);
        assert_eq!(machine.state_hash(), ahead);
    }

    #[test]
    fn gaps_on_the_side_hold_the_rewind_until_their_pictures_come() {
        let threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Inline);
        let mut machine = gb();
        let mut rewind = following(&machine, 10);
        rewind.run_gaps_on(threads.spawner());
        let mut screens = Vec::new();
        for _ in 0..300 {
            played(&mut machine, &mut rewind, 1);
            screens.push(machine.frame().to_vec());
        }
        let mut steps = 0;
        let mut waits = 0;
        while steps < 200 {
            if rewind.step_back(&machine) {
                steps += 1;
                let (shown, _) = rewind.picture().unwrap();
                assert!(shown == screens[299 - steps].as_slice(), "step {steps}");
            } else {
                waits += 1;
                assert!(waits <= steps, "waited at step {steps}");
                assert!(threads.run_side_jobs().is_empty());
            }
        }
        assert!(waits > 0);
        assert!(rewind.ahead.is_some());
        rewind.land(&mut machine, |_, _| {});
        assert!(rewind.ahead.is_none());
    }

    #[test]
    fn keys_let_go_while_rewinding_are_let_go_after_landing() {
        let mut machine = gb();
        let mut rewind = following(&machine, 10);
        rewind.feed(&mut machine, Input::Keys(Keys::A.bits()));
        for _ in 0..90 {
            machine.run_frame();
            machine.take_audio(&mut Vec::new());
            rewind.after_frame(&mut machine, || None);
        }
        for _ in 0..30 {
            assert!(rewind.step_back(&machine));
        }
        rewind.feed(&mut machine, Input::Keys(Keys::empty().bits()));
        rewind.land(&mut machine, |_, _| {});
        assert_eq!(machine.keys(), Keys::empty());
    }

    #[test]
    fn the_length_caps_how_far_back_it_goes() {
        let mut machine = gb();
        let mut rewind = following(&machine, 1);
        played(&mut machine, &mut rewind, 300);
        let mut steps = 0;
        while rewind.step_back(&machine) {
            steps += 1;
        }
        assert_eq!(steps, 118);
        assert!(!rewind.step_back(&machine), "the oldest state stays");
    }

    #[test]
    fn rewind_stops_at_its_own_length_when_clips_keep_more() {
        let mut machine = gb();
        let mut rewind = keeping(&machine, 4, 1);
        played(&mut machine, &mut rewind, 600);
        let kept = rewind
            .tape
            .as_ref()
            .map(|tape| tape.frame() - tape.oldest());
        assert_eq!(kept, Some(239), "the tape keeps the clip length");
        let mut steps = 0;
        while rewind.step_back(&machine) {
            steps += 1;
        }
        assert_eq!(steps, 118, "as far as a 1 s rewind goes");
    }

    #[test]
    fn a_clip_reaches_back_its_own_length_past_the_rewind() {
        let mut machine = gb();
        let fps = 1.0 / machine.timing().frame_secs();
        let mut rewind = keeping(&machine, 4, 1);
        played(&mut machine, &mut rewind, 600);
        let long = rewind.clip(&machine, 4).unwrap().frames();
        let short = rewind.clip(&machine, 2).unwrap().frames();
        assert!(long >= frames_in(4, fps), "{long} frames");
        assert!(
            short >= frames_in(2, fps) && short < long,
            "{short} of {long} frames"
        );

        let mut machine = gb();
        let mut clips_only = keeping(&machine, 4, 0);
        played(&mut machine, &mut clips_only, 600);
        let frames = clips_only.clip(&machine, 4).unwrap().frames();
        assert!(frames >= frames_in(4, fps), "{frames} frames");
    }

    #[test]
    fn lengths_read_in_seconds_and_minutes() {
        assert_eq!(RewindLength::Off.to_string(), "Off");
        assert_eq!(RewindLength::TenSeconds.to_string(), "10 s");
        assert_eq!(RewindLength::TwoMinutes.to_string(), "2 min");
        assert_eq!(ClipLength::Off.to_string(), "Off");
        assert_eq!(ClipLength::ThirtySeconds.to_string(), "30 s");
        assert_eq!(ClipLength::OneMinute.to_string(), "1 min");
    }

    #[test]
    fn a_clip_length_from_the_rewind_goes_at_least_as_far_back() {
        assert_eq!(
            ClipLength::reaching(RewindLength::Off),
            ClipLength::ThirtySeconds
        );
        assert_eq!(
            ClipLength::reaching(RewindLength::TenSeconds),
            ClipLength::TenSeconds
        );
        for rewind in RewindLength::iter() {
            let clip = ClipLength::reaching(rewind);
            assert!(clip.seconds().is_some(), "{rewind:?}");
            if let Some(seconds) = rewind.seconds() {
                assert!(clip.seconds() >= Some(seconds), "{rewind:?} to {clip:?}");
                assert!(
                    ClipLength::iter()
                        .filter_map(ClipLength::seconds)
                        .filter(|longer| *longer >= seconds)
                        .all(|longer| clip.seconds() <= Some(longer)),
                    "{rewind:?} to {clip:?} is not the shortest"
                );
            }
        }
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    fn walked(frame: usize) -> Keys {
        if frame < 1500 {
            return if frame % 60 < 6 {
                Keys::A
            } else {
                Keys::empty()
            };
        }
        [Keys::UP, Keys::LEFT, Keys::DOWN, Keys::RIGHT][frame / 90 % 4]
    }

    #[test]
    #[ignore = "real DS game in .roms"]
    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    fn platinum_history_bytes_for_two_minutes_of_rewind_and_five_of_clips() {
        let test = "platinum_history_bytes_for_two_minutes_of_rewind_and_five_of_clips";
        let roms = citrine_gb_test_roms::roms_dir();
        let Some(bytes) = citrine_gb_test_roms::read(test, roms.join("pt.nds")) else {
            return;
        };
        let mut machine =
            Machine::load(&Rom::new(&bytes), ConsoleKind::Nds, &Firmware::default()).unwrap();
        let saved = std::fs::read(roms.join("pt-club.sav")).ok();
        if let Some(sav) = &saved {
            let battery = machine.battery_from_file(sav);
            machine.put_battery(battery, NOW);
        }
        let fps = 1.0 / machine.timing().frame_secs();
        let mut rewind = keeping(&machine, 120, 120);
        let mut clips = keeping(&machine, 300, 0);
        let frames = frames_in(300, fps) as usize + 120;
        let (mut rewind_peak, mut clips_peak) = (0, 0);
        for frame in 0..frames {
            for input in [Input::Keys(walked(frame).bits()), Input::Mic(Vec::new())] {
                clips.record(input.clone());
                rewind.feed(&mut machine, input);
            }
            machine.run_frame();
            machine.take_audio(&mut Vec::new());
            rewind.after_frame(&mut machine, || None);
            clips.after_frame(&mut machine, || None);
            rewind_peak = rewind_peak.max(rewind.kept().1);
            clips_peak = clips_peak.max(clips.kept().1);
        }
        let mib = |bytes: usize| bytes as f64 / (1024.0 * 1024.0);
        let (rewind_frames, rewind_bytes) = rewind.kept();
        let (clips_frames, clips_bytes) = clips.kept();
        eprintln!(
            "platinum, save {}, cap {:.0} MiB, {frames} frames played",
            saved.is_some(),
            mib(cap(ConsoleKind::Nds))
        );
        eprintln!(
            "rewind 2 min: {rewind_frames} frames, {:.1} MiB now, {:.1} MiB peak",
            mib(rewind_bytes),
            mib(rewind_peak)
        );
        eprintln!(
            "clips 5 min: {clips_frames} frames, {:.1} MiB now, {:.1} MiB peak",
            mib(clips_bytes),
            mib(clips_peak)
        );
    }
}
