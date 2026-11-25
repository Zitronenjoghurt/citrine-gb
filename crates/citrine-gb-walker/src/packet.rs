pub const WIRE_XOR: u8 = 0xAA;
pub(crate) const HEADER_SIZE: usize = 8;
pub(crate) const MAX_PAYLOAD: usize = 128;
pub(crate) const ADVERTISE: u8 = 0xFC;
const CHECKSUM_SEED: u16 = 0x0002;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Packet {
    pub(crate) cmd: u8,
    pub(crate) extra: u8,
    pub(crate) session: [u8; 4],
    pub(crate) payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PacketError {
    Short,
    Long,
    Checksum,
}

impl Packet {
    pub(crate) fn new(cmd: u8, extra: u8, session: [u8; 4], payload: Vec<u8>) -> Self {
        Self {
            cmd,
            extra,
            session,
            payload,
        }
    }

    pub(crate) fn plain(&self) -> Vec<u8> {
        let mut bytes = vec![self.cmd, self.extra, 0, 0];
        bytes.extend_from_slice(&self.session);
        bytes.extend_from_slice(&self.payload);
        let sum = checksum(&bytes);
        bytes[2..4].copy_from_slice(&sum.to_le_bytes());
        bytes
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        self.plain().into_iter().map(|b| b ^ WIRE_XOR).collect()
    }

    pub(crate) fn decode(wire: &[u8]) -> Result<Self, PacketError> {
        if wire.len() < HEADER_SIZE {
            return Err(PacketError::Short);
        }
        if wire.len() > HEADER_SIZE + MAX_PAYLOAD {
            return Err(PacketError::Long);
        }
        let mut bytes: Vec<u8> = wire.iter().map(|b| b ^ WIRE_XOR).collect();
        let stored = u16::from_le_bytes([bytes[2], bytes[3]]);
        bytes[2] = 0;
        bytes[3] = 0;
        if checksum(&bytes) != stored {
            return Err(PacketError::Checksum);
        }
        Ok(Self {
            cmd: bytes[0],
            extra: bytes[1],
            session: [bytes[4], bytes[5], bytes[6], bytes[7]],
            payload: bytes[HEADER_SIZE..].to_vec(),
        })
    }
}

pub(crate) fn advertisement() -> Vec<u8> {
    vec![ADVERTISE ^ WIRE_XOR]
}

pub fn checksum(bytes: &[u8]) -> u16 {
    let (header, payload) = bytes.split_at(bytes.len().min(HEADER_SIZE));
    let seed = sum(header, CHECKSUM_SEED);
    if payload.is_empty() {
        seed
    } else {
        sum(payload, seed)
    }
}

fn sum(bytes: &[u8], seed: u16) -> u16 {
    let mut total = seed as u32;
    for (i, byte) in bytes.iter().enumerate() {
        let value = *byte as u32;
        total += if i % 2 == 0 { value << 8 } else { value };
    }
    while total >> 16 != 0 {
        total = (total & 0xFFFF) + (total >> 16);
    }
    total as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksum_of_a_bare_header_adds_even_bytes_high() {
        let header = [0x20, 0x01, 0, 0, 0x12, 0x34, 0x56, 0x78];
        let expected = 0x0002 + 0x2000 + 0x01 + 0x1200 + 0x34 + 0x5600 + 0x78;
        assert_eq!(checksum(&header), expected as u16);
    }

    #[test]
    fn the_checksum_folds_its_carry_back_in_until_it_fits() {
        let header = [0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF];
        let total: u32 = 0x0002 + 3 * 0xFFFF;
        assert_eq!(total, 0x2FFFF);
        let once = (total & 0xFFFF) + (total >> 16);
        assert_eq!(once, 0x10001);
        assert_eq!(checksum(&header), 0x0002);
    }

    #[test]
    fn the_payload_continues_from_the_header_sum() {
        let header = [0x0E, 0x02, 0, 0, 0, 0, 0, 0];
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(&[0xAB, 0xCD, 0xEF]);
        let expected = 0x0002 + 0x0E00 + 0x02 + 0xAB00 + 0xCD + 0xEF00;
        let folded = (expected & 0xFFFF) + (expected >> 16);
        assert_eq!(checksum(&bytes), folded as u16);
    }

    #[test]
    fn every_byte_goes_out_xored_with_the_checksum_little_endian() {
        let packet = Packet::new(0x24, 0x01, [0xDE, 0xAD, 0xBE, 0xEF], vec![]);
        let sum: u32 = 0x0002 + 0x2400 + 0x01 + 0xDE00 + 0xAD + 0xBE00 + 0xEF;
        let sum = ((sum & 0xFFFF) + (sum >> 16)) as u16;
        let plain = [
            0x24,
            0x01,
            sum as u8,
            (sum >> 8) as u8,
            0xDE,
            0xAD,
            0xBE,
            0xEF,
        ];
        let wire: Vec<u8> = plain.iter().map(|b| b ^ 0xAA).collect();
        assert_eq!(packet.encode(), wire);
        assert_eq!(Packet::decode(&wire), Ok(packet));
    }

    #[test]
    fn the_advertisement_is_one_byte_0x56_on_the_wire() {
        assert_eq!(advertisement(), vec![0x56]);
    }

    #[test]
    fn a_flipped_bit_fails_the_checksum() {
        let mut wire = Packet::new(0x0C, 0x01, [1, 2, 3, 4], vec![0x8F, 0x00, 0x10]).encode();
        wire[9] ^= 0x01;
        assert_eq!(Packet::decode(&wire), Err(PacketError::Checksum));
    }

    #[test]
    fn short_and_long_packets_are_refused() {
        assert_eq!(Packet::decode(&[0x56]), Err(PacketError::Short));
        assert_eq!(Packet::decode(&[0; 137]), Err(PacketError::Long));
        let full = Packet::new(0x02, 0x8F, [0; 4], vec![0x11; 128]).encode();
        assert_eq!(full.len(), 136);
        assert!(Packet::decode(&full).is_ok());
    }
}
