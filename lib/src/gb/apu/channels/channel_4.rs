use crate::ReadMemory;
use crate::gb::apu::WriteContext;
use crate::gb::apu::components::length_counter::LengthCounter;
use crate::gb::apu::components::volume_envelope::VolumeEnvelope;
use crate::gb::apu::registers::ch4_frequency::Channel4Frequency;
use crate::gb::apu::registers::ch124_volume::Channel124Volume;

const DMG_DELAYED_START: u8 = 6;

#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Channel4 {
    pub enabled: bool,
    pub lfsr: u16,
    pub output: bool,
    pub counter: u16,
    pub countdown: u16,
    pub countdown_reloaded: bool,
    pub did_step_counter: bool,
    pub alignment: u8,
    pub half_tick: bool,
    pub counter_active: bool,
    pub background_counter_active: bool,
    pub started_with_dac_disabled: bool,
    pub dmg_delayed_start: u8,
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

        if self.output {
            self.volume_envelope.current_volume
        } else {
            0
        }
    }

    fn nr43(&self) -> u8 {
        self.frequency.into()
    }

    fn divisor(&self) -> u16 {
        match (self.nr43() & 0b111) as u16 {
            0 => 2,
            divider => divider << 2,
        }
    }

    fn counter_bit(&self, counter: u16, nr43: u8) -> bool {
        (counter >> (nr43 >> 4)) & 1 == 1
    }

    pub fn quiet(&self, ticks: u16) -> bool {
        self.dmg_delayed_start == 0
            && (!(self.counter_active || self.background_counter_active)
                || self.countdown > ticks / 2)
    }

    pub fn skip(&mut self, ticks: u16) {
        let steps = ticks / 2;
        self.alignment = self.alignment.wrapping_add(steps as u8);
        if self.counter_active || self.background_counter_active {
            self.countdown -= steps;
            self.countdown_reloaded = false;
        }
    }

    pub fn tick(&mut self) -> bool {
        self.half_tick = !self.half_tick;
        if !self.half_tick {
            return false;
        }

        self.alignment = self.alignment.wrapping_add(1);

        if self.dmg_delayed_start > 0 {
            self.dmg_delayed_start -= 1;
            if self.dmg_delayed_start == 0 {
                self.trigger_noise(false, false);
                return true;
            }
        }

        if !self.counter_active && !self.background_counter_active {
            return false;
        }

        let divisor = self.divisor();
        if self.countdown == 0 {
            self.countdown = divisor;
        }

        if self.countdown > 1 {
            self.countdown -= 1;
            self.countdown_reloaded = false;
            return false;
        }

        self.countdown = divisor;
        self.countdown_reloaded = true;
        let nr43 = self.nr43();
        let old_bit = self.counter_bit(self.counter, nr43);
        self.counter = (self.counter + 1) & 0x3FFF;
        self.did_step_counter = true;
        if !old_bit && self.counter_bit(self.counter, nr43) && self.enabled {
            self.step_lfsr();
            return true;
        }
        false
    }

    fn step_lfsr(&mut self) {
        let high_bit_mask = if self.frequency.lfsr_width {
            0x4040
        } else {
            0x4000
        };
        let new_high_bit = (self.lfsr ^ (self.lfsr >> 1) ^ 1) & 1 == 1;
        self.lfsr >>= 1;
        if new_high_bit {
            self.lfsr |= high_bit_mask;
        } else {
            self.lfsr &= !high_bit_mask;
        }
        self.output = self.lfsr & 1 == 1;
    }

    fn trigger_noise(&mut self, cgb: bool, double_speed: bool) {
        self.volume_envelope.trigger(self.volume.into());
        self.lfsr = 0;
        self.prepare_start(cgb, double_speed);
        self.output = false;
        self.did_step_counter = self.alignment & 3 == 2;
        if self.dac_enabled() {
            self.enabled = true;
        }
    }

    fn prepare_start(&mut self, cgb: bool, double_speed: bool) {
        self.counter_active = self.dac_enabled();
        let was_started_with_dac_disabled = self.started_with_dac_disabled;
        self.started_with_dac_disabled = !self.counter_active;
        let mut divisor = (self.nr43() & 0b111) as u16;
        let was_background_counting = self.background_counter_active;
        self.background_counter_active = true;
        let mut instant_step = false;
        let mut div_1_glitch = false;
        let aligned = self.alignment & 3 == 0;

        let early_step =
            self.countdown == 1 || (self.countdown == 2 && self.enabled && !cgb && double_speed);
        if divisor > 1 && early_step {
            self.counter = (self.counter + 1) & 0x3FFF;
        } else if self.countdown == 2 && aligned && self.enabled {
            if divisor == 0 {
                divisor = 8;
            } else if divisor == 1 {
                if !self.did_step_counter {
                    div_1_glitch = true;
                }
                let nr43 = self.nr43();
                let old_bit = self.counter_bit(self.counter, nr43);
                self.counter = (self.counter + 1) & 0x3FFF;
                instant_step = !old_bit && self.counter_bit(self.counter, nr43);
            }
        }

        let mut countdown = if divisor == 0 { 6 } else { divisor * 4 + 6 };
        if self.alignment & 1 == 1 {
            if divisor == 0 {
                if !cgb || !was_background_counting {
                    countdown += 1;
                } else {
                    countdown -= 1;
                }
            } else if self.alignment & 2 != 0 {
                if divisor == 1 && !self.enabled {
                    countdown += 1;
                } else {
                    countdown -= 3;
                }
            } else {
                countdown -= 1;
                if divisor == 1 && self.enabled {
                    countdown -= 4;
                }
            }
        } else if divisor != 0 {
            if self.alignment & 2 != 0 {
                if double_speed && !cgb && divisor == 1 {
                    countdown += 2;
                } else {
                    countdown -= 2;
                }
            } else if (divisor > 1 && (!double_speed || cgb))
                || (divisor == 1 && self.enabled && self.nr43() & 0xF0 == 0)
            {
                countdown -= 4;
            }
        } else if double_speed && !cgb {
            countdown += 2;
        }

        if divisor > 1 {
            if !self.counter_active && aligned {
                countdown += 4;
            }
        } else if was_background_counting && !self.enabled && aligned {
            if divisor == 0 {
                if was_started_with_dac_disabled {
                    countdown += 28;
                }
            } else {
                countdown -= 4;
            }
        }

        if divisor == 0 && !cgb && was_background_counting && !self.enabled && double_speed {
            countdown -= 1;
        }
        if div_1_glitch {
            countdown -= 4;
        }
        self.countdown = countdown;

        self.lfsr = if divisor == 0 && self.enabled && self.alignment & 3 == 3 {
            0x0055
        } else {
            0
        };
        if instant_step {
            self.step_lfsr();
        }
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

    pub fn dac_enabled(&self) -> bool {
        self.volume.initial_volume > 0 || self.volume.envelope_direction
    }

    pub fn write_length(&mut self, value: u8) {
        self.initial_length_timer = value & 0x3F;
        self.length_counter
            .load(64, self.initial_length_timer as u16);
    }

    fn write_nr43(&mut self, value: u8, cgb: bool) {
        if self.countdown_reloaded {
            let divisor = match (value & 0b111) as u16 {
                0 => 2,
                divider => divider << 2,
            };
            let offsets: [u16; 4] = if cgb { [2, 1, 0, 3] } else { [2, 1, 4, 3] };
            self.countdown = if divisor == 2 {
                divisor
            } else {
                divisor + offsets[(self.alignment & 3) as usize]
            };
        }

        if !cgb && self.countdown_reloaded {
            let old = self.nr43();
            let counter = self.counter;
            if !self.counter_bit(counter, old)
                && self.counter_bit(counter, value)
                && (counter >> 7) & 1 == 1
            {
                let previous = (counter.wrapping_sub(1)) & 0x3FFF;
                if self.counter_bit(previous, old)
                    && !self.counter_bit(previous, value)
                    && (previous >> 7) & 1 == 1
                {
                    self.step_lfsr();
                }
            }
            self.apply_nr43(0xFF, cgb);
        }
        self.apply_nr43(value, cgb);
    }

    fn apply_nr43(&mut self, new: u8, cgb: bool) {
        let old = self.nr43();
        self.frequency = new.into();
        if (old ^ new) & 0xF0 == 0 {
            return;
        }

        let mut counter = self.counter;
        if !cgb && self.countdown_reloaded {
            counter |= counter.wrapping_sub(1) & 0x3FFF;
        }
        let old_bit = self.counter_bit(counter, old);
        let glitch_bit = self.counter_bit(counter, (old & 0x7F) | (new & 0x80));
        let new_bit = self.counter_bit(counter, new);

        if old_bit == new_bit && new_bit != glitch_bit {
            if !new_bit || !cgb || new & 0x80 == 0 {
                self.step_lfsr();
            }
        } else if !old_bit && new_bit {
            self.step_lfsr();
        }
    }

    pub fn write(&mut self, addr: u16, value: u8, ctx: WriteContext) {
        match addr {
            0xFF20 => self.write_length(value),
            0xFF21 => {
                let old_value: u8 = self.volume.into();
                self.volume = value.into();
                if !self.dac_enabled() {
                    if self.enabled && self.nr43() & 0b111 != 0 {
                        if self.countdown <= 2 {
                            self.counter = (self.counter + 1) & 0x3FFF;
                        }
                        self.background_counter_active = false;
                    }
                    self.counter_active = false;
                    self.enabled = false;
                } else if self.enabled {
                    self.volume_envelope.write_nrx2(value, old_value, ctx.cgb);
                }
            }
            0xFF22 => self.write_nr43(value, ctx.cgb),
            0xFF23 => {
                self.trigger = (value & 0b1000_0000) != 0;
                let expired = self.length_counter.write_control(
                    (value & 0b0100_0000) != 0,
                    self.trigger,
                    64,
                    ctx.frame_clocked_length,
                );
                if expired {
                    self.enabled = false;
                }
                if self.trigger {
                    if !ctx.cgb && self.alignment & 3 != 0 {
                        self.volume_envelope.trigger(self.volume.into());
                        self.dmg_delayed_start = DMG_DELAYED_START;
                    } else {
                        self.trigger_noise(ctx.cgb, ctx.double_speed);
                    }
                }
            }
            _ => {}
        }
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
