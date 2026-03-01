use crate::gb::cartridge::mbc::{mask_rom_bank, MbcInterface};

#[derive(Debug)]
pub struct Mbc1 {
    pub ram_enabled: bool,
    pub rom_bank_count: usize,
    pub ram_bank_count: usize,
    // 5-bit register at 2000–3FFF
    pub rom_bank_register: u8,
    // 2-bit register at 4000–5FFF (RAM bank OR upper ROM bits)
    pub secondary_register: u8,
    // 1-bit register at 6000–7FFF: 0 = simple, 1 = advanced
    pub advanced_banking_mode: bool,
}

impl Mbc1 {
    pub fn new(rom_bank_count: usize, ram_bank_count: usize) -> Self {
        Self {
            ram_enabled: false,
            rom_bank_count,
            ram_bank_count,
            rom_bank_register: 0,
            secondary_register: 0,
            advanced_banking_mode: false,
        }
    }

    fn write_ram_enabled(&mut self, value: u8) {
        self.ram_enabled = value & 0x0F == 0x0A;
    }

    fn write_rom_bank_number(&mut self, value: u8) {
        self.rom_bank_register = value & 0b11111;
    }

    fn write_secondary_register(&mut self, value: u8) {
        self.secondary_register = value & 0b11;
    }

    fn write_advanced_banking_mode(&mut self, value: u8) {
        self.advanced_banking_mode = value & 1 == 1;
    }
}

impl MbcInterface for Mbc1 {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => self.write_ram_enabled(value),
            0x2000..=0x3FFF => self.write_rom_bank_number(value),
            0x4000..=0x5FFF => self.write_secondary_register(value),
            0x6000..=0x7FFF => self.write_advanced_banking_mode(value),
            _ => {}
        };

        false
    }

    fn on_read(&self, _addr: u16) -> Option<u8> {
        None
    }

    fn rom_bank_low(&self) -> usize {
        if !self.advanced_banking_mode {
            0
        } else {
            mask_rom_bank((self.secondary_register as usize) << 5, self.rom_bank_count)
        }
    }

    fn rom_bank_high(&self) -> usize {
        let rom_reg = if self.rom_bank_register == 0 {
            1
        } else {
            self.rom_bank_register as usize
        };
        let bank = (self.secondary_register as usize) << 5 | rom_reg;
        mask_rom_bank(bank, self.rom_bank_count)
    }

    fn ram_bank(&self) -> Option<usize> {
        if !self.ram_enabled {
            return None;
        }

        let bank = if !self.advanced_banking_mode || self.ram_bank_count <= 1 {
            0
        } else {
            (self.secondary_register as usize) & (self.ram_bank_count - 1)
        };

        Some(bank)
    }

    fn soft_reset(&mut self) {
        *self = Self::new(self.rom_bank_count, self.ram_bank_count);
    }
}
