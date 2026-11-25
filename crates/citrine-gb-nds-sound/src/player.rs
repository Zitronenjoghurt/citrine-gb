use std::collections::HashMap;

use citrine_gb::nds::sound::mixer::{MIX_CYCLES, Mixer, SoundMemory};
use citrine_gb::nds::sound::output::Output;
use citrine_gb_nds_files::{RomError, Sbnk, Swar, Voice as Kind, Wave, sequence_data};

use crate::envelope::{Envelope, Stage};
use crate::sequence::{Event, Sequence, Track};
use crate::tables::{adjust_timer, channel_volume, decibels, sine};

const ARM7_CLOCK: f64 = 33_513_982.0;
const FRAME_CYCLES: i64 = 2728 * 64;
const BASE: u32 = 0x0200_0000;
const CHANNEL_REGISTERS: usize = 0x400;
const CHANNEL_BYTES: usize = 0x10;
const SOUNDCNT: usize = 0x500;
const SOUNDBIAS: usize = 0x504;
const MASTER_ON: u32 = (1 << 15) | 0x7F;
const CENTRE_BIAS: u32 = 0x200;
const CNT: usize = 0x0;
const SAD: usize = 0x4;
const TMR: usize = 0x8;
const LEN: usize = 0xC;
const START: u32 = 1 << 31;
const PSG_FORMAT: u32 = 3;
const PSG_TIMER: u16 = 4760;
const PSG_NOTE: u8 = 69;
const PCM_CHANNELS: [usize; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 14, 15, 8, 9, 10, 11, 12, 13];
const SQUARE_CHANNELS: [usize; 6] = [8, 9, 10, 11, 12, 13];
const NOISE_CHANNELS: [usize; 2] = [14, 15];
const MIX_BLOCK: usize = 64;
const MAX_LENGTH_FRAMES: u64 = 20 * 60 * 192;
const ALL: u32 = 0xFFFF_FFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sample {
    address: u32,
    control: u32,
    timer: u16,
    loop_start: u16,
    loop_length: u32,
}

#[derive(Debug, Default)]
struct WaveMemory {
    words: Vec<u32>,
}

impl SoundMemory for WaveMemory {
    fn read_word(&mut self, address: u32) -> u32 {
        let index = (address.wrapping_sub(BASE) / 4) as usize;
        self.words.get(index).copied().unwrap_or(0)
    }

    fn write_word(&mut self, _: u32, _: u32) {}
}

