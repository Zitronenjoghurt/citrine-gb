pub(crate) const INFO_SIZE: usize = 0x68;
pub(crate) const HEALTH_SIZE: usize = 0x18;
pub(crate) const IDENTITY_SIZE: usize = 0x28;
pub(crate) const FIRST_CLOCK: u32 = 0x0D2B_0B80;

pub const FLAG_INIT: u8 = 0x01;
pub const FLAG_HAS_POKEMON: u8 = 0x02;
pub const FLAG_POKEMON_JOINED: u8 = 0x04;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkerInfo(pub(crate) [u8; INFO_SIZE]);

impl WalkerInfo {
    pub(crate) fn from_slice(bytes: &[u8]) -> Self {
        let mut info = [0; INFO_SIZE];
        let len = bytes.len().min(INFO_SIZE);
        info[..len].copy_from_slice(&bytes[..len]);
        Self(info)
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn tid(&self) -> u16 {
        u16::from_le_bytes([self.0[0x0C], self.0[0x0D]])
    }

    pub fn sid(&self) -> u16 {
        u16::from_le_bytes([self.0[0x0E], self.0[0x0F]])
    }

    pub fn identity(&self) -> &[u8] {
        &self.0[0x10..0x10 + IDENTITY_SIZE]
    }

    pub fn trainer_name(&self) -> [u16; 8] {
        std::array::from_fn(|i| u16::from_le_bytes([self.0[0x48 + 2 * i], self.0[0x49 + 2 * i]]))
    }

    pub fn flags(&self) -> u8 {
        self.0[0x5B]
    }

    pub fn has_pokemon(&self) -> bool {
        self.flags() & FLAG_HAS_POKEMON != 0
    }

    pub fn day_end_hour(&self) -> u8 {
        self.flags() >> 3
    }

    pub fn last_sync(&self) -> u32 {
        self.be32(0x60)
    }

    pub fn step_count(&self) -> u32 {
        self.be32(0x64)
    }

    pub(crate) fn set_flags(&mut self, flags: u8) {
        self.0[0x5B] = flags;
    }

    pub(crate) fn set_step_count(&mut self, steps: u32) {
        self.0[0x64..0x68].copy_from_slice(&steps.to_be_bytes());
    }

    pub fn event_index(&self) -> u8 {
        self.0[0x5A]
    }

    pub(crate) fn mark_event(&mut self, event: u8) {
        self.0[0x38 + usize::from(event >> 3)] |= 1 << (event & 7);
    }

    pub(crate) fn has_event(&self, event: u8) -> bool {
        self.0[0x38 + usize::from(event >> 3)] & (1 << (event & 7)) != 0
    }

    pub(crate) fn start_walk_from(&mut self, peer: &WalkerInfo) {
        for (at, len) in [(0x00, 0x10), (0x48, 0x12), (0x5C, 2)] {
            self.0[at..at + len].copy_from_slice(peer.range(at, len));
        }
        self.0[0x5E] = 0;
        self.0[0x5F] = 2;
        self.set_flags((self.flags() & !FLAG_POKEMON_JOINED) | FLAG_INIT | FLAG_HAS_POKEMON);
    }

    pub(crate) fn reset(&mut self, identity: &[u8], steps: bool) {
        self.0[0x00..0x10].fill(0);
        self.0[0x10..0x10 + IDENTITY_SIZE].copy_from_slice(&identity[..IDENTITY_SIZE]);
        self.0[0x48..0x5A].fill(0);
        self.0[0x5A] = 0;
        self.set_flags(0);
        self.0[0x5E] = 0;
        self.0[0x5F] = 2;
        self.0[0x60..0x64].fill(0);
        if steps {
            self.0[0x64..0x68].fill(0);
        }
    }

    pub(crate) fn join(&mut self) {
        self.0.copy_within(0x00..0x04, 0x04);
        self.0.copy_within(0x08..0x0A, 0x0A);
        self.set_flags(self.flags() | FLAG_HAS_POKEMON | FLAG_POKEMON_JOINED);
    }

    pub(crate) fn end_walk(&mut self) {
        self.0[0x04..0x08].fill(0);
        self.0[0x0A..0x0C].fill(0);
        self.set_flags(self.flags() & !(FLAG_HAS_POKEMON | FLAG_POKEMON_JOINED));
    }

    fn range(&self, at: usize, len: usize) -> &[u8] {
        &self.0[at..at + len]
    }

    fn be32(&self, at: usize) -> u32 {
        u32::from_be_bytes([self.0[at], self.0[at + 1], self.0[at + 2], self.0[at + 3]])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health(pub(crate) [u8; HEALTH_SIZE]);

impl Health {
    pub(crate) fn from_slice(bytes: &[u8]) -> Self {
        let mut health = [0; HEALTH_SIZE];
        let len = bytes.len().min(HEALTH_SIZE);
        health[..len].copy_from_slice(&bytes[..len]);
        Self(health)
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn total_steps(&self) -> u32 {
        self.be32(0x00)
    }

    pub fn hours(&self) -> u32 {
        self.be32(0x04)
    }

    pub fn clock(&self) -> u32 {
        self.be32(0x08)
    }

    pub fn total_days(&self) -> u16 {
        self.be16(0x0C)
    }

    pub fn watts(&self) -> u16 {
        self.be16(0x0E)
    }

    pub fn walk_minutes(&self) -> u16 {
        self.be16(0x10)
    }

    pub fn steps_toward_watt(&self) -> u8 {
        self.0[0x12]
    }

    pub fn event_log_index(&self) -> u8 {
        self.0[0x13]
    }

    pub fn special_route(&self) -> bool {
        self.0[0x17] & 0x01 != 0
    }

    pub fn sound(&self) -> u8 {
        (self.0[0x17] >> 1) & 0x03
    }

    pub fn shade(&self) -> u8 {
        (self.0[0x17] >> 3) & 0x0F
    }

    pub(crate) fn reset(&mut self, steps: bool) {
        self.set_walk_minutes(0);
        self.set_watts(0);
        self.set_steps_toward_watt(0);
        self.set_event_log_index(0);
        self.0[0x17] = (self.0[0x17] & 0xA4) | 0x24;
        if steps {
            self.set_total_steps(0);
            self.set_hours(0);
            self.set_total_days(0);
            self.set_clock(FIRST_CLOCK);
        }
    }

    pub(crate) fn set_total_days(&mut self, days: u16) {
        self.0[0x0C..0x0E].copy_from_slice(&days.to_be_bytes());
    }

    pub(crate) fn set_special_route(&mut self, on: bool) {
        self.0[0x17] = (self.0[0x17] & !0x01) | u8::from(on);
    }

    pub(crate) fn set_sound(&mut self, sound: u8) {
        self.0[0x17] = (self.0[0x17] & !0x06) | ((sound & 0x03) << 1);
    }

    pub(crate) fn set_shade(&mut self, shade: u8) {
        self.0[0x17] = (self.0[0x17] & !0x78) | ((shade & 0x0F) << 3);
    }

    pub(crate) fn set_total_steps(&mut self, steps: u32) {
        self.0[0x00..0x04].copy_from_slice(&steps.to_be_bytes());
    }

    pub(crate) fn set_steps_toward_watt(&mut self, steps: u8) {
        self.0[0x12] = steps;
    }

    pub(crate) fn set_hours(&mut self, hours: u32) {
        self.0[0x04..0x08].copy_from_slice(&hours.to_be_bytes());
    }

    pub(crate) fn set_clock(&mut self, seconds: u32) {
        self.0[0x08..0x0C].copy_from_slice(&seconds.to_be_bytes());
    }

    pub(crate) fn set_watts(&mut self, watts: u16) {
        self.0[0x0E..0x10].copy_from_slice(&watts.to_be_bytes());
    }

    pub(crate) fn set_walk_minutes(&mut self, minutes: u16) {
        self.0[0x10..0x12].copy_from_slice(&minutes.to_be_bytes());
    }

    pub(crate) fn set_event_log_index(&mut self, index: u8) {
        self.0[0x13] = index;
    }

    fn be16(&self, at: usize) -> u16 {
        u16::from_be_bytes([self.0[at], self.0[at + 1]])
    }

    fn be32(&self, at: usize) -> u32 {
        u32::from_be_bytes([self.0[at], self.0[at + 1], self.0[at + 2], self.0[at + 3]])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PokemonSummary {
    pub species: u16,
    pub held_item: u16,
    pub moves: [u16; 4],
    pub level: u8,
    pub flags: [u8; 2],
}

impl PokemonSummary {
    pub const SIZE: usize = 16;

    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let word = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        Self {
            species: word(0),
            held_item: word(2),
            moves: std::array::from_fn(|i| word(4 + 2 * i)),
            level: bytes[12],
            flags: [bytes[13], bytes[14]],
        }
    }

    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[0..2].copy_from_slice(&self.species.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.held_item.to_le_bytes());
        for (i, id) in self.moves.iter().enumerate() {
            bytes[4 + 2 * i..6 + 2 * i].copy_from_slice(&id.to_le_bytes());
        }
        bytes[12] = self.level;
        bytes[13..15].copy_from_slice(&self.flags);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walker_info_reads_its_fields_at_their_offsets() {
        let mut raw = [0u8; INFO_SIZE];
        raw[0x0C..0x10].copy_from_slice(&[0x39, 0x30, 0x31, 0xD4]);
        raw[0x48..0x4C].copy_from_slice(&[0x2B, 0x01, 0x2C, 0x01]);
        raw[0x5B] = 0x03;
        raw[0x60..0x68].copy_from_slice(&[0, 0, 0x01, 0x02, 0, 0x01, 0x86, 0xA0]);
        let info = WalkerInfo(raw);
        assert_eq!((info.tid(), info.sid()), (12345, 0xD431));
        assert_eq!(info.trainer_name()[..2], [0x012B, 0x012C]);
        assert!(info.has_pokemon());
        assert_eq!(info.last_sync(), 0x0102);
        assert_eq!(info.step_count(), 100_000);
    }

    #[test]
    fn health_fields_are_big_endian() {
        let mut health = Health([0; HEALTH_SIZE]);
        health.set_watts(9_999);
        health.set_hours(99_999);
        assert_eq!(health.0[0x0E..0x10], [0x27, 0x0F]);
        assert_eq!(health.0[0x04..0x08], [0x00, 0x01, 0x86, 0x9F]);
        assert_eq!((health.watts(), health.hours()), (9_999, 99_999));
    }

    #[test]
    fn a_pokemon_summary_round_trips() {
        let summary = PokemonSummary {
            species: 25,
            held_item: 0x00D8,
            moves: [84, 98, 86, 0],
            level: 12,
            flags: [0x20, 0],
        };
        let bytes = summary.to_bytes();
        assert_eq!(bytes[..4], [25, 0, 0xD8, 0]);
        assert_eq!(PokemonSummary::from_bytes(&bytes), summary);
    }
}
