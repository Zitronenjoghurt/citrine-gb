use citrine_gb_walker::packet::{WIRE_XOR, checksum};

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
        let mut out = vec![self.cmd, self.extra, 0, 0];
        out.extend_from_slice(&self.session.to_be_bytes());
        out.extend_from_slice(&self.payload);
        let sum = checksum(&out);
        out[2..4].copy_from_slice(&sum.to_le_bytes());
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
        let stored = u16::from_le_bytes([plain[2], plain[3]]);
        let mut bytes = plain.to_vec();
        bytes[2] = 0;
        bytes[3] = 0;
        if checksum(&bytes) != stored {
            return None;
        }
        Some(Self {
            cmd: plain[0],
            extra: plain[1],
            session: u32::from_be_bytes(plain[4..8].try_into().ok()?),
            payload: plain[HEADER..].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
