use crate::error::{GbError, GbResult};
use crate::gb::cartridge::{RAM_BANK_SIZE, ROM_BANK_SIZE};
use crate::rom::header::{RomCartridgeType, RomHeader};

pub mod camera;
mod huc1;
mod huc3;
mod mbc1;
mod mbc2;
pub mod mbc3;
mod mbc5;
mod mbc6;
mod mbc7;
mod mmm01;
mod tama5;
mod wisdom_tree;

pub trait MbcInterface {
    /// Returns `true` if the write was consumed by the MBC
    fn on_write(&mut self, addr: u16, value: u8) -> bool;
    fn on_read(&self, addr: u16) -> Option<u8>;
    fn rom_bank_low(&self) -> usize;
    fn rom_bank_high(&self) -> usize;
    fn ram_bank(&self) -> Option<usize>;
    fn soft_reset(&mut self);

    fn rom_offset(&self, addr: u16) -> usize {
        match addr {
            0x0000..=0x3FFF => self.rom_bank_low() * ROM_BANK_SIZE + addr as usize,
            _ => self.rom_bank_high() * ROM_BANK_SIZE + (addr as usize & 0x3FFF),
        }
    }

    fn ram_offset(&self, addr: u16) -> Option<usize> {
        self.ram_bank()
            .map(|bank| bank * RAM_BANK_SIZE + (addr as usize & 0x1FFF))
    }

