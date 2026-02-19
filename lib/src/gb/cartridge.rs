use crate::error::{GbError, GbResult};
use crate::rom::Rom;
use crate::{ReadMemory, WriteMemory};

const ROM_BANK_SIZE: usize = 0x4000; // 16KiB
const RAM_BANK_SIZE: usize = 0x2000; // 8KiB

pub struct Cartridge {
    rom: Vec<[u8; ROM_BANK_SIZE]>,
    ram: Vec<[u8; RAM_BANK_SIZE]>,
}

impl Cartridge {
    pub fn new() -> Self {
        Self {
            rom: vec![[0; ROM_BANK_SIZE]; 2],
            ram: vec![[0; RAM_BANK_SIZE]; 1],
        }
    }

    pub fn load_rom(&mut self, rom: &Rom) -> GbResult<()> {
        let rom_banks = rom.rom_banks()?.max(2);
        let ram_banks = rom.ram_banks()?.max(1);

        self.rom = rom
            .data
            .chunks(ROM_BANK_SIZE)
            .map(|chunk| {
                let mut bank = [0u8; ROM_BANK_SIZE];
                bank[..chunk.len()].copy_from_slice(chunk);
                bank
            })
            .collect();

        if self.rom.len() > rom_banks {
            return Err(GbError::RomTooBig);
        }

        self.rom.resize(rom_banks, [0; ROM_BANK_SIZE]);
        self.ram = vec![[0; RAM_BANK_SIZE]; ram_banks];

        Ok(())
    }
}

impl ReadMemory for Cartridge {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.rom[0][addr as usize],
            0x4000..=0x7FFF => self.rom[1][(addr - 0x4000) as usize],
            0xA000..=0xBFFF => self.ram[0][(addr - 0xA000) as usize],
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Cartridge {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0xA000..=0xBFFF => self.ram[0][(addr - 0xA000) as usize] = value,
            _ => {}
        }
    }
}
