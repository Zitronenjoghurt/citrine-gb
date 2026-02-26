use crate::error::{GbError, GbResult};
use crate::gb::cartridge::mbc::MbcInterface;
use crate::rom::header::RomHeader;
use crate::rom::Rom;
use crate::{ReadMemory, WriteMemory};

mod mbc;

pub const ROM_BANK_SIZE: usize = 0x4000; // 16KiB
pub const RAM_BANK_SIZE: usize = 0x2000; // 8KiB

pub struct Cartridge {
    pub header: RomHeader,
    pub has_rom_loaded: bool,
    mbc: mbc::Mbc,
    rom: Vec<[u8; ROM_BANK_SIZE]>,
    ram: Vec<[u8; RAM_BANK_SIZE]>,
}

impl Default for Cartridge {
    fn default() -> Self {
        Self {
            header: RomHeader::default(),
            has_rom_loaded: false,
            mbc: mbc::Mbc::None,
            rom: vec![[0; ROM_BANK_SIZE]; 2],
            ram: vec![[0; RAM_BANK_SIZE]; 1],
        }
    }
}

impl Cartridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_rom(&mut self, rom: &Rom) -> GbResult<()> {
        let header = rom.header()?;
        let rom_banks = header.rom_banks.max(2);
        let ram_banks = header.ram_banks.max(1);
        self.mbc = mbc::Mbc::try_from(&header)?;

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

        self.header = header;
        self.has_rom_loaded = true;
        self.rom.resize(rom_banks, [0; ROM_BANK_SIZE]);
        self.ram = vec![[0; RAM_BANK_SIZE]; ram_banks];

        Ok(())
    }

    pub fn soft_reset(&mut self) {
        self.mbc.soft_reset();
        self.ram.iter_mut().for_each(|bank| bank.fill(0));
    }
}

impl ReadMemory for Cartridge {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.rom[self.mbc.rom_bank_low()][addr as usize],
            0x4000..=0x7FFF => self.rom[self.mbc.rom_bank_high()][(addr - 0x4000) as usize],
            0xA000..=0xBFFF => {
                if self.mbc.ram_enabled() {
                    self.ram[self.mbc.ram_bank()][(addr - 0xA000) as usize]
                } else {
                    0xFF
                }
            }
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Cartridge {
    fn write_naive(&mut self, addr: u16, value: u8) {
        self.mbc.on_write(addr, value);

        if self.mbc.ram_enabled() && (0xA000..=0xBFFF).contains(&addr) {
            self.ram[self.mbc.ram_bank()][(addr - 0xA000) as usize] = value;
        }
    }
}
