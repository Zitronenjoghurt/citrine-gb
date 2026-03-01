use crate::error::{GbError, GbResult};
use crate::rom::header::{RomCartridgeType, RomHeader};

mod mbc1;
mod mbc2;

pub trait MbcInterface {
    /// Returns `true` if the write was consumed by the MBC
    fn on_write(&mut self, addr: u16, value: u8) -> bool;
    fn on_read(&self, addr: u16) -> Option<u8>;
    fn rom_bank_low(&self) -> usize;
    fn rom_bank_high(&self) -> usize;
    fn ram_bank(&self) -> Option<usize>;
    fn soft_reset(&mut self);
}

#[derive(Debug)]
pub enum Mbc {
    None,
    Mbc1(mbc1::Mbc1),
    Mbc2(mbc2::Mbc2),
}

impl Mbc {
    pub fn get_internal_data(&self) -> Option<&[u8]> {
        if let Self::Mbc2(mbc) = self {
            Some(mbc.ram.as_slice())
        } else {
            None
        }
    }

    pub fn put_internal_data(&mut self, data: &[u8]) -> bool {
        if let Self::Mbc2(mbc) = self {
            mbc.ram.copy_from_slice(data);
            true
        } else {
            false
        }
    }
}

impl MbcInterface for Mbc {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        match self {
            Self::None => false,
            Self::Mbc1(mbc) => mbc.on_write(addr, value),
            Self::Mbc2(mbc) => mbc.on_write(addr, value),
        }
    }

    fn on_read(&self, addr: u16) -> Option<u8> {
        match self {
            Self::None => None,
            Self::Mbc1(mbc) => mbc.on_read(addr),
            Self::Mbc2(mbc) => mbc.on_read(addr),
        }
    }

    fn rom_bank_low(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Mbc1(mbc) => mbc.rom_bank_low(),
            Self::Mbc2(mbc) => mbc.rom_bank_low(),
        }
    }

    fn rom_bank_high(&self) -> usize {
        match self {
            Self::None => 1,
            Self::Mbc1(mbc) => mbc.rom_bank_high(),
            Self::Mbc2(mbc) => mbc.rom_bank_high(),
        }
    }

    fn ram_bank(&self) -> Option<usize> {
        match self {
            Self::None => None,
            Self::Mbc1(mbc) => mbc.ram_bank(),
            Self::Mbc2(mbc) => mbc.ram_bank(),
        }
    }

    fn soft_reset(&mut self) {
        match self {
            Self::None => {}
            Self::Mbc1(mbc) => mbc.soft_reset(),
            Self::Mbc2(mbc) => mbc.soft_reset(),
        }
    }
}

impl TryFrom<&RomHeader> for Mbc {
    type Error = GbError;

    fn try_from(header: &RomHeader) -> GbResult<Self> {
        let cartridge_type = header
            .cartridge_type
            .ok_or(GbError::MissingRomCartridgeType)?;

        let mbc = match cartridge_type {
            RomCartridgeType::Mbc1
            | RomCartridgeType::Mbc1Ram
            | RomCartridgeType::Mbc1RamBattery => {
                Self::Mbc1(mbc1::Mbc1::new(header.rom_banks, header.ram_banks))
            }
            RomCartridgeType::Mbc2 | RomCartridgeType::Mbc2Battery => {
                Self::Mbc2(mbc2::Mbc2::new(header.rom_banks))
            }
            _ => Self::None,
        };

        Ok(mbc)
    }
}

fn mask_rom_bank(bank: usize, rom_bank_count: usize) -> usize {
    bank & (rom_bank_count.next_power_of_two() - 1)
}
