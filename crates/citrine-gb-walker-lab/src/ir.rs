pub const WIRE_XOR: u8 = 0xAA;
pub const HEADER: usize = 8;
pub const MAX_PAYLOAD: usize = 128;

pub const CMD_ADVERTISE: u8 = 0xFC;
pub const CMD_ASSERT_MASTER: u8 = 0xFA;
pub const CMD_SLAVE_ACK: u8 = 0xF8;
pub const CMD_IDENTITY_REQ: u8 = 0x20;
pub const CMD_IDENTITY_RSP: u8 = 0x22;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub cmd: u8,
    pub extra: u8,
    pub session: u32,
    pub payload: Vec<u8>,
}

fn sum_into(mut sum: u32, bytes: &[u8]) -> u32 {
    for (i, &b) in bytes.iter().enumerate() {
        sum += if i % 2 == 0 {
            (b as u32) << 8
        } else {
            b as u32
        };
    }
    sum
}

fn fold(mut sum: u32) -> u16 {
    while sum > 0xFFFF {
        sum = (sum >> 16) + (sum & 0xFFFF);
    }
    sum as u16
}

pub fn checksum(header: &[u8; HEADER], payload: &[u8]) -> u16 {
    let mut h = *header;
    h[2] = 0;
    h[3] = 0;
    let seed = fold(sum_into(0x0002, &h)) as u32;
    fold(sum_into(seed, payload))
}

impl Packet {
    pub fn new(cmd: u8, extra: u8, session: u32, payload: &[u8]) -> Self {
        Self {
            cmd,
            extra,
            session,
            payload: payload.to_vec(),
        }
    }

    pub fn plain(&self) -> Vec<u8> {
        let mut header = [0u8; HEADER];
        header[0] = self.cmd;
        header[1] = self.extra;
        header[4..8].copy_from_slice(&self.session.to_be_bytes());
        let sum = checksum(&header, &self.payload);
        header[2] = sum as u8;
        header[3] = (sum >> 8) as u8;
        let mut out = header.to_vec();
        out.extend_from_slice(&self.payload);
        out
    }

    pub fn wire(&self) -> Vec<u8> {
        self.plain().iter().map(|b| b ^ WIRE_XOR).collect()
    }

    pub fn from_wire(wire: &[u8]) -> Option<Self> {
        let plain: Vec<u8> = wire.iter().map(|b| b ^ WIRE_XOR).collect();
        Self::from_plain(&plain)
    }

    pub fn from_plain(plain: &[u8]) -> Option<Self> {
        if plain.len() < HEADER || plain.len() > HEADER + MAX_PAYLOAD {
            return None;
        }
        let header: [u8; HEADER] = plain[..HEADER].try_into().ok()?;
        let payload = &plain[HEADER..];
        let sum = u16::from_le_bytes([header[2], header[3]]);
        if checksum(&header, payload) != sum {
            return None;
        }
        Some(Self {
            cmd: header[0],
            extra: header[1],
            session: u32::from_be_bytes(header[4..8].try_into().ok()?),
            payload: payload.to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_seeds_two_and_sums_even_bytes_high() {
        let header = [0u8; HEADER];
        assert_eq!(checksum(&header, &[]), 0x0002);
        assert_eq!(
            checksum(&header, &[0x01, 0x02, 0x03]),
            0x0002 + 0x0102 + 0x0300
        );
        let mut h = [0xFFu8; HEADER];
        h[2] = 0x12;
        h[3] = 0x34;
        assert_eq!(checksum(&h, &[]), fold(0x0002 + 0xFFFF * 3));
    }

    #[test]
    fn wire_round_trip_with_xor() {
        let p = Packet::new(CMD_ASSERT_MASTER, 0x01, 0x1234_5678, &[9, 8, 7]);
        let wire = p.wire();
        assert_eq!(wire[0], 0xFA ^ 0xAA);
        assert_eq!(Packet::from_wire(&wire), Some(p));
        let mut bad = wire.clone();
        bad[9] ^= 1;
        assert_eq!(Packet::from_wire(&bad), None);
    }
}
