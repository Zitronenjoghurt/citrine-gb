use crate::gb::cartridge::mbc::{MbcInterface, mask_bank_number};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Huc1 {
    pub rom_bank_count: usize,
    pub ram_bank_count: usize,
    pub rom_bank_register: u8,
    pub ram_bank_register: u8,
    pub ir_mode: bool,
    pub led: bool,
    pub light: bool,
}

impl Huc1 {
    pub fn new(rom_bank_count: usize, ram_bank_count: usize) -> Self {
        Self {
            rom_bank_count,
            ram_bank_count,
            rom_bank_register: 1,
            ram_bank_register: 0,
            ir_mode: false,
            led: false,
            light: false,
        }
    }
}

impl MbcInterface for Huc1 {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => self.ir_mode = value & 0x0F == 0x0E,
            0x2000..=0x3FFF => self.rom_bank_register = value & 0x3F,
            0x4000..=0x5FFF => self.ram_bank_register = value & 0x03,
            0xA000..=0xBFFF if self.ir_mode => self.led = value & 0x01 != 0,
            _ => {}
        }
        false
    }

    fn on_read(&self, addr: u16) -> Option<u8> {
        if self.ir_mode && (0xA000..=0xBFFF).contains(&addr) {
            Some(0xC0 | u8::from(self.light))
        } else {
            None
        }
    }

    fn rom_bank_low(&self) -> usize {
        0
    }

    fn rom_bank_high(&self) -> usize {
        mask_bank_number(self.rom_bank_register as usize, self.rom_bank_count)
    }

    fn ram_bank(&self) -> Option<usize> {
        if self.ir_mode {
            None
        } else {
            Some(mask_bank_number(
                self.ram_bank_register as usize,
                self.ram_bank_count.max(1),
            ))
        }
    }

    fn soft_reset(&mut self) {
        let light = self.light;
        *self = Self::new(self.rom_bank_count, self.ram_bank_count);
        self.light = light;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_is_always_on_outside_ir_mode() {
        let mut mbc = Huc1::new(64, 4);
        assert_eq!(mbc.ram_bank(), Some(0));
        mbc.on_write(0x0000, 0x00);
        mbc.on_write(0x4000, 0x03);
        assert_eq!(mbc.ram_bank(), Some(3));
    }

    #[test]
    fn rom_bank_takes_six_bits() {
        let mut mbc = Huc1::new(64, 4);
        assert_eq!(mbc.rom_bank_high(), 1);
        mbc.on_write(0x2000, 0xFF);
        assert_eq!(mbc.rom_bank_high(), 0x3F);
    }

    #[test]
    fn ir_mode_reads_the_light_and_drives_the_led() {
        let mut mbc = Huc1::new(64, 4);
        mbc.on_write(0x0000, 0x0E);
        assert_eq!(mbc.ram_bank(), None);
        assert_eq!(mbc.on_read(0xA000), Some(0xC0));
        mbc.light = true;
        assert_eq!(mbc.on_read(0xB123), Some(0xC1));
        mbc.on_write(0xA000, 0x01);
        assert!(mbc.led);
        mbc.on_write(0xA000, 0x00);
        assert!(!mbc.led);
        mbc.on_write(0x0000, 0x0A);
        assert_eq!(mbc.on_read(0xA000), None);
    }
}
