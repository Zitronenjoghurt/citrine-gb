pub const HEADER_BYTES: usize = 24;
pub const FCS_BYTES: usize = 4;
pub const MP_CMD: [u8; 6] = [0x03, 0x09, 0xBF, 0x00, 0x00, 0x00];
pub const MP_ACK: [u8; 6] = [0x03, 0x09, 0xBF, 0x00, 0x00, 0x03];
pub const MP_REPLY: [u8; 6] = [0x03, 0x09, 0xBF, 0x00, 0x00, 0x10];

const LONG_PREAMBLE_US: u64 = 192;
const SHORT_PREAMBLE_US: u64 = 96;
const RATE_1_MBIT: u8 = 0x0A;
const RATE_2_MBIT: u8 = 0x14;
const SEQUENCE: usize = 22;
const RETRY: u16 = 1 << 11;
const WEP: u16 = 1 << 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Rate {
    Mbit1,
    Mbit2,
}

impl Rate {
    pub fn from_code(code: u8) -> Self {
        if code == RATE_2_MBIT {
            Self::Mbit2
        } else {
            Self::Mbit1
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Self::Mbit1 => RATE_1_MBIT,
            Self::Mbit2 => RATE_2_MBIT,
        }
    }

    pub fn byte_us(self) -> u64 {
        match self {
            Self::Mbit1 => 8,
            Self::Mbit2 => 4,
        }
    }

    pub fn preamble_us(self, short: bool) -> u64 {
        if short && self == Self::Mbit2 {
            SHORT_PREAMBLE_US
        } else {
            LONG_PREAMBLE_US
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Management(u8),
    Control(u8),
    Data(u8),
    Reserved,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Frame {
    pub start: u64,
    pub channel: u8,
    pub rate: Rate,
    pub short_preamble: bool,
    pub bytes: Vec<u8>,
}

impl Frame {
    pub fn preamble_us(&self) -> u64 {
        self.rate.preamble_us(self.short_preamble)
    }

    pub fn data_us(&self) -> u64 {
        self.bytes.len() as u64 * self.rate.byte_us()
    }

    pub fn airtime_us(&self) -> u64 {
        self.preamble_us() + self.data_us()
    }

    pub fn end(&self) -> u64 {
        self.start + self.airtime_us()
    }

    pub fn control(&self) -> u16 {
        half(&self.bytes, 0)
    }

    pub fn kind(&self) -> Kind {
        let control = self.control();
        let subtype = ((control >> 4) & 0xF) as u8;
        match (control >> 2) & 3 {
            0 => Kind::Management(subtype),
            1 => Kind::Control(subtype),
            2 => Kind::Data(subtype),
            _ => Kind::Reserved,
        }
    }

    pub fn address(&self, number: usize) -> [u8; 6] {
        let at = 4 + 6 * (number - 1);
        std::array::from_fn(|index| self.bytes.get(at + index).copied().unwrap_or(0))
    }

    pub fn sequence(&self) -> u16 {
        half(&self.bytes, SEQUENCE)
    }

    pub fn retry(&self) -> bool {
        self.control() & RETRY != 0
    }

    pub fn wep(&self) -> bool {
        self.control() & WEP != 0
    }

    pub fn body_half(&self, offset: usize) -> u16 {
        half(&self.bytes, HEADER_BYTES + offset)
    }

    pub fn fcs_ok(&self) -> bool {
        let Some(split) = self.bytes.len().checked_sub(FCS_BYTES) else {
            return false;
        };
        let (body, fcs) = self.bytes.split_at(split);
        crc32(body).to_le_bytes() == fcs
    }

    pub fn is_mp_cmd(&self) -> bool {
        matches!(self.kind(), Kind::Data(_)) && self.address(1) == MP_CMD
    }

    pub fn is_mp_reply(&self) -> bool {
        matches!(self.kind(), Kind::Data(_)) && self.address(3) == MP_REPLY
    }

    pub fn is_mp_ack(&self) -> bool {
        matches!(self.kind(), Kind::Data(_)) && self.address(1) == MP_ACK
    }
}

pub fn half(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([
        bytes.get(at).copied().unwrap_or(0),
        bytes.get(at + 1).copied().unwrap_or(0),
    ])
}

pub fn set_half(bytes: &mut [u8], at: usize, value: u16) {
    if let Some(slot) = bytes.get_mut(at..at + 2) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

pub fn set_sequence(bytes: &mut [u8], number: u16) {
    set_half(bytes, SEQUENCE, number << 4);
}

pub fn mark_retry(bytes: &mut [u8]) {
    let control = half(bytes, 0) | RETRY;
    set_half(bytes, 0, control);
}

pub fn seal(bytes: &mut [u8]) {
    if let Some(split) = bytes.len().checked_sub(FCS_BYTES) {
        let crc = crc32(&bytes[..split]);
        bytes[split..].copy_from_slice(&crc.to_le_bytes());
    }
}

pub fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |crc, &byte| {
        (0..8).fold(crc ^ u32::from(byte), |crc, _| {
            if crc & 1 != 0 {
                crc >> 1 ^ 0xEDB8_8320
            } else {
                crc >> 1
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(bytes: Vec<u8>, rate: Rate, short: bool) -> Frame {
        Frame {
            start: 100,
            channel: 7,
            rate,
            short_preamble: short,
            bytes,
        }
    }

    #[test]
    fn the_crc_is_ieee_crc32() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn airtime_follows_rate_and_preamble() {
        let long = frame(vec![0; 32], Rate::Mbit1, true);
        assert_eq!(long.airtime_us(), 192 + 32 * 8);
        let short = frame(vec![0; 32], Rate::Mbit2, true);
        assert_eq!(short.airtime_us(), 96 + 32 * 4);
        assert_eq!(short.end(), 100 + 96 + 128);
        let long_at_two = frame(vec![0; 32], Rate::Mbit2, false);
        assert_eq!(long_at_two.preamble_us(), 192);
    }

    #[test]
    fn a_sealed_frame_passes_its_fcs() {
        let mut bytes = vec![0x80, 0x00, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0];
        seal(&mut bytes);
        let mut sealed = frame(bytes, Rate::Mbit2, false);
        assert!(sealed.fcs_ok());
        sealed.bytes[3] ^= 1;
        assert!(!sealed.fcs_ok());
    }

    #[test]
    fn kinds_and_mp_addresses_read_from_the_header() {
        let mut bytes = vec![0; 32];
        bytes[0] = 0x28;
        bytes[1] = 0x02;
        bytes[4..10].copy_from_slice(&MP_CMD);
        let cmd = frame(bytes, Rate::Mbit2, true);
        assert_eq!(cmd.kind(), Kind::Data(2));
        assert!(cmd.is_mp_cmd());
        assert!(!cmd.is_mp_reply());
        let mut beacon = vec![0; 40];
        beacon[0] = 0x80;
        assert_eq!(
            frame(beacon, Rate::Mbit1, false).kind(),
            Kind::Management(8)
        );
    }
}
