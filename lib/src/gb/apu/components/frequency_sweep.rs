#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct FrequencySweep {
    pub countdown: u8,
    pub calculate_countdown: u8,
    pub reload_timer: u8,
    pub instant_calculation_done: bool,
    pub unshifted: bool,
    pub shadow_period: u16,
    pub addend: u16,
    pub completed_addend: u16,
    pub restart_hold: u8,
}

impl FrequencySweep {
    pub fn trigger(&mut self, period: u16, nr10: u8, was_enabled: bool, lf_div: u16, cgb: bool) {
        let shift = nr10 & 0b111;
        self.instant_calculation_done = false;
        self.shadow_period = 0;
        self.completed_addend = 0;
        if shift != 0 {
            self.calculate_countdown = shift;
            self.reload_timer = if !cgb && lf_div == 0 { 3 } else { 2 };
            self.unshifted = false;
            if !was_enabled {
                self.reload_timer += 1;
            }
            self.addend = period >> shift;
        } else {
            self.addend = 0;
        }
        self.restart_hold = (2 - lf_div as u8) + if cgb { 2 } else { 0 };
        self.countdown = ((nr10 >> 4) & 0b111) ^ 0b111;
    }

    pub fn busy(&self) -> bool {
        self.calculate_countdown != 0
            || self.reload_timer != 0
            || self.restart_hold != 0
            || self.instant_calculation_done
    }

    pub fn div_event(&mut self, period: &mut u16, nr10: u8, lf_div: u16) {
        self.countdown = (self.countdown + 1) & 0b111;
        self.start_calculation(period, nr10, lf_div);
    }

    pub fn start_calculation(&mut self, period: &mut u16, nr10: u8, lf_div: u16) {
        if nr10 & 0x70 == 0 || self.countdown != 0b111 {
            return;
        }
        let shift = nr10 & 0b111;
        let negate = (nr10 >> 3) as u16 & 1;
        if shift != 0 {
            *period = (self.addend + self.shadow_period + negate) & 0x7FF;
        }
        if self.restart_hold == 0 {
            self.addend = *period >> shift;
        }
        self.calculate_countdown = shift;
        self.reload_timer = 1 + lf_div as u8;
        self.unshifted = shift == 0;
        self.countdown = ((nr10 >> 4) & 0b111) ^ 0b111;
        if self.calculate_countdown == 0 {
            self.instant_calculation_done = true;
        }
    }

    pub fn tick(&mut self, period: u16, nr10: u8, lf_div: u16) -> bool {
        let mut disable = false;
        let mut sweep_cycles = (lf_div == 0) as u8;

        if self.reload_timer > sweep_cycles {
            self.reload_timer -= sweep_cycles;
            sweep_cycles = 0;
        } else {
            if self.reload_timer != 0
                && self.calculate_countdown == 0
                && self.instant_calculation_done
            {
                disable |= self.calculation_done(period, nr10);
            }
            self.instant_calculation_done = false;
            sweep_cycles -= self.reload_timer;
            self.reload_timer = 0;
        }

        if self.calculate_countdown != 0 && (nr10 & 0b111 != 0 || self.unshifted) {
            if self.calculate_countdown > sweep_cycles {
                self.calculate_countdown -= sweep_cycles;
            } else {
                self.calculate_countdown = 0;
                disable |= self.calculation_done(period, nr10);
            }
        }

        self.restart_hold = self.restart_hold.saturating_sub(1);
        disable
    }

    fn calculation_done(&mut self, period: u16, nr10: u8) -> bool {
        if self.restart_hold == 0 {
            self.shadow_period = period;
        }
        let negate = nr10 & 0b1000 != 0;
        if negate {
            self.addend ^= 0x7FF;
        }
        let disable = self.shadow_period + self.addend > 0x7FF && !negate;
        self.completed_addend = self.addend;
        disable
    }

    pub fn write_nr10(
        &mut self,
        period: u16,
        old: u8,
        value: u8,
        lf_div: u16,
        cgb: bool,
        double_speed: bool,
    ) -> bool {
        let mut disable = false;
        if self.calculate_countdown != 0 || self.reload_timer != 0 {
            disable |= self.write_glitch(period, old, value, lf_div, cgb, double_speed);
        }
        let old_negate = if cgb { (old >> 3) as u16 & 1 } else { 1 };
        if self.shadow_period + self.completed_addend + old_negate > 0x7FF && value & 0b1000 == 0 {
            disable = true;
        }
        disable
    }

    fn write_glitch(
        &mut self,
        period: u16,
        old: u8,
        value: u8,
        lf_div: u16,
        cgb: bool,
        double_speed: bool,
    ) -> bool {
        if cgb {
            if self.reload_timer == 2 {
                self.calculate_countdown = value & 0b111;
                if self.calculate_countdown == 0 {
                    self.reload_timer = 0;
                }
            }
            if value & 0b111 != 0 && old & 0b111 == 0 && lf_div == 0 && self.calculate_countdown > 1
            {
                self.calculate_countdown -= 1;
            }
            return false;
        }

        if (self.reload_timer == 1 && lf_div == 0)
            || self.reload_timer > 1
            || self.calculate_countdown == 0
        {
            return false;
        }
        let zombie_step = if old & 0b111 == 0 {
            (lf_div == 1) != double_speed
        } else {
            double_speed && self.calculate_countdown == 1
        };
        if zombie_step {
            self.calculate_countdown -= 1;
            if self.calculate_countdown <= 1 {
                self.calculate_countdown = 0;
                return self.calculation_done(period, old);
            }
        }
        false
    }
}
