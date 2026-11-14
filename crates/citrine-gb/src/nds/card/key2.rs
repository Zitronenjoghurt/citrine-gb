const SEED_BITS: u32 = 39;
const SEED_MASK: u64 = (1 << SEED_BITS) - 1;
const SEED_BYTES: [u8; 8] = [0xE8, 0x4D, 0x5A, 0xB1, 0x17, 0x8F, 0x99, 0xD5];
pub const SEED1: u64 = 0x5C_879B_9B05;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Key2 {
    x: u64,
    y: u64,
}

impl Key2 {
    pub fn new(seed0: u64, seed1: u64) -> Self {
        Self {
            x: reverse(seed0),
            y: reverse(seed1),
        }
    }

    pub fn next(&mut self) -> u8 {
        self.x =
            ((self.x >> 5 ^ self.x >> 17 ^ self.x >> 18 ^ self.x >> 31) & 0xFF) + (self.x << 8);
        self.y =
            ((self.y >> 5 ^ self.y >> 23 ^ self.y >> 18 ^ self.y >> 31) & 0xFF) + (self.y << 8);
        self.x &= SEED_MASK;
        self.y &= SEED_MASK;
        (self.x ^ self.y) as u8
    }

    pub fn next_word(&mut self) -> u32 {
        u32::from_le_bytes([0; 4].map(|_| self.next()))
    }

    pub fn skip(&mut self, count: u32) {
        for _ in 0..count {
            self.next();
        }
    }

    pub fn apply(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            *byte ^= self.next();
        }
    }
}

pub fn seed0(parameter: u32, seed_select: u8) -> u64 {
    u64::from(parameter & 0x00FF_FFFF) << 15
        | 0x6000
        | u64::from(SEED_BYTES[usize::from(seed_select & 7)])
}

fn reverse(seed: u64) -> u64 {
    (seed & SEED_MASK).reverse_bits() >> (64 - SEED_BITS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_load_bit_reversed_over_39_bits() {
        let key = Key2::new(1, 1 << 38);
        assert_eq!(key.x, 1 << 38);
        assert_eq!(key.y, 1);
    }

    #[test]
    fn seed0_packs_the_parameter_and_the_header_byte() {
        assert_eq!(seed0(0x12_3456, 0), 0x12_3456 << 15 | 0x6000 | 0xE8);
        assert_eq!(seed0(0, 5), 0x6000 | 0x8F);
        assert_eq!(seed0(0, 0xF), 0x6000 | 0xD5);
    }

    #[test]
    fn the_registers_stay_inside_39_bits() {
        let mut key = Key2::new(SEED_MASK, SEED1);
        for _ in 0..1000 {
            key.next();
            assert_eq!(key.x & !SEED_MASK, 0);
            assert_eq!(key.y & !SEED_MASK, 0);
        }
    }

    #[test]
    fn one_step_shifts_in_the_tap_byte() {
        let mut key = Key2 { x: 1 << 5, y: 0 };
        assert_eq!(key.next(), 0x01);
        assert_eq!(key.x, 1 << 13 | 1);
    }

    #[test]
    fn applying_the_same_stream_twice_restores_the_data() {
        let data: Vec<u8> = (0..=255).collect();
        let mut scrambled = data.clone();
        Key2::new(0x58_C56D_E0E8, SEED1).apply(&mut scrambled);
        assert_ne!(scrambled, data);
        Key2::new(0x58_C56D_E0E8, SEED1).apply(&mut scrambled);
        assert_eq!(scrambled, data);
    }
}
