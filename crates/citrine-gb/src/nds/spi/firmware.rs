pub mod online;

use crate::nds::save::{Backup, SaveType};
use crate::nds::wifi::calibration::Calibration as WifiCalibration;
use crate::utils::crc16::crc16;

pub const FIRMWARE_BYTES: usize = 0x4_0000;
pub const USER_SETTINGS_BYTES: usize = 0x74;

const CONSOLE_TYPE: usize = 0x1D;
const CHIP_SIZE: usize = 0x14;
const ID: usize = 0x08;
const USER_SETTINGS_OFFSET: usize = 0x20;
const WIFI_CRC: usize = 0x2A;
const WIFI_LENGTH: usize = 0x2C;
const WIFI_CONFIG_BYTES: usize = 0x138;
const WIFI_VERSION: usize = 0x2F;
const MAC_ADDRESS: usize = 0x36;
const CHANNEL_MASK: usize = 0x3C;
const DEFAULT_MAC: [u8; 6] = [0x00, 0x09, 0xBF, 0x43, 0x47, 0x42];
const ORIGINAL_DS: u8 = 0xFF;
const COPY_BYTES: usize = 0x100;

const VERSION: usize = 0x00;
const COLOR: usize = 0x02;
const BIRTH_MONTH: usize = 0x03;
const BIRTH_DAY: usize = 0x04;
const NICKNAME: usize = 0x06;
const NICKNAME_LENGTH: usize = 0x1A;
const MESSAGE: usize = 0x1C;
const MESSAGE_LENGTH: usize = 0x50;
const ALARM_HOUR: usize = 0x52;
const ALARM_MINUTE: usize = 0x53;
const CALIBRATION: usize = 0x58;
const LANGUAGE_FLAGS: usize = 0x64;
const YEAR: usize = 0x66;
const UPDATE_COUNTER: usize = 0x70;
const SETTINGS_CRC: usize = 0x72;
const CRC_BYTES: usize = 0x70;
const NICKNAME_CHARS: usize = 10;
const MESSAGE_CHARS: usize = 26;
const SETTINGS_VERSION: u16 = 5;
const SETTINGS_OK: u16 = 0xEC00;
const BACKLIGHT_SHIFT: u16 = 4;
const COUNTER_MASK: u16 = 0x7F;
const LANGUAGE_MASK: u16 = 0x07;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Language {
    Japanese,
    English,
    French,
    German,
    Italian,
    Spanish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Calibration {
    pub adc1: (u16, u16),
    pub screen1: (u8, u8),
    pub adc2: (u16, u16),
    pub screen2: (u8, u8),
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            adc1: (0x200, 0x200),
            screen1: (0x21, 0x21),
            adc2: (0xE00, 0xA00),
            screen2: (0xE1, 0xA1),
        }
    }
}

impl Calibration {
    fn from_settings(settings: &[u8]) -> Self {
        let half = |offset: usize| u16::from_le_bytes([settings[offset], settings[offset + 1]]);
        Self {
            adc1: (half(CALIBRATION), half(CALIBRATION + 2)),
            screen1: (settings[CALIBRATION + 4], settings[CALIBRATION + 5]),
            adc2: (half(CALIBRATION + 6), half(CALIBRATION + 8)),
            screen2: (settings[CALIBRATION + 10], settings[CALIBRATION + 11]),
        }
    }

    fn write(&self, settings: &mut [u8]) {
        settings[CALIBRATION..CALIBRATION + 2].copy_from_slice(&self.adc1.0.to_le_bytes());
        settings[CALIBRATION + 2..CALIBRATION + 4].copy_from_slice(&self.adc1.1.to_le_bytes());
        settings[CALIBRATION + 4] = self.screen1.0;
        settings[CALIBRATION + 5] = self.screen1.1;
        settings[CALIBRATION + 6..CALIBRATION + 8].copy_from_slice(&self.adc2.0.to_le_bytes());
        settings[CALIBRATION + 8..CALIBRATION + 10].copy_from_slice(&self.adc2.1.to_le_bytes());
        settings[CALIBRATION + 10] = self.screen2.0;
        settings[CALIBRATION + 11] = self.screen2.1;
    }

