mod fifo;
mod output;
mod psg;
mod wave;

use super::timer::{Overflows, Timers};
use fifo::Fifo;
use output::Output;
use psg::{Levels, Psg};
use std::borrow::Cow;

pub const CLOCK_RATE: u32 = 16_777_216;
pub const CHANNEL_NAMES: [&str; 6] = [
    "Pulse 1",
    "Pulse 2",
    "Wave",
    "Noise",
    "Direct Sound A",
    "Direct Sound B",
];
pub const FIFO_ADDRESSES: [u32; 2] = [0x0400_00A0, 0x0400_00A4];

const SOUNDCNT_H: usize = 0x22;
const SOUNDCNT_H_HIGH: usize = SOUNDCNT_H + 1;
const SOUNDCNT_X: usize = 0x24;
const SOUNDBIAS: usize = 0x28;
const SOUNDBIAS_HIGH: usize = SOUNDBIAS + 1;
const REGISTER_BYTES: usize = 0x30;
const WAVE_RAM_FIRST: usize = 0x30;
const WAVE_RAM_LAST: usize = 0x3F;
const FIFO_A_FIRST: usize = 0x40;
const FIFO_A_LAST: usize = 0x43;
const FIFO_B_FIRST: usize = 0x44;
const FIFO_B_LAST: usize = 0x47;
const PSG_LAST: usize = 0x21;
const MASTER_ENABLE: u8 = 0x80;
const FIFO_RESETS: [u8; 2] = [0x08, 0x80];
const DEFAULT_BIAS: u16 = 0x0200;
const DAC_MAX: i32 = 0x3FF;
const BASE_SAMPLE_CYCLES: u32 = 512;
const PSG_CYCLES: u32 = 4;
const PSG_SHIFTS: [u32; 4] = [2, 1, 0, 0];
const READ_MASKS: [u16; REGISTER_BYTES / 2] = [
    0x007F, 0xFFC0, 0x4000, 0x0000, 0xFFC0, 0x0000, 0x4000, 0x0000, 0x00E0, 0xE000, 0x4000, 0x0000,
    0xFF00, 0x0000, 0x40FF, 0x0000, 0xFF77, 0x770F, 0x0080, 0x0000, 0xC3FE, 0x0000, 0x0000, 0x0000,
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Dac {
    pub left: u16,
    pub right: u16,
}

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct Sound {
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::bytes::array"))]
    registers: [u8; REGISTER_BYTES],
    psg: Psg,
    fifos: [Fifo; 2],
    psg_cycles: u32,
    sample_cycles: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    stepped: u32,
    dac: Dac,
    #[cfg_attr(feature = "serde", serde(skip))]
    output: Output,
    #[cfg_attr(feature = "serde", serde(skip))]
    muted: [bool; CHANNEL_NAMES.len()],
    #[cfg(feature = "debug")]
    #[cfg_attr(feature = "serde", serde(skip))]
    taps: Option<Vec<[f32; CHANNEL_NAMES.len()]>>,
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize)]
#[serde(rename = "Sound")]
struct SavedSound<'a> {
    #[serde(serialize_with = "crate::utils::bytes::array::serialize")]
    registers: [u8; REGISTER_BYTES],
    psg: &'a Psg,
    fifos: &'a [Fifo; 2],
    psg_cycles: u32,
    sample_cycles: u32,
    dac: Dac,
}

#[cfg(feature = "serde")]
impl serde::Serialize for Sound {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let psg = self.psg_now();
        let saved = SavedSound {
            registers: self.registers,
            psg: &psg,
            fifos: &self.fifos,
            psg_cycles: self.psg_cycles % PSG_CYCLES,
            sample_cycles: self.sample_cycles,
            dac: self.dac,
        };
        serde::Serialize::serialize(&saved, serializer)
    }
}

#[cfg(feature = "debug")]
const MAX_TAPS: usize = 4096;

impl Default for Sound {
    fn default() -> Self {
        let mut registers = [0; REGISTER_BYTES];
        registers[SOUNDBIAS..SOUNDBIAS + 2].copy_from_slice(&DEFAULT_BIAS.to_le_bytes());
        Self {
            registers,
            psg: Psg::default(),
            fifos: Default::default(),
            psg_cycles: 0,
            sample_cycles: 0,
            stepped: 0,
            dac: Dac::default(),
            output: Output::default(),
            muted: [false; CHANNEL_NAMES.len()],
            #[cfg(feature = "debug")]
            taps: None,
        }
    }
}

