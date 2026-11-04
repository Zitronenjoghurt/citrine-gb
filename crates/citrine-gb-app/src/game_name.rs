use crate::homebrew::HOMEBREW_GAMES;
use std::collections::BTreeMap;
use std::sync::OnceLock;

mod tags;

use tags::without_tags;

static TABLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/game_names.br"));
const KEY_LEN: usize = 8;
const UPPER_WORDS: &[&str] = &["DX", "GB", "GBC"];

struct Table {
    keys: Vec<[u8; KEY_LEN]>,
    names: Vec<u16>,
    titles: Vec<String>,
}

impl Table {
    fn load() -> Self {
        let mut bytes = Vec::new();
        let mut decompressor = brotli::Decompressor::new(TABLE, 4096);
        std::io::Read::read_to_end(&mut decompressor, &mut bytes)
            .expect("Failed to decompress the game names");
        let count = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        let (keys, rest) = bytes[4..].split_at(count * KEY_LEN);
        let (names, titles) = rest.split_at(count * 2);
        Self {
            keys: keys.as_chunks::<KEY_LEN>().0.to_vec(),
            names: names
                .as_chunks::<2>()
                .0
                .iter()
                .map(|name| u16::from_le_bytes(*name))
                .collect(),
            titles: std::str::from_utf8(titles)
                .expect("Game names are not UTF-8")
                .split('\n')
                .map(str::to_string)
                .collect(),
        }
    }

    fn get() -> &'static Self {
        static LOADED: OnceLock<Table> = OnceLock::new();
        LOADED.get_or_init(Self::load)
    }

    fn find(&self, sha1: &[u8; 20]) -> Option<&str> {
        let key: [u8; KEY_LEN] = sha1[..KEY_LEN].try_into().unwrap();
        let index = self.keys.binary_search(&key).ok()?;
        Some(&self.titles[self.names[index] as usize])
    }
}

pub fn known_name(sha1: &[u8; 20]) -> Option<&'static str> {
    if let Some(name) = Table::get().find(sha1) {
        return Some(name);
    }
    HOMEBREW_GAMES
        .iter()
        .find(|game| game.sha1 == *sha1)
        .map(|game| game.title)
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct GameNames(BTreeMap<String, String>);

impl GameNames {
    pub fn learn(&mut self, rom_key: &str, name: &str) -> bool {
        if name.is_empty() || self.0.get(rom_key).is_some_and(|known| known == name) {
            return false;
        }
        self.0.insert(rom_key.to_string(), name.to_string());
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(key, name)| (key.as_str(), name.as_str()))
    }

    pub fn name_or<'a>(&'a self, rom_key: Option<&str>, title: &'a str) -> &'a str {
        rom_key
            .and_then(|key| self.0.get(key))
            .map_or(title, String::as_str)
    }
}

pub fn name_or<'a>(sha1: &[u8; 20], title: &'a str) -> &'a str {
    known_name(sha1).unwrap_or(title)
}

pub fn game_name(sha1: &[u8; 20], file_name: Option<&str>, header_title: &str) -> String {
    if let Some(name) = known_name(sha1) {
        return name.to_string();
    }
    if let Some(name) = file_name
        .map(from_file_name)
        .filter(|name| !name.is_empty())
    {
        return name;
    }
    from_header(header_title)
}

fn from_file_name(file_name: &str) -> String {
    let stem = match file_name.rsplit_once('.') {
        Some((stem, extension))
            if !stem.is_empty()
                && (1..=4).contains(&extension.len())
                && extension.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            stem
        }
        _ => file_name,
    };
    let name = single_spaced(&without_tags(stem).replace('_', " "));
    let name = name
        .split(" - ")
        .map(article_first)
        .collect::<Vec<_>>()
        .join(" - ");
    let has_lower = name.chars().any(char::is_lowercase);
    let has_upper = name.chars().any(char::is_uppercase);
    if has_lower && has_upper {
        name
    } else {
        title_case(&name)
    }
}

fn from_header(title: &str) -> String {
    let title = title.trim();
    let title = title.replace('_', " ");
    let title = title.trim_end_matches(|c: char| !c.is_alphanumeric());
    title_case(&single_spaced(title))
}

