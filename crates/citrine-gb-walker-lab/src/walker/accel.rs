#[derive(Clone)]
pub struct Accel {
    regs: [u8; 0x80],
    addr: Option<(u8, bool)>,
}

impl Default for Accel {
    fn default() -> Self {
        let mut regs = [0; 0x80];
        regs[0x00] = 0x02;
        regs[0x01] = 0x12;
        regs[0x14] = 0x0E;
        Self { regs, addr: None }
    }
}

impl Accel {
    pub fn set_sample(&mut self, xyz: [i16; 3]) {
        for (i, v) in xyz.iter().enumerate() {
            let v = (*v).clamp(-512, 511) as u16 & 0x3FF;
            self.regs[0x02 + i * 2] = ((v & 3) << 6) as u8 | 1;
            self.regs[0x03 + i * 2] = (v >> 2) as u8;
        }
    }

    pub fn deselect(&mut self) {
        self.addr = None;
    }

    pub fn regs(&self) -> &[u8] {
        &self.regs
    }

    pub fn asleep(&self) -> bool {
        self.regs[0x0A] & 1 != 0
    }

    pub fn transfer(&mut self, mosi: u8) -> u8 {
        match self.addr {
            None => {
                self.addr = Some((mosi & 0x7F, mosi & 0x80 != 0));
                0xFF
            }
            Some((a, true)) => {
                let v = self.regs[a as usize];
                if (0x02..=0x07).contains(&a) && a % 2 == 1 {
                    self.regs[a as usize - 1] &= !1;
                }
                self.addr = Some(((a + 1) & 0x7F, true));
                v
            }
            Some((a, false)) => {
                self.regs[a as usize] = mosi;
                0xFF
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_samples_in_bma150_layout() {
        let mut a = Accel::default();
        a.set_sample([-1, 256, 3]);
        let out: Vec<u8> = [0x82, 0, 0, 0, 0, 0, 0]
            .iter()
            .map(|&b| a.transfer(b))
            .collect();
        assert_eq!(&out[1..], &[0xC1, 0xFF, 0x01, 0x40, 0xC1, 0x00]);
    }

    #[test]
    fn writes_stay_on_one_register() {
        let mut a = Accel::default();
        a.transfer(0x0A);
        a.transfer(0x01);
        a.deselect();
        assert!(a.asleep());
        assert_eq!(a.transfer(0x80), 0xFF);
        assert_eq!(a.transfer(0), 0x02);
    }
}
