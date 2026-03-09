use crate::gb::apu::components::frequency_sweep::FrequencySweep;
use crate::gb::apu::components::length_counter::LengthCounter;
use crate::gb::apu::components::square_wave::SquareWave;
use crate::gb::apu::components::volume_envelope::VolumeEnvelope;
use crate::gb::apu::registers::ch123_control::Channel123Control;
use crate::gb::apu::registers::ch124_volume::Channel124Volume;
use crate::gb::apu::registers::ch12_timer::Channel12Timer;
use crate::gb::apu::registers::ch1_sweep::Channel1Sweep;
use crate::{ReadMemory, WriteMemory};

#[derive(Debug, Default)]
pub struct Channel1 {
    pub enabled: bool,
    pub length_counter: LengthCounter,
    pub volume_envelope: VolumeEnvelope,
    pub square_wave: SquareWave,
    pub frequency_sweep: FrequencySweep,
    /// NR10 (0xFF10)
    pub sweep: Channel1Sweep,
    /// NR11 (0xFF11)
    pub timer: Channel12Timer,
    /// NR12 (0xFF12)
    pub volume: Channel124Volume,
    /// NR13 (0xFF13) => Write-only
    pub period_low: u8,
    /// NR14 (0xFF14)
    pub control: Channel123Control,
}

impl Channel1 {
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

        let disable = self.frequency_sweep.trigger(
            self.get_period(),
            self.sweep.pace,
            self.sweep.individual_step,
            self.sweep.direction,
        );

        if disable {
            self.enabled = false;
        }
    }

    pub fn clock_length(&mut self) {
        if self.length_counter.clock() {
            self.enabled = false;
        }
    }

    pub fn clock_volume_envelope(&mut self) {
        self.volume_envelope.clock();
    }

    pub fn clock_sweep(&mut self) {
        let (disable, new_period) = self.frequency_sweep.clock(
            self.sweep.pace,
            self.sweep.individual_step,
            self.sweep.direction,
        );

        if disable {
            self.enabled = false;
        }

        if let Some(period) = new_period {
            self.set_period(period);
            self.square_wave.set_frequency(period);
        }
    }

    pub fn get_period(&self) -> u16 {
        self.period_low as u16 | (((self.control.period_high & 0b111) as u16) << 8)
    }

    pub fn set_period(&mut self, period: u16) {
        self.period_low = (period & 0xFF) as u8;
        self.control.period_high = ((period >> 8) & 0b111) as u8;
    }

    pub fn dac_enabled(&self) -> bool {
        self.volume.initial_volume > 0 || self.volume.envelope_direction
    }
}

impl ReadMemory for Channel1 {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF10 => self.sweep.into(),
            0xFF11 => self.timer.into(),
            0xFF12 => self.volume.into(),
            0xFF13 => 0xFF,
            0xFF14 => self.control.into(),
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Channel1 {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF10 => self.sweep = value.into(),
            0xFF11 => {
                self.timer = value.into();
                self.length_counter
                    .reload(64u16.saturating_sub(self.timer.initial_length_timer as u16));
                self.square_wave.set_duty(self.timer.wave_duty);
            }
            0xFF12 => {
                self.volume = value.into();
                //self.volume_envelope.trigger(
                //    self.volume.envelope_direction,
                //    self.volume.initial_volume,
                //    self.volume.envelope_pace,
                //)
            }
            0xFF13 => self.period_low = value,
            0xFF14 => {
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
