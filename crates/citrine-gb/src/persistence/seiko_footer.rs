pub const SEIKO_FOOTER_LEN: usize = 16;
const TIME_LEN: usize = 7;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct SeikoFooter {
    pub time: [u8; TIME_LEN],
    pub status: u8,
    pub timestamp: u64,
}

impl SeikoFooter {
    pub fn encode(&self) -> [u8; SEIKO_FOOTER_LEN] {
        let mut out = [0u8; SEIKO_FOOTER_LEN];
        out[..TIME_LEN].copy_from_slice(&self.time);
        out[TIME_LEN] = self.status;
        out[TIME_LEN + 1..].copy_from_slice(&self.timestamp.to_le_bytes());
        out
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() != SEIKO_FOOTER_LEN {
            return None;
        }
        Some(Self {
            time: data[..TIME_LEN].try_into().ok()?,
            status: data[TIME_LEN],
            timestamp: u64::from_le_bytes(data[TIME_LEN + 1..].try_into().ok()?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_the_time_then_the_status_then_a_little_endian_timestamp() {
        let footer = SeikoFooter {
            time: [0x24, 0x02, 0x29, 0x04, 0x93, 0x59, 0x58],
            status: 0x40,
            timestamp: 0x0102_0304_0506_0708,
        };
        let bytes = footer.encode();
        assert_eq!(
            &bytes[..8],
            &[0x24, 0x02, 0x29, 0x04, 0x93, 0x59, 0x58, 0x40]
        );
        assert_eq!(&bytes[8..], &[8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(SeikoFooter::decode(&bytes), Some(footer));
        assert_eq!(SeikoFooter::decode(&bytes[1..]), None);
    }
}
