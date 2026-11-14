use citrine_gb::console::ConsoleKind;

const KEY_LEN: usize = 8;
const REGIONS: &[&str] = &[
    "World",
    "USA",
    "Europe",
    "Japan",
    "Germany",
    "France",
    "Italy",
    "Spain",
    "Korea",
    "Australia",
    "Asia",
    "Taiwan",
    "China",
    "Brazil",
    "Canada",
    "Netherlands",
    "Sweden",
    "UK",
];
const UNWANTED_TAGS: &[&str] = &["(Demo", "(Kiosk", "(Beta", "(Proto", "(Sample", "(Pirate"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum System {
    Gb,
    Gbc,
    Gba,
    Nds,
}

impl System {
    pub fn of(console: ConsoleKind, rom: &[u8]) -> Self {
        match console {
            ConsoleKind::Gba => Self::Gba,
            ConsoleKind::Nds => Self::Nds,
            _ if rom.get(0x143).is_some_and(|flag| flag & 0x80 != 0) => Self::Gbc,
            _ => Self::Gb,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Gb => "Nintendo - Game Boy",
            Self::Gbc => "Nintendo - Game Boy Color",
            Self::Gba => "Nintendo - Game Boy Advance",
            Self::Nds => "Nintendo - Nintendo DS",
        }
    }

    pub fn sibling(self) -> Option<Self> {
        match self {
            Self::Gb => Some(Self::Gbc),
            Self::Gbc => Some(Self::Gb),
            Self::Gba | Self::Nds => None,
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Self::Gb => "gb",
            Self::Gbc => "gbc",
            Self::Gba => "gba",
            Self::Nds => "nds",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Names {
    hashes: Vec<([u8; KEY_LEN], u32)>,
    serials: Vec<([u8; 4], u32)>,
    titles: Vec<(String, u32)>,
    names: Vec<String>,
}

impl Names {
    pub fn parse(dat: &str) -> Self {
        let mut names = Self::default();
        let mut serials: Vec<([u8; 4], Rank, u32)> = Vec::new();
        let mut titles: Vec<(String, Rank, u32)> = Vec::new();
        for game in dat.split("\ngame (").skip(1) {
            let Some(name) = quoted(game, "name") else {
                continue;
            };
            let index = names.names.len() as u32;
            names.names.push(name.to_owned());
            for hash in game.split(" sha1 ").skip(1) {
                if let Some(key) = hash.get(..KEY_LEN * 2).and_then(hex_key) {
                    names.hashes.push((key, index));
                }
            }
            let region = quoted(game, "region").unwrap_or_default();
            let title = title_key(name);
            if !title.is_empty() {
                titles.push((title, rank(name, region, b'E'), index));
            }
            for serial in quoted(game, "serial").unwrap_or_default().split(',') {
                let Ok(serial) = <[u8; 4]>::try_from(serial.trim().as_bytes()) else {
                    continue;
                };
                serials.push((serial, rank(name, region, serial[3]), index));
            }
        }
        names.hashes.sort();
        names.hashes.dedup_by_key(|entry| entry.0);
        serials.sort();
        serials.dedup_by_key(|entry| entry.0);
        names.serials = serials
            .into_iter()
            .map(|(serial, _, index)| (serial, index))
            .collect();
        titles.sort();
        names.titles = titles
            .into_iter()
            .map(|(title, _, index)| (title, index))
            .collect();
        names
    }

    pub fn find(&self, sha1: &[u8; 20], serial: Option<[u8; 4]>) -> Option<&str> {
        let key: [u8; KEY_LEN] = sha1[..KEY_LEN].try_into().ok()?;
        let by_hash = self
            .hashes
            .binary_search_by_key(&key, |entry| entry.0)
            .ok()
            .map(|at| self.hashes[at].1);
        let by_serial = || {
            let serial = serial?;
            self.serials
                .binary_search_by_key(&serial, |entry| entry.0)
                .ok()
                .map(|at| self.serials[at].1)
        };
        let index = by_hash.or_else(by_serial)?;
        self.names.get(index as usize).map(String::as_str)
    }

    pub fn find_title(&self, file_name: &str) -> Option<&str> {
        let stem = file_name
            .rsplit_once('.')
            .filter(|(_, ext)| !ext.contains(' ') && ext.len() <= 4)
            .map_or(file_name, |(stem, _)| stem);
        let title = title_key(stem);
        if title.is_empty() {
            return None;
        }
        let start = self.titles.partition_point(|entry| entry.0 < title);
        let found = self.titles[start..]
            .iter()
            .take_while(|entry| entry.0 == title)
            .filter_map(|entry| self.names.get(entry.1 as usize))
            .map(String::as_str)
            .collect::<Vec<_>>();
        let tags = stem.find(" (").map_or("", |at| &stem[at..]);
        let wanted = regions(tags);
        let same_region = |name: &str| regions(name).iter().any(|at| wanted.contains(at));
        found
            .iter()
            .find(|name| **name == stem)
            .or_else(|| found.iter().find(|name| same_region(name)))
            .or_else(|| found.first())
            .copied()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        crate::storage::compress(&postcard::to_stdvec(self).unwrap_or_default())
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(&crate::storage::decompress(bytes)?).ok()
    }
}

fn quoted<'a>(game: &'a str, field: &str) -> Option<&'a str> {
    let start = game.find(&format!("\t{field} \""))? + field.len() + 3;
    let rest = &game[start..];
    Some(&rest[..rest.find('"')?])
}

fn title_key(name: &str) -> String {
    let title = name.find(" (").map_or(name, |at| &name[..at]);
    let mut words = String::new();
    for word in fold(title).split(|c: char| !c.is_ascii_alphanumeric()) {
        if word.is_empty() || word == "the" {
            continue;
        }
        if !words.is_empty() {
            words.push(' ');
        }
        words.push_str(word);
    }
    words
}

fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            '&' => out.push_str(" and "),
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => out.push('a'),
            'é' | 'è' | 'ê' | 'ë' => out.push('e'),
            'í' | 'ì' | 'î' | 'ï' => out.push('i'),
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => out.push('o'),
            'ú' | 'ù' | 'û' | 'ü' => out.push('u'),
            'ñ' => out.push('n'),
            'ç' => out.push('c'),
            'ß' => out.push_str("ss"),
            c => out.push(c),
        }
    }
    out
}

