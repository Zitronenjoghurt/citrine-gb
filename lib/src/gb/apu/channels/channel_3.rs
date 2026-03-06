use crate::gb::apu::components::length_counter::LengthCounter;
use crate::gb::apu::registers::ch123_control::Channel123Control;
use crate::{ReadMemory, WriteMemory};

#[derive(Debug, Default)]
pub struct Channel3 {
    pub enabled: bool,
    pub wave_step: u8,
    pub frequency_timer: u16,
    pub length_counter: LengthCounter,
    /// NR30 (0xFF1A) => Most significant bit
    pub dac_enabled: bool,
    /// NR31 (0xFF1B) => Write only
    pub initial_length_timer: u8,
    /// NR32 (0xFF1C) => Bits 5-6
    pub output_level: u8,
    /// NR33 (0xFF1D) => Write only
    pub period_low: u8,
    /// NR34 (0xFF1E)
    pub control: Channel123Control,
    /// (FF30-FF3F)
    pub wave_ram: [u8; 16],
}

impl Channel3 {
    pub fn sample(&self) -> u8 {
        if !self.enabled {
            return 0;
        }

        let base = self.get_wave_value();
        match self.output_level & 0b11 {
            0 => 0,
            1 => base,
            2 => (base >> 1) & 0b1111,
            3 => (base >> 2) & 0b1111,
            _ => unreachable!(),
        }
    }

    pub fn tick(&mut self) {
        if !self.enabled {
            return;
        }

        if self.frequency_timer > 0 {
            self.frequency_timer -= 1;
        }

        if self.frequency_timer == 0 {
            self.reset_frequency_timer();
            self.wave_step = (self.wave_step + 1) & 0b11111;
        }
    }

    pub fn trigger(&mut self) {
        if self.dac_enabled {
            self.enabled = true;
        }

        self.wave_step = 0;
        self.reset_frequency_timer();
        self.length_counter.trigger(256);
    }

    pub fn clock_length(&mut self) {
        if self.length_counter.clock() {
            self.enabled = false;
        }
    }

    pub fn get_period(&self) -> u16 {
        (self.period_low as u16) | (((self.control.period_high & 0b111) as u16) << 8)
    }

    pub fn get_wave_value(&self) -> u8 {
        let byte = self.wave_ram[self.wave_step as usize / 2];
        if self.wave_step.is_multiple_of(2) {
            (byte >> 4) & 0b1111
        } else {
            byte & 0b1111
        }
    }

    pub fn reset_frequency_timer(&mut self) {
        self.frequency_timer = (2048 - self.get_period()) * 2;
    }

    pub fn dac_enabled(&self) -> bool {
        self.dac_enabled
    }
}

impl ReadMemory for Channel3 {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF1A => 0x7F | ((self.dac_enabled as u8) << 7),
            0xFF1C => 0x9F | ((self.output_level & 0b11) << 5),
            0xFF1E => self.control.into(),
            // ToDo: Wave RAM read quirks => https://gbdev.io/pandocs/Audio_Registers.html#ff30ff3f--wave-pattern-ram
            0xFF30..=0xFF3F => self.wave_ram[addr as usize - 0xFF30],
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Channel3 {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF1A => {
                self.dac_enabled = (value & 0b1000_0000) != 0;
                if !self.dac_enabled {
                    self.enabled = false;
                }
            }
            0xFF1B => {
                self.initial_length_timer = value;
                self.length_counter
                    .reload(256u16.saturating_sub(self.initial_length_timer as u16));
            }
            0xFF1C => self.output_level = (value >> 5) & 0b11,
            0xFF1D => self.period_low = value,
            0xFF1E => {
                self.control = value.into();
                self.length_counter.enabled = self.control.length_enable;
                if self.control.trigger {
                    self.trigger();
                }
            }
            // ToDo: Wave RAM write quirks => https://gbdev.io/pandocs/Audio_Registers.html#ff30ff3f--wave-pattern-ram
            0xFF30..=0xFF3F => self.wave_ram[addr as usize - 0xFF30] = value,
            _ => {}
        }
    }
}
