use crate::{Game, Language, Version, identify};

fn header(title: &[u8], overseas: bool, revision: u8, checksum: u16) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x134..0x134 + title.len()].copy_from_slice(title);
    rom[0x14A] = u8::from(overseas);
    rom[0x14C] = revision;
    rom[0x14E..0x150].copy_from_slice(&checksum.to_be_bytes());
    rom
}

fn found(version: Version, language: Language, revision: u8) -> Option<Game> {
    Some(Game {
        version,
        language,
        revision,
    })
}

#[test]
fn gen1_english_carts_are_known_by_title() {
    assert_eq!(
        identify(&header(b"POKEMON RED", true, 0, 0x91E6)),
        found(Version::Red, Language::English, 0)
    );
    assert_eq!(
        identify(&header(b"POKEMON BLUE", true, 0, 0x9D0A)),
        found(Version::Blue, Language::English, 0)
    );
    assert_eq!(
        identify(&header(b"POKEMON YELLOW", true, 0, 0x047C)),
        found(Version::Yellow, Language::English, 0)
    );
}

#[test]
fn gen1_european_carts_are_known_by_global_checksum() {
    assert_eq!(
        identify(&header(b"POKEMON RED", true, 0, 0x5CDC)),
        found(Version::Red, Language::German, 0)
    );
    assert_eq!(
        identify(&header(b"POKEMON BLUE", true, 0, 0x14D7)),
        found(Version::Blue, Language::Spanish, 0)
    );
    assert_eq!(
        identify(&header(b"POKEMON RED", true, 0, 0x14D7)),
        found(Version::Red, Language::English, 0)
    );
}

#[test]
fn european_yellow_names_its_language_in_the_title() {
    assert_eq!(
        identify(&header(b"POKEMON YELAPSF", true, 0, 0)),
        found(Version::Yellow, Language::French, 0)
    );
    assert_eq!(
        identify(&header(b"POKEMON YELAPSD", true, 0, 0)),
        found(Version::Yellow, Language::German, 0)
    );
}

#[test]
fn gen1_japanese_carts_have_destination_japan() {
    assert_eq!(
        identify(&header(b"POKEMON GREEN", false, 1, 0)),
        found(Version::Green, Language::Japanese, 1)
    );
    assert_eq!(
        identify(&header(b"POKEMON RED", false, 0, 0x5CDC)),
        found(Version::Red, Language::Japanese, 0)
    );
}

#[test]
fn gen2_language_is_the_last_letter_of_the_game_code() {
    assert_eq!(
        identify(&header(b"POKEMON_GLDAAUD", true, 0, 0)),
        found(Version::Gold, Language::German, 0)
    );
    assert_eq!(
        identify(&header(b"POKEMON_SLVAAXK", true, 0, 0)),
        found(Version::Silver, Language::Korean, 0)
    );
    assert_eq!(
        identify(&header(b"POKEMON_SLVAAXE", true, 1, 0)),
        found(Version::Silver, Language::English, 1)
    );
    assert_eq!(
        identify(&header(b"PM_CRYSTAL\0BYTU", true, 0, 0)),
        found(Version::Crystal, Language::English, 0)
    );
    assert_eq!(
        identify(&header(b"PM_CRYSTAL\0BXTJ", false, 0, 0)),
        found(Version::Crystal, Language::Japanese, 0)
    );
    assert_eq!(
        identify(&header(b"PM_CRYSTAL\0BYTE", true, 1, 0)),
        found(Version::Crystal, Language::English, 1)
    );
}

#[test]
fn other_games_are_not_identified() {
    assert_eq!(identify(&header(b"TETRIS", false, 0, 0)), None);
    assert_eq!(identify(&header(b"POKEMON PINBALL", true, 0, 0)), None);
    assert_eq!(identify(&header(b"POKEMON_GLDAAUX", true, 0, 0)), None);
    assert_eq!(identify(&[0u8; 0x100]), None);
}
