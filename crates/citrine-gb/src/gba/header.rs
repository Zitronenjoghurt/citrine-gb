use crate::rom::header::{RomHeader, RomLicensee};
use std::ops::Range;

const TITLE: Range<usize> = 0xA0..0xAC;
const GAME_CODE: Range<usize> = 0xAC..0xB0;
const MAKER: Range<usize> = 0xB0..0xB2;
const VERSION: usize = 0xBC;
const CHECKSUM: usize = 0xBD;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GbaHeader {
    pub title: String,
    pub game_code: String,
    pub maker_code: String,
    pub maker: RomLicensee,
    pub version: u8,
    pub provided_checksum: u8,
    pub actual_checksum: u8,
}

impl GbaHeader {
    pub fn parse(rom: &[u8]) -> Self {
        let maker_code: [u8; 2] = rom
            .get(MAKER)
            .and_then(|code| code.try_into().ok())
            .unwrap_or([0; 2]);
        Self {
            title: text(rom, TITLE),
            game_code: text(rom, GAME_CODE),
            maker_code: text(rom, MAKER),
            maker: match &maker_code {
                b"01" => RomLicensee::Nintendo,
                _ => RomHeader::licensee_for_code(maker_code),
            },
            version: rom.get(VERSION).copied().unwrap_or(0),
            provided_checksum: rom.get(CHECKSUM).copied().unwrap_or(0),
            actual_checksum: checksum(rom),
        }
    }
}

fn text(rom: &[u8], range: Range<usize>) -> String {
    let bytes = rom.get(range).unwrap_or_default();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

fn checksum(rom: &[u8]) -> u8 {
    let bytes = rom.get(TITLE.start..CHECKSUM).unwrap_or_default();
    bytes
        .iter()
        .fold(0u8, |sum, &b| sum.wrapping_sub(b))
        .wrapping_sub(0x19)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0xC0];
        rom[TITLE.start..TITLE.start + 7].copy_from_slice(b"CITRINE");
        rom[GAME_CODE].copy_from_slice(b"ZCSE");
        rom[MAKER].copy_from_slice(b"01");
        rom[VERSION] = 2;
        rom[0xB2] = 0x96;
        rom[CHECKSUM] = checksum(&rom);
        rom
    }

    #[test]
    fn the_header_names_the_game_its_code_and_maker() {
        let header = GbaHeader::parse(&rom());
        assert_eq!(header.title, "CITRINE");
        assert_eq!(header.game_code, "ZCSE");
        assert_eq!(header.maker_code, "01");
        assert_eq!(header.maker, RomLicensee::Nintendo);
        assert_eq!(header.version, 2);
        assert_eq!(header.provided_checksum, header.actual_checksum);
    }

    #[test]
    fn the_checksum_matches_the_bios_rule() {
        let mut rom = rom();
        rom[CHECKSUM] = 0;
        let header = GbaHeader::parse(&rom);
        let sum = rom[TITLE.start..CHECKSUM]
            .iter()
            .fold(0x19u8, |sum, &b| sum.wrapping_add(b))
            .wrapping_add(header.actual_checksum);
        assert_eq!(sum, 0);
    }

    #[test]
    fn a_short_file_gives_an_empty_header() {
        let header = GbaHeader::parse(&[0; 4]);
        assert_eq!(header.title, "");
        assert_eq!(header.game_code, "");
        assert_eq!(header.maker, RomLicensee::Unknown);
    }
}
