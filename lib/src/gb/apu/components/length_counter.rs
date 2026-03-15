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

    pub fn trigger(&mut self, length: u16) {
        if self.counter == 0 {
            self.counter = length;
        }
    }

    pub fn reload(&mut self, length: u16) {
        self.counter = length;
    }
}
