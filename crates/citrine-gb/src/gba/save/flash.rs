use super::{ERASED, fitted};

const BANK_SIZE: usize = 0x1_0000;
const SECTOR_SIZE: usize = 0x1000;
const ATMEL_PAGE: u8 = 128;
const UNLOCK_FIRST: (usize, u8) = (0x5555, 0xAA);
const UNLOCK_SECOND: (usize, u8) = (0x2AAA, 0x55);
const COMMAND: usize = 0x5555;
const ENTER_ID: u8 = 0x90;
const EXIT_ID: u8 = 0xF0;
const PREPARE_ERASE: u8 = 0x80;
const ERASE_CHIP: u8 = 0x10;
const ERASE_SECTOR: u8 = 0x30;
const WRITE_BYTE: u8 = 0xA0;
const SWITCH_BANK: u8 = 0xB0;

pub(super) fn starts_command(offset: usize, value: u8) -> bool {
    (offset, value) == UNLOCK_FIRST
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    not(any(feature = "serde", test)),
    expect(dead_code, reason = "only save states pick these chips")
)]
pub enum FlashChip {
    Sst,
    Macronix64K,
    Panasonic,
    Atmel,
    Sanyo,
    Macronix128K,
}

impl FlashChip {
    pub fn id(self) -> u16 {
        match self {
            Self::Sst => 0xD4BF,
            Self::Macronix64K => 0x1CC2,
            Self::Panasonic => 0x1B32,
            Self::Atmel => 0x3D1F,
            Self::Sanyo => 0x1362,
            Self::Macronix128K => 0x09C2,
        }
    }

    pub fn is_large(self) -> bool {
        matches!(self, Self::Sanyo | Self::Macronix128K)
    }

    fn size(self) -> usize {
        if self.is_large() {
            2 * BANK_SIZE
        } else {
            BANK_SIZE
        }
    }

    fn bytes_per_write(self) -> u8 {
        if self == Self::Atmel { ATMEL_PAGE } else { 1 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Unlock {
    Locked,
    First,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Mode {
    Normal,
    Erase,
    Write(u8),
    Bank,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Flash {
    chip: FlashChip,
    data: Vec<u8>,
    bank: usize,
    unlock: Unlock,
    mode: Mode,
    id_mode: bool,
}

impl Flash {
    pub fn new(chip: FlashChip) -> Self {
        Self {
            chip,
            data: vec![ERASED; chip.size()],
            bank: 0,
            unlock: Unlock::Locked,
            mode: Mode::Normal,
            id_mode: false,
        }
    }

    pub fn chip(&self) -> FlashChip {
        self.chip
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn load(&mut self, bytes: &[u8]) {
        self.data = fitted(bytes, self.chip.size());
    }

    pub fn read(&self, offset: usize) -> u8 {
        let offset = offset % BANK_SIZE;
        match (self.id_mode, offset) {
            (true, 0) => self.chip.id() as u8,
            (true, 1) => (self.chip.id() >> 8) as u8,
            _ => self.data[self.bank * BANK_SIZE + offset],
        }
    }

    pub fn write(&mut self, offset: usize, value: u8) -> bool {
        let offset = offset % BANK_SIZE;
        match self.mode {
            Mode::Write(left) => {
                self.data[self.bank * BANK_SIZE + offset] = value;
                self.mode = if left > 1 {
                    Mode::Write(left - 1)
                } else {
                    Mode::Normal
                };
                return true;
            }
            Mode::Bank if offset == 0 => {
                self.bank = usize::from(value & 1);
                self.mode = Mode::Normal;
                return false;
            }
            _ => {}
        }
        match (self.unlock, (offset, value)) {
            (Unlock::Locked, UNLOCK_FIRST) => self.unlock = Unlock::First,
            (Unlock::First, UNLOCK_SECOND) => self.unlock = Unlock::Open,
            (Unlock::Open, (COMMAND, command)) => {
                self.unlock = Unlock::Locked;
                return self.command(command);
            }
            (Unlock::Open, (sector, ERASE_SECTOR)) if self.mode == Mode::Erase => {
                self.unlock = Unlock::Locked;
                self.mode = Mode::Normal;
                let start = self.bank * BANK_SIZE + (sector & !(SECTOR_SIZE - 1));
                self.data[start..start + SECTOR_SIZE].fill(ERASED);
                return true;
            }
            (_, (COMMAND, EXIT_ID)) => {
                self.unlock = Unlock::Locked;
                self.mode = Mode::Normal;
                self.id_mode = false;
            }
            _ => self.unlock = Unlock::Locked,
        }
        false
    }

    fn command(&mut self, command: u8) -> bool {
        let erasing = self.mode == Mode::Erase;
        self.mode = Mode::Normal;
        match command {
            ENTER_ID => self.id_mode = true,
            EXIT_ID => self.id_mode = false,
            PREPARE_ERASE => self.mode = Mode::Erase,
            ERASE_CHIP if erasing => {
                self.data.fill(ERASED);
                return true;
            }
            WRITE_BYTE => self.mode = Mode::Write(self.chip.bytes_per_write()),
            SWITCH_BANK if self.chip.is_large() => self.mode = Mode::Bank,
            _ => {}
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{Flash, FlashChip};

    #[test]
    fn every_flash_chip_answers_with_its_id() {
        let chips = [
            (FlashChip::Sst, [0xBF, 0xD4]),
            (FlashChip::Macronix64K, [0xC2, 0x1C]),
            (FlashChip::Panasonic, [0x32, 0x1B]),
            (FlashChip::Atmel, [0x1F, 0x3D]),
            (FlashChip::Sanyo, [0x62, 0x13]),
            (FlashChip::Macronix128K, [0xC2, 0x09]),
        ];
        for (chip, [maker, device]) in chips {
            let mut flash = Flash::new(chip);
            for (offset, value) in [(0x5555, 0xAA), (0x2AAA, 0x55), (0x5555, 0x90)] {
                flash.write(offset, value);
            }
            assert_eq!([flash.read(0), flash.read(1)], [maker, device], "{chip:?}");
            for (offset, value) in [(0x5555, 0xAA), (0x2AAA, 0x55), (0x5555, 0xF0)] {
                flash.write(offset, value);
            }
            assert_eq!([flash.read(0), flash.read(1)], [0xFF, 0xFF], "{chip:?}");
        }
    }

    #[test]
    fn atmel_writes_a_128_byte_page_per_command() {
        let mut flash = Flash::new(FlashChip::Atmel);
        for (offset, value) in [(0x5555, 0xAA), (0x2AAA, 0x55), (0x5555, 0xA0)] {
            flash.write(offset, value);
        }
        for i in 0..128 {
            assert!(flash.write(0x80 + i, i as u8));
        }
        assert!(!flash.write(0x100, 0x00));
        assert_eq!(flash.read(0xFF), 0x7F);
        assert_eq!(flash.read(0x100), 0xFF);
    }
}
