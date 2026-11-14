pub const RTCFLG: u16 = 0xF067;
pub const RSECDR: u16 = 0xF068;
pub const RMINDR: u16 = 0xF069;
pub const RHRDR: u16 = 0xF06A;
pub const RWKDR: u16 = 0xF06B;
pub const RTCCR1: u16 = 0xF06C;
pub const RTCCR2: u16 = 0xF06D;
pub const RTCCSR: u16 = 0xF06F;

pub const QUARTER_CYCLES: u64 = 3_686_400 / 4;

#[derive(Clone, Default)]
pub struct Rtc {
    pub sec: u8,
    pub min: u8,
    pub hour: u8,
    pub week: u8,
    cr1: u8,
    cr2: u8,
    csr: u8,
    flags: u8,
    quarter: u8,
    acc: u64,
}

fn bcd(v: u8) -> u8 {
    ((v / 10) << 4) | (v % 10)
}

fn unbcd(v: u8) -> u8 {
    (v >> 4) * 10 + (v & 0xF)
}

impl Rtc {
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            RTCFLG => self.flags,
            RSECDR => bcd(self.sec),
            RMINDR => bcd(self.min),
            RHRDR => bcd(self.hour),
            RWKDR => self.week,
            RTCCR1 => self.cr1,
            RTCCR2 => self.cr2,
            RTCCSR => self.csr,
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        match addr {
            RTCFLG => self.flags &= v,
            RSECDR => self.sec = unbcd(v & 0x7F),
            RMINDR => self.min = unbcd(v & 0x7F),
            RHRDR => self.hour = unbcd(v & 0x3F),
            RWKDR => self.week = v & 7,
            RTCCR1 => {
                self.cr1 = v & 0xF8;
                if v & 0x10 != 0 {
                    self.sec = 0;
                    self.min = 0;
                    self.hour = 0;
                    self.week = 0;
                    self.cr2 = 0;
                    self.flags = 0;
                    self.quarter = 0;
                    self.acc = 0;
                }
            }
            RTCCR2 => self.cr2 = v,
            RTCCSR => self.csr = v,
            _ => {}
        }
    }

    pub fn running(&self) -> bool {
        self.cr1 & 0x80 != 0
    }

    pub fn tick(&mut self, cycles: u64) {
        if !self.running() {
            return;
        }
        self.acc += cycles;
        while self.acc >= QUARTER_CYCLES {
            self.acc -= QUARTER_CYCLES;
            self.quarter_tick();
        }
    }

    fn raise(&mut self, bit: u8) {
        if self.cr2 & bit != 0 {
            self.flags |= bit;
        }
    }

    fn quarter_tick(&mut self) {
        self.quarter = (self.quarter + 1) % 4;
        self.raise(0x01);
        if self.quarter.is_multiple_of(2) {
            self.raise(0x02);
        }
        if self.quarter != 0 {
            return;
        }
        self.raise(0x04);
        self.sec += 1;
        if self.sec < 60 {
            return;
        }
        self.sec = 0;
        self.raise(0x08);
        self.min += 1;
        if self.min < 60 {
            return;
        }
        self.min = 0;
        self.raise(0x10);
        self.hour += 1;
        let hours = if self.cr1 & 0x40 != 0 { 24 } else { 12 };
        if self.hour < hours {
            return;
        }
        self.hour = 0;
        if hours == 12 {
            self.cr1 ^= 0x20;
            if self.cr1 & 0x20 != 0 {
                return;
            }
        }
        self.raise(0x20);
        self.week = (self.week + 1) % 7;
        if self.week == 0 {
            self.raise(0x40);
        }
    }

    pub fn cycles_to_tick(&self) -> Option<u64> {
        self.running().then(|| QUARTER_CYCLES - self.acc)
    }

    pub fn pending(&self) -> Option<u8> {
        let p = self.flags & self.cr2;
        (p != 0).then(|| p.trailing_zeros() as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_bcd_seconds_and_flags_enabled_periods() {
        let mut r = Rtc::default();
        r.write(RTCCR1, 0xC0);
        r.write(RTCCR2, 0x04);
        r.write(RSECDR, 0x59);
        r.tick(QUARTER_CYCLES * 3);
        assert_eq!(r.pending(), None);
        r.tick(QUARTER_CYCLES);
        assert_eq!(r.read(RSECDR), 0x00);
        assert_eq!(r.read(RMINDR), 0x01);
        assert_eq!(r.pending(), Some(2));
        r.write(RTCFLG, !0x04);
        assert_eq!(r.pending(), None);
    }
}
