use super::CLOCK_HZ;

pub const TMWD: u16 = 0xFFB0;
pub const TCSRWD1: u16 = 0xFFB1;
pub const TCSRWD2: u16 = 0xFFB2;
pub const TCWD: u16 = 0xFFB3;

const ROSC_HZ: u64 = 1_000_000;
const SUBCLOCK_HZ: u64 = 32_768;

const TCWE: u8 = 0x40;
const TCSRWE: u8 = 0x10;
const WDON: u8 = 0x04;
const WRST: u8 = 0x01;
const OVF: u8 = 0x80;
const WT_IT: u8 = 0x20;
const IEOVF: u8 = 0x08;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    Reset,
    Interrupt,
}

#[derive(Clone)]
pub struct Wdt {
    cks: u8,
    csr1: u8,
    csr2: u8,
    count: u8,
    acc: u64,
}

impl Default for Wdt {
    fn default() -> Self {
        Self {
            cks: 0,
            csr1: WDON,
            csr2: 0,
            count: 0,
            acc: 0,
        }
    }
}

impl Wdt {
    pub fn after_reset(&self) -> Self {
        Self {
            csr1: WDON | (self.csr1 & WRST),
            ..Self::default()
        }
    }

    fn period(&self) -> Option<u64> {
        match self.cks {
            0..=3 => Some(2048 * CLOCK_HZ / ROSC_HZ),
            4 => Some(16 * CLOCK_HZ / SUBCLOCK_HZ),
            5 => Some(256 * CLOCK_HZ / SUBCLOCK_HZ),
            6 | 7 => None,
            n => Some(64 << (n - 8)),
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            TMWD => 0xF0 | self.cks,
            TCSRWD1 => 0xAA | self.csr1,
            TCSRWD2 => 0x57 | self.csr2,
            _ => self.count,
        }
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        let unlocked = self.csr1 & TCSRWE != 0;
        match addr {
            TMWD => self.cks = v & 0x0F,
            TCSRWD1 => {
                let mut c = self.csr1;
                if v & 0x80 == 0 {
                    c = (c & !TCWE) | (v & TCWE);
                }
                if v & 0x20 == 0 {
                    c = (c & !TCSRWE) | (v & TCSRWE);
                }
                if unlocked && v & 0x08 == 0 {
                    c = (c & !WDON) | (v & WDON);
                }
                if unlocked && v & 0x02 == 0 {
                    c = (c & !WRST) | (v & WRST);
                }
                self.csr1 = c;
            }
            TCSRWD2 => {
                let mut c = self.csr2 & (v | !OVF);
                if unlocked && v & 0x40 == 0 {
                    c = (c & !WT_IT) | (v & WT_IT);
                }
                if unlocked && v & 0x10 == 0 {
                    c = (c & !IEOVF) | (v & IEOVF);
                }
                self.csr2 = c;
            }
            _ => {
                if self.csr1 & TCWE != 0 {
                    self.count = v;
                    self.acc = 0;
                }
            }
        }
    }

    pub fn tick(&mut self, cycles: u64) -> Option<Overflow> {
        if self.csr1 & WDON == 0 {
            return None;
        }
        let p = self.period()?;
        self.acc += cycles;
        let mut out = None;
        while self.acc >= p {
            self.acc -= p;
            let (c, over) = self.count.overflowing_add(1);
            self.count = c;
            if over {
                if self.csr2 & WT_IT != 0 {
                    self.csr2 |= OVF;
                    if self.csr2 & IEOVF != 0 {
                        out = Some(Overflow::Interrupt);
                    }
                } else {
                    self.csr1 |= WRST;
                    return Some(Overflow::Reset);
                }
            }
        }
        out
    }

    pub fn interrupt(&self) -> bool {
        self.csr2 & (WT_IT | OVF | IEOVF) == WT_IT | OVF | IEOVF
    }

    pub fn cycles_to_overflow(&self) -> Option<u64> {
        if self.csr1 & WDON == 0 {
            return None;
        }
        Some((256 - self.count as u64) * self.period()? - self.acc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn armed_for_two_seconds() -> Wdt {
        let mut w = Wdt::default();
        w.write(TMWD, 0xF5);
        w.write(TCSRWD1, 0x5E);
        w.write(TCWD, 0x00);
        w.write(TCSRWD1, 0x9E);
        w
    }

    #[test]
    fn runs_from_reset_on_the_on_chip_oscillator() {
        let w = Wdt::default();
        assert_eq!(w.read(TCSRWD1), 0xAE);
        assert_eq!(w.cycles_to_overflow(), Some(256 * 7549));
    }

    #[test]
    fn phi_w_256_from_zero_overflows_after_two_seconds_and_resets() {
        let mut w = armed_for_two_seconds();
        assert_eq!(w.cycles_to_overflow(), Some(2 * CLOCK_HZ));
        assert_eq!(w.tick(2 * CLOCK_HZ - 1), None);
        assert_eq!(w.tick(1), Some(Overflow::Reset));
        let after = w.after_reset();
        assert_eq!(after.read(TCSRWD1) & (WRST | WDON), WRST | WDON);
        assert_eq!(after.read(TMWD), 0xF0);
    }

    #[test]
    fn locked_bits_ignore_writes() {
        let mut w = Wdt::default();
        w.write(TCSRWD1, 0xA2);
        assert_eq!(w.read(TCSRWD1) & WDON, WDON);
        w.write(TCWD, 0x80);
        assert_eq!(w.read(TCWD), 0);
        w.write(TCSRWD1, 0x9E);
        w.write(TCSRWD1, 0xA2);
        assert_eq!(w.read(TCSRWD1) & WDON, 0);
        assert_eq!(w.cycles_to_overflow(), None);
    }

    #[test]
    fn interval_mode_raises_an_interrupt_instead_of_a_reset() {
        let mut w = armed_for_two_seconds();
        w.write(TCSRWD2, 0x28);
        assert_eq!(w.tick(2 * CLOCK_HZ), Some(Overflow::Interrupt));
        assert!(w.interrupt());
        let seen = w.read(TCSRWD2);
        w.write(TCSRWD2, seen & !OVF);
        assert!(!w.interrupt());
    }
}
