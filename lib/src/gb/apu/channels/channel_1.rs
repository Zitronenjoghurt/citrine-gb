use crate::ReadMemory;
use crate::gb::apu::WriteContext;
use crate::gb::apu::components::frequency_sweep::FrequencySweep;
use crate::gb::apu::components::length_counter::LengthCounter;
use crate::gb::apu::components::square_wave::SquareWave;
use crate::gb::apu::components::volume_envelope::VolumeEnvelope;
use crate::gb::apu::registers::ch1_sweep::Channel1Sweep;
use crate::gb::apu::registers::ch12_timer::Channel12Timer;
use crate::gb::apu::registers::ch123_control::Channel123Control;
use crate::gb::apu::registers::ch124_volume::Channel124Volume;

#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

    pub fn quiet(&self, ticks: u16) -> bool {
        !self.enabled || self.square_wave.quiet(ticks)
    }

    pub fn skip(&mut self, ticks: u16) {
        if self.enabled {
            self.square_wave.skip(ticks);
        }
    }

    pub fn tick(&mut self) -> bool {
        if self.enabled {
            self.square_wave.tick(self.get_period())
        } else {
            false
        }
    }

    pub fn trigger(&mut self, ctx: WriteContext) {
        let was_enabled = self.enabled;
        if self.dac_enabled() {
            self.enabled = true;
        }

        self.volume_envelope.trigger(self.volume.into());

        self.square_wave
            .trigger(self.get_period(), was_enabled, ctx.lf_div);
        self.frequency_sweep.trigger(
            self.get_period(),
            self.nr10(),
            was_enabled,
            ctx.lf_div,
            ctx.cgb,
        );
    }

    fn nr10(&self) -> u8 {
        u8::from(self.sweep) & 0x7F
    }

    pub fn clock_length(&mut self) {
        if self.length_counter.clock() {
            self.enabled = false;
        }
    }

    pub fn envelope_div_event(&mut self, div_apu: u8) {
        self.volume_envelope.div_event(div_apu);
    }

    pub fn envelope_tick(&mut self) {
        self.volume_envelope.tick(self.volume.into());
    }

    pub fn envelope_secondary_div_event(&mut self) {
        if self.enabled {
            self.volume_envelope.secondary_div_event(self.volume.into());
        }
    }

    pub fn clock_sweep(&mut self, lf_div: u16) {
        let mut period = self.get_period();
        self.frequency_sweep
            .div_event(&mut period, self.nr10(), lf_div);
        self.set_period(period);
    }

    pub fn sweep_busy(&self) -> bool {
        self.frequency_sweep.busy()
    }

    pub fn sweep_tick(&mut self, lf_div: u16) {
        if self
            .frequency_sweep
            .tick(self.get_period(), self.nr10(), lf_div)
        {
            self.enabled = false;
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

    pub fn write_length(&mut self, value: u8) {
        self.length_counter.load(64, (value & 0x3F) as u16);
    }

    pub fn write(&mut self, addr: u16, value: u8, ctx: WriteContext) {
        match addr {
            0xFF10 => {
                let old = self.nr10();
                self.sweep = value.into();
                let value = self.nr10();
                if self.frequency_sweep.write_nr10(
                    self.get_period(),
                    old,
                    value,
                    ctx.lf_div,
                    ctx.cgb,
                    ctx.double_speed,
                ) {
                    self.enabled = false;
                }
                let mut period = self.get_period();
                self.frequency_sweep
                    .start_calculation(&mut period, value, ctx.lf_div);
                self.set_period(period);
            }
            0xFF11 => {
                self.timer = value.into();
                self.write_length(value);
                self.square_wave.set_duty(self.timer.wave_duty);
            }
            0xFF12 => {
                let old_value: u8 = self.volume.into();
                self.volume = value.into();
                if !self.dac_enabled() {
                    self.enabled = false;
                } else if self.enabled {
                    self.volume_envelope.write_nrx2(value, old_value, ctx.cgb);
                }
            }
            0xFF13 => {
                self.period_low = value;
                self.square_wave.period_written(self.get_period());
            }
            0xFF14 => {
                if value & 0x80 == 0
                    && self.enabled
                    && self.control.period_high == 7
                    && value & 0b111 != 7
                {
                    self.square_wave
                        .high_period_dropped(self.get_period(), ctx.cgb);
                }
                self.control = value.into();
                self.square_wave.period_written(self.get_period());
                let expired = self.length_counter.write_control(
                    self.control.length_enable,
                    self.control.trigger,
                    64,
                    ctx.frame_clocked_length,
                );
                if expired {
                    self.enabled = false;
                }
                if self.control.trigger {
                    self.trigger(ctx);
                }
            }
            _ => {}
        }
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
