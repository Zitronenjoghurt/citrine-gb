pub(crate) const BLOCKS_START: usize = 0x08;
pub(crate) const BLOCKS_END: usize = 0x88;
const BLOCK: usize = 0x20;

const ORDERS: [[u8; 4]; 24] = [
    [0, 1, 2, 3],
    [0, 1, 3, 2],
    [0, 2, 1, 3],
    [0, 2, 3, 1],
    [0, 3, 1, 2],
    [0, 3, 2, 1],
    [1, 0, 2, 3],
    [1, 0, 3, 2],
    [1, 2, 0, 3],
    [1, 2, 3, 0],
    [1, 3, 0, 2],
    [1, 3, 2, 0],
    [2, 0, 1, 3],
    [2, 0, 3, 1],
    [2, 1, 0, 3],
    [2, 1, 3, 0],
    [2, 3, 0, 1],
    [2, 3, 1, 0],
    [3, 0, 1, 2],
    [3, 0, 2, 1],
    [3, 1, 0, 2],
    [3, 1, 2, 0],
    [3, 2, 0, 1],
    [3, 2, 1, 0],
];

pub(crate) fn shuffle_index(pid: u32) -> usize {
    ((pid & 0x3E000) >> 13) as usize % 24
}

pub(crate) fn checksum(plain: &[u8]) -> u16 {
    plain[BLOCKS_START..BLOCKS_END]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0u16, |sum, w| {
            sum.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
        })
}

pub(crate) fn xor_stream(bytes: &mut [u8], seed: u32) {
    let mut state = seed;
    for word in bytes.as_chunks_mut::<2>().0.iter_mut() {
        state = state.wrapping_mul(0x41C6_4E6D).wrapping_add(0x6073);
        let key = ((state >> 16) as u16).to_le_bytes();
        word[0] ^= key[0];
        word[1] ^= key[1];
    }
}

pub(crate) fn unshuffle(bytes: &mut [u8], pid: u32) {
    let order = ORDERS[shuffle_index(pid)];
    let stored: Vec<u8> = bytes[BLOCKS_START..BLOCKS_END].to_vec();
    for (position, &block) in order.iter().enumerate() {
        let to = BLOCKS_START + usize::from(block) * BLOCK;
        let from = position * BLOCK;
        bytes[to..to + BLOCK].copy_from_slice(&stored[from..from + BLOCK]);
    }
}

pub(crate) fn shuffle(bytes: &mut [u8], pid: u32) {
    let order = ORDERS[shuffle_index(pid)];
    let plain: Vec<u8> = bytes[BLOCKS_START..BLOCKS_END].to_vec();
    for (position, &block) in order.iter().enumerate() {
        let to = BLOCKS_START + position * BLOCK;
        let from = usize::from(block) * BLOCK;
        bytes[to..to + BLOCK].copy_from_slice(&plain[from..from + BLOCK]);
    }
}

pub(crate) fn decrypt(bytes: &mut [u8]) {
    let pid = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let check = u16::from_le_bytes([bytes[6], bytes[7]]);
    xor_stream(&mut bytes[BLOCKS_START..BLOCKS_END], u32::from(check));
    if bytes.len() > BLOCKS_END {
        xor_stream(&mut bytes[BLOCKS_END..], pid);
    }
    unshuffle(bytes, pid);
}

pub(crate) fn encrypt(bytes: &mut [u8]) {
    let pid = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let check = u16::from_le_bytes([bytes[6], bytes[7]]);
    shuffle(bytes, pid);
    xor_stream(&mut bytes[BLOCKS_START..BLOCKS_END], u32::from(check));
    if bytes.len() > BLOCKS_END {
        xor_stream(&mut bytes[BLOCKS_END..], pid);
    }
}
