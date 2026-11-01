use crate::gb::cartridge::RtcRegisters;

pub const RTC_FOOTER_LEN: usize = 48;
const RTC_FOOTER_32_BIT_LEN: usize = 44;
const SAMEBOY_LEGACY_LEN: usize = 13;
const REGISTERS_LEN: usize = 20;

pub const RTC_FILE_LEN: usize = 32;
const RTC_FILE_OLD_LEN: usize = 40;
const RTC_FILE_LATCHED: usize = 5;
const RTC_FILE_TIMESTAMP: usize = 16;

pub const EARLIEST_TIMESTAMP: u64 = 852_076_800;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct RtcFooter {
    pub real: RtcRegisters,
    pub latched: RtcRegisters,
    pub timestamp: u64,
}

impl RtcFooter {
    pub fn encode(&self) -> [u8; RTC_FOOTER_LEN] {
        let mut out = [0u8; RTC_FOOTER_LEN];
        for (i, value) in self
            .real
            .to_bytes()
            .into_iter()
            .chain(self.latched.to_bytes())
            .enumerate()
        {
            out[i * 4] = value;
        }
        out[REGISTERS_LEN * 2..].copy_from_slice(&self.timestamp.to_le_bytes());
        out
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        match data.len() {
            RTC_FOOTER_LEN => Some(Self {
                real: registers_at(data, 0),
                latched: registers_at(data, REGISTERS_LEN),
                timestamp: u64::from_le_bytes(data[REGISTERS_LEN * 2..].try_into().ok()?),
            }),
            RTC_FOOTER_32_BIT_LEN => Some(Self {
                real: registers_at(data, 0),
                latched: registers_at(data, REGISTERS_LEN),
                timestamp: u32::from_le_bytes(data[REGISTERS_LEN * 2..].try_into().ok()?) as u64,
            }),
            SAMEBOY_LEGACY_LEN => {
                let real = RtcRegisters::from_bytes(data[..5].try_into().ok()?);
                Some(Self {
                    real,
                    latched: real,
                    timestamp: u64::from_le_bytes(data[5..].try_into().ok()?),
                })
            }
            _ => None,
        }
    }

    pub fn decode_rtc_file(data: &[u8]) -> Option<Self> {
        if data.len() != RTC_FILE_LEN && data.len() != RTC_FILE_OLD_LEN {
            return None;
        }
        Some(Self {
            real: RtcRegisters::from_bytes(data[..RTC_FILE_LATCHED].try_into().ok()?),
            latched: RtcRegisters::from_bytes(
                data[RTC_FILE_LATCHED..RTC_FILE_LATCHED * 2]
                    .try_into()
                    .ok()?,
            ),
            timestamp: u64::from_le_bytes(
                data[RTC_FILE_TIMESTAMP..RTC_FILE_TIMESTAMP + 8]
                    .try_into()
                    .ok()?,
            ),
        })
    }

    pub fn encode_rtc_file(&self) -> [u8; RTC_FILE_LEN] {
        let mut out = [0u8; RTC_FILE_LEN];
        out[..RTC_FILE_LATCHED].copy_from_slice(&self.real.to_bytes());
        out[RTC_FILE_LATCHED..RTC_FILE_LATCHED * 2].copy_from_slice(&self.latched.to_bytes());
        out[RTC_FILE_TIMESTAMP..RTC_FILE_TIMESTAMP + 8]
            .copy_from_slice(&self.timestamp.to_le_bytes());
        out
    }

    pub fn seconds_until(&self, now: u64) -> u64 {
        if self.timestamp < EARLIEST_TIMESTAMP {
            return 0;
        }
        now.saturating_sub(self.timestamp)
    }
}

