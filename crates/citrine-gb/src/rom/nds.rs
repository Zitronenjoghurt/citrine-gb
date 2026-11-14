use crate::rom::header::{RomHeader, RomLicensee};
use crate::utils::crc16::crc16;

pub const NDS_LOGO_CRC: u16 = 0xCF56;
pub const ICON_SIZE: usize = 32;
pub const ICON_BYTES: usize = 0x220;
const HEADER_LEN: usize = 0x160;
const HEADER_CRC_AT: usize = 0x15E;
const LOGO_CRC_AT: usize = 0x15C;
const BANNER_AT: usize = 0x68;
const USED_AT: usize = 0x80;
const BANNER_LEN: usize = 0x840;
const BANNER_MAX_LEN: usize = 0xA40;
const ICON_AT: usize = 0x20;
const PALETTE_AT: usize = 0x220;
const TITLES_AT: usize = 0x240;
const TITLE_LEN: usize = 0x100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NdsLanguage {
    Japanese,
    English,
    French,
    German,
    Italian,
    Spanish,
    Chinese,
    Korean,
}

impl NdsLanguage {
    pub const ALL: [NdsLanguage; 8] = [
        NdsLanguage::Japanese,
        NdsLanguage::English,
        NdsLanguage::French,
        NdsLanguage::German,
        NdsLanguage::Italian,
        NdsLanguage::Spanish,
        NdsLanguage::Chinese,
        NdsLanguage::Korean,
    ];

