use crate::eeprom::{Block, Eeprom};
use crate::info::{HEALTH_SIZE, Health, WalkerInfo};
use crate::rules::Rules;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ram {
    pub health: [u8; HEALTH_SIZE],
    pub today_steps: u32,
    pub hour_steps: u16,
    pub event_seconds: u16,
    pub awake_seconds: u8,
    pub rollover_hour: u8,
    pub walking: bool,
}

impl Ram {
    pub const SIZE: usize = HEALTH_SIZE + 11;

    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[..HEALTH_SIZE].copy_from_slice(&self.health);
        let rest = &mut bytes[HEALTH_SIZE..];
        rest[0..4].copy_from_slice(&self.today_steps.to_le_bytes());
        rest[4..6].copy_from_slice(&self.hour_steps.to_le_bytes());
        rest[6..8].copy_from_slice(&self.event_seconds.to_le_bytes());
        rest[8] = self.awake_seconds;
        rest[9] = self.rollover_hour;
        rest[10] = u8::from(self.walking);
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; Self::SIZE] = bytes.get(..Self::SIZE)?.try_into().ok()?;
        let rest = &bytes[HEALTH_SIZE..];
        Some(Self {
            health: bytes[..HEALTH_SIZE].try_into().ok()?,
            today_steps: u32::from_le_bytes(rest[0..4].try_into().ok()?),
            hour_steps: u16::from_le_bytes([rest[4], rest[5]]),
            event_seconds: u16::from_le_bytes([rest[6], rest[7]]),
            awake_seconds: rest[8],
            rollover_hour: rest[9],
            walking: match rest[10] {
                0 => false,
                1 => true,
                _ => return None,
            },
        })
    }
}

pub(crate) fn boot(eeprom: &mut Eeprom, rules: &Rules) -> Ram {
    if !eeprom.has_magic() {
        eeprom.format();
    }
    let info = WalkerInfo::from_slice(&eeprom.read_reliable(Block::Info).unwrap_or_default());
    let health = Health::from_slice(&eeprom.read_reliable(Block::Health).unwrap_or_default());
    Ram {
        health: health.0,
        today_steps: 0,
        hour_steps: 0,
        event_seconds: rules.home_event_seconds,
        awake_seconds: rules.wake_seconds,
        rollover_hour: info.day_end_hour(),
        walking: info.has_pokemon(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_round_trips_through_its_bytes() {
        let ram = Ram {
            health: std::array::from_fn(|i| i as u8),
            today_steps: 12_345,
            hour_steps: 678,
            event_seconds: 3_600,
            awake_seconds: 90,
            rollover_hour: 6,
            walking: true,
        };
        let bytes = ram.to_bytes();
        assert_eq!(bytes[HEALTH_SIZE..HEALTH_SIZE + 4], 12_345u32.to_le_bytes());
        assert_eq!(Ram::from_bytes(&bytes), Some(ram));
        let mut bad = bytes;
        bad[Ram::SIZE - 1] = 2;
        assert_eq!(Ram::from_bytes(&bad), None);
        assert_eq!(Ram::from_bytes(&bytes[..Ram::SIZE - 1]), None);
    }
}
