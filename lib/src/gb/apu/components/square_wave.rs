#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SquareWave {
    pub duty_pattern: u8,
    pub duty_step: u8,
    pub frequency_timer: u16,
}

impl SquareWave {
    const DUTY_TABLE: [[bool; 8]; 4] = [
        [false, false, false, false, false, false, false, true],
        [true, false, false, false, false, false, false, true],
        [true, false, false, false, false, true, true, true],
        [false, true, true, true, true, true, true, false],
    ];

    pub fn tick(&mut self, period: u16) {
        if self.frequency_timer > 0 {
            self.frequency_timer -= 1;
        }

        if self.frequency_timer == 0 {
            self.set_frequency(period);
            self.duty_step = (self.duty_step + 1) & 0b111;
        }
    }

    pub fn set_duty(&mut self, wave_duty: u8) {
        self.duty_pattern = wave_duty & 0b11;
    }

    pub fn set_frequency(&mut self, period: u16) {
        self.frequency_timer = (2048 - period) * 4;
    }

    pub fn sample(&self) -> u8 {
        if Self::DUTY_TABLE[self.duty_pattern as usize][self.duty_step as usize] {
            1
        } else {
            0
        }
    }
}