    pub fn from_tag(tag: &str) -> Option<Self> {
        let primary = tag.split(['-', '_', '.']).next()?.to_ascii_lowercase();
        Some(match primary.as_str() {
            "ja" => Self::Japanese,
            "en" => Self::English,
            "fr" => Self::French,
            "de" => Self::German,
            "it" => Self::Italian,
            "es" => Self::Spanish,
            "zh" => Self::Chinese,
            "ko" => Self::Korean,
            _ => return None,
        })
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NdsHeader {
    pub title: String,
    pub game_code: [u8; 4],
    pub maker: [u8; 2],
    pub version: u8,
    pub banner_offset: usize,
    pub provided_checksum: u16,
    pub actual_checksum: u16,
}

impl NdsHeader {
    pub fn is_nds(data: &[u8]) -> bool {
        data.len() >= HEADER_LEN
            && read_u16(data, LOGO_CRC_AT) == NDS_LOGO_CRC
            && read_u16(data, HEADER_CRC_AT) == crc16(0xFFFF, &data[..HEADER_CRC_AT])
    }

    pub fn played_len(head: &[u8], len: u64) -> u64 {
        if !Self::is_nds(head) {
            return len;
        }
        let used = u64::from(read_u32(head, USED_AT));
        if (HEADER_LEN as u64..=len).contains(&used) {
            used
        } else {
            len
        }
    }

    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_LEN {
            return None;
        }
        Some(Self {
            title: ascii(&data[..12]),
            game_code: data[0x0C..0x10].try_into().ok()?,
            maker: data[0x10..0x12].try_into().ok()?,
            version: data[0x1E],
            banner_offset: read_u32(data, BANNER_AT) as usize,
            provided_checksum: read_u16(data, HEADER_CRC_AT),
            actual_checksum: crc16(0xFFFF, &data[..HEADER_CRC_AT]),
        })
    }

    pub fn game_code(&self) -> String {
        ascii(&self.game_code)
    }

    pub fn maker(&self) -> String {
        ascii(&self.maker)
    }

    pub fn maker_licensee(&self) -> RomLicensee {
        RomHeader::maker_for_code(self.maker)
    }

    pub fn is_homebrew(&self) -> bool {
        self.game_code == *b"####" || self.game_code == [0; 4]
    }

    pub fn banner(&self, data: &[u8]) -> Option<NdsBanner> {
        if self.banner_offset == 0 {
            return None;
        }
        let banner = data.get(self.banner_offset..)?;
        NdsBanner::parse(&banner[..banner.len().min(BANNER_MAX_LEN)])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NdsBanner {
    pub icon: [u8; ICON_BYTES],
    titles: Vec<Option<String>>,
}

impl NdsBanner {
    pub fn parse(banner: &[u8]) -> Option<Self> {
        if banner.len() < BANNER_LEN {
            return None;
        }
        let version = read_u16(banner, 0);
        let languages = match version {
            0x0001 => 6,
            0x0002 => 7,
            0x0003 | 0x0103 => 8,
            _ => return None,
        };
        let titles = NdsLanguage::ALL
            .iter()
            .map(|language| {
                let at = TITLES_AT + language.index() * TITLE_LEN;
                banner
                    .get(at..at + TITLE_LEN)
                    .filter(|_| language.index() < languages)
                    .map(utf16)
                    .filter(|title| !title.is_empty())
            })
            .collect();
        Some(Self {
            icon: banner[ICON_AT..ICON_AT + ICON_BYTES].try_into().ok()?,
            titles,
        })
    }

    pub fn text(&self, language: NdsLanguage) -> Option<&str> {
        self.titles[language.index()]
            .as_deref()
            .or(self.titles[NdsLanguage::English.index()].as_deref())
            .or_else(|| self.titles.iter().flatten().next().map(String::as_str))
    }

    pub fn title(&self, language: NdsLanguage) -> Option<String> {
        let lines: Vec<&str> = self.lines(language);
        match lines.len() {
            0 => None,
            1 => Some(lines[0].to_string()),
            count => Some(lines[..count - 1].join(" ")),
        }
    }

    pub fn publisher(&self, language: NdsLanguage) -> Option<String> {
        let lines = self.lines(language);
        (lines.len() > 1).then(|| lines[lines.len() - 1].to_string())
    }

    fn lines(&self, language: NdsLanguage) -> Vec<&str> {
        self.text(language)
            .map(|text| {
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

pub fn icon_rgba(icon: &[u8]) -> Option<Vec<u8>> {
    if icon.len() != ICON_BYTES {
        return None;
    }
    let palette: Vec<[u8; 4]> = (0..16)
        .map(|index| {
            let color = read_u16(icon, PALETTE_AT - ICON_AT + index * 2);
            let channel = |shift: u16| {
                let value = ((color >> shift) & 0x1F) as u8;
                (value << 3) | (value >> 2)
            };
            let alpha = if index == 0 { 0 } else { 0xFF };
            [channel(0), channel(5), channel(10), alpha]
        })
        .collect();
    let mut rgba = vec![0; ICON_SIZE * ICON_SIZE * 4];
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let tile = (y / 8) * 4 + x / 8;
            let byte = icon[tile * 32 + (y % 8) * 4 + (x % 8) / 2];
            let index = if x % 2 == 0 { byte & 0x0F } else { byte >> 4 };
            let pixel = (y * ICON_SIZE + x) * 4;
            rgba[pixel..pixel + 4].copy_from_slice(&palette[usize::from(index)]);
        }
    }
    Some(rgba)
}

fn read_u16(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

fn read_u32(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap_or_default())
}

fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&byte| byte != 0)
        .map(|&byte| byte as char)
        .collect::<String>()
        .trim()
        .to_string()
}

fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16_lossy(&units).trim().to_string()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn nds_rom(titles: &[&str]) -> Vec<u8> {
        let mut data = vec![0; 0x1000];
        data[..12].copy_from_slice(b"CITRINE TEST");
        data[0x0C..0x10].copy_from_slice(b"####");
        data[0x10..0x12].copy_from_slice(b"00");
        data[0x68..0x6C].copy_from_slice(&0x200u32.to_le_bytes());
        data[0x15C..0x15E].copy_from_slice(&NDS_LOGO_CRC.to_le_bytes());
        let crc = crc16(0xFFFF, &data[..0x15E]);
        data[0x15E..0x160].copy_from_slice(&crc.to_le_bytes());
        let banner = &mut data[0x200..0x200 + BANNER_LEN];
        banner[0..2].copy_from_slice(&1u16.to_le_bytes());
        banner[PALETTE_AT + 2..PALETTE_AT + 4].copy_from_slice(&0x001Fu16.to_le_bytes());
        banner[ICON_AT] = 0x10;
        for (index, title) in titles.iter().enumerate() {
            let at = TITLES_AT + index * TITLE_LEN;
            for (unit, value) in title.encode_utf16().enumerate() {
                banner[at + unit * 2..at + unit * 2 + 2].copy_from_slice(&value.to_le_bytes());
            }
        }
        data
    }

    pub(crate) fn with_used(mut data: Vec<u8>, used: u32) -> Vec<u8> {
        data[USED_AT..USED_AT + 4].copy_from_slice(&used.to_le_bytes());
        let crc = crc16(0xFFFF, &data[..HEADER_CRC_AT]);
        data[HEADER_CRC_AT..HEADER_LEN].copy_from_slice(&crc.to_le_bytes());
        data
    }

    #[test]
    fn a_ds_game_plays_up_to_its_used_size() {
        let data = with_used(nds_rom(&[]), 0xC00);
        assert_eq!(NdsHeader::played_len(&data, 0x1000), 0xC00);
        assert_eq!(NdsHeader::played_len(&data, 0x800), 0x800);
        let unset = with_used(nds_rom(&[]), 0);
        assert_eq!(NdsHeader::played_len(&unset, 0x1000), 0x1000);
        assert_eq!(NdsHeader::played_len(&[0; 0x200], 0x200), 0x200);
    }

    #[test]
    fn a_header_with_both_crcs_is_a_ds_game() {
        let data = nds_rom(&[]);
        assert!(NdsHeader::is_nds(&data));
        let mut broken = data.clone();
        broken[0] ^= 1;
        assert!(!NdsHeader::is_nds(&broken));
        let header = NdsHeader::parse(&broken).unwrap();
        assert_ne!(header.provided_checksum, header.actual_checksum);
        assert!(NdsHeader::parse(&data[..0x100]).is_none());
        let header = NdsHeader::parse(&data).unwrap();
        assert_eq!(header.provided_checksum, header.actual_checksum);
        assert_eq!(header.title, "CITRINE TEST");
        assert_eq!(header.game_code(), "####");
        assert_eq!(header.maker_licensee(), RomLicensee::None);
        assert!(header.is_homebrew());
    }

    #[test]
    fn the_banner_title_follows_the_language_and_falls_back_to_english() {
        let data = nds_rom(&[
            "",
            "Both Screens\nConsole\nblocksds.skylyrac.net",
            "Deux écrans\nBlocksDS",
        ]);
        let header = NdsHeader::parse(&data).unwrap();
        let banner = header.banner(&data).unwrap();
        assert_eq!(
            banner.title(NdsLanguage::French).as_deref(),
            Some("Deux écrans")
        );
        assert_eq!(
            banner.publisher(NdsLanguage::French).as_deref(),
            Some("BlocksDS")
        );
        assert_eq!(
            banner.title(NdsLanguage::German).as_deref(),
            Some("Both Screens Console")
        );
        assert_eq!(
            banner.title(NdsLanguage::Korean).as_deref(),
            Some("Both Screens Console")
        );
        assert_eq!(
            banner.title(NdsLanguage::Japanese).as_deref(),
            Some("Both Screens Console")
        );
    }

    #[test]
    fn a_newer_banner_holds_chinese_and_korean_titles() {
        let mut data = nds_rom(&["", "English"]);
        data[0x200..0x202].copy_from_slice(&0x0103u16.to_le_bytes());
        let at = 0x200 + TITLES_AT + 7 * TITLE_LEN;
        for (unit, value) in "한국어".encode_utf16().enumerate() {
            data[at + unit * 2..at + unit * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        let banner = NdsHeader::parse(&data).unwrap().banner(&data).unwrap();
        assert_eq!(banner.title(NdsLanguage::Korean).as_deref(), Some("한국어"));
        assert_eq!(
            banner.title(NdsLanguage::Chinese).as_deref(),
            Some("English")
        );
        let short = &data[..0x200 + BANNER_LEN];
        let banner = NdsHeader::parse(short).unwrap().banner(short).unwrap();
        assert_eq!(
            banner.title(NdsLanguage::Korean).as_deref(),
            Some("English")
        );
    }

    #[test]
    fn the_icon_reads_its_tiles_and_palette() {
        let data = nds_rom(&["Icon"]);
        let banner = NdsHeader::parse(&data).unwrap().banner(&data).unwrap();
        let rgba = icon_rgba(&banner.icon).unwrap();
        assert_eq!(rgba.len(), ICON_SIZE * ICON_SIZE * 4);
        assert_eq!(&rgba[0..4], &[0, 0, 0, 0]);
        assert_eq!(&rgba[4..8], &[0xFF, 0, 0, 0xFF]);
        assert!(icon_rgba(&[0; 4]).is_none());
    }

    #[test]
    fn language_tags_pick_their_banner_language() {
        assert_eq!(NdsLanguage::from_tag("de-DE"), Some(NdsLanguage::German));
        assert_eq!(
            NdsLanguage::from_tag("en_US.UTF-8"),
            Some(NdsLanguage::English)
        );
        assert_eq!(NdsLanguage::from_tag("pt-BR"), None);
    }
}
