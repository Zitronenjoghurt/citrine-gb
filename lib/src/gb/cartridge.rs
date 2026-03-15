use crate::disassembly::DisassemblySource;
use crate::error::{GbError, GbResult};
use crate::gb::cartridge::mbc::MbcInterface;
use crate::rom::header::RomHeader;
use crate::rom::Rom;
use crate::{ReadMemory, WriteMemory};
use std::fmt::{Display, Formatter};

mod mbc;

pub const ROM_BANK_SIZE: usize = 0x4000; // 16KiB
pub const RAM_BANK_SIZE: usize = 0x2000; // 8KiB

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cartridge {
    pub header: RomHeader,
    pub has_rom_loaded: bool,
    has_battery: bool,
    sram_dirty: bool,
    mbc: mbc::Mbc,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    rom: Vec<[u8; ROM_BANK_SIZE]>,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    ram: Vec<[u8; RAM_BANK_SIZE]>,
}

impl Default for Cartridge {
    fn default() -> Self {
        Self {
            header: RomHeader::default(),
            has_rom_loaded: false,
            has_battery: false,
            sram_dirty: false,
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

        self.has_battery = header
            .cartridge_type
            .map(|ct| ct.has_battery())
            .unwrap_or(false);
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

    #[cfg(feature = "persistence")]
    pub fn poll_sram_dump(
        &mut self,
        force: bool,
    ) -> Option<crate::persistence::sram_dump::SramDump> {
        if !self.has_battery || (!self.sram_dirty && !force) {
            return None;
        };

        let dump = if let Some(data) = self.mbc.get_internal_data() {
            Some(crate::persistence::sram_dump::SramDump::from_slice(data))
        } else {
            Some(crate::persistence::sram_dump::SramDump::from_banks(
                self.ram.as_slice(),
            ))
        };

        self.sram_dirty = false;

        dump
    }

    #[cfg(feature = "persistence")]
    pub fn put_sram_dump(&mut self, dump: crate::persistence::sram_dump::SramDump) {
        if !self.has_battery {
            return;
        };

        let data = dump.as_slice();

        let internal = self.mbc.put_internal_data(data);
        if internal {
            return;
        };

        for (bank, chunk) in self.ram.iter_mut().zip(data.chunks(RAM_BANK_SIZE)) {
            bank[..chunk.len()].copy_from_slice(chunk);
        }
    }

    pub fn supports_sram_saves(&self) -> bool {
        self.has_battery
    }
}

impl ReadMemory for Cartridge {
    fn read_naive(&self, addr: u16) -> u8 {
        if let Some(value) = self.mbc.on_read(addr) {
            return value;
        };

        match addr {
            0x0000..=0x3FFF => self.rom[self.mbc.rom_bank_low()][addr as usize],
            0x4000..=0x7FFF => self.rom[self.mbc.rom_bank_high()][(addr - 0x4000) as usize],
            0xA000..=0xBFFF => {
                if let Some(bank) = self.mbc.ram_bank() {
                    self.ram[bank][(addr - 0xA000) as usize]
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
        let consumed = self.mbc.on_write(addr, value);
        if consumed {
            self.sram_dirty = true;
            return;
        }

        if let Some(bank) = self.mbc.ram_bank()
            && (0xA000..=0xBFFF).contains(&addr)
        {
            self.ram[bank][(addr - 0xA000) as usize] = value;
            self.sram_dirty = true;
        }
    }
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RomLocation {
    pub bank: u32,
    pub offset: u16,
}

impl RomLocation {
    pub fn offset(&self, offset: i16) -> RomLocation {
        RomLocation {
            bank: self.bank,
            offset: (self.offset as i16 + offset) as u16,
        }
    }
}

impl Display for RomLocation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.bank > 0xFF {
            write!(f, "{:04X}:{:04X}", self.bank, self.offset)
        } else {
            write!(f, "{:02X}:{:04X}", self.bank, self.offset)
        }
    }
}

impl DisassemblySource for Cartridge {
    fn read_rom_address(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.rom[self.mbc.rom_bank_low()][addr as usize],
            0x4000..=0x7FFF => self.rom[self.mbc.rom_bank_high()][(addr - 0x4000) as usize],
            _ => 0xFF,
        }
    }

    fn probe_rom_location(&self, addr: u16) -> RomLocation {
        match addr {
            0x0000..=0x3FFF => RomLocation {
                bank: self.mbc.rom_bank_low() as u32,
                offset: addr,
            },
            0x4000..=0x7FFF => RomLocation {
                bank: self.mbc.rom_bank_high() as u32,
                offset: addr - 0x4000,
            },
            _ => RomLocation {
                bank: 0,
                offset: addr,
            },
        }
    }

    fn read_rom_location(&self, loc: RomLocation) -> u8 {
        self.rom
            .get(loc.bank as usize)
            .map(|bank| bank.get(loc.offset as usize).copied().unwrap_or(0xFF))
            .unwrap_or(0xFF)
    }
}
