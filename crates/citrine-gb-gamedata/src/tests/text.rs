use super::*;
use crate::text::{decode, decode_strict, is_name};

#[test]
fn gen1_english_has_contractions_and_the_poke_glyph() {
    let game = game(Version::Red, Language::English);
    assert_eq!(decode(game, &[0x8B, 0xBB, 0x50]), "L'd");
    assert_eq!(decode(game, &[0x8B, 0xBF, 0x50]), "L'v");
    assert_eq!(decode(game, &[0x54, 0x50]), "POKé");
    assert_eq!(decode(game, &[0x91, 0x84, 0x83, 0x50, 0x80]), "RED");
}

#[test]
fn gen2_german_has_umlauts() {
    let game = game(Version::Gold, Language::German);
    assert_eq!(decode(game, &[0xC0, 0xA1, 0x50]), "Äb");
    assert_eq!(decode(game, &[0xC1, 0xC2]), "ÖÜ");
}

#[test]
fn french_has_accents_and_elisions() {
    let game = game(Version::Gold, Language::French);
    assert_eq!(decode(game, &[0xBA, 0x50]), "à");
    assert_eq!(decode(game, &[0xD4, 0x80, 0x50]), "c'A");
    assert_eq!(decode(game, &[0xE9, 0x50]), "&");
}

#[test]
fn italian_and_spanish_have_inverted_marks() {
    let game = game(Version::Crystal, Language::Italian);
    assert_eq!(decode(game, &[0xE4, 0xE5, 0x50]), "¿¡");
    let game = super::game(Version::Red, Language::Spanish);
    assert_eq!(decode(game, &[0xD7, 0x50]), "&");
}

#[test]
fn japanese_shared_kana_follow_the_word_around_them() {
    let game = game(Version::Red, Language::Japanese);
    assert_eq!(decode(game, &[0x80, 0xCD, 0x50]), "アヘ");
    assert_eq!(decode(game, &[0xB1, 0xCD, 0x50]), "あへ");
    assert_eq!(decode(game, &[0xD8, 0x96, 0x50]), "リヌ");
    assert_eq!(decode(game, &[0x3D, 0x47, 0xB1]), "べぺあ");
    assert_eq!(decode(game, &[0x3D, 0x47, 0x80]), "ベペア");
    assert_eq!(decode(game, &[0x54, 0xF7, 0x50]), "ポケモン１");
}

#[test]
fn korean_reads_two_byte_syllables_between_single_bytes() {
    let game = game(Version::Gold, Language::Korean);
    assert_eq!(decode(game, &[0x01, 0x01, 0x01, 0x02, 0x50]), "가각");
    assert_eq!(decode(game, &[0x80, 0x01, 0x01, 0xF7, 0x50]), "A가1");
}

#[test]
fn strict_decoding_refuses_unknown_bytes() {
    let game = game(Version::Red, Language::English);
    assert_eq!(decode_strict(game, &[0x80, 0x50]).as_deref(), Some("A"));
    assert_eq!(decode_strict(game, &[0x80, 0x01, 0x50]), None);
    assert_eq!(decode_strict(game, &[0x50]), None);
    assert_eq!(decode(game, &[0x80, 0x01, 0x50]), "A\u{FFFD}");
    assert!(is_name(game, &[0x80, 0x50, 0x00]));
    assert!(!is_name(game, &[0x80, 0x80]));
}

#[test]
fn names_encode_back_to_the_bytes_they_decode_from() {
    let red = game(Version::Red, Language::English);
    let name = crate::encode_name(red, "PCNYa", 11).unwrap();
    assert_eq!(&name[..6], &[0x8F, 0x82, 0x8D, 0x98, 0xA0, 0x50]);
    assert!(name[6..].iter().all(|&b| b == 0x50));
    assert_eq!(crate::text::decode(red, &name), "PCNYa");

    let green = game(Version::Green, Language::Japanese);
    let name = crate::encode_name(green, "コロコロ", 6).unwrap();
    assert_eq!(name.len(), 6);
    assert_eq!(name[4..], [0x50, 0x50]);
    assert_eq!(crate::text::decode(green, &name), "コロコロ");

    let german = game(Version::Silver, Language::German);
    assert_eq!(
        crate::text::decode(german, &crate::encode_name(german, "MÜNCHEN", 11).unwrap()),
        "MÜNCHEN"
    );
}

#[test]
fn names_that_dont_fit_or_have_no_glyph_are_refused() {
    let red = game(Version::Red, Language::English);
    assert!(crate::encode_name(red, "", 11).is_none());
    assert!(crate::encode_name(red, "ABCDEFGHIJK", 11).is_none());
    assert!(crate::encode_name(red, "マクハリ", 11).is_none());
}

#[test]
fn money_carries_the_game_currency_sign() {
    assert_eq!(
        game(Version::Silver, Language::English).money(12345),
        "¥12,345"
    );
    assert_eq!(
        game(Version::Red, Language::German).money(999_999),
        "¥999,999"
    );
    assert_eq!(
        game(Version::Gold, Language::Japanese).money(3000),
        "3,000円"
    );
    assert_eq!(game(Version::Gold, Language::Korean).money(42), "₩42");
    assert_eq!(game(Version::Crystal, Language::English).money(0), "¥0");
}
