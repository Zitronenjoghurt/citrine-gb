use std::collections::HashMap;

use super::data::{GameData, Kind, Rod, Slot, tile};
use super::{POKE_BALL, Reason, SAFARI_BALL, SPORT_BALL, UNDISCOVERED, form_allowed};
use crate::pkm::{Gender, Language, Pokemon};
use crate::rng::{Calls, LeadOutcome, Method, Spread, WildRoll, wild_rolls};
use crate::tables::Game;

pub const EAST_SEA: [u16; 4] = [11, 27, 28, 39];

const GRASS: [u8; 12] = [20, 40, 50, 60, 70, 80, 85, 90, 94, 98, 99, 100];
const WATER: [u8; 5] = [60, 90, 95, 99, 100];
const J_GOOD_ROD: [u8; 5] = [40, 80, 95, 99, 100];
const K_ROD: [u8; 5] = [40, 70, 85, 95, 100];
const ROCK_SMASH: [u8; 2] = [80, 100];
const BUG_CONTEST: [u8; 10] = [80, 60, 50, 40, 30, 20, 15, 10, 5, 0];
const SAFARI_SLOTS: u16 = 10;
const LEVEL_EVOLUTIONS: [u16; 6] = [4, 8, 9, 10, 22, 23];
const ANY_TIME_EVOLUTIONS: [u16; 3] = [5, 6, 7];
const MALE_ONLY: [u16; 2] = [16, 22];
const FEMALE_ONLY: [u16; 2] = [17, 23];
const DITTO_EGG_GROUP: u8 = 13;
const KOREAN_PLATINUM_NO_HATCH: [u16; 2] = [63, 85];
const MANAPHY: u16 = 490;
const SPLIT_GENDER_EGGS: [u16; 2] = [29, 314];
const SPLIT_GENDER_BIT: u32 = 0x8000;
const INCENSE_BABIES: [u16; 9] = [298, 360, 406, 433, 438, 439, 440, 446, 458];
const WILD_BALLS: [u8; 14] = [1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const APRICORN_BALLS: [u8; 7] = [17, 18, 19, 20, 21, 22, 23];

pub(crate) struct Facts<'a> {
    pub data: &'static GameData,
    pub pkm: &'a Pokemon,
    pub level: u8,
    pub location: u16,
    pub ball: u8,
}

pub(crate) fn method_of(game: Game) -> Method {
    if super::data::is_hgss(game) {
        Method::K
    } else {
        Method::J
    }
}

pub(crate) fn calls_of(kind: Kind) -> Calls {
    let (before_slot, level, until_perfect_iv) = match kind {
        Kind::Grass => (0, false, false),
        Kind::Surf => (0, true, false),
        Kind::Rod(_) | Kind::RockSmash => (1, true, false),
        Kind::SafariGrass | Kind::SafariSurf => (0, false, true),
        Kind::SafariRod(_) => (1, false, true),
        Kind::BugContest => (2, true, true),
    };
    Calls {
        before_slot,
        level,
        until_perfect_iv,
    }
}

fn roll(method: Method, out: u16) -> u8 {
    match method {
        Method::J => (out / 656) as u8,
        _ => (out % 100) as u8,
    }
}

fn pick(table: &[u8], roll: u8) -> u8 {
    table
        .iter()
        .position(|&edge| roll < edge)
        .unwrap_or(table.len() - 1) as u8
}

pub(crate) fn slot_of(method: Method, kind: Kind, out: u16) -> u8 {
    let r = roll(method, out);
    match (kind, method) {
        (Kind::Grass, _) => pick(&GRASS, r),
        (Kind::Surf, _) | (Kind::Rod(Rod::Old), Method::J) => pick(&WATER, r),
        (Kind::Rod(_), Method::J) => pick(&J_GOOD_ROD, r),
        (Kind::Rod(_), _) => pick(&K_ROD, r),
        (Kind::RockSmash, _) => pick(&ROCK_SMASH, r),
        (Kind::SafariGrass | Kind::SafariSurf | Kind::SafariRod(_), _) => {
            (out % SAFARI_SLOTS) as u8
        }
        (Kind::BugContest, _) => BUG_CONTEST
            .iter()
            .position(|&floor| r >= floor)
            .unwrap_or(BUG_CONTEST.len() - 1) as u8,
    }
}