    fn ram_write_offset(&self, addr: u16) -> Option<usize> {
        self.ram_offset(addr)
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Mbc {
    None,
    Mbc1(mbc1::Mbc1),
    Mbc2(mbc2::Mbc2),
    Mbc3(mbc3::Mbc3),
    Mbc5(mbc5::Mbc5),
    Mbc7(mbc7::Mbc7),
    Huc1(huc1::Huc1),
    Huc3(huc3::Huc3),
    Camera(camera::Camera),
    Tama5(tama5::Tama5),
    Mbc6(mbc6::Mbc6),
    Mmm01(mmm01::Mmm01),
    WisdomTree(wisdom_tree::WisdomTree),
}

#[allow(dead_code)]
impl Mbc {
    pub fn set_multicart(&mut self, multicart: bool) {
        if let Self::Mbc1(mbc) = self {
            mbc.multicart = multicart;
        }
    }

    pub fn get_internal_data(&self, now: u64) -> Option<Vec<u8>> {
        match self {
            Self::Tama5(mbc) => Some(mbc.save(now)),
            Self::Mbc2(mbc) => Some(mbc.ram.to_vec()),
            Self::Mbc7(mbc) => Some(mbc.eeprom.bytes()),
            _ => None,
        }
    }

    pub fn put_internal_data(&mut self, data: &[u8], now: u64) -> bool {
        match self {
            Self::Tama5(mbc) => {
                mbc.load(data, now);
                true
            }
            Self::Mbc2(mbc) => {
                let len = data.len().min(mbc.ram.len());
                mbc.ram[..len].copy_from_slice(&data[..len]);
                true
            }
            Self::Mbc7(mbc) => {
                mbc.eeprom.put_bytes(data);
                true
            }
            _ => false,
        }
    }

    pub fn has_ir(&self) -> bool {
        matches!(self, Self::Huc1(_) | Self::Huc3(_))
    }

    pub fn ir_led(&self) -> bool {
        match self {
            Self::Huc1(mbc) => mbc.led,
            Self::Huc3(mbc) => mbc.led,
            _ => false,
        }
    }

    pub fn set_ir_light(&mut self, light: bool) {
        match self {
            Self::Huc1(mbc) => mbc.light = light,
            Self::Huc3(mbc) => mbc.light = light,
            _ => {}
        }
    }

    pub fn battery_tail(&self, now: u64) -> Option<Vec<u8>> {
        match self {
            Self::Huc3(mbc) => Some(mbc.clock.encode(now).to_vec()),
            Self::Mbc6(mbc) => Some(mbc.flash.clone()),
            _ => None,
        }
    }

    pub fn put_battery_tail(&mut self, data: &[u8], now: u64) {
        match self {
            Self::Huc3(mbc) => {
                if let Some(clock) = huc3::Huc3Clock::decode(data, now) {
                    mbc.clock = clock;
                }
            }
            Self::Mbc6(mbc) => {
                let len = data.len().min(mbc.flash.len());
                mbc.flash[..len].copy_from_slice(&data[..len]);
            }
            _ => {}
        }
    }

    pub fn has_tilt(&self) -> bool {
        matches!(self, Self::Mbc7(_))
    }

    pub fn set_tilt(&mut self, x: f32, y: f32) {
        if let Self::Mbc7(mbc) = self {
            mbc.set_tilt(x, y);
        }
    }

    pub fn cycle(&mut self, ram: &mut [[u8; RAM_BANK_SIZE]]) -> bool {
        match self {
            Self::Mbc3(mbc) => mbc.cycle(),
            Self::Mbc5(mbc) => mbc.cycle(),
            Self::Huc3(mbc) => mbc.cycle(),
            Self::Tama5(mbc) => mbc.cycle(),
            Self::Camera(mbc) => return mbc.cycle(ram),
            _ => {}
        }
        false
    }

    pub fn has_camera(&self) -> bool {
        matches!(self, Self::Camera(_))
    }

    pub fn set_camera_frame(&mut self, frame: &[u8]) {
        if let Self::Camera(mbc) = self {
            mbc.set_frame(frame);
        }
    }

    pub fn has_rumble(&self) -> bool {
        matches!(self, Self::Mbc5(mbc) if mbc.rumble)
    }

    pub fn take_rumble(&mut self) -> f32 {
        match self {
            Self::Mbc5(mbc) => mbc.take_rumble(),
            _ => 0.0,
        }
    }

    pub fn rtc(&self) -> Option<&mbc3::Rtc> {
        match self {
            Self::Mbc3(mbc) if mbc.has_rtc => Some(&mbc.rtc),
            _ => None,
        }
    }

    pub fn rtc_mut(&mut self) -> Option<&mut mbc3::Rtc> {
        match self {
            Self::Mbc3(mbc) if mbc.has_rtc => Some(&mut mbc.rtc),
            _ => None,
        }
    }
}

macro_rules! dispatch {
    ($self:ident, $none:expr, $mbc:ident => $call:expr) => {
        match $self {
            Self::None => $none,
            Self::Mbc1($mbc) => $call,
            Self::Mbc2($mbc) => $call,
            Self::Mbc3($mbc) => $call,
            Self::Mbc5($mbc) => $call,
            Self::Mbc7($mbc) => $call,
            Self::Huc1($mbc) => $call,
            Self::Huc3($mbc) => $call,
            Self::Camera($mbc) => $call,
            Self::Tama5($mbc) => $call,
            Self::Mbc6($mbc) => $call,
            Self::Mmm01($mbc) => $call,
            Self::WisdomTree($mbc) => $call,
        }
    };
}

impl MbcInterface for Mbc {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        dispatch!(self, false, mbc => mbc.on_write(addr, value))
    }

    fn on_read(&self, addr: u16) -> Option<u8> {
        dispatch!(self, None, mbc => mbc.on_read(addr))
    }

    fn rom_bank_low(&self) -> usize {
        dispatch!(self, 0, mbc => mbc.rom_bank_low())
    }

    fn rom_bank_high(&self) -> usize {
        dispatch!(self, 1, mbc => mbc.rom_bank_high())
    }

    fn ram_bank(&self) -> Option<usize> {
        dispatch!(self, None, mbc => mbc.ram_bank())
    }

    fn rom_offset(&self, addr: u16) -> usize {
        dispatch!(self, addr as usize & 0x7FFF, mbc => mbc.rom_offset(addr))
    }

    fn ram_offset(&self, addr: u16) -> Option<usize> {
        dispatch!(self, None, mbc => mbc.ram_offset(addr))
    }

    fn ram_write_offset(&self, addr: u16) -> Option<usize> {
        dispatch!(self, None, mbc => mbc.ram_write_offset(addr))
    }

    fn soft_reset(&mut self) {
        dispatch!(self, (), mbc => mbc.soft_reset())
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
            RomCartridgeType::Mbc3
            | RomCartridgeType::Mbc3Ram
            | RomCartridgeType::Mbc3RamBattery => {
                Self::Mbc3(mbc3::Mbc3::new(header.rom_banks, header.ram_banks, false))
            }
            RomCartridgeType::Mbc3TimerBattery | RomCartridgeType::Mbc3TimerRamBattery => {
                Self::Mbc3(mbc3::Mbc3::new(header.rom_banks, header.ram_banks, true))
            }
            RomCartridgeType::Mbc5
            | RomCartridgeType::Mbc5Ram
            | RomCartridgeType::Mbc5RamBattery => {
                Self::Mbc5(mbc5::Mbc5::new(header.rom_banks, header.ram_banks))
            }
            RomCartridgeType::Mbc5Rumble
            | RomCartridgeType::Mbc5RumbleRam
            | RomCartridgeType::Mbc5RumbleRamBattery => {
                Self::Mbc5(mbc5::Mbc5::with_rumble(header.rom_banks, header.ram_banks))
            }
            RomCartridgeType::Mbc7SensorRumbleRamBattery => {
                Self::Mbc7(mbc7::Mbc7::new(header.rom_banks))
            }
            RomCartridgeType::HuC1RamBattery => {
                Self::Huc1(huc1::Huc1::new(header.rom_banks, header.ram_banks))
            }
            RomCartridgeType::HuC3 => {
                Self::Huc3(huc3::Huc3::new(header.rom_banks, header.ram_banks))
            }
            RomCartridgeType::PocketCamera => {
                Self::Camera(camera::Camera::new(header.rom_banks, header.ram_banks))
            }
            RomCartridgeType::BandaiTama5 => Self::Tama5(tama5::Tama5::new(header.rom_banks)),
            RomCartridgeType::Mbc6 => {
                Self::Mbc6(mbc6::Mbc6::new(header.rom_banks, header.ram_banks))
            }
            RomCartridgeType::Mmm01
            | RomCartridgeType::Mmm01Ram
            | RomCartridgeType::Mmm01RamBattery => {
                Self::Mmm01(mmm01::Mmm01::new(header.rom_banks, header.ram_banks))
            }
            RomCartridgeType::WisdomTree => {
                Self::WisdomTree(wisdom_tree::WisdomTree::new(header.rom_banks))
            }
            _ => Self::None,
        };

        Ok(mbc)
    }
}

fn mask_bank_number(bank: usize, bank_count: usize) -> usize {
    bank & (bank_count.next_power_of_two() - 1)
}
