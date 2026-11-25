use crate::file::{Event, Header, Input, Replay};
use citrine_gb::cheats::Cheat;
use citrine_gb::console::{Console, Keys, Touch};
use citrine_gb::error::{GbError, GbResult};
use citrine_gb::machine::Machine;
use std::borrow::Borrow;
use std::collections::VecDeque;
use std::sync::Arc;

pub const CHECK_EVERY: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub keyframe_every: u64,
    pub keep_frames: u64,
    pub cap: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark(u64);

impl Mark {
    pub fn frame(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Held {
    keys: Keys,
    touch: Option<Touch>,
    motion: (f32, f32, f32),
    sun: f32,
    camera: Option<Arc<[u8]>>,
    cheats: Arc<[String]>,
}

impl Held {
    pub fn take(&mut self, input: &Input) -> bool {
        match input {
            Input::Keys(bits) => {
                let keys = Keys::from_bits_retain(*bits);
                std::mem::replace(&mut self.keys, keys) != keys
            }
            Input::Touch(at) => {
                let touch = at.map(|(x, y)| Touch { x, y });
                std::mem::replace(&mut self.touch, touch) != touch
            }
            Input::Motion(x, y, turn) => {
                std::mem::replace(&mut self.motion, (*x, *y, *turn)) != (*x, *y, *turn)
            }
            Input::Sun(light) => std::mem::replace(&mut self.sun, *light) != *light,
            Input::Camera(frame) => {
                if self.camera.as_deref() == Some(&frame[..]) {
                    return false;
                }
                self.camera = Some(frame.as_slice().into());
                true
            }
            Input::Cheats(codes) => {
                if self.cheats[..] == codes[..] {
                    return false;
                }
                self.cheats = codes.as_slice().into();
                true
            }
            Input::Mic(samples) => !samples.is_empty(),
            Input::Reset | Input::HoldClock(_) | Input::SetClock(_) | Input::ShiftClock(..) => true,
        }
    }

    fn changes_from(&self, old: &Held) -> Vec<Input> {
        let mut inputs = Vec::new();
        if self.keys != old.keys {
            inputs.push(Input::Keys(self.keys.bits()));
        }
        if self.touch != old.touch {
            inputs.push(Input::Touch(self.touch.map(|touch| (touch.x, touch.y))));
        }
        if self.motion != old.motion {
            let (x, y, turn) = self.motion;
            inputs.push(Input::Motion(x, y, turn));
        }
        if self.sun != old.sun {
            inputs.push(Input::Sun(self.sun));
        }
        if self.camera != old.camera
            && let Some(camera) = &self.camera
        {
            inputs.push(Input::Camera(camera.to_vec()));
        }
        if self.cheats != old.cheats {
            inputs.push(Input::Cheats(self.cheats.to_vec()));
        }
        inputs
    }

    pub fn inputs(&self) -> Vec<Input> {
        let mut inputs = vec![Input::Keys(self.keys.bits())];
        if let Some(touch) = self.touch {
            inputs.push(Input::Touch(Some((touch.x, touch.y))));
        }
        if self.motion != (0.0, 0.0, 0.0) {
            let (x, y, turn) = self.motion;
            inputs.push(Input::Motion(x, y, turn));
        }
        if self.sun != 0.0 {
            inputs.push(Input::Sun(self.sun));
        }
        if let Some(camera) = &self.camera {
            inputs.push(Input::Camera(camera.to_vec()));
        }
        if !self.cheats.is_empty() {
            inputs.push(Input::Cheats(self.cheats.to_vec()));
        }
        inputs
    }
}

pub fn apply(machine: &mut Machine, input: &Input) {
    match input {
        Input::Keys(bits) => machine.set_keys(Keys::from_bits_retain(*bits)),
        Input::Touch(at) => machine.set_touch(at.map(|(x, y)| Touch { x, y })),
        Input::Reset => machine.soft_reset(),
        Input::Mic(samples) => machine.set_mic(samples),
        Input::Motion(x, y, turn) => machine.set_motion(*x, *y, *turn),
        Input::Sun(light) => machine.set_sunlight(*light),
        Input::Camera(frame) => machine.set_camera_frame(frame),
        Input::Cheats(_) => {}
        Input::HoldClock(secs) => {
            machine.hold_clock(*secs);
        }
        Input::SetClock(local) => machine.set_local_clock(*local),
        Input::ShiftClock(seconds, now) => machine.shift_clock(*seconds, *now),
    }
}

struct Feeder {
    cheats: Vec<Cheat>,
    mic: bool,
}

impl Feeder {
    fn new(machine: &mut Machine, held: &Held) -> Self {
        let mut feeder = Self {
            cheats: Vec::new(),
            mic: false,
        };
        for input in held.inputs() {
            feeder.take(machine, &input);
        }
        feeder
    }

    fn take(&mut self, machine: &mut Machine, input: &Input) {
        match input {
            Input::Cheats(codes) => {
                let kind = machine.kind();
                self.cheats = codes
                    .iter()
                    .filter_map(|code| Cheat::parse(kind, code).ok())
                    .collect();
            }
            Input::Mic(_) => self.mic = true,
            _ => {}
        }
        apply(machine, input);
    }

    fn run_frame(&mut self, machine: &mut Machine) {
        self.ready(machine);
        machine.run_frame();
    }

    fn ready(&mut self, machine: &mut Machine) {
        if !std::mem::take(&mut self.mic) {
            machine.set_mic(&[]);
        }
        machine.apply_cheats(&self.cheats);
    }
}

struct Keyframe {
    frame: u64,
    held: Held,
    packed: Arc<[u8]>,
}

struct Pinned {
    frame: u64,
    save: Vec<u8>,
    rewind: Vec<u8>,
    held: Held,
    header: Header,
}

impl Header {
    pub fn of(machine: &Machine, now: u64) -> Self {
        let id = machine.rom_id();
        Self {
            console: format!("{:?}", machine.kind()),
            game: machine.title(),
            crc32: id.crc32,
            sha1: id.sha1.iter().map(|b| format!("{b:02x}")).collect(),
            start_unix: now,
            frame_rate: 1.0 / machine.timing().frame_secs(),
            ..Self::default()
        }
    }
}

pub struct Tape {
    limits: Limits,
    frame: u64,
    held: Held,
    events: VecDeque<Event>,
    checks: VecDeque<(u64, u64)>,
    keyframes: VecDeque<Keyframe>,
    marks: Vec<Pinned>,
    scratch: Vec<u8>,
}

impl Tape {
    pub fn start(machine: &mut Machine, limits: Limits, held: Held) -> GbResult<Self> {
        let mut tape = Self {
            limits,
            frame: 0,
            held,
            events: VecDeque::new(),
            checks: VecDeque::new(),
            keyframes: VecDeque::new(),
            marks: Vec::new(),
            scratch: Vec::new(),
        };
        tape.keyframe(machine)?;
        Ok(tape)
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
        self.trim();
    }

    pub fn feed(&mut self, machine: &mut Machine, input: Input) {
        apply(machine, &input);
        self.record(input);
    }

    pub fn hold(&mut self, machine: &mut Machine, held: &Held) {
        for input in held.changes_from(&self.held) {
            self.feed(machine, input);
        }
    }

    pub fn record(&mut self, input: Input) {
        if self.held.take(&input) {
            self.events.push_back(Event {
                frame: self.frame,
                input,
            });
        }
    }

    pub fn after_frame(&mut self, machine: &mut Machine) -> GbResult<()> {
        self.frame += 1;
        if self.frame.is_multiple_of(CHECK_EVERY) {
            self.checks.push_back((self.frame, machine.state_hash()));
        }
        if self.frame.is_multiple_of(self.limits.keyframe_every) {
            self.keyframe(machine)?;
        }
        self.trim();
        Ok(())
    }

    pub fn mark(&mut self, machine: &mut Machine, now: u64) -> GbResult<Mark> {
        machine.finish_picture();
        let save = machine.save_state(now)?;
        let rewind = machine.rewind_state()?;
        let header = Header::of(machine, now);
        self.marks.retain(|pinned| pinned.frame != self.frame);
        self.marks.push(Pinned {
            frame: self.frame,
            save,
            rewind,
            held: self.held.clone(),
            header,
        });
        Ok(Mark(self.frame))
    }

    pub fn unmark(&mut self, mark: Mark) {
        self.marks.retain(|pinned| pinned.frame != mark.0);
        self.trim();
    }

    pub fn restart(&mut self, machine: &mut Machine, mark: Mark) -> GbResult<bool> {
        let Some(pinned) = self.marks.iter().find(|pinned| pinned.frame == mark.0) else {
            return Ok(false);
        };
        machine.rewind_to(&pinned.rewind)?;
        self.frame = pinned.frame;
        self.held = pinned.held.clone();
        let frame = self.frame;
        self.events.retain(|event| event.frame < frame);
        self.checks.retain(|(at, _)| *at <= frame);
        self.keyframes.retain(|keyframe| keyframe.frame <= frame);
        self.marks.retain(|pinned| pinned.frame <= frame);
        Ok(true)
    }

    pub fn cut(&self, from: Mark, machine: &Machine) -> Option<Replay> {
        let pinned = self.marks.iter().find(|pinned| pinned.frame == from.0)?;
        Some(self.cut_from(
            pinned.frame,
            &pinned.held,
            pinned.save.clone(),
            pinned.header.clone(),
            machine,
        ))
    }

    pub fn cut_from(
        &self,
        frame: u64,
        held: &Held,
        save: Vec<u8>,
        header: Header,
        machine: &Machine,
    ) -> Replay {
        let span = Span {
            from: frame,
            to: self.frame.max(frame),
            lead: held.inputs(),
            closed: true,
        };
        span.replay(
            save,
            header,
            self.events.iter(),
            self.checks.iter().copied(),
            machine.state_hash(),
        )
    }

    pub fn clip(&self, machine: &Machine, back: u64) -> Option<Clip> {
        let first = self
            .keyframe_before(self.frame.saturating_sub(back))
            .unwrap_or(0);
        let keyframe = self.keyframes.get(first)?;
        let start = self
            .events
            .partition_point(|event| event.frame < keyframe.frame);
        Some(Clip {
            from: keyframe.frame,
            to: self.frame,
            held: keyframe.held.clone(),
            packed: keyframe.packed.clone(),
            keyframes: self
                .keyframes
                .iter()
                .skip(first)
                .map(|keyframe| {
                    (
                        keyframe.frame,
                        keyframe.held.clone(),
                        keyframe.packed.clone(),
                    )
                })
                .collect(),
            events: self.events.range(start..).cloned().collect(),
            checks: self.checks.iter().copied().collect(),
            last_hash: machine.state_hash(),
        })
    }

    fn keyframe(&mut self, machine: &mut Machine) -> GbResult<()> {
        machine.finish_picture();
        machine.rewind_state_into(&mut self.scratch)?;
        self.keyframes.push_back(Keyframe {
            frame: self.frame,
            held: self.held.clone(),
            packed: lz4_flex::compress_prepend_size(&self.scratch).into(),
        });
        Ok(())
    }

    fn trim(&mut self) {
        let oldest = self.frame.saturating_sub(self.limits.keep_frames);
        while self.keyframes.len() > 1 && self.keyframes[1].frame <= oldest {
            self.keyframes.pop_front();
        }
        let mut packed: usize = self.keyframes.iter().map(|k| k.packed.len()).sum();
        while self.keyframes.len() > 1 && packed > self.limits.cap {
            if let Some(keyframe) = self.keyframes.pop_front() {
                packed -= keyframe.packed.len();
            }
        }
        let floor = self
            .marks
            .iter()
            .map(|pinned| pinned.frame)
            .chain(self.keyframes.front().map(|keyframe| keyframe.frame))
            .min()
            .unwrap_or(self.frame);
        while self.events.front().is_some_and(|event| event.frame < floor) {
            self.events.pop_front();
        }
        while self.checks.front().is_some_and(|(at, _)| *at <= floor) {
            self.checks.pop_front();
        }
    }

    pub fn oldest(&self) -> u64 {
        self.keyframes
            .front()
            .map_or(self.frame, |keyframe| keyframe.frame)
    }

    pub fn gap(&self, to: u64) -> Option<Gap> {
        if to > self.frame {
            return None;
        }
        let keyframe = self
            .keyframes
            .iter()
            .rfind(|keyframe| keyframe.frame < to)?;
        let start = self
            .events
            .partition_point(|event| event.frame < keyframe.frame);
        Some(Gap {
            from: keyframe.frame,
            to,
            held: keyframe.held.clone(),
            packed: keyframe.packed.clone(),
            events: self
                .events
                .range(start..)
                .take_while(|event| event.frame < to)
                .cloned()
                .collect(),
        })
    }

    pub fn landing(&self, at: u64) -> Option<u64> {
        self.keyframe_before(at)
            .map(|index| self.keyframes[index].frame)
    }

    pub fn land(
        &mut self,
        machine: &mut Machine,
        at: u64,
        mut each: impl FnMut(u64, &mut Machine),
    ) -> GbResult<bool> {
        let Some(index) = self.keyframe_before(at) else {
            return Ok(false);
        };
        self.held = self.run(machine, index, at, &mut each)?;
        self.frame = at;
        self.events.retain(|event| event.frame < at);
        self.checks.retain(|(frame, _)| *frame <= at);
        self.keyframes.truncate(index + 1);
        self.marks.retain(|pinned| pinned.frame <= at);
        Ok(true)
    }

    fn keyframe_before(&self, at: u64) -> Option<usize> {
        if at > self.frame {
            return None;
        }
        self.keyframes
            .iter()
            .rposition(|keyframe| keyframe.frame <= at)
    }

    fn run(
        &self,
        machine: &mut Machine,
        index: usize,
        to: u64,
        each: &mut impl FnMut(u64, &mut Machine),
    ) -> GbResult<Held> {
        let keyframe = &self.keyframes[index];
        let start = self
            .events
            .partition_point(|event| event.frame < keyframe.frame);
        run_gap(
            machine,
            &keyframe.packed,
            &keyframe.held,
            self.events.range(start..),
            keyframe.frame..to,
            each,
        )
    }

    pub fn bytes(&self) -> usize {
        self.keyframes.iter().map(|k| k.packed.len()).sum::<usize>()
            + self.events.iter().map(event_bytes).sum::<usize>()
            + self.checks.len() * 16
            + self
                .marks
                .iter()
                .map(|pinned| pinned.save.len() + pinned.rewind.len())
                .sum::<usize>()
    }
}

struct Span {
    from: u64,
    to: u64,
    lead: Vec<Input>,
    closed: bool,
}

impl Span {
    fn replay<'e>(
        &self,
        start: Vec<u8>,
        header: Header,
        events: impl Iterator<Item = &'e Event>,
        checks: impl Iterator<Item = (u64, u64)>,
        last_hash: u64,
    ) -> Replay {
        let (from, to, closed) = (self.from, self.to, self.closed);
        let mut kept: Vec<Event> = self
            .lead
            .iter()
            .map(|input| Event {
                frame: 0,
                input: input.clone(),
            })
            .collect();
        kept.extend(
            events
                .filter(|event| (from..to).contains(&event.frame) || (closed && event.frame == to))
                .map(|event| Event {
                    frame: event.frame - from,
                    input: event.input.clone(),
                }),
        );
        let checks: Vec<(u64, u64)> = checks.filter(|(at, _)| *at > from && *at <= to).collect();
        Replay {
            header: Header {
                frames: to - from,
                check_from: checks.first().map_or(0, |(at, _)| at - from),
                last_hash,
                ..header
            },
            start,
            events: kept,
            checks: checks.iter().map(|(_, hash)| *hash).collect(),
            perf: None,
            video: None,
            thumb: None,
        }
    }
}

pub struct Clip {
    from: u64,
    to: u64,
    held: Held,
    packed: Arc<[u8]>,
    keyframes: Vec<(u64, Held, Arc<[u8]>)>,
    events: Vec<Event>,
    checks: Vec<(u64, u64)>,
    last_hash: u64,
}

impl Clip {
    pub fn frames(&self) -> u64 {
        self.to - self.from
    }

