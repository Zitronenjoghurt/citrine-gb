use crate::gb::apu::Step;

const TRIGGER_DELAY: u16 = 10;
const RETRIGGER_DELAY: u16 = 6;

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct SquareWave {
    pub duty_pattern: u8,
    pub duty_step: u8,
    pub frequency_timer: u16,
    pub suppressed: bool,
    pub output: bool,
    pub just_reloaded: bool,
    pub did_tick: bool,
}

impl SquareWave {
    const DUTY_TABLE: [[bool; 8]; 4] = [
        [false, false, false, false, false, false, false, true],
        [true, false, false, false, false, false, false, true],
        [true, false, false, false, false, true, true, true],
        [false, true, true, true, true, true, true, false],
    ];

    const STEADY_RELOADS: [[[u8; 2]; 8]; 4] = {
        let mut table = [[[8u8; 2]; 8]; 4];
        let mut pattern = 0;
        while pattern < 4 {
            let mut step = 0;
            while step < 8 {
                let mut now = 0;
                while now < 2 {
                    let mut ahead = 7;
                    while ahead >= 1 {
                        if Self::DUTY_TABLE[pattern][(step + ahead) & 0b111] != (now == 1) {
                            table[pattern][step][now] = ahead as u8;
                        }
                        ahead -= 1;
                    }
                    now += 1;
                }
                step += 1;
            }
            pattern += 1;
        }
        table
    };

    pub fn tick(&mut self, period: u16) -> Step {
        self.just_reloaded = false;
        self.frequency_timer = self.frequency_timer.saturating_sub(1);

        if self.frequency_timer == 0 {
            let before = self.sample();
            self.set_frequency(period);
            self.duty_step = (self.duty_step + 1) & 0b111;
            self.output = Self::DUTY_TABLE[self.duty_pattern as usize][self.duty_step as usize];
            self.suppressed = false;
            self.just_reloaded = true;
            self.did_tick = true;
            return if self.sample() == before {
                Step::Same
            } else {
                Step::Changed
            };
        }
        Step::Idle
    }

    pub fn quiet(&self, ticks: u16) -> bool {
        self.frequency_timer > ticks
    }

    pub fn skip(&mut self, ticks: u16) {
        self.frequency_timer -= ticks;
        self.just_reloaded = false;
    }

    pub fn quiet_ticks(&self) -> u32 {
        u32::from(self.frequency_timer.saturating_sub(1))
    }

    pub fn steady_ticks(&self, period: u16) -> u32 {
        let reloads = Self::STEADY_RELOADS[self.duty_pattern as usize]
            [usize::from(self.duty_step & 0b111)][usize::from(self.sample())];
        self.quiet_ticks() + u32::from(reloads - 1) * u32::from((2048 - period) * 4)
    }

    pub fn advance(&mut self, ticks: u32, period: u16) {
        self.just_reloaded = false;
        if ticks < u32::from(self.frequency_timer) {
            self.frequency_timer -= ticks as u16;
            return;
        }
        let first = u32::from(self.frequency_timer.max(1));
        if ticks < first {
            return;
        }
        let reload = u32::from((2048 - period) * 4);
        let since = (ticks - first) % reload;
        let reloads = 1 + (ticks - first) / reload;
        self.frequency_timer = (reload - since) as u16;
        self.duty_step = ((u32::from(self.duty_step) + reloads) & 0b111) as u8;
        self.output = Self::DUTY_TABLE[self.duty_pattern as usize][self.duty_step as usize];
        self.suppressed = false;
        self.just_reloaded = since == 0;
        self.did_tick = true;
    }

    pub fn trigger(&mut self, period: u16, was_enabled: bool, lf_div: u16) {
        self.did_tick = false;
        if was_enabled {
            self.frequency_timer = (2048 - period) * 4 + RETRIGGER_DELAY - 2 * lf_div;
        } else {
            self.frequency_timer = (2048 - period) * 4 + TRIGGER_DELAY - 2 * lf_div;
            self.suppressed = true;
        }
    }

    pub fn high_period_dropped(&mut self, old_period: u16, cgb: bool) {
        let reload = (2048 - old_period) * 4;
        let reloaded = if cgb {
            self.frequency_timer == reload || self.frequency_timer == reload - 2
        } else {
            self.frequency_timer == reload
        };
        if self.did_tick && reloaded {
            self.duty_step = (self.duty_step + 7) & 0b111;
            self.suppressed = false;
        }
    }

