use crate::gb::cartridge::mbc::{MbcInterface, mask_bank_number};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mbc5 {
    pub rom_bank_count: usize,
    pub ram_bank_count: usize,
    pub ram_enabled: bool,
    pub rom_bank_register_low: u8,
    pub rom_bank_register_high: u8,
    pub ram_bank_register: u8,
    #[cfg_attr(feature = "serde", serde(default))]
    pub rumble: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub motor: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub motor_cycles: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub cycles: u32,
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
            rumble: false,
            motor: false,
            motor_cycles: 0,
            cycles: 0,
        }
    }

    pub fn with_rumble(rom_bank_count: usize, ram_bank_count: usize) -> Self {
        Self {
            rumble: true,
            ..Self::new(rom_bank_count, ram_bank_count)
        }
    }

    pub fn cycle(&mut self) {
        if self.rumble {
            self.cycles = self.cycles.saturating_add(1);
            if self.motor {
                self.motor_cycles = self.motor_cycles.saturating_add(1);
            }
        }
    }

    pub fn take_rumble(&mut self) -> f32 {
        let level = if self.cycles == 0 {
            if self.motor { 1.0 } else { 0.0 }
        } else {
            self.motor_cycles as f32 / self.cycles as f32
        };
        self.motor_cycles = 0;
        self.cycles = 0;
        level
    }

    fn write_ram_bank(&mut self, value: u8) {
        if self.rumble {
            self.motor = value & 0x08 != 0;
            self.ram_bank_register = value & 0x07;
        } else {
            self.ram_bank_register = value & 0x0F;
        }
    }
}

impl MbcInterface for Mbc5 {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => self.ram_enabled = value & 0x0F == 0x0A,
            0x2000..=0x2FFF => self.rom_bank_register_low = value,
            0x3000..=0x3FFF => self.rom_bank_register_high = value & 0x01,
            0x4000..=0x5FFF => self.write_ram_bank(value),
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
        *self = Self {
            rumble: self.rumble,
            ..Self::new(self.rom_bank_count, self.ram_bank_count)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rumble_bit_drives_the_motor_and_leaves_three_ram_bank_bits() {
        let mut mbc = Mbc5::with_rumble(64, 4);
        mbc.on_write(0x0000, 0x0A);
        mbc.on_write(0x4000, 0x0B);
        assert!(mbc.motor);
        assert_eq!(mbc.ram_bank(), Some(3));
        mbc.on_write(0x4000, 0x02);
        assert!(!mbc.motor);
        assert_eq!(mbc.ram_bank(), Some(2));
    }

    #[test]
    fn plain_mbc5_keeps_bit_three_for_ram_banks() {
        let mut mbc = Mbc5::new(64, 16);
        mbc.on_write(0x0000, 0x0A);
        mbc.on_write(0x4000, 0x0B);
        assert!(!mbc.motor);
        assert_eq!(mbc.ram_bank(), Some(11));
    }

    #[test]
    fn rumble_level_is_the_share_of_cycles_with_the_motor_on() {
        let mut mbc = Mbc5::with_rumble(64, 0);
        mbc.on_write(0x4000, 0x08);
        (0..300).for_each(|_| mbc.cycle());
        mbc.on_write(0x4000, 0x00);
        (0..100).for_each(|_| mbc.cycle());
        assert_eq!(mbc.take_rumble(), 0.75);
        (0..100).for_each(|_| mbc.cycle());
        assert_eq!(mbc.take_rumble(), 0.0);
    }

    #[test]
    fn soft_reset_stops_the_motor_but_keeps_the_rumble_wiring() {
        let mut mbc = Mbc5::with_rumble(64, 0);
        mbc.on_write(0x4000, 0x08);
        mbc.soft_reset();
        assert!(mbc.rumble);
        assert!(!mbc.motor);
    }
}
