use crate::gb::cartridge::mbc::MbcInterface;
use crate::gb::cartridge::{RAM_BANK_SIZE, ROM_BANK_SIZE};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mmm01 {
    pub rom_bank_count: usize,
    pub ram_bank_count: usize,
    pub mapped: bool,
    pub ram_enabled: bool,
    pub ram_mask: u8,
    pub rom_low: u8,
    pub rom_mid: u8,
    pub rom_high: u8,
    pub ram_low: u8,
    pub ram_high: u8,
    pub mode: bool,
    pub mode_locked: bool,
    pub rom_mask: u8,
    pub multiplex: bool,
}

impl Mmm01 {
    pub fn new(rom_bank_count: usize, ram_bank_count: usize) -> Self {
        Self {
            rom_bank_count,
            ram_bank_count,
            mapped: false,
            ram_enabled: false,
            ram_mask: 0,
            rom_low: 0,
            rom_mid: 0,
            rom_high: 0,
            ram_low: 0,
            ram_high: 0,
            mode: false,
            mode_locked: false,
            rom_mask: 0,
            multiplex: false,
        }
    }

    fn rom_bank(&self, upper: bool) -> usize {
        let low = if upper {
            let low = self.rom_low;
            if low & !self.rom_mask & 0x1F == 0 {
                low | 0x01
            } else {
                low
            }
        } else {
            self.rom_low & self.rom_mask
        };
        let mid = if self.multiplex {
            if upper || self.mode {
                self.ram_low
            } else {
                self.ram_low & self.ram_mask
            }
        } else {
            self.rom_mid
        };
        ((self.rom_high as usize) << 7) | ((mid as usize) << 5) | low as usize
    }

    fn ram_bank_number(&self) -> usize {
        let low = if self.multiplex {
            self.rom_mid
        } else if self.mode {
            self.ram_low
        } else {
            self.ram_low & self.ram_mask
        };
        ((self.ram_high as usize) << 2) | low as usize
    }
}

impl MbcInterface for Mmm01 {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => {
                self.ram_enabled = value & 0x0F == 0x0A;
                if !self.mapped {
                    self.ram_mask = (value >> 4) & 0x03;
                    self.mapped = value & 0x40 != 0;
                }
            }
            0x2000..=0x3FFF => {
                self.rom_low = (self.rom_low & self.rom_mask) | (value & 0x1F & !self.rom_mask);
                if !self.mapped {
                    self.rom_mid = (value >> 5) & 0x03;
                }
            }
            0x4000..=0x5FFF => {
                self.ram_low = (self.ram_low & self.ram_mask) | (value & 0x03 & !self.ram_mask);
                if !self.mapped {
                    self.ram_high = (value >> 2) & 0x03;
                    self.rom_high = (value >> 4) & 0x03;
                    self.mode_locked = value & 0x40 != 0;
                }
            }
            0x6000..=0x7FFF => {
                if !self.mode_locked {
                    self.mode = value & 0x01 != 0;
                }
                if !self.mapped {
                    self.rom_mask = (value >> 1) & 0x1E;
                    self.multiplex = value & 0x40 != 0;
                }
            }
            _ => {}
        }
        false
    }

    fn on_read(&self, _addr: u16) -> Option<u8> {
        None
    }

    fn rom_bank_low(&self) -> usize {
        if self.mapped {
            self.rom_bank(false) % self.rom_bank_count.max(1)
        } else {
            self.rom_bank_count.saturating_sub(2)
        }
    }

    fn rom_bank_high(&self) -> usize {
        if self.mapped {
            self.rom_bank(true) % self.rom_bank_count.max(1)
        } else {
            self.rom_bank_count.saturating_sub(1)
        }
    }

    fn ram_bank(&self) -> Option<usize> {
        if self.ram_enabled && self.ram_bank_count > 0 {
            Some(self.ram_bank_number() % self.ram_bank_count)
        } else {
            None
        }
    }

    fn rom_offset(&self, addr: u16) -> usize {
        match addr {
            0x0000..=0x3FFF => self.rom_bank_low() * ROM_BANK_SIZE + addr as usize,
            _ => self.rom_bank_high() * ROM_BANK_SIZE + (addr as usize & 0x3FFF),
        }
    }

    fn ram_offset(&self, addr: u16) -> Option<usize> {
        self.ram_bank()
            .map(|bank| bank * RAM_BANK_SIZE + (addr as usize & 0x1FFF))
    }

    fn soft_reset(&mut self) {
        *self = Self::new(self.rom_bank_count, self.ram_bank_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boots_into_the_last_32k() {
        let mbc = Mmm01::new(64, 0);
        assert_eq!(mbc.rom_bank_low(), 62);
        assert_eq!(mbc.rom_bank_high(), 63);
    }

    #[test]
    fn the_menu_maps_a_game_slice() {
        let mut mbc = Mmm01::new(512, 0);
        mbc.on_write(0x2000, 0x40 | 0x02);
        mbc.on_write(0x6000, 0x1C << 1);
        mbc.on_write(0x0000, 0x40);
        assert!(mbc.mapped);
        assert_eq!(mbc.rom_bank_low(), 0x40);
        assert_eq!(mbc.rom_bank_high(), 0x40 + 0x02);
    }

    #[test]
    fn masked_rom_bits_stay_fixed_after_mapping() {
        let mut mbc = Mmm01::new(64, 0);
        mbc.on_write(0x2000, 0x10);
        mbc.on_write(0x6000, 0x18 << 1);
        mbc.on_write(0x0000, 0x40);
        assert_eq!(mbc.rom_bank_high(), 0x11);
        mbc.on_write(0x2000, 0x07);
        assert_eq!(mbc.rom_bank_high(), 0x17);
        assert_eq!(mbc.rom_bank_low(), 0x10);
        mbc.on_write(0x2000, 0x00);
        assert_eq!(mbc.rom_bank_high(), 0x11);
    }

    #[test]
    fn extended_bits_lock_once_mapped() {
        let mut mbc = Mmm01::new(512, 0);
        mbc.on_write(0x4000, 0x10);
        mbc.on_write(0x0000, 0x40);
        mbc.on_write(0x4000, 0x20);
        mbc.on_write(0x2000, 0x60);
        assert_eq!(mbc.rom_high, 0x01);
        assert_eq!(mbc.rom_mid, 0x00);
        assert_eq!(mbc.rom_bank_high(), 0x80 | 0x01);
    }

    #[test]
    fn mode_write_lock_holds_the_mode() {
        let mut mbc = Mmm01::new(64, 4);
        mbc.on_write(0x4000, 0x40);
        mbc.on_write(0x6000, 0x01);
        assert!(!mbc.mode);
    }

    #[test]
    fn ram_banks_use_the_game_select_bits() {
        let mut mbc = Mmm01::new(64, 16);
        mbc.on_write(0x4000, 0x08 | 0x01);
        mbc.on_write(0x0000, 0x4A);
        mbc.on_write(0x6000, 0x01);
        assert_eq!(mbc.ram_bank(), Some(0x09));
    }
}
