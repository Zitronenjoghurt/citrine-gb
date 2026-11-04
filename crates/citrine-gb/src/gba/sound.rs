mod fifo;
mod output;
mod psg;
mod wave;

use super::timer::{Overflows, Timers};
use fifo::Fifo;
use output::Output;
use psg::{Levels, Psg};

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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
                    value | u16::from(self.psg.enabled_flags())
                } else {
                    value
                }
            }
            WAVE_RAM_FIRST..=WAVE_RAM_LAST => {
                let index = offset - WAVE_RAM_FIRST;
                u16::from_le_bytes([
                    self.psg.ch3.read_ram(index),
                    self.psg.ch3.read_ram(index + 1),
                ])
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: usize, value: u16, mask: u16) {
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

    fn step(&mut self, cycles: u32) {
        let mut cycles = cycles;
        while cycles > 0 {
            let period = self.sample_cycles();
            let chunk = cycles.min(period.saturating_sub(self.sample_cycles).max(1));
            if self.master_enabled() {
                self.psg_cycles += chunk;
                self.psg.run(self.psg_cycles / PSG_CYCLES);
                self.psg_cycles %= PSG_CYCLES;
            }
            self.output.advance(chunk);
            self.sample_cycles += chunk;
            cycles -= chunk;
            if self.sample_cycles >= period {
                self.sample_cycles = 0;
                self.sample();
            }
        }
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

    fn step_to(&mut self, at: u32) {
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
        let psg = self.psg.levels(&self.muted[..4]);
        let shift = PSG_SHIFTS[usize::from(control & 0b11)];
        let mut levels = Levels {
            left: psg.left >> shift,
            right: psg.right >> shift,
        };
        for (index, fifo) in self.fifos.iter().enumerate() {
            if self.muted[4 + index] {
                continue;
            }
            let full_volume = (control >> (2 + index)) & 1 != 0;
            let level = i32::from(fifo.sample()) * if full_volume { 4 } else { 2 };
            let routing = control >> (8 + 4 * index);
            if routing & 0b01 != 0 {
                levels.right += level;
            }
            if routing & 0b10 != 0 {
                levels.left += level;
            }
        }
        levels
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
mod tests;