    pub fn seek_points(&self, replay: &Replay) -> SeekPoints {
        let check_from = replay.header.check_from;
        let keys = self
            .keyframes
            .iter()
            .filter(|(frame, _, _)| (self.from..self.to).contains(frame))
            .map(|(frame, held, packed)| {
                let at = frame - self.from;
                let check = if check_from == 0 || at < check_from {
                    0
                } else {
                    ((at - check_from) / CHECK_EVERY + 1) as usize
                };
                SeekKey {
                    frame: at,
                    next: replay.events.partition_point(|event| event.frame < at),
                    check: check.min(replay.checks.len()),
                    held: held.clone(),
                    packed: packed.clone(),
                }
            })
            .collect();
        SeekPoints {
            every: CHECK_EVERY,
            keys,
        }
    }

    pub fn cut(&self, twin: &mut Machine, now: u64) -> GbResult<Replay> {
        let state =
            lz4_flex::decompress_size_prepended(&self.packed).map_err(|_| GbError::DamagedState)?;
        twin.rewind_to(&state)?;
        let save = twin.save_state(now)?;
        let span = Span {
            from: self.from,
            to: self.to,
            lead: self.held.inputs(),
            closed: true,
        };
        Ok(span.replay(
            save,
            Header::of(twin, now),
            self.events.iter(),
            self.checks.iter().copied(),
            self.last_hash,
        ))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CropError {
    #[error("the crop starts after it ends or before the player")]
    Range,
    #[error(transparent)]
    Diverged(#[from] Diverged),
    #[error(transparent)]
    Machine(#[from] GbError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the replay went another way by frame {frame}")]
pub struct Diverged {
    pub frame: u64,
    pub expected: u64,
    pub found: u64,
}

pub struct Gap {
    from: u64,
    to: u64,
    held: Held,
    packed: Arc<[u8]>,
    events: Vec<Event>,
}

impl Gap {
    pub fn from(&self) -> u64 {
        self.from
    }

    pub fn to(&self) -> u64 {
        self.to
    }

    pub fn run(
        &self,
        machine: &mut Machine,
        mut each: impl FnMut(u64, &mut Machine),
    ) -> GbResult<()> {
        run_gap(
            machine,
            &self.packed,
            &self.held,
            self.events.iter(),
            self.from..self.to,
            &mut each,
        )
        .map(|_| ())
    }
}

fn run_gap<'a>(
    machine: &mut Machine,
    packed: &[u8],
    held: &Held,
    events: impl Iterator<Item = &'a Event>,
    frames: std::ops::Range<u64>,
    each: &mut impl FnMut(u64, &mut Machine),
) -> GbResult<Held> {
    let state = lz4_flex::decompress_size_prepended(packed).map_err(|_| GbError::DamagedState)?;
    machine.rewind_to(&state)?;
    let mut held = held.clone();
    let mut feeder = Feeder::new(machine, &held);
    let mut events = events.peekable();
    for frame in frames {
        while let Some(event) = events.next_if(|event| event.frame == frame) {
            held.take(&event.input);
            feeder.take(machine, &event.input);
        }
        feeder.run_frame(machine);
        machine.take_audio(&mut Vec::new());
        each(frame + 1, machine);
    }
    Ok(held)
}

fn event_bytes(event: &Event) -> usize {
    std::mem::size_of::<Event>()
        + match &event.input {
            Input::Mic(samples) => samples.len() * 2,
            Input::Camera(frame) => frame.len(),
            Input::Cheats(codes) => codes.iter().map(String::len).sum(),
            _ => 0,
        }
}

pub struct Player<R: Borrow<Replay>> {
    replay: R,
    played: u64,
    next: usize,
    check: usize,
    feeder: Feeder,
    held: Held,
    seek: Option<Seek>,
}

struct Seek {
    every: u64,
    cap: usize,
    keys: Vec<SeekKey>,
    scratch: Vec<u8>,
}

#[derive(Clone)]
struct SeekKey {
    frame: u64,
    next: usize,
    check: usize,
    held: Held,
    packed: Arc<[u8]>,
}

#[derive(Clone, Default)]
pub struct SeekPoints {
    every: u64,
    keys: Vec<SeekKey>,
}

impl SeekPoints {
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn pictures(&self, machine: &mut Machine, count: usize) -> GbResult<Vec<(u64, Vec<u8>)>> {
        let count = count.min(self.keys.len());
        let mut pictures = Vec::with_capacity(count);
        for index in 0..count {
            let key = &self.keys[index * self.keys.len() / count];
            let state = lz4_flex::decompress_size_prepended(&key.packed)
                .map_err(|_| GbError::DamagedState)?;
            machine.rewind_to(&state)?;
            pictures.push((key.frame, machine.frame().to_vec()));
        }
        Ok(pictures)
    }
}

impl Seek {
    fn bytes(&self) -> usize {
        self.keys.iter().map(|key| key.packed.len()).sum()
    }

    fn thin(&mut self) {
        while self.keys.len() > 2 && self.bytes() > self.cap {
            let mut index = 0;
            self.keys.retain(|_| {
                index += 1;
                index % 2 == 1
            });
            self.every *= 2;
        }
    }
}

impl<R: Borrow<Replay>> Player<R> {
    pub fn new(replay: R) -> Self {
        Self {
            replay,
            played: 0,
            next: 0,
            check: 0,
            feeder: Feeder {
                cheats: Vec::new(),
                mic: false,
            },
            held: Held::default(),
            seek: None,
        }
    }

    pub fn seekable(mut self, cap: usize) -> Self {
        self.seek = Some(Seek {
            every: CHECK_EVERY,
            cap,
            keys: Vec::new(),
            scratch: Vec::new(),
        });
        self
    }

    pub fn seekable_from(mut self, cap: usize, points: SeekPoints) -> Self {
        self.seek = Some(Seek {
            every: points.every.max(CHECK_EVERY),
            cap,
            keys: points.keys,
            scratch: Vec::new(),
        });
        self
    }

    pub fn seek_points(&self) -> SeekPoints {
        self.seek
            .as_ref()
            .map_or_else(SeekPoints::default, |seek| SeekPoints {
                every: seek.every,
                keys: seek.keys.clone(),
            })
    }

    pub fn seek_bytes(&self) -> usize {
        self.seek.as_ref().map_or(0, Seek::bytes)
    }

    pub fn seek_every(&self) -> Option<u64> {
        self.seek.as_ref().map(|seek| seek.every)
    }

    pub fn keeps_next_frame(&self) -> bool {
        let next = self.played + 1;
        self.seek.as_ref().is_some_and(|seek| {
            next.is_multiple_of(seek.every) && seek.keys.last().is_none_or(|key| key.frame < next)
        })
    }

    pub fn seek(&mut self, machine: &mut Machine, to: u64) -> GbResult<()> {
        let to = to.clamp(1, self.frames().max(1));
        let Some(seek) = &self.seek else {
            return Ok(());
        };
        let Some(key) = seek.keys.iter().rfind(|key| key.frame < to) else {
            return Ok(());
        };
        if to > self.played && key.frame <= self.played {
            return Ok(());
        }
        let state =
            lz4_flex::decompress_size_prepended(&key.packed).map_err(|_| GbError::DamagedState)?;
        machine.rewind_to(&state)?;
        self.feeder = Feeder::new(machine, &key.held);
        self.held = key.held.clone();
        self.played = key.frame;
        self.next = key.next;
        self.check = key.check;
        Ok(())
    }

    fn keep(&mut self, machine: &mut Machine) -> GbResult<()> {
        let Some(seek) = &mut self.seek else {
            return Ok(());
        };
        if !self.played.is_multiple_of(seek.every)
            || seek.keys.last().is_some_and(|key| key.frame >= self.played)
        {
            return Ok(());
        }
        machine.finish_picture();
        machine.rewind_state_into(&mut seek.scratch)?;
        seek.keys.push(SeekKey {
            frame: self.played,
            next: self.next,
            check: self.check,
            held: self.held.clone(),
            packed: lz4_flex::compress_prepend_size(&seek.scratch).into(),
        });
        seek.thin();
        Ok(())
    }

    pub fn held(&self) -> &Held {
        &self.held
    }

    pub fn crop(
        &mut self,
        machine: &mut Machine,
        from: u64,
        to: u64,
        now: u64,
    ) -> Result<Replay, CropError> {
        let to = to.min(self.frames());
        if from >= to || self.played > from {
            return Err(CropError::Range);
        }
        while self.played < from {
            self.step(machine)?;
            machine.take_audio(&mut Vec::new());
        }
        let replay = self.replay.borrow();
        let (start, header) = if from == 0 {
            (replay.start.clone(), replay.header.clone())
        } else {
            machine.finish_picture();
            let save = machine.save_state(now)?;
            let header = Header {
                start_unix: now,
                ..replay.header.clone()
            };
            (save, header)
        };
        let lead = if from == 0 {
            Vec::new()
        } else {
            self.held.inputs()
        };
        let check_from = replay.header.check_from;
        let checks: Vec<(u64, u64)> = replay
            .checks
            .iter()
            .enumerate()
            .filter(|_| check_from > 0)
            .map(|(index, hash)| (check_from + index as u64 * CHECK_EVERY, *hash))
            .collect();
        while self.played < to {
            self.step(machine)?;
            machine.take_audio(&mut Vec::new());
        }
        let replay = self.replay.borrow();
        let span = Span {
            from,
            to,
            lead,
            closed: to == replay.header.frames,
        };
        Ok(span.replay(
            start,
            header,
            replay.events.iter(),
            checks.into_iter(),
            machine.state_hash(),
        ))
    }

    pub fn played(&self) -> u64 {
        self.played
    }

    pub fn frames(&self) -> u64 {
        self.replay.borrow().header.frames
    }

    pub fn is_done(&self) -> bool {
        self.played >= self.frames()
    }

    pub fn replay(&self) -> &R {
        &self.replay
    }

    pub fn into_replay(self) -> R {
        self.replay
    }

    pub fn step(&mut self, machine: &mut Machine) -> Result<(), Diverged> {
        self.step_with(machine, Machine::run_frame)
    }

    pub fn step_with(
        &mut self,
        machine: &mut Machine,
        run: impl FnOnce(&mut Machine),
    ) -> Result<(), Diverged> {
        if self.keep(machine).is_err() {
            self.seek = None;
        }
        let replay = self.replay.borrow();
        while let Some(event) = replay
            .events
            .get(self.next)
            .filter(|e| e.frame == self.played)
        {
            self.held.take(&event.input);
            self.feeder.take(machine, &event.input);
            self.next += 1;
        }
        if self.seek.is_some() {
            machine.set_keys(self.held.keys);
            machine.set_touch(self.held.touch);
        }
        self.feeder.ready(machine);
        run(machine);
        self.played += 1;
        let header = &replay.header;
        if self.played >= header.check_from
            && header.check_from > 0
            && (self.played - header.check_from).is_multiple_of(CHECK_EVERY)
            && let Some(&expected) = replay.checks.get(self.check)
        {
            self.check += 1;
            self.compare(expected, machine)?;
        }
        if self.played == header.frames {
            while let Some(event) = replay
                .events
                .get(self.next)
                .filter(|e| e.frame == self.played)
            {
                self.held.take(&event.input);
                self.feeder.take(machine, &event.input);
                self.next += 1;
            }
            self.compare(header.last_hash, machine)?;
        }
        Ok(())
    }

    fn compare(&self, expected: u64, machine: &Machine) -> Result<(), Diverged> {
        let found = machine.state_hash();
        if found == expected {
            return Ok(());
        }
        Err(Diverged {
            frame: self.played,
            expected,
            found,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb::console::ConsoleKind;
    use citrine_gb::machine::Firmware;
    use citrine_gb::rom::Rom;

    const NOW: u64 = 1_780_000_000;
    const LIMITS: Limits = Limits {
        keyframe_every: 60,
        keep_frames: 600,
        cap: usize::MAX,
    };
    const SNAKE: &[u8] = include_bytes!("../../citrine-gb-app/homebrew/snake.gb");
    const PETRIS: &[u8] = include_bytes!("../../citrine-gb-app/homebrew/petris.gbc");
    #[cfg(feature = "gba")]
    const PENTRIS: &[u8] = include_bytes!("../../citrine-gb-app/homebrew/gba/pentris.gba.br");
    #[cfg(feature = "nds")]
    const CUBE: &[u8] =
        include_bytes!("../../citrine-gb-app/fixtures/nds/blocksds-lighting_cube.nds");

    fn game(bytes: &[u8], kind: ConsoleKind) -> (Rom, Machine) {
        let rom = Rom::new(bytes);
        let machine = Machine::load(&rom, kind, &Firmware::default()).unwrap();
        (rom, machine)
    }

    #[cfg(feature = "gba")]
    fn unbrotli(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut brotli::Decompressor::new(bytes, 4096), &mut out).unwrap();
        out
    }

    struct Script {
        keys: Keys,
        touch: Option<Touch>,
        reset: bool,
    }

    fn controls(frame: u64, touch: bool) -> Script {
        let keys = match frame % 90 {
            0..10 => Keys::START,
            20..30 => Keys::A,
            40..55 => Keys::RIGHT | Keys::B,
            60..70 => Keys::DOWN,
            _ => Keys::empty(),
        };
        let touch = (touch && frame % 50 < 12).then_some(Touch {
            x: (frame % 256) as u8,
            y: (frame % 192) as u8,
        });
        Script {
            keys,
            touch,
            reset: frame == 250,
        }
    }

    fn play(machine: &mut Machine, tape: &mut Tape, from: u64, frames: u64, touch: bool) {
        play_fed(machine, tape, from, frames, touch, &mut |_, _, _| {});
    }

    fn play_fed(
        machine: &mut Machine,
        tape: &mut Tape,
        from: u64,
        frames: u64,
        touch: bool,
        fed: &mut impl FnMut(u64, &mut Machine, &mut Tape),
    ) {
        for frame in from..from + frames {
            let now = controls(frame, touch);
            if now.reset {
                tape.feed(machine, Input::Reset);
            }
            tape.feed(machine, Input::Keys(now.keys.bits()));
            tape.feed(machine, Input::Touch(now.touch.map(|at| (at.x, at.y))));
            tape.feed(machine, Input::Mic(Vec::new()));
            fed(frame, machine, tape);
            let kind = machine.kind();
            let cheats: Vec<Cheat> = tape
                .held
                .cheats
                .iter()
                .filter_map(|code| Cheat::parse(kind, code).ok())
                .collect();
            machine.apply_cheats(&cheats);
            machine.run_frame();
            machine.take_audio(&mut Vec::new());
            tape.after_frame(machine).unwrap();
        }
    }

    fn records_and_plays_back(
        rom: Rom,
        mut machine: Machine,
        touch: bool,
        fed: &mut impl FnMut(u64, &mut Machine, &mut Tape),
    ) -> Replay {
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        play_fed(&mut machine, &mut tape, 0, 37, touch, fed);
        let mark = tape.mark(&mut machine, NOW).unwrap();
        play_fed(&mut machine, &mut tape, 37, 400, touch, fed);
        let replay = tape.cut(mark, &machine).unwrap();
        let replay = Replay::read(&replay.write().unwrap()).unwrap();
        let back = played_back(&replay, &rom).unwrap();
        assert_eq!(back.state_hash(), machine.state_hash());
        replay
    }

    fn has(replay: &Replay, wanted: impl Fn(&Input) -> bool) -> bool {
        replay.events.iter().any(|event| wanted(&event.input))
    }

    #[test]
    fn cheats_turned_on_before_the_mark_play_back() {
        let (rom, machine) = game(SNAKE, ConsoleKind::Gb);
        let replay = records_and_plays_back(rom, machine, false, &mut |frame, machine, tape| {
            if frame == 10 {
                tape.feed(machine, Input::Cheats(vec!["01FFA0C0".into()]));
            }
            if frame == 200 {
                tape.feed(machine, Input::Cheats(Vec::new()));
            }
        });
        assert!(has(
            &replay,
            |input| matches!(input, Input::Cheats(codes) if codes.len() == 1)
        ));
        assert!(has(
            &replay,
            |input| matches!(input, Input::Cheats(codes) if codes.is_empty())
        ));
    }

    #[test]
    fn a_cheat_left_out_of_the_replay_is_caught() {
        let (rom, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        let mark = tape.mark(&mut machine, NOW).unwrap();
        play_fed(
            &mut machine,
            &mut tape,
            0,
            120,
            false,
            &mut |frame, machine, tape| {
                if frame == 10 {
                    tape.feed(machine, Input::Cheats(vec!["01FFA0C0".into()]));
                }
            },
        );
        let mut replay = tape.cut(mark, &machine).unwrap();
        replay
            .events
            .retain(|event| !matches!(event.input, Input::Cheats(_)));
        assert!(played_back(&replay, &rom).is_err());
    }

    fn clock_rom() -> Vec<u8> {
        let mut data = vec![0u8; 0x8000];
        data[0x0147] = 0x10;
        data[0x0149] = 0x02;
        data
    }

    #[test]
    fn clock_holds_and_jumps_play_back() {
        let bytes = clock_rom();
        let (rom, machine) = game(&bytes, ConsoleKind::Gb);
        let replay = records_and_plays_back(rom, machine, false, &mut |frame, machine, tape| {
            if frame.is_multiple_of(7) {
                let secs = 0.004;
                machine.hold_clock(secs);
                tape.record(Input::HoldClock(secs));
            }
            if frame == 120 {
                tape.feed(machine, Input::ShiftClock(3 * 3600, NOW));
            }
            if frame == 300 {
                tape.feed(machine, Input::ShiftClock(-60, NOW));
            }
        });
        assert!(has(&replay, |input| matches!(input, Input::ShiftClock(..))));
        assert!(has(&replay, |input| matches!(input, Input::HoldClock(_))));
    }

    #[test]
    fn inputs_fed_after_the_last_frame_play_back_before_the_last_check() {
        let (rom, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        play(&mut machine, &mut tape, 0, 37, false);
        let mark = tape.mark(&mut machine, NOW).unwrap();
        play(&mut machine, &mut tape, 37, 100, false);
        tape.feed(&mut machine, Input::Keys(Keys::A.bits()));
        let replay = tape.cut(mark, &machine).unwrap();
        let replay = Replay::read(&replay.write().unwrap()).unwrap();
        let back = played_back(&replay, &rom).unwrap();
        assert_eq!(back.state_hash(), machine.state_hash());

        let mut twin = Machine::from_state(
            &replay.start,
            &rom,
            replay.header.start_unix,
            &Firmware::default(),
        )
        .unwrap();
        let crop = Player::new(&replay)
            .crop(&mut twin, 0, replay.header.frames, NOW)
            .unwrap();
        assert!(played_back(&crop, &rom).is_ok());
    }

    #[test]
    fn a_clock_jump_left_out_of_the_replay_is_caught() {
        let bytes = clock_rom();
        let (rom, mut machine) = game(&bytes, ConsoleKind::Gb);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        let mark = tape.mark(&mut machine, NOW).unwrap();
        play_fed(
            &mut machine,
            &mut tape,
            0,
            120,
            false,
            &mut |frame, machine, tape| {
                if frame == 10 {
                    tape.feed(machine, Input::ShiftClock(3600, NOW));
                }
            },
        );
        let mut replay = tape.cut(mark, &machine).unwrap();
        replay
            .events
            .retain(|event| !matches!(event.input, Input::ShiftClock(..)));
        assert!(played_back(&replay, &rom).is_err());
    }

    #[test]
    #[cfg(feature = "gba")]
    fn gba_tilt_turn_and_sunlight_play_back() {
        let bytes = unbrotli(PENTRIS);
        let (rom, machine) = game(&bytes, ConsoleKind::Gba);
        let replay = records_and_plays_back(rom, machine, false, &mut |frame, machine, tape| {
            let x = ((frame / 20) % 5) as f32 * 0.25 - 0.5;
            tape.feed(machine, Input::Motion(x, -x, x * 0.5));
            tape.feed(machine, Input::Sun(((frame / 40) % 3) as f32 * 0.5));
        });
        let motions = replay
            .events
            .iter()
            .filter(|event| matches!(event.input, Input::Motion(..)))
            .count();
        assert!((2..30).contains(&motions), "{motions}");
        assert!(has(&replay, |input| matches!(input, Input::Sun(_))));
    }

    #[test]
    #[cfg(feature = "nds")]
    fn ds_microphone_and_clock_play_back() {
        let (rom, machine) = game(CUBE, ConsoleKind::Nds);
        let replay = records_and_plays_back(rom, machine, true, &mut |frame, machine, tape| {
            if (100..160).contains(&frame) {
                let samples = (0..800)
                    .map(|i| ((i as f32 + frame as f32 * 7.0) * 0.05).sin() * 9000.0)
                    .map(|sample| sample as i16)
                    .collect();
                tape.feed(machine, Input::Mic(samples));
            }
            if frame == 200 {
                tape.feed(machine, Input::SetClock(1_780_003_600));
            }
        });
        let mics = replay
            .events
            .iter()
            .filter(|event| matches!(event.input, Input::Mic(_)))
            .count();
        assert_eq!(mics, 60);
        assert!(has(&replay, |input| matches!(input, Input::SetClock(_))));
    }

    #[test]
    fn unchanged_held_inputs_are_kept_once() {
        let (_, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        for _ in 0..3 {
            tape.feed(&mut machine, Input::Camera(vec![7; 16]));
            tape.feed(&mut machine, Input::Motion(0.5, 0.0, 0.0));
            tape.feed(&mut machine, Input::Sun(0.25));
            tape.feed(&mut machine, Input::Cheats(vec!["01FFA0C0".into()]));
            tape.feed(&mut machine, Input::Mic(Vec::new()));
            tape.after_frame(&mut machine).unwrap();
        }
        assert_eq!(tape.events.len(), 4);
    }

    fn played_back(replay: &Replay, rom: &Rom) -> Result<Machine, Diverged> {
        let mut machine = Machine::from_state(
            &replay.start,
            rom,
            replay.header.start_unix,
            &Firmware::default(),
        )
        .unwrap();
        let mut player = Player::new(replay);
        while !player.is_done() {
            player.step(&mut machine)?;
            machine.take_audio(&mut Vec::new());
        }
        Ok(machine)
    }

    fn plays_back_what_was_played(rom: Rom, machine: Machine, touch: bool) {
        plays_back_frames(rom, machine, touch, 400);
    }

    fn plays_back_frames(rom: Rom, mut machine: Machine, touch: bool, frames: u64) {
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        play(&mut machine, &mut tape, 0, 37, touch);
        let mark = tape.mark(&mut machine, NOW).unwrap();
        play(&mut machine, &mut tape, 37, frames, touch);
        let replay = tape.cut(mark, &machine).unwrap();
        let replay = Replay::read(&replay.write().unwrap()).unwrap();
        assert_eq!(replay.header.frames, frames);
        assert_eq!(replay.header.check_from, 23);
        assert_eq!(replay.checks.len() as u64, (37 + frames) / CHECK_EVERY);

        let back = played_back(&replay, &rom).unwrap();
        assert_eq!(back.state_hash(), machine.state_hash());
        assert!(back.frame() == machine.frame());
    }

    #[test]
    fn a_game_boy_replay_plays_back_what_was_played() {
        let (rom, machine) = game(SNAKE, ConsoleKind::Gb);
        plays_back_what_was_played(rom, machine, false);
    }

    #[test]
    fn a_game_boy_color_replay_plays_back_what_was_played() {
        let (rom, machine) = game(PETRIS, ConsoleKind::Gbc);
        plays_back_what_was_played(rom, machine, false);
    }

    #[test]
    #[cfg(feature = "gba")]
    fn a_gba_replay_plays_back_what_was_played() {
        let (rom, machine) = game(&unbrotli(PENTRIS), ConsoleKind::Gba);
        plays_back_what_was_played(rom, machine, false);
    }

    #[test]
    #[cfg(feature = "nds")]
    fn a_ds_replay_with_touch_plays_back_what_was_played() {
        let (rom, machine) = game(CUBE, ConsoleKind::Nds);
        plays_back_what_was_played(rom, machine, true);
    }

    fn commercial(test: &str, file: &str, kind: ConsoleKind) {
        let path = citrine_gb_test_roms::roms_dir().join(file);
        let Some(bytes) = citrine_gb_test_roms::read(test, path) else {
            return;
        };
        let (rom, machine) = game(&bytes, kind);
        plays_back_frames(rom, machine, false, 3600);
    }

    #[test]
    fn a_pokemon_crystal_replay_plays_back_what_was_played() {
        commercial(
            "a_pokemon_crystal_replay_plays_back_what_was_played",
            "crystal.gbc",
            ConsoleKind::Gbc,
        );
    }

    #[test]
    #[cfg(feature = "gba")]
    fn a_pokemon_emerald_replay_plays_back_what_was_played() {
        commercial(
            "a_pokemon_emerald_replay_plays_back_what_was_played",
            "emerald.gba",
            ConsoleKind::Gba,
        );
    }

    fn hashes_while_playing(
        machine: &mut Machine,
        tape: &mut Tape,
        frames: u64,
        touch: bool,
    ) -> Vec<u64> {
        let mut hashes = vec![machine.state_hash()];
        for frame in 0..frames {
            play(machine, tape, frame, 1, touch);
            hashes.push(machine.state_hash());
        }
        hashes
    }

    fn lands_back_and_plays_on_the_same(bytes: &[u8], kind: ConsoleKind, touch: bool) {
        let (_, mut machine) = game(bytes, kind);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        let hashes = hashes_while_playing(&mut machine, &mut tape, 300, touch);

        let worker = machine.twin();
        let gap = tape.gap(170).unwrap();
        assert_eq!((gap.from(), gap.to()), (120, 170));
        play(&mut machine, &mut tape, 300, 30, touch);
        let (gap, mut worker) = sent((gap, worker));
        let mut shown = Vec::new();
        gap.run(&mut worker, |frame, worker| {
            shown.push((frame, worker.state_hash()));
        })
        .unwrap();
        assert_eq!(shown.len(), 50);
        for (frame, hash) in shown {
            assert_eq!(hash, hashes[frame as usize], "frame {frame}");
        }

        assert!(tape.land(&mut machine, 180, |_, _| {}).unwrap());
        assert_eq!(tape.frame(), 180);
        assert_eq!(machine.state_hash(), hashes[180]);
        play(&mut machine, &mut tape, 180, 120, touch);
        assert_eq!(machine.state_hash(), hashes[300]);
    }

    #[test]
    fn a_game_boy_lands_back_and_plays_on_the_way_it_did() {
        lands_back_and_plays_on_the_same(SNAKE, ConsoleKind::Gb, false);
    }

    #[test]
    fn a_game_boy_color_lands_back_and_plays_on_the_way_it_did() {
        lands_back_and_plays_on_the_same(PETRIS, ConsoleKind::Gbc, false);
    }

    #[test]
    #[cfg(feature = "gba")]
    fn a_gba_lands_back_and_plays_on_the_way_it_did() {
        lands_back_and_plays_on_the_same(&unbrotli(PENTRIS), ConsoleKind::Gba, false);
    }

    #[test]
    #[cfg(feature = "nds")]
    fn a_ds_with_touch_lands_back_and_plays_on_the_way_it_did() {
        lands_back_and_plays_on_the_same(CUBE, ConsoleKind::Nds, true);
    }

    #[test]
    fn it_never_lands_before_the_oldest_keyframe_or_after_now() {
        let (_, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        play(&mut machine, &mut tape, 0, 1000, false);
        assert_eq!(tape.oldest(), 360);
        assert!(!tape.land(&mut machine, 359, |_, _| {}).unwrap());
        assert!(!tape.land(&mut machine, 1001, |_, _| {}).unwrap());
        assert_eq!(tape.frame(), 1000);
        assert_eq!(tape.landing(359), None);
        assert_eq!(tape.landing(1001), None);
        assert_eq!(tape.landing(360), Some(360));
        assert_eq!(tape.landing(1000), Some(960));
        assert!(tape.gap(360).is_none());
        assert!(tape.gap(1001).is_none());
        assert_eq!(tape.gap(361).map(|gap| gap.from()), Some(360));
        assert_eq!(tape.gap(1000).map(|gap| gap.from()), Some(960));
        assert!(tape.land(&mut machine, 360, |_, _| {}).unwrap());
        assert_eq!(tape.frame(), 360);
    }

    #[test]
    fn a_recording_rewound_inside_plays_back_what_was_kept() {
        let (rom, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        play(&mut machine, &mut tape, 0, 37, false);
        let mark = tape.mark(&mut machine, NOW).unwrap();
        play(&mut machine, &mut tape, 37, 263, false);
        assert!(tape.land(&mut machine, 200, |_, _| {}).unwrap());
        play(&mut machine, &mut tape, 207, 200, false);

        let replay = tape.cut(mark, &machine).unwrap();
        assert_eq!(replay.header.frames, 363);
        let back = played_back(&replay, &rom).unwrap();
        assert_eq!(back.state_hash(), machine.state_hash());
    }

    #[test]
    fn a_replay_with_an_added_reset_stops_at_the_next_check() {
        let (rom, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        let mark = tape.mark(&mut machine, NOW).unwrap();
        play(&mut machine, &mut tape, 0, 300, false);
        let mut replay = tape.cut(mark, &machine).unwrap();
        let at = replay.events.partition_point(|event| event.frame < 130);
        replay.events.insert(
            at,
            Event {
                frame: 130,
                input: Input::Reset,
            },
        );

        let diverged = played_back(&replay, &rom).err().unwrap();
        assert_eq!(diverged.frame, 180);
    }

    #[test]
    fn start_over_lands_on_the_mark_and_records_on_from_there() {
        let (rom, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        play(&mut machine, &mut tape, 0, 90, false);
        let mark = tape.mark(&mut machine, NOW).unwrap();
        let at_mark = machine.state_hash();
        play(&mut machine, &mut tape, 90, 200, false);
        assert_ne!(machine.state_hash(), at_mark);

        assert!(tape.restart(&mut machine, mark).unwrap());
        assert_eq!(machine.state_hash(), at_mark);
        assert_eq!(tape.frame(), 90);
        play(&mut machine, &mut tape, 1000, 150, false);
        let replay = tape.cut(mark, &machine).unwrap();
        assert_eq!(replay.header.frames, 150);
        let back = played_back(&replay, &rom).unwrap();
        assert_eq!(back.state_hash(), machine.state_hash());
    }

    #[test]
    fn the_tape_drops_keyframes_older_than_its_window_but_never_a_mark() {
        let (_, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        let mark = tape.mark(&mut machine, NOW).unwrap();
        play(&mut machine, &mut tape, 0, 1500, false);
        assert_eq!(tape.keyframes.front().unwrap().frame, 900);
        assert_eq!(tape.cut(mark, &machine).unwrap().header.frames, 1500);
        tape.unmark(mark);
        assert!(tape.events.front().is_none_or(|event| event.frame >= 900));
        assert!(tape.cut(mark, &machine).is_none());
    }

    #[test]
    fn a_tape_over_its_cap_drops_its_oldest_keyframes_but_keeps_one() {
        let (_, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        play(&mut machine, &mut tape, 0, 300, false);
        let one = tape.keyframes.back().unwrap().packed.len();
        tape.set_limits(Limits {
            cap: one * 2,
            ..LIMITS
        });
        let packed: usize = tape.keyframes.iter().map(|k| k.packed.len()).sum();
        assert!(packed <= one * 2);
        assert!(tape.keyframes.len() < 6);
        assert_eq!(tape.keyframes.back().unwrap().frame, 300);
        tape.set_limits(Limits { cap: 0, ..LIMITS });
        assert_eq!(tape.oldest(), 300);
        play(&mut machine, &mut tape, 300, 60, false);
        assert_eq!(tape.oldest(), 360);
        assert!(tape.events.front().is_none_or(|event| event.frame >= 360));
    }

    #[test]
    fn inputs_held_before_the_tape_started_carry_into_its_gaps() {
        let (_, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let cheat = Input::Cheats(vec!["01FFA0C0".into()]);
        let mut held = Held::default();
        assert!(held.take(&cheat));
        assert!(!held.take(&cheat));
        let mut tape = Tape::start(&mut machine, LIMITS, held.clone()).unwrap();
        assert_eq!(tape.keyframes[0].held, held);
        let hashes = hashes_while_playing(&mut machine, &mut tape, 100, false);

        let mut twin = machine.twin();
        let mut last = 0;
        tape.gap(100)
            .unwrap()
            .run(&mut twin, |frame, twin| {
                assert_eq!(twin.state_hash(), hashes[frame as usize], "frame {frame}");
                last = frame;
            })
            .unwrap();
        assert_eq!(last, 100);
    }

    #[test]
    fn holding_after_a_landing_feeds_only_what_changed() {
        let (_, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let mut held = Held::default();
        held.take(&Input::Cheats(vec!["01FFA0C0".into()]));
        let mut tape = Tape::start(&mut machine, LIMITS, held).unwrap();
        play(&mut machine, &mut tape, 0, 300, false);
        assert!(tape.land(&mut machine, 200, |_, _| {}).unwrap());
        let events = tape.events.len();

        let mut now = Held::default();
        now.take(&Input::Keys(tape.held.keys.bits()));
        tape.hold(&mut machine, &now);
        assert!(tape.held.cheats.is_empty());
        assert_eq!(tape.events.len(), events + 1);

        now.take(&Input::Keys(Keys::A.bits()));
        tape.hold(&mut machine, &now);
        assert_eq!(machine.keys(), Keys::A);
        tape.hold(&mut machine, &now);
        assert_eq!(tape.events.len(), events + 2);
        assert_eq!(tape.held, now);
    }

    fn recorded(bytes: &[u8], kind: ConsoleKind, frames: u64) -> (Rom, Replay) {
        let (rom, mut machine) = game(bytes, kind);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        play(&mut machine, &mut tape, 0, 37, false);
        let mark = tape.mark(&mut machine, NOW).unwrap();
        play(&mut machine, &mut tape, 37, frames, false);
        let replay = tape.cut(mark, &machine).unwrap();
        (rom, Replay::read(&replay.write().unwrap()).unwrap())
    }

    fn hashes_while_seeking(cap: usize) -> (Vec<u64>, Vec<u64>, Option<u64>) {
        let (rom, replay) = recorded(SNAKE, ConsoleKind::Gb, 400);
        let mut machine =
            Machine::from_state(&replay.start, &rom, NOW, &Firmware::default()).unwrap();
        let mut player = Player::new(&replay).seekable(cap);
        let mut straight = Vec::new();
        while !player.is_done() {
            player.step(&mut machine).unwrap();
            machine.take_audio(&mut Vec::new());
            straight.push(machine.state_hash());
        }
        let mut sought = Vec::new();
        for to in [150, 61, 1, 399, 200] {
            player.seek(&mut machine, to).unwrap();
            assert!(
                player.played() <= to,
                "{} after seeking to {to}",
                player.played()
            );
            while player.played() < to {
                player.step(&mut machine).unwrap();
                machine.take_audio(&mut Vec::new());
            }
            sought.push(machine.state_hash());
        }
        let wanted = [150, 61, 1, 399, 200]
            .iter()
            .map(|to| straight[*to as usize - 1])
            .collect();
        (wanted, sought, player.seek_every())
    }

    #[test]
    fn a_seekable_player_lands_on_each_frame_it_seeks_to() {
        let (wanted, sought, every) = hashes_while_seeking(usize::MAX);
        assert_eq!(sought, wanted);
        assert_eq!(every, Some(CHECK_EVERY));
    }

    #[test]
    fn a_seekable_player_names_each_frame_it_keeps_before_playing_it() {
        let (rom, replay) = recorded(SNAKE, ConsoleKind::Gb, 400);
        let mut machine =
            Machine::from_state(&replay.start, &rom, NOW, &Firmware::default()).unwrap();
        let mut player = Player::new(&replay).seekable(usize::MAX);
        let mut named = vec![0];
        while !player.is_done() {
            if player.keeps_next_frame() {
                named.push(player.played() + 1);
            }
            player.step(&mut machine).unwrap();
            machine.take_audio(&mut Vec::new());
        }
        let kept: Vec<u64> = player
            .seek_points()
            .keys
            .iter()
            .map(|key| key.frame)
            .collect();
        assert_eq!(kept, named);
        assert_eq!(kept, [0, 60, 120, 180, 240, 300, 360]);
        assert!(!Player::new(&replay).keeps_next_frame());
    }

    #[test]
    fn seek_points_thin_out_past_their_cap_and_still_land() {
        let (wanted, sought, every) = hashes_while_seeking(1);
        assert_eq!(sought, wanted);
        assert!(every.unwrap() > CHECK_EVERY, "{every:?}");
    }

    fn sent<T: Send + 'static>(value: T) -> T {
        value
    }

    fn clips_the_last_frames(bytes: &[u8], kind: ConsoleKind, touch: bool) {
        let (rom, mut machine) = game(bytes, kind);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        play(&mut machine, &mut tape, 0, 700, touch);
        let clip = tape.clip(&machine, u64::MAX).unwrap();
        assert_eq!(clip.frames(), 640);
        let (clip, mut twin) = sent((clip, machine.twin()));
        let at_clip = machine.state_hash();
        play(&mut machine, &mut tape, 700, 30, touch);
        let replay = clip.cut(&mut twin, NOW + 9).unwrap();
        let replay = Replay::read(&replay.write().unwrap()).unwrap();
        assert_eq!(replay.header.frames, 640);
        assert_eq!(replay.header.check_from, 60);
        assert_eq!(replay.header.start_unix, NOW + 9);
        assert_eq!(replay.checks.len(), 10);
        assert!(has(&replay, |input| matches!(input, Input::Reset)));

        let back = played_back(&replay, &rom).unwrap();
        assert_eq!(back.state_hash(), at_clip);

        let points = clip.seek_points(&replay);
        assert_eq!(points.len(), 11);
        let pictures = points.pictures(&mut back.twin(), 4).unwrap();
        let frames: Vec<u64> = pictures.iter().map(|(frame, _)| *frame).collect();
        assert_eq!(frames, [0, 120, 300, 480]);
        assert!(
            pictures
                .iter()
                .all(|(_, pixels)| pixels.len() == back.frame().len())
        );
        let straight = straight_hashes(&replay, &rom);
        let mut machine = Machine::from_state(
            &replay.start,
            &rom,
            replay.header.start_unix,
            &Firmware::default(),
        )
        .unwrap();
        let mut player = Player::new(&replay).seekable_from(usize::MAX, points.clone());
        for to in [639, 61, 300, 1, 450, 640] {
            player.seek(&mut machine, to).unwrap();
            assert!(to - player.played() <= LIMITS.keyframe_every, "{to}");
            while player.played() < to {
                player.step(&mut machine).unwrap();
                machine.take_audio(&mut Vec::new());
            }
            assert_eq!(machine.state_hash(), straight[to as usize], "{to}");
            if let Some((_, picture)) = pictures.iter().find(|(frame, _)| *frame == to) {
                assert_eq!(machine.frame(), picture.as_slice(), "{to}");
            }
        }
        let mut machine = Machine::from_state(
            &replay.start,
            &rom,
            replay.header.start_unix,
            &Firmware::default(),
        )
        .unwrap();
        let mut player = Player::new(&replay).seekable_from(usize::MAX, points);
        player.seek(&mut machine, 500).unwrap();
        assert_eq!(player.played(), 480);
        let crop = player.crop(&mut machine, 500, 610, NOW + 60).unwrap();
        let back = played_back(&crop, &rom).unwrap();
        assert_eq!(back.state_hash(), straight[610]);
    }

    #[test]
    fn a_clip_starts_at_the_keyframe_before_how_far_back_it_reaches() {
        let (rom, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        play(&mut machine, &mut tape, 0, 700, false);
        let frames = |back: u64| tape.clip(&machine, back).unwrap().frames();
        assert_eq!(frames(100), 100);
        assert_eq!(frames(130), 160);
        assert_eq!(frames(u64::MAX), 640);
        let clip = tape.clip(&machine, 130).unwrap();
        let replay = clip.cut(&mut machine.twin(), NOW).unwrap();
        assert_eq!(replay.header.frames, 160);
        let back = played_back(&replay, &rom).unwrap();
        assert_eq!(back.state_hash(), machine.state_hash());
    }

    #[test]
    fn a_game_boy_clip_of_the_last_frames_plays_back_what_was_played() {
        clips_the_last_frames(SNAKE, ConsoleKind::Gb, false);
    }

    #[test]
    fn a_game_boy_color_clip_of_the_last_frames_plays_back_what_was_played() {
        clips_the_last_frames(PETRIS, ConsoleKind::Gbc, false);
    }

    #[test]
    #[cfg(feature = "gba")]
    fn a_gba_clip_of_the_last_frames_plays_back_what_was_played() {
        clips_the_last_frames(&unbrotli(PENTRIS), ConsoleKind::Gba, false);
    }

    #[test]
    #[cfg(feature = "nds")]
    fn a_ds_clip_with_touch_of_the_last_frames_plays_back_what_was_played() {
        clips_the_last_frames(CUBE, ConsoleKind::Nds, true);
    }

    #[test]
    fn a_tape_cut_from_a_frame_with_its_held_inputs_plays_back() {
        let (rom, mut machine) = game(SNAKE, ConsoleKind::Gb);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        play(&mut machine, &mut tape, 0, 45, false);
        let held = tape.held.clone();
        let save = machine.save_state(NOW).unwrap();
        let header = Header::of(&machine, NOW);
        let from = tape.frame();
        play(&mut machine, &mut tape, 45, 200, false);
        let replay = tape.cut_from(from, &held, save, header, &machine);
        assert_eq!(replay.header.frames, 200);
        assert_eq!(replay.header.check_from, 15);
        let back = played_back(&replay, &rom).unwrap();
        assert_eq!(back.state_hash(), machine.state_hash());
    }

    fn straight_hashes(replay: &Replay, rom: &Rom) -> Vec<u64> {
        let mut machine = Machine::from_state(
            &replay.start,
            rom,
            replay.header.start_unix,
            &Firmware::default(),
        )
        .unwrap();
        let mut player = Player::new(replay);
        let mut hashes = vec![machine.state_hash()];
        while !player.is_done() {
            player.step(&mut machine).unwrap();
            machine.take_audio(&mut Vec::new());
            hashes.push(machine.state_hash());
        }
        hashes
    }

    fn cropped(replay: &Replay, rom: &Rom, from: u64, to: u64) -> Result<Replay, CropError> {
        let mut machine = Machine::from_state(
            &replay.start,
            rom,
            replay.header.start_unix,
            &Firmware::default(),
        )
        .unwrap();
        let cropped = Player::new(replay).crop(&mut machine, from, to, NOW + 60)?;
        Ok(Replay::read(&cropped.write().unwrap()).unwrap())
    }

    fn crops_play_back_to_where_they_end(bytes: &[u8], kind: ConsoleKind, touch: bool) {
        let (rom, mut machine) = game(bytes, kind);
        let mut tape = Tape::start(&mut machine, LIMITS, Held::default()).unwrap();
        play(&mut machine, &mut tape, 0, 37, touch);
        let mark = tape.mark(&mut machine, NOW).unwrap();
        play(&mut machine, &mut tape, 37, 400, touch);
        let replay = tape.cut(mark, &machine).unwrap();
        let straight = straight_hashes(&replay, &rom);
        for (from, to) in [
            (100, 250),
            (1, 400),
            (0, 61),
            (150, 151),
            (23, 83),
            (330, 400),
        ] {
            let crop = cropped(&replay, &rom, from, to).unwrap();
            assert_eq!(crop.header.frames, to - from, "{from}..{to}");
            assert_eq!(crop.header.last_hash, straight[to as usize], "{from}..{to}");
            let back = played_back(&crop, &rom).unwrap();
            assert_eq!(back.state_hash(), straight[to as usize], "{from}..{to}");
        }
    }

    #[test]
    fn a_game_boy_crop_plays_back_to_where_it_ends() {
        crops_play_back_to_where_they_end(SNAKE, ConsoleKind::Gb, false);
    }

    #[test]
    #[cfg(feature = "gba")]
    fn a_gba_crop_plays_back_to_where_it_ends() {
        crops_play_back_to_where_they_end(&unbrotli(PENTRIS), ConsoleKind::Gba, false);
    }

    #[test]
    #[cfg(feature = "nds")]
    fn a_ds_crop_with_touch_plays_back_to_where_it_ends() {
        crops_play_back_to_where_they_end(CUBE, ConsoleKind::Nds, true);
    }

    #[test]
    fn a_crop_of_the_whole_replay_is_the_replay() {
        let (rom, replay) = recorded(SNAKE, ConsoleKind::Gb, 400);
        assert_eq!(cropped(&replay, &rom, 0, 400).unwrap(), replay);
        assert_eq!(cropped(&replay, &rom, 0, 900).unwrap(), replay);
    }

    #[test]
    fn a_crop_of_a_crop_keeps_its_checks() {
        let (rom, replay) = recorded(SNAKE, ConsoleKind::Gb, 400);
        let straight = straight_hashes(&replay, &rom);
        let once = cropped(&replay, &rom, 50, 350).unwrap();
        assert_eq!(once.header.check_from, 33);
        assert_eq!(once.checks.len(), 5);
        let twice = cropped(&once, &rom, 70, 200).unwrap();
        assert_eq!(twice.header.check_from, 23);
        assert_eq!(twice.checks.len(), 2);
        let back = played_back(&twice, &rom).unwrap();
        assert_eq!(back.state_hash(), straight[250]);
    }

    #[test]
    fn a_crop_that_ends_before_it_starts_or_behind_the_player_is_refused() {
        let (rom, replay) = recorded(SNAKE, ConsoleKind::Gb, 120);
        assert!(matches!(
            cropped(&replay, &rom, 80, 80),
            Err(CropError::Range)
        ));
        assert!(matches!(
            cropped(&replay, &rom, 120, 400),
            Err(CropError::Range)
        ));
        let mut machine = Machine::from_state(
            &replay.start,
            &rom,
            replay.header.start_unix,
            &Firmware::default(),
        )
        .unwrap();
        let mut player = Player::new(&replay);
        for _ in 0..30 {
            player.step(&mut machine).unwrap();
        }
        assert!(matches!(
            player.crop(&mut machine, 10, 60, NOW),
            Err(CropError::Range)
        ));
        assert!(player.crop(&mut machine, 30, 60, NOW).is_ok());
    }

    #[test]
    fn a_crop_of_a_replay_that_went_another_way_is_refused() {
        let (rom, mut replay) = recorded(SNAKE, ConsoleKind::Gb, 300);
        replay.checks[1] ^= 1;
        assert!(matches!(
            cropped(&replay, &rom, 10, 250),
            Err(CropError::Diverged(_))
        ));
    }

    fn clip_of_a_real_game(test: &str, file: &str, kind: ConsoleKind, keyframe_every: u64) {
        let path = citrine_gb_test_roms::roms_dir().join(file);
        let Some(bytes) = citrine_gb_test_roms::read(test, path) else {
            return;
        };
        let limits = Limits {
            keyframe_every,
            keep_frames: 1800,
            cap: usize::MAX,
        };
        let (rom, mut machine) = game(&bytes, kind);
        let mut tape = Tape::start(&mut machine, limits, Held::default()).unwrap();
        play(&mut machine, &mut tape, 0, 3600, kind == ConsoleKind::Nds);
        let at_clip = machine.state_hash();
        let clock = std::time::Instant::now();
        let clip = tape.clip(&machine, u64::MAX).unwrap();
        let mut twin = machine.twin();
        let ui = clock.elapsed();
        let clock = std::time::Instant::now();
        let replay = clip.cut(&mut twin, NOW + 9).unwrap();
        let cut = clock.elapsed();
        let clock = std::time::Instant::now();
        let written = replay.write().unwrap();
        let write = clock.elapsed();
        let replay = Replay::read(&written).unwrap();
        assert_eq!(replay.header.frames, 1800);
        let back = played_back(&replay, &rom).unwrap();
        assert_eq!(back.state_hash(), at_clip);

        let clock = std::time::Instant::now();
        let crop = cropped(&replay, &rom, 600, 1200).unwrap();
        let crop_ms = clock.elapsed();
        let crop_bytes = crop.write().unwrap().len();
        let straight = straight_hashes(&replay, &rom);
        let back = played_back(&crop, &rom).unwrap();
        assert_eq!(back.state_hash(), straight[1200]);
        eprintln!(
            "{file}: clip of 1800 frames, ui {:.2} ms, cut {:.1} ms, write {:.1} ms, \
             file {} KB (start {} KB); crop 600..1200 in {:.0} ms, file {} KB",
            ui.as_secs_f64() * 1e3,
            cut.as_secs_f64() * 1e3,
            write.as_secs_f64() * 1e3,
            written.len() / 1024,
            replay.start.len() / 1024,
            crop_ms.as_secs_f64() * 1e3,
            crop_bytes / 1024,
        );
    }

    #[test]
    fn a_pokemon_crystal_clip_and_crop_play_back() {
        clip_of_a_real_game(
            "a_pokemon_crystal_clip_and_crop_play_back",
            "crystal.gbc",
            ConsoleKind::Gbc,
            60,
        );
    }

    #[test]
    #[cfg(feature = "gba")]
    fn a_pokemon_emerald_clip_and_crop_play_back() {
        clip_of_a_real_game(
            "a_pokemon_emerald_clip_and_crop_play_back",
            "emerald.gba",
            ConsoleKind::Gba,
            60,
        );
    }

    #[test]
    #[ignore]
    #[cfg(feature = "nds")]
    fn a_pokemon_platinum_clip_and_crop_play_back() {
        clip_of_a_real_game(
            "a_pokemon_platinum_clip_and_crop_play_back",
            "pt.nds",
            ConsoleKind::Nds,
            30,
        );
    }

    #[test]
    fn a_plain_player_keeps_no_seek_points() {
        let (rom, replay) = recorded(SNAKE, ConsoleKind::Gb, 120);
        let mut machine =
            Machine::from_state(&replay.start, &rom, NOW, &Firmware::default()).unwrap();
        let mut player = Player::new(&replay);
        while !player.is_done() {
            player.step(&mut machine).unwrap();
        }
        assert_eq!(player.seek_bytes(), 0);
        assert_eq!(player.seek_every(), None);
    }
}
