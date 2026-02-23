use crate::error::{GbError, GbResult};
use crate::rom::header::{RomCartridgeType, RomHeader};

mod mbc1;

pub trait MbcInterface {
    fn ram_enabled(&self) -> bool;
    fn on_write(&mut self, addr: u16, value: u8);
    fn rom_bank_low(&self) -> usize;
    fn rom_bank_high(&self) -> usize;
    fn ram_bank(&self) -> usize;
    fn soft_reset(&mut self);
}

#[derive(Debug)]
pub enum Mbc {
    None,
    Mbc1(mbc1::Mbc1),
}

impl MbcInterface for Mbc {
    fn ram_enabled(&self) -> bool {
        match self {
            Self::None => false,
            Self::Mbc1(mbc) => mbc.ram_enabled(),
        }
    }

    fn on_write(&mut self, addr: u16, value: u8) {
        match self {
            Self::None => {}
            Self::Mbc1(mbc) => mbc.on_write(addr, value),
        }
    }

    fn rom_bank_low(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Mbc1(mbc) => mbc.rom_bank_low(),
        }
    }

    fn rom_bank_high(&self) -> usize {
        match self {
            Self::None => 1,
            Self::Mbc1(mbc) => mbc.rom_bank_high(),
        }
    }

    fn ram_bank(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Mbc1(mbc) => mbc.ram_bank(),
        }
    }

    fn soft_reset(&mut self) {
        match self {
            Self::None => {}
            Self::Mbc1(mbc) => mbc.soft_reset(),
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
            RomCartridgeType::Mbc1 => {
                Self::Mbc1(mbc1::Mbc1::new(header.rom_banks, header.ram_banks))
            }
            _ => Self::None,
        };

        Ok(mbc)
    }
}
