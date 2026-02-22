use crate::error::{GbError, GbResult};
use crate::rom::header::RomCartridgeType;
use crate::rom::Rom;

mod mbc1;

pub trait MbcInterface {
    fn ram_enabled(&self) -> bool;
    fn on_write(&mut self, addr: u16, value: u8);
    fn rom_bank_low(&self) -> usize;
    fn rom_bank_high(&self) -> usize;
    fn ram_bank(&self) -> usize;
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
}

impl TryFrom<&Rom> for Mbc {
    type Error = GbError;

    fn try_from(rom: &Rom) -> GbResult<Self> {
        let cartridge_type = rom
            .cartridge_type()?
            .ok_or(GbError::MissingRomCartridgeType)?;
        let rom_banks = rom.rom_banks()?;
        let ram_banks = rom.ram_banks()?;

        let mbc = match cartridge_type {
            RomCartridgeType::Mbc1 => Self::Mbc1(mbc1::Mbc1::new(rom_banks, ram_banks)),
            _ => Self::None,
        };

        Ok(mbc)
    }
}