fn regions(name: &str) -> Vec<&str> {
    name.match_indices(" (")
        .filter_map(|(at, _)| {
            let rest = &name[at + 2..];
            Some(&rest[..rest.find(')')?])
        })
        .flat_map(|tag| tag.split(", "))
        .filter(|word| REGIONS.contains(word))
        .collect()
}

fn hex_key(hex: &str) -> Option<[u8; KEY_LEN]> {
    let mut key = [0; KEY_LEN];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(key)
}

type Rank = (bool, bool, usize);

fn rank(name: &str, region: &str, letter: u8) -> Rank {
    let unwanted = UNWANTED_TAGS.iter().any(|tag| name.contains(tag));
    let home = match letter {
        b'E' => "USA",
        b'P' => "Europe",
        b'J' => "Japan",
        b'D' => "Germany",
        b'F' => "France",
        b'I' => "Italy",
        b'S' => "Spain",
        b'K' => "Korea",
        b'U' => "Australia",
        _ => "",
    };
    let elsewhere = home.is_empty() || !region.contains(home);
    (unwanted, elsewhere, name.matches('(').count())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAT: &str = r#"clrmamepro (
	name "Nintendo - Nintendo DS"
)

game (
	name "Pokemon - Platinum Version (Europe)"
	region "Europe"
	serial "CPUE"
	rom ( name "Pokemon - Platinum Version (Europe).nds" size 134217728 crc 71ACA63A md5 5159324028BFF42BF5EFDB03EBC8B051 sha1 8D15CD443D4E83E20493C79D3F9DC408C4F51AE0 serial "CPUE" )
)
game (
	name "Pokemon - Platinum Version (USA) (Rev 1)"
	region "USA"
	serial "CPUE"
	rom ( name "Pokemon - Platinum Version (USA) (Rev 1).nds" size 134217728 crc 69D628E8 md5 AB828B0D13F09469A71460A34D0DE51B sha1 0862EC35B24DE5C7E2DCB88C9EEA0873110D755C serial "CPUE" )
)
game (
	name "Pokemon - Platinum Version (USA)"
	region "USA"
	serial "CPUE"
	rom ( name "Pokemon - Platinum Version (USA).nds" size 134217728 crc 9253921D md5 D66AD7A2A0068B5D46E0781CA4953AE9 sha1 CE81046EDA7D232513069519CB2085349896DEC7 serial "CPUE" )
)
game (
	name "Mario Kart DS (USA) (Kiosk)"
	region "USA"
	serial "AMCE"
	rom ( name "x.nds" size 1 crc 0 md5 0 sha1 1111111111111111111111111111111111111111 serial "AMCE" )
)
game (
	name "Mario Kart DS (USA, Australia)"
	region "USA"
	serial "AMCE, AMCU"
	rom ( name "y.nds" size 1 crc 0 md5 0 sha1 2222222222222222222222222222222222222222 serial "AMCE" )
)
"#;

    fn sha1(hex: &str) -> [u8; 20] {
        let mut out = [0; 20];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }

    #[test]
    fn a_dump_is_found_by_its_sha1_before_its_game_code() {
        let names = Names::parse(DAT);
        let rev = sha1("0862EC35B24DE5C7E2DCB88C9EEA0873110D755C");
        assert_eq!(
            names.find(&rev, Some(*b"CPUE")),
            Some("Pokemon - Platinum Version (USA) (Rev 1)")
        );
    }

    #[test]
    fn a_game_code_picks_its_own_region_and_the_plainest_release() {
        let names = Names::parse(DAT);
        let trimmed = [0xAB; 20];
        assert_eq!(
            names.find(&trimmed, Some(*b"CPUE")),
            Some("Pokemon - Platinum Version (USA)")
        );
        assert_eq!(
            names.find(&trimmed, Some(*b"AMCE")),
            Some("Mario Kart DS (USA, Australia)")
        );
        assert_eq!(
            names.find(&trimmed, Some(*b"AMCU")),
            Some("Mario Kart DS (USA, Australia)")
        );
    }

    #[test]
    fn an_unknown_game_has_no_name() {
        let names = Names::parse(DAT);
        assert_eq!(names.find(&[0xAB; 20], Some(*b"ZZZZ")), None);
        assert_eq!(names.find(&[0xAB; 20], None), None);
        assert!(Names::parse("").is_empty());
    }

    const GB_DAT: &str = r#"clrmamepro (
	name "Nintendo - Game Boy"
)

