use crate::game::{Game, Generation};

const SUPER: u8 = 20;
const NOT_VERY: u8 = 5;
const NO_EFFECT: u8 = 0;
const FORESIGHT_RULES: u8 = 0xFE;
const END: u8 = 0xFF;
const MIN_RULES: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effect {
    NoEffect,
    Quarter,
    Half,
    Normal,
    Double,
    Quadruple,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TypeChart {
    rules: Vec<(u8, u8, u8)>,
}

impl TypeChart {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn effect(&self, attack: u8, defenders: &[u8]) -> Effect {
        let mut defenders = defenders.to_vec();
        defenders.dedup();
        let mut tenths = 1u32;
        let mut scale = 1u32;
        for defender in defenders {
            let multiplier = self
                .rules
                .iter()
                .find(|(a, d, _)| *a == attack && *d == defender)
                .map_or(10, |rule| rule.2);
            tenths *= u32::from(multiplier);
            scale *= 10;
        }
        match tenths * 100 / scale {
            0 => Effect::NoEffect,
            1..=25 => Effect::Quarter,
            26..=50 => Effect::Half,
            51..=100 => Effect::Normal,
            101..=200 => Effect::Double,
            _ => Effect::Quadruple,
        }
    }
}

pub(crate) fn load(game: Game, rom: &[u8]) -> TypeChart {
    let types = match game.generation() {
        Generation::One => 0x1B,
        Generation::Two => 0x1C,
        Generation::Three => return TypeChart::default(),
    };
    let rule = |at: usize| -> Option<(u8, u8, u8)> {
        let bytes = rom.get(at..at + 3)?;
        (bytes[0] < types && bytes[1] < types && matches!(bytes[2], SUPER | NOT_VERY | NO_EFFECT))
            .then_some((bytes[0], bytes[1], bytes[2]))
    };
    for phase in 0..3 {
        let mut start = phase;
        let mut at = phase;
        while at + 3 <= rom.len() {
            if rule(at).is_some() {
                at += 3;
                continue;
            }
            if let Some(chart) = chart(rom, start, at, &rule) {
                return chart;
            }
            at += 3;
            start = at;
        }
    }
    TypeChart::default()
}

fn chart(
    rom: &[u8],
    start: usize,
    end: usize,
    rule: &dyn Fn(usize) -> Option<(u8, u8, u8)>,
) -> Option<TypeChart> {
    if (end - start) / 3 < MIN_RULES || !matches!(rom.get(end), Some(&(END | FORESIGHT_RULES))) {
        return None;
    }
    let mut rules: Vec<(u8, u8, u8)> = (start..end).step_by(3).filter_map(rule).collect();
    if rom[end] == FORESIGHT_RULES {
        let mut at = end + 1;
        while let Some(extra) = rule(at) {
            rules.push(extra);
            at += 3;
        }
        if rom.get(at) != Some(&END) {
            return None;
        }
    }
    let supers = rules.iter().filter(|r| r.2 == SUPER).count();
    let mut pairs: Vec<(u8, u8)> = rules.iter().map(|r| (r.0, r.1)).collect();
    pairs.sort_unstable();
    pairs.dedup();
    let mut attackers: Vec<u8> = rules.iter().map(|r| r.0).collect();
    attackers.sort_unstable();
    attackers.dedup();
    (supers >= 10 && attackers.len() >= 8 && pairs.len() == rules.len())
        .then_some(TypeChart { rules })
}
