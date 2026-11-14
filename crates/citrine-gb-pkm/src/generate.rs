use crate::legality::{self, Kind, Slot};
use crate::pkm::{Date, Extra, Gen, Language, Party, Pokemon, is_shiny};
use crate::rng::{self, Lead};
use crate::tables::{BaseStats, Tables};
use crate::{Stats, names};

const POKE_BALL: u8 = 4;
const DAY_CARE: u16 = 2000;
const PLATINUM: u8 = 12;
const UNOWN: u16 = 201;
const SHEDINJA: u16 = 292;
const SEED_TRIES: usize = 4000;
const MOST_EVS: u32 = 510;
const MOST_EVS_EACH: u32 = 252;

#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }

    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trainer {
    pub version: u8,
    pub language: Language,
    pub name: String,
    pub female: bool,
    pub tid: u16,
    pub sid: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Catch {
    pub location: u16,
    pub slot: Slot,
}

impl Catch {
    pub fn species(&self) -> u16 {
        self.slot.encounter.species
    }

    pub fn min_level(&self) -> u8 {
        self.slot.encounter.min_level
    }

    pub fn max_level(&self) -> u8 {
        self.slot.encounter.max_level
    }
}

pub fn catches(version: u8) -> Vec<Catch> {
    legality::slots(version)
        .into_iter()
        .flat_map(|(location, slots)| {
            slots
                .iter()
                .filter(|s| {
                    !s.block
                        && s.kind != Kind::RockSmash
                        && s.tile().is_some()
                        && s.encounter.species != UNOWN
                })
                .map(move |&slot| Catch { location, slot })
        })
        .collect()
}

pub fn weight(kind: Kind) -> u64 {
    match kind {
        Kind::Grass => 6,
        Kind::Surf => 2,
        _ => 1,
    }
}

pub fn evolved(version: u8, species: u16, from: u8, to: u8) -> u16 {
    let Some(tables) = legality::tables(version) else {
        return species;
    };
    (from + 1..=to).fold(species, |s, l| level_evolution(tables, s, l).unwrap_or(s))
}

pub fn egg_species(version: u8) -> Vec<u16> {
    let Some(tables) = legality::tables(version) else {
        return Vec::new();
    };
    (1..=493u16)
        .filter(|&s| {
            let b = base(tables, s);
            !b.egg_groups.contains(&15)
                && !b.egg_groups.contains(&13)
                && s != 490
                && !tables
                    .evolutions
                    .iter()
                    .any(|evos| evos.iter().any(|e| e.into == s))
                && tables.learnsets[usize::from(s)]
                    .iter()
                    .any(|m| m.level == 1)
        })
        .collect()
}

pub fn wild(
    trainer: &Trainer,
    rng: &mut Rng,
    catch: &Catch,
    grow_to: Option<u8>,
    met: Date,
) -> Option<Pokemon> {
    let tables = legality::tables(trainer.version)?;
    let method = legality::method(trainer.version)?;
    let slot = catch.slot;
    let e = slot.encounter;
    let b = base(tables, e.species);
    let calls = legality::calls(slot.kind);
    let lead = match rng.below(10) {
        0..=5 => Lead::None,
        6..=8 => Lead::Synchronize(rng.below(25) as u8),
        _ if !matches!(b.gender_ratio, 0 | 254 | 255) && !calls.until_perfect_iv => {
            Lead::CuteCharm {
                female: rng.chance(50),
            }
        }
        _ => Lead::None,
    };
    let (roll, met_level) = (0..SEED_TRIES).find_map(|_| {
        let roll = rng::wild(method, rng.next_u64() as u32, calls, lead, b.gender_ratio);
        let level = match roll.level {
            Some(out) => legality::rolled_level(out, e.min_level, e.max_level),
            None => e.min_level,
        };
        (legality::fits(method, &slot, level, &roll)
            && !is_shiny(roll.spread.pid, trainer.tid, trainer.sid))
        .then_some((roll, level))
    })?;
    let mut pkm = Pokemon::new(Gen::Four);
    pkm.pid = roll.spread.pid;
    pkm.ivs = roll.spread.ivs;
    pkm.form = e.form;
    if matches!(e.species, 422 | 423) && trainer.version == PLATINUM {
        pkm.form = u8::from(legality::EAST_SEA.contains(&catch.location));
    }
    let level = grow_to.unwrap_or(met_level).max(met_level).min(100);
    let mut species = e.species;
    let mut history = vec![(species, met_level)];
    for l in met_level + 1..=level {
        if let Some(into) = level_evolution(tables, species, l) {
            species = into;
        }
        history.push((species, l));
    }
    if level > met_level {
        let mut left = MOST_EVS.min(u32::from(level - met_level) * 20);
        let mut evs = [0u8; 6];
        for ev in &mut evs {
            let give = rng.below(u64::from(left.min(MOST_EVS_EACH)) + 1) as u32;
            *ev = give as u8;
            left -= give;
        }
        pkm.evs = Stats::from_array(evs);
    }
    dress(tables, &mut pkm, trainer, species, level, &history)?;
    let balls = legality::balls(trainer.version, &slot, catch.location);
    let ball = if balls.len() == 1 || rng.chance(60) && balls.contains(&POKE_BALL) {
        if balls.contains(&POKE_BALL) {
            POKE_BALL
        } else {
            *balls.first()?
        }
    } else {
        *rng.pick(&balls)
    };
    place(
        &mut pkm,
        is_hgss(trainer.version),
        catch.location,
        ball,
        None,
    );
    pkm.met_level = met_level;
    pkm.ground_tile = slot.tile()?;
    pkm.met_date = met;
    Some(pkm)
}

pub fn hatched(
    trainer: &Trainer,
    rng: &mut Rng,
    species: u16,
    level: u8,
    laid: Date,
    hatched_on: Date,
) -> Option<Pokemon> {
    let tables = legality::tables(trainer.version)?;
    let mut pkm = Pokemon::new(Gen::Four);
    pkm.pid = rng.next_u64() as u32;
    if matches!(species, 29 | 314) {
        pkm.pid &= !0x8000;
    }
    if is_shiny(pkm.pid, trainer.tid, trainer.sid) {
        return None;
    }
    pkm.ivs = Stats::from_array(std::array::from_fn(|_| rng.below(32) as u8));
    let level = level.clamp(1, 100);
    let mut current = species;
    let mut history = vec![(species, 1)];
    for l in 2..=level {
        if let Some(into) = level_evolution(tables, current, l) {
            current = into;
        }
        history.push((current, l));
    }
    if species == 422 && trainer.version == PLATINUM {
        pkm.form = rng.below(2) as u8;
    }
    dress(tables, &mut pkm, trainer, current, level, &history)?;
    pkm.friendship = 120;
    let places = legality::hatch_places(trainer.version);
    let &hatch_place = rng.pick(&places);
    if trainer.version == PLATINUM
        && trainer.language == Language::Korean
        && matches!(hatch_place, 63 | 85)
    {
        return None;
    }
    place(
        &mut pkm,
        is_hgss(trainer.version),
        hatch_place,
        POKE_BALL,
        Some(DAY_CARE),
    );
    pkm.met_level = 0;
    pkm.egg_date = laid;
    pkm.met_date = hatched_on;
    Some(pkm)
}

pub fn party(version: u8, pkm: &Pokemon) -> Option<Party> {
    let tables = legality::tables(version)?;
    let b = base(tables, pkm.species);
    let curve = tables.growth.get(usize::from(b.growth))?;
    let level = (1..=100u8)
        .rev()
        .find(|&l| curve[usize::from(l)] <= pkm.exp)
        .unwrap_or(1);
    let nature = usize::from(pkm.nature());
    let (raised, lowered) = (1 + nature / 5, 1 + nature % 5);
    let bases = b.stats.to_array();
    let ivs = pkm.ivs.to_array();
    let evs = pkm.evs.to_array();
    let stats: [u16; 6] = std::array::from_fn(|i| {
        let core = (2 * u32::from(bases[i]) + u32::from(ivs[i]) + u32::from(evs[i]) / 4)
            * u32::from(level)
            / 100;
        if i == 0 {
            return if pkm.species == SHEDINJA {
                1
            } else {
                (core + u32::from(level) + 10) as u16
            };
        }
        let plain = core + 5;
        let value = if raised == lowered {
            plain
        } else if i == raised {
            plain * 110 / 100
        } else if i == lowered {
            plain * 90 / 100
        } else {
            plain
        };
        value as u16
    });
    let stats = Stats::from_array(stats);
    Some(Party {
        level,
        hp: stats.hp,
        stats,
        ..Party::default()
    })
}

fn is_hgss(version: u8) -> bool {
    matches!(version, 7 | 8)
}

fn base(tables: &Tables, species: u16) -> BaseStats {
    tables.base_stats[usize::from(species)]
}

fn level_evolution(tables: &Tables, species: u16, level: u8) -> Option<u16> {
    tables.evolutions[usize::from(species)]
        .iter()
        .find(|e| e.method == 4 && e.param <= u16::from(level))
        .map(|e| e.into)
}

fn dress(
    tables: &Tables,
    pkm: &mut Pokemon,
    trainer: &Trainer,
    species: u16,
    level: u8,
    history: &[(u16, u8)],
) -> Option<()> {
    let b = base(tables, species);
    pkm.species = species;
    pkm.tid = trainer.tid;
    pkm.sid = trainer.sid;
    pkm.version = trainer.version;
    pkm.language = trainer.language;
    pkm.gender = legality::gender_of(pkm.pid, b.gender_ratio);
    pkm.ability = match b.abilities[(pkm.pid & 1) as usize] {
        0 => b.abilities[0],
        a => a,
    };
    pkm.exp = tables.growth[usize::from(b.growth)][usize::from(level)];
    let mut moves: Vec<u16> = Vec::new();
    let first = history.first().copied();
    for &(at_species, at_level) in history {
        for m in &tables.learnsets[usize::from(at_species)] {
            let learn = if first == Some((at_species, at_level)) {
                m.level <= at_level
            } else {
                m.level == at_level
            };
            if learn && m.level > 0 && !moves.contains(&m.move_id) {
                moves.push(m.move_id);
                if moves.len() > 4 {
                    moves.remove(0);
                }
            }
        }
    }
    if moves.is_empty() {
        return None;
    }
    for (i, &m) in moves.iter().enumerate() {
        pkm.moves[i] = m;
        pkm.pp[i] = tables.pp[usize::from(m)];
    }
    let name = names::species(trainer.language, species)?;
    pkm.set_nickname(&name).ok()?;
    pkm.set_ot(&trainer.name).ok()?;
    pkm.ot_female = trainer.female;
    pkm.friendship = b.base_friendship;
    Some(())
}

fn place(pkm: &mut Pokemon, hgss: bool, location: u16, ball: u8, egg: Option<u16>) {
    let (dp, ext) = legality::met_location_fields(location);
    pkm.met_location = dp;
    let (dp_ball, hgss_ball) = legality::ball_fields(hgss, ball);
    pkm.ball = dp_ball;
    pkm.egg_location = egg.map_or(0, |e| legality::met_location_fields(e).0);
    pkm.extra = Extra::Gen4 {
        shiny_leaves: 0,
        egg_location_ext: egg.unwrap_or(0),
        met_location_ext: ext,
        hgss_ball,
        walking_mood: 0,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trainer(version: u8) -> Trainer {
        Trainer {
            version,
            language: Language::English,
            name: String::from("Lucas"),
            female: false,
            tid: 12345,
            sid: 54321,
        }
    }

    const MET: Date = Date {
        year: 11,
        month: 3,
        day: 14,
    };

    #[test]
    fn wild_catches_pass_the_checker_in_every_game() {
        for version in [7, 8, 12] {
            let mut rng = Rng::new(u64::from(version) * 7919);
            let all = catches(version);
            assert!(!all.is_empty());
            let mut made = 0;
            for _ in 0..300 {
                let catch = rng.pick(&all);
                let grow = rng.chance(30).then(|| catch.max_level() + 5);
                let Some(pkm) = wild(&trainer(version), &mut rng, catch, grow, MET) else {
                    continue;
                };
                assert_eq!(legality::check(&pkm), Ok(()), "{version} {catch:?}");
                made += 1;
            }
            assert!(made > 200, "{version}: {made}");
        }
    }

    #[test]
    fn hatched_eggs_pass_the_checker() {
        for version in [8, 12] {
            let mut rng = Rng::new(99);
            let eggs = egg_species(version);
            let laid = Date { day: 2, ..MET };
            let mut made = 0;
            for _ in 0..200 {
                let species = *rng.pick(&eggs);
                if let Some(pkm) = hatched(&trainer(version), &mut rng, species, 1, laid, MET) {
                    assert_eq!(legality::check(&pkm), Ok(()), "{version} {species}");
                    made += 1;
                }
            }
            assert!(made > 150, "{version}: {made}");
        }
    }

    #[test]
    fn party_stats_follow_the_formula() {
        let mut pkm = Pokemon::new(Gen::Four);
        pkm.species = 387;
        pkm.pid = 0;
        pkm.ivs = Stats::splat(31);
        let tables = legality::tables(12).unwrap();
        pkm.exp = tables.growth[usize::from(base(tables, 387).growth)][50];
        let party = party(12, &pkm).unwrap();
        assert_eq!(party.level, 50);
        assert_eq!(party.stats.hp, 130);
        assert_eq!(party.stats.atk, 88);
        assert_eq!(party.hp, party.stats.hp);
    }

    #[test]
    fn growing_follows_level_evolutions() {
        assert_eq!(evolved(12, 387, 5, 18), 388);
        assert_eq!(evolved(12, 387, 5, 17), 387);
    }
}
