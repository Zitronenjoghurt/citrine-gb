use crate::gb::cartridge::mbc::{MbcInterface, mask_bank_number};

pub const EEPROM_SIZE: usize = 256;
const WORDS: usize = EEPROM_SIZE / 2;
const CENTRE: f32 = 0x81D0 as f32;
const PER_G: f32 = 0x70 as f32;
const ERASED: u16 = 0x8000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum EepromState {
    Idle,
    Command {
        bits: u8,
        value: u16,
    },
    Reading {
        address: u8,
        bits: u8,
        value: u16,
    },
    Writing {
        target: WriteTarget,
        bits: u8,
        value: u16,
    },
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum WriteTarget {
    Word(u8),
    All,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Eeprom {
    pub words: Vec<u16>,
    writable: bool,
    cs: bool,
    clk: bool,
    di: bool,
    out: bool,
    state: EepromState,
}

impl Default for Eeprom {
    fn default() -> Self {
        Self {
            words: vec![0xFFFF; WORDS],
            writable: false,
            cs: false,
            clk: false,
            di: false,
            out: true,
            state: EepromState::Idle,
        }
    }
}

impl Eeprom {
    pub fn bytes(&self) -> Vec<u8> {
        self.words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }

    pub fn put_bytes(&mut self, data: &[u8]) {
        for (word, pair) in self.words.iter_mut().zip(data.as_chunks::<2>().0) {
            *word = u16::from_le_bytes(*pair);
        }
    }

    fn read_port(&self) -> u8 {
        (u8::from(self.cs) << 7)
            | (u8::from(self.clk) << 6)
            | (u8::from(self.di) << 1)
            | u8::from(self.out)
    }

    fn write_port(&mut self, value: u8) -> bool {
        let cs = value & 0x80 != 0;
        let clk = value & 0x40 != 0;
        let di = value & 0x02 != 0;
        let rising = cs && clk && !self.clk;
        self.di = di;
        self.clk = clk;
        if !cs {
            if self.cs && !matches!(self.state, EepromState::Idle) {
                self.out = true;
            }
            self.cs = false;
            self.state = EepromState::Idle;
            return false;
        }
        self.cs = true;
        if rising { self.clock(di) } else { false }
    }

    fn clock(&mut self, di: bool) -> bool {
        let bit = u16::from(di);
        match self.state {
            EepromState::Idle | EepromState::Done => {
                if di && self.state == EepromState::Idle {
                    self.state = EepromState::Command { bits: 0, value: 0 };
                }
                false
            }
            EepromState::Command { bits, value } => {
                let value = (value << 1) | bit;
                if bits + 1 < 10 {
                    self.state = EepromState::Command {
                        bits: bits + 1,
                        value,
                    };
                    return false;
                }
                self.command(value)
            }
            EepromState::Reading {
                address,
                bits,
                value,
            } => {
                self.out = value & 0x8000 != 0;
                let value = value << 1;
                if bits + 1 < 16 {
                    self.state = EepromState::Reading {
                        address,
                        bits: bits + 1,
                        value,
                    };
                } else {
                    let address = (address + 1) % WORDS as u8;
                    self.state = EepromState::Reading {
                        address,
                        bits: 0,
                        value: self.words[address as usize],
                    };
                }
                false
            }
            EepromState::Writing {
                target,
                bits,
                value,
            } => {
                let value = (value << 1) | bit;
                if bits + 1 < 16 {
                    self.state = EepromState::Writing {
                        target,
                        bits: bits + 1,
                        value,
                    };
                    return false;
                }
                self.state = EepromState::Done;
                self.out = true;
                if !self.writable {
                    return false;
                }
                match target {
                    WriteTarget::Word(address) => self.words[address as usize] = value,
                    WriteTarget::All => self.words.fill(value),
                }
                true
            }
        }
    }

    fn command(&mut self, value: u16) -> bool {
        let address = (value & 0x7F) as u8;
        self.state = EepromState::Done;
        match value >> 8 {
            0b10 => {
                self.out = false;
                self.state = EepromState::Reading {
                    address,
                    bits: 0,
                    value: self.words[address as usize],
                };
                false
            }
            0b01 => {
                self.state = EepromState::Writing {
                    target: WriteTarget::Word(address),
                    bits: 0,
                    value: 0,
                };
                false
            }
            0b11 => {
                self.out = true;
                if self.writable {
                    self.words[address as usize] = 0xFFFF;
                }
                self.writable
            }
            _ => match (value >> 6) & 0b11 {
                0b11 => {
                    self.writable = true;
                    false
                }
                0b00 => {
                    self.writable = false;
                    false
                }
                0b10 => {
                    self.out = true;
                    if self.writable {
                        self.words.fill(0xFFFF);
                    }
                    self.writable
                }
                _ => {
                    self.state = EepromState::Writing {
                        target: WriteTarget::All,
                        bits: 0,
                        value: 0,
                    };
                    false
                }
            },
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mbc7 {
    pub rom_bank_count: usize,
    pub rom_bank_register: u8,
    pub enabled_low: bool,
    pub enabled_high: bool,
    pub tilt: (u16, u16),
    pub latched: (u16, u16),
    pub latch_erased: bool,
    pub eeprom: Eeprom,
}

impl Mbc7 {
    pub fn new(rom_bank_count: usize) -> Self {
        Self {
            rom_bank_count,
            rom_bank_register: 1,
            enabled_low: false,
            enabled_high: false,
            tilt: (CENTRE as u16, CENTRE as u16),
            latched: (ERASED, ERASED),
            latch_erased: false,
            eeprom: Eeprom::default(),
        }
    }

    pub fn set_tilt(&mut self, x: f32, y: f32) {
        let axis = |g: f32| (CENTRE + g.clamp(-4.0, 4.0) * PER_G).round() as u16;
        self.tilt = (axis(-x), axis(y));
    }

    fn enabled(&self) -> bool {
        self.enabled_low && self.enabled_high
    }
}

impl MbcInterface for Mbc7 {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => self.enabled_low = value == 0x0A,
            0x2000..=0x3FFF => self.rom_bank_register = value & 0x7F,
            0x4000..=0x5FFF => self.enabled_high = value == 0x40,
            0xA000..=0xAFFF if self.enabled() => match (addr >> 4) & 0x0F {
                0x0 if value == 0x55 => {
                    self.latched = (ERASED, ERASED);
                    self.latch_erased = true;
                }
                0x1 if value == 0xAA && self.latch_erased => {
                    self.latched = self.tilt;
                    self.latch_erased = false;
                }
                0x8 => return self.eeprom.write_port(value),
                _ => {}
            },
            _ => {}
        }
        false
    }

    fn on_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0xA000..=0xAFFF if self.enabled() => Some(match (addr >> 4) & 0x0F {
                0x2 => self.latched.0 as u8,
                0x3 => (self.latched.0 >> 8) as u8,
                0x4 => self.latched.1 as u8,
                0x5 => (self.latched.1 >> 8) as u8,
                0x6 => 0x00,
                0x8 => self.eeprom.read_port(),
                _ => 0xFF,
            }),
            0xA000..=0xBFFF => Some(0xFF),
            _ => None,
        }
    }

    fn rom_bank_low(&self) -> usize {
        0
    }

    fn rom_bank_high(&self) -> usize {
        mask_bank_number(self.rom_bank_register as usize, self.rom_bank_count)
    }

    fn ram_bank(&self) -> Option<usize> {
        None
    }

    fn soft_reset(&mut self) {
        let eeprom = std::mem::take(&mut self.eeprom);
        let tilt = self.tilt;
        *self = Self::new(self.rom_bank_count);
        self.eeprom = Eeprom {
            words: eeprom.words,
            ..Eeprom::default()
        };
        self.tilt = tilt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cart() -> Mbc7 {
        let mut mbc = Mbc7::new(64);
        mbc.on_write(0x0000, 0x0A);
        mbc.on_write(0x4000, 0x40);
        mbc
    }

    fn send_bits(mbc: &mut Mbc7, bits: &[u8]) {
        for &bit in bits {
            let di = bit << 1;
            mbc.on_write(0xA080, 0x80 | di);
            mbc.on_write(0xA080, 0xC0 | di);
        }
    }

    fn send(mbc: &mut Mbc7, value: u32, count: u8) {
        let bits: Vec<u8> = (0..count).rev().map(|i| ((value >> i) & 1) as u8).collect();
        send_bits(mbc, &bits);
    }

    fn select(mbc: &mut Mbc7) {
        mbc.on_write(0xA080, 0x00);
        mbc.on_write(0xA080, 0x80);
        send(mbc, 1, 1);
    }

    fn deselect(mbc: &mut Mbc7) {
        mbc.on_write(0xA080, 0x00);
    }

    fn command(mbc: &mut Mbc7, opcode: u32, address: u32) {
        select(mbc);
        send(mbc, (opcode << 8) | address, 10);
    }

    fn read_word(mbc: &mut Mbc7, address: u32) -> u16 {
        command(mbc, 0b10, address);
        assert_eq!(mbc.on_read(0xA080).unwrap() & 1, 0);
        let mut value = 0u16;
        for _ in 0..16 {
            send(mbc, 0, 1);
            value = (value << 1) | u16::from(mbc.on_read(0xA080).unwrap() & 1);
        }
        deselect(mbc);
        value
    }

    fn write_word(mbc: &mut Mbc7, address: u32, value: u16) {
        command(mbc, 0b01, address);
        send(mbc, value as u32, 16);
        deselect(mbc);
    }

    fn enable_writes(mbc: &mut Mbc7) {
        command(mbc, 0b00, 0b1100_0000);
        deselect(mbc);
    }

    #[test]
    fn registers_read_ff_until_both_enables_are_set() {
        let mut mbc = Mbc7::new(64);
        assert_eq!(mbc.on_read(0xA020), Some(0xFF));
        mbc.on_write(0x0000, 0x0A);
        assert_eq!(mbc.on_read(0xA020), Some(0xFF));
        mbc.on_write(0x4000, 0x40);
        assert_eq!(mbc.on_read(0xA020), Some(0x00));
        assert_eq!(mbc.on_read(0xA030), Some(0x80));
    }

    #[test]
    fn latching_takes_the_tilt_only_after_an_erase() {
        let mut mbc = cart();
        mbc.set_tilt(1.0, -0.5);
        mbc.on_write(0xA010, 0xAA);
        assert_eq!(mbc.latched, (ERASED, ERASED));
        mbc.on_write(0xA000, 0x55);
        mbc.on_write(0xA010, 0xAA);
        assert_eq!(mbc.latched, (0x81D0 - 0x70, 0x81D0 - 0x38));
        let x = u16::from_le_bytes([mbc.on_read(0xA020).unwrap(), mbc.on_read(0xA030).unwrap()]);
        let y = u16::from_le_bytes([mbc.on_read(0xA040).unwrap(), mbc.on_read(0xA050).unwrap()]);
        assert_eq!((x, y), (0x8160, 0x8198));
    }

    #[test]
    fn registers_ignore_the_low_and_high_address_bits() {
        let mut mbc = cart();
        mbc.on_write(0xA000, 0x55);
        mbc.on_write(0xAF1F, 0xAA);
        assert_eq!(mbc.on_read(0xA72C), Some(0xD0));
        assert_eq!(mbc.on_read(0xA060), Some(0x00));
        assert_eq!(mbc.on_read(0xA070), Some(0xFF));
        assert_eq!(mbc.on_read(0xB000), Some(0xFF));
    }

    #[test]
    fn rom_bank_takes_seven_bits() {
        let mut mbc = Mbc7::new(128);
        mbc.on_write(0x2000, 0xFF);
        assert_eq!(mbc.rom_bank_high(), 0x7F);
    }

    #[test]
    fn eeprom_writes_need_ewen_first() {
        let mut mbc = cart();
        write_word(&mut mbc, 5, 0x1234);
        assert_eq!(read_word(&mut mbc, 5), 0xFFFF);
        enable_writes(&mut mbc);
        write_word(&mut mbc, 5, 0x1234);
        assert_eq!(read_word(&mut mbc, 5), 0x1234);
    }

    #[test]
    fn eeprom_reports_ready_after_a_write() {
        let mut mbc = cart();
        enable_writes(&mut mbc);
        write_word(&mut mbc, 1, 0xBEEF);
        mbc.on_write(0xA080, 0x80);
        assert_eq!(mbc.on_read(0xA080).unwrap() & 1, 1);
    }

    #[test]
    fn eeprom_erase_eral_and_wral() {
        let mut mbc = cart();
        enable_writes(&mut mbc);
        write_word(&mut mbc, 3, 0x0000);
        command(&mut mbc, 0b11, 3);
        deselect(&mut mbc);
        assert_eq!(read_word(&mut mbc, 3), 0xFFFF);
        command(&mut mbc, 0b00, 0b0100_0000);
        send(&mut mbc, 0xA5A5, 16);
        deselect(&mut mbc);
        assert!(mbc.eeprom.words.iter().all(|&w| w == 0xA5A5));
        command(&mut mbc, 0b00, 0b1000_0000);
        deselect(&mut mbc);
        assert!(mbc.eeprom.words.iter().all(|&w| w == 0xFFFF));
    }

    #[test]
    fn ewds_locks_writes_again() {
        let mut mbc = cart();
        enable_writes(&mut mbc);
        command(&mut mbc, 0b00, 0);
        deselect(&mut mbc);
        write_word(&mut mbc, 9, 0x0101);
        assert_eq!(read_word(&mut mbc, 9), 0xFFFF);
    }

    #[test]
    fn eeprom_bytes_round_trip() {
        let mut mbc = cart();
        enable_writes(&mut mbc);
        write_word(&mut mbc, 0, 0x1234);
        write_word(&mut mbc, 127, 0xABCD);
        let bytes = mbc.eeprom.bytes();
        assert_eq!(bytes.len(), EEPROM_SIZE);
        assert_eq!(&bytes[..2], &[0x34, 0x12]);
        let mut other = cart();
        other.eeprom.put_bytes(&bytes);
        assert_eq!(read_word(&mut other, 127), 0xABCD);
    }

    #[test]
    fn soft_reset_keeps_the_eeprom() {
        let mut mbc = cart();
        enable_writes(&mut mbc);
        write_word(&mut mbc, 2, 0x0042);
        mbc.soft_reset();
        assert_eq!(mbc.eeprom.words[2], 0x0042);
        assert!(!mbc.enabled());
    }
}
