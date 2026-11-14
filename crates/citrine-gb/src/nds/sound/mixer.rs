use super::{CHANNELS, Channel, seven_bit};

pub const FIRST: usize = 0x400;
pub const LAST: usize = 0x51F;
pub const MIX_CYCLES: u32 = 1024;

const CHANNELS_END: usize = 0x500;
const CHANNEL_BYTES: usize = 0x10;
const SOUNDCNT: usize = 0x500;
const SOUNDBIAS: usize = 0x504;
const SNDCAPCNT: usize = 0x508;
const SNDCAP0DAD: usize = 0x510;
const SNDCAP0LEN: usize = 0x514;
const SNDCAP1DAD: usize = 0x518;
const SNDCAP1LEN: usize = 0x51C;

const CONTROL_MASK: u32 = 0xBF7F;
const BIAS_MASK: u32 = 0x3FF;
const MASTER_VOLUME: u32 = 0x7F;
const LEFT_SHIFT: u32 = 8;
const RIGHT_SHIFT: u32 = 10;
const SKIP_CH1: u32 = 1 << 12;
const SKIP_CH3: u32 = 1 << 13;
const MASTER_ENABLE: u32 = 1 << 15;
const MASTER_SHIFT: u32 = 7;
const FRACTION_BITS: u32 = 8;
const BIAS_SHIFT: u32 = 6;
const CENTER: i32 = 0x8000;

const CAPTURE_ADD: u8 = 1 << 0;
const CAPTURE_CHANNEL: u8 = 1 << 1;
const CAPTURE_ONE_SHOT: u8 = 1 << 2;
const CAPTURE_PCM8: u8 = 1 << 3;
const CAPTURE_START: u8 = 1 << 7;
const CAPTURE_MASK: u8 = 0x8F;
const CAPTURE_ADDRESS_MASK: u32 = 0x07FF_FFFC;
const CAPTURE_LENGTH_MASK: u32 = 0xFFFF;
const FIFO_BYTES: usize = 16;
const FIFO_WORDS: usize = 4;
const TIMER_TICKS: u32 = MIX_CYCLES / 2;
const TIMER_RANGE: u32 = 0x1_0000;

