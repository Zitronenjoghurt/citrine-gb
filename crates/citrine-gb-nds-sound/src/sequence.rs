pub const TRACKS: usize = 16;
const TEMPO_UNIT: u32 = 240;
const DEFAULT_TEMPO: u32 = 120;
const STACK: usize = 3;
const STEPS_PER_TICK: usize = 4096;
const VARIABLES: usize = 32;
const NO_OVERRIDE: u8 = 0xFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Note {
        track: usize,
        key: u8,
        velocity: u8,
        length: u32,
        tie: bool,
    },
    Release {
        track: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    Call(usize),
    Loop { start: usize, left: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modulation {
    pub depth: u8,
    pub speed: u8,
    pub kind: u8,
    pub range: u8,
    pub delay: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pos: usize,
    wait: u32,
    active: bool,
    stack: Vec<Frame>,
    condition: bool,
    pub program: u16,
    pub volume: u8,
    pub expression: u8,
    pub pan: u8,
    pub transpose: i8,
    pub bend: i8,
    pub bend_range: u8,
    pub priority: u8,
    pub note_wait: bool,
    pub tie: bool,
    pub modulation: Modulation,
    pub envelope: [u8; 4],
}

impl Track {
    fn new(pos: usize) -> Self {
        Self {
            pos,
            wait: 0,
            active: true,
            stack: Vec::new(),
            condition: false,
            program: 0,
            volume: 127,
            expression: 127,
            pan: 64,
            transpose: 0,
            bend: 0,
            bend_range: 2,
            priority: 64,
            note_wait: true,
            tie: false,
            modulation: Modulation {
                depth: 0,
                speed: 16,
                kind: 0,
                range: 1,
                delay: 0,
            },
            envelope: [NO_OVERRIDE; 4],
        }
    }

    pub fn envelope_override(&self, part: usize) -> Option<u8> {
        self.envelope
            .get(part)
            .copied()
            .filter(|&v| v != NO_OVERRIDE)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prefix {
    None,
    Random,
    Variable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arg {
    U8,
    S8,
    S16,
    Length,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    data: Vec<u8>,
    tracks: Vec<Option<Track>>,
    variables: [i16; VARIABLES],
    tempo: u32,
    tempo_count: u32,
    master_volume: u8,
    random: u32,
    loops: u32,
    loop_target: Option<usize>,
    lead_trace: Option<Vec<usize>>,
}

impl Sequence {
    pub fn new(data: &[u8], master_volume: u8) -> Self {
        let mut tracks = vec![None; TRACKS];
        tracks[0] = Some(Track::new(0));
        Self {
            data: data.to_vec(),
            tracks,
            variables: [-1; VARIABLES],
            tempo: DEFAULT_TEMPO,
            tempo_count: 0,
            master_volume,
            random: 0x1234_5678,
            loops: 0,
            loop_target: None,
            lead_trace: None,
        }
    }

    pub fn track(&self, index: usize) -> Option<&Track> {
        self.tracks.get(index)?.as_ref()
    }

    pub fn master_volume(&self) -> u8 {
        self.master_volume
    }

    pub fn loops(&self) -> u32 {
        self.loops
    }

    pub fn loop_target(&self) -> Option<usize> {
        self.loop_target
    }

    pub fn trace_lead(&mut self) {
        self.lead_trace = Some(Vec::new());
    }

    pub fn take_lead_trace(&mut self) -> Vec<usize> {
        self.lead_trace
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    pub fn finished(&self) -> bool {
        self.tracks.iter().flatten().all(|track| !track.active)
    }

    pub fn ticks_due(&mut self) -> u32 {
        self.tempo_count += self.tempo;
        let due = self.tempo_count / TEMPO_UNIT;
        self.tempo_count %= TEMPO_UNIT;
        due
    }

    pub fn tick(&mut self, events: &mut Vec<Event>) {
        for index in 0..TRACKS {
            self.run_track(index, events);
        }
    }

    fn run_track(&mut self, index: usize, events: &mut Vec<Event>) {
        let Some(track) = self.tracks[index].as_mut() else {
            return;
        };
        if !track.active {
            return;
        }
        if track.wait > 0 {
            track.wait -= 1;
            if track.wait > 0 {
                return;
            }
        }
        for _ in 0..STEPS_PER_TICK {
            let running = self.tracks[index]
                .as_ref()
                .is_some_and(|t| t.active && t.wait == 0);
            if !running {
                return;
            }
            if self.lead_trace.is_some()
                && index == self.first_track()
                && let Some(trace) = &mut self.lead_trace
                && let Some(track) = &self.tracks[index]
            {
                trace.push(track.pos);
            }
            if self.step(index, events).is_none() {
                self.end_track(index, events);
                return;
            }
        }
        self.end_track(index, events);
    }

    fn end_track(&mut self, index: usize, events: &mut Vec<Event>) {
        if let Some(track) = self.tracks[index].as_mut() {
            track.active = false;
        }
        events.push(Event::Release { track: index });
    }

    fn byte(&mut self, index: usize) -> Option<u8> {
        let track = self.tracks[index].as_mut()?;
        let value = *self.data.get(track.pos)?;
        track.pos += 1;
        Some(value)
    }

    fn bytes<const N: usize>(&mut self, index: usize) -> Option<[u8; N]> {
        let mut out = [0; N];
        for byte in &mut out {
            *byte = self.byte(index)?;
        }
        Some(out)
    }

    fn length(&mut self, index: usize) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = self.byte(index)?;
            value = (value << 7) | u32::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        Some(value)
    }

    fn offset(&mut self, index: usize) -> Option<usize> {
        let [a, b, c] = self.bytes::<3>(index)?;
        Some(usize::from(a) | usize::from(b) << 8 | usize::from(c) << 16)
    }

    fn read(&mut self, index: usize, arg: Arg) -> Option<i32> {
        Some(match arg {
            Arg::U8 => i32::from(self.byte(index)?),
            Arg::S8 => i32::from(self.byte(index)? as i8),
            Arg::S16 => i32::from(i16::from_le_bytes(self.bytes::<2>(index)?)),
            Arg::Length => self.length(index)? as i32,
        })
    }

    fn last(&mut self, index: usize, arg: Arg, prefix: Prefix) -> Option<i32> {
        match prefix {
            Prefix::None => self.read(index, arg),
            Prefix::Random => {
                let min = self.read(index, Arg::S16)?;
                let max = self.read(index, Arg::S16)?;
                Some(self.random_between(min, max))
            }
            Prefix::Variable => {
                let variable = usize::from(self.byte(index)?);
                Some(i32::from(*self.variables.get(variable)?))
            }
        }
    }

    fn random_between(&mut self, min: i32, max: i32) -> i32 {
        self.random = self
            .random
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let span = (max - min).unsigned_abs() + 1;
        min.min(max) + ((self.random >> 16) % span) as i32
    }

    fn step(&mut self, index: usize, events: &mut Vec<Event>) -> Option<()> {
        let mut op = self.byte(index)?;
        let mut execute = true;
        let mut prefix = Prefix::None;
        loop {
            match op {
                0xA2 => {
                    execute &= self.tracks[index].as_ref()?.condition;
                }
                0xA0 => prefix = Prefix::Random,
                0xA1 => prefix = Prefix::Variable,
                _ => break,
            }
            op = self.byte(index)?;
        }
        match op {
            0x00..=0x7F => {
                let velocity = self.byte(index)?;
                let length = self.last(index, Arg::Length, prefix)?.max(0) as u32;
                if execute {
                    let track = self.tracks[index].as_mut()?;
                    let key = (i32::from(op) + i32::from(track.transpose)).clamp(0, 127) as u8;
                    if track.note_wait {
                        track.wait = length;
                    }
                    events.push(Event::Note {
                        track: index,
                        key,
                        velocity: velocity & 0x7F,
                        length,
                        tie: track.tie,
                    });
                }
            }
            0x80 => {
                let length = self.last(index, Arg::Length, prefix)?.max(0) as u32;
                if execute {
                    self.tracks[index].as_mut()?.wait = length;
                }
            }
            0x81 => {
                let program = self.last(index, Arg::Length, prefix)?;
                if execute {
                    self.tracks[index].as_mut()?.program = program as u16;
                }
            }
            0x93 => {
                let target = usize::from(self.byte(index)?);
                let at = self.offset(index)?;
                if execute && target < TRACKS && target != index {
                    self.tracks[target] = Some(Track::new(at));
                }
            }
            0x94 => {
                let at = self.offset(index)?;
                if execute {
                    let first = self.first_track();
                    let track = self.tracks[index].as_mut()?;
                    if at < track.pos && index == first {
                        self.loops += 1;
                        self.loop_target = Some(at);
                    }
                    track.pos = at;
                }
            }
            0x95 => {
                let at = self.offset(index)?;
                let track = self.tracks[index].as_mut()?;
                if execute && track.stack.len() < STACK {
                    track.stack.push(Frame::Call(track.pos));
                    track.pos = at;
                }
            }
            0xFD => {
                let track = self.tracks[index].as_mut()?;
                if execute {
                    while let Some(frame) = track.stack.pop() {
                        if let Frame::Call(to) = frame {
                            track.pos = to;
                            break;
                        }
                    }
                }
            }
            0xD4 => {
                let count = self.last(index, Arg::U8, prefix)? as u8;
                let track = self.tracks[index].as_mut()?;
                if execute && track.stack.len() < STACK {
                    track.stack.push(Frame::Loop {
                        start: track.pos,
                        left: count,
                    });
                }
            }
            0xFC => {
                let first = self.first_track();
                let track = self.tracks[index].as_mut()?;
                if execute && let Some(Frame::Loop { start, left }) = track.stack.last_mut() {
                    let start = *start;
                    if *left == 0 {
                        track.pos = start;
                        if index == first {
                            self.loops += 1;
                            self.loop_target = Some(start);
                        }
                    } else {
                        *left -= 1;
                        if *left == 0 {
                            track.stack.pop();
                        } else {
                            track.pos = start;
                        }
                    }
                }
            }
            0xFE => {
                self.bytes::<2>(index)?;
            }
            0xFF => {
                if execute {
                    return None;
                }
            }
            0xB0..=0xBD => {
                let variable = usize::from(self.byte(index)?);
                let value = self.last(index, Arg::S16, prefix)?;
                if execute {
                    self.variable(index, op, variable, value)?;
                }
            }
            0xC0..=0xD6 => {
                let arg = match op {
                    0xC3 | 0xC4 => Arg::S8,
                    _ => Arg::U8,
                };
                let value = self.last(index, arg, prefix)?;
                if execute {
                    self.control(index, op, value, events)?;
                }
            }
            0xE0..=0xE3 => {
                let value = self.last(index, Arg::S16, prefix)?;
                if execute {
                    match op {
                        0xE0 => self.tracks[index].as_mut()?.modulation.delay = value.max(0) as u16,
                        0xE1 => self.tempo = value.clamp(1, 1023) as u32,
                        _ => {}
                    }
                }
            }
            _ => return None,
        }
        Some(())
    }

    fn first_track(&self) -> usize {
        self.tracks
            .iter()
            .position(|t| t.as_ref().is_some_and(|t| t.active))
            .unwrap_or(0)
    }

    fn variable(&mut self, index: usize, op: u8, variable: usize, value: i32) -> Option<()> {
        let current = i32::from(*self.variables.get(variable)?);
        let set = |vars: &mut [i16; VARIABLES], v: i32| vars[variable] = v as i16;
        match op {
            0xB0 => set(&mut self.variables, value),
            0xB1 => set(&mut self.variables, current.wrapping_add(value)),
            0xB2 => set(&mut self.variables, current.wrapping_sub(value)),
            0xB3 => set(&mut self.variables, current.wrapping_mul(value)),
            0xB4 if value != 0 => set(&mut self.variables, current / value),
            0xB5 if value >= 0 => set(&mut self.variables, current << value.min(15)),
            0xB5 => set(&mut self.variables, current >> (-value).min(15)),
            0xB6 => {
                let random = self.random_between(0, value);
                set(&mut self.variables, random);
            }
            0xB8..=0xBD => {
                let condition = match op {
                    0xB8 => current == value,
                    0xB9 => current >= value,
                    0xBA => current > value,
                    0xBB => current <= value,
                    0xBC => current < value,
                    _ => current != value,
                };
                self.tracks[index].as_mut()?.condition = condition;
            }
            _ => {}
        }
        Some(())
    }

    fn control(&mut self, index: usize, op: u8, value: i32, events: &mut Vec<Event>) -> Option<()> {
        let byte = value.clamp(0, 255) as u8;
        if op == 0xC2 {
            self.master_volume = byte.min(127);
            return Some(());
        }
        let track = self.tracks[index].as_mut()?;
        match op {
            0xC0 => track.pan = byte.min(127),
            0xC1 => track.volume = byte.min(127),
            0xC3 => track.transpose = value as i8,
            0xC4 => track.bend = value as i8,
            0xC5 => track.bend_range = byte,
            0xC6 => track.priority = byte,
            0xC7 => track.note_wait = byte != 0,
            0xC8 => {
                let was = track.tie;
                track.tie = byte != 0;
                if was && !track.tie {
                    events.push(Event::Release { track: index });
                }
            }
            0xCA => track.modulation.depth = byte,
            0xCB => track.modulation.speed = byte,
            0xCC => track.modulation.kind = byte,
            0xCD => track.modulation.range = byte,
            0xD0..=0xD3 => track.envelope[usize::from(op - 0xD0)] = byte,
            0xD5 => track.expression = byte.min(127),
            _ => {}
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_ticks(data: &[u8], ticks: u64) -> Vec<(u64, u8, u32)> {
        let mut sequence = Sequence::new(data, 127);
        let mut out = Vec::new();
        let mut events = Vec::new();
        for tick in 0..ticks {
            sequence.tick(&mut events);
            for event in events.drain(..) {
                if let Event::Note { key, length, .. } = event {
                    out.push((tick, key, length));
                }
            }
        }
        out
    }

    #[test]
    fn notes_wait_for_their_length_and_rests_wait_too() {
        let data = [0x3C, 100, 48, 0x80, 24, 0x3E, 100, 0x81, 0x00, 0xFF];
        assert_eq!(note_ticks(&data, 400), [(0, 60, 48), (72, 62, 128)]);
    }

    #[test]
    fn calls_return_and_counted_loops_repeat() {
        let data = [
            0xD4, 2, 0x95, 11, 0, 0, 0xFC, 0x40, 1, 6, 0xFF, 0x3C, 1, 12, 0xFD,
        ];
        assert_eq!(
            note_ticks(&data, 100),
            [(0, 60, 12), (12, 60, 12), (24, 64, 6)]
        );
    }

    #[test]
    fn opened_tracks_play_alongside_and_backward_jumps_count_loops() {
        let data = [
            0xFE, 3, 0, 0x93, 1, 15, 0, 0, 0x3C, 0x7F, 16, 0x94, 8, 0, 0, 0x30, 0x7F, 8, 0xFF,
        ];
        let mut sequence = Sequence::new(&data, 127);
        let mut events = Vec::new();
        let mut notes = Vec::new();
        for tick in 0..40 {
            sequence.tick(&mut events);
            for event in events.drain(..) {
                match event {
                    Event::Note { track, key, .. } => notes.push((tick, track, key)),
                    Event::Release { track } => notes.push((tick, track, 0xFF)),
                }
            }
        }
        assert_eq!(
            notes,
            [
                (0, 0, 60),
                (0, 1, 48),
                (8, 1, 0xFF),
                (16, 0, 60),
                (32, 0, 60)
            ]
        );
        assert_eq!(sequence.loops(), 2);
        assert!(!sequence.finished());
    }

    #[test]
    fn conditions_skip_commands_and_variables_feed_arguments() {
        let data = [
            0xB0, 0, 5, 0, 0xB8, 0, 5, 0, 0xA2, 0x3C, 0x7F, 1, 0xB8, 0, 6, 0, 0xA2, 0x3E, 0x7F, 1,
            0xA1, 0x40, 0x7F, 0, 0xFF,
        ];
        assert_eq!(note_ticks(&data, 20), [(0, 60, 1), (1, 64, 5)]);
    }

    #[test]
    fn the_tempo_sets_how_many_ticks_each_frame_runs() {
        let mut sequence = Sequence::new(&[0xE1, 240, 0, 0xFF], 127);
        assert_eq!(
            (0..4).map(|_| sequence.ticks_due()).collect::<Vec<_>>(),
            [0, 1, 0, 1]
        );
        sequence.tick(&mut Vec::new());
        assert_eq!(
            (0..2).map(|_| sequence.ticks_due()).collect::<Vec<_>>(),
            [1, 1]
        );
        assert!(sequence.finished());
    }
}
