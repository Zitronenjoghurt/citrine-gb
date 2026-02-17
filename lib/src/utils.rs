pub fn lo(value: u16) -> u8 {
    value as u8
}

pub fn hi(value: u16) -> u8 {
    (value >> 8) as u8
}

pub fn lh(value: u16) -> (u8, u8) {
    (lo(value), hi(value))
}

pub fn word(lo: u8, hi: u8) -> u16 {
    u16::from_le_bytes([lo, hi])
}
