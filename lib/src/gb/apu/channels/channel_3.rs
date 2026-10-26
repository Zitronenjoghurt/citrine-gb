use crate::ReadMemory;
use crate::gb::apu::WriteContext;
use crate::gb::apu::components::length_counter::LengthCounter;
use crate::gb::apu::registers::ch123_control::Channel123Control;

const TRIGGER_DELAY: u16 = 6;

#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Channel3 {
    pub enabled: bool,
    pub wave_step: u8,
    pub sample_buffer: u8,
    pub just_read: bool,
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

    pub fn quiet(&self, ticks: u16) -> bool {
        !self.enabled || self.frequency_timer > ticks
    }

    pub fn skip(&mut self, ticks: u16) {
        self.just_read = false;
        if self.enabled {
            self.frequency_timer -= ticks;
        }
    }

    pub fn tick(&mut self) -> bool {
        self.just_read = false;
        if !self.enabled {
            return false;
        }

        self.frequency_timer = self.frequency_timer.saturating_sub(1);
        if self.frequency_timer == 0 {
            self.reset_frequency_timer();
            self.wave_step = (self.wave_step + 1) & 0b11111;
            self.sample_buffer = self.wave_ram[self.wave_step as usize / 2];
            self.just_read = true;
            return true;
        }
        false
    }

    fn about_to_read(&self) -> bool {
        self.enabled && self.frequency_timer == 2
    }

    pub fn trigger(&mut self, cgb: bool) {
        if self.about_to_read() {
            if !cgb {
                let offset = ((self.wave_step as usize + 1) >> 1) & 0xF;
                if offset < 4 {
                    self.wave_ram[0] = self.wave_ram[offset];
                } else {
                    let start = offset & !3;
                    self.wave_ram.copy_within(start..start + 4, 0);
                }
            }
            self.sample_buffer = self.wave_ram[0];
        }

        if self.dac_enabled {
            self.enabled = true;
        }

        self.wave_step = 0;
        self.frequency_timer = (2048 - self.get_period()) * 2 + TRIGGER_DELAY;
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
        if self.wave_step.is_multiple_of(2) {
            (self.sample_buffer >> 4) & 0b1111
        } else {
            self.sample_buffer & 0b1111
        }
    }

    pub fn reset_frequency_timer(&mut self) {
        self.frequency_timer = (2048 - self.get_period()) * 2;
    }

    pub fn dac_enabled(&self) -> bool {
        self.dac_enabled
    }

    fn wave_index(&self, addr: u16, cgb: bool) -> Option<usize> {
        if !self.enabled {
            Some(addr as usize - 0xFF30)
        } else if cgb || self.just_read {
            Some(self.wave_step as usize / 2)
        } else {
            None
        }
    }

    pub fn read_wave(&self, addr: u16, cgb: bool) -> u8 {
        match self.wave_index(addr, cgb) {
            Some(index) => self.wave_ram[index],
            None => 0xFF,
        }
    }

    pub fn write_wave(&mut self, addr: u16, value: u8, cgb: bool) {
        if let Some(index) = self.wave_index(addr, cgb) {
            self.wave_ram[index] = value;
        }
    }

    pub fn write_length(&mut self, value: u8) {
        self.initial_length_timer = value;
        self.length_counter.load(256, value as u16);
    }

    pub fn write(&mut self, addr: u16, value: u8, ctx: WriteContext) {
        match addr {
            0xFF1A => {
                self.dac_enabled = (value & 0b1000_0000) != 0;
                if !self.dac_enabled {
                    self.enabled = false;
                }
            }
            0xFF1B => self.write_length(value),
            0xFF1C => self.output_level = (value >> 5) & 0b11,
            0xFF1D => self.period_low = value,
            0xFF1E => {
                self.control = value.into();
                let expired = self.length_counter.write_control(
                    self.control.length_enable,
                    self.control.trigger,
                    256,
                    ctx.frame_clocked_length,
                );
                if expired {
                    self.enabled = false;
                }
                if self.control.trigger {
                    self.trigger(ctx.cgb);
                }
            }
            _ => {}
        }
    }
}

impl ReadMemory for Channel3 {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF1A => 0x7F | ((self.dac_enabled as u8) << 7),
            0xFF1C => 0x9F | ((self.output_level & 0b11) << 5),
            0xFF1E => self.control.into(),
            _ => 0xFF,
        }
    }
}