const FRIENDLY_LEAD_BONUS: u8 = 50;
const BUG_CONTEST_RATE: u8 = 25;

fn bite_rate(kind: Kind) -> Option<u8> {
    match kind {
        Kind::Rod(rod) | Kind::SafariRod(rod) => Some(match rod {
            Rod::Old => 25,
            Rod::Good => 50,
            Rod::Super => 75,
        }),
        _ => None,
    }
}

fn starts(method: Method, kind: Kind, before_slot: [u16; 2]) -> bool {
    match (kind, bite_rate(kind)) {
        (_, Some(rate)) if method == Method::J => roll(method, before_slot[0]) < rate,
        (_, Some(rate)) => roll(method, before_slot[0]) < rate + FRIENDLY_LEAD_BONUS,
        (Kind::BugContest, _) => roll(method, before_slot[1]) < BUG_CONTEST_RATE,
        _ => true,
    }
}

pub(crate) fn level_from(out: u16, min: u8, max: u8) -> u8 {
    let range = u16::from(max - min) + 1;
    min + (out % range) as u8
}

pub(crate) fn fits(method: Method, slot: &Slot, met_level: u8, roll_: &WildRoll) -> bool {
    let e = &slot.encounter;
    let index = slot_of(method, slot.kind, roll_.slot);
    let slot_ok = if slot.block {
        index <= e.slot
    } else {
        index == e.slot
    };
    let level_ok = match roll_.level {
        Some(out) if e.min_level != e.max_level => {
            level_from(out, e.min_level, e.max_level) == met_level
        }
        _ => true,
    };
    let charmed = matches!(
        roll_.lead,
        LeadOutcome::CuteCharmPass | LeadOutcome::CuteCharmFail
    );
    let lead_ok = !(charmed && calls_of(slot.kind).until_perfect_iv);
    slot_ok && level_ok && lead_ok && starts(method, slot.kind, roll_.before_slot)
}

pub(crate) fn balls_for(data: &GameData, slot: &Slot, location: u16) -> Vec<u8> {
    match slot.kind {
        Kind::SafariGrass | Kind::SafariSurf | Kind::SafariRod(_) => vec![SAFARI_BALL],
        Kind::BugContest => vec![SPORT_BALL],
        _ if data.game == Game::Platinum && location == super::data::GREAT_MARSH => {
            vec![SAFARI_BALL]
        }
        _ if super::data::is_hgss(data.game) => [&WILD_BALLS[..], &APRICORN_BALLS[..]].concat(),
        _ => WILD_BALLS.to_vec(),
    }
}

pub(crate) fn family(data: &GameData, species: u16) -> Vec<u16> {
    let mut chain = vec![species];
    let mut at = 0;
    while at < chain.len() {
        if let Some(from) = data.evolves_from.get(&chain[at]) {
            for &(s, _) in from {
                if !chain.contains(&s) {
                    chain.push(s);
                }
            }
        }
        at += 1;
    }
    chain
}

fn can_evolve(data: &GameData, from: u16, into: u16, level: u8, met: u8, gender: Gender) -> bool {
    if from == into {
        return true;
    }
    data.evolves_from.get(&into).is_some_and(|parents| {
        parents.iter().any(|&(parent, e)| {
            let step = if LEVEL_EVOLUTIONS.contains(&e.method) {
                level > met && u16::from(level) >= e.param
            } else if ANY_TIME_EVOLUTIONS.contains(&e.method) {
                true
            } else {
                level > met
            };
            let gender_ok = !(MALE_ONLY.contains(&e.method) && gender != Gender::Male
                || FEMALE_ONLY.contains(&e.method) && gender != Gender::Female);
            step && gender_ok && can_evolve(data, from, parent, level, met, gender)
        })
    })
}

