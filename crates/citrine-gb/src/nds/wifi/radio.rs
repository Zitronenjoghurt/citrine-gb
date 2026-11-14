#[cfg(test)]
use super::calibration::RadioWrite;
use super::calibration::{Calibration, RadioType};

const REGISTERS: usize = 0x40;
const SERIAL24_INDEX_MASK: u16 = 0x1F;
const SERIAL24_READ: u16 = 1 << 7;
const SERIAL20_WRITE: u16 = 5;
const SERIAL20_READ: u16 = 6;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Radio {
    kind: RadioType,
    registers: Vec<u32>,
}

impl Radio {
    pub fn new(calibration: &Calibration) -> Self {
        let mut registers = vec![0; REGISTERS];
        for write in &calibration.radio_init {
            registers[usize::from(write.index) % REGISTERS] = write.value;
        }
        Self {
            kind: calibration.radio,
            registers,
        }
    }

    pub fn registers(&self) -> &[u32] {
        &self.registers
    }

    pub fn transfer(&mut self, data1: u16, data2: u16) -> (u16, u16) {
        match self.kind {
            RadioType::Serial24 => {
                let index = usize::from((data2 >> 2) & SERIAL24_INDEX_MASK);
                if data2 & SERIAL24_READ != 0 {
                    let value = self.registers[index];
                    (value as u16, (data2 & !3) | ((value >> 16) as u16 & 3))
                } else {
                    self.registers[index] = (u32::from(data2 & 3) << 16) | u32::from(data1);
                    (data1, data2)
                }
            }
            RadioType::Serial20 => {
                let index = usize::from((data1 >> 8) & 0x3F);
                match data2 & 0xF {
                    SERIAL20_WRITE => {
                        self.registers[index] = u32::from(data1 & 0xFF);
                        (data1, data2)
                    }
                    SERIAL20_READ => (
                        (data1 & 0xFF00) | (self.registers[index] as u16 & 0xFF),
                        data2,
                    ),
                    _ => (data1, data2),
                }
            }
        }
    }
}

#[cfg(test)]
pub fn words(kind: RadioType, write: RadioWrite) -> (u16, u16) {
    match kind {
        RadioType::Serial24 => (
            write.value as u16,
            (u16::from(write.index) << 2) | ((write.value >> 16) as u16 & 3),
        ),
        RadioType::Serial20 => (
            (u16::from(write.index) << 8) | (write.value as u16 & 0xFF),
            SERIAL20_WRITE,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nds::spi::firmware::Firmware;

    fn calibration() -> Calibration {
        Calibration::read(Firmware::default().bytes())
    }

    #[test]
    fn start_values_come_from_the_firmware_table() {
        let calibration = calibration();
        let radio = Radio::new(&calibration);
        for write in &calibration.radio_init {
            assert_eq!(radio.registers()[usize::from(write.index)], write.value);
        }
    }

    #[test]
    fn a_serial24_write_carries_eighteen_bits() {
        let mut calibration = calibration();
        calibration.radio = RadioType::Serial24;
        let mut radio = Radio::new(&calibration);
        radio.transfer(0x2345, (9 << 2) | 0x3);
        assert_eq!(radio.registers()[9], 0x3_2345);
        let (low, high) = radio.transfer(0, (9 << 2) | SERIAL24_READ);
        assert_eq!(low, 0x2345);
        assert_eq!(high & 3, 3);
    }

    #[test]
    fn a_serial20_write_carries_one_byte_by_index() {
        let mut calibration = calibration();
        calibration.radio = RadioType::Serial20;
        let mut radio = Radio::new(&calibration);
        radio.transfer(0x1F5A, SERIAL20_WRITE);
        assert_eq!(radio.registers()[0x1F], 0x5A);
        let (data1, _) = radio.transfer(0x1F00, SERIAL20_READ);
        assert_eq!(data1, 0x1F5A);
        radio.transfer(0x5F33, SERIAL20_WRITE);
        assert_eq!(radio.registers()[0x1F], 0x33);
    }
}
