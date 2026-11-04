mod detect;
mod eeprom;
mod flash;

use eeprom::{Eeprom, EepromSize};
use flash::{Flash, FlashChip};

const SRAM_SMALL: usize = 0x8000;
const SRAM_LARGE: usize = 0x1_0000;
const ERASED: u8 = 0xFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SaveType {
    Auto,
    Sram32K,
    Sram64K,
    Flash64K,
    Flash128K,
    Eeprom,
    Eeprom512,
    Eeprom8K,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Chip {
    Auto(Vec<u8>),
    Sram(Vec<u8>),
    Flash(Flash),
    Eeprom(Eeprom),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Backup {
    chip: Chip,
    dirty: bool,
}

impl Backup {
    pub fn new(kind: SaveType) -> Self {
        let chip = match kind {
            SaveType::Auto => Chip::Auto(Vec::new()),
            SaveType::Sram32K => Chip::Sram(vec![ERASED; SRAM_SMALL]),
            SaveType::Sram64K => Chip::Sram(vec![ERASED; SRAM_LARGE]),
            SaveType::Flash64K => Chip::Flash(Flash::new(FlashChip::Panasonic)),
            SaveType::Flash128K => Chip::Flash(Flash::new(FlashChip::Sanyo)),
            SaveType::Eeprom => Chip::Eeprom(Eeprom::new(None)),
            SaveType::Eeprom512 => Chip::Eeprom(Eeprom::new(Some(EepromSize::Small))),
            SaveType::Eeprom8K => Chip::Eeprom(Eeprom::new(Some(EepromSize::Large))),
        };
        Self { chip, dirty: false }
    }

    pub fn for_rom(rom: &[u8]) -> Self {
        Self::new(SaveType::detect(rom))
    }

    pub fn kind(&self) -> SaveType {
        match &self.chip {
            Chip::Auto(_) => SaveType::Auto,
            Chip::Sram(data) if data.len() == SRAM_SMALL => SaveType::Sram32K,
            Chip::Sram(_) => SaveType::Sram64K,
            Chip::Flash(flash) if flash.chip().is_large() => SaveType::Flash128K,
            Chip::Flash(_) => SaveType::Flash64K,
            Chip::Eeprom(eeprom) => match eeprom.size() {
                None => SaveType::Eeprom,
                Some(EepromSize::Small) => SaveType::Eeprom512,
                Some(EepromSize::Large) => SaveType::Eeprom8K,
            },
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match &self.chip {
            Chip::Auto(data) | Chip::Sram(data) => data,
            Chip::Flash(flash) => flash.bytes(),
            Chip::Eeprom(eeprom) => eeprom.bytes(),
        }
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn load(&mut self, bytes: &[u8]) {
        match &mut self.chip {
            Chip::Auto(data) => *data = bytes.to_vec(),
            Chip::Sram(data) => {
                let size = if bytes.len() > SRAM_SMALL {
                    SRAM_LARGE
                } else {
                    data.len()
                };
                *data = fitted(bytes, size);
            }
            Chip::Flash(flash) => {
                if bytes.len() > flash.bytes().len() {
                    *flash = Flash::new(FlashChip::Sanyo);
                }
                flash.load(bytes);
            }
            Chip::Eeprom(eeprom) => eeprom.load(bytes),
        }
    }

    pub fn read8(&self, offset: usize) -> u8 {
        match &self.chip {
            Chip::Sram(data) => data[offset % data.len()],
            Chip::Flash(flash) => flash.read(offset),
            Chip::Auto(data) if !data.is_empty() => data[offset % data.len()],
            Chip::Auto(_) | Chip::Eeprom(_) => ERASED,
        }
    }

    pub fn write8(&mut self, offset: usize, value: u8) {
        if let Chip::Auto(_) = self.chip {
            let kind = if flash::starts_command(offset, value) {
                SaveType::Flash64K
            } else {
                SaveType::Sram32K
            };
            self.settle(kind);
        }
        match &mut self.chip {
            Chip::Sram(data) => {
                let len = data.len();
                data[offset % len] = value;
                self.dirty = true;
            }
            Chip::Flash(flash) => self.dirty |= flash.write(offset, value),
            Chip::Auto(_) | Chip::Eeprom(_) => {}
        }
    }

    pub fn uses_eeprom(&self) -> bool {
        matches!(self.chip, Chip::Eeprom(_))
    }

    pub fn read_eeprom(&self) -> u16 {
        match &self.chip {
            Chip::Eeprom(eeprom) => eeprom.read(),
            _ => 0,
        }
    }

    pub fn write_eeprom(&mut self, value: u16) {
        if let Chip::Auto(_) = self.chip {
            self.settle(SaveType::Eeprom);
        }
        if let Chip::Eeprom(eeprom) = &mut self.chip {
            self.dirty |= eeprom.write(value & 1 == 1);
        }
    }

    pub fn eeprom_dma(&mut self, units: u32) {
        if let Chip::Auto(_) = self.chip {
            self.settle(SaveType::Eeprom);
        }
        if let Chip::Eeprom(eeprom) = &mut self.chip {
            eeprom.start_transfer(units);
        }
    }

    fn settle(&mut self, kind: SaveType) {
        if let Chip::Auto(pending) = &mut self.chip {
            let pending = std::mem::take(pending);
            *self = Self::new(kind);
            if !pending.is_empty() {
                self.load(&pending);
            }
        }
    }
}

fn fitted(bytes: &[u8], size: usize) -> Vec<u8> {
    let mut data = vec![ERASED; size];
    let len = bytes.len().min(size);
    data[..len].copy_from_slice(&bytes[..len]);
    data
}

#[cfg(test)]
mod tests;
