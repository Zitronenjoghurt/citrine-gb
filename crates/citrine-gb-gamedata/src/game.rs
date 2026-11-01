#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Version {
    Red,
    Green,
    Blue,
    Yellow,
    Gold,
    Silver,
    Crystal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Japanese,
    English,
    German,
    French,
    Italian,
    Spanish,
    Korean,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Generation {
    One,
    Two,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Game {
    pub version: Version,
    pub language: Language,
    pub revision: u8,
}

impl Version {
    pub fn generation(self) -> Generation {
        match self {
            Version::Red | Version::Green | Version::Blue | Version::Yellow => Generation::One,
            Version::Gold | Version::Silver | Version::Crystal => Generation::Two,
        }
    }
}

impl Game {
    pub fn generation(self) -> Generation {
        self.version.generation()
    }

    pub fn is_japanese(self) -> bool {
        self.language == Language::Japanese
    }

    pub fn name_length(self) -> usize {
        match self.language {
            Language::Japanese => 6,
            _ => 11,
        }
    }

    pub fn species_name_length(self) -> usize {
        match self.language {
            Language::Japanese => 5,
            _ => 10,
        }
    }

    pub fn money(self, amount: u32) -> String {
        let sign = crate::text::decode(self, &[MONEY_SIGN]);
        let digits = amount.to_string();
        let mut grouped = String::new();
        for (i, digit) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                grouped.push(',');
            }
            grouped.push(digit);
        }
        if self.is_japanese() {
            format!("{grouped}{sign}")
        } else {
            format!("{sign}{grouped}")
        }
    }
}

const MONEY_SIGN: u8 = 0xF0;

const TITLE: usize = 0x134;
const CODE: usize = 0x13F;
const DESTINATION: usize = 0x14A;
const REVISION: usize = 0x14C;
pub(crate) const GLOBAL_CHECKSUM: usize = 0x14E;

const GEN1_EUROPE: [(Version, u16, Language); 8] = [
    (Version::Red, 0x5CDC, Language::German),
    (Version::Red, 0x7AFC, Language::French),
    (Version::Red, 0x89D2, Language::Italian),
    (Version::Red, 0x384A, Language::Spanish),
    (Version::Blue, 0x2EBC, Language::German),
    (Version::Blue, 0x56A4, Language::French),
    (Version::Blue, 0x5E9C, Language::Italian),
    (Version::Blue, 0x14D7, Language::Spanish),
];

pub fn identify(rom: &[u8]) -> Option<Game> {
    if rom.len() < 0x150 {
        return None;
    }
    let title = &rom[TITLE..CODE + 4];
    let revision = rom[REVISION];
    let japan = rom[DESTINATION] == 0;
    if let Some(game) = identify_gen2(title, revision) {
        return Some(game);
    }
    let version = gen1_version(title)?;
    let checksum = u16::from_be_bytes([rom[GLOBAL_CHECKSUM], rom[GLOBAL_CHECKSUM + 1]]);
    let language = if japan {
        Language::Japanese
    } else if let Some(language) = yellow_language(title) {
        language
    } else {
        GEN1_EUROPE
            .iter()
            .find(|(v, sum, _)| *v == version && *sum == checksum)
            .map_or(Language::English, |(_, _, language)| *language)
    };
    Some(Game {
        version,
        language,
        revision,
    })
}

fn identify_gen2(title: &[u8], revision: u8) -> Option<Game> {
    let version = if title.starts_with(b"POKEMON_GLDAAU") {
        Version::Gold
    } else if title.starts_with(b"POKEMON_SLVAAX") {
        Version::Silver
    } else if title.starts_with(b"PM_CRYSTAL\0BYT") || title.starts_with(b"PM_CRYSTAL\0BXT") {
        Version::Crystal
    } else {
        return None;
    };
    let language = match title[14] {
        b'J' => Language::Japanese,
        b'E' | b'U' => Language::English,
        b'D' => Language::German,
        b'F' => Language::French,
        b'I' => Language::Italian,
        b'S' => Language::Spanish,
        b'K' => Language::Korean,
        _ => return None,
    };
    Some(Game {
        version,
        language,
        revision,
    })
}

fn gen1_version(title: &[u8]) -> Option<Version> {
    let name = title.split(|&b| b == 0).next().unwrap_or(title);
    if name.starts_with(b"POKEMON YEL") {
        return Some(Version::Yellow);
    }
    match name {
        b"POKEMON RED" => Some(Version::Red),
        b"POKEMON GREEN" => Some(Version::Green),
        b"POKEMON BLUE" => Some(Version::Blue),
        _ => None,
    }
}

fn yellow_language(title: &[u8]) -> Option<Language> {
    match &title[..15] {
        b"POKEMON YELAPSD" => Some(Language::German),
        b"POKEMON YELAPSF" => Some(Language::French),
        b"POKEMON YELAPSI" => Some(Language::Italian),
        b"POKEMON YELAPSS" => Some(Language::Spanish),
        _ => None,
    }
}
