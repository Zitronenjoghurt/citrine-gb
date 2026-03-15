use crate::gb::cartridge::mbc::{mask_bank_number, MbcInterface};

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mbc3 {
    pub has_rtc: bool,
    pub ram_rtc_enabled: bool,
    pub rom_bank_count: usize,
    pub ram_bank_count: usize,
    pub rom_bank_register: u8,
    pub ram_rtc_select: u8,
    pub latched_zero: bool,
    pub latched_one: bool,
}

impl Mbc3 {
    pub fn new(rom_bank_count: usize, ram_bank_count: usize, has_rtc: bool) -> Self {
        Self {
            has_rtc,
            ram_rtc_enabled: false,
            rom_bank_count,
            ram_bank_count,
            rom_bank_register: 0,
            ram_rtc_select: 0,
            latched_zero: false,
            latched_one: false,
        }
    }

    fn write_ram_rtc_enabled(&mut self, value: u8) {
        self.ram_rtc_enabled = value & 0x0F == 0x0A;
    }

    fn write_rom_bank_number(&mut self, value: u8) {
        self.rom_bank_register = value & 0x7F;
    }

    fn write_ram_rtc_select(&mut self, value: u8) {
        self.ram_rtc_select = value;
    }

    fn write_clock_latch(&mut self, value: u8) {
        if !self.has_rtc {
            return;
        };

        if value == 0 {
            self.latched_zero = true;
            self.latched_one = false;
        } else if value == 1 && self.latched_zero {
            self.latched_one = true;
        } else {
            self.latched_zero = false;
            self.latched_one = false;
        }
    }
}

impl MbcInterface for Mbc3 {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => self.write_ram_rtc_enabled(value),
            0x2000..=0x3FFF => self.write_rom_bank_number(value),
            0x4000..=0x5FFF => self.write_ram_rtc_select(value),
            0x6000..=0x7FFF => self.write_clock_latch(value),
            _ => {}
        }

        false
    }

    fn on_read(&self, _addr: u16) -> Option<u8> {
        // ToDo: RTC
        None
    }

    fn rom_bank_low(&self) -> usize {
        0
    }

    fn rom_bank_high(&self) -> usize {
        mask_bank_number(self.rom_bank_register as usize, self.rom_bank_count)
    }

    fn ram_bank(&self) -> Option<usize> {
        if !self.ram_rtc_enabled || self.ram_rtc_select > 7 {
            None
        } else {
            Some(mask_bank_number(
                self.ram_rtc_select as usize,
                self.ram_bank_count,
            ))
        }
    }

    fn soft_reset(&mut self) {
        *self = Self::new(self.rom_bank_count, self.ram_bank_count, self.has_rtc);
    }
}