impl WaveMemory {
    fn add(&mut self, wave: &Wave) -> Sample {
        let address = BASE + self.words.len() as u32 * 4;
        let bytes = &wave.data[..(wave.words() * 4).min(wave.data.len())];
        self.words.extend(bytes.chunks(4).map(|chunk| {
            let mut word = [0; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            u32::from_le_bytes(word)
        }));
        let repeat = if wave.looped { 1 } else { 2 };
        Sample {
            address,
            control: (wave.format.register() << 29) | (repeat << 27),
            timer: wave.timer,
            loop_start: wave.loop_start,
            loop_length: wave.loop_length,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Voice {
    track: Option<usize>,
    key: u8,
    base_note: u8,
    velocity: u8,
    length: Option<u32>,
    envelope: Envelope,
    pan: u8,
    timer: u16,
    control: u32,
    priority: u8,
    lfo_wait: u16,
    lfo_counter: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Length {
    pub seconds: f64,
    pub loop_start: Option<f64>,
}

pub struct Player {
    mixer: Mixer,
    memory: WaveMemory,
    bank: Option<Sbnk>,
    samples: HashMap<(usize, usize), Sample>,
    sequence: Option<Sequence>,
    voices: [Option<Voice>; 16],
    output: Output,
    pending: Vec<i16>,
    until_frame: i64,
    mixes: u64,
    events: Vec<Event>,
    length: Option<Length>,
}

impl Player {
    fn empty(sample_rate: u32) -> Self {
        let mut mixer = Mixer::new();
        mixer.write(SOUNDCNT, MASTER_ON, ALL);
        mixer.write(SOUNDBIAS, CENTRE_BIAS, ALL);
        Self {
            mixer,
            memory: WaveMemory::default(),
            bank: None,
            samples: HashMap::new(),
            sequence: None,
            voices: [None; 16],
            output: Output::new(sample_rate),
            pending: Vec::new(),
            until_frame: 0,
            mixes: 0,
            events: Vec::new(),
            length: None,
        }
    }

    pub fn sequence(
        sseq: &[u8],
        bank: &[u8],
        archives: [Option<&[u8]>; 4],
        volume: u8,
        sample_rate: u32,
    ) -> Result<Self, RomError> {
        let data = sequence_data(sseq)?;
        let mut player = Self::empty(sample_rate);
        player.bank = Some(Sbnk::parse(bank)?);
        for (slot, archive) in archives.into_iter().enumerate() {
            let Some(bytes) = archive else {
                continue;
            };
            let swar = Swar::parse(bytes)?;
            for index in 0..swar.len() {
                if let Ok(wave) = swar.wave(index) {
                    let sample = player.memory.add(&wave);
                    player.samples.insert((slot, index), sample);
                }
            }
        }
        let sequence = Sequence::new(data, volume);
        player.length = Some(measure(sequence.clone()));
        player.sequence = Some(sequence);
        Ok(player)
    }

    pub fn wave(wave: &Wave, sample_rate: u32) -> Self {
        let mut player = Self::empty(sample_rate);
        let sample = player.memory.add(wave);
        let samples = wave.sample_count() as f64;
        player.length = Some(Length {
            seconds: samples * f64::from(wave.timer) * 2.0 / ARM7_CLOCK,
            loop_start: wave
                .looped
                .then(|| wave.loop_sample() as f64 * f64::from(wave.timer) * 2.0 / ARM7_CLOCK),
        });
        let voice = Voice {
            track: None,
            key: 60,
            base_note: 60,
            velocity: 127,
            length: None,
            envelope: Envelope::new(127, 127, 127, 127),
            pan: 64,
            timer: sample.timer,
            control: sample.control,
            priority: 127,
            lfo_wait: 0,
            lfo_counter: 0,
        };
        player.start(0, voice, &sample);
        player
    }

    pub fn length(&self) -> Option<Length> {
        self.length
    }

    pub fn elapsed(&self) -> f64 {
        self.mixes as f64 * f64::from(MIX_CYCLES) / ARM7_CLOCK
    }

    pub fn position(&self) -> f64 {
        let elapsed = self.elapsed();
        match self.length {
            Some(Length {
                seconds,
                loop_start: Some(start),
            }) if elapsed > seconds && seconds > start => {
                start + (elapsed - seconds) % (seconds - start)
            }
            _ => elapsed,
        }
    }

    pub fn loops(&self) -> u32 {
        self.sequence.as_ref().map_or(0, Sequence::loops)
    }

    pub fn is_done(&self) -> bool {
        self.sequence.as_ref().is_none_or(Sequence::finished)
            && self.voices.iter().all(Option::is_none)
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        if rate != self.output.sample_rate() {
            self.output.set_sample_rate(rate);
            self.pending.clear();
        }
    }

    pub fn render(&mut self, frames: usize, out: &mut Vec<f32>) {
        self.fill(frames * 2);
        out.extend(
            self.pending
                .drain(..frames * 2)
                .map(|s| f32::from(s) / 32768.0),
        );
    }

    pub fn render_pcm(&mut self, frames: usize, out: &mut Vec<i16>) {
        self.fill(frames * 2);
        out.extend(self.pending.drain(..frames * 2));
    }

    fn fill(&mut self, wanted: usize) {
        while self.pending.len() < wanted {
            for _ in 0..MIX_BLOCK {
                self.until_frame -= i64::from(MIX_CYCLES);
                if self.until_frame < 0 {
                    self.frame();
                    self.until_frame += FRAME_CYCLES;
                }
                self.mixer.mix(&mut self.memory, false);
                self.mixes += 1;
                for sample in self.mixer.take_samples() {
                    self.output.push(sample);
                }
            }
            self.output.take_raw(&mut self.pending);
        }
    }

    fn frame(&mut self) {
        let due = self.sequence.as_mut().map_or(0, Sequence::ticks_due);
        for _ in 0..due {
            self.tick();
        }
        self.update();
    }

    fn tick(&mut self) {
        for voice in self.voices.iter_mut().flatten() {
            if let Some(left) = voice.length.as_mut()
                && *left > 0
            {
                *left -= 1;
                if *left == 0 {
                    voice.envelope.release();
                }
            }
        }
        let Some(sequence) = self.sequence.as_mut() else {
            return;
        };
        sequence.tick(&mut self.events);
        let events = std::mem::take(&mut self.events);
        for event in &events {
            match *event {
                Event::Note {
                    track,
                    key,
                    velocity,
                    length,
                    tie,
                } => self.note_on(track, key, velocity, length, tie),
                Event::Release { track } => {
                    for voice in self.voices.iter_mut().flatten() {
                        if voice.track == Some(track) && voice.length.is_none() {
                            voice.envelope.release();
                        }
                    }
                }
            }
        }
        self.events = events;
        self.events.clear();
    }

    fn note_on(&mut self, track: usize, key: u8, velocity: u8, length: u32, tie: bool) {
        if tie
            && let Some(voice) = self.voices.iter_mut().flatten().find(|v| {
                v.track == Some(track) && v.length.is_none() && v.envelope.stage() != Stage::Release
            })
        {
            voice.key = key;
            voice.velocity = velocity;
            return;
        }
        let Some(params) = self.sequence.as_ref().and_then(|s| s.track(track)) else {
            return;
        };
        let Some(def) = self
            .bank
            .as_ref()
            .and_then(|bank| bank.instrument(usize::from(params.program)))
            .and_then(|instrument| instrument.note(key))
        else {
            return;
        };
        let envelope = Envelope::new(
            params.envelope_override(0).unwrap_or(def.attack),
            params.envelope_override(1).unwrap_or(def.decay),
            params.envelope_override(2).unwrap_or(def.sustain),
            params.envelope_override(3).unwrap_or(def.release),
        );
        let priority = params.priority;
        let lfo_wait = params.modulation.delay;
        let (channels, sample, base_note): (&[usize], Sample, u8) = match def.voice {
            Kind::Pcm { wave, archive } => {
                let Some(&sample) = self.samples.get(&(usize::from(archive), usize::from(wave)))
                else {
                    return;
                };
                (&PCM_CHANNELS, sample, def.base_note)
            }
            Kind::Square { duty } => (&SQUARE_CHANNELS, psg(u32::from(duty) << 24), PSG_NOTE),
            Kind::Noise => (&NOISE_CHANNELS, psg(0), PSG_NOTE),
        };
        let Some(channel) = self.free_channel(channels, priority) else {
            return;
        };
        let voice = Voice {
            track: Some(track),
            key,
            base_note,
            velocity,
            length: (!tie).then_some(length.max(1)),
            envelope,
            pan: def.pan,
            timer: sample.timer,
            control: sample.control,
            priority,
            lfo_wait,
            lfo_counter: 0,
        };
        self.start(channel, voice, &sample);
    }

    fn free_channel(&self, channels: &[usize], priority: u8) -> Option<usize> {
        if let Some(&free) = channels.iter().find(|&&c| self.voices[c].is_none()) {
            return Some(free);
        }
        channels
            .iter()
            .copied()
            .filter_map(|c| self.voices[c].map(|v| (c, v)))
            .filter(|(_, v)| v.priority <= priority)
            .min_by_key(|(_, v)| {
                (
                    v.priority,
                    v.envelope.stage() != Stage::Release,
                    v.envelope.level(),
                )
            })
            .map(|(c, _)| c)
    }

    fn start(&mut self, channel: usize, voice: Voice, sample: &Sample) {
        let at = CHANNEL_REGISTERS + channel * CHANNEL_BYTES;
        self.mixer.write(at + CNT, 0, ALL);
        self.mixer.write(at + SAD, sample.address, ALL);
        self.mixer.write(
            at + TMR,
            u32::from(sample.loop_start) << 16 | u32::from(0u16.wrapping_sub(sample.timer)),
            ALL,
        );
        self.mixer.write(at + LEN, sample.loop_length, ALL);
        self.mixer.write(at + CNT, sample.control | START, ALL);
        self.voices[channel] = Some(voice);
    }

    fn update(&mut self) {
        for channel in 0..self.voices.len() {
            let Some(mut voice) = self.voices[channel] else {
                continue;
            };
            let at = CHANNEL_REGISTERS + channel * CHANNEL_BYTES;
            voice.envelope.step();
            if voice.envelope.stage() == Stage::Done || !self.mixer.channel(channel).running() {
                self.mixer.write(at + CNT, 0, ALL);
                self.voices[channel] = None;
                continue;
            }
            let track = voice
                .track
                .and_then(|t| self.sequence.as_ref()?.track(t))
                .cloned();
            let master = self.sequence.as_ref().map_or(127, Sequence::master_volume);
            let (pitch, volume, pan) = modulation(&mut voice, track.as_ref());
            let (volume, pan, bend) = match &track {
                Some(t) => (
                    decibels(master)
                        + decibels(t.volume)
                        + decibels(t.expression)
                        + decibels(voice.velocity)
                        + volume,
                    i32::from(t.pan) + i32::from(voice.pan) - 64 + pan,
                    (i32::from(t.bend) * i32::from(t.bend_range)) >> 1,
                ),
                None => (decibels(voice.velocity), i32::from(voice.pan), 0),
            };
            let (level, divider) = channel_volume(volume + (voice.envelope.level() >> 7));
            let pitch = (i32::from(voice.key) - i32::from(voice.base_note)) * 64 + bend + pitch;
            let timer = adjust_timer(voice.timer, pitch);
            let control = voice.control
                | START
                | u32::from(level)
                | u32::from(divider) << 8
                | (pan.clamp(0, 127) as u32) << 16;
            self.mixer.write(at + CNT, control, ALL);
            self.mixer
                .write(at + TMR, u32::from(0u16.wrapping_sub(timer)), 0xFFFF);
            self.voices[channel] = Some(voice);
        }
    }
}

fn psg(duty: u32) -> Sample {
    Sample {
        address: BASE,
        control: PSG_FORMAT << 29 | duty,
        timer: PSG_TIMER,
        loop_start: 0,
        loop_length: 0,
    }
}

fn modulation(voice: &mut Voice, track: Option<&Track>) -> (i32, i32, i32) {
    let Some(track) = track else {
        return (0, 0, 0);
    };
    let lfo = track.modulation;
    if lfo.depth == 0 {
        return (0, 0, 0);
    }
    if voice.lfo_wait > 0 {
        voice.lfo_wait -= 1;
        return (0, 0, 0);
    }
    voice.lfo_counter = voice.lfo_counter.wrapping_add(u16::from(lfo.speed) << 6);
    let value = sine((voice.lfo_counter >> 8) as u8) * i32::from(lfo.depth) * i32::from(lfo.range);
    match lfo.kind {
        1 => (0, (value * 60) >> 14, 0),
        2 => (0, 0, value >> 8),
        _ => (value >> 8, 0, 0),
    }
}

pub fn sequence_length(sseq: &[u8]) -> Result<Length, RomError> {
    Ok(measure(Sequence::new(sequence_data(sseq)?, 127)))
}

pub fn wave_length(wave: &Wave) -> Length {
    let seconds = |samples: usize| samples as f64 * f64::from(wave.timer) * 2.0 / ARM7_CLOCK;
    Length {
        seconds: seconds(wave.sample_count()),
        loop_start: wave.looped.then(|| seconds(wave.loop_sample())),
    }
}

fn measure(mut sequence: Sequence) -> Length {
    let mut events = Vec::new();
    let mut frames = 0u64;
    let mut reached = HashMap::new();
    sequence.trace_lead();
    while !sequence.finished() && sequence.loops() == 0 && frames < MAX_LENGTH_FRAMES {
        for _ in 0..sequence.ticks_due() {
            sequence.tick(&mut events);
            events.clear();
            for at in sequence.take_lead_trace() {
                reached.entry(at).or_insert(frames);
            }
        }
        frames += 1;
    }
    let seconds = |frames: u64| frames as f64 * FRAME_CYCLES as f64 / ARM7_CLOCK;
    let loop_start = (sequence.loops() > 0).then(|| {
        let at = sequence
            .loop_target()
            .and_then(|at| reached.get(&at).copied());
        seconds(at.unwrap_or(0))
    });
    Length {
        seconds: seconds(frames),
        loop_start,
    }
}

const _: fn() = || {
    fn send<T: Send>() {}
    send::<Player>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_nds_files::fixtures::{bank, sseq, wave};
    use citrine_gb_nds_files::{Instrument, NoteDef, WaveFormat};

    fn channel_samples(player: &mut Player, count: usize) -> Vec<i16> {
        let mut out = Vec::new();
        for _ in 0..count + 16 {
            player.mixer.mix(&mut player.memory, false);
            player.mixer.take_samples().for_each(drop);
            out.push(player.mixer.channel(0).sample());
        }
        let first = out.iter().position(|&s| s != 0).unwrap_or(0);
        out[first..first + count].to_vec()
    }

    fn wave_player(format: WaveFormat, data: &[u8]) -> Player {
        let bytes = wave::build_swav(&wave::build_info(format, true, 0, data));
        let wave = Wave::parse_swav(&bytes).unwrap();
        let mut player = Player::wave(&wave, 48_000);
        let at = CHANNEL_REGISTERS + TMR;
        player.mixer.write(at, 0x10000 - 512, 0xFFFF);
        player
    }

    #[test]
    fn a_pcm8_swav_plays_its_bytes_as_the_high_byte() {
        let mut player = wave_player(WaveFormat::Pcm8, &[0x10, 0x7F, 0x80, 0xF0]);
        assert_eq!(
            channel_samples(&mut player, 4),
            [0x1000, 0x7F00, -0x8000, -0x1000]
        );
    }

    #[test]
    fn a_pcm16_swav_plays_its_halfwords() {
        let mut player = wave_player(WaveFormat::Pcm16, &[0x34, 0x12, 0x00, 0x90]);
        assert_eq!(channel_samples(&mut player, 2), [0x1234, -0x7000]);
    }

    #[test]
    fn an_adpcm_swav_decodes_from_its_header() {
        let mut player = wave_player(
            WaveFormat::Adpcm,
            &[0x00, 0x01, 0, 0, 0x07, 0x8F, 0x00, 0x80],
        );
        assert_eq!(
            channel_samples(&mut player, 8),
            [267, 269, 244, 241, 244, 247, 249, 247]
        );
    }

    fn note(attack: u8, release: u8) -> NoteDef {
        NoteDef {
            voice: Kind::Pcm {
                wave: 0,
                archive: 0,
            },
            base_note: 60,
            attack,
            decay: 127,
            sustain: 127,
            release,
            pan: 64,
        }
    }

    fn fixture_player(data: &[u8]) -> Player {
        let ramp: Vec<u8> = (0..64u8).map(|i| i.wrapping_mul(8)).collect();
        let swar = wave::build_swar(&[wave::build_info(WaveFormat::Pcm8, true, 0, &ramp)]);
        let bank = bank::build(&[
            Some(Instrument::Single(note(127, 120))),
            Some(Instrument::Single(NoteDef {
                voice: Kind::Square { duty: 3 },
                ..note(120, 100)
            })),
        ]);
        Player::sequence(
            &sseq::build(data),
            &bank,
            [Some(&swar), None, None, None],
            100,
            32_768,
        )
        .unwrap()
    }

    const SONG: [u8; 31] = [
        0xFE, 0x03, 0x00, 0x93, 0x01, 0x17, 0x00, 0x00, 0xE1, 0x96, 0x00, 0xC1, 0x70, 0x3C, 0x7F,
        0x18, 0x43, 0x60, 0x18, 0x48, 0x50, 0x30, 0xFF, 0x81, 0x01, 0xC0, 0x20, 0x30, 0x7F, 0x48,
        0xFF,
    ];

    #[test]
    fn a_sequence_renders_to_the_same_pcm_every_time() {
        let mut player = fixture_player(&SONG);
        let length = player.length().unwrap();
        assert_eq!(length.loop_start, None);
        assert!(
            (length.seconds - 0.8127).abs() < 0.001,
            "{}",
            length.seconds
        );
        let mut pcm = Vec::new();
        while !player.is_done() {
            player.render_pcm(1024, &mut pcm);
        }
        assert!(pcm.iter().any(|&s| s.unsigned_abs() > 1000));
        let hash = pcm.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, &s| {
            (hash ^ u64::from(s as u16)).wrapping_mul(0x0100_0000_01b3)
        });
        assert_eq!((pcm.len(), hash), (147_456, 2_636_582_395_492_530_685));
    }

    #[test]
    fn notes_start_on_the_frame_their_tick_falls_in() {
        let mut player = fixture_player(&SONG);
        let mut started = Vec::new();
        for frame in 0..200 {
            player.frame();
            for (channel, slot) in player.voices.iter().enumerate().take(16) {
                if let Some(voice) = *slot
                    && !started
                        .iter()
                        .any(|&(c, k, _)| c == channel && k == voice.key)
                {
                    started.push((channel, voice.key, frame));
                }
            }
        }
        assert_eq!(started, [(0, 60, 1), (8, 48, 1), (1, 67, 40), (2, 72, 78)]);
    }

    #[test]
    fn a_loop_back_to_a_command_inside_a_tick_finds_that_tick() {
        let player = fixture_player(&[0xC1, 0x7F, 0x3C, 0x7F, 48, 0x94, 2, 0, 0]);
        let frame = FRAME_CYCLES as f64 / ARM7_CLOCK;
        let length = player.length().unwrap();
        assert_eq!(length.loop_start, Some(frame));
        assert_eq!(length.seconds, 98.0 * frame);
    }

    #[test]
    fn a_looping_song_knows_where_it_loops_and_folds_its_position() {
        let mut player = fixture_player(&[0x3C, 0x7F, 48, 0x3E, 0x7F, 48, 0x94, 3, 0, 0]);
        let frame = FRAME_CYCLES as f64 / ARM7_CLOCK;
        let length = player.length().unwrap();
        assert_eq!(length.loop_start, Some(97.0 * frame));
        assert_eq!(length.seconds, 194.0 * frame);
        let mut pcm = Vec::new();
        player.render_pcm(48_000, &mut pcm);
        assert!(player.elapsed() > length.seconds);
        assert!(player.position() < length.seconds);
        assert!(player.position() >= 97.0 * frame);
        assert_eq!(player.loops(), 1);
        assert!(!player.is_done());
    }

    #[test]
    fn a_one_shot_wave_ends_and_reports_done() {
        let bytes = wave::build_swav(&wave::build_info(WaveFormat::Pcm8, false, 0, &[0x40; 64]));
        let wave = Wave::parse_swav(&bytes).unwrap();
        let mut player = Player::wave(&wave, 32_768);
        assert!(!player.is_done());
        let mut pcm = Vec::new();
        player.render_pcm(4096, &mut pcm);
        assert!(player.is_done());
        assert!(player.elapsed() > 0.1);
    }
}
