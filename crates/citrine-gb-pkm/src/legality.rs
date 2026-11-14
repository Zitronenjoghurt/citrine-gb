mod data;
mod origin;

use crate::pkm::{Date, Extra, Gender, Language, Pokemon};
use crate::tables::BaseStats;
use data::GameData;

pub use data::{Kind, Rod, Slot};
pub use origin::EAST_SEA;

pub fn tables(version: u8) -> Option<&'static crate::tables::Tables> {
    GameData::for_version(version).map(|d| &d.tables)
}

pub fn slots(version: u8) -> Vec<(u16, &'static [Slot])> {
    let mut all: Vec<(u16, &'static [Slot])> = GameData::for_version(version)
        .map(|d| d.slots.iter().map(|(&l, s)| (l, s.as_slice())).collect())
        .unwrap_or_default();
    all.sort_by_key(|&(l, _)| l);
    all
}

pub fn hatch_places(version: u8) -> Vec<u16> {
    let mut all: Vec<u16> = GameData::for_version(version)
        .map(|d| d.hatch_places.iter().copied().collect())
        .unwrap_or_default();
    all.sort_unstable();
    all
}

pub fn method(version: u8) -> Option<crate::rng::Method> {
    GameData::for_version(version).map(|d| origin::method_of(d.game))
}

pub fn calls(kind: Kind) -> crate::rng::Calls {
    origin::calls_of(kind)
}

pub fn slot_index(method: crate::rng::Method, kind: Kind, out: u16) -> u8 {
    origin::slot_of(method, kind, out)
}

pub fn rolled_level(out: u16, min: u8, max: u8) -> u8 {
    origin::level_from(out, min, max)
}

pub fn fits(
    method: crate::rng::Method,
    slot: &Slot,
    met_level: u8,
    roll: &crate::rng::WildRoll,
) -> bool {
    origin::fits(method, slot, met_level, roll)
}

pub fn balls(version: u8, slot: &Slot, location: u16) -> Vec<u8> {
    GameData::for_version(version)
        .map(|d| origin::balls_for(d, slot, location))
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Reason {
    #[error("only Gen 4 Pokémon are checked")]
    Generation,
    #[error("no tables for game version {0}")]
    Version(u8),
    #[error("eggs can't be traded")]
    Egg,
    #[error("species {0} isn't in this game")]
    Species(u16),
    #[error("form {0} can't be found here")]
    Form(u8),
    #[error("the language byte names no language")]
    Language,
    #[error("the nickname has {len} characters, {max} at most")]
    NicknameTooLong { len: usize, max: usize },
    #[error("the trainer name has {len} characters, {max} at most")]
    TrainerNameTooLong { len: usize, max: usize },
    #[error("the trainer name is empty")]
    TrainerNameEmpty,
    #[error("the experience doesn't fit the level")]
    Experience,
    #[error("the gender doesn't match the PID")]
    Gender,
    #[error("the ability doesn't match the PID")]
    Ability,
    #[error("ball {0} can't catch this Pokémon here")]
    Ball(u8),
    #[error("the two ball bytes disagree")]
    BallBytes,
    #[error("the met location fields disagree")]
    MetLocation,
    #[error("the egg location fields are wrong")]
    EggLocation,
    #[error("the dates are impossible")]
    Date,
    #[error("ground tile {0} doesn't match where it was met")]
    GroundTile(u8),
    #[error("move {0} can't be learned by level up by then")]
    Move(u16),
    #[error("the moves are empty or repeat")]
    Moves,
    #[error("the PP of move slot {0} is wrong")]
    Pp(usize),
    #[error("the EVs add up to more than 510")]
    Evs,
    #[error("met level {0} is impossible")]
    MetLevel(u8),
    #[error("no wild slot at location {0} gives this species at this level")]
    NoEncounter(u16),
    #[error("the PID and IVs fit no game call order for the slot")]
    Correlation,
    #[error("this species can't hatch from an egg")]
    NotBreedable,
    #[error("{0} aren't checked yet")]
    Unchecked(&'static str),
}

const MAX_EVS: u32 = 510;
const MAX_LEVEL: usize = 100;
const DAY_CARE: u16 = 2000;
const FARAWAY_PLACE: u16 = 3002;
const LAST_DP_LOCATION: u16 = 111;
const POKE_BALL: u8 = 4;
const SAFARI_BALL: u8 = 5;
const CHERISH_BALL: u8 = 16;
const SPORT_BALL: u8 = 24;
const UNDISCOVERED: u8 = 15;
const UNOWN: u16 = 201;
const SHELLOS: u16 = 422;
const GASTRODON: u16 = 423;
const UNOWN_FORMS: u8 = 28;

pub fn check(pkm: &Pokemon) -> Result<(), Reason> {
    let Extra::Gen4 { .. } = pkm.extra else {
        return Err(Reason::Generation);
    };
    let data = GameData::for_version(pkm.version).ok_or(Reason::Version(pkm.version))?;
    if pkm.is_egg {
        return Err(Reason::Egg);
    }
    let base = data
        .tables
        .base_stats
        .get(usize::from(pkm.species))
        .filter(|_| (1..=493).contains(&pkm.species))
        .ok_or(Reason::Species(pkm.species))?;
    names(pkm)?;
    let level = level_of(data, base, pkm.exp)?;
    if pkm.party.is_some_and(|p| usize::from(p.level) != level) {
        return Err(Reason::Experience);
    }
    if pkm.gender != gender_of(pkm.pid, base.gender_ratio) {
        return Err(Reason::Gender);
    }
    let ability = match base.abilities[(pkm.pid & 1) as usize] {
        0 => base.abilities[0],
        a => a,
    };
    if pkm.ability != ability {
        return Err(Reason::Ability);
    }
    if pkm
        .evs
        .to_array()
        .iter()
        .map(|&e| u32::from(e))
        .sum::<u32>()
        > MAX_EVS
    {
        return Err(Reason::Evs);
    }
    if pkm.fateful_encounter {
        return Err(Reason::Unchecked("event Pokémon"));
    }
    if pkm.ribbons.count() > 0 {
        return Err(Reason::Unchecked("Pokémon with ribbons"));
    }
    let location = met_location(pkm)?;
    let ball = ball_of(data, pkm)?;
    moves(data, pkm, level)?;
    if !valid(pkm.met_date) {
        return Err(Reason::Date);
    }
    let egg_location = match pkm.extra {
        Extra::Gen4 {
            egg_location_ext, ..
        } => egg_location_ext,
        Extra::Gen5 { .. } => 0,
    };
    let origin = origin::Facts {
        data,
        pkm,
        level: level as u8,
        location,
        ball,
    };
    if pkm.egg_location == 0 && egg_location == 0 {
        if pkm.egg_date != Date::default() {
            return Err(Reason::Date);
        }
        origin.wild()
    } else {
        if pkm.egg_location != DAY_CARE || egg_location != DAY_CARE {
            return Err(Reason::EggLocation);
        }
        if !valid(pkm.egg_date) || date_key(pkm.egg_date) > date_key(pkm.met_date) {
            return Err(Reason::Date);
        }
        origin.hatched()
    }
}

pub fn name_limits(language: Language) -> (usize, usize) {
    match language {
        Language::Japanese | Language::Korean => (5, 5),
        _ => (10, 7),
    }
}

fn text_len(units: &[u16]) -> usize {
    units.iter().take_while(|&&u| u != 0xFFFF).count()
}

fn names(pkm: &Pokemon) -> Result<(), Reason> {
    if matches!(pkm.language, Language::Other(_)) {
        return Err(Reason::Language);
    }
    let (nickname, ot) = name_limits(pkm.language);
    let len = text_len(&pkm.nickname);
    if len > nickname {
        return Err(Reason::NicknameTooLong { len, max: nickname });
    }
    let len = text_len(&pkm.ot_name);
    if len == 0 {
        return Err(Reason::TrainerNameEmpty);
    }
    if len > ot {
        return Err(Reason::TrainerNameTooLong { len, max: ot });
    }
    Ok(())
}

fn level_of(data: &GameData, base: &BaseStats, exp: u32) -> Result<usize, Reason> {
    let curve = data
        .tables
        .growth
        .get(usize::from(base.growth))
        .ok_or(Reason::Experience)?;
    if curve.get(MAX_LEVEL).is_none_or(|&max| exp > max) {
        return Err(Reason::Experience);
    }
    Ok((1..=MAX_LEVEL)
        .rev()
        .find(|&l| curve[l] <= exp)
        .unwrap_or(1))
}

pub fn gender_of(pid: u32, ratio: u8) -> Gender {
    match ratio {
        255 => Gender::Genderless,
        254 => Gender::Female,
        0 => Gender::Male,
        ratio if ((pid & 0xFF) as u8) < ratio => Gender::Female,
        _ => Gender::Male,
    }
}

pub fn met_location_fields(location: u16) -> (u16, u16) {
    let dp = if location <= LAST_DP_LOCATION || location >= DAY_CARE {
        location
    } else {
        FARAWAY_PLACE
    };
    (dp, location)
}

fn met_location(pkm: &Pokemon) -> Result<u16, Reason> {
    let Extra::Gen4 {
        met_location_ext, ..
    } = pkm.extra
    else {
        return Err(Reason::Generation);
    };
    if met_location_fields(met_location_ext) != (pkm.met_location, met_location_ext) {
        return Err(Reason::MetLocation);
    }
    Ok(met_location_ext)
}

pub fn ball_fields(game_is_hgss: bool, ball: u8) -> (u8, u8) {
    match game_is_hgss {
        true if ball > CHERISH_BALL => (POKE_BALL, ball),
        true => (ball, ball),
        false => (ball, 0),
    }
}

fn ball_of(data: &GameData, pkm: &Pokemon) -> Result<u8, Reason> {
    let Extra::Gen4 { hgss_ball, .. } = pkm.extra else {
        return Err(Reason::Generation);
    };
    let hgss = data::is_hgss(data.game);
    let ball = if hgss { hgss_ball } else { pkm.ball };
    if ball_fields(hgss, ball) != (pkm.ball, hgss_ball) {
        return Err(Reason::BallBytes);
    }
    Ok(ball)
}

fn moves(data: &GameData, pkm: &Pokemon, level: usize) -> Result<(), Reason> {
    let known: Vec<u16> = pkm.moves.iter().copied().filter(|&m| m != 0).collect();
    let mut sorted = known.clone();
    sorted.sort_unstable();
    sorted.dedup();
    if known.is_empty() || sorted.len() != known.len() || pkm.moves[..known.len()] != known[..] {
        return Err(Reason::Moves);
    }
    let chain = origin::family(data, pkm.species);
    for &m in &known {
        let learned = chain.iter().any(|&s| {
            data.tables.learnsets.get(usize::from(s)).is_some_and(|l| {
                l.iter()
                    .any(|lm| lm.move_id == m && usize::from(lm.level) <= level)
            })
        });
        if !learned {
            return Err(Reason::Move(m));
        }
    }
    for i in 0..4 {
        let base = data
            .tables
            .pp
            .get(usize::from(pkm.moves[i]))
            .copied()
            .unwrap_or(0);
        let ups = pkm.pp_ups[i];
        let max = u16::from(base) * (5 + u16::from(ups.min(3))) / 5;
        let empty = pkm.moves[i] == 0 && (pkm.pp[i] != 0 || ups != 0);
        if empty || ups > 3 || u16::from(pkm.pp[i]) > max {
            return Err(Reason::Pp(i));
        }
    }
    Ok(())
}

fn valid(date: Date) -> bool {
    let days = match date.month {
        2 if date.year.is_multiple_of(4) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1..=12 => 31,
        _ => return false,
    };
    (1..=days).contains(&date.day) && date.year <= 99
}

fn date_key(date: Date) -> (u8, u8, u8) {
    (date.year, date.month, date.day)
}

fn form_allowed(data: &GameData, species: u16, form: u8, location: Option<u16>) -> bool {
    match species {
        UNOWN => form < UNOWN_FORMS,
        SHELLOS | GASTRODON if !data::is_hgss(data.game) => match location {
            Some(l) => form == u8::from(origin::EAST_SEA.contains(&l)),
            None => form <= 1,
        },
        _ => form == 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pkm::Gen;
    use crate::rng::{self, Lead, LeadOutcome};
    use crate::stats::Stats;

    const PLATINUM: u8 = 12;
    const HEART_GOLD: u8 = 7;
    const SOUL_SILVER: u8 = 8;

    fn base(version: u8, species: u16) -> BaseStats {
        tables(version).unwrap().base_stats[usize::from(species)]
    }

    fn dress(version: u8, slot: &Slot, location: u16, level: u8, spread: rng::Spread) -> Pokemon {
        let t = tables(version).unwrap();
        let species = slot.encounter.species;
        let b = base(version, species);
        let mut pkm = Pokemon::new(Gen::Four);
        pkm.species = species;
        pkm.pid = spread.pid;
        pkm.ivs = spread.ivs;
        pkm.tid = 12345;
        pkm.sid = 54321;
        pkm.version = version;
        pkm.language = Language::English;
        pkm.gender = gender_of(pkm.pid, b.gender_ratio);
        pkm.ability = match b.abilities[(pkm.pid & 1) as usize] {
            0 => b.abilities[0],
            a => a,
        };
        pkm.exp = t.growth[usize::from(b.growth)][usize::from(level)];
        pkm.friendship = b.base_friendship;
        let mut learned: Vec<u16> = Vec::new();
        for m in &t.learnsets[usize::from(species)] {
            if m.level <= level && !learned.contains(&m.move_id) {
                learned.push(m.move_id);
            }
        }
        for (i, &m) in learned.iter().rev().take(4).rev().enumerate() {
            pkm.moves[i] = m;
            pkm.pp[i] = t.pp[usize::from(m)];
        }
        pkm.set_nickname("NAME").unwrap();
        pkm.set_ot("Lucas").unwrap();
        pkm.met_level = level;
        pkm.met_date = Date {
            year: 9,
            month: 3,
            day: 14,
        };
        let (dp, ext) = met_location_fields(location);
        pkm.met_location = dp;
        let hgss = matches!(version, HEART_GOLD | SOUL_SILVER);
        let ball = balls(version, slot, location)[0];
        let (dp_ball, hgss_ball) = ball_fields(hgss, ball);
        pkm.ball = dp_ball;
        pkm.ground_tile = slot.tile().unwrap_or(0);
        pkm.extra = Extra::Gen4 {
            shiny_leaves: 0,
            egg_location_ext: 0,
            met_location_ext: ext,
            hgss_ball,
            walking_mood: 0,
        };
        pkm
    }

    fn wild(version: u8, kind: Kind, lead: Lead) -> (Pokemon, LeadOutcome) {
        wild_where(version, kind, lead, |_, _| true)
    }

    fn wild_where(
        version: u8,
        kind: Kind,
        lead: Lead,
        keep: impl Fn(u16, &Slot) -> bool,
    ) -> (Pokemon, LeadOutcome) {
        let method = method(version).unwrap();
        for (location, slots) in slots(version) {
            for slot in slots.iter().filter(|s| {
                s.kind == kind
                    && !s.block
                    && ![UNOWN, SHELLOS, GASTRODON].contains(&s.encounter.species)
                    && keep(location, s)
            }) {
                let ratio = base(version, slot.encounter.species).gender_ratio;
                for seed in 0..3000u32 {
                    let roll = rng::wild(
                        method,
                        seed.wrapping_mul(0x2545_F491),
                        calls(kind),
                        lead,
                        ratio,
                    );
                    let e = slot.encounter;
                    let level = match roll.level {
                        Some(out) => rolled_level(out, e.min_level, e.max_level),
                        None => e.min_level,
                    };
                    let wanted = match lead {
                        Lead::None => LeadOutcome::None,
                        Lead::Synchronize(_) => LeadOutcome::SyncPass,
                        Lead::CuteCharm { .. } => LeadOutcome::CuteCharmPass,
                    };
                    let shiny = crate::pkm::is_shiny(roll.spread.pid, 12345, 54321);
                    if roll.lead == wanted && !shiny && fits(method, slot, level, &roll) {
                        return (
                            dress(version, slot, location, level, roll.spread),
                            roll.lead,
                        );
                    }
                }
            }
        }
        panic!("no {kind:?} slot in version {version}");
    }

    #[test]
    fn wild_pokemon_from_every_kind_of_slot_pass() {
        for (version, kind) in [
            (PLATINUM, Kind::Grass),
            (PLATINUM, Kind::Surf),
            (PLATINUM, Kind::Rod(Rod::Old)),
            (PLATINUM, Kind::Rod(Rod::Super)),
            (HEART_GOLD, Kind::Grass),
            (HEART_GOLD, Kind::Surf),
            (HEART_GOLD, Kind::SafariGrass),
            (HEART_GOLD, Kind::BugContest),
            (SOUL_SILVER, Kind::Grass),
            (SOUL_SILVER, Kind::Rod(Rod::Good)),
            (SOUL_SILVER, Kind::SafariGrass),
            (SOUL_SILVER, Kind::BugContest),
        ] {
            let (pkm, _) = wild(version, kind, Lead::None);
            assert_eq!(check(&pkm), Ok(()), "{version} {kind:?}");
        }
    }

    #[test]
    fn synchronize_and_cute_charm_leads_pass() {
        for version in [PLATINUM, SOUL_SILVER] {
            let (pkm, outcome) = wild(version, Kind::Grass, Lead::Synchronize(11));
            assert_eq!(
                (check(&pkm), outcome, pkm.nature()),
                (Ok(()), LeadOutcome::SyncPass, 11)
            );
            let (pkm, outcome) = wild(version, Kind::Grass, Lead::CuteCharm { female: true });
            assert_eq!((check(&pkm), outcome), (Ok(()), LeadOutcome::CuteCharmPass));
            assert!(pkm.pid < 25);
        }
    }

    #[test]
    fn cute_charm_is_refused_where_the_game_rerolls_for_a_perfect_iv() {
        let (pkm, _) = wild(SOUL_SILVER, Kind::SafariGrass, Lead::None);
        let slots = slots(SOUL_SILVER);
        let slot = slots
            .iter()
            .flat_map(|(_, s)| s.iter())
            .find(|s| s.kind == Kind::SafariGrass)
            .unwrap();
        let roll = rng::WildRoll {
            seed: 0,
            before_slot: [0; 2],
            slot: u16::from(slot.encounter.slot),
            level: None,
            lead: LeadOutcome::CuteCharmPass,
            spread: rng::Spread {
                pid: pkm.pid,
                ivs: pkm.ivs,
            },
        };
        let method = method(SOUL_SILVER).unwrap();
        assert!(!fits(method, slot, slot.encounter.min_level, &roll));
        let plain = rng::WildRoll {
            lead: LeadOutcome::None,
            ..roll
        };
        assert!(fits(method, slot, slot.encounter.min_level, &plain));
    }

    #[test]
    fn a_wrong_ball_is_refused() {
        let (mut pkm, _) = wild(PLATINUM, Kind::Grass, Lead::None);
        pkm.ball = SAFARI_BALL;
        assert_eq!(check(&pkm), Err(Reason::Ball(SAFARI_BALL)));
        let (mut pkm, _) = wild(SOUL_SILVER, Kind::SafariGrass, Lead::None);
        pkm.ball = POKE_BALL;
        if let Extra::Gen4 { hgss_ball, .. } = &mut pkm.extra {
            *hgss_ball = POKE_BALL;
        }
        assert_eq!(check(&pkm), Err(Reason::Ball(POKE_BALL)));
    }

    #[test]
    fn the_hgss_ball_byte_must_match() {
        let (mut pkm, _) = wild(SOUL_SILVER, Kind::Grass, Lead::None);
        if let Extra::Gen4 { hgss_ball, .. } = &mut pkm.extra {
            *hgss_ball = 0;
        }
        assert_eq!(check(&pkm), Err(Reason::BallBytes));
    }

    #[test]
    fn a_level_the_slot_never_gives_is_refused() {
        let (mut pkm, _) = wild(PLATINUM, Kind::Grass, Lead::None);
        pkm.met_level = 99;
        assert_eq!(check(&pkm), Err(Reason::MetLevel(99)));
        let (mut pkm, _) = wild(PLATINUM, Kind::Grass, Lead::None);
        pkm.met_level -= 1;
        let t = tables(PLATINUM).unwrap();
        pkm.exp =
            t.growth[usize::from(base(PLATINUM, pkm.species).growth)][usize::from(pkm.met_level)];
        assert!(matches!(
            check(&pkm),
            Err(Reason::NoEncounter(_) | Reason::Correlation)
        ));
    }

    #[test]
    fn a_move_it_cant_learn_is_refused() {
        let (mut pkm, _) = wild(PLATINUM, Kind::Grass, Lead::None);
        pkm.moves[0] = 460;
        pkm.pp[0] = 5;
        assert_eq!(check(&pkm), Err(Reason::Move(460)));
    }

    #[test]
    fn names_have_a_length_per_language() {
        let (mut pkm, _) = wild(PLATINUM, Kind::Grass, Lead::None);
        pkm.set_nickname("ABCDEFGHIJ").unwrap();
        assert_eq!(check(&pkm), Ok(()));
        pkm.language = Language::Japanese;
        pkm.set_nickname("アイウエオカ").unwrap();
        assert_eq!(check(&pkm), Err(Reason::NicknameTooLong { len: 6, max: 5 }));
        pkm.set_nickname("アイウエオ").unwrap();
        pkm.set_ot("ヒカリヒカリ").unwrap();
        assert_eq!(
            check(&pkm),
            Err(Reason::TrainerNameTooLong { len: 6, max: 5 })
        );
    }

    #[test]
    fn pid_and_ivs_from_no_call_order_are_refused() {
        let (mut pkm, _) = wild(PLATINUM, Kind::Grass, Lead::None);
        pkm.ivs = Stats::splat(31);
        assert_eq!(check(&pkm), Err(Reason::Correlation));
    }

    #[test]
    fn places_outside_diamond_and_pearl_use_faraway_place() {
        assert_eq!(met_location_fields(16), (16, 16));
        assert_eq!(met_location_fields(120), (FARAWAY_PLACE, 120));
        let (mut pkm, _) = wild(SOUL_SILVER, Kind::Grass, Lead::None);
        assert_eq!(pkm.met_location, FARAWAY_PLACE);
        pkm.met_location = 16;
        assert_eq!(check(&pkm), Err(Reason::MetLocation));
    }

    fn hatched(version: u8, species: u16) -> Pokemon {
        let slot = Slot {
            kind: Kind::Grass,
            block: false,
            encounter: crate::tables::Encounter {
                place: 0,
                season: None,
                method: crate::tables::EncounterMethod::Grass,
                slot: 0,
                species,
                form: 0,
                min_level: 1,
                max_level: 1,
            },
            tiles: 1,
        };
        let place = hatch_places(version)[3];
        let mut pkm = dress(
            version,
            &slot,
            place,
            1,
            rng::Spread {
                pid: 0x1234_5678,
                ivs: Stats::splat(10),
            },
        );
        pkm.met_level = 0;
        let (dp_ball, hgss_ball_byte) = ball_fields(version == SOUL_SILVER, POKE_BALL);
        pkm.ball = dp_ball;
        pkm.egg_location = DAY_CARE;
        pkm.egg_date = pkm.met_date;
        if let Extra::Gen4 {
            egg_location_ext,
            hgss_ball,
            ..
        } = &mut pkm.extra
        {
            *egg_location_ext = DAY_CARE;
            *hgss_ball = hgss_ball_byte;
        }
        pkm
    }

    #[test]
    fn hatched_pokemon_pass_and_follow_the_egg_rules() {
        for version in [PLATINUM, SOUL_SILVER] {
            assert_eq!(check(&hatched(version, 1)), Ok(()));
            assert_eq!(check(&hatched(version, 172)), Ok(()));
            assert_eq!(check(&hatched(version, 489)), Ok(()));
            assert_eq!(check(&hatched(version, 132)), Err(Reason::NotBreedable));
            assert_eq!(check(&hatched(version, 490)), Err(Reason::NotBreedable));
            let mut nidoran = hatched(version, 29);
            assert_eq!(check(&nidoran), Ok(()));
            nidoran.pid |= 0x8000;
            assert_eq!(check(&nidoran), Err(Reason::Gender));
            assert_eq!(check(&hatched(version, 2)), Err(Reason::NotBreedable));
            assert_eq!(check(&hatched(version, 150)), Err(Reason::NotBreedable));
            let mut late = hatched(version, 1);
            late.egg_date.year += 1;
            assert_eq!(check(&late), Err(Reason::Date));
        }
    }

    #[test]
    fn korean_platinum_eggs_never_hatch_at_two_places() {
        let mut korean = hatched(PLATINUM, 1);
        korean.language = Language::Korean;
        korean.set_nickname("이상해씨").unwrap();
        korean.set_ot("민수").unwrap();
        assert_eq!(check(&korean), Ok(()));
        let (dp, ext) = met_location_fields(85);
        korean.met_location = dp;
        if let Extra::Gen4 {
            met_location_ext, ..
        } = &mut korean.extra
        {
            *met_location_ext = ext;
        }
        assert_eq!(check(&korean), Err(Reason::MetLocation));
        korean.language = Language::English;
        korean.set_nickname("BULBASAUR").unwrap();
        korean.set_ot("Lucas").unwrap();
        assert_eq!(check(&korean), Ok(()));
    }

    #[test]
    fn a_heartgold_only_wild_pokemon_passes_from_heartgold_and_not_from_soulsilver() {
        let soul_silver = slots(SOUL_SILVER);
        let (mut pkm, _) = wild_where(HEART_GOLD, Kind::Grass, Lead::None, |location, slot| {
            !soul_silver.iter().any(|(place, slots)| {
                *place == location
                    && slots
                        .iter()
                        .any(|s| s.encounter.species == slot.encounter.species)
            })
        });
        assert_eq!(check(&pkm), Ok(()));
        pkm.version = SOUL_SILVER;
        assert!(matches!(check(&pkm), Err(Reason::NoEncounter(_))));
    }

    #[test]
    fn other_games_and_generations_are_not_checked() {
        let (mut pkm, _) = wild(PLATINUM, Kind::Grass, Lead::None);
        pkm.version = 10;
        assert_eq!(check(&pkm), Err(Reason::Version(10)));
        assert_eq!(check(&Pokemon::new(Gen::Five)), Err(Reason::Generation));
    }
}
