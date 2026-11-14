mod gen4;

use super::{Gen, Language};

const END: u16 = 0xFFFF;
const GEN5_MALE: u16 = 0x246D;
const GEN5_FEMALE: u16 = 0x246E;

pub fn decode(generation: Gen, units: &[u16]) -> String {
    units
        .iter()
        .take_while(|&&u| u != END)
        .map(|&u| match generation {
            Gen::Four => gen4::char_of(u).unwrap_or(char::REPLACEMENT_CHARACTER),
            Gen::Five => match u {
                GEN5_MALE => '♂',
                GEN5_FEMALE => '♀',
                u => char::from_u32(u32::from(u)).unwrap_or(char::REPLACEMENT_CHARACTER),
            },
        })
        .collect()
}

pub fn encode<const N: usize>(
    generation: Gen,
    language: Language,
    text: &str,
) -> Result<[u16; N], TextError> {
    let mut units = [0u16; N];
    let mut len = 0;
    for c in text.chars() {
        if len + 1 >= N {
            return Err(TextError::TooLong(N - 1));
        }
        units[len] = match generation {
            Gen::Four => {
                gen4::unit_of(c, language == Language::Japanese).ok_or(TextError::Unsupported(c))?
            }
            Gen::Five => match c {
                '♂' if language != Language::Japanese => GEN5_MALE,
                '♀' if language != Language::Japanese => GEN5_FEMALE,
                c => u16::try_from(u32::from(c))
                    .ok()
                    .filter(|&u| u != END)
                    .ok_or(TextError::Unsupported(c))?,
            },
        };
        len += 1;
    }
    units[len] = END;
    Ok(units)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    #[error("at most {0} characters fit")]
    TooLong(usize),
    #[error("the game can't show {0:?}")]
    Unsupported(char),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen4_names_use_the_games_own_table() {
        let units: [u16; 8] = encode(Gen::Four, Language::English, "Lucas 7").unwrap();
        assert_eq!(
            units,
            [0x136, 0x159, 0x147, 0x145, 0x157, 0x1DE, 0x128, END]
        );
        assert_eq!(decode(Gen::Four, &units), "Lucas 7");
    }

    #[test]
    fn gen5_names_are_utf16() {
        let units: [u16; 8] = encode(Gen::Five, Language::English, "Hilda").unwrap();
        assert_eq!(&units[..6], &[0x48, 0x69, 0x6C, 0x64, 0x61, END]);
        assert_eq!(decode(Gen::Five, &units), "Hilda");
    }

    #[test]
    fn names_past_the_limit_are_refused() {
        assert_eq!(
            encode::<8>(Gen::Five, Language::English, "Cheren12"),
            Err(TextError::TooLong(7))
        );
        assert_eq!(
            encode::<8>(Gen::Four, Language::English, "N\u{1F600}"),
            Err(TextError::Unsupported('\u{1F600}'))
        );
    }

    #[test]
    fn gen4_japanese_names_use_kana_codes() {
        let units: [u16; 6] = encode(Gen::Four, Language::Japanese, "ヒカリ").unwrap();
        assert_eq!(units, [0x0083, 0x005C, 0x009B, END, 0, 0]);
        assert_eq!(decode(Gen::Four, &units), "ヒカリ");
    }

    #[test]
    fn gen4_korean_names_use_the_hangul_range() {
        let units: [u16; 6] = encode(Gen::Four, Language::Korean, "루비").unwrap();
        assert_eq!(units[..3], [0x06D9, 0x07FD, END]);
        assert_eq!(decode(Gen::Four, &units), "루비");
    }

    #[test]
    fn gen4_symbols_follow_the_games_width_per_language() {
        let japanese: [u16; 4] = encode(Gen::Four, Language::Japanese, "ニ♂ー").unwrap();
        assert_eq!(japanese[..3], [0x007C, 0x00EE, 0x00F1]);
        let english: [u16; 4] = encode(Gen::Four, Language::English, "♂'«").unwrap();
        assert_eq!(english[..3], [0x01BB, 0x01B3, 0x01B7]);
        assert_eq!(decode(Gen::Four, &japanese), "ニ♂ー");
        assert_eq!(decode(Gen::Four, &english), "♂'«");
    }

    #[test]
    fn gen5_gender_symbols_use_the_games_own_codes_outside_japan() {
        let english: [u16; 4] = encode(Gen::Five, Language::English, "N♂♀").unwrap();
        assert_eq!(english[..3], [0x004E, 0x246D, 0x246E]);
        let japanese: [u16; 3] = encode(Gen::Five, Language::Japanese, "♂♀").unwrap();
        assert_eq!(japanese[..2], [0x2642, 0x2640]);
        assert_eq!(decode(Gen::Five, &english), "N♂♀");
        assert_eq!(decode(Gen::Five, &japanese), "♂♀");
    }

    #[test]
    fn gen4_korean_starts_one_past_the_block() {
        let units: [u16; 4] = encode(Gen::Four, Language::Korean, "가디").unwrap();
        assert_eq!(units[0], 0x0401);
        assert_eq!(decode(Gen::Four, &[0x0400, END]), "\u{FFFD}");
        let jamo: [u16; 2] = encode(Gen::Four, Language::Korean, "ㄱ").unwrap();
        assert_eq!(jamo[0], 0x0D31);
    }

    #[test]
    fn every_gen4_code_round_trips_in_its_own_width() {
        let mut checked = 0;
        for unit in 1..0x1000u16 {
            if let Some(c) = gen4::char_of(unit) {
                checked += 1;
                let full = gen4::unit_of(c, true);
                let half = gen4::unit_of(c, false);
                assert!(full == Some(unit) || half == Some(unit), "{unit:#x}");
            }
        }
        assert!(checked > 2800);
    }
}
