use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Developer,
    Publisher,
    Genre,
    Year,
    Month,
}

impl Field {
    pub const ALL: [Self; 5] = [
        Self::Developer,
        Self::Publisher,
        Self::Genre,
        Self::Year,
        Self::Month,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Developer => "developer",
            Self::Publisher => "publisher",
            Self::Genre => "genre",
            Self::Year => "releaseyear",
            Self::Month => "releasemonth",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Facts {
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u16>,
    pub month: Option<u8>,
    pub day: Option<u8>,
}

impl Facts {
    fn set(&mut self, field: Field, value: &str) {
        match field {
            Field::Developer => self.developer = Some(value.to_owned()),
            Field::Publisher => self.publisher = Some(value.to_owned()),
            Field::Genre => self.genre = Some(value.to_owned()),
            Field::Year => self.year = value.parse().ok(),
            Field::Month => {
                self.month = value.parse().ok().filter(|month| (1..=12).contains(month));
            }
        }
    }

    pub fn fill(&mut self, other: &Facts) {
        if self.developer.is_none() {
            self.developer.clone_from(&other.developer);
        }
        if self.publisher.is_none() {
            self.publisher.clone_from(&other.publisher);
        }
        if self.genre.is_none() {
            self.genre.clone_from(&other.genre);
        }
        if self.year.is_none() {
            self.year = other.year;
            self.month = other.month;
            self.day = other.day;
        }
    }

    pub fn is_whole(&self) -> bool {
        self.developer.is_some()
            && self.publisher.is_some()
            && self.genre.is_some()
            && self.year.is_some()
    }
}

const WIKIDATA_GENRES: [(&str, &str); 44] = [
    ("role-playing video game", "Role-playing (RPG)"),
    ("action role-playing game", "Role-playing (RPG)"),
    ("tactical role-playing game", "Role-playing (RPG)"),
    ("japanese role-playing game", "Role-playing (RPG)"),
    ("platform game", "Platform"),
    ("action game", "Action"),
    ("action-adventure game", "Adventure"),
    ("adventure game", "Adventure"),
    ("graphic adventure game", "Adventure"),
    ("point-and-click adventure game", "Adventure"),
    ("sports video game", "Sports"),
    ("sports game", "Sports"),
    ("racing video game", "Racing"),
    ("racing game", "Racing"),
    ("kart racing game", "Racing"),
    ("puzzle video game", "Puzzle"),
    ("puzzle game", "Puzzle"),
    ("strategy video game", "Strategy"),
    ("strategy game", "Strategy"),
    ("turn-based strategy video game", "Strategy"),
    ("turn-based strategy", "Strategy"),
    ("real-time strategy", "Strategy"),
    ("simulation video game", "Simulation"),
    ("simulation game", "Simulation"),
    ("life simulation game", "Simulation"),
    ("board game", "Board"),
    ("digital board game", "Board"),
    ("shooter game", "Shooter"),
    ("shooter video game", "Shooter"),
    ("first-person shooter", "Shooter"),
    ("third-person shooter", "Shooter"),
    ("shoot 'em up", "Shoot'em Up"),
    ("beat 'em up", "Beat'em Up"),
    ("fighting game", "Fighting"),
    ("card game", "Card"),
    ("collectible card game", "Card"),
    ("digital collectible card game", "Card"),
    ("pinball video game", "Pinball"),
    ("educational video game", "Educational"),
    ("educational game", "Educational"),
    ("quiz game", "Quiz"),
    ("music video game", "Music / Dancing"),
    ("rhythm game", "Music / Dancing"),
    ("casual game", "Casual Game"),
];

pub fn genre_name(label: &str) -> String {
    let label = label.trim();
    let lower = label.to_lowercase();
    WIKIDATA_GENRES
        .iter()
        .find(|(wikidata, _)| *wikidata == lower)
        .map_or_else(|| upper_first(label), |(_, name)| (*name).to_owned())
}

fn upper_first(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Sheet {
    games: HashMap<u32, Facts>,
}

impl Sheet {
    pub fn read(&mut self, field: Field, dat: &str) {
        #[cfg(test)]
        count_parse();
        let mut value: Option<String> = None;
        for line in dat.lines() {
            let line = line.trim();
            if line.starts_with("game (") || line == "game(" {
                value = None;
            } else if let Some(rest) = line
                .strip_prefix(field.key())
                .and_then(|rest| rest.strip_prefix([' ', '\t']))
            {
                value = unquote(rest);
            } else if let (Some(crc), Some(value)) = (crc_of(line), value.as_deref()) {
                self.games.entry(crc).or_default().set(field, value);
            }
        }
    }

    pub fn get(&self, crc: u32) -> Option<&Facts> {
        self.games.get(&crc)
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }
}

#[cfg(test)]
thread_local! {
    static PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn count_parse() {
    PARSES.with(|parses| parses.set(parses.get() + 1));
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn parses_here() -> usize {
    PARSES.with(std::cell::Cell::get)
}

fn unquote(text: &str) -> Option<String> {
    let text = text.trim();
    let text = text.strip_prefix('"').unwrap_or(text);
    let text = text.strip_suffix('"').unwrap_or(text).trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn crc_of(line: &str) -> Option<u32> {
    let rest = line.strip_prefix("rom")?.trim_start().strip_prefix('(')?;
    let mut words = rest.split_whitespace();
    words.find(|word| *word == "crc")?;
    u32::from_str_radix(words.next()?, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVELOPER: &str = "clrmamepro (\n\tname \"Nintendo - Game Boy Advance\"\n\tdescription \"Nintendo - Game Boy Advance\"\n)\n\ngame (\n\tcomment \"Aladdin (Japan)\"\n\tdeveloper \"Capcom\"\n\trom ( crc 587F1AEF )\n)\n\ngame (\n\tcomment \"Mystery (USA)\"\n\tdeveloper \"\"Natsume\", Vicarious Visions\"\n\trom ( crc 000206C0 )\n)\n";
    const YEAR: &str = "game (\n\tcomment \"Aladdin (Japan)\"\n\treleaseyear \"2002\"\n\trom ( crc 587F1AEF )\n)\n";
    const MONTH: &str = "game (\n\tcomment \"Aladdin (Japan)\"\n\treleasemonth 4\n\trom ( crc 587F1AEF )\n)\ngame (\n\tcomment \"Odd (Japan)\"\n\treleasemonth \"13\"\n\trom ( crc 000206C0 )\n)\n";
    const GENRE_NO_ROM: &str = "game (\n\tcomment \"A note\"\n\tgenre \"Action\"\n)\ngame (\n\tcomment \"Aladdin (Japan)\"\n\trom ( crc 587F1AEF )\n)\n";

    #[test]
    fn a_metadat_file_fills_its_field_by_crc() {
        let mut sheet = Sheet::default();
        sheet.read(Field::Developer, DEVELOPER);
        sheet.read(Field::Year, YEAR);
        sheet.read(Field::Month, MONTH);
        let aladdin = sheet.get(0x587F_1AEF).unwrap();
        assert_eq!(aladdin.developer.as_deref(), Some("Capcom"));
        assert_eq!(aladdin.year, Some(2002));
        assert_eq!(aladdin.month, Some(4));
        let mystery = sheet.get(0x0002_06C0).unwrap();
        assert_eq!(
            mystery.developer.as_deref(),
            Some("\"Natsume\", Vicarious Visions")
        );
        assert_eq!(mystery.month, None);
    }

    #[test]
    fn a_value_never_leaks_into_the_next_game() {
        let mut sheet = Sheet::default();
        sheet.read(Field::Genre, GENRE_NO_ROM);
        assert!(sheet.is_empty());
    }

    #[test]
    fn fill_keeps_what_is_known() {
        let mut facts = Facts {
            developer: Some("Konami".to_owned()),
            ..Facts::default()
        };
        facts.fill(&Facts {
            developer: Some("KCET".to_owned()),
            publisher: Some("Konami".to_owned()),
            year: Some(2003),
            month: Some(5),
            ..Facts::default()
        });
        assert_eq!(facts.developer.as_deref(), Some("Konami"));
        assert_eq!(facts.publisher.as_deref(), Some("Konami"));
        assert_eq!((facts.year, facts.month), (Some(2003), Some(5)));
        assert!(!facts.is_whole());
    }

    #[test]
    fn a_genre_shows_one_name_from_either_source() {
        for (libretro, wikidata) in [
            ("Role-playing (RPG)", "role-playing video game"),
            ("Role-playing (RPG)", "action role-playing game"),
            ("Platform", "platform game"),
            ("Shoot'em Up", "shoot 'em up"),
            ("Music / Dancing", "rhythm game"),
        ] {
            assert_eq!(genre_name(libretro), libretro);
            assert_eq!(genre_name(wikidata), libretro, "{wikidata}");
        }
        assert_eq!(genre_name("survival horror"), "Survival horror");
    }

    #[test]
    fn a_sheet_survives_the_cache() {
        let mut sheet = Sheet::default();
        sheet.read(Field::Developer, DEVELOPER);
        let back: Sheet = serde_json::from_slice(&serde_json::to_vec(&sheet).unwrap()).unwrap();
        assert_eq!(back, sheet);
    }
}
