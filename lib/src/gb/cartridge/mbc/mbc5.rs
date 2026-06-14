use crate::gb::cartridge::mbc::{MbcInterface, mask_bank_number};

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mbc5 {
    pub rom_bank_count: usize,
    pub ram_bank_count: usize,
    pub ram_enabled: bool,
    pub rom_bank_register_low: u8,
    pub rom_bank_register_high: u8,
    pub ram_bank_register: u8,
}

impl Mbc5 {
    pub fn new(rom_bank_count: usize, ram_bank_count: usize) -> Self {
        Self {
            rom_bank_count,
            ram_bank_count,
            ram_enabled: false,
            rom_bank_register_low: 1,
            rom_bank_register_high: 0,
            ram_bank_register: 0,
        }
    }
}

impl MbcInterface for Mbc5 {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => self.ram_enabled = value & 0x0F == 0x0A,
            0x2000..=0x2FFF => self.rom_bank_register_low = value,
            0x3000..=0x3FFF => self.rom_bank_register_high = value & 0x01,
            0x4000..=0x5FFF => self.ram_bank_register = value & 0x0F,
            _ => {}
        }

        false
    }

    fn on_read(&self, _addr: u16) -> Option<u8> {
        None
    }

    fn rom_bank_low(&self) -> usize {
        0
    }

    fn rom_bank_high(&self) -> usize {
        let bank =
            ((self.rom_bank_register_high as usize) << 8) | self.rom_bank_register_low as usize;
        mask_bank_number(bank, self.rom_bank_count)
    }

    fn ram_bank(&self) -> Option<usize> {
        if !self.ram_enabled {
            None
        } else {
            Some(mask_bank_number(
                self.ram_bank_register as usize,
                self.ram_bank_count,
            ))
        }
    }

    fn soft_reset(&mut self) {
        *self = Self::new(self.rom_bank_count, self.ram_bank_count);
    }
}