game (
	name "Legend of Zelda, The - Oracle of Ages (Europe) (En,Fr,De,Es,It)"
	region "Europe"
	rom ( name "e.gbc" size 1 crc 0 md5 0 sha1 9F2DFF3C46406839F839A8D535353DFC1E3E8670 )
)
game (
	name "Legend of Zelda, The - Oracle of Ages (USA, Australia)"
	region "USA"
	rom ( name "u.gbc" size 1 crc 0 md5 0 sha1 880374FB978B18AF4AA529E2E32F7FFB4D7DD2F4 )
)
game (
	name "Pocket Monsters - Midori (Japan) (SGB Enhanced)"
	region "Japan"
	rom ( name "g.gb" size 1 crc 0 md5 0 sha1 D7037C83E1AE5B39BDE3C30787637BA1D4C48CE2 )
)
game (
	name "Spot - Cool Adventure (Japan) (En)"
	region "Japan"
	rom ( name "j.gb" size 1 crc 0 md5 0 sha1 1111111111111111111111111111111111111111 )
)
game (
	name "Spot - The Cool Adventure (USA)"
	region "USA"
	rom ( name "s.gb" size 1 crc 0 md5 0 sha1 2222222222222222222222222222222222222222 )
)
game (
	name "Pokemon - Yellow Version - Special Pikachu Edition (USA, Europe) (CGB+SGB Enhanced)"
	region "USA"
	rom ( name "y.gb" size 1 crc 0 md5 0 sha1 CC7D03262EBFAF2F06772C1A480C7D9D5F4A38E1 )
)
"#;

    #[test]
    fn pokemon_yellow_is_found_by_its_sha1() {
        let names = Names::parse(GB_DAT);
        assert_eq!(
            names.find(&sha1("CC7D03262EBFAF2F06772C1A480C7D9D5F4A38E1"), None),
            Some(
                "Pokemon - Yellow Version - Special Pikachu Edition (USA, Europe) (CGB+SGB Enhanced)"
            )
        );
    }

    #[test]
    fn a_file_name_finds_its_game_whatever_the_article_and_tags() {
        let names = Names::parse(GB_DAT);
        assert_eq!(
            names.find_title("The Legend of Zelda - Oracle of Ages.gbc"),
            Some("Legend of Zelda, The - Oracle of Ages (USA, Australia)")
        );
        assert_eq!(
            names.find_title("Legend of Zelda, The - Oracle of Ages (Europe).gbc"),
            Some("Legend of Zelda, The - Oracle of Ages (Europe) (En,Fr,De,Es,It)")
        );
        assert_eq!(
            names.find_title("Pocket Monsters - Midori (Japan) (Translated En).gb"),
            Some("Pocket Monsters - Midori (Japan) (SGB Enhanced)")
        );
        assert_eq!(
            names.find_title("Spot - The Cool Adventure (USA).gb"),
            Some("Spot - The Cool Adventure (USA)")
        );
        assert_eq!(
            names.find_title("Spot - Cool Adventure (Japan).gb"),
            Some("Spot - Cool Adventure (Japan) (En)")
        );
    }

    #[test]
    fn a_file_name_that_is_not_a_title_finds_nothing() {
        let names = Names::parse(GB_DAT);
        assert_eq!(names.find_title("Pokemon Yellow.gb"), None);
        assert_eq!(names.find_title("Oracle of Ages.gbc"), None);
        assert_eq!(names.find_title("rom.gb"), None);
        assert_eq!(names.find_title(".gb"), None);
        assert_eq!(names.find_title(""), None);
    }

    #[test]
    fn colour_and_mono_lists_are_siblings() {
        assert_eq!(System::Gb.sibling(), Some(System::Gbc));
        assert_eq!(System::Gbc.sibling(), Some(System::Gb));
        assert_eq!(System::Gba.sibling(), None);
        assert_eq!(System::Nds.sibling(), None);
    }

    #[test]
    fn the_names_survive_the_store() {
        let names = Names::parse(DAT);
        assert_eq!(Names::from_bytes(&names.to_bytes()), Some(names));
        assert_eq!(Names::from_bytes(b"not a list"), None);
    }

    #[test]
    fn a_colour_cartridge_looks_in_the_colour_list() {
        let mut rom = vec![0; 0x150];
        assert_eq!(System::of(ConsoleKind::Gb, &rom), System::Gb);
        rom[0x143] = 0x80;
        assert_eq!(System::of(ConsoleKind::Gbc, &rom), System::Gbc);
        rom[0x143] = 0xC0;
        assert_eq!(System::of(ConsoleKind::Gb, &rom), System::Gbc);
        assert_eq!(System::of(ConsoleKind::Gba, &rom), System::Gba);
        assert_eq!(System::of(ConsoleKind::Nds, &rom), System::Nds);
    }
}
