pub mod adpcm;
pub mod mixer;
pub mod output;
pub mod psg;

use adpcm::Adpcm;
use psg::Noise;

pub const CHANNELS: usize = 16;

const CNT: usize = 0x0;
const SAD: usize = 0x4;
const TMR_PNT: usize = 0x8;
const LEN: usize = 0xC;

const CNT_MASK: u32 = 0xFF7F_837F;
const SAD_MASK: u32 = 0x07FF_FFFC;
const LEN_MASK: u32 = 0x003F_FFFF;
const VOLUME: u32 = 0x7F;
const DIVIDER_SHIFT: u32 = 8;
const HOLD: u32 = 1 << 15;
const PAN_SHIFT: u32 = 16;
const DUTY_SHIFT: u32 = 24;
const REPEAT_SHIFT: u32 = 27;
const FORMAT_SHIFT: u32 = 29;
const START: u32 = 1 << 31;

const DIVIDER_SHIFTS: [u32; 4] = [0, 1, 2, 4];
const FULL_SCALE: i64 = 128;
const MIXER_DROP: u32 = 10;
const TIMER_RANGE: u32 = 0x1_0000;
const FIRST_SQUARE: usize = 8;
const FIRST_NOISE: usize = 14;
const PSG_DELAY: i32 = 1;
const PCM_DELAY: i32 = 2;
const ADPCM_DELAY: i32 = 10;
const ADPCM_HEADER: i32 = 8;
const FIFO_WORDS: usize = 8;
const FIFO_BYTES: usize = 4 * FIFO_WORDS;
const BURST_BYTES: u32 = 16;
const REFILL_LEVEL: u32 = 16;
const BIOS_END: u32 = 0x4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Pcm8,
    Pcm16,
    Adpcm,
    Psg,
}

impl Format {
    fn samples_per_word(self) -> i32 {
        match self {
            Format::Pcm8 => 4,
            Format::Pcm16 => 2,
            _ => 8,
        }
    }

