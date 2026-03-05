#[derive(Debug, Default)]
pub struct VolumeEnvelope {
    /// Direction of the envelope
    pub increasing: bool,
    /// Current volume (0-15)
    pub current_volume: u8,
    pub pace: u8,
    pub timer: u8,
}

impl VolumeEnvelope {
    pub fn clock(&mut self) {
        if self.pace == 0 {
            return;
        }

        if self.timer > 0 {
            self.timer -= 1;
        }

        if self.timer == 0 {
            self.timer = self.pace;

            if self.increasing && self.current_volume < 15 {
                self.current_volume += 1;
            } else if !self.increasing && self.current_volume > 0 {
                self.current_volume -= 1;
            }
        }
    }

    pub fn trigger(&mut self, increasing: bool, initial_volume: u8, pace: u8) {
        self.increasing = increasing;
        self.current_volume = initial_volume;
        self.pace = pace;
        self.timer = pace;
    }
}