impl Facts<'_> {
    pub(crate) fn wild(&self) -> Result<(), Reason> {
        let pkm = self.pkm;
        if pkm.met_level == 0 || pkm.met_level > self.level {
            return Err(Reason::MetLevel(pkm.met_level));
        }
        if pkm.species == super::UNOWN {
            return Err(Reason::Unchecked("wild Unown"));
        }
        let family = family(self.data, pkm.species);
        let candidates: Vec<&Slot> = self
            .data
            .slots
            .get(&self.location)
            .into_iter()
            .flatten()
            .filter(|s| {
                let e = &s.encounter;
                family.contains(&e.species)
                    && (e.min_level..=e.max_level).contains(&pkm.met_level)
                    && can_evolve(
                        self.data,
                        e.species,
                        pkm.species,
                        self.level,
                        pkm.met_level,
                        pkm.gender,
                    )
            })
            .collect();
        if candidates.is_empty() {
            return Err(Reason::NoEncounter(self.location));
        }
        let formed: Vec<&Slot> = candidates
            .into_iter()
            .filter(|_| form_allowed(self.data, pkm.species, pkm.form, Some(self.location)))
            .collect();
        if formed.is_empty() {
            return Err(Reason::Form(pkm.form));
        }
        let balled: Vec<&Slot> = formed
            .into_iter()
            .filter(|s| balls_for(self.data, s, self.location).contains(&self.ball))
            .collect();
        if balled.is_empty() {
            return Err(Reason::Ball(self.ball));
        }
        let tiled: Vec<&Slot> = balled
            .into_iter()
            .filter(|s| s.allows_tile(pkm.ground_tile))
            .collect();
        if tiled.is_empty() {
            return Err(Reason::GroundTile(pkm.ground_tile));
        }
        let method = method_of(self.data.game);
        let spread = Spread {
            pid: pkm.pid,
            ivs: pkm.ivs,
        };
        let mut rolls: HashMap<(Calls, u8), Vec<WildRoll>> = HashMap::new();
        for slot in tiled {
            let calls = calls_of(slot.kind);
            let ratio =
                self.data.tables.base_stats[usize::from(slot.encounter.species)].gender_ratio;
            let found = rolls
                .entry((calls, ratio))
                .or_insert_with(|| wild_rolls(method, spread, calls, ratio));
            if found.iter().any(|r| fits(method, slot, pkm.met_level, r)) {
                return Ok(());
            }
        }
        Err(Reason::Correlation)
    }

    pub(crate) fn hatched(&self) -> Result<(), Reason> {
        let pkm = self.pkm;
        if pkm.met_level != 0 {
            return Err(Reason::MetLevel(pkm.met_level));
        }
        if self.ball != POKE_BALL {
            return Err(Reason::Ball(self.ball));
        }
        if pkm.ground_tile != tile::NONE {
            return Err(Reason::GroundTile(pkm.ground_tile));
        }
        let korean_platinum = self.data.game == Game::Platinum && pkm.language == Language::Korean;
        if !self.data.hatch_places.contains(&self.location)
            || korean_platinum && KOREAN_PLATINUM_NO_HATCH.contains(&self.location)
        {
            return Err(Reason::MetLocation);
        }
        if !form_allowed(self.data, pkm.species, pkm.form, None) {
            return Err(Reason::Form(pkm.form));
        }
        let tables = &self.data.tables;
        let undiscovered = |s: u16| {
            tables.base_stats[usize::from(s)]
                .egg_groups
                .contains(&UNDISCOVERED)
        };
        let breedable = |s: u16| {
            let ditto = tables.base_stats[usize::from(s)]
                .egg_groups
                .contains(&DITTO_EGG_GROUP);
            let from_parents = !undiscovered(s)
                || tables.evolutions[usize::from(s)]
                    .iter()
                    .any(|e| !undiscovered(e.into));
            !ditto && s != MANAPHY && from_parents
        };
        let root = *family(self.data, pkm.species)
            .last()
            .unwrap_or(&pkm.species);
        let mut eggs = vec![root];
        if INCENSE_BABIES.contains(&root) {
            eggs.extend(tables.evolutions[usize::from(root)].iter().map(|e| e.into));
        }
        let hatchable = eggs.iter().any(|&egg| {
            breedable(egg) && can_evolve(self.data, egg, pkm.species, self.level, 0, pkm.gender)
        });
        if !hatchable {
            return Err(Reason::NotBreedable);
        }
        if SPLIT_GENDER_EGGS.contains(&root) && pkm.pid & SPLIT_GENDER_BIT != 0 {
            return Err(Reason::Gender);
        }
        Ok(())
    }
}
