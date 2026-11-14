use crate::gb::ic::ICInterface;
use crate::gb::serial::Serial;
use crate::{ReadMemory, WriteMemory};

const LAZY_LIMIT: u32 = 1 << 16;

const TIMA_BITS: [u16; 8] = [0, 0, 0, 0, 1 << 9, 1 << 3, 1 << 5, 1 << 7];

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Timer {
    pub div: u16,
    pub tima: u8,
    pub tma: u8,
    pub tac: u8,
    prev_and: bool,
    overflow_pending: bool,
    is_reloading: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    owed: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    lazy: u32,
}

impl Default for Timer {
    fn default() -> Self {
        Self {
            div: 0xAB,
            tima: 0x00,
            tma: 0x00,
            tac: 0xF8,
            prev_and: false,
            overflow_pending: false,
            is_reloading: false,
            owed: 0,
            lazy: 0,
        }
    }
}

impl Timer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cycle(&mut self, ic: &mut impl ICInterface) {
        self.is_reloading = false;

        if self.overflow_pending {
            self.overflow_pending = false;
            self.is_reloading = true;
            self.tima = self.tma;
            ic.request_interrupt(crate::gb::ic::Interrupt::Timer);
        }

        self.div = self.div.wrapping_add(4);
        self.check_falling_edge();
    }

    #[inline(always)]
    pub fn owe_cycle(&mut self) -> bool {
        if self.lazy == 0 {
            return false;
        }
        self.lazy -= 1;
        self.owed += 1;
        true
    }

    pub fn lazy_cycles(&self) -> u32 {
        self.lazy
    }

    pub fn owe_cycles(&mut self, cycles: u32) {
        self.lazy -= cycles;
        self.owed += cycles;
    }

    pub fn settle(&mut self, serial: &mut Serial, cgb_mode: bool) {
        let owed = std::mem::take(&mut self.owed);
        if owed > 0 {
            let old_div = self.div;
            self.advance(owed);
            serial.div_advanced(old_div, owed, cgb_mode);
        }
    }

    pub fn set_lazy(&mut self, cycles: u32) {
        self.lazy = cycles.min(LAZY_LIMIT);
    }

    pub fn quiet_cycles(&self) -> u32 {
        if self.overflow_pending || self.prev_and != self.current_and() {
            return 0;
        }
        let bit = TIMA_BITS[(self.tac & 0b111) as usize];
        if bit == 0 {
            return u32::MAX;
        }
        let period = u32::from(bit) * 2;
        let div = u32::from(self.div);
        let edges_left = u32::from(255 - self.tima);
        ((div / period + edges_left + 1) * period - 1 - div) / 4
    }

    pub fn advance(&mut self, cycles: u32) {
        let div = u32::from(self.div);
        let end = div + cycles * 4;
        let bit = TIMA_BITS[(self.tac & 0b111) as usize];
        if bit != 0 {
            let period = u32::from(bit) * 2;
            self.tima = self.tima.wrapping_add((end / period - div / period) as u8);
        }
        self.div = end as u16;
        self.is_reloading = false;
        self.prev_and = self.current_and();
    }

    fn tima_bit(&self) -> u16 {
        match self.tac & 0b11 {
            0b00 => 1 << 9,
            0b01 => 1 << 3,
            0b10 => 1 << 5,
            0b11 => 1 << 7,
            _ => unreachable!(),
        }
    }

    fn timer_enabled(&self) -> bool {
        self.tac & 0b100 != 0
    }

    fn current_and(&self) -> bool {
        self.div & TIMA_BITS[(self.tac & 0b111) as usize] != 0
    }

    fn check_falling_edge(&mut self) {
        let new_and = self.current_and();
        if self.prev_and && !new_and {
            self.increment_tima();
        }
        self.prev_and = new_and;
    }

    fn increment_tima(&mut self) {
        self.tima = self.tima.wrapping_add(1);
        if self.tima == 0 {
            self.overflow_pending = true;
        }
    }

    pub fn speed_switch_reset(&mut self, lookahead: bool) {
        let lookahead = if lookahead && self.tac & 0b11 == 0b01 {
            4
        } else {
            0
        };
        let bit_set =
            self.timer_enabled() && (self.div.wrapping_add(lookahead) & self.tima_bit()) != 0;
        self.div = 0;
        self.prev_and = false;
        if bit_set {
            self.increment_tima();
        }
    }

    pub fn soft_reset(&mut self) {
        *self = Self::default();
    }
}

impl ReadMemory for Timer {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF04 => (self.div >> 8) as u8,
            0xFF05 => self.tima,
            0xFF06 => self.tma,
            0xFF07 => self.tac | 0xF8,
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Timer {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF04 => {
                self.div = 0;
                self.check_falling_edge();
            }
            0xFF05 => {
                if !self.is_reloading {
                    self.tima = value;
                    self.overflow_pending = false;
                }
            }
            0xFF06 => {
                self.tma = value;
                if self.is_reloading {
                    self.tima = value;
                }
            }
            0xFF07 => {
                self.tac = value;
                self.check_falling_edge();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gb::ic::InterruptController;

    #[test]
    fn advancing_quiet_cycles_matches_cycling_one_by_one() {
        for tac in 0..8u8 {
            for tima in [0x00, 0x7F, 0xFD, 0xFE, 0xFF] {
                for div in [0x0000, 0x0003, 0x00AB, 0x01FC, 0x3FF0, 0xFFF8, 0xFFFF] {
                    let mut timer = Timer::new();
                    timer.tac = tac;
                    timer.tima = tima;
                    timer.div = div;
                    timer.prev_and = timer.current_and();
                    let cycles = timer.quiet_cycles().min(20_000);
                    let mut stepped = timer.clone();
                    let mut ic = InterruptController::new();
                    for _ in 0..cycles {
                        stepped.cycle(&mut ic);
                    }
                    timer.advance(cycles);
                    let at = format!("tac {tac} tima {tima} div {div:04X}");
                    assert_eq!(format!("{timer:?}"), format!("{stepped:?}"), "{at}");
                    assert!(!stepped.overflow_pending, "{at}");
                    if timer.quiet_cycles() == 0 {
                        stepped.cycle(&mut ic);
                        assert!(stepped.overflow_pending || tac & 0b100 == 0, "{at}");
                    }
                }
            }
        }
    }
}
