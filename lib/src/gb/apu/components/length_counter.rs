#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LengthCounter {
    pub counter: u16,
    pub enabled: bool,
}

impl LengthCounter {
    pub fn clock(&mut self) -> bool {
        if self.enabled && self.counter > 0 {
            self.counter -= 1;
            if self.counter == 0 {
                return true;
            }
        }
        false
    }

    pub fn load(&mut self, max: u16, value: u16) {
        self.counter = max - value;
    }

    pub fn write_control(
        &mut self,
        enable: bool,
        trigger: bool,
        max: u16,
        frame_clocked_length: bool,
    ) -> bool {
        let mut was_enabled = self.enabled;
        if trigger && self.counter == 0 {
            self.counter = max;
            was_enabled = false;
        }

        let mut expired = false;
        if enable && !was_enabled && frame_clocked_length && self.counter > 0 {
            self.counter -= 1;
            if self.counter == 0 {
                if trigger {
                    self.counter = max - 1;
                } else {
                    expired = true;
                }
            }
        }

        self.enabled = enable;
        expired
    }
}
