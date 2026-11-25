#![no_main]

use citrine_gb_pkm::pkm::BOX_SIZE;
use citrine_gb_pkm::{Gen, Pokemon, legality};
use libfuzzer_sys::fuzz_target;

const BLOCKS_START: usize = 0x08;
const BLOCKS_END: usize = 0x88;

fn xor_stream(bytes: &mut [u8], seed: u32) {
    let mut state = seed;
    for word in bytes.chunks_exact_mut(2) {
        state = state.wrapping_mul(0x41C6_4E6D).wrapping_add(0x6073);
        let key = ((state >> 16) as u16).to_le_bytes();
        word[0] ^= key[0];
        word[1] ^= key[1];
    }
}

fn seal(bytes: &mut [u8]) {
    let check = bytes[BLOCKS_START..BLOCKS_END]
        .chunks_exact(2)
        .fold(0u16, |sum, w| {
            sum.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
        });
    bytes[6..8].copy_from_slice(&check.to_le_bytes());
    let pid = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    xor_stream(&mut bytes[BLOCKS_START..BLOCKS_END], u32::from(check));
    if bytes.len() > BLOCKS_END {
        xor_stream(&mut bytes[BLOCKS_END..], pid);
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&head, rest)) = data.split_first() else {
        return;
    };
    let generation = if head & 1 == 0 { Gen::Four } else { Gen::Five };
    let _ = Pokemon::decode(generation, rest);
    let size = if head & 2 == 0 {
        BOX_SIZE
    } else {
        generation.party_size()
    };
    let mut bytes = vec![0; size];
    let len = rest.len().min(size);
    bytes[..len].copy_from_slice(&rest[..len]);
    seal(&mut bytes);
    let Ok(pkm) = Pokemon::decode(generation, &bytes) else {
        return;
    };
    let _ = pkm.generation();
    let _ = pkm.nickname_text();
    let _ = pkm.ot_text();
    let _ = pkm.nature();
    let _ = pkm.is_shiny();
    let _ = legality::check(&pkm);
    let _ = Pokemon::decode(generation, &pkm.encode());
    let _ = Pokemon::decode(generation, &pkm.encode_box());
});
