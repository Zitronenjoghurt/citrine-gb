#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RomId {
    pub crc32: u32,
    pub sha1: [u8; 20],
    pub sha256: [u8; 32],
}

impl RomId {
    pub fn new(data: &[u8]) -> Self {
        Self {
            crc32: crc32fast::hash(data),
            sha1: <sha1::Sha1 as sha1::Digest>::digest(data).into(),
            sha256: <sha2::Sha256 as sha2::Digest>::digest(data).into(),
        }
    }

    pub fn crc32_hex_string(&self) -> String {
        format!("{:08X}", self.crc32)
    }

    pub fn sha1_hex_string(&self) -> String {
        hex_upper(&self.sha1)
    }

    pub fn sha256_hex_string(&self) -> String {
        hex_upper(&self.sha256)
    }
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crc32_matches_the_standard_check_value() {
        assert_eq!(RomId::new(b"123456789").crc32_hex_string(), "CBF43926");
    }

    #[test]
    fn the_sha1_matches_the_fips_example() {
        assert_eq!(
            RomId::new(b"abc").sha1_hex_string(),
            "A9993E364706816ABA3E25717850C26C9CD0D89D"
        );
    }

    #[test]
    fn the_sha256_matches_the_fips_example() {
        assert_eq!(
            RomId::new(b"abc").sha256_hex_string(),
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        );
    }

    #[test]
    fn the_header_reads_its_hashes_from_the_rom_id() {
        let mut data = vec![0; 0x8000];
        data[0x134..0x138].copy_from_slice(b"TEST");
        let header = crate::rom::header::RomHeader::new(&data).unwrap();
        assert_eq!(header.id(), RomId::new(&data));
        assert_eq!(
            header.sha256_hex_string(),
            RomId::new(&data).sha256_hex_string()
        );
    }
}
