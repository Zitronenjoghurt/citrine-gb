use super::calibration::Calibration;

const REGISTERS: usize = 0x100;
const CHIP_ID: u8 = 0x6D;
const FIXED: [(u8, u8); 4] = [(0x00, CHIP_ID), (0x4D, 0x00), (0x5D, 0x01), (0x64, 0xFF)];
const READ_ZERO: [(u8, u8); 6] = [
    (0x0D, 0x12),
    (0x16, 0x1A),
    (0x27, 0x27),
    (0x5E, 0x61),
    (0x66, 0x66),
    (0x69, 0xFF),
];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Baseband {
    registers: Vec<u8>,
}

impl Baseband {
    pub fn new(calibration: &Calibration) -> Self {
        let mut baseband = Self {
            registers: vec![0; REGISTERS],
        };
        for (index, &value) in calibration.baseband.iter().enumerate() {
            baseband.write(index as u8, value);
        }
        baseband
    }

    pub fn read(&self, index: u8) -> u8 {
        if let Some(&(_, value)) = FIXED.iter().find(|&&(at, _)| at == index) {
            return value;
        }
        if read_zero(index) {
            return 0;
        }
        self.registers[usize::from(index)]
    }

    pub fn write(&mut self, index: u8, value: u8) {
        if !read_zero(index) && FIXED.iter().all(|&(at, _)| at != index) {
            self.registers[usize::from(index)] = value;
        }
    }
}

fn read_zero(index: u8) -> bool {
    READ_ZERO
        .iter()
        .any(|&(first, last)| (first..=last).contains(&index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nds::spi::firmware::Firmware;

    fn baseband() -> Baseband {
        Baseband::new(&Calibration::read(Firmware::default().bytes()))
    }

    #[test]
    fn register_zero_is_the_mm3155_chip_id() {
        let mut baseband = baseband();
        baseband.write(0x00, 0x03);
        assert_eq!(baseband.read(0x00), 0x6D);
        assert_eq!(baseband.read(0x5D), 0x01);
        assert_eq!(baseband.read(0x64), 0xFF);
    }

    #[test]
    fn start_values_come_from_the_firmware_table() {
        let baseband = baseband();
        assert_eq!(baseband.read(0x01), 0x17);
        assert_eq!(baseband.read(0x1E), 0x24);
        assert_eq!(baseband.read(0x35), 0x2A);
    }

    #[test]
    fn unused_registers_read_zero_and_ignore_writes() {
        let mut baseband = baseband();
        for index in [0x0D, 0x12, 0x16, 0x1A, 0x27, 0x5E, 0x66, 0x69, 0xFF] {
            baseband.write(index, 0x5A);
            assert_eq!(baseband.read(index), 0, "{index:#x}");
        }
        for index in [0x01, 0x13, 0x28, 0x4E, 0x62, 0x65, 0x68] {
            baseband.write(index, 0x5A);
            assert_eq!(baseband.read(index), 0x5A, "{index:#x}");
        }
    }
}
