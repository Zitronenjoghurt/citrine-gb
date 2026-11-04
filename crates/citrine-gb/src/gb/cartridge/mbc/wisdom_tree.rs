use crate::gb::cartridge::mbc::MbcInterface;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WisdomTree {
    pub bank_count: usize,
    pub bank: u8,
}

impl WisdomTree {
    pub fn new(rom_bank_count: usize) -> Self {
        Self {
            bank_count: (rom_bank_count / 2).max(1),
            bank: 0,
        }
    }

    fn selected(&self) -> usize {
        self.bank as usize % self.bank_count
    }
}

impl MbcInterface for WisdomTree {
    fn on_write(&mut self, addr: u16, _value: u8) -> bool {
        if addr < 0x8000 {
            self.bank = addr as u8;
        }
        false
    }

    fn on_read(&self, _addr: u16) -> Option<u8> {
        None
    }

    fn rom_bank_low(&self) -> usize {
        self.selected() * 2
    }

    fn rom_bank_high(&self) -> usize {
        self.selected() * 2 + 1
    }

    fn ram_bank(&self) -> Option<usize> {
        None
    }

    fn soft_reset(&mut self) {
        self.bank = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_low_address_byte_picks_a_32k_bank() {
        let mut mbc = WisdomTree::new(16);
        mbc.on_write(0x0003, 0xFF);
        assert_eq!((mbc.rom_bank_low(), mbc.rom_bank_high()), (6, 7));
        mbc.on_write(0x7F01, 0x00);
        assert_eq!((mbc.rom_bank_low(), mbc.rom_bank_high()), (2, 3));
        mbc.on_write(0xA005, 0x00);
        assert_eq!(mbc.rom_bank_low(), 2);
    }
}