    pub fn period_written(&mut self, period: u16) {
        if self.just_reloaded {
            self.set_frequency(period);
        }
    }

    pub fn set_duty(&mut self, wave_duty: u8) {
        self.duty_pattern = wave_duty & 0b11;
    }

    pub fn set_frequency(&mut self, period: u16) {
        self.frequency_timer = (2048 - period) * 4;
    }

    pub fn sample(&self) -> u8 {
        if !self.suppressed && self.output {
            1
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advancing_over_quiet_ticks_matches_ticking_them_one_by_one() {
        for timer in 0..40 {
            let start = SquareWave {
                frequency_timer: timer,
                just_reloaded: true,
                duty_pattern: 2,
                ..SquareWave::default()
            };
            let quiet = start.quiet_ticks();
            let mut ticked = start.clone();
            for _ in 0..quiet {
                assert_eq!(ticked.tick(0x700), Step::Idle);
            }
            let mut advanced = start.clone();
            if quiet > 0 {
                advanced.advance(quiet, 0x700);
            }
            assert_eq!(format!("{advanced:?}"), format!("{ticked:?}"));
            assert_ne!(ticked.tick(0x700), Step::Idle);
        }
    }

    fn starts() -> impl Iterator<Item = SquareWave> {
        (0..4u8).flat_map(|duty_pattern| {
            (0..8u8).flat_map(move |duty_step| {
                [0u16, 1, 2, 7, 30]
                    .into_iter()
                    .flat_map(move |frequency_timer| {
                        [false, true].into_iter().map(move |suppressed| SquareWave {
                            duty_pattern,
                            duty_step,
                            frequency_timer,
                            suppressed,
                            output: SquareWave::DUTY_TABLE[duty_pattern as usize]
                                [duty_step as usize],
                            just_reloaded: frequency_timer == 1,
                            did_tick: !suppressed,
                        })
                    })
            })
        })
    }

    #[test]
    fn advancing_over_steady_ticks_matches_ticking_them_one_by_one() {
        for period in [0x7FE, 0x7FF, 0x7F0] {
            for start in starts() {
                let steady = start.steady_ticks(period);
                let mut ticked = start.clone();
                for _ in 0..steady {
                    assert_ne!(ticked.tick(period), Step::Changed);
                }
                let mut advanced = start.clone();
                if steady > 0 {
                    advanced.advance(steady, period);
                }
                assert_eq!(format!("{advanced:?}"), format!("{ticked:?}"));
                assert_eq!(ticked.tick(period), Step::Changed);
            }
        }
    }

    #[test]
    fn advancing_one_tick_past_the_quiet_ticks_matches_ticking_them() {
        for timer in 0..40 {
            let start = SquareWave {
                frequency_timer: timer,
                just_reloaded: true,
                duty_pattern: 2,
                ..SquareWave::default()
            };
            let ticks = start.quiet_ticks() + 1;
            let mut ticked = start.clone();
            for _ in 0..ticks {
                ticked.tick(0x700);
            }
            let mut advanced = start.clone();
            advanced.advance(ticks, 0x700);
            assert_eq!(format!("{advanced:?}"), format!("{ticked:?}"), "{timer}");
        }
    }

    #[test]
    fn advancing_one_tick_past_the_steady_ticks_matches_ticking_them() {
        for period in [0x7FE, 0x7FF, 0x7F0] {
            for start in starts() {
                let ticks = start.steady_ticks(period) + 1;
                let mut ticked = start.clone();
                for _ in 0..ticks {
                    ticked.tick(period);
                }
                let mut advanced = start.clone();
                advanced.advance(ticks, period);
                assert_eq!(
                    format!("{advanced:?}"),
                    format!("{ticked:?}"),
                    "{start:?} {period:#x}"
                );
            }
        }
    }

    #[test]
    fn advancing_matches_ticking_one_by_one() {
        for period in [0x7FE, 0x7FF, 0x7F0] {
            for start in starts() {
                for ticks in [1, 2, 3, 8, 9, 31, 64, 65, 200] {
                    let mut ticked = start.clone();
                    for _ in 0..ticks {
                        ticked.tick(period);
                    }
                    let mut advanced = start.clone();
                    advanced.advance(ticks, period);
                    assert_eq!(format!("{advanced:?}"), format!("{ticked:?}"));
                }
            }
        }
    }
}
