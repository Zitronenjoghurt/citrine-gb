const TRIGGER_DELAY: u16 = 10;
const RETRIGGER_DELAY: u16 = 6;

#[derive(Debug, Default)]
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

    pub fn tick(&mut self, period: u16) -> bool {
        self.just_reloaded = false;
        self.frequency_timer = self.frequency_timer.saturating_sub(1);

        if self.frequency_timer == 0 {
            self.set_frequency(period);
            self.duty_step = (self.duty_step + 1) & 0b111;
            self.output = Self::DUTY_TABLE[self.duty_pattern as usize][self.duty_step as usize];
            self.suppressed = false;
            self.just_reloaded = true;
            self.did_tick = true;
            return true;
        }
        false
    }

    pub fn quiet(&self, ticks: u16) -> bool {
        self.frequency_timer > ticks
    }

    pub fn skip(&mut self, ticks: u16) {
        self.frequency_timer -= ticks;
        self.just_reloaded = false;
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