pub trait SoundMemory {
    fn read_word(&mut self, address: u32) -> u32;
    fn write_word(&mut self, address: u32, value: u32);
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mixer {
    channels: Vec<Channel>,
    control: u32,
    bias: u32,
    captures: [Capture; 2],
    until_mix: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    samples: Vec<[i16; 2]>,
    #[cfg_attr(feature = "serde", serde(skip))]
    sample_due: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Capture {
    control: u8,
    destination: u32,
    length: u32,
    timer: u32,
    position: u32,
    written: u32,
    fifo: [u8; FIFO_BYTES],
    fifo_start: usize,
    fifo_level: usize,
}

impl Mixer {
    pub fn new() -> Self {
        Self {
            channels: (0..CHANNELS).map(Channel::new).collect(),
            until_mix: MIX_CYCLES,
            ..Self::default()
        }
    }

    pub fn channel(&self, index: usize) -> &Channel {
        &self.channels[index]
    }

    pub fn read(&self, offset: usize) -> u32 {
        match offset & !3 {
            FIRST..CHANNELS_END => {
                let at = offset - FIRST;
                self.channels[at / CHANNEL_BYTES].read(at % CHANNEL_BYTES)
            }
            SOUNDCNT => self.control,
            SOUNDBIAS => self.bias,
            SNDCAPCNT => {
                u32::from(self.captures[0].control) | u32::from(self.captures[1].control) << 8
            }
            SNDCAP0DAD => self.captures[0].destination,
            SNDCAP1DAD => self.captures[1].destination,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: usize, value: u32, mask: u32) {
        self.catch_up();
        match offset & !3 {
            FIRST..CHANNELS_END => {
                let at = offset - FIRST;
                self.channels[at / CHANNEL_BYTES].write(at % CHANNEL_BYTES, value, mask);
            }
            SOUNDCNT => self.control = merge(self.control, value, mask & CONTROL_MASK),
            SOUNDBIAS => self.bias = merge(self.bias, value, mask & BIAS_MASK),
            SNDCAPCNT => {
                for index in 0..2 {
                    let shift = 8 * index as u32;
                    if (mask >> shift) & 0xFF != 0 {
                        let reload = self.channels[1 + 2 * index].timer();
                        self.captures[index].write_control((value >> shift) as u8, reload);
                    }
                }
            }
            SNDCAP0DAD => self.captures[0].write_destination(value, mask),
            SNDCAP0LEN => self.captures[0].write_length(value, mask),
            SNDCAP1DAD => self.captures[1].write_destination(value, mask),
            SNDCAP1LEN => self.captures[1].write_length(value, mask),
            _ => {}
        }
    }

    pub fn until_mix(&self) -> u32 {
        self.until_mix
    }

    pub fn elapse(&mut self, cycles: u32) -> u32 {
        let mut mixes = 0;
        let mut left = cycles;
        while left >= self.until_mix {
            left -= self.until_mix;
            self.until_mix = MIX_CYCLES;
            mixes += 1;
        }
        self.until_mix -= left;
        mixes
    }

    pub fn mix(&mut self, memory: &mut impl SoundMemory, muted: bool) {
        self.sample_due = false;
        let mut read = |address: u32| memory.read_word(address);
        for channel in &mut self.channels {
            channel.tick(MIX_CYCLES, &mut read);
        }
        if muted && !self.capturing() {
            self.sample_due = true;
            return;
        }
        let outputs = self.outputs();
        let mixed = self.mixed(&outputs);
        if self.control & MASTER_ENABLE != 0 {
            for (index, source) in mixed.into_iter().enumerate() {
                if self.captures[index].running() {
                    let sample = self.capture_sample(index, source, &outputs);
                    let reload = self.channels[1 + 2 * index].timer();
                    self.captures[index].run(sample, reload, memory);
                }
            }
        }
        self.push_sample(&outputs, mixed);
    }

    pub fn take_samples(&mut self) -> std::vec::Drain<'_, [i16; 2]> {
        self.catch_up();
        self.samples.drain(..)
    }

    fn catch_up(&mut self) {
        if self.sample_due {
            self.sample_due = false;
            let outputs = self.outputs();
            let mixed = self.mixed(&outputs);
            self.push_sample(&outputs, mixed);
        }
    }

    fn capturing(&self) -> bool {
        self.control & MASTER_ENABLE != 0 && self.captures.iter().any(Capture::running)
    }

    fn outputs(&self) -> [(i32, i32); CHANNELS] {
        std::array::from_fn(|i| self.channels[i].output())
    }

    #[inline(always)]
    fn mixed(&self, outputs: &[(i32, i32); CHANNELS]) -> [i32; 2] {
        let (mut left, mut right) = outputs
            .iter()
            .fold((0i32, 0i32), |(left, right), &(l, r)| (left + l, right + r));
        for index in [1, 3] {
            if !self.reaches_mixer(index) {
                left -= outputs[index].0;
                right -= outputs[index].1;
            }
        }
        [left, right]
    }

    #[inline(always)]
    fn push_sample(
        &mut self,
        outputs: &[(i32, i32); CHANNELS],
        [mixed_left, mixed_right]: [i32; 2],
    ) {
        let (mut left, mut right) = (0, 0);
        if self.control & MASTER_ENABLE != 0 {
            left = self.select(self.control >> LEFT_SHIFT, mixed_left, outputs, |o| o.0);
            right = self.select(self.control >> RIGHT_SHIFT, mixed_right, outputs, |o| o.1);
        }
        let master = seven_bit(self.control & MASTER_VOLUME);
        let bias = (self.bias << BIAS_SHIFT) as i32 - CENTER;
        let level = |value: i32| {
            let scaled = ((i64::from(value) * master) >> MASTER_SHIFT) >> FRACTION_BITS;
            (scaled as i32 + bias).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
        };
        self.samples.push([level(left), level(right)]);
    }

    fn reaches_mixer(&self, index: usize) -> bool {
        match index {
            1 => self.control & SKIP_CH1 == 0 && !self.captures[0].mutes_partner(&self.channels[0]),
            3 => self.control & SKIP_CH3 == 0 && !self.captures[1].mutes_partner(&self.channels[2]),
            _ => true,
        }
    }

    fn select(
        &self,
        source: u32,
        mixed: i32,
        outputs: &[(i32, i32); CHANNELS],
        side: impl Fn((i32, i32)) -> i32,
    ) -> i32 {
        match source & 3 {
            0 => mixed,
            1 => side(outputs[1]),
            2 => side(outputs[3]),
            _ => side(outputs[1]) + side(outputs[3]),
        }
    }

    fn capture_sample(&self, index: usize, mixed: i32, outputs: &[(i32, i32); CHANNELS]) -> i32 {
        let capture = &self.captures[index];
        if capture.control & CAPTURE_CHANNEL == 0 {
            return (mixed >> FRACTION_BITS).clamp(i32::from(i16::MIN), i32::from(i16::MAX));
        }
        let side = |output: (i32, i32)| if index == 0 { output.0 } else { output.1 };
        let own = side(outputs[2 * index]) >> FRACTION_BITS;
        let partner = side(outputs[2 * index + 1]) >> FRACTION_BITS;
        if capture.control & CAPTURE_ADD != 0 {
            i32::from((own + partner) as i16)
        } else if own < 0 && partner < 0 {
            i32::from(i16::MIN)
        } else {
            own.clamp(i32::from(i16::MIN), i32::from(i16::MAX))
        }
    }
}

impl Capture {
    fn running(&self) -> bool {
        self.control & CAPTURE_START != 0
    }

    fn mutes_partner(&self, own: &Channel) -> bool {
        self.running() && self.control & CAPTURE_ADD != 0 && !own.running()
    }

    fn write_control(&mut self, value: u8, reload: u16) {
        if value & CAPTURE_START != 0 && !self.running() {
            self.timer = u32::from(reload);
            self.position = 0;
            self.written = 0;
            self.fifo_start = 0;
            self.fifo_level = 0;
        }
        let mut value = value & CAPTURE_MASK;
        if value & CAPTURE_START == 0 {
            value &= !CAPTURE_ADD;
        }
        self.control = value;
    }

    fn write_destination(&mut self, value: u32, mask: u32) {
        self.destination = merge(self.destination, value, mask & CAPTURE_ADDRESS_MASK);
    }

    fn write_length(&mut self, value: u32, mask: u32) {
        let words = merge(self.length / 4, value, mask & CAPTURE_LENGTH_MASK);
        self.length = words.max(1) * 4;
    }

    fn run(&mut self, sample: i32, reload: u16, memory: &mut impl SoundMemory) {
        self.timer += TIMER_TICKS;
        while self.timer >= TIMER_RANGE {
            self.timer = u32::from(reload) + self.timer - TIMER_RANGE;
            if self.control & CAPTURE_PCM8 != 0 {
                self.push(&[(sample >> 8) as u8], memory);
                self.position += 1;
            } else {
                self.push(&(sample as i16).to_le_bytes(), memory);
                self.position += 2;
            }
            if self.position >= self.length {
                self.flush(memory);
                if self.control & CAPTURE_ONE_SHOT != 0 {
                    self.control &= !CAPTURE_START;
                    return;
                }
                self.position = 0;
            }
        }
    }

    fn push(&mut self, bytes: &[u8], memory: &mut impl SoundMemory) {
        for &byte in bytes {
            self.fifo[(self.fifo_start + self.fifo_level) % FIFO_BYTES] = byte;
            self.fifo_level += 1;
        }
        if self.fifo_level >= FIFO_BYTES {
            self.flush(memory);
        }
    }

    fn flush(&mut self, memory: &mut impl SoundMemory) {
        for _ in 0..FIFO_WORDS {
            if self.fifo_level < 4 {
                return;
            }
            let word = u32::from_le_bytes(std::array::from_fn(|i| {
                self.fifo[(self.fifo_start + i) % FIFO_BYTES]
            }));
            memory.write_word(self.destination + self.written, word);
            self.fifo_start = (self.fifo_start + 4) % FIFO_BYTES;
            self.fifo_level -= 4;
            self.written += 4;
            if self.written >= self.length {
                self.written = 0;
                return;
            }
        }
    }
}

fn merge(old: u32, value: u32, mask: u32) -> u32 {
    (old & !mask) | (value & mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: u32 = 0xFFFF_FFFF;
    const BASE: u32 = 0x0200_0000;
    const CAPTURE_AT: u32 = 0x0200_1000;
    const PCM16: u32 = 1 << 29;
    const LOOP: u32 = 1 << 27;
    const START: u32 = 1 << 31;
    const FULL_CENTRE: u32 = 0x7F | (0x40 << 16);
    const ON_FULL: u32 = MASTER_ENABLE | 0x7F;
    const BIAS_CENTRE: u32 = 0x200;

    #[derive(Default)]
    struct Memory {
        words: std::collections::HashMap<u32, u32>,
    }

    impl SoundMemory for Memory {
        fn read_word(&mut self, address: u32) -> u32 {
            self.words.get(&address).copied().unwrap_or(0)
        }

        fn write_word(&mut self, address: u32, value: u32) {
            self.words.insert(address, value);
        }
    }

    fn channel_offset(index: usize, register: usize) -> usize {
        FIRST + index * CHANNEL_BYTES + register
    }

    fn steady(mixer: &mut Mixer, index: usize, control: u32) {
        mixer.write(channel_offset(index, 4), BASE, ALL);
        mixer.write(channel_offset(index, 8), 0xFE00, ALL);
        mixer.write(channel_offset(index, 12), 4, ALL);
        mixer.write(
            channel_offset(index, 0),
            control | PCM16 | LOOP | START,
            ALL,
        );
    }

    fn rig(level: i16) -> (Mixer, Memory) {
        let mut memory = Memory::default();
        let word = u32::from(level as u16) * 0x1_0001;
        for index in 0..4 {
            memory.write_word(BASE + 4 * index, word);
        }
        let mut mixer = Mixer::new();
        mixer.write(SOUNDCNT, ON_FULL, ALL);
        mixer.write(SOUNDBIAS, BIAS_CENTRE, ALL);
        (mixer, memory)
    }

    fn settle(mixer: &mut Mixer, memory: &mut Memory, mixes: usize) -> [i16; 2] {
        for _ in 0..mixes {
            mixer.mix(memory, false);
        }
        let last = *mixer.samples.last().unwrap();
        mixer.samples.clear();
        last
    }

    #[test]
    fn silence_at_the_centre_bias_mixes_to_zero() {
        let (mut mixer, mut memory) = rig(0);
        assert_eq!(settle(&mut mixer, &mut memory, 4), [0, 0]);
    }

    #[test]
    fn a_disabled_mixer_outputs_only_the_bias_per_gbatek() {
        let (mut mixer, mut memory) = rig(0x4000);
        steady(&mut mixer, 0, FULL_CENTRE);
        mixer.write(SOUNDCNT, 0x7F, ALL);
        mixer.write(SOUNDBIAS, 0x300, ALL);
        assert_eq!(settle(&mut mixer, &mut memory, 8), [0x4000, 0x4000]);
    }

    #[test]
    fn one_channel_at_full_volume_and_centre_pan_gives_half_each_side() {
        let (mut mixer, mut memory) = rig(0x4000);
        steady(&mut mixer, 0, FULL_CENTRE);
        assert_eq!(settle(&mut mixer, &mut memory, 8), [0x2000, 0x2000]);
    }

    #[test]
    fn master_volume_scales_by_n_over_128() {
        let (mut mixer, mut memory) = rig(0x4000);
        steady(&mut mixer, 0, FULL_CENTRE);
        mixer.write(SOUNDCNT, MASTER_ENABLE | 0x40, ALL);
        assert_eq!(settle(&mut mixer, &mut memory, 8), [0x1000, 0x1000]);
    }

    #[test]
    fn the_output_clips_at_the_16_bit_range() {
        let (mut mixer, mut memory) = rig(0x7FFF);
        for index in 0..4 {
            steady(&mut mixer, index, FULL_CENTRE);
        }
        assert_eq!(settle(&mut mixer, &mut memory, 8), [0x7FFF, 0x7FFF]);
    }

    #[test]
    fn channel_1_can_skip_the_mixer_and_feed_the_left_output_alone() {
        let (mut mixer, mut memory) = rig(0x4000);
        steady(&mut mixer, 0, FULL_CENTRE);
        steady(&mut mixer, 1, 0x7F);
        mixer.write(SOUNDCNT, ON_FULL | SKIP_CH1, ALL);
        assert_eq!(settle(&mut mixer, &mut memory, 8), [0x2000, 0x2000]);
        mixer.write(SOUNDCNT, ON_FULL | SKIP_CH1 | 1 << LEFT_SHIFT, ALL);
        assert_eq!(settle(&mut mixer, &mut memory, 1), [0x4000, 0x2000]);
    }

    #[test]
    fn capture_0_writes_the_left_mixer_as_pcm16_and_stops_after_one_shot() {
        let (mut mixer, mut memory) = rig(0x4000);
        steady(&mut mixer, 0, FULL_CENTRE);
        mixer.write(channel_offset(1, 8), 0xFE00, 0xFFFF);
        mixer.write(SNDCAP0DAD, CAPTURE_AT, ALL);
        mixer.write(SNDCAP0LEN, 4, ALL);
        mixer.write(SNDCAPCNT, u32::from(CAPTURE_START | CAPTURE_ONE_SHOT), ALL);
        settle(&mut mixer, &mut memory, 64);
        assert_eq!(mixer.read(SNDCAPCNT) & 0x80, 0);
        let words: Vec<u32> = (0..4)
            .map(|i| memory.read_word(CAPTURE_AT + 4 * i))
            .collect();
        assert_eq!(words.last(), Some(&0x2000_2000));
        assert_eq!(memory.read_word(CAPTURE_AT + 16), 0);
    }

    #[test]
    fn a_capture_ring_played_back_on_channel_1_comes_out_as_one_steady_delayed_copy() {
        let mut memory = Memory::default();
        for index in 0..128u32 {
            let low = (2 * index) * 64;
            let high = (2 * index + 1) * 64;
            memory.write_word(BASE + 4 * index, low | high << 16);
        }
        let mut mixer = Mixer::new();
        mixer.write(SOUNDCNT, ON_FULL | SKIP_CH1 | 1 << LEFT_SHIFT, ALL);
        mixer.write(SOUNDBIAS, BIAS_CENTRE, ALL);
        mixer.write(channel_offset(0, 4), BASE, ALL);
        mixer.write(channel_offset(0, 8), 0xFE00, ALL);
        mixer.write(channel_offset(0, 12), 128, ALL);
        mixer.write(
            channel_offset(0, 0),
            FULL_CENTRE | PCM16 | LOOP | START,
            ALL,
        );
        mixer.write(channel_offset(1, 4), CAPTURE_AT, ALL);
        mixer.write(channel_offset(1, 8), 0xFE00, ALL);
        mixer.write(channel_offset(1, 12), 32, ALL);
        mixer.write(SNDCAP0DAD, CAPTURE_AT, ALL);
        mixer.write(SNDCAP0LEN, 32, ALL);
        mixer.write(SNDCAPCNT, u32::from(CAPTURE_START), ALL);
        mixer.write(channel_offset(1, 0), 0x7F | PCM16 | LOOP | START, ALL);
        settle(&mut mixer, &mut memory, 300);
        for _ in 0..600 {
            mixer.mix(&mut memory, false);
        }
        let left: Vec<i32> = mixer.samples.iter().map(|s| i32::from(s[0])).collect();
        let uneven = left
            .windows(2)
            .filter(|pair| pair[1] - pair[0] != 32 && pair[1] - pair[0] != 32 - 256 * 32)
            .count();
        assert_eq!(uneven, 0, "{:?}", &left[..40]);
    }

    #[test]
    fn a_capture_length_of_zero_counts_as_one_word_per_gbatek() {
        let mut capture = Capture::default();
        capture.write_length(0, ALL);
        assert_eq!(capture.length, 4);
    }

    #[test]
    fn mixes_come_every_1024_cycles() {
        let mut mixer = Mixer::new();
        assert_eq!(mixer.elapse(1023), 0);
        assert_eq!(mixer.elapse(1), 1);
        assert_eq!(mixer.elapse(3 * MIX_CYCLES + 5), 3);
        assert_eq!(mixer.until_mix(), MIX_CYCLES - 5);
    }

    #[test]
    fn a_muted_mixer_ends_each_frame_on_the_sample_a_heard_one_ends_on() {
        let (mut heard, mut heard_memory) = rig(0x4000);
        let (mut muted, mut muted_memory) = rig(0x4000);
        steady(&mut heard, 0, FULL_CENTRE);
        steady(&mut muted, 0, FULL_CENTRE);
        for frame in 0..4 {
            for mix in 0..6 {
                for (mixer, memory, quiet) in [
                    (&mut heard, &mut heard_memory, false),
                    (&mut muted, &mut muted_memory, true),
                ] {
                    if mix == frame {
                        let volume = 0x10 * mix + frame + 1;
                        mixer.write(channel_offset(0, 0), volume | PCM16 | LOOP | START, ALL);
                    }
                    mixer.mix(memory, quiet);
                    if mix == 5 && frame % 2 == 1 {
                        mixer.write(SOUNDCNT, MASTER_ENABLE | (0x20 + frame), ALL);
                    }
                }
            }
            let heard_samples: Vec<_> = heard.take_samples().collect();
            let muted_samples: Vec<_> = muted.take_samples().collect();
            assert_eq!(muted_samples.last(), heard_samples.last(), "frame {frame}");
            assert!(muted_samples.len() < heard_samples.len(), "frame {frame}");
            assert_eq!(muted, heard, "frame {frame}");
        }
    }
}
