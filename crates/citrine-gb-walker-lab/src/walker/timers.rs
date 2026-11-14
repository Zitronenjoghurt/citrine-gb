pub const TMB1: u16 = 0xF0D0;
pub const TCB1: u16 = 0xF0D1;
pub const TMRW: u16 = 0xF0F0;
pub const TCRW: u16 = 0xF0F1;
pub const TIERW: u16 = 0xF0F2;
pub const TSRW: u16 = 0xF0F3;
pub const TCNT: u16 = 0xF0F6;
pub const GRA: u16 = 0xF0F8;
pub const GRD: u16 = 0xF0FF;

pub const SUBCLOCK_CYCLES: u64 = 112;

#[derive(Clone, Default)]
pub struct TimerB1 {
    mode: u8,
    count: u8,
    reload: u8,
    acc: u64,
    pub overflowed: bool,
}

impl TimerB1 {
    fn period(&self) -> u64 {
        match self.mode & 7 {
            0 => 8192,
            1 => 2048,
            2 => 256,
            3 => 64,
            4 => 16,
            5 => 4,
            6 => 115_200,
            _ => 28_800,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            TMB1 => self.mode | 0x38,
            _ => self.count,
        }
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        match addr {
            TMB1 => self.mode = v & 0xC7,
            _ => {
                self.reload = v;
                self.count = v;
            }
        }
    }

    pub fn tick(&mut self, cycles: u64) {
        if self.mode & 0x40 == 0 {
            return;
        }
        self.acc += cycles;
        let p = self.period();
        while self.acc >= p {
            self.acc -= p;
            let (c, over) = self.count.overflowing_add(1);
            self.count = c;
            if over {
                self.overflowed = true;
                if self.mode & 0x80 != 0 {
                    self.count = self.reload;
                }
            }
        }
    }

    pub fn cycles_to_overflow(&self) -> Option<u64> {
        if self.mode & 0x40 == 0 {
            return None;
        }
        Some((256 - self.count as u64) * self.period() - self.acc)
    }
}

#[derive(Clone, Default)]
pub struct TimerW {
    regs: [u8; 16],
    acc: u64,
}

fn at(addr: u16) -> usize {
    (addr - TMRW) as usize
}

impl TimerW {
    fn word(&self, i: usize) -> u16 {
        u16::from_be_bytes([self.regs[i], self.regs[i + 1]])
    }

    fn set_word(&mut self, i: usize, v: u16) {
        self.regs[i..i + 2].copy_from_slice(&v.to_be_bytes());
    }

    fn period(&self) -> Option<u64> {
        Some(match (self.regs[at(TCRW)] >> 4) & 7 {
            0 => 1,
            1 => 2,
            2 => 4,
            3 => 8,
            4 => SUBCLOCK_CYCLES,
            5 => SUBCLOCK_CYCLES * 4,
            6 => SUBCLOCK_CYCLES * 16,
            _ => return None,
        })
    }

    pub fn read(&self, addr: u16) -> u8 {
        self.regs[at(addr)]
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        let i = at(addr);
        if addr == TSRW {
            self.regs[i] &= v | 0x70;
        } else {
            self.regs[i] = v;
        }
    }

    pub fn running(&self) -> bool {
        self.regs[at(TMRW)] & 0x80 != 0
    }

    pub fn tick(&mut self, cycles: u64) {
        if !self.running() {
            return;
        }
        let Some(p) = self.period() else { return };
        self.acc += cycles;
        let n = self.acc / p;
        self.acc %= p;
        if n == 0 {
            return;
        }
        let gra = self.word(at(GRA));
        let clear = self.regs[at(TCRW)] & 0x80 != 0;
        let mut tcnt = self.word(at(TCNT));
        for _ in 0..n {
            if clear && tcnt == gra {
                tcnt = 0;
            } else {
                let (t, over) = tcnt.overflowing_add(1);
                tcnt = t;
                if over {
                    self.regs[at(TSRW)] |= 0x80;
                }
            }
            for (k, bit) in [(GRA, 1u8), (GRA + 2, 2), (GRA + 4, 4), (GRA + 6, 8)] {
                if self.word(at(k)) == tcnt {
                    self.regs[at(TSRW)] |= bit;
                }
            }
        }
        self.set_word(at(TCNT), tcnt);
    }

    pub fn interrupt(&self) -> bool {
        self.regs[at(TSRW)] & self.regs[at(TIERW)] & 0x8F != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_b1_reloads_at_sixteen_hertz_from_the_subclock() {
        let mut t = TimerB1::default();
        t.write(TCB1, 0xF8);
        t.write(TMB1, 0xC7);
        assert_eq!(t.cycles_to_overflow(), Some(8 * 28_800));
        t.tick(8 * 28_800 - 1);
        assert!(!t.overflowed);
        t.tick(1);
        assert!(t.overflowed);
        assert_eq!(t.read(TCB1), 0xF8);
    }

    #[test]
    fn timer_w_compare_match_clears_on_gra() {
        let mut t = TimerW::default();
        t.write(TCRW, 0x80);
        t.write(GRA, 0x00);
        t.write(GRA + 1, 99);
        t.write(TIERW, 0x01);
        t.write(TMRW, 0x80);
        t.tick(98);
        assert!(!t.interrupt());
        t.tick(1);
        assert!(t.interrupt());
        t.tick(1);
        assert_eq!(t.read(TCNT + 1), 0);
        t.write(TSRW, 0x00);
        assert!(!t.interrupt());
    }
}
