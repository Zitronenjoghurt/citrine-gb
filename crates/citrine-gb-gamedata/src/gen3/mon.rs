use super::{Met, Mon, MoveSlot, PartyStats, Pokemon, Spread, text, u16_at, u32_at};
use crate::game::Language;
use crate::state::{Gender, Stats};

pub(crate) const BOX_SIZE: usize = 80;
pub(crate) const PARTY_SIZE: usize = 100;
const DATA: usize = 0x20;
const BLOCK: usize = 12;
const FLAGS: usize = 0x13;
const BAD_EGG: u8 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Growth {
    MediumFast,
    Erratic,
    Fluctuating,
    MediumSlow,
    Fast,
    Slow,
}

impl Growth {
    pub fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => Self::MediumFast,
            1 => Self::Erratic,
            2 => Self::Fluctuating,
            3 => Self::MediumSlow,
            4 => Self::Fast,
            5 => Self::Slow,
            _ => return None,
        })
    }

    pub fn exp(self, level: u8) -> u32 {
        let n = i64::from(level);
        let cube = n * n * n;
        let exp = match self {
            Self::MediumFast => cube,
            Self::Erratic => match n {
                ..50 => cube * (100 - n) / 50,
                50..68 => cube * (150 - n) / 100,
                68..98 => cube * ((1911 - 10 * n) / 3) / 500,
                _ => cube * (160 - n) / 100,
            },
            Self::Fluctuating => match n {
                ..15 => cube * ((n + 1) / 3 + 24) / 50,
                15..36 => cube * (n + 14) / 50,
                _ => cube * (n / 2 + 32) / 50,
            },
            Self::MediumSlow => 6 * cube / 5 - 15 * n * n + 100 * n - 140,
            Self::Fast => 4 * cube / 5,
            Self::Slow => 5 * cube / 4,
        };
        u32::try_from(exp.max(0)).unwrap_or(u32::MAX)
    }

    pub fn level(self, exp: u32) -> u8 {
        (2..=100)
            .rev()
            .find(|&level| self.exp(level) <= exp)
            .unwrap_or(1)
    }
}

fn positions(personality: u32) -> [usize; 4] {
    let mut rest = vec![0, 1, 2, 3];
    let mut n = (personality % 24) as usize;
    let mut at = [0; 4];
    for (position, step) in [6, 2, 1, 1].into_iter().enumerate() {
        at[rest.remove(n / step)] = position;
        n %= step;
    }
    at
}

fn language(byte: u8) -> Option<Language> {
    Some(match byte {
        1 => Language::Japanese,
        2 => Language::English,
        3 => Language::French,
        4 => Language::Italian,
        5 => Language::German,
        7 => Language::Spanish,
        _ => return None,
    })
}

pub(crate) fn read(bytes: &[u8]) -> Option<Pokemon> {
    if bytes.iter().all(|&b| b == 0) {
        return None;
    }
    let personality = u32_at(bytes, 0);
    let ot = u32_at(bytes, 4);
    let key = personality ^ ot;
    let data: Vec<u8> = bytes[DATA..DATA + 4 * BLOCK]
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|word| (u32::from_le_bytes(*word) ^ key).to_le_bytes())
        .collect();
    let sum = data.as_chunks::<2>().0.iter().fold(0u16, |sum, half| {
        sum.wrapping_add(u16::from_le_bytes(*half))
    });
    if sum != u16_at(bytes, 0x1C) || bytes[FLAGS] & BAD_EGG != 0 {
        return Some(Pokemon::BadEgg);
    }
    let at = positions(personality);
    let block = |kind: usize| &data[at[kind] * BLOCK..][..BLOCK];
    let (growth, attacks, effort, misc) = (block(0), block(1), block(2), block(3));
    let species = u16_at(growth, 0);
    if species == 0 {
        return None;
    }
    let language = language(bytes[0x12]);
    let table = language.unwrap_or(Language::English);
    let genes = u32_at(misc, 4);
    let iv = |n: u32| ((genes >> (5 * n)) & 0x1F) as u8;
    let origins = u16_at(misc, 2);
    let party = bytes.len() >= PARTY_SIZE;
    let mon = Box::new(Mon {
        species,
        personality,
        ot_id: ot as u16,
        ot_secret_id: (ot >> 16) as u16,
        nickname: text::decode(&bytes[0x08..0x12], table),
        ot_name: text::decode(&bytes[0x14..0x1B], table),
        language,
        held_item: Some(u16_at(growth, 2)).filter(|&item| item != 0),
        exp: u32_at(growth, 4),
        level: party.then(|| bytes[0x54]),
        friendship: growth[9],
        moves: (0..4)
            .filter_map(|i| {
                let id = u16_at(attacks, 2 * i);
                (id != 0).then(|| MoveSlot {
                    id,
                    pp: attacks[8 + i],
                    pp_ups: (growth[8] >> (2 * i)) & 0x03,
                })
            })
            .collect(),
        ivs: Spread {
            hp: iv(0),
            attack: iv(1),
            defense: iv(2),
            speed: iv(3),
            special_attack: iv(4),
            special_defense: iv(5),
        },
        evs: Spread {
            hp: effort[0],
            attack: effort[1],
            defense: effort[2],
            speed: effort[3],
            special_attack: effort[4],
            special_defense: effort[5],
        },
        ability_slot: (genes >> 31) as u8,
        pokerus: misc[0],
        met: Met {
            location: misc[1],
            level: (origins & 0x7F) as u8,
            game: ((origins >> 7) & 0x0F) as u8,
            ball: ((origins >> 11) & 0x0F) as u8,
            ot_gender: if origins >> 15 == 1 {
                Gender::Female
            } else {
                Gender::Male
            },
        },
        party: party.then(|| PartyStats {
            status: u32_at(bytes, 0x50),
            hp: u16_at(bytes, 0x56),
            stats: Stats {
                max_hp: u16_at(bytes, 0x58),
                attack: u16_at(bytes, 0x5A),
                defense: u16_at(bytes, 0x5C),
                speed: u16_at(bytes, 0x5E),
                special_attack: u16_at(bytes, 0x60),
                special_defense: u16_at(bytes, 0x62),
            },
        }),
    });
    Some(if genes >> 30 & 1 == 1 {
        Pokemon::Egg(mon)
    } else {
        Pokemon::Hatched(mon)
    })
}
