pub const KEY_TABLE_OFFSET: usize = 0x30;
pub const KEY_TABLE_BYTES: usize = 0x1048;
const KEY_TABLE_WORDS: usize = KEY_TABLE_BYTES / 4;
const CARD_MODULO: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct KeyTable(Vec<u32>);

impl KeyTable {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let table = bytes.get(..KEY_TABLE_BYTES)?;
        Some(Self(
            table
                .as_chunks::<4>()
                .0
                .iter()
                .map(|word| u32::from_le_bytes(*word))
                .collect(),
        ))
    }

    pub fn from_arm7_bios(bios: &[u8]) -> Option<Self> {
        Self::from_bytes(bios.get(KEY_TABLE_OFFSET..)?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Key1 {
    words: Vec<u32>,
    keycode: [u32; 3],
}

impl Key1 {
    pub fn new(table: &KeyTable, gamecode: u32, level: u8) -> Self {
        let mut key = Self {
            words: table.0.clone(),
            keycode: [gamecode, gamecode >> 1, gamecode << 1],
        };
        debug_assert_eq!(key.words.len(), KEY_TABLE_WORDS);
        if level >= 1 {
            key.apply_keycode();
        }
        if level >= 2 {
            key.apply_keycode();
        }
        key.keycode[1] = key.keycode[1].wrapping_shl(1);
        key.keycode[2] >>= 1;
        if level >= 3 {
            key.apply_keycode();
        }
        key
    }

    pub fn for_commands(table: &KeyTable, gamecode: u32) -> Self {
        Self::new(table, gamecode, 2)
    }

    pub fn encrypt(&self, value: u64) -> u64 {
        let mut y = value as u32;
        let mut x = (value >> 32) as u32;
        for index in 0..0x10 {
            let z = self.words[index] ^ x;
            x = self.round(z) ^ y;
            y = z;
        }
        join(x ^ self.words[0x10], y ^ self.words[0x11])
    }

    pub fn decrypt(&self, value: u64) -> u64 {
        let mut y = value as u32;
        let mut x = (value >> 32) as u32;
        for index in (0x02..0x12).rev() {
            let z = self.words[index] ^ x;
            x = self.round(z) ^ y;
            y = z;
        }
        join(x ^ self.words[0x01], y ^ self.words[0x00])
    }

    fn round(&self, z: u32) -> u32 {
        let [a, b, c, d] = z.to_be_bytes();
        let mut x = self.words[0x012 + usize::from(a)];
        x = x.wrapping_add(self.words[0x112 + usize::from(b)]);
        x ^= self.words[0x212 + usize::from(c)];
        x.wrapping_add(self.words[0x312 + usize::from(d)])
    }

    fn apply_keycode(&mut self) {
        let high = self.encrypt(join(self.keycode[1], self.keycode[2]));
        self.keycode[1] = high as u32;
        self.keycode[2] = (high >> 32) as u32;
        let low = self.encrypt(join(self.keycode[0], self.keycode[1]));
        self.keycode[0] = low as u32;
        self.keycode[1] = (low >> 32) as u32;
        for index in 0..0x12 {
            self.words[index] ^= self.keycode[index % CARD_MODULO].swap_bytes();
        }
        let mut scratch = 0u64;
        for index in (0..0x412).step_by(2) {
            scratch = self.encrypt(scratch);
            self.words[index] = (scratch >> 32) as u32;
            self.words[index + 1] = scratch as u32;
        }
    }
}

fn join(low: u32, high: u32) -> u64 {
    u64::from(high) << 32 | u64::from(low)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREEBIOS7: &[u8] = include_bytes!("../../../assets/nds-bios/freebios7.bin");

    fn table() -> KeyTable {
        let bytes: Vec<u8> = (0..KEY_TABLE_BYTES)
            .map(|index| (index.wrapping_mul(0x9E37) >> 3) as u8)
            .collect();
        KeyTable::from_bytes(&bytes).unwrap()
    }

    #[test]
    fn the_table_comes_from_offset_30_of_the_arm7_bios() {
        let mut bios = vec![0; KEY_TABLE_OFFSET];
        bios.extend((0..KEY_TABLE_BYTES).map(|index| index as u8));
        let table = KeyTable::from_arm7_bios(&bios).unwrap();
        assert_eq!(table.0[0], 0x0302_0100);
        assert_eq!(table.0.len(), KEY_TABLE_WORDS);
    }

    #[test]
    fn freebios_fills_the_key_table_slot() {
        let table = KeyTable::from_arm7_bios(FREEBIOS7).unwrap();
        assert_eq!(table.0[0].to_le_bytes(), FREEBIOS7[0x30..0x34]);
    }

    #[test]
    fn a_short_bios_has_no_table() {
        assert!(KeyTable::from_arm7_bios(&[0; KEY_TABLE_OFFSET + KEY_TABLE_BYTES - 1]).is_none());
    }

    #[test]
    fn decrypt_undoes_encrypt_at_every_level() {
        for level in 0..=3 {
            let key = Key1::new(&table(), u32::from_le_bytes(*b"ABCD"), level);
            for value in [0, 1, 0x0123_4567_89AB_CDEF, u64::MAX] {
                let encrypted = key.encrypt(value);
                assert_ne!(encrypted, value, "level {level}");
                assert_eq!(key.decrypt(encrypted), value, "level {level}");
            }
        }
    }

    #[test]
    fn levels_and_gamecodes_give_different_keys() {
        let a = Key1::new(&table(), 0x4142_4344, 2);
        let b = Key1::new(&table(), 0x4142_4345, 2);
        let c = Key1::new(&table(), 0x4142_4344, 3);
        assert_ne!(a.encrypt(0), b.encrypt(0));
        assert_ne!(a.encrypt(0), c.encrypt(0));
        assert_eq!(Key1::for_commands(&table(), 0x4142_4344), a);
    }

    #[test]
    fn level_zero_keeps_the_table_words() {
        let key = Key1::new(&table(), 7, 0);
        assert_eq!(key.words, table().0);
    }

    #[test]
    fn encrypt_is_the_blowfish_round_on_the_table() {
        let mut words = vec![0; KEY_TABLE_WORDS];
        words[0x10] = 0xAAAA_0000;
        words[0x11] = 0x0000_5555;
        let key = Key1 {
            words,
            keycode: [0; 3],
        };
        assert_eq!(key.encrypt(0), 0x0000_5555_AAAA_0000);
    }
}
