pub fn crc16(initial: u16, bytes: &[u8]) -> u16 {
    bytes.iter().fold(initial, |crc, &byte| {
        (0..8).fold(crc ^ u16::from(byte), |crc, _| {
            if crc & 1 != 0 {
                crc >> 1 ^ 0xA001
            } else {
                crc >> 1
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crc_is_the_bios_crc16() {
        assert_eq!(crc16(0xFFFF, b"123456789"), 0x4B37);
        assert_eq!(crc16(0, b"123456789"), 0xBB3D);
    }
}
