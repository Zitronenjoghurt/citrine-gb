use std::collections::VecDeque;

pub const SMR3: u16 = 0xFF98;
pub const BRR3: u16 = 0xFF99;
pub const SCR3: u16 = 0xFF9A;
pub const TDR3: u16 = 0xFF9B;
pub const SSR3: u16 = 0xFF9C;
pub const RDR3: u16 = 0xFF9D;

const TIE: u8 = 0x80;
const RIE: u8 = 0x40;
const TE: u8 = 0x20;
const RE: u8 = 0x10;
const TEIE: u8 = 0x04;
const TDRE: u8 = 0x80;
const RDRF: u8 = 0x40;
const OER: u8 = 0x20;
const FER: u8 = 0x10;
const PER: u8 = 0x08;
const TEND: u8 = 0x04;

#[derive(Clone)]
pub struct Sci3 {
    smr: u8,
    brr: u8,
    scr: u8,
    ssr: u8,
    rdr: u8,
    tdr: u8,
    seen: u8,
    shifting: Option<(u8, u64)>,
    rx: VecDeque<(u64, u8)>,
    pub tx: Vec<(u64, u8)>,
    pub overruns: u64,
    pub dropped: u64,
    pub reads: u64,
}

impl Default for Sci3 {
    fn default() -> Self {
        Self {
            smr: 0,
            brr: 0xFF,
            scr: 0,
            ssr: TDRE | TEND,
            rdr: 0,
            tdr: 0xFF,
            seen: 0,
            shifting: None,
            rx: VecDeque::new(),
            tx: Vec::new(),
            overruns: 0,
            dropped: 0,
            reads: 0,
        }
    }
}

impl Sci3 {
    pub fn after_reset(&mut self) -> Self {
        Self {
            tx: std::mem::take(&mut self.tx),
            overruns: self.overruns,
            dropped: self.dropped,
            reads: self.reads,
            ..Self::default()
        }
    }

    pub fn byte_cycles(&self) -> u64 {
        let n = (self.smr & 3) as u64;
        10 * 32 * 4u64.pow(n as u32) * (self.brr as u64 + 1)
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            SMR3 => self.smr,
            BRR3 => self.brr,
            SCR3 => self.scr,
            TDR3 => self.tdr,
            SSR3 => {
                self.seen = self.ssr;
                self.ssr
            }
            RDR3 => {
                self.reads += 1;
                self.ssr &= !RDRF;
                self.rdr
            }
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, v: u8, now: u64) {
        match addr {
            SMR3 => self.smr = v,
            BRR3 => self.brr = v,
            SCR3 => {
                self.scr = v;
                if v & TE == 0 {
                    self.ssr |= TDRE;
                }
            }
            TDR3 => {
                self.tdr = v;
                self.ssr &= !(TDRE | TEND);
                self.start(now);
            }
            SSR3 => self.ssr &= v | !(self.seen & (TDRE | RDRF | OER | FER | PER | TEND)),
            _ => {}
        }
    }

    fn start(&mut self, now: u64) {
        if self.shifting.is_none() && self.ssr & TDRE == 0 && self.scr & TE != 0 {
            self.shifting = Some((self.tdr, now + self.byte_cycles()));
            self.ssr |= TDRE;
        }
    }

    pub fn receive(&mut self, bytes: &[u8], now: u64) {
        let step = self.byte_cycles();
        let mut at = self.rx.back().map_or(now, |&(t, _)| t.max(now)) + step;
        for &b in bytes {
            self.rx.push_back((at, b));
            at += step;
        }
    }

    pub fn rx_pending(&self) -> usize {
        self.rx.len()
    }

    pub fn next_event(&self) -> Option<u64> {
        let tx = self.shifting.map(|(_, t)| t);
        let rx = self.rx.front().map(|&(t, _)| t);
        match (tx, rx) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    pub fn tick(&mut self, now: u64) {
        while let Some((b, done)) = self.shifting {
            if done > now {
                break;
            }
            self.tx.push((done, b));
            self.shifting = None;
            if self.ssr & TDRE == 0 {
                self.start(done);
            } else {
                self.ssr |= TEND;
            }
        }
        while let Some(&(at, b)) = self.rx.front() {
            if at > now {
                break;
            }
            self.rx.pop_front();
            if self.scr & RE == 0 {
                self.dropped += 1;
                continue;
            }
            if self.ssr & RDRF != 0 {
                self.ssr |= OER;
                self.overruns += 1;
            } else {
                self.rdr = b;
                self.ssr |= RDRF;
            }
        }
    }

    pub fn interrupt(&self) -> bool {
        (self.scr & TIE != 0 && self.ssr & TDRE != 0)
            || (self.scr & RIE != 0 && self.ssr & (RDRF | OER | FER | PER) != 0)
            || (self.scr & TEIE != 0 && self.ssr & TEND != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Sci3 {
        let mut s = Sci3::default();
        s.write(BRR3, 0, 0);
        s.write(SCR3, TE | RE, 0);
        s
    }

    #[test]
    fn transmit_takes_ten_bit_times_at_115200() {
        let mut s = ready();
        assert_eq!(s.byte_cycles(), 320);
        s.write(TDR3, 0x56, 100);
        s.write(TDR3, 0x57, 101);
        assert_eq!(s.read(SSR3) & (TDRE | TEND), 0);
        s.tick(420);
        assert_eq!(s.tx, [(420, 0x56)]);
        assert_eq!(s.read(SSR3) & (TDRE | TEND), TDRE);
        s.tick(740);
        assert_eq!(s.tx[1], (740, 0x57));
        assert_eq!(s.read(SSR3) & TEND, TEND);
    }

    #[test]
    fn received_bytes_arrive_one_byte_time_apart_and_overrun() {
        let mut s = ready();
        s.receive(&[1, 2], 0);
        s.tick(320);
        assert_eq!(s.read(SSR3) & RDRF, RDRF);
        assert_eq!(s.read(RDR3), 1);
        s.tick(640);
        assert_eq!(s.read(RDR3), 2);
        s.receive(&[3, 4], 640);
        s.tick(1280);
        assert_eq!(s.read(SSR3) & OER, OER);
        assert_eq!(s.read(RDR3), 3);
    }

    #[test]
    fn writing_back_a_stale_status_keeps_a_byte_that_arrived_since() {
        let mut s = ready();
        s.receive(&[7], 0);
        let stale = s.read(SSR3);
        s.tick(320);
        s.write(SSR3, stale, 320);
        assert_eq!(s.read(SSR3) & RDRF, RDRF);
        assert_eq!(s.read(RDR3), 7);
    }
}