    fn header_words(self) -> i32 {
        i32::from(self == Format::Adpcm)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repeat {
    Manual,
    Loop,
    OneShot,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Channel {
    index: usize,
    control: u32,
    source: u32,
    timer: u16,
    loop_start: u16,
    length: u32,
    counter: u32,
    odd_cycle: bool,
    position: i32,
    sample: i16,
    finishing: bool,
    adpcm: Adpcm,
    loop_adpcm: Adpcm,
    noise: Noise,
    #[cfg_attr(feature = "serde", serde(default))]
    fifo: Fifo,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Fifo {
    words: [u32; FIFO_WORDS],
    read: usize,
    write: usize,
    offset: u32,
    level: u32,
    primed: bool,
    adpcm_byte: u8,
}

impl Channel {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            ..Self::default()
        }
    }

    pub fn read(&self, offset: usize) -> u32 {
        if offset & !3 == CNT { self.control } else { 0 }
    }

    pub fn write(&mut self, offset: usize, value: u32, mask: u32) {
        match offset & !3 {
            CNT => {
                let was_running = self.running();
                self.control = merge(self.control, value, mask & CNT_MASK);
                if self.running() && !was_running {
                    self.start();
                } else if !self.running() && !self.finishing {
                    self.release();
                }
            }
            SAD => self.source = merge(self.source, value, mask & SAD_MASK),
            TMR_PNT => {
                let word = merge(
                    u32::from(self.timer) | (u32::from(self.loop_start) << 16),
                    value,
                    mask,
                );
                self.timer = word as u16;
                self.loop_start = (word >> 16) as u16;
            }
            LEN => self.length = merge(self.length, value, mask & LEN_MASK),
            _ => {}
        }
    }

    pub fn running(&self) -> bool {
        self.control & START != 0
    }

    pub fn tick(&mut self, cycles: u32, memory: &mut impl FnMut(u32) -> u32) {
        if !self.running() && !self.finishing {
            return;
        }
        let half = cycles + u32::from(self.odd_cycle);
        self.odd_cycle = !half.is_multiple_of(2);
        let mut ticks = half / 2;
        while ticks > 0 && (self.running() || self.finishing) {
            let until = TIMER_RANGE - self.counter;
            if ticks < until {
                self.counter += ticks;
                return;
            }
            ticks -= until;
            self.counter = u32::from(self.timer);
            self.advance(memory);
        }
    }

    pub fn sample(&self) -> i16 {
        self.sample
    }

    pub fn timer(&self) -> u16 {
        self.timer
    }

    pub fn output(&self) -> (i32, i32) {
        if self.sample == 0 {
            return (0, 0);
        }
        let divided = (i64::from(self.sample) << 4) >> DIVIDER_SHIFTS[self.divider()];
        let scaled = divided * seven_bit(self.control & VOLUME);
        let pan = seven_bit((self.control >> PAN_SHIFT) & 0x7F);
        let left = (scaled * (FULL_SCALE - pan)) >> MIXER_DROP;
        let right = (scaled * pan) >> MIXER_DROP;
        (left as i32, right as i32)
    }

    fn divider(&self) -> usize {
        ((self.control >> DIVIDER_SHIFT) & 3) as usize
    }

    fn hold(&self) -> bool {
        self.control & HOLD != 0
    }

    fn duty(&self) -> u8 {
        ((self.control >> DUTY_SHIFT) & 7) as u8
    }

    fn repeat(&self) -> Repeat {
        match (self.control >> REPEAT_SHIFT) & 3 {
            0 => Repeat::Manual,
            1 => Repeat::Loop,
            _ => Repeat::OneShot,
        }
    }

    fn format(&self) -> Format {
        match (self.control >> FORMAT_SHIFT) & 3 {
            0 => Format::Pcm8,
            1 => Format::Pcm16,
            2 => Format::Adpcm,
            _ => Format::Psg,
        }
    }

    fn start(&mut self) {
        self.counter = u32::from(self.timer);
        self.finishing = false;
        self.sample = 0;
        self.noise = Noise::default();
        self.fifo = Fifo::default();
        self.position = -match self.format() {
            Format::Pcm8 | Format::Pcm16 => PCM_DELAY,
            Format::Adpcm => ADPCM_DELAY,
            Format::Psg => PSG_DELAY,
        };
    }

    fn release(&mut self) {
        if !self.hold() {
            self.sample = 0;
        }
    }

    fn advance(&mut self, memory: &mut impl FnMut(u32) -> u32) {
        if self.finishing {
            self.finishing = false;
            self.release();
            return;
        }
        let format = self.format();
        if format == Format::Psg {
            if self.position < 0 {
                self.position += 1;
                return;
            }
            self.sample = self.psg();
            self.position = self.position.wrapping_add(1);
            return;
        }
        if !self.fifo.primed {
            self.fifo.primed = true;
            self.buffer(memory);
            self.buffer(memory);
        }
        if self.position < 0 {
            if format == Format::Adpcm && self.position == -ADPCM_HEADER {
                self.adpcm = Adpcm::from_header(self.pop(memory, 4));
            }
            self.position += 1;
            return;
        }
        let end = self.end(format);
        if self.position >= end && self.repeat() == Repeat::Loop {
            self.position = self.loop_sample(format);
            self.adpcm = self.loop_adpcm;
        }
        self.sample = self.fetch(format, memory);
        self.position += 1;
        if self.position >= end && self.repeat() == Repeat::OneShot {
            self.control &= !START;
            self.finishing = true;
        }
    }

    fn psg(&mut self) -> i16 {
        if self.index >= FIRST_NOISE {
            self.noise.step()
        } else if self.index >= FIRST_SQUARE {
            psg::square(self.duty(), (self.position % 8) as u8)
        } else {
            0
        }
    }

    fn fetch(&mut self, format: Format, memory: &mut impl FnMut(u32) -> u32) -> i16 {
        match format {
            Format::Pcm8 => i16::from(self.pop(memory, 1) as u8 as i8) << 8,
            Format::Pcm16 => self.pop(memory, 2) as u16 as i16,
            _ => self.adpcm_sample(memory),
        }
    }

    fn adpcm_sample(&mut self, memory: &mut impl FnMut(u32) -> u32) -> i16 {
        if self.position == self.loop_sample(Format::Adpcm) {
            self.loop_adpcm = self.adpcm;
        }
        let nibble = if self.position % 2 == 0 {
            self.fifo.adpcm_byte = self.pop(memory, 1) as u8;
            self.fifo.adpcm_byte & 0xF
        } else {
            self.fifo.adpcm_byte >> 4
        };
        self.adpcm.decode(nibble)
    }

    fn pop(&mut self, memory: &mut impl FnMut(u32) -> u32, bytes: u32) -> u32 {
        let fifo = &mut self.fifo;
        let mut value = 0;
        for shift in 0..bytes {
            let word = fifo.words[fifo.read / 4];
            value |= ((word >> ((fifo.read % 4) * 8)) & 0xFF) << (8 * shift);
            fifo.read = (fifo.read + 1) % FIFO_BYTES;
        }
        fifo.level = fifo.level.saturating_sub(bytes);
        if fifo.level <= REFILL_LEVEL {
            self.buffer(memory);
        }
        value
    }

    fn buffer(&mut self, memory: &mut impl FnMut(u32) -> u32) {
        let total = 4 * (u32::from(self.loop_start) + self.length);
        if self.fifo.offset >= total {
            match self.repeat() {
                Repeat::Loop => self.fifo.offset = 4 * u32::from(self.loop_start),
                Repeat::OneShot => return,
                Repeat::Manual => {}
            }
        }
        let burst = match total.checked_sub(self.fifo.offset) {
            Some(left) if left > 0 => left.min(BURST_BYTES),
            _ => BURST_BYTES,
        };
        for _ in 0..burst.div_ceil(4) {
            let address = self.source.wrapping_add(self.fifo.offset);
            self.fifo.words[self.fifo.write] = if address >= BIOS_END {
                memory(address & !3)
            } else {
                0
            };
            self.fifo.offset += 4;
            self.fifo.write = (self.fifo.write + 1) % FIFO_WORDS;
        }
        self.fifo.level += burst;
    }

    fn loop_sample(&self, format: Format) -> i32 {
        (i32::from(self.loop_start) - format.header_words()).max(0) * format.samples_per_word()
    }

    fn end(&self, format: Format) -> i32 {
        let words = i32::from(self.loop_start) + self.length as i32 - format.header_words();
        words.max(0) * format.samples_per_word()
    }
}

fn seven_bit(value: u32) -> i64 {
    if value == 0x7F {
        FULL_SCALE
    } else {
        i64::from(value)
    }
}

fn merge(old: u32, value: u32, mask: u32) -> u32 {
    (old & !mask) | (value & mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u32 = 0x0200_0000;
    const ALL: u32 = 0xFFFF_FFFF;
    const PCM8: u32 = 0;
    const PCM16: u32 = 1 << FORMAT_SHIFT;
    const ADPCM: u32 = 2 << FORMAT_SHIFT;
    const PSG: u32 = 3 << FORMAT_SHIFT;
    const LOOP: u32 = 1 << REPEAT_SHIFT;
    const ONE_SHOT: u32 = 2 << REPEAT_SHIFT;
    const LOUD: u32 = 0x7F | (0x40 << PAN_SHIFT);

    struct Rig {
        channel: Channel,
        words: Vec<u32>,
    }

    impl Rig {
        fn new(index: usize, words: Vec<u32>, loop_start: u16, length: u32) -> Self {
            let mut channel = Channel::new(index);
            channel.write(SAD, BASE, ALL);
            channel.write(TMR_PNT, 0xFFFF | (u32::from(loop_start) << 16), ALL);
            channel.write(LEN, length, ALL);
            Self { channel, words }
        }

        fn start(&mut self, control: u32) {
            self.channel.write(CNT, control | START, ALL);
        }

        fn samples(&mut self, count: usize) -> Vec<i16> {
            let words = &self.words;
            let mut memory = |address: u32| {
                let index = ((address - BASE) / 4) as usize;
                words.get(index).copied().unwrap_or(0)
            };
            (0..count)
                .map(|_| {
                    self.channel.tick(2, &mut memory);
                    self.channel.sample()
                })
                .collect()
        }
    }

    fn pcm8_words(bytes: &[i8]) -> Vec<u32> {
        bytes
            .chunks(4)
            .map(|chunk| {
                chunk
                    .iter()
                    .enumerate()
                    .fold(0, |word, (i, &b)| word | (u32::from(b as u8) << (i * 8)))
            })
            .collect()
    }

    fn ramp() -> Vec<u32> {
        pcm8_words(&(1..=16).collect::<Vec<i8>>())
    }

    fn scaled(values: &[i16]) -> Vec<i16> {
        values.iter().map(|v| v << 8).collect()
    }

    #[test]
    fn pcm8_plays_its_first_sample_on_the_third_timer_overflow_like_melonds() {
        let mut rig = Rig::new(0, ramp(), 0, 4);
        rig.start(PCM8 | LOUD);
        let mut expected = vec![0, 0];
        expected.extend(scaled(&[1, 2, 3, 4]));
        assert_eq!(rig.samples(6), expected);
    }

    #[test]
    fn pcm8_bytes_read_as_the_high_byte() {
        let mut rig = Rig::new(0, pcm8_words(&[-128, 127, 0, 1]), 0, 4);
        rig.start(PCM8);
        assert_eq!(rig.samples(4)[2..], [-0x8000, 0x7F00]);
    }

    #[test]
    fn pcm16_reads_halfwords_in_order() {
        let mut rig = Rig::new(0, vec![0x8000_1234, 0x0000_7FFF], 0, 4);
        rig.start(PCM16);
        assert_eq!(rig.samples(5)[2..], [0x1234, -0x8000, 0x7FFF]);
    }

    #[test]
    fn one_shot_stops_after_the_last_sample_plays_out() {
        let mut rig = Rig::new(0, ramp(), 0, 4);
        rig.start(PCM8 | ONE_SHOT);
        let out = rig.samples(2 + 16);
        assert_eq!(out[17], 16 << 8);
        assert!(!rig.channel.running());
        assert_eq!(rig.samples(1), [0]);
        assert_eq!(rig.samples(4), [0; 4]);
    }

    #[test]
    fn hold_keeps_the_last_sample_after_one_shot() {
        let mut rig = Rig::new(0, ramp(), 0, 4);
        rig.start(PCM8 | ONE_SHOT | HOLD);
        rig.samples(2 + 16);
        assert_eq!(rig.samples(3), [16 << 8; 3]);
        rig.channel.write(CNT, PCM8 | ONE_SHOT, ALL);
        assert_eq!(rig.channel.sample(), 0);
    }

    #[test]
    fn loop_mode_plays_the_start_once_then_repeats_the_loop() {
        let mut rig = Rig::new(0, ramp(), 1, 3);
        rig.start(PCM8 | LOOP);
        let out = rig.samples(2 + 16 + 14);
        let mut expected = vec![0, 0];
        expected.extend(scaled(&(1..=16).collect::<Vec<i16>>()));
        expected.extend(scaled(&(5..=16).collect::<Vec<i16>>()));
        expected.extend(scaled(&[5, 6]));
        assert_eq!(out, expected);
        assert!(rig.channel.running());
    }

    #[test]
    fn manual_mode_reads_on_past_the_length() {
        let mut words = ramp();
        words.push(pcm8_words(&[17, 18, 19, 20])[0]);
        let mut rig = Rig::new(0, words, 0, 4);
        rig.start(PCM8);
        assert_eq!(rig.samples(2 + 18)[18..], scaled(&[17, 18]));
    }

    #[test]
    fn adpcm_reads_its_header_and_then_nibbles_low_first() {
        let mut rig = Rig::new(0, vec![0x0000_0000, 0x0000_4F70], 0, 2);
        rig.start(ADPCM);
        let out = rig.samples(10 + 4);
        assert_eq!(out[..10], [0; 10]);
        assert_eq!(out[10..], [0, 11, -19, 19]);
    }

    #[test]
    fn adpcm_loops_restore_the_saved_state() {
        let mut rig = Rig::new(0, vec![0x0000_0000, 0x7777_7777, 0x0000_0000], 2, 1);
        rig.start(ADPCM | LOOP);
        let out = rig.samples(10 + 8 + 8 + 8);
        let first_loop = &out[10 + 8..10 + 16];
        let second_loop = &out[10 + 16..10 + 24];
        assert_eq!(first_loop, second_loop);
        assert_ne!(first_loop[0], 0);
    }

    #[test]
    fn adpcm_looping_from_the_first_data_word_replays_without_rereading_the_header() {
        let words = vec![0x0000_0000, 0x7777_7777, 0x1111_1111, 0x9999_9999];
        let mut rig = Rig::new(0, words, 1, 3);
        rig.start(ADPCM | LOOP);
        let out = rig.samples(10 + 24 * 3);
        let first = &out[10..34];
        assert_ne!(first[0], 0);
        assert_eq!(&out[34..58], first);
        assert_eq!(&out[58..82], first);
    }

    #[test]
    fn square_channels_play_their_duty() {
        let mut rig = Rig::new(8, Vec::new(), 0, 0);
        rig.start(PSG | (3 << DUTY_SHIFT));
        let out = rig.samples(1 + 16);
        assert_eq!(out[0], 0);
        let high = psg::HIGH;
        let low = psg::LOW;
        assert_eq!(out[1..9], [low, low, low, low, high, high, high, high]);
        assert_eq!(out[1..9], out[9..17]);
    }

    #[test]
    fn noise_channels_play_the_lfsr() {
        let mut rig = Rig::new(14, Vec::new(), 0, 0);
        rig.start(PSG);
        let mut noise = Noise::default();
        let expected: Vec<i16> = (0..8).map(|_| noise.step()).collect();
        assert_eq!(rig.samples(9)[1..], expected);
    }

    #[test]
    fn psg_format_is_silent_below_channel_8() {
        let mut rig = Rig::new(7, Vec::new(), 0, 0);
        rig.start(PSG);
        assert_eq!(rig.samples(9), [0; 9]);
    }

    #[test]
    fn a_restart_resets_the_square_phase() {
        let mut rig = Rig::new(9, Vec::new(), 0, 0);
        rig.start(PSG);
        rig.samples(5);
        rig.channel.write(CNT, PSG, ALL);
        rig.start(PSG);
        assert_eq!(rig.samples(2)[1], psg::LOW);
    }

    #[test]
    fn the_timer_counts_up_from_its_reload_at_half_the_cpu_clock() {
        let mut rig = Rig::new(0, ramp(), 0, 4);
        rig.channel.write(TMR_PNT, 0xFFFE, 0xFFFF);
        rig.start(PCM8);
        let words = rig.words.clone();
        let mut memory = |address: u32| {
            words
                .get(((address - BASE) / 4) as usize)
                .copied()
                .unwrap_or(0)
        };
        rig.channel.tick(4 * 2 + 3, &mut memory);
        assert_eq!(rig.channel.sample(), 0);
        rig.channel.tick(1, &mut memory);
        assert_eq!(rig.channel.sample(), 1 << 8);
    }

    #[test]
    fn full_volume_centre_pan_gives_half_each_side() {
        let mut channel = Channel::new(0);
        channel.sample = 0x7FFF;
        channel.control = LOUD;
        assert_eq!(channel.output(), (0x7FFF * 128, 0x7FFF * 128));
    }

    #[test]
    fn pan_0_and_127_put_the_sample_on_one_side() {
        let mut channel = Channel::new(0);
        channel.sample = 0x7FFF;
        channel.control = 0x7F;
        assert_eq!(channel.output(), (0x7FFF * 256, 0));
        channel.control = 0x7F | (0x7F << PAN_SHIFT);
        assert_eq!(channel.output(), (0, 0x7FFF * 256));
    }

    #[test]
    fn the_divider_shifts_by_0_1_2_or_4() {
        let mut channel = Channel::new(0);
        channel.sample = 0x4000;
        for (divider, expected) in [
            (0, 0x4000 * 256),
            (1, 0x2000 * 256),
            (2, 0x1000 * 256),
            (3, 0x400 * 256),
        ] {
            channel.control = 0x7F | (divider << DIVIDER_SHIFT);
            assert_eq!(channel.output().0, expected, "divider {divider}");
        }
    }

    #[test]
    fn volume_scales_by_n_over_128() {
        let mut channel = Channel::new(0);
        channel.sample = -0x4000;
        channel.control = 32;
        assert_eq!(channel.output().0, -0x4000 * 256 / 4);
    }

    #[test]
    fn only_the_control_register_reads_back() {
        let mut channel = Channel::new(0);
        channel.write(CNT, !START, ALL);
        channel.write(SAD, BASE, ALL);
        assert_eq!(channel.read(CNT), CNT_MASK & !START);
        assert_eq!(channel.read(SAD), 0);
    }
}
