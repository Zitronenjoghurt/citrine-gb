use crate::ReadMemory;
use crate::gb::apu::Step;
use crate::gb::apu::WriteContext;
use crate::gb::apu::components::length_counter::LengthCounter;
use crate::gb::apu::registers::ch123_control::Channel123Control;

const TRIGGER_DELAY: u16 = 6;

#[derive(Debug, Clone, Default)]
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

    pub fn quiet_ticks(&self) -> u32 {
        if self.enabled {
            u32::from(self.frequency_timer.saturating_sub(1))
        } else {
            u32::MAX
        }
    }

    pub fn steady_ticks(&self) -> u32 {
        let shift = match self.output_level & 0b11 {
            0 => return u32::MAX,
            level => level - 1,
        };
        if !self.enabled {
            return u32::MAX;
        }
        let now = self.get_wave_value() >> shift;
        let mut ahead = 1;
        while self.wave_value_at(self.wave_step + ahead) >> shift == now {
            if ahead == 32 {
                return u32::MAX;
            }
            ahead += 1;
        }
        let quiet = u32::from(self.frequency_timer.saturating_sub(1));
        quiet + u32::from(ahead - 1) * u32::from((2048 - self.get_period()) * 2)
    }

    fn wave_value_at(&self, step: u8) -> u8 {
        let step = step & 0b11111;
        let byte = self.wave_ram[usize::from(step / 2)];
        if step.is_multiple_of(2) {
            byte >> 4
        } else {
            byte & 0b1111
        }
    }

    pub fn advance(&mut self, ticks: u32) {
        self.just_read = false;
        if !self.enabled {
            return;
        }
        if ticks < u32::from(self.frequency_timer) {
            self.frequency_timer -= ticks as u16;
            return;
        }
        let first = u32::from(self.frequency_timer.max(1));
        if ticks < first {
            return;
        }
        let reload = u32::from((2048 - self.get_period()) * 2);
        let rest = ticks - first;
        self.frequency_timer = (reload - rest % reload) as u16;
        self.wave_step = ((u32::from(self.wave_step) + 1 + rest / reload) & 0b11111) as u8;
        self.sample_buffer = self.wave_ram[usize::from(self.wave_step / 2)];
        self.just_read = rest.is_multiple_of(reload);
    }

    pub fn tick(&mut self) -> Step {
        self.just_read = false;
        if !self.enabled {
            return Step::Idle;
        }

        self.frequency_timer = self.frequency_timer.saturating_sub(1);
        if self.frequency_timer == 0 {
            let before = self.get_wave_value();
            self.reset_frequency_timer();
            self.wave_step = (self.wave_step + 1) & 0b11111;
            self.sample_buffer = self.wave_ram[self.wave_step as usize / 2];
            self.just_read = true;
            return if self.get_wave_value() == before {
                Step::Same
            } else {
                Step::Changed
            };
        }
        Step::Idle
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advancing_over_quiet_ticks_matches_ticking_them_one_by_one() {
        for timer in 0..40 {
            for enabled in [false, true] {
                let start = Channel3 {
                    enabled,
                    frequency_timer: timer,
                    just_read: true,
                    wave_ram: core::array::from_fn(|i| i as u8 * 17),
                    ..Channel3::default()
                };
                let quiet = start.quiet_ticks().min(100);
                let mut ticked = start.clone();
                for _ in 0..quiet {
                    assert_eq!(ticked.tick(), Step::Idle);
                }
                let mut advanced = start.clone();
                if quiet > 0 {
                    advanced.advance(quiet);
                }
                assert_eq!(format!("{advanced:?}"), format!("{ticked:?}"));
                assert_eq!(ticked.tick() != Step::Idle, enabled);
            }
        }
    }

    fn steady_starts() -> impl Iterator<Item = Channel3> {
        let waves: [[u8; 16]; 3] = [
            core::array::from_fn(|i| i as u8 * 17),
            [
                0x02, 0x46, 0x8A, 0xCE, 0xFF, 0xFE, 0xED, 0xDC, 0xCB, 0xA9, 0x87, 0x65, 0x44, 0x33,
                0x22, 0x11,
            ],
            [0x77; 16],
        ];
        waves.into_iter().flat_map(|wave_ram| {
            (0..4u8).flat_map(move |output_level| {
                [0u8, 1, 6, 31].into_iter().flat_map(move |wave_step| {
                    [0u16, 1, 2, 7]
                        .into_iter()
                        .map(move |frequency_timer| Channel3 {
                            enabled: true,
                            dac_enabled: true,
                            wave_step,
                            sample_buffer: wave_ram[usize::from(wave_step / 2)],
                            frequency_timer,
                            output_level,
                            period_low: 0xFC,
                            control: Channel123Control {
                                period_high: 7,
                                ..Channel123Control::default()
                            },
                            wave_ram,
                            ..Channel3::default()
                        })
                })
            })
        })
    }

    #[test]
    fn advancing_over_steady_ticks_matches_ticking_them_one_by_one() {
        for start in steady_starts() {
            let steady = start.steady_ticks().min(1000);
            let mut ticked = start.clone();
            for _ in 0..steady {
                ticked.tick();
                assert_eq!(ticked.sample(), start.sample());
            }
            let mut advanced = start.clone();
            if steady > 0 {
                advanced.advance(steady);
            }
            assert_eq!(format!("{advanced:?}"), format!("{ticked:?}"));
            if steady < 1000 {
                assert_eq!(ticked.tick(), Step::Changed);
                assert_ne!(ticked.sample(), start.sample());
            }
        }
    }

    #[test]
    fn advancing_one_tick_past_the_quiet_ticks_matches_ticking_them() {
        for timer in 0..40 {
            for enabled in [false, true] {
                let start = Channel3 {
                    enabled,
                    frequency_timer: timer,
                    just_read: true,
                    wave_ram: core::array::from_fn(|i| i as u8 * 17),
                    ..Channel3::default()
                };
                let quiet = start.quiet_ticks();
                if quiet >= 100 {
                    continue;
                }
                let mut ticked = start.clone();
                for _ in 0..=quiet {
                    ticked.tick();
                }
                let mut advanced = start.clone();
                advanced.advance(quiet + 1);
                assert_eq!(format!("{advanced:?}"), format!("{ticked:?}"), "{start:?}");
            }
        }
    }

    #[test]
    fn advancing_one_tick_past_the_steady_ticks_matches_ticking_them() {
        for start in steady_starts() {
            let steady = start.steady_ticks();
            if steady >= 1000 {
                continue;
            }
            let mut ticked = start.clone();
            for _ in 0..=steady {
                ticked.tick();
            }
            let mut advanced = start.clone();
            advanced.advance(steady + 1);
            assert_eq!(format!("{advanced:?}"), format!("{ticked:?}"), "{start:?}");
        }
    }
}
