#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FrequencySweep {
    pub timer: u8,
    pub enabled: bool,
    pub shadow_period: u16,
}

impl FrequencySweep {
    /// Called when the channel is triggered via NR14
    /// Returns true if the channel should be immediately disabled due to overflow.
    pub fn trigger(&mut self, current_period: u16, pace: u8, shift: u8, decrease: bool) -> bool {
        self.shadow_period = current_period;
        self.timer = if pace == 0 { 8 } else { pace };
        self.enabled = pace > 0 || shift > 0;

        if shift > 0 {
            let (_, overflow) = self.calculate(self.shadow_period, shift, decrease);
            if overflow {
                return true;
            }
        }

        false
    }

    pub fn clock(&mut self, pace: u8, shift: u8, decrease: bool) -> (bool, Option<u16>) {
        let mut disable = false;
        let mut new_period_out = None;

        if self.timer > 0 {
            self.timer -= 1;
        }

        if self.timer == 0 {
            self.timer = if pace == 0 { 8 } else { pace };

            if self.enabled && pace > 0 {
                let (new_period, overflow) = self.calculate(self.shadow_period, shift, decrease);

                if overflow {
                    disable = true;
                } else if shift > 0 {
                    self.shadow_period = new_period;
                    new_period_out = Some(new_period);

                    // Hardware quirk: do a second calculation & overflow check BUT but do NOT save the new frequency
                    let (_, overflow2) = self.calculate(self.shadow_period, shift, decrease);
                    if overflow2 {
                        disable = true;
                    }
                }
            }
        }

        (disable, new_period_out)
    }

    fn calculate(&self, target_period: u16, shift: u8, decrease: bool) -> (u16, bool) {
        let offset = target_period >> shift;

        let new_period = if decrease {
            target_period.saturating_sub(offset)
        } else {
            target_period + offset
        };

        let overflow = new_period > 2047;

        (new_period, overflow)
    }
}