fn registers_at(data: &[u8], offset: usize) -> RtcRegisters {
    let mut bytes = [0u8; 5];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = data[offset + i * 4];
    }
    RtcRegisters::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_761_400_000;

    fn footer() -> RtcFooter {
        RtcFooter {
            real: RtcRegisters::from_bytes([12, 34, 5, 0xAB, 0x41]),
            latched: RtcRegisters::from_bytes([11, 34, 5, 0xAB, 0x01]),
            timestamp: NOW,
        }
    }

    #[test]
    fn encodes_registers_as_little_endian_words_then_the_timestamp() {
        let bytes = footer().encode();
        assert_eq!(
            bytes[..20],
            [
                12, 0, 0, 0, 34, 0, 0, 0, 5, 0, 0, 0, 0xAB, 0, 0, 0, 0x41, 0, 0, 0
            ]
        );
        assert_eq!(
            bytes[20..40],
            [
                11, 0, 0, 0, 34, 0, 0, 0, 5, 0, 0, 0, 0xAB, 0, 0, 0, 0x01, 0, 0, 0
            ]
        );
        assert_eq!(bytes[40..], NOW.to_le_bytes());
    }

    #[test]
    fn round_trips() {
        assert_eq!(RtcFooter::decode(&footer().encode()), Some(footer()));
    }

    #[test]
    fn reads_the_44_byte_footer_with_a_32_bit_timestamp() {
        let mut bytes = footer().encode()[..44].to_vec();
        bytes[40..44].copy_from_slice(&(NOW as u32).to_le_bytes());
        assert_eq!(RtcFooter::decode(&bytes), Some(footer()));
    }

    #[test]
    fn reads_the_old_13_byte_sameboy_footer() {
        let mut bytes = vec![12, 34, 5, 0xAB, 0x41];
        bytes.extend_from_slice(&NOW.to_le_bytes());
        let decoded = RtcFooter::decode(&bytes).expect("decoded");
        assert_eq!(decoded.real, footer().real);
        assert_eq!(decoded.latched, footer().real);
        assert_eq!(decoded.timestamp, NOW);
    }

    #[test]
    fn rejects_other_lengths() {
        assert_eq!(RtcFooter::decode(&[]), None);
        assert_eq!(RtcFooter::decode(&[0; 47]), None);
    }

    #[test]
    fn unused_bits_are_dropped() {
        let mut bytes = [0xFFu8; RTC_FOOTER_LEN];
        bytes[40..].copy_from_slice(&NOW.to_le_bytes());
        let decoded = RtcFooter::decode(&bytes).expect("decoded");
        assert_eq!(decoded.real.to_bytes(), [0x3F, 0x3F, 0x1F, 0xFF, 0xC1]);
    }

    #[test]
    fn encodes_the_rtc_file_as_sameboys_rtc_section() {
        let bytes = footer().encode_rtc_file();
        assert_eq!(bytes[..5], [12, 34, 5, 0xAB, 0x41]);
        assert_eq!(bytes[5..10], [11, 34, 5, 0xAB, 0x01]);
        assert_eq!(bytes[10..16], [0; 6]);
        assert_eq!(bytes[16..24], NOW.to_le_bytes());
        assert_eq!(bytes[24..], [0; 8]);
    }

    #[test]
    fn round_trips_the_rtc_file() {
        assert_eq!(
            RtcFooter::decode_rtc_file(&footer().encode_rtc_file()),
            Some(footer())
        );
    }

    #[test]
    fn reads_the_40_byte_rtc_file_of_sameboy_before_0_15() {
        let mut bytes = footer().encode_rtc_file().to_vec();
        bytes.resize(40, 0);
        bytes[24] = 1;
        assert_eq!(RtcFooter::decode_rtc_file(&bytes), Some(footer()));
    }

    #[test]
    fn rejects_rtc_files_of_other_lengths() {
        assert_eq!(RtcFooter::decode_rtc_file(&[0; 31]), None);
        assert_eq!(RtcFooter::decode_rtc_file(&footer().encode()), None);
    }

    #[test]
    fn elapsed_time_ignores_timestamps_from_the_future_or_before_1997() {
        let mut footer = footer();
        assert_eq!(footer.seconds_until(NOW + 90), 90);
        assert_eq!(footer.seconds_until(NOW - 5), 0);
        footer.timestamp = 1000;
        assert_eq!(footer.seconds_until(NOW), 0);
    }
}
