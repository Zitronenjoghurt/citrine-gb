use crate::game::Language;

pub(crate) const TERMINATOR: u8 = 0xFF;
const UNUSED: char = '\0';

const LATIN_LOW: [&str; 3] = [
    " ÀÁÂÇÈÉÊËÌ\0ÎÏÒÓÔ",
    "ŒÙÚÛÑßàá\0çèéêëì\0",
    "îïòóôœùúûñºª\0&+\0",
];
const LATIN_HIGH: [&str; 6] = [
    "\u{0}0123456789!?.-·",
    "…“”‘’♂♀¥,×/ABCDE",
    "FGHIJKLMNOPQRSTU",
    "VWXYZabcdefghijk",
    "lmnopqrstuvwxyz▶",
    ":ÄÖÜäöü",
];
const KANA: [&str; 11] = [
    "　あいうえおかきくけこさしすせそ",
    "たちつてとなにぬねのはひふへほま",
    "みむめもやゆよらりるれろわをんぁ",
    "ぃぅぇぉゃゅょがぎぐげござじずぜ",
    "ぞだぢづでどばびぶべぼぱぴぷぺぽ",
    "っアイウエオカキクケコサシスセソ",
    "タチツテトナニヌネノハヒフヘホマ",
    "ミムメモヤユヨラリルレロワヲンァ",
    "ィゥェォャュョガギグゲゴザジズゼ",
    "ゾダヂヅデドバビブベボパピプペポ",
    "ッ０１２３４５６７８９！？。ー・",
];
const JAPANESE_HIGH: [&str; 5] = [
    "…『』「」♂♀円．×／ＡＢＣＤＥ",
    "ＦＧＨＩＪＫＬＭＮＯＰＱＲＳＴＵ",
    "ＶＷＸＹＺａｂｃｄｅｆｇｈｉｊｋ",
    "ｌｍｎｏｐｑｒｓｔｕｖｗｘｙｚ▶",
    "：ÄÖÜäöü",
];

enum Glyph {
    Char(char),
    Str(&'static str),
}

fn cell(rows: &[&str], first: u8, b: u8) -> Option<Glyph> {
    let index = usize::from(b.checked_sub(first)?);
    let c = rows.get(index / 16)?.chars().nth(index % 16)?;
    (c != UNUSED).then_some(Glyph::Char(c))
}

fn latin(b: u8) -> Option<Glyph> {
    Some(match b {
        0x2C => Glyph::Str("er"),
        0x00..=0x2F => cell(&LATIN_LOW, 0x00, b)?,
        0x34 => Glyph::Str("Lv"),
        0x35 => Glyph::Char('='),
        0x36 => Glyph::Char(';'),
        0x51 => Glyph::Char('¿'),
        0x52 => Glyph::Char('¡'),
        0x53 => Glyph::Str("PK"),
        0x54 => Glyph::Str("MN"),
        0x55 => Glyph::Str("PO"),
        0x56 => Glyph::Str("Ké"),
        0x57 => Glyph::Str("BL"),
        0x58 => Glyph::Str("OC"),
        0x59 => Glyph::Str("K"),
        0x5A => Glyph::Char('Í'),
        0x5B => Glyph::Char('%'),
        0x5C => Glyph::Char('('),
        0x5D => Glyph::Char(')'),
        0x68 => Glyph::Char('â'),
        0x6F => Glyph::Char('í'),
        0x79 => Glyph::Char('↑'),
        0x7A => Glyph::Char('↓'),
        0x7B => Glyph::Char('←'),
        0x7C => Glyph::Char('→'),
        0x84 => Glyph::Str("e"),
        0x85 => Glyph::Char('<'),
        0x86 => Glyph::Char('>'),
        0xA0 => Glyph::Str("re"),
        0xA1..=0xF6 => cell(&LATIN_HIGH, 0xA0, b)?,
        _ => return None,
    })
}

fn japanese(b: u8) -> Option<Glyph> {
    match b {
        0x00..=0xAF => cell(&KANA, 0x00, b),
        0xB0..=0xF6 => cell(&JAPANESE_HIGH, 0xB0, b),
        _ => None,
    }
}

fn glyph(language: Language, b: u8) -> Option<Glyph> {
    match language {
        Language::Japanese => japanese(b),
        _ => latin(b),
    }
}

pub fn decode(bytes: &[u8], language: Language) -> String {
    let end = bytes
        .iter()
        .position(|&b| b == TERMINATOR)
        .unwrap_or(bytes.len());
    let mut text = String::new();
    for &b in &bytes[..end] {
        match glyph(language, b) {
            Some(Glyph::Char(c)) => text.push(c),
            Some(Glyph::Str(s)) => text.push_str(s),
            None => text.push('\u{FFFD}'),
        }
    }
    text
}

pub fn encode(text: &str, language: Language) -> Option<Vec<u8>> {
    text.chars()
        .map(|c| {
            (0..TERMINATOR).find(|&b| matches!(glyph(language, b), Some(Glyph::Char(g)) if g == c))
        })
        .collect()
}
