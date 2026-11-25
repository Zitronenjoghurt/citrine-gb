const HEADER_BYTES: usize = 0x200;

pub fn game_key(rom: &[u8]) -> u64 {
    digest(&rom[..rom.len().min(HEADER_BYTES)]).max(1)
}

pub fn digest(bytes: &[u8]) -> u64 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut hash = 0xCBF2_9CE4_8422_2325u64 ^ bytes.len() as u64;
    let (chunks, rest) = bytes.as_chunks::<8>();
    for chunk in chunks {
        let word = u64::from_le_bytes(*chunk);
        hash = (hash ^ word).wrapping_mul(K).rotate_left(29);
    }
    for &byte in rest {
        hash = (hash ^ u64::from(byte)).wrapping_mul(K).rotate_left(29);
    }
    hash ^ (hash >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_key_reads_only_the_header() {
        let mut rom = vec![7; 0x1000];
        let key = game_key(&rom);
        rom[0x200] = 8;
        assert_eq!(game_key(&rom), key);
        rom[0x14E] = 8;
        assert_ne!(game_key(&rom), key);
    }
}