    pub fn to_adc(&self, x: u8, y: u8) -> (u16, u16) {
        let axis = |pixel: u8, screen1: u8, screen2: u8, adc1: u16, adc2: u16| {
            let span = i32::from(screen2) - i32::from(screen1);
            if span == 0 {
                return adc1;
            }
            let pixel = i32::from(pixel) - (i32::from(screen1) - 1);
            let adc = pixel * (i32::from(adc2) - i32::from(adc1)) / span + i32::from(adc1);
            adc.clamp(0, 0xFFF) as u16
        };
        (
            axis(x, self.screen1.0, self.screen2.0, self.adc1.0, self.adc2.0),
            axis(y, self.screen1.1, self.screen2.1, self.adc1.1, self.adc2.1),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UserSettings {
    pub nickname: String,
    pub message: String,
    pub favorite_color: u8,
    pub birthday: (u8, u8),
    pub language: Language,
    pub alarm: (u8, u8),
    pub backlight: u8,
    pub year: u16,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            nickname: "Citrine".into(),
            message: String::new(),
            favorite_color: 0,
            birthday: (1, 1),
            language: Language::English,
            alarm: (0, 0),
            backlight: 3,
            year: 2026,
        }
    }
}

impl UserSettings {
    fn to_bytes(&self, counter: u16) -> [u8; USER_SETTINGS_BYTES] {
        let mut bytes = [0; USER_SETTINGS_BYTES];
        bytes[VERSION..VERSION + 2].copy_from_slice(&SETTINGS_VERSION.to_le_bytes());
        let length = write_utf16(&mut bytes[MESSAGE..], &self.message, MESSAGE_CHARS);
        bytes[MESSAGE_LENGTH] = length;
        bytes[ALARM_HOUR] = self.alarm.0;
        bytes[ALARM_MINUTE] = self.alarm.1;
        Calibration::default().write(&mut bytes);
        let flags = u16::from(self.backlight & 3) << BACKLIGHT_SHIFT | SETTINGS_OK;
        bytes[LANGUAGE_FLAGS..LANGUAGE_FLAGS + 2].copy_from_slice(&flags.to_le_bytes());
        bytes[YEAR] = self.year.saturating_sub(2000).min(255) as u8;
        self.write_profile(&mut bytes, counter);
        bytes
    }

