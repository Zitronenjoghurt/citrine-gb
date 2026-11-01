use crate::game::{Game, Generation, Language};

pub(crate) const TERMINATOR: u8 = 0x50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Charset {
    English1,
    English2,
    FrenchGerman,
    ItalianSpanish,
    Japanese,
    Korean,
}

enum Glyph {
    Char(char),
    Str(&'static str),
    Kana(char, char),
}

const KATAKANA: &str = "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフホマミムメモヤユヨラルレロワヲンッャュョィ";
const HIRAGANA: &str = "あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわをんっゃゅょ";
const KATAKANA_VOICED: &str = "ガギグゲゴザジズゼゾダヂヅデド";
const HIRAGANA_VOICED: &str = "がぎぐげござじずぜぞだぢづでど";
const FULL_DIGITS: &str = "０１２３４５６７８９";
const FRENCH_GERMAN_ACCENTS: &str = "àèéùßçÄÖÜäöüëïâôûêî";
const FRENCH_GERMAN_CONTRACTIONS: [&str; 12] = [
    "c'", "d'", "j'", "l'", "m'", "n'", "p'", "s'", "'s", "t'", "u'", "y'",
];
const ITALIAN_SPANISH_ACCENTS: &str = "àèéùÀÁÄÖÜäöüÈÉÌÍÑÒÓÙÚáìíñòóúº";
const CONTRACTIONS: [&str; 7] = ["'d", "'l", "'m", "'r", "'s", "'t", "'v"];

fn charset(game: Game) -> Charset {
    match (game.language, game.generation()) {
        (Language::Japanese, _) => Charset::Japanese,
        (Language::Korean, _) => Charset::Korean,
        (Language::French | Language::German, _) => Charset::FrenchGerman,
        (Language::Italian | Language::Spanish, _) => Charset::ItalianSpanish,
        (Language::English, Generation::One) => Charset::English1,
        (Language::English, Generation::Two) => Charset::English2,
    }
}

fn nth(table: &str, index: u8) -> Option<char> {
    table.chars().nth(usize::from(index))
}

fn latin(game: Game, set: Charset, b: u8) -> Option<Glyph> {
    let gen1 = game.generation() == Generation::One;
    let glyph = match b {
        0x54 => Glyph::Str("POKé"),
        0x7F => Glyph::Char(' '),
        0x80..=0x99 => Glyph::Char((b'A' + (b - 0x80)) as char),
        0x9A..=0x9F => Glyph::Char(nth("():;[]", b - 0x9A)?),
        0xA0..=0xB9 => Glyph::Char((b'a' + (b - 0xA0)) as char),
        0xE0 => Glyph::Char('\''),
        0xE1 => Glyph::Str("PK"),
        0xE2 => Glyph::Str("MN"),
        0xE3 => Glyph::Char('-'),
        0xE6 => Glyph::Char('?'),
        0xE7 => Glyph::Char('!'),
        0xE8 | 0xF2 => Glyph::Char('.'),
        0xEF => Glyph::Char('♂'),
        0xF0 => Glyph::Char('¥'),
        0xF1 => Glyph::Char('×'),
        0xF3 => Glyph::Char('/'),
        0xF4 => Glyph::Char(','),
        0xF5 => Glyph::Char('♀'),
        0xF6..=0xFF => Glyph::Char((b'0' + (b - 0xF6)) as char),
        _ => {
            return match set {
                Charset::English1 => match b {
                    0xBA => Some(Glyph::Char('é')),
                    0xBB..=0xBF => Some(Glyph::Str(
                        ["'d", "'l", "'s", "'t", "'v"][usize::from(b - 0xBB)],
                    )),
                    0xE4 => Some(Glyph::Str("'r")),
                    0xE5 => Some(Glyph::Str("'m")),
                    _ => None,
                },
                Charset::English2 => match b {
                    0xC0..=0xC5 => Some(Glyph::Char(nth("ÄÖÜäöü", b - 0xC0)?)),
                    0xD0..=0xD6 => Some(Glyph::Str(CONTRACTIONS[usize::from(b - 0xD0)])),
                    0xE9 => Some(Glyph::Char('&')),
                    0xEA => Some(Glyph::Char('é')),
                    _ => None,
                },
                Charset::FrenchGerman => match b {
                    0xBA..=0xCC => Some(Glyph::Char(nth(FRENCH_GERMAN_ACCENTS, b - 0xBA)?)),
                    0xD4..=0xDF => Some(Glyph::Str(
                        FRENCH_GERMAN_CONTRACTIONS[usize::from(b - 0xD4)],
                    )),
                    0xE4 if gen1 => Some(Glyph::Char('+')),
                    0xE9 if !gen1 => Some(Glyph::Char('&')),
                    _ => None,
                },
                Charset::ItalianSpanish => match b {
                    0xBA..=0xD6 => Some(Glyph::Char(nth(ITALIAN_SPANISH_ACCENTS, b - 0xBA)?)),
                    0xD7 if gen1 => Some(Glyph::Char('&')),
                    0xD8..=0xDE => Some(Glyph::Str(CONTRACTIONS[usize::from(b - 0xD8)])),
                    0xE4 => Some(Glyph::Char('¿')),
                    0xE5 => Some(Glyph::Char('¡')),
                    0xE9 if !gen1 => Some(Glyph::Char('&')),
                    _ => None,
                },
                _ => None,
            };
        }
    };
    Some(glyph)
}

fn japanese(b: u8) -> Option<Glyph> {
    let glyph = match b {
        0x05..=0x13 => Glyph::Char(nth(KATAKANA_VOICED, b - 0x05)?),
        0x19..=0x1C => Glyph::Char(nth("バビブボ", b - 0x19)?),
        0x26..=0x34 => Glyph::Char(nth(HIRAGANA_VOICED, b - 0x26)?),
        0x3A..=0x3C | 0x3E => Glyph::Char(nth("ばびぶべぼ", b - 0x3A)?),
        0x3D => Glyph::Kana('ベ', 'べ'),
        0x40..=0x43 => Glyph::Char(nth("パピプポ", b - 0x40)?),
        0x44..=0x46 | 0x48 => Glyph::Char(nth("ぱぴぷぺぽ", b - 0x44)?),
        0x47 => Glyph::Kana('ペ', 'ぺ'),
        0x54 => Glyph::Str("ポケモン"),
        0x6E => Glyph::Char('ぃ'),
        0x6F => Glyph::Char('ぅ'),
        0x70..=0x78 => Glyph::Char(nth("「」『』・⋯ぁぇぉ", b - 0x70)?),
        0x7F => Glyph::Char('\u{3000}'),
        0x80..=0xB0 => Glyph::Char(nth(KATAKANA, b - 0x80)?),
        0xCD => Glyph::Kana('ヘ', 'へ'),
        0xD8 => Glyph::Kana('リ', 'り'),
        0xB1..=0xE2 => Glyph::Char(nth(HIRAGANA, b - 0xB1)?),
        0xE3..=0xEB => Glyph::Char(nth("ー゜゛？！。ァゥェ", b - 0xE3)?),
        0xEF => Glyph::Char('♂'),
        0xF0..=0xF5 => Glyph::Char(nth("円×．／ォ♀", b - 0xF0)?),
        0xF6..=0xFF => Glyph::Char(nth(FULL_DIGITS, b - 0xF6)?),
        _ => return None,
    };
    Some(glyph)
}

fn korean_single(b: u8) -> Option<Glyph> {
    let glyph = match b {
        0x54 => Glyph::Str("포켓몬"),
        0x7F => Glyph::Char(' '),
        0x80..=0x99 => Glyph::Char((b'A' + (b - 0x80)) as char),
        0x9A..=0x9F => Glyph::Char(nth("():;[]", b - 0x9A)?),
        0xA0..=0xB9 => Glyph::Char((b'a' + (b - 0xA0)) as char),
        0xC0..=0xC5 => Glyph::Char(nth("ÄÖÜäöü", b - 0xC0)?),
        0xD0..=0xD6 => Glyph::Str(CONTRACTIONS[usize::from(b - 0xD0)]),
        0xE0 => Glyph::Char('\''),
        0xE1 => Glyph::Str("PK"),
        0xE2 => Glyph::Str("MN"),
        0xE3 => Glyph::Char('-'),
        0xE6..=0xEA => Glyph::Char(nth("?!.&é", b - 0xE6)?),
        0xEF => Glyph::Char('♂'),
        0xF0..=0xF5 => Glyph::Char(nth("₩×./,♀", b - 0xF0)?),
        0xF6..=0xFF => Glyph::Char((b'0' + (b - 0xF6)) as char),
        _ => return None,
    };
    Some(glyph)
}

fn glyphs(game: Game, bytes: &[u8]) -> Vec<Option<Glyph>> {
    let set = charset(game);
    let bytes = bytes
        .iter()
        .position(|&b| b == TERMINATOR)
        .map_or(bytes, |end| &bytes[..end]);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        out.push(match set {
            Charset::Japanese => japanese(b),
            Charset::Korean if (0x01..=0x0B).contains(&b) => {
                let index = bytes.get(i).copied();
                i += 1;
                index.and_then(|index| crate::hangul::glyph(b, index).map(Glyph::Char))
            }
            Charset::Korean => korean_single(b),
            _ => latin(game, set, b),
        });
    }
    out
}