impl Sound {
    pub fn read(&self, offset: usize) -> u16 {
        match offset {
            0..REGISTER_BYTES => {
                let value = self.half(offset) & READ_MASKS[offset / 2];
                if offset == SOUNDCNT_X {
                    value | u16::from(self.psg_now().enabled_flags())
                } else {
                    value
                }
            }
            WAVE_RAM_FIRST..=WAVE_RAM_LAST => {
                let index = offset - WAVE_RAM_FIRST;
                let psg = self.psg_now();
                u16::from_le_bytes([psg.ch3.read_ram(index), psg.ch3.read_ram(index + 1)])
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: usize, value: u16, mask: u16) {
        if mask == 0xFFFF && offset & 1 == 0 && (FIFO_A_FIRST..=FIFO_B_LAST).contains(&offset) {
            if self.master_enabled() {
                let fifo = &mut self.fifos[(offset - FIFO_A_FIRST) / 4];
                let [low, high] = value.to_le_bytes();
                fifo.push(low);
                fifo.push(high);
            }
            return;
        }
        for (lane, (byte, lane_mask)) in value
            .to_le_bytes()
            .into_iter()
            .zip(mask.to_le_bytes())
            .enumerate()
        {
            if lane_mask != 0 {
                self.write_byte(offset + lane, byte);
            }
        }
    }

    fn write_byte(&mut self, offset: usize, value: u8) {
        if matches!(offset, 0..=PSG_LAST | SOUNDCNT_X | WAVE_RAM_FIRST..=WAVE_RAM_LAST) {
            self.run_psg();
        }
        match offset {
            0..=PSG_LAST => {
                if self.master_enabled() {
                    self.registers[offset] = value;
                    self.psg.write(offset, value);
                }
            }
            SOUNDCNT_H => self.registers[offset] = value,
            SOUNDCNT_H_HIGH => {
                self.registers[offset] = value & !(FIFO_RESETS[0] | FIFO_RESETS[1]);
                for (fifo, reset) in self.fifos.iter_mut().zip(FIFO_RESETS) {
                    if value & reset != 0 {
                        fifo.reset();
                    }
                }
            }
            SOUNDCNT_X => self.write_master(value),
            SOUNDBIAS | SOUNDBIAS_HIGH => self.registers[offset] = value,
            WAVE_RAM_FIRST..=WAVE_RAM_LAST => {
                self.psg.ch3.write_ram(offset - WAVE_RAM_FIRST, value)
            }
            FIFO_A_FIRST..=FIFO_A_LAST if self.master_enabled() => self.fifos[0].push(value),
            FIFO_B_FIRST..=FIFO_B_LAST if self.master_enabled() => self.fifos[1].push(value),
            _ => {}
        }
    }

    fn write_master(&mut self, value: u8) {
        let was_enabled = self.master_enabled();
        self.registers[SOUNDCNT_X] = value & MASTER_ENABLE;
        match (was_enabled, self.master_enabled()) {
            (false, true) => self.psg.power_on(),
            (true, false) => {
                self.psg.power_off();
                self.registers[..=PSG_LAST].fill(0);
                self.fifos.iter_mut().for_each(Fifo::reset);
            }
            _ => {}
        }
    }

    fn half(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.registers[offset], self.registers[offset + 1]])
    }

    pub fn master_enabled(&self) -> bool {
        self.registers[SOUNDCNT_X] & MASTER_ENABLE != 0
    }

    fn control(&self) -> u16 {
        self.half(SOUNDCNT_H)
    }

    pub fn fifo_timer(&self, fifo: usize) -> usize {
        usize::from((self.control() >> (10 + 4 * fifo)) & 1)
    }

    fn bias(&self) -> u16 {
        self.half(SOUNDBIAS)
    }

    fn resolution(&self) -> u32 {
        u32::from(self.bias() >> 14)
    }

    pub fn sample_cycles(&self) -> u32 {
        BASE_SAMPLE_CYCLES >> self.resolution()
    }

    pub fn fifo_len(&self, fifo: usize) -> usize {
        self.fifos[fifo].len()
    }

    pub fn dac(&self) -> Dac {
        self.dac
    }

    #[inline(always)]
    fn step(&mut self, cycles: u32) {
        if cycles < self.sample_cycles().saturating_sub(self.sample_cycles) {
            if self.master_enabled() {
                self.psg_cycles += cycles;
            }
            self.output.advance(cycles);
            self.sample_cycles += cycles;
        } else {
            self.step_samples(cycles);
        }
    }

    #[inline(never)]
    fn step_samples(&mut self, cycles: u32) {
        let mut cycles = cycles;
        while cycles > 0 {
            let period = self.sample_cycles();
            let chunk = cycles.min(period.saturating_sub(self.sample_cycles).max(1));
            if self.master_enabled() {
                self.psg_cycles += chunk;
            }
            self.output.advance(chunk);
            self.sample_cycles += chunk;
            cycles -= chunk;
            if self.sample_cycles >= period {
                self.sample_cycles = 0;
                self.run_psg();
                self.sample();
            }
        }
    }

    fn run_psg(&mut self) {
        self.psg.run(self.psg_cycles / PSG_CYCLES);
        self.psg_cycles %= PSG_CYCLES;
    }

