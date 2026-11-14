use crate::pkm::{Gen, Language};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Game {
    Diamond,
    Pearl,
    Platinum,
    HeartGold,
    SoulSilver,
    Black,
    White,
    Black2,
    White2,
}

const MAP_NAMES: &str = "fielddata/maptable/mapname.bin";

pub(crate) struct Paths {
    pub personal: &'static str,
    pub learnsets: &'static str,
    pub evolutions: &'static str,
    pub encounters: &'static str,
    pub messages: &'static str,
    pub species_names: usize,
    pub growth: Option<&'static str>,
    pub moves: Option<&'static str>,
    pub map_names: Option<&'static str>,
    pub zones: Option<&'static str>,
    pub swaps: Option<&'static str>,
    pub safari: Option<&'static str>,
    pub bug_contest: Option<&'static str>,
}

impl Game {
    pub fn identify(code: [u8; 4]) -> Option<(Self, Language)> {
        let game = match &code[..3] {
            b"ADA" => Self::Diamond,
            b"APA" => Self::Pearl,
            b"CPU" => Self::Platinum,
            b"IPK" => Self::HeartGold,
            b"IPG" => Self::SoulSilver,
            b"IRB" => Self::Black,
            b"IRA" => Self::White,
            b"IRE" => Self::Black2,
            b"IRD" => Self::White2,
            _ => return None,
        };
        let language = match code[3] {
            b'J' => Language::Japanese,
            b'E' | b'O' => Language::English,
            b'F' => Language::French,
            b'I' => Language::Italian,
            b'D' => Language::German,
            b'S' => Language::Spanish,
            b'K' => Language::Korean,
            other => Language::Other(other),
        };
        Some((game, language))
    }

    pub fn generation(self) -> Gen {
        match self {
            Self::Diamond | Self::Pearl | Self::Platinum | Self::HeartGold | Self::SoulSilver => {
                Gen::Four
            }
            Self::Black | Self::White | Self::Black2 | Self::White2 => Gen::Five,
        }
    }

    pub fn version_byte(self) -> u8 {
        match self {
            Self::Diamond => 10,
            Self::Pearl => 11,
            Self::Platinum => 12,
            Self::HeartGold => 7,
            Self::SoulSilver => 8,
            Self::White => 20,
            Self::Black => 21,
            Self::White2 => 22,
            Self::Black2 => 23,
        }
    }

    pub(crate) fn paths(self) -> Paths {
        let dp = |personal, encounters| Paths {
            personal,
            learnsets: "poketool/personal/wotbl.narc",
            evolutions: "poketool/personal/evo.narc",
            encounters,
            messages: "msgdata/msg.narc",
            species_names: 362,
            growth: Some("poketool/personal/growtbl.narc"),
            moves: Some("poketool/waza/waza_tbl.narc"),
            map_names: Some(MAP_NAMES),
            zones: None,
            swaps: None,
            safari: None,
            bug_contest: None,
        };
        let hgss = |encounters| Paths {
            personal: "a/0/0/2",
            learnsets: "a/0/3/3",
            evolutions: "a/0/3/4",
            encounters,
            messages: "a/0/2/7",
            species_names: 237,
            growth: Some("a/0/0/3"),
            moves: Some("a/0/1/1"),
            map_names: Some(MAP_NAMES),
            zones: None,
            swaps: None,
            safari: Some("a/2/3/0"),
            bug_contest: Some("data/mushi/mushi_encount.bin"),
        };
        let bw = |encounters, species_names| Paths {
            personal: "a/0/1/6",
            learnsets: "a/0/1/8",
            evolutions: "a/0/1/9",
            encounters,
            messages: "a/0/0/2",
            species_names,
            growth: None,
            moves: None,
            map_names: None,
            zones: None,
            swaps: None,
            safari: None,
            bug_contest: None,
        };
        match self {
            Self::Diamond => dp(
                "poketool/personal/personal.narc",
                "fielddata/encountdata/d_enc_data.narc",
            ),
            Self::Pearl => dp(
                "poketool/personal_pearl/personal.narc",
                "fielddata/encountdata/p_enc_data.narc",
            ),
            Self::Platinum => Paths {
                personal: "poketool/personal/pl_personal.narc",
                learnsets: "poketool/personal/wotbl.narc",
                evolutions: "poketool/personal/evo.narc",
                encounters: "fielddata/encountdata/pl_enc_data.narc",
                messages: "msgdata/pl_msg.narc",
                species_names: 412,
                growth: Some("poketool/personal/pl_growtbl.narc"),
                moves: Some("poketool/waza/pl_waza_tbl.narc"),
                map_names: Some(MAP_NAMES),
                zones: None,
                swaps: Some("arc/encdata_ex.narc"),
                safari: None,
                bug_contest: None,
            },
            Self::HeartGold => hgss("a/0/3/7"),
            Self::SoulSilver => hgss("a/1/3/6"),
            Self::Black | Self::White => Paths {
                zones: Some("a/0/1/2"),
                ..bw("a/1/2/6", 70)
            },
            Self::Black2 | Self::White2 => bw("a/1/2/7", 90),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_codes_name_the_game_and_its_language() {
        assert_eq!(
            Game::identify(*b"CPUJ"),
            Some((Game::Platinum, Language::Japanese))
        );
        assert_eq!(
            Game::identify(*b"IPKK"),
            Some((Game::HeartGold, Language::Korean))
        );
        assert_eq!(
            Game::identify(*b"IRDE"),
            Some((Game::White2, Language::English))
        );
        assert_eq!(Game::identify(*b"AMCE"), None);
    }
}
