use super::{MAC_ADDRESS, WIFI_CRC, WIFI_LENGTH, settings_base};
use crate::utils::crc16::crc16;

pub const SLOT_BYTES: usize = 0x100;
pub const SLOTS: usize = 3;
pub const CONNECTIONS_BYTES: usize = SLOT_BYTES * SLOTS;
pub const MAC_PREFIX: [u8; 3] = [0x00, 0x09, 0xBF];

const CONNECTIONS_BELOW_SETTINGS: usize = 0x400;
const SSID: usize = 0x40;
const SSID_BYTES: usize = 0x20;
const PRIMARY_DNS: usize = 0xC8;
const STATUS: usize = 0xE7;
const CONFIGURED: usize = 0xEF;
const USER_ID: usize = 0xF0;
const CHECKSUM: usize = 0xFE;
const STATUS_NORMAL: u8 = 0x00;
const STATUS_EMPTY: u8 = 0xFF;
const MAX_WIFI_CONFIG: usize = 0x1D4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessPointSlot {
    pub ssid: Vec<u8>,
    pub dns: Option<[u8; 4]>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Online {
    pub mac: Option<[u8; 6]>,
    pub connections: Option<Vec<u8>>,
    pub access_point: Option<AccessPointSlot>,
}

impl Online {
    pub fn apply(&self, image: &mut [u8]) {
        if let Some(mac) = self.mac {
            write_mac(image, mac);
        }
        let base = connections_base(image.len());
        let area = &mut image[base..base + CONNECTIONS_BYTES];
        if let Some(saved) = self
            .connections
            .as_deref()
            .filter(|saved| saved.len() == CONNECTIONS_BYTES)
        {
            area.copy_from_slice(saved);
        }
        if let Some(access_point) = &self.access_point {
            access_point.write(&mut area[..SLOT_BYTES]);
        }
    }
}

impl AccessPointSlot {
    fn write(&self, slot: &mut [u8]) {
        let kept = if slot[STATUS] == STATUS_EMPTY || !slot_valid(slot) {
            [0; CHECKSUM - USER_ID]
        } else {
            slot[USER_ID..CHECKSUM].try_into().unwrap_or_default()
        };
        slot.fill(0);
        let ssid = &self.ssid[..self.ssid.len().min(SSID_BYTES)];
        slot[SSID..SSID + ssid.len()].copy_from_slice(ssid);
        if let Some(dns) = self.dns {
            slot[PRIMARY_DNS..PRIMARY_DNS + 4].copy_from_slice(&dns);
        }
        slot[STATUS] = STATUS_NORMAL;
        slot[CONFIGURED] = 1;
        slot[USER_ID..CHECKSUM].copy_from_slice(&kept);
        seal(slot);
    }
}

pub fn write_empty_slots(image: &mut [u8]) {
    let base = connections_base(image.len());
    for slot in image[base..base + CONNECTIONS_BYTES].chunks_mut(SLOT_BYTES) {
        slot.fill(0);
        slot[STATUS] = STATUS_EMPTY;
        slot[CONFIGURED] = 1;
        seal(slot);
    }
}

fn seal(slot: &mut [u8]) {
    let checksum = crc16(0, &slot[..CHECKSUM]);
    slot[CHECKSUM..].copy_from_slice(&checksum.to_le_bytes());
}

pub fn connections(image: &[u8]) -> &[u8] {
    let base = connections_base(image.len());
    &image[base..base + CONNECTIONS_BYTES]
}

pub fn mac(image: &[u8]) -> [u8; 6] {
    image[MAC_ADDRESS..MAC_ADDRESS + 6]
        .try_into()
        .unwrap_or_default()
}

pub fn user_id(slot: &[u8]) -> Option<u64> {
    let mut bytes = [0; 8];
    bytes[..6].copy_from_slice(&slot[USER_ID..USER_ID + 6]);
    let id = u64::from_le_bytes(bytes) & 0x07FF_FFFF_FFFF;
    (slot_valid(slot) && slot[STATUS] != STATUS_EMPTY && id != 0).then_some(id)
}

fn slot_valid(slot: &[u8]) -> bool {
    crc16(0, &slot[..CHECKSUM]) == u16::from_le_bytes([slot[CHECKSUM], slot[CHECKSUM + 1]])
}

fn connections_base(size: usize) -> usize {
    settings_base(size) - CONNECTIONS_BELOW_SETTINGS
}

fn write_mac(image: &mut [u8], mac: [u8; 6]) {
    image[MAC_ADDRESS..MAC_ADDRESS + 6].copy_from_slice(&mac);
    let length = usize::from(u16::from_le_bytes([
        image[WIFI_LENGTH],
        image[WIFI_LENGTH + 1],
    ]));
    let length = length.min(MAX_WIFI_CONFIG);
    let crc = crc16(0, &image[WIFI_LENGTH..WIFI_LENGTH + length]);
    image[WIFI_CRC..WIFI_CRC + 2].copy_from_slice(&crc.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::super::{FIRMWARE_BYTES, Firmware, UserSettings};
    use super::*;

    const MAC: [u8; 6] = [0x00, 0x09, 0xBF, 0x12, 0x34, 0x56];

    fn ours() -> AccessPointSlot {
        AccessPointSlot {
            ssid: b"citrine".to_vec(),
            dns: Some([178, 62, 43, 212]),
        }
    }

    fn built(online: &Online) -> Vec<u8> {
        let mut image = Firmware::built_in(&UserSettings::default())
            .bytes()
            .to_vec();
        online.apply(&mut image);
        image
    }

    fn wifi_crc_holds(image: &[u8]) -> bool {
        let length = usize::from(u16::from_le_bytes([
            image[WIFI_LENGTH],
            image[WIFI_LENGTH + 1],
        ]));
        crc16(0, &image[WIFI_LENGTH..WIFI_LENGTH + length])
            == u16::from_le_bytes([image[WIFI_CRC], image[WIFI_CRC + 1]])
    }

    #[test]
    fn the_mac_is_written_with_a_fresh_wifi_crc() {
        let image = built(&Online {
            mac: Some(MAC),
            ..Online::default()
        });
        assert_eq!(mac(&image), MAC);
        assert!(wifi_crc_holds(&image));
    }

    #[test]
    fn the_first_slot_points_at_our_access_point_at_3fa00() {
        let image = built(&Online {
            access_point: Some(ours()),
            ..Online::default()
        });
        assert_eq!(FIRMWARE_BYTES - 0x600, 0x3FA00);
        let slot = &image[0x3FA00..0x3FB00];
        assert_eq!(&slot[0x40..0x47], b"citrine");
        assert_eq!(slot[0xE8], 0);
        assert_eq!(&slot[0xC0..0xC8], &[0; 8]);
        assert_eq!(&slot[0xC8..0xCC], &[178, 62, 43, 212]);
        assert_eq!(slot[0xE7], 0);
        assert!(slot_valid(slot));
        assert_eq!(user_id(slot), None);
        assert_eq!(image[0x3FB00 + 0xE7], 0xFF);
    }

    #[test]
    fn no_dns_leaves_the_slot_on_automatic_dns() {
        let image = built(&Online {
            access_point: Some(AccessPointSlot {
                dns: None,
                ..ours()
            }),
            ..Online::default()
        });
        assert_eq!(&image[0x3FAC8..0x3FAD0], &[0; 8]);
    }

    #[test]
    fn saved_connections_come_back_and_keep_the_wfc_user_id() {
        let mut first = built(&Online {
            access_point: Some(ours()),
            ..Online::default()
        });
        let slot = &mut first[0x3FA00..0x3FB00];
        slot[0xF0..0xF6].copy_from_slice(&[0x10, 0x32, 0x54, 0x76, 0x98, 0x02]);
        let checksum = crc16(0, &slot[..0xFE]);
        slot[0xFE..].copy_from_slice(&checksum.to_le_bytes());
        let saved = connections(&first).to_vec();
        let image = built(&Online {
            connections: Some(saved),
            access_point: Some(AccessPointSlot {
                dns: None,
                ..ours()
            }),
            ..Online::default()
        });
        let slot = &image[0x3FA00..0x3FB00];
        assert_eq!(user_id(slot), Some(0x0298_7654_3210));
        assert_eq!(&slot[0xC8..0xCC], &[0; 4]);
    }

    #[test]
    fn saved_connections_of_the_wrong_size_are_ignored() {
        let image = built(&Online {
            connections: Some(vec![0; 10]),
            ..Online::default()
        });
        assert_eq!(image[0x3FA00 + 0xE7], 0xFF);
    }

    #[test]
    fn the_built_in_image_has_three_valid_empty_slots() {
        let image = built(&Online::default());
        for slot in connections(&image).chunks(SLOT_BYTES) {
            assert!(slot_valid(slot));
            assert_eq!(slot[0xE7], 0xFF);
            assert_eq!(slot[0xEF], 1);
            assert_eq!(&slot[..0xE7], &[0; 0xE7]);
        }
    }

    #[test]
    fn our_slot_leaves_the_other_two_valid() {
        let image = built(&Online {
            access_point: Some(ours()),
            ..Online::default()
        });
        for slot in connections(&image).chunks(SLOT_BYTES) {
            assert!(slot_valid(slot));
        }
    }
}