    fn psg_now(&self) -> Cow<'_, Psg> {
        let ticks = self.psg_cycles / PSG_CYCLES;
        if ticks == 0 {
            return Cow::Borrowed(&self.psg);
        }
        let mut psg = self.psg.clone();
        psg.run(ticks);
        Cow::Owned(psg)
    }

    #[inline(always)]
    pub fn timer_overflows(&mut self, overflows: Overflows, timers: &Timers) -> [bool; 2] {
        if !self.master_enabled() || overflows.count(0) | overflows.count(1) == 0 {
            return [false; 2];
        }
        self.tick_fifos(overflows, timers)
    }

    pub fn finish_step(&mut self, cycles: u32) {
        self.step_to(cycles);
        self.stepped = 0;
    }

    #[inline(always)]
    pub(crate) fn step_to(&mut self, at: u32) {
        if at > self.stepped {
            self.step(at - self.stepped);
            self.stepped = at;
        }
    }

    #[inline(never)]
    fn tick_fifos(&mut self, overflows: Overflows, timers: &Timers) -> [bool; 2] {
        let timer = [0, 1].map(|fifo| self.fifo_timer(fifo));
        let period = timer.map(|timer| timers.period(timer));
        let mut next = timer.map(|timer| overflows.first_at(timer));
        let mut left = timer.map(|timer| overflows.count(timer));
        let mut requests = [false; 2];
        while let Some(fifo) = (0..2)
            .filter(|&fifo| left[fifo] > 0)
            .min_by_key(|&fifo| next[fifo])
        {
            self.step_to(next[fifo]);
            requests[fifo] |= self.fifos[fifo].tick();
            next[fifo] = next[fifo].saturating_add(period[fifo]);
            left[fifo] -= 1;
        }
        requests
    }

    fn sample(&mut self) {
        #[cfg(feature = "debug")]
        self.tap();
        let levels = self.levels();
        let bias = i32::from(self.bias() & 0x03FE);
        let mask = (0x03FE_u16 << self.resolution()) & 0x03FF;
        let dac = |level: i32| (level + bias).clamp(0, DAC_MAX) as u16 & mask;
        self.dac = Dac {
            left: dac(levels.left),
            right: dac(levels.right),
        };
        self.output.set(
            i32::from(self.dac.left) - bias,
            i32::from(self.dac.right) - bias,
        );
    }

    fn levels(&self) -> Levels {
        if !self.master_enabled() {
            return Levels::default();
        }
        let control = self.control();
        let [ch1, ch2, ch3, ch4, ..] = self.muted;
        let psg = self.psg.levels([ch1, ch2, ch3, ch4]);
        let shift = PSG_SHIFTS[usize::from(control & 0b11)];
        let [a, b] = [self.direct_sound(0, control), self.direct_sound(1, control)];
        Levels {
            left: (psg.left >> shift) + a.left + b.left,
            right: (psg.right >> shift) + a.right + b.right,
        }
    }

    #[inline(always)]
    fn direct_sound(&self, index: usize, control: u16) -> Levels {
        let heard = -i32::from(!self.muted[4 + index]);
        let volume = 1 + ((control >> (2 + index)) & 1);
        let level = (i32::from(self.fifos[index].sample()) << volume) & heard;
        let routing = i32::from(control >> (8 + 4 * index));
        Levels {
            left: level & -((routing >> 1) & 1),
            right: level & -(routing & 1),
        }
    }

    pub(crate) fn clamp_loaded(&mut self) {
        self.psg_cycles %= PSG_CYCLES;
        self.sample_cycles = self.sample_cycles.min(BASE_SAMPLE_CYCLES - 1);
    }

    pub fn copy_state_from(&mut self, source: &Sound) {
        let Sound {
            registers,
            psg,
            fifos,
            psg_cycles,
            sample_cycles,
            stepped,
            dac,
            output: _,
            muted: _,
            #[cfg(feature = "debug")]
                taps: _,
        } = self;
        *registers = source.registers;
        psg.clone_from(&source.psg);
        fifos.clone_from(&source.fifos);
        *psg_cycles = source.psg_cycles;
        *sample_cycles = source.sample_cycles;
        *stepped = source.stepped;
        *dac = source.dac;
    }

    pub fn set_silent(&mut self, silent: bool) {
        self.output.set_silent(silent);
    }

    #[cfg(feature = "debug")]
    pub fn record_taps(&mut self, on: bool) {
        if on != self.taps.is_some() {
            self.taps = on.then(Vec::new);
        }
    }

    #[cfg(feature = "debug")]
    pub fn take_taps(&mut self) -> Vec<[f32; CHANNEL_NAMES.len()]> {
        self.taps.as_mut().map(std::mem::take).unwrap_or_default()
    }

    #[cfg(feature = "debug")]
    fn tap(&mut self) {
        let Some(taps) = &self.taps else {
            return;
        };
        if taps.len() >= MAX_TAPS {
            return;
        }
        let mut levels = [0.0; CHANNEL_NAMES.len()];
        if self.master_enabled() {
            for (level, psg) in levels.iter_mut().zip(self.psg.channel_levels()) {
                *level = psg as f32 / 15.0;
            }
            let control = self.control();
            for (index, (level, fifo)) in levels[4..].iter_mut().zip(&self.fifos).enumerate() {
                if (control >> (8 + 4 * index)) & 0b11 != 0 {
                    let sample = f32::from(fifo.sample()) / 128.0;
                    *level = if sample == 0.0 { f32::EPSILON } else { sample };
                }
            }
        }
        if let Some(taps) = &mut self.taps {
            taps.push(levels);
        }
    }

    pub fn take_audio(&mut self, out: &mut Vec<f32>) {
        self.output.take(out);
    }

    pub fn sample_rate(&self) -> u32 {
        self.output.sample_rate()
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.output.set_sample_rate(rate);
    }

    pub fn channel_enabled(&self, index: usize) -> bool {
        self.muted.get(index).is_some_and(|muted| !muted)
    }

    pub fn keep_settings(&mut self, old: &Sound) {
        self.set_sample_rate(old.sample_rate());
        self.muted = old.muted;
    }

    pub fn set_channel_enabled(&mut self, index: usize, enabled: bool) {
        if let Some(muted) = self.muted.get_mut(index) {
            *muted = !enabled;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::bus::Bus;
    use crate::gba::timer::START_DELAY;

    #[test]
    fn loaded_sound_counters_past_a_sample_are_cut_to_fit() {
        let mut sound = Sound {
            psg_cycles: u32::MAX,
            sample_cycles: u32::MAX,
            ..Sound::default()
        };
        sound.clamp_loaded();
        assert!(sound.psg_cycles < PSG_CYCLES && sound.sample_cycles < BASE_SAMPLE_CYCLES);
        sound.step(4 * BASE_SAMPLE_CYCLES + 3);
    }

    #[test]
    fn real_sound_counters_are_unchanged_by_the_load_clamp() {
        let mut sound = Sound::default();
        sound.step(3 * BASE_SAMPLE_CYCLES + 77);
        sound.psg_cycles %= PSG_CYCLES;
        let before = (sound.psg_cycles, sound.sample_cycles);
        sound.clamp_loaded();
        assert_eq!((sound.psg_cycles, sound.sample_cycles), before);
    }

    const SOUND1CNT_L: u32 = 0x0400_0060;
    const SOUND1CNT_H: u32 = 0x0400_0062;
    const SOUND1CNT_X: u32 = 0x0400_0064;
    const SOUND2CNT_L: u32 = 0x0400_0068;
    const SOUND2CNT_H: u32 = 0x0400_006C;
    const SOUND3CNT_L: u32 = 0x0400_0070;
    const SOUND3CNT_H: u32 = 0x0400_0072;
    const SOUND3CNT_X: u32 = 0x0400_0074;
    const SOUND4CNT_L: u32 = 0x0400_0078;
    const SOUND4CNT_H: u32 = 0x0400_007C;
    const SOUNDCNT_L: u32 = 0x0400_0080;
    const SOUNDCNT_H: u32 = 0x0400_0082;
    const SOUNDCNT_X: u32 = 0x0400_0084;
    const SOUNDBIAS: u32 = 0x0400_0088;
    const WAVE_RAM: u32 = 0x0400_0090;
    const FIFO_A: u32 = 0x0400_00A0;
    const FIFO_B: u32 = 0x0400_00A4;
    const DMA1SAD: u32 = 0x0400_00BC;
    const DMA1DAD: u32 = 0x0400_00C0;
    const DMA1CNT_H: u32 = 0x0400_00C6;
    const TM0CNT_L: u32 = 0x0400_0100;
    const TM0CNT_H: u32 = 0x0400_0102;
    const TM1CNT_L: u32 = 0x0400_0104;
    const TM1CNT_H: u32 = 0x0400_0106;
    const EWRAM: u32 = 0x0200_0000;
    const SILENCE: u16 = 0x200;
    const QUARTER_SECOND: u32 = CLOCK_RATE / 4;
    const FIFO_DMA: u16 = 0xB640;
    const TIMER_ON: u16 = 0x0080;

    fn bus() -> Bus {
        Bus::new(&[])
    }

    fn sound_on(bus: &mut Bus, control: u16) {
        bus.write16(SOUNDCNT_X, 0x0080);
        bus.write16(SOUNDCNT_L, 0xFF77);
        bus.write16(SOUNDCNT_H, control);
    }

    fn run(bus: &mut Bus, cycles: u32) {
        let mut left = cycles;
        while left > 0 {
            let chunk = left.min(64);
            bus.step(chunk);
            if bus.dma_active() {
                bus.run_dma();
            }
            left -= chunk;
        }
    }

    fn record(bus: &mut Bus, cycles: u32) -> Vec<Dac> {
        let every = bus.sound().sample_cycles();
        (0..cycles / every)
            .map(|_| {
                run(bus, every);
                bus.sound().dac()
            })
            .collect()
    }

    fn lefts(samples: &[Dac]) -> Vec<u16> {
        samples.iter().map(|dac| dac.left).collect()
    }

    fn rising_edges(levels: &[u16]) -> usize {
        levels.windows(2).filter(|pair| pair[1] > pair[0]).count()
    }

    fn changes(levels: &[u16]) -> Vec<u16> {
        let mut changes = levels.to_vec();
        changes.dedup();
        changes
    }

    fn start_timer(bus: &mut Bus, timer: u32, period: u16) {
        let (reload, control) = if timer == 0 {
            (TM0CNT_L, TM0CNT_H)
        } else {
            (TM1CNT_L, TM1CNT_H)
        };
        bus.write16(reload, period.wrapping_neg());
        bus.write16(control, TIMER_ON);
    }

    fn push(bus: &mut Bus, fifo: u32, samples: [i8; 4]) {
        bus.write32(fifo, u32::from_le_bytes(samples.map(|sample| sample as u8)));
    }

    fn fill_wave_bank(bus: &mut Bus, bytes: [u8; 16]) {
        for (index, pair) in bytes.chunks(2).enumerate() {
            bus.write16(
                WAVE_RAM + 2 * index as u32,
                u16::from_le_bytes([pair[0], pair[1]]),
            );
        }
    }

    #[test]
    fn sound1_plays_a_square_at_the_period_its_registers_set() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0002);
        bus.write16(SOUND1CNT_H, 0xF080);
        bus.write16(SOUND1CNT_X, 0x8000 | 1920);

        let levels = lefts(&record(&mut bus, QUARTER_SECOND));

        assert!(
            (255..=257).contains(&rising_edges(&levels)),
            "{}",
            rising_edges(&levels)
        );
        let mut seen = levels[16..].to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen, [SILENCE - 120, SILENCE + 120]);
        assert_eq!(bus.read16(SOUNDCNT_X), 0x0081);
    }

    #[test]
    fn psg_registers_read_back_without_their_write_only_bits() {
        let mut bus = bus();
        sound_on(&mut bus, 0);
        bus.write16(SOUND1CNT_H, 0xF0BF);
        bus.write16(SOUND1CNT_X, 0x4780);
        bus.write16(SOUND2CNT_L, 0xA0FF);
        bus.write16(SOUND2CNT_H, 0xC123);
        bus.write16(SOUNDCNT_H, 0xFFFF);

        assert_eq!(bus.read16(SOUND1CNT_H), 0xF080);
        assert_eq!(bus.read16(SOUND1CNT_X), 0x4000);
        assert_eq!(bus.read16(SOUND2CNT_L), 0xA0C0);
        assert_eq!(bus.read16(SOUND2CNT_H), 0x4000);
        assert_eq!(bus.read16(SOUNDCNT_L), 0xFF77);
        assert_eq!(bus.read16(SOUNDCNT_H), 0x770F);
        assert_eq!(bus.read16(SOUNDCNT_X), 0x0082);
        assert_eq!(bus.read16(SOUNDBIAS), SILENCE);
    }

    #[test]
    fn master_off_clears_the_psg_registers_and_ignores_writes_to_them() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0002);
        bus.write16(SOUND1CNT_H, 0xF080);

        bus.write16(SOUNDCNT_X, 0);
        bus.write16(SOUNDCNT_L, 0x1234);

        assert_eq!(bus.read16(SOUNDCNT_L), 0);
        assert_eq!(bus.read16(SOUND1CNT_H), 0);
        assert_eq!(bus.read16(SOUNDCNT_H), 0x0002);
        bus.write16(SOUNDCNT_X, 0x0080);
        bus.write16(SOUNDCNT_L, 0x1234);
        assert_eq!(bus.read16(SOUNDCNT_L), 0x1234 & 0xFF77);
    }

    #[test]
    fn master_off_empties_the_fifos_and_drops_writes_to_them() {
        let mut bus = bus();
        sound_on(&mut bus, 0);
        bus.write32(FIFO_A, 0x0403_0201);
        bus.write32(FIFO_B, 0x0403_0201);

        bus.write16(SOUNDCNT_X, 0);
        assert_eq!(bus.sound().fifo_len(0), 0);
        assert_eq!(bus.sound().fifo_len(1), 0);
        bus.write32(FIFO_A, 0x0403_0201);
        assert_eq!(bus.sound().fifo_len(0), 0);

        bus.write16(SOUNDCNT_X, 0x0080);
        bus.write32(FIFO_A, 0x0403_0201);
        assert_eq!(bus.sound().fifo_len(0), 4);
    }

    #[test]
    fn the_cpu_sees_the_wave_bank_that_is_not_playing() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0002);
        bus.write16(SOUND3CNT_L, 0x0000);
        fill_wave_bank(
            &mut bus,
            [
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
        );
        assert_eq!(bus.read16(WAVE_RAM), 0xFFFF);

        bus.write16(SOUND3CNT_L, 0x00C0);
        assert_eq!(bus.read16(WAVE_RAM), 0x0000);
        bus.write16(SOUND3CNT_H, 0x2000);
        bus.write16(SOUND3CNT_X, 0x8000 | 1984);

        let levels = lefts(&record(&mut bus, QUARTER_SECOND));

        assert!(
            (255..=257).contains(&rising_edges(&levels)),
            "{}",
            rising_edges(&levels)
        );
        assert_eq!(levels.iter().max(), Some(&(SILENCE + 120)));
        assert_eq!(levels.iter().min(), Some(&(SILENCE - 120)));
    }

    #[test]
    fn wave_64_digit_mode_plays_both_banks_in_turn() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0002);
        bus.write16(SOUND3CNT_L, 0x0040);
        fill_wave_bank(&mut bus, [0xFF; 16]);
        bus.write16(SOUND3CNT_H, 0x2000);

        bus.write16(SOUND3CNT_L, 0x0080);
        bus.write16(SOUND3CNT_X, 0x8000 | 1984);
        let one_bank = lefts(&record(&mut bus, QUARTER_SECOND));

        bus.write16(SOUND3CNT_L, 0x00A0);
        bus.write16(SOUND3CNT_X, 0x8000 | 1984);
        let two_banks = lefts(&record(&mut bus, QUARTER_SECOND));

        assert_eq!(changes(&one_bank[16..]), [SILENCE + 120]);
        assert!(
            (127..=129).contains(&rising_edges(&two_banks)),
            "{}",
            rising_edges(&two_banks)
        );
    }

    #[test]
    fn wave_volume_follows_its_level_bits_and_the_forced_75_percent() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0002);
        bus.write16(SOUND3CNT_L, 0x0040);
        fill_wave_bank(&mut bus, [0xFF; 16]);
        bus.write16(SOUND3CNT_L, 0x0080);
        bus.write16(SOUND3CNT_X, 0x8000 | 1984);

        let mut level = |volume: u16| {
            bus.write16(SOUND3CNT_H, volume);
            run(&mut bus, 4096);
            bus.sound().dac().left
        };

        assert_eq!(level(0x2000), SILENCE + 120);
        assert_eq!(level(0x4000), SILENCE + 56);
        assert_eq!(level(0x6000), SILENCE + 24);
        assert_eq!(level(0x8000), SILENCE + 88);
        assert_eq!(level(0x0000), SILENCE);
    }

    #[test]
    fn soundcnt_h_scales_the_psg_to_a_quarter_half_or_full() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0000);
        bus.write16(SOUND2CNT_L, 0xF080);
        bus.write16(SOUND2CNT_H, 0x8000 | 1920);

        let mut peak = |control: u16| {
            bus.write16(SOUNDCNT_H, control);
            let levels = lefts(&record(&mut bus, CLOCK_RATE / 64));
            *levels.iter().max().unwrap() - SILENCE
        };

        assert_eq!(peak(0x0000), 30);
        assert_eq!(peak(0x0001), 60);
        assert_eq!(peak(0x0002), 120);
    }

    #[test]
    fn soundcnt_l_master_volume_and_routing_set_each_side() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0002);
        bus.write16(SOUNDCNT_L, 0x2073);
        bus.write16(SOUND2CNT_L, 0xF080);
        bus.write16(SOUND2CNT_H, 0x8000 | 1920);

        let samples = record(&mut bus, CLOCK_RATE / 64);

        assert_eq!(
            samples.iter().map(|dac| dac.left).max(),
            Some(SILENCE + 120)
        );
        assert_eq!(samples.iter().map(|dac| dac.right).max(), Some(SILENCE));
        bus.write16(SOUNDCNT_L, 0x2273);
        let samples = record(&mut bus, CLOCK_RATE / 64);
        assert_eq!(
            samples.iter().map(|dac| dac.right).max(),
            Some(SILENCE + 60)
        );
    }

    #[test]
    fn a_fifo_plays_one_sample_per_overflow_of_its_timer() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0304);
        push(&mut bus, FIFO_A, [10, -20, 30, 40]);
        assert_eq!(bus.sound().fifo_len(0), 4);

        start_timer(&mut bus, 0, 512);
        let levels = lefts(&record(&mut bus, 512 * 8));

        assert_eq!(
            changes(&levels),
            [
                SILENCE,
                SILENCE + 40,
                SILENCE - 80,
                SILENCE + 120,
                SILENCE + 160
            ]
        );
        assert_eq!(bus.sound().fifo_len(0), 0);
    }

    #[test]
    fn each_fifo_has_its_own_timer_volume_and_sides() {
        let mut bus = bus();
        sound_on(&mut bus, 0x6108);
        push(&mut bus, FIFO_A, [100; 4]);
        push(&mut bus, FIFO_B, [-50; 4]);

        start_timer(&mut bus, 1, 256);
        run(&mut bus, 1024);
        assert_eq!(
            bus.sound().dac(),
            Dac {
                left: SILENCE - 200,
                right: SILENCE
            }
        );
        assert_eq!(bus.sound().fifo_len(0), 4);

        start_timer(&mut bus, 0, 256);
        run(&mut bus, 1024);
        assert_eq!(
            bus.sound().dac(),
            Dac {
                left: SILENCE - 200,
                right: SILENCE + 200
            }
        );
    }

    #[test]
    fn a_fifo_asks_its_dma_for_more_once_three_words_wait_behind_the_playing_one() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0304);
        for word in 0..8 {
            bus.write32(EWRAM + 4 * word, 0x7F7F_7F7F);
        }
        bus.write32(DMA1SAD, EWRAM);
        bus.write32(DMA1DAD, FIFO_A);
        bus.write16(DMA1CNT_H, FIFO_DMA);
        for _ in 0..5 {
            push(&mut bus, FIFO_A, [1; 4]);
        }
        start_timer(&mut bus, 0, 256);

        bus.step(256 + START_DELAY);
        assert_eq!(bus.sound().fifo_len(0), 19);
        assert!(!bus.dma_active());

        bus.step(4 * 256);
        assert_eq!(bus.sound().fifo_len(0), 15);
        assert!(!bus.dma_active());

        bus.step(256);
        assert_eq!(bus.sound().fifo_len(0), 14);
        assert!(bus.dma_active());
        bus.run_dma();
        assert_eq!(bus.sound().fifo_len(0), 30);
    }

    #[test]
    fn an_empty_fifo_asks_on_the_first_overflow_and_again_on_the_third() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0304);
        bus.write32(DMA1SAD, EWRAM);
        bus.write32(DMA1DAD, FIFO_A);
        bus.write16(DMA1CNT_H, FIFO_DMA);
        start_timer(&mut bus, 0, 256);

        bus.step(256 + START_DELAY);
        assert!(bus.dma_active());
        bus.run_dma();
        assert_eq!(bus.sound().fifo_len(0), 16);

        bus.step(256);
        assert_eq!(bus.sound().fifo_len(0), 15);
        assert!(!bus.dma_active());

        bus.step(256);
        assert_eq!(bus.sound().fifo_len(0), 14);
        assert!(bus.dma_active());
    }

    #[test]
    fn a_fifo_request_starts_only_the_dma_aimed_at_that_fifo() {
        let mut bus = bus();
        sound_on(&mut bus, 0x4304);
        bus.write32(DMA1SAD, EWRAM);
        bus.write32(DMA1DAD, FIFO_B);
        bus.write16(DMA1CNT_H, FIFO_DMA);
        push(&mut bus, FIFO_A, [1; 4]);
        start_timer(&mut bus, 0, 256);

        bus.step(256 + START_DELAY);

        assert_eq!(bus.sound().fifo_len(0), 3);
        assert!(!bus.dma_active());
    }

    #[test]
    fn the_reset_bit_empties_a_fifo_and_reads_back_as_zero() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0304);
        push(&mut bus, FIFO_A, [1; 4]);
        push(&mut bus, FIFO_B, [1; 4]);

        bus.write16(SOUNDCNT_H, 0x0B04);

        assert_eq!(bus.sound().fifo_len(0), 0);
        assert_eq!(bus.sound().fifo_len(1), 4);
        assert_eq!(bus.read16(SOUNDCNT_H), 0x0304);
    }

    #[test]
    fn soundbias_sets_the_silent_level_and_the_output_clips_to_10_bits() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0304);
        start_timer(&mut bus, 0, 256);

        bus.write16(SOUNDBIAS, 0x0100);
        run(&mut bus, 1024);
        assert_eq!(bus.sound().dac().left, 0x100);

        push(&mut bus, FIFO_A, [-128; 4]);
        run(&mut bus, 1024);
        assert_eq!(bus.sound().dac().left, 0);

        bus.write16(SOUNDBIAS, 0x03FE);
        push(&mut bus, FIFO_A, [127; 4]);
        run(&mut bus, 1024);
        assert_eq!(bus.sound().dac().left, 0x3FE);
    }

    #[test]
    fn soundbias_resolution_sets_the_sampling_cycle_and_drops_low_bits() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0304);
        let cycles: Vec<u32> = (0..4)
            .map(|resolution| {
                bus.write16(SOUNDBIAS, resolution << 14 | SILENCE);
                bus.sound().sample_cycles()
            })
            .collect();
        assert_eq!(cycles, [512, 256, 128, 64]);

        push(&mut bus, FIFO_A, [7; 4]);
        start_timer(&mut bus, 0, 64);
        run(&mut bus, 256);
        assert_eq!(bus.sound().dac().left, SILENCE + 16);

        bus.write16(SOUNDBIAS, 0x4000 | SILENCE);
        run(&mut bus, 512);
        assert_eq!(bus.sound().dac().left, SILENCE + 28);
    }

    #[test]
    fn take_audio_gives_stereo_floats_at_the_chosen_rate() {
        let mut bus = bus();
        bus.sound_mut().set_sample_rate(48_000);
        sound_on(&mut bus, 0x0002);
        bus.write16(SOUND1CNT_H, 0xF080);
        bus.write16(SOUND1CNT_X, 0x8000 | 1920);

        let mut audio = Vec::new();
        for _ in 0..4 {
            run(&mut bus, QUARTER_SECOND);
            bus.sound_mut().take_audio(&mut audio);
        }

        assert!((95_800..=96_200).contains(&audio.len()), "{}", audio.len());
        let peak = audio
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        assert!((0.2..=0.5).contains(&peak), "{peak}");
    }

    #[test]
    fn undrained_audio_stays_bounded() {
        let mut bus = bus();
        bus.sound_mut().set_sample_rate(8_000);
        sound_on(&mut bus, 0x0002);
        bus.write16(SOUND1CNT_H, 0xF080);
        bus.write16(SOUND1CNT_X, 0x8000 | 1920);

        run(&mut bus, 3 * CLOCK_RATE);

        let mut audio = Vec::new();
        bus.sound_mut().take_audio(&mut audio);
        assert!(audio.len() <= 16_000, "{}", audio.len());
    }

    #[test]
    fn a_muted_channel_leaves_the_mix() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0306);
        bus.write16(SOUND1CNT_H, 0xF080);
        bus.write16(SOUND1CNT_X, 0x8000 | 1920);
        push(&mut bus, FIFO_A, [50; 4]);
        start_timer(&mut bus, 0, 256);

        bus.sound_mut().set_channel_enabled(0, false);
        let levels = lefts(&record(&mut bus, CLOCK_RATE / 64));
        assert_eq!(changes(&levels[4..]), [SILENCE + 200]);

        bus.sound_mut().set_channel_enabled(4, false);
        run(&mut bus, 1024);
        assert_eq!(bus.sound().dac().left, SILENCE);
        assert_eq!(CHANNEL_NAMES.len(), 6);
    }

    fn play_alone(channel: usize) -> Bus {
        let mut bus = bus();
        match channel {
            0 => {
                sound_on(&mut bus, 0x0002);
                bus.write16(SOUND1CNT_H, 0xF080);
                bus.write16(SOUND1CNT_X, 0x8000 | 1920);
            }
            1 => {
                sound_on(&mut bus, 0x0002);
                bus.write16(SOUND2CNT_L, 0xF080);
                bus.write16(SOUND2CNT_H, 0x8000 | 1920);
            }
            2 => {
                sound_on(&mut bus, 0x0002);
                bus.write16(SOUND3CNT_L, 0x0040);
                fill_wave_bank(&mut bus, [0xF0; 16]);
                bus.write16(SOUND3CNT_H, 0x2000);
                bus.write16(SOUND3CNT_L, 0x0080);
                bus.write16(SOUND3CNT_X, 0x8000 | 1984);
            }
            3 => {
                sound_on(&mut bus, 0x0002);
                bus.write16(SOUND4CNT_L, 0xF000);
                bus.write16(SOUND4CNT_H, 0x8000);
            }
            4 => {
                sound_on(&mut bus, 0x0304);
                push(&mut bus, FIFO_A, [50; 4]);
                start_timer(&mut bus, 0, 256);
            }
            _ => {
                sound_on(&mut bus, 0x3008);
                push(&mut bus, FIFO_B, [50; 4]);
                start_timer(&mut bus, 0, 256);
            }
        }
        bus
    }

    #[test]
    fn each_mute_silences_its_own_channel() {
        for (channel, name) in CHANNEL_NAMES.iter().enumerate() {
            let mut playing = play_alone(channel);
            let heard = lefts(&record(&mut playing, CLOCK_RATE / 64));
            assert!(heard.iter().any(|&level| level != SILENCE), "{name} plays");

            let mut muted = play_alone(channel);
            muted.sound_mut().set_channel_enabled(channel, false);
            let heard = lefts(&record(&mut muted, CLOCK_RATE / 64));
            assert!(
                heard.iter().all(|&level| level == SILENCE),
                "{name} is silent"
            );
        }
    }

    #[test]
    #[ignore = "writes a listening file to CITRINE_GBA_WAV"]
    fn render_listening_wav() {
        let path = std::env::var("CITRINE_GBA_WAV").expect("CITRINE_GBA_WAV");
        let rate = 48_000;
        let mut bus = bus();
        bus.sound_mut().set_sample_rate(rate);
        sound_on(&mut bus, 0x0306);
        bus.write16(SOUND2CNT_L, 0x0080);

        let fifo_period = 512;
        let half_wave = 37;
        for index in 0..CLOCK_RATE / fifo_period * 2 {
            let level = if (index / half_wave) % 2 == 0 {
                48
            } else {
                -48
            };
            bus.write8(EWRAM + index, level as u8);
        }
        bus.write32(DMA1SAD, EWRAM);
        bus.write32(DMA1DAD, FIFO_A);
        bus.write16(DMA1CNT_H, FIFO_DMA);

        let notes = [1546, 1650, 1714, 1797];
        let note_cycles = CLOCK_RATE / 4;
        let mut audio = Vec::new();
        for second in 0..3 {
            if second == 1 {
                start_timer(&mut bus, 0, fifo_period as u16);
            }
            for period in notes {
                if second != 1 {
                    bus.write16(SOUND1CNT_H, 0xF3C0);
                    bus.write16(SOUND1CNT_X, 0x8000 | period);
                }
                run(&mut bus, note_cycles);
                bus.sound_mut().take_audio(&mut audio);
            }
        }

        std::fs::write(path, wav_bytes(&audio, rate)).expect("write wav");
    }

    fn wav_bytes(audio: &[f32], rate: u32) -> Vec<u8> {
        let data_len = (audio.len() * 2) as u32;
        let mut bytes = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in audio {
            let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[cfg(feature = "debug")]
    #[test]
    fn a_silent_direct_sound_channel_plots_on_the_midline_and_an_unrouted_one_as_off() {
        let mut bus = bus();
        sound_on(&mut bus, 0x0304);
        bus.sound_mut().record_taps(true);
        run(&mut bus, 4096);

        let mut debugger = crate::debug::Debugger::new();
        for levels in bus.sound_mut().take_taps() {
            debugger.push_waves(&levels);
        }
        let waves = debugger.waves();
        assert!(!waves[4].is_empty());
        assert!(waves[4].iter().all(|&level| (level - 0.5).abs() < 1.0e-6));
        assert!(waves[5].iter().all(|&level| level == 0.0));
    }

    fn fifo_audio_in_steps_of(step: u32) -> Vec<f32> {
        let mut bus = bus();
        bus.sound_mut().set_sample_rate(48_000);
        sound_on(&mut bus, 0x0304);
        for word in 0..7_i8 {
            push(
                &mut bus,
                FIFO_A,
                [word * 9, -word * 7, 60 - word * 5, word * 3 - 40],
            );
        }
        start_timer(&mut bus, 0, 1254);
        let mut left = 1254 * 30;
        while left > 0 {
            let chunk = left.min(step);
            bus.step(chunk);
            left -= chunk;
        }
        let mut audio = Vec::new();
        bus.sound_mut().take_audio(&mut audio);
        audio
    }

    #[test]
    fn a_long_cpu_step_plays_every_fifo_sample_at_its_overflow() {
        let fine = fifo_audio_in_steps_of(16);
        assert_eq!(fifo_audio_in_steps_of(64), fine);
        assert_eq!(fifo_audio_in_steps_of(3_000), fine);
        assert_eq!(fifo_audio_in_steps_of(7_000), fine);
    }

    fn start_every_psg_channel(bus: &mut Bus) {
        bus.sound_mut().set_sample_rate(48_000);
        sound_on(bus, 0x0002);
        bus.write16(SOUND1CNT_L, 0x0023);
        bus.write16(SOUND1CNT_H, 0xF2BE);
        bus.write16(SOUND1CNT_X, 0xC000 | 1700);
        bus.write16(SOUND2CNT_L, 0x797C);
        bus.write16(SOUND2CNT_H, 0xC000 | 1800);
        bus.write16(SOUND3CNT_L, 0x0040);
        fill_wave_bank(
            bus,
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54,
                0x32, 0x10,
            ],
        );
        bus.write16(SOUND3CNT_H, 0x20F0);
        bus.write16(SOUND3CNT_L, 0x00A0);
        bus.write16(SOUND3CNT_X, 0xC000 | 1900);
        bus.write16(SOUND4CNT_L, 0xA23C);
        bus.write16(SOUND4CNT_H, 0xC000 | 0x0039);
    }

    #[test]
    fn the_psg_runs_once_a_sample_but_reads_and_saves_as_if_it_ran_every_step() {
        let mut every_step = bus();
        let mut per_sample = bus();
        start_every_psg_channel(&mut every_step);
        start_every_psg_channel(&mut per_sample);
        let mut flags = Vec::new();
        for step in 0..4_000_u32 {
            if step == 2_000 {
                for bus in [&mut every_step, &mut per_sample] {
                    bus.write16(SOUND1CNT_X, 0xC000 | 1500);
                    bus.write16(SOUND4CNT_H, 0xC000 | 0x0021);
                }
            }
            let cycles = 1 + step * 7 % 997;
            every_step.step(cycles);
            every_step.sound_mut().run_psg();
            per_sample.step(cycles);
            if step % 50 == 0 {
                let flag = every_step.read16(SOUNDCNT_X);
                assert_eq!(per_sample.read16(SOUNDCNT_X), flag, "step {step}");
                flags.push(flag);
                assert_eq!(
                    per_sample.read16(WAVE_RAM + 4),
                    every_step.read16(WAVE_RAM + 4),
                    "step {step}"
                );
                #[cfg(feature = "persistence")]
                assert_eq!(
                    rmp_serde::to_vec(per_sample.sound()).unwrap(),
                    rmp_serde::to_vec(every_step.sound()).unwrap(),
                    "step {step}"
                );
            }
        }
        flags.dedup();
        assert!(flags.len() > 2, "{flags:?}");
        let mut fine = Vec::new();
        let mut batched = Vec::new();
        every_step.sound_mut().take_audio(&mut fine);
        per_sample.sound_mut().take_audio(&mut batched);
        assert!(!fine.is_empty());
        assert_eq!(batched, fine);
    }
}
