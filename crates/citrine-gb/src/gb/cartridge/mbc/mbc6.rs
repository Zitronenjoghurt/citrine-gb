use crate::gb::cartridge::mbc::MbcInterface;

pub const FLASH_SIZE: usize = 0x10_0000;
const FLASH_BANK: usize = 0x2000;
const RAM_HALF: usize = 0x1000;
const SECTOR: usize = 0x2_0000;
const BLOCK: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Unlock {
    None,
    First,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum FlashMode {
    Read,
    Id,
    EraseArmed,
    Program,
    Status,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mbc6 {
    pub rom_bank_count: usize,
    pub ram_bank_count: usize,
    pub ram_enabled: bool,
    pub ram_banks: [u8; 2],
    pub rom_banks: [u8; 2],
    pub from_flash: [bool; 2],
    pub flash_enabled: bool,
    pub flash_writable: bool,
    unlock: Unlock,
    mode: FlashMode,
    block: Vec<u8>,
    block_base: Option<usize>,
    block_count: usize,
    pub flash: Vec<u8>,
}

impl Mbc6 {
    pub fn new(rom_bank_count: usize, ram_bank_count: usize) -> Self {
        Self {
            rom_bank_count,
            ram_bank_count,
            ram_enabled: false,
            ram_banks: [0, 0],
            rom_banks: [0, 0],
            from_flash: [false, false],
            flash_enabled: false,
            flash_writable: false,
            unlock: Unlock::None,
            mode: FlashMode::Read,
            block: Vec::new(),
            block_base: None,
            block_count: 0,
            flash: vec![0xFF; FLASH_SIZE],
        }
    }

    fn window(addr: u16) -> usize {
        usize::from(addr >= 0x6000)
    }

    fn flash_address(&self, addr: u16) -> usize {
        let window = Self::window(addr);
        (self.rom_banks[window] as usize & 0x7F) * FLASH_BANK + (addr as usize & 0x1FFF)
    }

    fn sector_writable(&self, address: usize) -> bool {
        address >= SECTOR || self.flash_writable
    }

    fn write_flash(&mut self, address: usize, value: u8) -> bool {
        if value == 0xF0 {
            self.mode = FlashMode::Read;
            self.unlock = Unlock::None;
            self.block_base = None;
            return false;
        }
        let command = address & 0x7FFF;
        match self.mode {
            FlashMode::Program => return self.program(address, value),
            FlashMode::EraseArmed if self.unlock == Unlock::Done => {
                self.unlock = Unlock::None;
                match value {
                    0x30 => {
                        let sector = address / SECTOR * SECTOR;
                        self.mode = FlashMode::Status;
                        if self.sector_writable(sector) {
                            self.flash[sector..sector + SECTOR].fill(0xFF);
                            return true;
                        }
                        return false;
                    }
                    0x10 if command == 0x5555 => {
                        let from = if self.flash_writable { 0 } else { SECTOR };
                        self.flash[from..].fill(0xFF);
                        self.mode = FlashMode::Status;
                        return true;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        match (self.unlock, command, value) {
            (_, 0x5555, 0xAA) => {
                self.unlock = Unlock::First;
                return false;
            }
            (Unlock::First, 0x2AAA, 0x55) => {
                self.unlock = Unlock::Done;
                return false;
            }
            _ => {}
        }
        if command == 0x5555 {
            match value {
                0x80 => self.mode = FlashMode::EraseArmed,
                0x90 => self.mode = FlashMode::Id,
                0xA0 => {
                    self.mode = FlashMode::Program;
                    self.block_base = None;
                }
                _ => {}
            }
        }
        self.unlock = Unlock::None;
        false
    }

    fn program(&mut self, address: usize, value: u8) -> bool {
        let base = address / BLOCK * BLOCK;
        if self.block_base != Some(base) {
            self.block_base = Some(base);
            self.block = vec![0xFF; BLOCK];
            self.block_count = 0;
        }
        if self.block_count >= BLOCK && address - base == BLOCK - 1 {
            self.block_base = None;
            self.mode = FlashMode::Status;
            if !self.sector_writable(base) {
                return false;
            }
            for (cell, byte) in self.flash[base..base + BLOCK].iter_mut().zip(&self.block) {
                *cell &= byte;
            }
            return true;
        }
        self.block[address - base] = value;
        self.block_count += 1;
        false
    }

    fn read_flash(&self, addr: u16) -> u8 {
        if !self.flash_enabled {
            return 0xFF;
        }
        let address = self.flash_address(addr);
        match self.mode {
            FlashMode::Id => {
                if address & 0x01 == 0 {
                    0xC2
                } else {
                    0x81
                }
            }
            FlashMode::Status => 0x80,
            _ => self.flash[address],
        }
    }
}

impl MbcInterface for Mbc6 {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        match addr {
            0x0000..=0x03FF => self.ram_enabled = value & 0x0F == 0x0A,
            0x0400..=0x07FF => self.ram_banks[0] = value & 0x07,
            0x0800..=0x0BFF => self.ram_banks[1] = value & 0x07,
            0x0C00..=0x0FFF => self.flash_enabled = value & 0x01 != 0,
            0x1000 => self.flash_writable = value & 0x01 != 0,
            0x2000..=0x27FF => self.rom_banks[0] = value & 0x7F,
            0x2800..=0x2FFF => self.from_flash[0] = value == 0x08,
            0x3000..=0x37FF => self.rom_banks[1] = value & 0x7F,
            0x3800..=0x3FFF => self.from_flash[1] = value == 0x08,
            0x4000..=0x7FFF if self.from_flash[Self::window(addr)] && self.flash_enabled => {
                let address = self.flash_address(addr);
                return self.write_flash(address, value);
            }
            _ => {}
        }
        false
    }

    fn on_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x4000..=0x7FFF if self.from_flash[Self::window(addr)] => Some(self.read_flash(addr)),
            _ => None,
        }
    }

    fn rom_bank_low(&self) -> usize {
        0
    }

    fn rom_bank_high(&self) -> usize {
        self.rom_banks[0] as usize / 2
    }

    fn ram_bank(&self) -> Option<usize> {
        None
    }

    fn rom_offset(&self, addr: u16) -> usize {
        match addr {
            0x0000..=0x3FFF => addr as usize,
            _ => {
                let bank = self.rom_banks[Self::window(addr)] as usize;
                bank * FLASH_BANK + (addr as usize & 0x1FFF)
            }
        }
    }

    fn ram_offset(&self, addr: u16) -> Option<usize> {
        if !self.ram_enabled || !(0xA000..=0xBFFF).contains(&addr) {
            return None;
        }
        let half = usize::from(addr >= 0xB000);
        Some(self.ram_banks[half] as usize * RAM_HALF + (addr as usize & 0x0FFF))
    }

    fn soft_reset(&mut self) {
        let flash = std::mem::take(&mut self.flash);
        *self = Self::new(self.rom_bank_count, self.ram_bank_count);
        self.flash = flash;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cart() -> Mbc6 {
        let mut mbc = Mbc6::new(64, 4);
        mbc.on_write(0x0C00, 0x01);
        mbc.on_write(0x2800, 0x08);
        mbc
    }

    fn at(mbc: &mut Mbc6, bank: u8, addr: u16, value: u8) {
        mbc.on_write(0x2000, bank);
        mbc.on_write(addr, value);
    }

    fn unlock(mbc: &mut Mbc6) {
        at(mbc, 2, 0x5555, 0xAA);
        at(mbc, 1, 0x4AAA, 0x55);
    }

    #[test]
    fn two_rom_half_banks_map_8k_each() {
        let mut mbc = Mbc6::new(64, 4);
        mbc.on_write(0x2000, 0x05);
        mbc.on_write(0x3000, 0x0A);
        assert_eq!(mbc.rom_offset(0x4123), 5 * 0x2000 + 0x123);
        assert_eq!(mbc.rom_offset(0x6123), 10 * 0x2000 + 0x123);
        assert_eq!(mbc.rom_offset(0x1234), 0x1234);
        assert_eq!(mbc.on_read(0x4000), None);
    }

    #[test]
    fn two_ram_half_banks_map_4k_each_once_enabled() {
        let mut mbc = Mbc6::new(64, 4);
        mbc.on_write(0x0400, 0x03);
        mbc.on_write(0x0800, 0x06);
        assert_eq!(mbc.ram_offset(0xA010), None);
        mbc.on_write(0x0000, 0x0A);
        assert_eq!(mbc.ram_offset(0xA010), Some(3 * 0x1000 + 0x10));
        assert_eq!(mbc.ram_offset(0xB010), Some(6 * 0x1000 + 0x10));
    }

    #[test]
    fn flash_reads_ff_while_disabled() {
        let mut mbc = cart();
        mbc.flash[0] = 0x12;
        assert_eq!(mbc.on_read(0x4000), Some(0x12));
        mbc.on_write(0x0C00, 0x00);
        assert_eq!(mbc.on_read(0x4000), Some(0xFF));
    }

    #[test]
    fn id_mode_reads_the_jedec_ids_until_f0() {
        let mut mbc = cart();
        unlock(&mut mbc);
        at(&mut mbc, 2, 0x5555, 0x90);
        assert_eq!(mbc.on_read(0x4000), Some(0xC2));
        assert_eq!(mbc.on_read(0x4001), Some(0x81));
        mbc.on_write(0x4000, 0xF0);
        assert_eq!(mbc.on_read(0x4000), Some(0xFF));
    }

    fn program_block(mbc: &mut Mbc6, bank: u8, offset: u16, data: &[u8; BLOCK]) {
        unlock(mbc);
        at(mbc, 2, 0x5555, 0xA0);
        mbc.on_write(0x2000, bank);
        for (i, byte) in data.iter().enumerate() {
            mbc.on_write(0x4000 + offset + i as u16, *byte);
        }
        mbc.on_write(0x4000 + offset + BLOCK as u16 - 1, 0x00);
    }

    #[test]
    fn programming_a_block_ands_it_into_flash_and_reports_done() {
        let mut mbc = cart();
        let mut data = [0u8; BLOCK];
        data.iter_mut().enumerate().for_each(|(i, b)| *b = i as u8);
        program_block(&mut mbc, 16, 0x80, &data);
        assert_eq!(mbc.on_read(0x4080), Some(0x80));
        mbc.on_write(0x4000, 0xF0);
        assert_eq!(mbc.on_read(0x4085), Some(5));
        assert_eq!(mbc.flash[16 * 0x2000 + 0x80 + 127], 127);
    }

    #[test]
    fn sector_zero_needs_the_write_enable() {
        let mut mbc = cart();
        program_block(&mut mbc, 0, 0, &[0u8; BLOCK]);
        mbc.on_write(0x4000, 0xF0);
        assert_eq!(mbc.flash[0], 0xFF);
        mbc.on_write(0x1000, 0x01);
        program_block(&mut mbc, 0, 0, &[0u8; BLOCK]);
        assert_eq!(mbc.flash[0], 0x00);
    }

    #[test]
    fn sector_erase_fills_ff() {
        let mut mbc = cart();
        mbc.flash[2 * SECTOR..3 * SECTOR].fill(0);
        unlock(&mut mbc);
        at(&mut mbc, 2, 0x5555, 0x80);
        unlock(&mut mbc);
        at(&mut mbc, 32, 0x4000, 0x30);
        assert!(mbc.flash[2 * SECTOR..3 * SECTOR].iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn chip_erase_spares_sector_zero_without_write_enable() {
        let mut mbc = cart();
        mbc.flash.fill(0);
        unlock(&mut mbc);
        at(&mut mbc, 2, 0x5555, 0x80);
        unlock(&mut mbc);
        at(&mut mbc, 2, 0x5555, 0x10);
        assert_eq!(mbc.flash[0], 0x00);
        assert_eq!(mbc.flash[SECTOR], 0xFF);
    }
}
