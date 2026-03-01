use crate::gb::cartridge::mbc::{mask_rom_bank, MbcInterface};

#[derive(Debug)]
pub struct Mbc2 {
    pub ram_enabled: bool,
    // 512 half-bytes of RAM (only the lower nibble of each byte is used)
    pub ram: Box<[u8; 512]>,
    pub rom_bank_count: usize,
    pub rom_bank_register: u8,
}

impl Mbc2 {
    pub fn new(rom_bank_count: usize) -> Self {
        Self {
            ram_enabled: false,
            ram: Box::new([0; 512]),
            rom_bank_count,
            rom_bank_register: 0,
        }
    }

    fn write_control(&mut self, address: u16, value: u8) {
        let ram_control = (address & 0x100) == 0;

        if ram_control {
            self.ram_enabled = value & 0x0F == 0x0A;
        } else {
            self.rom_bank_register = value & 0x0F;
        }
    }
}

impl MbcInterface for Mbc2 {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        if (0x0000..=0x3FFF).contains(&addr) {
            self.write_control(addr, value);
            return false;
        }

        if !self.ram_enabled || !(0xA000..=0xBFFF).contains(&addr) {
            return false;
        }

        let index = ((addr - 0xA000) % 0x0200) as usize;
        self.ram[index] = value | 0xF0;

        true
    }

    fn on_read(&self, addr: u16) -> Option<u8> {
        if !self.ram_enabled || !(0xA000..=0xBFFF).contains(&addr) {
            return None;
        }

        let index = ((addr - 0xA000) % 0x0200) as usize;
        Some(self.ram[index] | 0xF0)
    }

    fn rom_bank_low(&self) -> usize {
        0
    }

    fn rom_bank_high(&self) -> usize {
        let bank = if self.rom_bank_register == 0 {
            1
        } else {
            self.rom_bank_register as usize
        };
        mask_rom_bank(bank, self.rom_bank_count)
    }

    fn ram_bank(&self) -> Option<usize> {
        None
    }

    fn soft_reset(&mut self) {
        *self = Self::new(self.rom_bank_count);
    }
}
