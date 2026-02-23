use crate::gb::ic::ICInterface;
use crate::{ReadMemory, WriteMemory};

#[derive(Debug)]
pub struct Timer {
    pub div: u16,
    pub tima: u8,
    pub tma: u8,
    pub tac: u8,
    prev_and: bool,
    overflow_pending: bool,
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
        }
    }
}

impl Timer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cycle(&mut self, ic: &mut impl ICInterface) {
        if self.overflow_pending {
            self.overflow_pending = false;
            self.tima = self.tma;
            ic.request_interrupt(crate::gb::ic::Interrupt::Timer);
        }

        self.div = self.div.wrapping_add(4);
        self.check_falling_edge();
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
        self.timer_enabled() && ((self.div & self.tima_bit()) != 0)
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
            0xFF07 => self.tac,
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
                self.overflow_pending = false;
                self.tima = value;
            }
            0xFF06 => self.tma = value,
            0xFF07 => {
                self.tac = value;
                self.check_falling_edge();
            }
            _ => {}
        }
    }
}