    fn write_profile(&self, bytes: &mut [u8], counter: u16) {
        bytes[COLOR] = self.favorite_color & 0x0F;
        bytes[BIRTH_MONTH] = self.birthday.0;
        bytes[BIRTH_DAY] = self.birthday.1;
        bytes[NICKNAME..NICKNAME_LENGTH].fill(0);
        let length = write_utf16(&mut bytes[NICKNAME..], &self.nickname, NICKNAME_CHARS);
        bytes[NICKNAME_LENGTH] = length;
        let flags = u16::from_le_bytes([bytes[LANGUAGE_FLAGS], bytes[LANGUAGE_FLAGS + 1]]);
        let flags = flags & !LANGUAGE_MASK | self.language as u16;
        bytes[LANGUAGE_FLAGS..LANGUAGE_FLAGS + 2].copy_from_slice(&flags.to_le_bytes());
        bytes[UPDATE_COUNTER..UPDATE_COUNTER + 2].copy_from_slice(&counter.to_le_bytes());
        let crc = crc16(0xFFFF, &bytes[..CRC_BYTES]);
        bytes[SETTINGS_CRC..SETTINGS_CRC + 2].copy_from_slice(&crc.to_le_bytes());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Firmware {
    flash: Backup,
}

impl Default for Firmware {
    fn default() -> Self {
        Self::built_in(&UserSettings::default())
    }
}

impl Firmware {
    pub fn new(image: &[u8]) -> Self {
        let kind = if image.len() > FIRMWARE_BYTES {
            SaveType::Flash512K
        } else {
            SaveType::Flash256K
        };
        let mut flash = Backup::new(kind);
        flash.load(image);
        Self { flash }
    }

    pub fn built_in(settings: &UserSettings) -> Self {
        let mut image = vec![0xFF; FIRMWARE_BYTES];
        image[ID..ID + 4].copy_from_slice(b"MACP");
        image[CHIP_SIZE..CHIP_SIZE + 2].copy_from_slice(&(2u16 << 12).to_le_bytes());
        image[CONSOLE_TYPE] = ORIGINAL_DS;
        let offset = (settings_base(FIRMWARE_BYTES) / 8) as u16;
        image[USER_SETTINGS_OFFSET..USER_SETTINGS_OFFSET + 2]
            .copy_from_slice(&offset.to_le_bytes());
        write_wifi(&mut image);
        online::write_empty_slots(&mut image);
        for copy in 0..2 {
            let start = settings_base(FIRMWARE_BYTES) + copy * COPY_BYTES;
            let bytes = settings.to_bytes(copy as u16);
            image[start..start + USER_SETTINGS_BYTES].copy_from_slice(&bytes);
        }
        Self::new(&image)
    }

    pub fn with_user_settings(image: &[u8], settings: &UserSettings) -> Self {
        let mut image = Self::new(image).bytes().to_vec();
        let base = settings_base(image.len());
        image[USER_SETTINGS_OFFSET..USER_SETTINGS_OFFSET + 2]
            .copy_from_slice(&((base / 8) as u16).to_le_bytes());
        let (current, counter) = match current_settings(&image) {
            Some(bytes) => (bytes.to_vec(), counter(bytes)),
            None => (settings.to_bytes(0).to_vec(), COUNTER_MASK),
        };
        for copy in 0..2 {
            let mut bytes = current.clone();
            settings.write_profile(&mut bytes, (counter + 1 + copy as u16) & COUNTER_MASK);
            let start = base + copy * COPY_BYTES;
            image[start..start + USER_SETTINGS_BYTES].copy_from_slice(&bytes);
        }
        Self::new(&image)
    }

    pub fn bytes(&self) -> &[u8] {
        self.flash.bytes()
    }

    pub fn dirty(&self) -> bool {
        self.flash.dirty()
    }

    pub fn clear_dirty(&mut self) {
        self.flash.clear_dirty();
    }

    pub fn transfer(&mut self, byte: u8) -> u8 {
        self.flash.transfer(byte)
    }

    pub fn deselect(&mut self) {
        self.flash.deselect();
    }

    pub fn user_settings(&self) -> Option<&[u8]> {
        current_settings(self.bytes())
    }

    pub fn mac(&self) -> [u8; 6] {
        online::mac(self.bytes())
    }

    pub fn nickname(&self) -> String {
        self.user_settings().map_or_else(String::new, |bytes| {
            let length = usize::from(bytes[NICKNAME_LENGTH]).min(NICKNAME_CHARS);
            let units: Vec<u16> = bytes[NICKNAME..NICKNAME + 2 * length]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            String::from_utf16_lossy(&units)
        })
    }

    pub fn favorite_color(&self) -> u8 {
        self.user_settings().map_or(0, |bytes| bytes[COLOR] & 0x0F)
    }

    pub fn connections(&self) -> &[u8] {
        online::connections(self.bytes())
    }

    pub fn calibration(&self) -> Calibration {
        self.user_settings()
            .map_or_else(Calibration::default, Calibration::from_settings)
    }
}

fn settings_base(size: usize) -> usize {
    size - 2 * COPY_BYTES
}

fn counter(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[UPDATE_COUNTER], bytes[UPDATE_COUNTER + 1]])
}

fn current_settings(image: &[u8]) -> Option<&[u8]> {
    let base = settings_base(image.len());
    let copy = |index: usize| {
        let start = base + index * COPY_BYTES;
        let bytes = &image[start..start + USER_SETTINGS_BYTES];
        let crc = u16::from_le_bytes([bytes[SETTINGS_CRC], bytes[SETTINGS_CRC + 1]]);
        (crc16(0xFFFF, &bytes[..CRC_BYTES]) == crc).then_some(bytes)
    };
    match (copy(0), copy(1)) {
        (Some(first), Some(second)) => {
            if counter(second) & COUNTER_MASK == (counter(first) + 1) & COUNTER_MASK {
                Some(second)
            } else {
                Some(first)
            }
        }
        (first, second) => first.or(second),
    }
}

fn write_wifi(image: &mut [u8]) {
    image[WIFI_LENGTH..WIFI_LENGTH + 2].copy_from_slice(&(WIFI_CONFIG_BYTES as u16).to_le_bytes());
    image[WIFI_VERSION] = 5;
    image[MAC_ADDRESS..MAC_ADDRESS + 6].copy_from_slice(&DEFAULT_MAC);
    image[CHANNEL_MASK..CHANNEL_MASK + 2].copy_from_slice(&0x3FFEu16.to_le_bytes());
    WifiCalibration::write_built_in(image);
    let crc = crc16(0, &image[WIFI_LENGTH..WIFI_LENGTH + WIFI_CONFIG_BYTES]);
    image[WIFI_CRC..WIFI_CRC + 2].copy_from_slice(&crc.to_le_bytes());
}

fn write_utf16(bytes: &mut [u8], text: &str, limit: usize) -> u8 {
    let mut length = 0;
    for (index, unit) in text.encode_utf16().take(limit).enumerate() {
        bytes[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        length += 1;
    }
    length
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(firmware: &mut Firmware, bytes: &[u8]) -> Vec<u8> {
        let replies = bytes.iter().map(|&byte| firmware.transfer(byte)).collect();
        firmware.deselect();
        replies
    }

    fn read(firmware: &mut Firmware, address: u32, length: usize) -> Vec<u8> {
        let [_, high, middle, low] = address.to_be_bytes();
        let mut bytes = vec![0x03, high, middle, low];
        bytes.resize(4 + length, 0);
        command(firmware, &bytes)[4..].to_vec()
    }

    fn settings_at(firmware: &Firmware, copy: usize) -> &[u8] {
        let start = settings_base(FIRMWARE_BYTES) + copy * COPY_BYTES;
        &firmware.bytes()[start..start + USER_SETTINGS_BYTES]
    }

    #[test]
    fn reads_return_the_image_from_any_address() {
        let image: Vec<u8> = (0..FIRMWARE_BYTES).map(|index| (index / 3) as u8).collect();
        let mut firmware = Firmware::new(&image);
        assert_eq!(read(&mut firmware, 0x1_2345, 16), image[0x1_2345..0x1_2355]);
        assert_eq!(
            read(&mut firmware, 0x3_FFFE, 4),
            [image[0x3_FFFE], image[0x3_FFFF], image[0], image[1]]
        );
    }

    #[test]
    fn the_chip_reads_as_an_st_m45pe20() {
        let mut firmware = Firmware::default();
        assert_eq!(
            command(&mut firmware, &[0x9F, 0, 0, 0])[1..],
            [0x20, 0x40, 0x12]
        );
    }

    #[test]
    fn status_shows_the_write_enable_latch() {
        let mut firmware = Firmware::default();
        assert_eq!(command(&mut firmware, &[0x05, 0, 0]), [0xFF, 0, 0]);
        command(&mut firmware, &[0x06]);
        assert_eq!(command(&mut firmware, &[0x05, 0])[1], 0x02);
        command(&mut firmware, &[0x04]);
        assert_eq!(command(&mut firmware, &[0x05, 0])[1], 0);
    }

    #[test]
    fn page_write_replaces_bytes_and_needs_write_enable() {
        let mut firmware = Firmware::default();
        command(&mut firmware, &[0x0A, 0x03, 0xFE, 0x00, 0x12, 0x34]);
        assert_ne!(read(&mut firmware, 0x3_FE00, 2), [0x12, 0x34]);
        command(&mut firmware, &[0x06]);
        command(&mut firmware, &[0x0A, 0x03, 0xFE, 0x00, 0x12, 0x34]);
        assert_eq!(read(&mut firmware, 0x3_FE00, 2), [0x12, 0x34]);
        assert!(firmware.dirty());
        assert_eq!(command(&mut firmware, &[0x05, 0])[1], 0);
    }

    #[test]
    fn page_program_only_clears_bits() {
        let mut firmware = Firmware::new(&[0xF0; FIRMWARE_BYTES]);
        command(&mut firmware, &[0x06]);
        command(&mut firmware, &[0x02, 0, 0x10, 0, 0x3C]);
        assert_eq!(read(&mut firmware, 0x1000, 1), [0x30]);
    }

    #[test]
    fn page_and_sector_erase_fill_with_ff() {
        let mut firmware = Firmware::new(&[0; FIRMWARE_BYTES]);
        command(&mut firmware, &[0x06]);
        command(&mut firmware, &[0xDB, 0, 0x01, 0x80]);
        assert_eq!(read(&mut firmware, 0x100, 0x100), [0xFF; 0x100]);
        assert_eq!(read(&mut firmware, 0x200, 1), [0]);
        command(&mut firmware, &[0x06]);
        command(&mut firmware, &[0xD8, 0x02, 0x34, 0x56]);
        assert!(
            read(&mut firmware, 0x2_0000, 0x1_0000)
                .iter()
                .all(|&byte| byte == 0xFF)
        );
        assert_eq!(read(&mut firmware, 0x1_FFFF, 1), [0]);
        assert_eq!(read(&mut firmware, 0x3_0000, 1), [0]);
    }

    #[test]
    fn deep_power_down_ignores_reads_until_release() {
        let mut firmware = Firmware::default();
        command(&mut firmware, &[0xB9]);
        assert_eq!(command(&mut firmware, &[0x9F, 0])[1], 0xFF);
        command(&mut firmware, &[0xAB]);
        assert_eq!(command(&mut firmware, &[0x9F, 0])[1], 0x20);
    }

    #[test]
    fn the_built_in_header_points_at_the_user_settings() {
        let firmware = Firmware::default();
        let bytes = firmware.bytes();
        assert_eq!(&bytes[ID..ID + 3], b"MAC");
        assert_eq!(bytes[CONSOLE_TYPE], ORIGINAL_DS);
        assert_eq!(u16::from_le_bytes([bytes[0x20], bytes[0x21]]), 0x7FC0);
        assert_eq!(u16::from_le_bytes([bytes[0x14], bytes[0x15]]) >> 12, 2);
        let crc = u16::from_le_bytes([bytes[WIFI_CRC], bytes[WIFI_CRC + 1]]);
        assert_eq!(crc, crc16(0, &bytes[0x2C..0x2C + 0x138]));
        assert_eq!(bytes[MAC_ADDRESS..MAC_ADDRESS + 3], [0x00, 0x09, 0xBF]);
    }

    #[test]
    fn built_in_settings_hold_the_user_fields() {
        let settings = UserSettings {
            nickname: "Clémentine Long".into(),
            message: "hi".into(),
            favorite_color: 11,
            birthday: (12, 24),
            language: Language::German,
            alarm: (7, 30),
            backlight: 2,
            year: 2026,
        };
        let firmware = Firmware::built_in(&settings);
        let bytes = firmware.user_settings().unwrap();
        assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]), 5);
        assert_eq!(bytes[COLOR], 11);
        assert_eq!(bytes[BIRTH_MONTH..=BIRTH_DAY], [12, 24]);
        assert_eq!(bytes[NICKNAME_LENGTH], 10);
        let nickname: Vec<u16> = bytes[NICKNAME..NICKNAME + 20]
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(String::from_utf16(&nickname).unwrap(), "Clémentine");
        assert_eq!(bytes[MESSAGE_LENGTH], 2);
        assert_eq!(bytes[ALARM_HOUR..=ALARM_MINUTE], [7, 30]);
        let flags = u16::from_le_bytes([bytes[LANGUAGE_FLAGS], bytes[LANGUAGE_FLAGS + 1]]);
        assert_eq!(flags & 7, 3);
        assert_eq!(flags >> 4 & 3, 2);
        assert_eq!(flags & 1 << 9, 0);
        assert_eq!(flags & 0xEC00, 0xEC00);
        assert_eq!(bytes[YEAR], 26);
    }

    #[test]
    fn the_newer_copy_wins_by_its_counter() {
        let firmware = Firmware::default();
        assert_eq!(firmware.user_settings().unwrap(), settings_at(&firmware, 1));
        let mut image = firmware.bytes().to_vec();
        let second = settings_base(FIRMWARE_BYTES) + COPY_BYTES;
        image[second + UPDATE_COUNTER] = 5;
        let crc = crc16(0xFFFF, &image[second..second + CRC_BYTES]);
        image[second + SETTINGS_CRC..second + SETTINGS_CRC + 2].copy_from_slice(&crc.to_le_bytes());
        let firmware = Firmware::new(&image);
        assert_eq!(firmware.user_settings().unwrap(), settings_at(&firmware, 0));
    }

    #[test]
    fn the_counter_wraps_at_7f() {
        let mut image = Firmware::default().bytes().to_vec();
        for (copy, counter) in [(0, 0x7Fu8), (1, 0)] {
            let start = settings_base(FIRMWARE_BYTES) + copy * COPY_BYTES;
            image[start + UPDATE_COUNTER] = counter;
            let crc = crc16(0xFFFF, &image[start..start + CRC_BYTES]);
            image[start + SETTINGS_CRC..start + SETTINGS_CRC + 2]
                .copy_from_slice(&crc.to_le_bytes());
        }
        let firmware = Firmware::new(&image);
        assert_eq!(firmware.user_settings().unwrap(), settings_at(&firmware, 1));
    }

    #[test]
    fn a_bad_crc_falls_back_to_the_other_copy() {
        let mut image = Firmware::default().bytes().to_vec();
        image[settings_base(FIRMWARE_BYTES) + COPY_BYTES + COLOR] ^= 1;
        let firmware = Firmware::new(&image);
        assert_eq!(firmware.user_settings().unwrap(), settings_at(&firmware, 0));
        let blank = Firmware::new(&[0xFF; FIRMWARE_BYTES]);
        assert!(blank.user_settings().is_none());
        assert_eq!(blank.calibration(), Calibration::default());
    }

    #[test]
    fn default_calibration_maps_pixels_to_sixteen_steps() {
        let calibration = Firmware::default().calibration();
        assert_eq!(calibration, Calibration::default());
        assert_eq!(calibration.to_adc(0, 0), (0, 0));
        assert_eq!(calibration.to_adc(128, 96), (0x800, 0x600));
        assert_eq!(calibration.to_adc(255, 191), (0xFF0, 0xBF0));
    }

    #[test]
    fn calibration_follows_the_firmware_points() {
        let calibration = Calibration {
            adc1: (0x300, 0x100),
            screen1: (0x11, 0x11),
            adc2: (0xB00, 0x900),
            screen2: (0x91, 0x91),
        };
        assert_eq!(calibration.to_adc(0x10, 0x10), (0x300, 0x100));
        assert_eq!(calibration.to_adc(0x90, 0x90), (0xB00, 0x900));
        assert_eq!(calibration.to_adc(0x50, 0x50), (0x700, 0x500));
    }

    fn modbus_crc(bytes: &[u8]) -> u16 {
        let mut crc = 0xFFFFu16;
        for &byte in bytes {
            crc ^= u16::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    crc >> 1 ^ 0xA001
                } else {
                    crc >> 1
                };
            }
        }
        crc
    }

    fn half(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([bytes[at], bytes[at + 1]])
    }

    fn nickname(bytes: &[u8]) -> String {
        let units: Vec<u16> = (0..usize::from(bytes[0x1A]))
            .map(|index| half(bytes, 0x06 + 2 * index))
            .collect();
        String::from_utf16(&units).unwrap()
    }

    fn dump_shaped(counter: u8) -> Vec<u8> {
        let mut image: Vec<u8> = (0..FIRMWARE_BYTES)
            .map(|index| (index * 7 + index / 251) as u8)
            .collect();
        image[0x20..0x22].copy_from_slice(&0x7FC0u16.to_le_bytes());
        let block = 0x3_FE00;
        let mut settings = image[block..block + 0x74].to_vec();
        settings[0x00..0x02].copy_from_slice(&5u16.to_le_bytes());
        settings[0x1A] = 4;
        settings[0x06..0x0E].copy_from_slice(&[b'D', 0, b'u', 0, b'm', 0, b'p', 0]);
        settings[0x64..0x66].copy_from_slice(&0xFC35u16.to_le_bytes());
        settings[0x70..0x72].copy_from_slice(&u16::from(counter).to_le_bytes());
        let crc = modbus_crc(&settings[..0x70]);
        settings[0x72..0x74].copy_from_slice(&crc.to_le_bytes());
        image[block..block + 0x74].copy_from_slice(&settings);
        image
    }

    fn profile() -> UserSettings {
        UserSettings {
            nickname: "Zitrone".into(),
            favorite_color: 9,
            birthday: (3, 14),
            language: Language::French,
            ..UserSettings::default()
        }
    }

    #[test]
    fn the_firmware_crc_is_the_bios_crc16_with_ffff() {
        assert_eq!(crc16(0xFFFF, b"123456789"), 0x4B37);
        let bytes = Firmware::built_in(&profile())
            .user_settings()
            .unwrap()
            .to_vec();
        assert_eq!(half(&bytes, 0x72), modbus_crc(&bytes[..0x70]));
    }

    #[test]
    fn the_settings_block_has_gbatek_offsets() {
        let firmware = Firmware::built_in(&profile());
        let image = firmware.bytes();
        assert_eq!(usize::from(half(image, 0x20)) * 8, 0x3_FE00);
        for (copy, at) in [(0u16, 0x3_FE00), (1, 0x3_FF00)] {
            let bytes = &image[at..at + 0x74];
            assert_eq!(half(bytes, 0x00), 5);
            assert_eq!(bytes[0x02], 9);
            assert_eq!([bytes[0x03], bytes[0x04]], [3, 14]);
            assert_eq!(nickname(bytes), "Zitrone");
            assert_eq!(bytes[0x14..0x1A], [0; 6]);
            assert_eq!(half(bytes, 0x64) & 7, 2);
            assert_eq!(half(bytes, 0x70), copy);
            assert_eq!(half(bytes, 0x72), modbus_crc(&bytes[..0x70]));
        }
    }

    #[test]
    fn a_user_firmware_keeps_everything_but_the_profile() {
        let image = dump_shaped(0x7F);
        let firmware = Firmware::with_user_settings(&image, &profile());
        let bytes = firmware.bytes();
        let copies = [0x3_FE00..0x3_FE74, 0x3_FF00..0x3_FF74];
        for (index, (&new, &old)) in bytes.iter().zip(&image).enumerate() {
            if !copies.iter().any(|range| range.contains(&index)) {
                assert_eq!(new, old, "byte {index:#X} changed");
            }
        }
        let current = firmware.user_settings().unwrap();
        assert_eq!(current, &bytes[0x3_FF00..0x3_FF74]);
        assert_eq!(nickname(current), "Zitrone");
        assert_eq!(current[0x02], 9);
        assert_eq!([current[0x03], current[0x04]], [3, 14]);
        assert_eq!(half(current, 0x64), 0xFC32);
        assert_eq!(half(current, 0x70), 1);
        assert_eq!(half(&bytes[0x3_FE00..], 0x70), 0);
        let old = &image[0x3_FE00..0x3_FE74];
        assert_eq!(current[0x1C..0x64], old[0x1C..0x64]);
        assert_eq!(current[0x66..0x70], old[0x66..0x70]);
    }

    #[test]
    fn a_user_firmware_without_valid_settings_gets_a_whole_block() {
        let firmware = Firmware::with_user_settings(&[0xFF; FIRMWARE_BYTES], &profile());
        assert_eq!(half(firmware.bytes(), 0x20), 0x7FC0);
        let current = firmware.user_settings().unwrap();
        assert_eq!(nickname(current), "Zitrone");
        assert_eq!(firmware.calibration(), Calibration::default());
    }

    #[test]
    fn a_short_image_is_padded_before_the_settings_go_in() {
        let firmware = Firmware::with_user_settings(&[0xFF; 0x100], &profile());
        assert_eq!(firmware.bytes().len(), FIRMWARE_BYTES);
        assert_eq!(nickname(firmware.user_settings().unwrap()), "Zitrone");
    }

    #[test]
    fn a_large_image_keeps_its_size() {
        let firmware = Firmware::new(&[0; 0x8_0000]);
        assert_eq!(firmware.bytes().len(), 0x8_0000);
    }
}
