#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct VolumeEnvelope {
    /// Current volume (0-15)
    pub current_volume: u8,
    pub countdown: u8,
    pub clock: bool,
    pub locked: bool,
    pub should_lock: bool,
}

impl VolumeEnvelope {
    pub fn trigger(&mut self, nrx2: u8) {
        self.locked = false;
        self.clock = false;
        self.current_volume = nrx2 >> 4;
        self.countdown = nrx2 & 0b111;
    }

    pub fn div_event(&mut self, div_apu: u8) {
        if div_apu & 0b111 == 0b111 && !self.clock {
            self.countdown = self.countdown.wrapping_sub(1) & 0b111;
        }
    }

    pub fn secondary_div_event(&mut self, nrx2: u8) {
        if self.countdown == 0 {
            self.countdown = nrx2 & 0b111;
            self.set_clock(self.countdown != 0, nrx2 & 0b1000 != 0);
        }
    }

    pub fn tick(&mut self, nrx2: u8) {
        if !self.clock {
            return;
        }
        self.set_clock(false, false);
        if self.locked || nrx2 & 0b111 == 0 {
            return;
        }
        self.step(nrx2);
    }

    pub fn write_nrx2(&mut self, value: u8, old_value: u8, cgb: bool) {
        if cgb {
            self.glitch(value, old_value);
        } else {
            self.glitch(0xFF, old_value);
            self.glitch(value, 0xFF);
        }
    }

    fn step(&mut self, nrx2: u8) {
        self.current_volume = if nrx2 & 0b1000 != 0 {
            self.current_volume.wrapping_add(1)
        } else {
            self.current_volume.wrapping_sub(1)
        } & 0xF;
    }

    fn set_clock(&mut self, value: bool, increasing: bool) {
        if self.clock == value {
            return;
        }
        self.clock = value;
        if value {
            self.should_lock = (self.current_volume == 0xF && increasing)
                || (self.current_volume == 0 && !increasing);
        } else {
            self.locked |= self.should_lock;
        }
    }

    fn glitch(&mut self, value: u8, old_value: u8) {
        if self.clock {
            self.countdown = value & 0b111;
        }

        let mut should_tick = value & 0b111 != 0 && old_value & 0b111 == 0 && !self.locked;
        if value & 0xF == 0b1000 && old_value & 0xF == 0b1000 && !self.locked {
            should_tick = true;
        }

        if (value ^ old_value) & 0b1000 != 0 {
            if value & 0b1000 != 0 {
                if old_value & 0b111 == 0 && !self.locked {
                    self.current_volume ^= 0xF;
                } else {
                    self.current_volume = 0xE_u8.wrapping_sub(self.current_volume) & 0xF;
                }
                should_tick = false;
            } else {
                self.current_volume = 0x10_u8.wrapping_sub(self.current_volume) & 0xF;
            }
        }

        if should_tick {
            self.step(value);
        } else if value & 0b111 == 0 && self.clock {
            self.set_clock(false, false);
        }
    }
}