fn single_spaced(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn article_first(part: &str) -> String {
    match part.strip_suffix(", The") {
        Some(rest) => format!("The {rest}"),
        None => part.to_string(),
    }
}

fn title_case(name: &str) -> String {
    name.split(' ')
        .map(|word| {
            let upper = word.to_uppercase();
            if UPPER_WORDS.contains(&upper.as_str()) || is_roman_numeral(&upper) {
                return upper;
            }
            let mut out = String::with_capacity(word.len());
            let mut start = true;
            for c in word.chars() {
                if start {
                    out.extend(c.to_uppercase());
                } else {
                    out.extend(c.to_lowercase());
                }
                start = matches!(c, '-' | '.' | '/' | '&' | ':');
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_roman_numeral(word: &str) -> bool {
    matches!(
        word,
        "II" | "III" | "IV" | "VI" | "VII" | "VIII" | "IX" | "XI" | "XII"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLEYWAY: &str = "0cf2b8d0428f389f5361f67a0cd1ace05a1c75cc";

    fn sha1(hex: &str) -> [u8; 20] {
        let mut out = [0; 20];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }

    #[test]
    fn the_table_loads_both_lists_with_tidy_names() {
        let table = Table::get();
        assert!(table.keys.len() > 3000, "{}", table.keys.len());
        assert_eq!(table.keys.len(), table.names.len());
        assert!(table.keys.is_sorted());
        for title in &table.titles {
            assert!(!title.is_empty());
            assert_eq!(without_tags(title), title);
        }
    }

    #[test]
    fn a_listed_game_gets_its_name_without_the_region() {
        assert_eq!(
            game_name(&sha1(ALLEYWAY), Some("alley.gb"), "ALLEY WAY"),
            "Alleyway"
        );
    }

    #[test]
    fn a_listed_gba_game_gets_its_name_without_the_region_or_revision() {
        let cases = [
            (
                "5de8536afe1f0078ee6fe1089f890e8c7aa0a6e8",
                "Metroid - Zero Mission",
            ),
            (
                "7c7107b87c3ccf6e3dbceb9cf80ceeffb25a1857",
                "Pocket Monsters - Fire Red",
            ),
            ("c37e3bae84b53e6972ed7608541a62896fa5d6a3", "Fire Emblem"),
            (
                "2c7f8bc4ce37043a8f62713038591e861f812499",
                "Pokémon Pinball - Ruby & Sapphire",
            ),
        ];
        for (hash, name) in cases {
            assert_eq!(known_name(&sha1(hash)), Some(name));
            assert_eq!(game_name(&sha1(hash), Some("x.gba"), "IGNORED"), name);
        }
    }

    #[test]
    fn an_unknown_gba_rom_is_named_after_its_file_without_the_tags() {
        let unknown = [0xAB; 20];
        let cases = [
            (
                "Metroid - Zero Mission (USA, Europe) (Rev 1).gba",
                "Metroid - Zero Mission",
            ),
            (
                "Pocket Monsters - Fire Red (Japan, rev. 1).gba",
                "Pocket Monsters - Fire Red",
            ),
            (
                "Fire Emblem (Europe, English / French / German) [!].gba",
                "Fire Emblem",
            ),
        ];
        for (file, name) in cases {
            assert_eq!(game_name(&unknown, Some(file), "IGNORED"), name);
        }
    }

    #[test]
    fn a_shelf_game_gets_its_shelf_title() {
        for game in HOMEBREW_GAMES {
            assert_eq!(game_name(&game.sha1, Some("x.gb"), "IGNORED"), game.title);
        }
    }

    #[test]
    fn an_unknown_rom_is_named_after_its_file_without_the_tags() {
        let unknown = [0xAB; 20];
        let cases = [
            (
                "Legend of Zelda, The - Link's Awakening DX (USA, Europe) (Rev 2) (SGB Enhanced).gbc",
                "The Legend of Zelda - Link's Awakening DX",
            ),
            ("Pokemon Silver (U) [C][!].gbc", "Pokemon Silver"),
            ("my_cool_hack.gb", "My Cool Hack"),
            ("TETRIS.GB", "Tetris"),
            ("Mega Man V.gb", "Mega Man V"),
            ("Kirby's Dream Land 2.gb", "Kirby's Dream Land 2"),
        ];
        for (file, name) in cases {
            assert_eq!(game_name(&unknown, Some(file), "IGNORED"), name);
        }
    }

    #[test]
    fn without_a_usable_file_name_the_header_title_is_tidied() {
        let unknown = [0xAB; 20];
        assert_eq!(game_name(&unknown, None, "POKEMON_SLV"), "Pokemon Slv");
        assert_eq!(
            game_name(&unknown, Some("(Unknown).gb"), "SUPER MARIOLAND"),
            "Super Marioland"
        );
        assert_eq!(game_name(&unknown, None, "ZELDA II__-"), "Zelda II");
        assert_eq!(game_name(&unknown, None, "DR.MARIO"), "Dr.Mario");
        assert_eq!(game_name(&unknown, None, "  "), "");
    }
}