fn render(glyphs: &[Option<Glyph>]) -> String {
    let katakana = glyphs
        .iter()
        .find_map(|glyph| match glyph {
            Some(Glyph::Char(c)) if ('\u{30A1}'..='\u{30FA}').contains(c) => Some(true),
            Some(Glyph::Char(c)) if ('\u{3041}'..='\u{3096}').contains(c) => Some(false),
            _ => None,
        })
        .unwrap_or(false);
    let mut text = String::new();
    for glyph in glyphs {
        match glyph {
            Some(Glyph::Char(c)) => text.push(*c),
            Some(Glyph::Str(s)) => text.push_str(s),
            Some(Glyph::Kana(kata, hira)) => text.push(if katakana { *kata } else { *hira }),
            None => text.push('\u{FFFD}'),
        }
    }
    text
}

pub(crate) fn decode(game: Game, bytes: &[u8]) -> String {
    render(&glyphs(game, bytes))
}

pub(crate) fn decode_strict(game: Game, bytes: &[u8]) -> Option<String> {
    let glyphs = glyphs(game, bytes);
    (!glyphs.is_empty() && glyphs.iter().all(Option::is_some)).then(|| render(&glyphs))
}

pub(crate) fn is_name(game: Game, bytes: &[u8]) -> bool {
    bytes.contains(&TERMINATOR) && decode_strict(game, bytes).is_some()
}

fn glyph_of(game: Game, b: u8) -> Option<Glyph> {
    match charset(game) {
        Charset::Japanese => japanese(b),
        Charset::Korean => korean_single(b),
        set => latin(game, set, b),
    }
}

pub(crate) fn encode(game: Game, text: &str) -> Option<Vec<u8>> {
    text.chars()
        .map(|c| {
            (0..=u8::MAX).find(|&b| {
                b != TERMINATOR
                    && match glyph_of(game, b) {
                        Some(Glyph::Char(g)) => g == c,
                        Some(Glyph::Kana(kata, hira)) => kata == c || hira == c,
                        _ => false,
                    }
            })
        })
        .collect()
}
