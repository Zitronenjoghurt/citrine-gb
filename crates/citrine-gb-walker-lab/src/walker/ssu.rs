pub const SSCRH: u16 = 0xF0E0;
pub const SSCRL: u16 = 0xF0E1;
pub const SSMR: u16 = 0xF0E2;
pub const SSER: u16 = 0xF0E3;
pub const SSSR: u16 = 0xF0E4;
pub const SSRDR: u16 = 0xF0E9;
pub const SSTDR: u16 = 0xF0EB;

const TE: u8 = 0x80;
const RE: u8 = 0x40;
const RSSTP: u8 = 0x20;
const RDRF: u8 = 0x02;
const TDRE: u8 = 0x04;
const TEND: u8 = 0x08;
const ORER: u8 = 0x40;

#[derive(Clone, Default)]
pub struct Ssu {
    sscrh: u8,
    sscrl: u8,
    ssmr: u8,
    sser: u8,
    sssr: u8,
    rdr: u8,
    tdr: u8,
    stopped: bool,
}

pub trait Spi {
    fn exchange(&mut self, mosi: u8) -> u8;
}

impl Ssu {
    pub fn read(&mut self, addr: u16, spi: &mut impl Spi) -> u8 {
        match addr {
            SSCRH => self.sscrh,
            SSCRL => self.sscrl,
            SSMR => self.ssmr,
            SSER => self.sser,
            SSSR => self.sssr,
            SSRDR => {
                let v = self.rdr;
                self.sssr &= !RDRF;
                if self.sser & (TE | RE) == RE && !self.stopped {
                    self.receive(spi.exchange(0xFF));
                    self.stopped = self.sser & RSSTP != 0;
                }
                v
            }
            SSTDR => self.tdr,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, v: u8, spi: &mut impl Spi) {
        match addr {
            SSCRH => self.sscrh = v,
            SSCRL => self.sscrl = v,
            SSMR => self.ssmr = v,
            SSER => {
                self.sser = v;
                if v & RSSTP == 0 {
                    self.stopped = false;
                }
                if v & TE != 0 {
                    self.sssr |= TDRE | TEND;
                }
            }
            SSSR => self.sssr &= v | !(ORER | TEND | TDRE | RDRF | 0x01),
            SSTDR => {
                self.tdr = v;
                if self.sser & TE != 0 {
                    let miso = spi.exchange(v);
                    if self.sser & RE != 0 {
                        self.receive(miso);
                    }
                    self.sssr |= TDRE | TEND;
                }
            }
            _ => {}
        }
    }

    fn receive(&mut self, v: u8) {
        if self.sssr & RDRF != 0 {
            self.sssr |= ORER;
        }
        self.rdr = v;
        self.sssr |= RDRF;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo(Vec<u8>);

    impl Spi for Echo {
        fn exchange(&mut self, mosi: u8) -> u8 {
            self.0.push(mosi);
            mosi.wrapping_add(1)
        }
    }

    #[test]
    fn transmit_exchanges_and_sets_flags() {
        let mut s = Ssu::default();
        let mut e = Echo(Vec::new());
        s.write(SSER, TE | RE, &mut e);
        s.write(SSTDR, 0x41, &mut e);
        assert_eq!(
            s.read(SSSR, &mut e) & (TDRE | TEND | RDRF),
            TDRE | TEND | RDRF
        );
        assert_eq!(s.read(SSRDR, &mut e), 0x42);
        assert_eq!(s.read(SSSR, &mut e) & RDRF, 0);
        assert_eq!(e.0, [0x41]);
    }

    #[test]
    fn receive_only_clocks_on_each_read_until_stopped() {
        let mut s = Ssu::default();
        let mut e = Echo(Vec::new());
        s.write(SSER, RE, &mut e);
        s.read(SSRDR, &mut e);
        assert_eq!(s.read(SSSR, &mut e) & RDRF, RDRF);
        s.write(SSER, RE | RSSTP, &mut e);
        assert_eq!(s.read(SSRDR, &mut e), 0x00);
        assert_eq!(e.0.len(), 2);
        s.read(SSRDR, &mut e);
        assert_eq!(e.0.len(), 2);
    }
}
