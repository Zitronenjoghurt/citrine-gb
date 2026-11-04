use crate::gb::apu::WriteContext;
use crate::gb::apu::components::length_counter::LengthCounter;
use crate::gb::apu::registers::ch123_control::Channel123Control;

const TRIGGER_DELAY: u16 = 6;
const BANK_BYTES: usize = 16;
const BANK_DIGITS: u8 = 32;
const LENGTH_MAX: u16 = 256;
const VOLUME_QUARTERS: [i32; 4] = [0, 4, 2, 1];
const FORCED_QUARTERS: i32 = 3;

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Wave {
    pub enabled: bool,
    pub dac_enabled: bool,
    pub two_banks: bool,
    pub bank: u8,
    pub wave_step: u8,
    pub sample_buffer: u8,
    pub frequency_timer: u16,
    pub length_counter: LengthCounter,
    pub output_level: u8,
    pub forced_level: bool,
    pub period_low: u8,
    pub control: Channel123Control,
    pub ram: [[u8; BANK_BYTES]; 2],
}

impl Wave {
    pub fn digit(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        if self.wave_step.is_multiple_of(2) {
            self.sample_buffer >> 4
        } else {
            self.sample_buffer & 0x0F
        }
    }

    pub fn level(&self) -> i32 {
        if !self.enabled || !self.dac_enabled {
            return 0;
        }
        let quarters = if self.forced_level {
            FORCED_QUARTERS
        } else {
            VOLUME_QUARTERS[usize::from(self.output_level & 0b11)]
        };
        ((2 * i32::from(self.digit()) - 15) * quarters) >> 2
    }

    pub fn quiet(&self, ticks: u16) -> bool {
        !self.enabled || self.frequency_timer > ticks
    }

    pub fn skip(&mut self, ticks: u16) {
        if self.enabled {
            self.frequency_timer -= ticks;
        }
    }

    pub fn tick(&mut self) -> bool {
        if !self.enabled {
            return false;
        }
        self.frequency_timer = self.frequency_timer.saturating_sub(1);
        if self.frequency_timer != 0 {
            return false;
        }
        self.reset_frequency_timer();
        let digits = if self.two_banks {
            2 * BANK_DIGITS
        } else {
            BANK_DIGITS
        };
        self.wave_step = (self.wave_step + 1) % digits;
        self.sample_buffer = self.playing_byte();
        true
    }

    fn playing_byte(&self) -> u8 {
        let bank = (self.bank ^ (self.wave_step / BANK_DIGITS)) & 1;
        let index = usize::from(self.wave_step % BANK_DIGITS) / 2;
        self.ram[usize::from(bank)][index]
    }

    pub fn trigger(&mut self) {
        if self.dac_enabled {
            self.enabled = true;
        }
        self.wave_step = 0;
        self.frequency_timer = (2048 - self.period()) * 2 + TRIGGER_DELAY;
    }

    pub fn clock_length(&mut self) {
        if self.length_counter.clock() {
            self.enabled = false;
        }
    }

    pub fn period(&self) -> u16 {
        u16::from(self.period_low) | (u16::from(self.control.period_high & 0b111) << 8)
    }

    fn reset_frequency_timer(&mut self) {
        self.frequency_timer = (2048 - self.period()) * 2;
    }

    pub fn read_ram(&self, index: usize) -> u8 {
        self.ram[usize::from(self.bank ^ 1)][index]
    }

    pub fn write_ram(&mut self, index: usize, value: u8) {
        self.ram[usize::from(self.bank ^ 1)][index] = value;
    }

    pub fn write_select(&mut self, value: u8) {
        self.two_banks = value & 0x20 != 0;
        self.bank = (value >> 6) & 1;
        self.dac_enabled = value & 0x80 != 0;
        if !self.dac_enabled {
            self.enabled = false;
        }
    }

    pub fn write_length(&mut self, value: u8) {
        self.length_counter.load(LENGTH_MAX, u16::from(value));
    }

    pub fn write_volume(&mut self, value: u8) {
        self.output_level = (value >> 5) & 0b11;
        self.forced_level = value & 0x80 != 0;
    }

    pub fn write_period_low(&mut self, value: u8) {
        self.period_low = value;
    }

    pub fn write_control(&mut self, value: u8, ctx: WriteContext) {
        self.control = value.into();
        let expired = self.length_counter.write_control(
            self.control.length_enable,
            self.control.trigger,
            LENGTH_MAX,
            ctx.frame_clocked_length,
        );
        if expired {
            self.enabled = false;
        }
        if self.control.trigger {
            self.trigger();
        }
    }
}
