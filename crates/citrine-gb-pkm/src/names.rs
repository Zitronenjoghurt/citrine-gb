use std::collections::HashMap;
use std::sync::OnceLock;

use crate::pkm::Language;

const SPECIES_NAMES: &str = include_str!("../data/species_names.tsv");

fn table() -> &'static HashMap<(u8, u16), &'static str> {
    static TABLE: OnceLock<HashMap<(u8, u16), &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        SPECIES_NAMES
            .lines()
            .skip(1)
            .filter_map(|line| {
                let mut cols = line.split('\t');
                let language = language_named(cols.next()?)?;
                let species = cols.next()?.parse().ok()?;
                Some(((language.to_byte(), species), cols.next()?))
            })
            .collect()
    })
}

fn language_named(name: &str) -> Option<Language> {
    [
        Language::Japanese,
        Language::English,
        Language::French,
        Language::Italian,
        Language::German,
        Language::Spanish,
        Language::Korean,
    ]
    .into_iter()
    .find(|l| format!("{l:?}") == name)
}

pub fn languages() -> Vec<Language> {
    let mut found: Vec<Language> = table()
        .keys()
        .map(|&(l, _)| Language::from_byte(l))
        .collect();
    found.sort_by_key(|l| l.to_byte());
    found.dedup();
    found
}

pub fn species(language: Language, species: u16) -> Option<String> {
    let name = table().get(&(language.to_byte(), species))?;
    Some(match language {
        Language::Japanese | Language::Korean => (*name).to_owned(),
        _ => name.to_uppercase(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_names_are_uppercase_as_gen_4_stores_them() {
        assert_eq!(species(Language::English, 387).as_deref(), Some("TURTWIG"));
        assert_eq!(species(Language::English, 0), None);
        assert!(languages().contains(&Language::English));
    }

    #[test]
    fn every_gen_4_language_names_all_493_species() {
        assert_eq!(
            languages(),
            [
                Language::Japanese,
                Language::English,
                Language::French,
                Language::Italian,
                Language::German,
                Language::Spanish,
                Language::Korean,
            ]
        );
        for language in languages() {
            for s in 1..=493 {
                assert!(species(language, s).is_some(), "{language:?} {s}");
            }
            assert_eq!(species(language, 494), None);
        }
        assert_eq!(species(Language::French, 4).as_deref(), Some("SALAMECHE"));
        assert_eq!(species(Language::Korean, 474).as_deref(), Some("폴리곤Z"));
    }

    fn names_match_rom(var: &str) {
        let path = std::env::var(var).unwrap_or_else(|_| panic!("{var} names no ROM"));
        let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let tables = crate::tables::extract(&rom).unwrap();
        for s in 1..=493u16 {
            assert_eq!(
                species(tables.language, s).as_deref(),
                Some(tables.species_names[usize::from(s)].as_str()),
                "{:?} {s}",
                tables.language
            );
        }
    }

    #[test]
    #[ignore = "needs Platinum named in CITRINE_PT"]
    fn english_names_match_platinum() {
        names_match_rom("CITRINE_PT");
    }

    #[test]
    #[ignore = "needs SoulSilver named in CITRINE_SS"]
    fn english_names_match_soulsilver() {
        names_match_rom("CITRINE_SS");
    }

    #[test]
    #[ignore = "needs German HeartGold named in CITRINE_HG"]
    fn german_names_match_heartgold() {
        names_match_rom("CITRINE_HG");
    }
}
