use super::{Growth, Mon};
use crate::state::{Gender, Stats};

pub(crate) const RECORD: usize = 28;
const TYPES: u8 = 18;
const EGG_GROUPS: u8 = 16;
const GENDERLESS: u8 = 255;
const ALWAYS_FEMALE: u8 = 254;
const SHEDINJA: u16 = 292;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaseStats {
    pub hp: u8,
    pub attack: u8,
    pub defense: u8,
    pub speed: u8,
    pub special_attack: u8,
    pub special_defense: u8,
    pub types: [u8; 2],
    pub catch_rate: u8,
    pub exp_yield: u8,
    pub gender_ratio: u8,
    pub growth: Growth,
    pub abilities: [u8; 2],
}

impl BaseStats {
    pub(crate) fn parse(r: &[u8]) -> Option<Self> {
        if !plausible(r) {
            return None;
        }
        Some(Self {
            hp: r[0],
            attack: r[1],
            defense: r[2],
            speed: r[3],
            special_attack: r[4],
            special_defense: r[5],
            types: [r[6], r[7]],
            catch_rate: r[8],
            exp_yield: r[9],
            gender_ratio: r[16],
            growth: Growth::from_byte(r[19])?,
            abilities: [r[22], r[23]],
        })
    }

    pub fn ability(&self, slot: u8) -> u8 {
        match self.abilities {
            [first, 0] => first,
            [first, second] => {
                if slot == 0 {
                    first
                } else {
                    second
                }
            }
        }
    }

    pub fn gender(&self, personality: u32) -> Option<Gender> {
        match self.gender_ratio {
            GENDERLESS => None,
            ALWAYS_FEMALE => Some(Gender::Female),
            ratio if (personality & 0xFF) < u32::from(ratio) => Some(Gender::Female),
            _ => Some(Gender::Male),
        }
    }

    pub(crate) fn stats(&self, mon: &Mon, level: u8, national: u16) -> Stats {
        let level = u32::from(level);
        let core = |base: u8, iv: u8, ev: u8| {
            (2 * u32::from(base) + u32::from(iv) + u32::from(ev) / 4) * level / 100
        };
        let nature = mon.nature() as usize;
        let (up, down) = (nature / 5, nature % 5);
        let other = |index: usize, base: u8, iv: u8, ev: u8| {
            let plain = core(base, iv, ev) + 5;
            let scaled = if up == down {
                plain
            } else if index == up {
                plain * 110 / 100
            } else if index == down {
                plain * 90 / 100
            } else {
                plain
            };
            scaled as u16
        };
        let (iv, ev) = (mon.ivs, mon.evs);
        let max_hp = if national == SHEDINJA {
            1
        } else {
            (core(self.hp, iv.hp, ev.hp) + level + 10) as u16
        };
        Stats {
            max_hp,
            attack: other(0, self.attack, iv.attack, ev.attack),
            defense: other(1, self.defense, iv.defense, ev.defense),
            speed: other(2, self.speed, iv.speed, ev.speed),
            special_attack: other(3, self.special_attack, iv.special_attack, ev.special_attack),
            special_defense: other(
                4,
                self.special_defense,
                iv.special_defense,
                ev.special_defense,
            ),
        }
    }
}

pub(crate) fn plausible(r: &[u8]) -> bool {
    r.len() == RECORD
        && r[6] < TYPES
        && r[7] < TYPES
        && r[19] <= 5
        && r[20] < EGG_GROUPS
        && r[21] < EGG_GROUPS
        && r[26] == 0
        && r[27] == 0
}
