use crate::game_name::game_name;
use std::ops::Range;

pub const HEAD_BYTES: usize = 0x150;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RomKind {
    GameBoy,
    Advance,
    Ds,
}

impl RomKind {
    fn title_bytes(self) -> Range<usize> {
        match self {
            Self::GameBoy => 0x134..0x144,
            Self::Advance => 0xA0..0xAC,
            Self::Ds => 0x00..0x0C,
        }
    }
}

pub fn header_title(kind: RomKind, head: &[u8]) -> String {
    let Some(bytes) = head.get(kind.title_bytes()) else {
        return String::new();
    };
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let title: String = bytes[..end]
        .iter()
        .filter(|b| b.is_ascii_graphic() || **b == b' ')
        .map(|&b| b as char)
        .collect();
    let title = title.trim();
    let colour = head.get(0x143).is_some_and(|flag| flag & 0x80 != 0);
    match kind {
        RomKind::GameBoy
            if colour
                && title.len() == 15
                && title[11..]
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) =>
        {
            title[..11].to_string()
        }
        _ => title.to_string(),
    }
}

pub fn ds_game_code(head: &[u8]) -> Option<[u8; 4]> {
    head.get(0x0C..0x10)?.try_into().ok()
}

pub fn rom_name(kind: RomKind, sha1: &[u8; 20], head: &[u8], file_name: &str) -> String {
    let title = header_title(kind, head);
    #[cfg(feature = "nds")]
    if kind == RomKind::Ds
        && let Some(code) = ds_game_code(head)
    {
        return crate::game_name::ds_game_name(code, None, Some(file_name), &title);
    }
    game_name(sha1, Some(file_name), &title)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(at: usize, bytes: &[u8]) -> Vec<u8> {
        let mut head = vec![0; HEAD_BYTES];
        head[at..at + bytes.len()].copy_from_slice(bytes);
        head
    }

    #[test]
    fn each_console_reads_its_title_from_its_own_place() {
        assert_eq!(
            header_title(RomKind::GameBoy, &head(0x134, b"ALLEY WAY")),
            "ALLEY WAY"
        );
        assert_eq!(
            header_title(RomKind::Advance, &head(0xA0, b"METROID4USA\0AMTE")),
            "METROID4USA"
        );
        assert_eq!(
            header_title(RomKind::Ds, &head(0x00, b"POKEMON PL\0\0CPUE")),
            "POKEMON PL"
        );
        assert_eq!(
            ds_game_code(&head(0x00, b"POKEMON PL\0\0CPUE")),
            Some(*b"CPUE")
        );
    }

    #[test]
    fn a_colour_title_drops_its_maker_code() {
        let mut rom = head(0x134, b"POKEMON YELLAPSE");
        rom[0x143] = 0x80;
        assert_eq!(header_title(RomKind::GameBoy, &rom), "POKEMON YEL");
    }

    #[test]
    fn a_short_or_blank_head_gives_an_empty_title() {
        assert_eq!(header_title(RomKind::Advance, &[0; 0x20]), "");
        assert_eq!(
            header_title(RomKind::GameBoy, &head(0x134, b"\x01\x02")),
            ""
        );
        assert_eq!(ds_game_code(&[0; 8]), None);
    }

    #[test]
    fn an_unknown_rom_is_named_after_its_file_then_its_title() {
        let rom = head(0xA0, b"HOMEBREWGAME");
        assert_eq!(
            rom_name(RomKind::Advance, &[0xAB; 20], &rom, "my_game (Demo).gba"),
            "My Game"
        );
        assert_eq!(
            rom_name(RomKind::Advance, &[0xAB; 20], &rom, "(Demo).gba"),
            "Homebrewgame"
        );
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_ds_game_is_named_by_its_game_code() {
        let rom = head(0x00, b"POKEMON PL\0\0CPUE");
        assert_eq!(
            rom_name(RomKind::Ds, &[0xAB; 20], &rom, "4997.nds"),
            crate::game_name::known_ds_name(*b"CPUE").unwrap()
        );
    }
}
