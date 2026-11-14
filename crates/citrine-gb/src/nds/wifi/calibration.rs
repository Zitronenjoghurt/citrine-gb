pub const CHANNELS: usize = 14;
pub const BASEBAND_ENTRIES: usize = 0x69;
pub const REGISTERS: [usize; 16] = [
    0x146, 0x148, 0x14A, 0x14C, 0x120, 0x122, 0x154, 0x144, 0x130, 0x132, 0x140, 0x142, 0x038,
    0x124, 0x128, 0x150,
];

const CONSOLE_TYPE: usize = 0x1D;
const MAC: usize = 0x36;
const RF_TYPE: usize = 0x40;
const RF_BITS: usize = 0x41;
const RF_ENTRIES: usize = 0x42;
const RF_CHANNEL_INDICES: usize = 0x43;
const REGISTER_VALUES: usize = 0x44;
const BASEBAND: usize = 0x64;
const RF_INIT: usize = 0xCE;
const TYPE2_CHANNELS: usize = 0xF2;
const TYPE2_ENTRIES: usize = 12;
const VALUE_BITS: u32 = 18;
const VALUE_MASK: u32 = (1 << VALUE_BITS) - 1;
const DS_LITE: u8 = 0x20;
const RF_UNUSED: usize = 0x3E;
const RF_UNUSED_TAIL: std::ops::Range<usize> = 0x134..0x162;
const TYPE3: u8 = 3;
const TYPE3_BITS: u8 = 0x94;
const TYPE3_BASEBAND_BLOCKS: u8 = 2;
const BUILT_IN_REGISTERS: [u16; 16] = [
    0x0002, 0x0017, 0x0026, 0x1818, 0x0048, 0x4840, 0x0058, 0x0042, 0x0146, 0x8064, 0xE6E6, 0x2443,
    0x000E, 0x0001, 0x0001, 0x0402,
];
const BUILT_IN_BASEBAND: [u8; BASEBAND_ENTRIES] = [
    0x03, 0x17, 0x40, 0x00, 0x1B, 0x6C, 0x48, 0x80, 0x38, 0x00, 0x35, 0x07, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC7, 0xBB, 0x01, 0x24, 0x7F,
    0x5A, 0x01, 0x3F, 0x01, 0x3F, 0x36, 0x1D, 0x00, 0x78, 0x35, 0x55, 0x12, 0x34, 0x1C, 0x00, 0x01,
    0x0E, 0x38, 0x03, 0x70, 0xC5, 0x2A, 0x0A, 0x08, 0x04, 0x01, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFE,
    0xFE, 0xFE, 0xFE, 0xFC, 0xFC, 0xFA, 0xFA, 0xFA, 0xFA, 0xFA, 0xF8, 0xF8, 0xF6, 0x00, 0x12, 0x14,
    0x12, 0x41, 0x23, 0x03, 0x04, 0x70, 0x35, 0x0E, 0x2C, 0x2C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x0E, 0x00, 0x00, 0x12, 0x28, 0x1C,
];
const BUILT_IN_RF: [u8; 0x29] = [
    0x31, 0x4C, 0x4F, 0x21, 0x00, 0x10, 0xB0, 0x08, 0xFA, 0x15, 0x26, 0xE6, 0xC1, 0x01, 0x0E, 0x50,
    0x05, 0x00, 0x6D, 0x12, 0x00, 0x00, 0x01, 0xFF, 0x0E, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x06,
    0x06, 0x00, 0x00, 0x00, 0x18, 0x00, 0x02, 0x00, 0x00,
];
const BUILT_IN_CHANNELS: [u8; 0x3C] = [
    0x1E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0E, 0x0E, 0x0E, 0x0E, 0x0E, 0x0E, 0x0E, 0x16, 0x26,
    0x1C, 0x1C, 0x1C, 0x1D, 0x1D, 0x1D, 0x1E, 0x1E, 0x1E, 0x1E, 0x1F, 0x1E, 0x1F, 0x18, 0x01, 0x4B,
    0x4B, 0x4B, 0x4B, 0x4C, 0x4C, 0x4C, 0x4C, 0x4C, 0x4C, 0x4C, 0x4D, 0x4D, 0x4D, 0x02, 0x6C, 0x71,
    0x76, 0x5B, 0x40, 0x45, 0x4A, 0x2F, 0x34, 0x39, 0x3E, 0x03, 0x08, 0x14,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RadioType {
    Serial24,
    Serial20,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RadioWrite {
    pub index: u8,
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Calibration {
    pub lite: bool,
    pub mac: [u8; 6],
    pub registers: [u16; 16],
    pub baseband: Vec<u8>,
    pub radio: RadioType,
    pub radio_bits: u8,
    pub radio_init: Vec<RadioWrite>,
    pub channels: Vec<[RadioWrite; 2]>,
}

impl Calibration {
    pub fn read(firmware: &[u8]) -> Self {
        let byte = |offset: usize| firmware.get(offset).copied().unwrap_or(0xFF);
        let half = |offset: usize| u16::from_le_bytes([byte(offset), byte(offset + 1)]);
        let radio = if byte(RF_TYPE) == 3 {
            RadioType::Serial20
        } else {
            RadioType::Serial24
        };
        let (radio_init, channels) = match radio {
            RadioType::Serial24 => serial24_tables(&byte),
            RadioType::Serial20 => serial20_tables(&byte),
        };
        Self {
            lite: byte(CONSOLE_TYPE) == DS_LITE,
            mac: std::array::from_fn(|index| byte(MAC + index)),
            registers: std::array::from_fn(|index| half(REGISTER_VALUES + 2 * index)),
            baseband: (0..BASEBAND_ENTRIES)
                .map(|index| byte(BASEBAND + index))
                .collect(),
            radio,
            radio_bits: byte(RF_BITS),
            radio_init,
            channels,
        }
    }

    pub fn channel_of(&self, radio: &[u32]) -> u8 {
        self.channels
            .iter()
            .position(|watched| {
                watched
                    .iter()
                    .all(|write| radio.get(usize::from(write.index)) == Some(&write.value))
            })
            .map_or(0, |index| index as u8 + 1)
    }

    pub fn write_built_in(image: &mut [u8]) {
        image[RF_UNUSED..RF_UNUSED + 2].fill(0xFF);
        image[RF_TYPE] = TYPE3;
        image[RF_BITS] = TYPE3_BITS;
        image[RF_ENTRIES] = BUILT_IN_RF.len() as u8;
        image[RF_CHANNEL_INDICES] = 2;
        for (index, value) in BUILT_IN_REGISTERS.iter().enumerate() {
            let at = REGISTER_VALUES + 2 * index;
            image[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        image[BASEBAND..BASEBAND + BASEBAND_ENTRIES].copy_from_slice(&BUILT_IN_BASEBAND);
        image[BASEBAND + BASEBAND_ENTRIES] = 0;
        let rf_end = RF_INIT + BUILT_IN_RF.len();
        image[RF_INIT..rf_end].copy_from_slice(&BUILT_IN_RF);
        image[rf_end] = TYPE3_BASEBAND_BLOCKS;
        image[rf_end + 1..rf_end + 1 + BUILT_IN_CHANNELS.len()].copy_from_slice(&BUILT_IN_CHANNELS);
        image[RF_UNUSED_TAIL].fill(0xFF);
    }
}

fn word(byte: &impl Fn(usize) -> u8, at: usize) -> RadioWrite {
    let raw = u32::from_le_bytes([byte(at), byte(at + 1), byte(at + 2), 0]);
    RadioWrite {
        index: (raw >> VALUE_BITS) as u8 & 0x3F,
        value: raw & VALUE_MASK,
    }
}

fn serial24_tables(byte: &impl Fn(usize) -> u8) -> (Vec<RadioWrite>, Vec<[RadioWrite; 2]>) {
    let entries = usize::from(byte(RF_ENTRIES)).min(TYPE2_ENTRIES);
    let init = (0..entries)
        .map(|entry| word(byte, RF_INIT + 3 * entry))
        .collect();
    let channels = (0..CHANNELS)
        .map(|channel| {
            let at = TYPE2_CHANNELS + 6 * channel;
            [word(byte, at), word(byte, at + 3)]
        })
        .collect();
    (init, channels)
}

fn serial20_tables(byte: &impl Fn(usize) -> u8) -> (Vec<RadioWrite>, Vec<[RadioWrite; 2]>) {
    let entries = usize::from(byte(RF_ENTRIES));
    let init = (0..entries)
        .map(|index| RadioWrite {
            index: index as u8,
            value: u32::from(byte(RF_INIT + index)),
        })
        .collect();
    let baseband_blocks = usize::from(byte(RF_INIT + entries));
    let radio_blocks = RF_INIT + entries + 1 + baseband_blocks * (CHANNELS + 1);
    let block = |number: usize| radio_blocks + number * (CHANNELS + 1);
    let channels = (0..CHANNELS)
        .map(|channel| {
            std::array::from_fn(|number| RadioWrite {
                index: byte(block(number)),
                value: u32::from(byte(block(number) + 1 + channel)),
            })
        })
        .collect();
    (init, channels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nds::spi::firmware::Firmware;

    fn built_in() -> Calibration {
        Calibration::read(Firmware::default().bytes())
    }

    #[test]
    fn the_built_in_firmware_has_melonds_type3_table() {
        let calibration = built_in();
        assert_eq!(calibration.radio, RadioType::Serial20);
        assert_eq!(calibration.radio_bits, 0x94);
        assert_eq!(calibration.radio_init.len(), 0x29);
        assert_eq!(calibration.radio_init[0].value, 0x31);
        assert_eq!(calibration.radio_init[0x28].value, 0x00);
        assert_eq!(calibration.mac[..3], [0x00, 0x09, 0xBF]);
        assert!(!calibration.lite);
    }

    #[test]
    fn the_built_in_baseband_table_carries_melonds_start_values() {
        let calibration = built_in();
        assert_eq!(calibration.baseband.len(), 0x69);
        assert_eq!(calibration.baseband[0x01], 0x17);
        assert_eq!(calibration.baseband[0x1E], 0x24);
        assert_eq!(calibration.baseband[0x35], 0x2A);
    }

    #[test]
    fn the_register_table_follows_gbatek_offsets() {
        let calibration = built_in();
        assert_eq!(calibration.registers[3], 0x1818);
        assert_eq!(calibration.registers[12], 0x000E);
        assert_eq!(REGISTERS[12], 0x038);
    }

    #[test]
    fn the_built_in_image_matches_melonds_from_0x3e_to_0x161() {
        let image = Firmware::default().bytes().to_vec();
        assert_eq!(image[0x3E..0x40], [0xFF, 0xFF]);
        assert_eq!(image[0x40..0x44], [3, 0x94, 0x29, 2]);
        assert_eq!(image[0xCD], 0);
        assert_eq!(image[0xF7], 2);
        assert_eq!(image[0xF8], 0x1E);
        assert_eq!(image[0x133], 0x14);
        assert!(image[0x134..0x162].iter().all(|&byte| byte == 0xFF));
    }

    #[test]
    fn each_channel_has_its_own_pair_of_rf_writes() {
        let calibration = built_in();
        let mut radio = vec![0; 64];
        for channel in 1..=14u8 {
            for write in calibration.channels[usize::from(channel) - 1] {
                radio[usize::from(write.index)] = write.value;
            }
            assert_eq!(calibration.channel_of(&radio), channel);
        }
        radio[1] = 0;
        assert_eq!(calibration.channel_of(&radio), 0);
    }

    #[test]
    fn a_serial20_table_reads_the_rf_blocks_after_the_baseband_blocks() {
        let mut image = vec![0xFF; 0x200];
        image[RF_TYPE] = 3;
        image[RF_ENTRIES] = 0x29;
        image[0xF7] = 2;
        image[0x116] = 0x01;
        image[0x125] = 0x02;
        for channel in 0..14 {
            image[0x117 + channel] = 0x40 + channel as u8;
            image[0x126 + channel] = 0x60 + channel as u8;
        }
        let calibration = Calibration::read(&image);
        assert_eq!(calibration.radio, RadioType::Serial20);
        assert_eq!(calibration.radio_init.len(), 0x29);
        let ch7 = calibration.channels[6];
        assert_eq!((ch7[0].index, ch7[0].value), (1, 0x46));
        assert_eq!((ch7[1].index, ch7[1].value), (2, 0x66));
    }
}
