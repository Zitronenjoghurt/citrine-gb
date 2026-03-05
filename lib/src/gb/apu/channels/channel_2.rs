use crate::gb::apu::components::length_counter::LengthCounter;
use crate::gb::apu::components::square_wave::SquareWave;
use crate::gb::apu::components::volume_envelope::VolumeEnvelope;
use crate::gb::apu::registers::ch12_control::Channel12Control;
use crate::gb::apu::registers::ch12_timer::Channel12Timer;
use crate::gb::apu::registers::ch12_volume::Channel12Volume;
use crate::{ReadMemory, WriteMemory};

#[derive(Debug, Default)]
pub struct Channel2 {
    pub enabled: bool,
    pub length_counter: LengthCounter,
    pub volume_envelope: VolumeEnvelope,
    pub square_wave: SquareWave,
    /// NR21 (0xFF16)
    pub timer: Channel12Timer,
    /// NR22 (0xFF17)
    pub volume: Channel12Volume,
    /// NR23 (0xFF18) => Write-only
    pub period_low: u8,
    /// NR24 (0xFF19)
    pub control: Channel12Control,
}

impl Channel2 {
    /// Returns the current volume of the channel (0-15)
    pub fn sample(&self) -> u8 {
        if !self.enabled {
            return 0;
        };

        self.square_wave.sample() * self.volume_envelope.current_volume
    }

    pub fn tick(&mut self) {
        if self.enabled {
            self.square_wave.tick(self.get_period());
        }
    }

    pub fn trigger(&mut self) {
        self.enabled = true;

        if self.length_counter.counter == 0 {
            self.length_counter.counter = 64;
        }

        self.volume_envelope.trigger(
            self.volume.envelope_direction,
            self.volume.initial_volume,
            self.volume.envelope_pace,
        );

        self.square_wave.set_frequency(self.get_period());
    }

    pub fn clock_length(&mut self) {
        if self.length_counter.clock() {
            self.enabled = false;
        }
    }

    pub fn clock_volume_envelope(&mut self) {
        self.volume_envelope.clock();
    }

    pub fn get_period(&self) -> u16 {
        self.period_low as u16 | (((self.control.period_high & 0b111) as u16) << 8)
    }

    pub fn dac_enabled(&self) -> bool {
        self.volume.initial_volume > 0 || self.volume.envelope_direction
    }
}

impl ReadMemory for Channel2 {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF16 => self.timer.into(),
            0xFF17 => self.volume.into(),
            0xFF18 => 0xFF,
            0xFF19 => self.control.into(),
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Channel2 {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF16 => {
                self.timer = value.into();
                self.length_counter
                    .trigger(64u16.saturating_sub(self.timer.initial_length_timer as u16));
                self.square_wave.set_duty(self.timer.wave_duty);
            }
            0xFF17 => {
                self.volume = value.into();
                self.volume_envelope.trigger(
                    self.volume.envelope_direction,
                    self.volume.initial_volume,
                    self.volume.envelope_pace,
                )
            }
            0xFF18 => self.period_low = value,
            0xFF19 => {
                self.control = value.into();
                self.length_counter.enabled = self.control.length_enable;
                if self.control.trigger {
                    self.trigger();
                }
            }
            _ => {}
        }
    }
}
