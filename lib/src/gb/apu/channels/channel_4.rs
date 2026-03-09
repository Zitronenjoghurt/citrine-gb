use crate::gb::apu::components::length_counter::LengthCounter;
use crate::gb::apu::components::volume_envelope::VolumeEnvelope;
use crate::gb::apu::registers::ch124_volume::Channel124Volume;
use crate::gb::apu::registers::ch4_frequency::Channel4Frequency;
use crate::{ReadMemory, WriteMemory};

#[derive(Debug, Default)]
pub struct Channel4 {
    pub enabled: bool,
    pub lfsr: u16,
    pub frequency_timer: u32,
    pub length_counter: LengthCounter,
    pub volume_envelope: VolumeEnvelope,
    /// NR41 (0xFF20) => Write only (6 lower bits)
    pub initial_length_timer: u8,
    /// NR42 (0xFF21)
    pub volume: Channel124Volume,
    /// NR43 (0xFF22)
    pub frequency: Channel4Frequency,
    /// NR44 (0xFF23) => bit 7
    pub trigger: bool,
}

impl Channel4 {
    pub fn sample(&self) -> u8 {
        if !self.enabled {
            return 0;
        };

        if (self.lfsr & 1) == 0 {
            self.volume_envelope.current_volume
        } else {
            0
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

            if self.frequency.clock_shift < 14 {
                let xor_bit = (self.lfsr & 1) ^ ((self.lfsr >> 1) & 1);

                self.lfsr >>= 1;
                self.lfsr |= xor_bit << 14;

                if self.frequency.lfsr_width {
                    self.lfsr &= !(1 << 6);
                    self.lfsr |= xor_bit << 6;
                }
            }
        }
    }

    pub fn trigger(&mut self) {
        self.enabled = true;

        if self.length_counter.counter == 0 {
            self.length_counter.counter = 64;
        }

        self.reset_frequency_timer();
        self.lfsr = 0x7FFF;

        self.volume_envelope.trigger(
            self.volume.envelope_direction,
            self.volume.initial_volume,
            self.volume.envelope_pace,
        );
    }

    pub fn clock_length(&mut self) {
        if self.length_counter.clock() {
            self.enabled = false;
        }
    }

    pub fn clock_volume_envelope(&mut self) {
        self.volume_envelope.clock();
    }

    pub fn dac_enabled(&self) -> bool {
        self.volume.initial_volume > 0 || self.volume.envelope_direction
    }

    fn reset_frequency_timer(&mut self) {
        let divisor = if self.frequency.clock_divider == 0 {
            8
        } else {
            (self.frequency.clock_divider as u32) * 16
        };
        self.frequency_timer = divisor << self.frequency.clock_shift;
    }
}

impl ReadMemory for Channel4 {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF21 => self.volume.into(),
            0xFF22 => self.frequency.into(),
            0xFF23 => 0xBF | ((self.length_counter.enabled as u8) << 6),
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Channel4 {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF20 => {
                self.initial_length_timer = value & 0x3F;
                self.length_counter
                    .reload(64u16.saturating_sub(self.initial_length_timer as u16))
            }
            0xFF21 => {
                self.volume = value.into();
                //self.volume_envelope.trigger(
                //    self.volume.envelope_direction,
                //    self.volume.initial_volume,
                //    self.volume.envelope_pace,
                //);
            }
            0xFF22 => self.frequency = value.into(),
            0xFF23 => {
                self.length_counter.enabled = (value & 0b0100_0000) != 0;
                self.trigger = (value & 0b1000_0000) != 0;
                if self.trigger {
                    self.trigger();
                }
            }
            _ => {}
        }
    }
}
