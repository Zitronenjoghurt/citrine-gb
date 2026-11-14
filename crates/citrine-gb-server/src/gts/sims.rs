mod names;
mod places;

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use citrine_gb_pkm::generate::{self, Catch, Rng, Trainer};
use citrine_gb_pkm::legality::{self, Kind};
use citrine_gb_pkm::pkm::{Date, Gender};
use citrine_gb_pkm::tables::EncounterMethod;
use citrine_gb_pkm::{Language, Pokemon};

use super::crypt::Generation;
use super::record::{EITHER, Record, Trailer, Wanted, date_of};
use crate::db::{Db, GtsListing, GtsRow, Result, SimRow};

const HEARTGOLD: u8 = 7;
const SOULSILVER: u8 = 8;
const PLATINUM: u8 = 12;
const VERSIONS: [u8; 3] = [PLATINUM, HEARTGOLD, SOULSILVER];
const GENERATION: i64 = 4;

const HOUR: u64 = 3600;
const DAY: u64 = 86_400;
const PER_PLAYER: i64 = 3;
const MOST_OPEN: i64 = 60;
const ACTIVE_FOR: u64 = 7 * DAY;
const GIVE_UP_AFTER: u64 = 10 * DAY;
const RETHINK_AFTER: u64 = 2 * DAY;
const RETHINK_CHANCE: u64 = 20;
const BACKDATE_UP_TO: u64 = 36 * HOUR;
const TAKE_AFTER: u64 = 3 * HOUR;
const TAKE_SPREAD: u64 = 9 * HOUR;
const TAKE_CHANCE: u64 = 10;
const MOST_TAKES: usize = 2;
const DEPOSIT_CHANCE: u64 = 65;
const FAIR: i32 = 10;
const FIRST_CAP: u32 = 12;
const CAP_PER_DAY: u32 = 3;
const LAST_CAP: u32 = 70;
const EVENING: u64 = 18;
const EVENING_HOURS: u64 = 5;
const MAKE_TRIES: usize = 24;

const STARTERS: [u16; 12] = [1, 4, 7, 152, 155, 158, 252, 255, 258, 387, 390, 393];
const SINNOH_STARTERS: [u16; 3] = [387, 390, 393];
const JOHTO_STARTERS: [u16; 3] = [152, 155, 158];

const LANGUAGES: [(Language, u64); 7] = [
    (Language::Japanese, 30),
    (Language::English, 34),
    (Language::French, 8),
    (Language::German, 8),
    (Language::Spanish, 7),
    (Language::Italian, 6),
    (Language::Korean, 7),
];

pub fn legendary(species: u16) -> bool {
    matches!(
        species,
        144..=146 | 150 | 151 | 243..=245 | 249..=251 | 377..=386 | 480..=493
    )
}

pub fn cap(active_players: i64) -> i64 {
    (active_players * PER_PLAYER).min(MOST_OPEN)
}

struct Sim {
    row: SimRow,
    language: Language,
    version: u8,
}

impl Sim {
    fn new(row: SimRow) -> Self {
        Self {
            language: Language::from_byte(row.language as u8),
            version: row.version as u8,
            row,
        }
    }

    fn trainer(&self) -> Trainer {
        Trainer {
            version: self.version,
            language: self.language,
            name: self.row.name.clone(),
            female: self.row.female,
            tid: self.row.tid as u16,
            sid: self.row.sid as u16,
        }
    }

    fn trailer(&self) -> Trailer<'_> {
        Trailer {
            name: &self.row.name,
            language: self.language,
            female: self.row.female,
            tid: self.row.tid as u16,
            country: self.row.country as u8,
            region: self.row.region as u8,
            avatar: self.row.avatar as u8,
            version: self.version,
        }
    }

    fn cap(&self) -> u8 {
        (FIRST_CAP + CAP_PER_DAY * self.row.days as u32).min(LAST_CAP) as u8
    }

    fn history_day(&self, rng: &mut Rng, now: u64) -> u64 {
        let started = self.row.started_at.min(now);
        let span = (now - started) / DAY;
        started + rng.below(span + 1) * DAY + rng.below(DAY)
    }

    fn next_evening(&self, rng: &mut Rng, now: u64) -> u64 {
        let offset = self.row.utc_offset * HOUR as i64;
        let local = now.saturating_add_signed(offset);
        let day = local / DAY + 1 + u64::from(rng.chance(40));
        let at = day * DAY + (EVENING + rng.below(EVENING_HOURS)) * HOUR + rng.below(HOUR);
        at.saturating_add_signed(-offset).max(now + HOUR)
    }
}

fn slot_rate(kind: Kind, method: EncounterMethod, slot: u8) -> u64 {
    const GRASS: [u64; 12] = [20, 20, 10, 10, 10, 10, 5, 5, 4, 4, 1, 1];
    const WATER: [u64; 5] = [60, 30, 5, 4, 1];
    let base = match kind {
        Kind::Grass | Kind::SafariGrass | Kind::BugContest => GRASS.get(usize::from(slot)),
        Kind::Surf | Kind::Rod(_) | Kind::SafariSurf | Kind::SafariRod(_) => {
            WATER.get(usize::from(slot))
        }
        Kind::RockSmash => None,
    };
    let rate = base.copied().unwrap_or(10);
    if special(method) {
        rate.div_ceil(4)
    } else {
        rate
    }
}

fn special(method: EncounterMethod) -> bool {
    matches!(
        method,
        EncounterMethod::Swarm
            | EncounterMethod::Radar
            | EncounterMethod::DualSlot(_)
            | EncounterMethod::HoennSound
            | EncounterMethod::SinnohSound
            | EncounterMethod::SurfSwarm
            | EncounterMethod::FishingSwarm
    )
}

fn catch_rate(catch: &Catch) -> u64 {
    slot_rate(
        catch.slot.kind,
        catch.slot.encounter.method,
        catch.slot.encounter.slot,
    )
}

fn rarities() -> &'static HashMap<u16, i32> {
    static RARITY: OnceLock<HashMap<u16, i32>> = OnceLock::new();
    RARITY.get_or_init(|| {
        let mut rarity: HashMap<u16, i32> = HashMap::new();
        for version in VERSIONS {
            for catch in generate::catches(version) {
                let rate = catch_rate(&catch);
                let mut r = match rate {
                    20.. => 1,
                    10.. => 2,
                    5.. => 3,
                    4.. => 4,
                    _ => 5,
                };
                if catch.min_level() > 30 {
                    r += 1;
                }
                let best = rarity.entry(catch.species()).or_insert(r);
                *best = (*best).min(r);
            }
        }
        if let Some(tables) = legality::tables(PLATINUM) {
            for _ in 0..3 {
                for (from, evolutions) in tables.evolutions.iter().enumerate() {
                    let Some(&r) = rarity.get(&(from as u16)) else {
                        continue;
                    };
                    for e in evolutions {
                        rarity.entry(e.into).or_insert(r + 2);
                    }
                }
            }
        }
        rarity
    })
}

pub fn value(species: u16, level: u8) -> i32 {
    let rarity = rarities().get(&species).copied().unwrap_or(6);
    rarity * 10 + i32::from(level) / 3
}

fn catchable(version: u8) -> &'static HashSet<u16> {
    static SETS: OnceLock<HashMap<u8, HashSet<u16>>> = OnceLock::new();
    let sets = SETS.get_or_init(|| {
        VERSIONS
            .iter()
            .map(|&v| (v, generate::catches(v).iter().map(Catch::species).collect()))
            .collect()
    });
    static EMPTY: OnceLock<HashSet<u16>> = OnceLock::new();
    sets.get(&version)
        .unwrap_or_else(|| EMPTY.get_or_init(HashSet::new))
}

fn exclusives_elsewhere(version: u8) -> Vec<u16> {
    let own = catchable(version);
    let others: HashSet<u16> = match version {
        HEARTGOLD => catchable(SOULSILVER).clone(),
        SOULSILVER => catchable(HEARTGOLD).clone(),
        _ => catchable(HEARTGOLD)
            .union(catchable(SOULSILVER))
            .copied()
            .collect(),
    };
    let mut found: Vec<u16> = others.difference(own).copied().collect();
    found.sort_unstable();
    found
}

fn gender_ratio(version: u8, species: u16) -> u8 {
    legality::tables(version)
        .and_then(|t| t.base_stats.get(usize::from(species)))
        .map_or(255, |b| b.gender_ratio)
}

fn bracket(level: u8) -> (u8, u8) {
    if level <= 10 {
        (0, 10)
    } else {
        let low = (level - 1) / 10 * 10 + 1;
        (low, (low + 9).min(100))
    }
}

fn ask(sim: &Sim, rng: &mut Rng, offered: u16, level: u8) -> Wanted {
    let worth = value(offered, level);
    let cap = sim.cap();
    let mut pool: Vec<u16> = exclusives_elsewhere(sim.version);
    for catch in generate::catches(sim.version) {
        if catch.min_level() > cap {
            pool.push(catch.species());
        } else {
            let grown = generate::evolved(sim.version, catch.species(), catch.max_level(), 100);
            if grown != catch.species() {
                pool.push(grown);
            }
        }
    }
    pool.extend(STARTERS);
    pool.retain(|&s| s != offered && !legendary(s) && s <= 493);
    pool.sort_unstable();
    pool.dedup();
    let any_level = rng.chance(50);
    let (min, max) = if any_level { (0, 0) } else { bracket(level) };
    let asked_level = if any_level {
        level
    } else {
        ((u16::from(min.max(1)) + u16::from(max)) / 2) as u8
    };
    let fair: Vec<u16> = pool
        .iter()
        .copied()
        .filter(|&s| (value(s, asked_level) - worth).abs() <= FAIR)
        .collect();
    let species = if fair.is_empty() {
        pool.iter()
            .copied()
            .min_by_key(|&s| (value(s, asked_level) - worth).abs())
            .unwrap_or(STARTERS[0])
    } else {
        *rng.pick(&fair)
    };
    let gender = match gender_ratio(sim.version, species) {
        0 | 254 | 255 => EITHER,
        _ if rng.chance(80) => EITHER,
        _ => 1 + rng.below(2) as u8,
    };
    Wanted {
        species,
        gender,
        min,
        max,
    }
}

fn weighted_pick<'a>(rng: &mut Rng, pool: &'a [Catch]) -> Option<&'a Catch> {
    let total: u64 = pool
        .iter()
        .map(|c| generate::weight(c.slot.kind) * catch_rate(c))
        .sum();
    if total == 0 {
        return None;
    }
    let mut roll = rng.below(total);
    for catch in pool {
        let w = generate::weight(catch.slot.kind) * catch_rate(catch);
        if roll < w {
            return Some(catch);
        }
        roll -= w;
    }
    pool.last()
}

fn with_party(version: u8, mut pkm: Pokemon) -> Option<Pokemon> {
    pkm.party = Some(generate::party(version, &pkm)?);
    Some(pkm)
}

fn caught(sim: &Sim, rng: &mut Rng, now: u64) -> Option<Pokemon> {
    let cap = sim.cap();
    let pool: Vec<Catch> = generate::catches(sim.version)
        .into_iter()
        .filter(|c| c.max_level() <= cap && !legendary(c.species()))
        .collect();
    let trainer = sim.trainer();
    for _ in 0..MAKE_TRIES {
        let catch = weighted_pick(rng, &pool)?;
        let grow = (rng.chance(35) && catch.max_level() < cap)
            .then(|| catch.max_level() + 1 + rng.below(u64::from(cap - catch.max_level())) as u8);
        let met = date_of(sim.history_day(rng, now));
        if let Some(pkm) = generate::wild(&trainer, rng, catch, grow, met) {
            return with_party(sim.version, pkm);
        }
    }
    None
}

fn bred(sim: &Sim, rng: &mut Rng, now: u64) -> Option<Pokemon> {
    let eggs = generate::egg_species(sim.version);
    let starters: &[u16] = if sim.version == PLATINUM {
        &SINNOH_STARTERS
    } else {
        &JOHTO_STARTERS
    };
    let trainer = sim.trainer();
    for _ in 0..MAKE_TRIES {
        let species = if rng.chance(50) {
            *rng.pick(starters)
        } else {
            let own: Vec<u16> = eggs
                .iter()
                .copied()
                .filter(|s| catchable(sim.version).contains(s))
                .collect();
            *rng.pick(if own.is_empty() { &eggs } else { &own })
        };
        if !eggs.contains(&species) {
            continue;
        }
        let level = if rng.chance(70) {
            1
        } else {
            1 + rng.below(u64::from(sim.cap().min(20))) as u8
        };
        let hatched_at = sim.history_day(rng, now);
        let laid = date_of(hatched_at.saturating_sub((1 + rng.below(3)) * DAY));
        let hatched_on = date_of(hatched_at);
        if !ordered(laid, hatched_on) {
            continue;
        }
        if let Some(pkm) = generate::hatched(&trainer, rng, species, level, laid, hatched_on) {
            return with_party(sim.version, pkm);
        }
    }
    None
}

fn ordered(a: Date, b: Date) -> bool {
    (a.year, a.month, a.day) <= (b.year, b.month, b.day)
}

fn offer(sim: &Sim, rng: &mut Rng, now: u64) -> Option<Record> {
    let pkm = if rng.chance(80) {
        caught(sim, rng, now)?
    } else {
        bred(sim, rng, now)?
    };
    let level = pkm.party.as_ref()?.level;
    let wanted = ask(sim, rng, pkm.species, level);
    Record::compose(&pkm, wanted, &sim.trailer())
}

pub(super) fn listing(record: &Record) -> GtsListing {
    let wanted = record.wanted();
    GtsListing {
        species: i64::from(record.species()),
        gender: i64::from(record.gender()),
        level: i64::from(record.level()),
        wanted_species: i64::from(wanted.species),
        wanted_gender: i64::from(wanted.gender),
        wanted_min: i64::from(wanted.min),
        wanted_max: i64::from(wanted.max),
        country: i64::from(record.country()),
    }
}

fn gender_fits(wanted: u8, gender: Gender) -> bool {
    matches!(
        (wanted, gender),
        (EITHER, _) | (1, Gender::Male) | (2, Gender::Female)
    )
}

fn answer(sim: &Sim, rng: &mut Rng, wanted: Wanted, now: u64) -> Option<Pokemon> {
    let cap = sim.cap();
    let top = if wanted.max == 0 {
        cap.max(wanted.min)
    } else {
        wanted.max
    };
    let low = wanted.min.max(1);
    if low > top {
        return None;
    }
    let options: Vec<(Catch, u8)> = generate::catches(sim.version)
        .into_iter()
        .filter(|c| c.min_level() <= cap)
        .filter_map(|c| {
            let from = low.max(c.max_level());
            (from..=top)
                .find(|&l| {
                    generate::evolved(sim.version, c.species(), c.max_level(), l) == wanted.species
                })
                .map(|l| (c, l))
        })
        .collect();
    if options.is_empty() {
        return None;
    }
    let trainer = sim.trainer();
    for _ in 0..MAKE_TRIES {
        let (catch, first) = rng.pick(&options);
        let level = first + rng.below(u64::from(top - first) + 1) as u8;
        let grow = (level > catch.max_level()).then_some(level);
        let met = date_of(sim.history_day(rng, now));
        let Some(pkm) = generate::wild(&trainer, rng, catch, grow, met) else {
            continue;
        };
        let level_now = generate::party(sim.version, &pkm)?.level;
        if pkm.species == wanted.species
            && gender_fits(wanted.gender, pkm.gender)
            && (wanted.min == 0 || level_now >= wanted.min)
            && (wanted.max == 0 || level_now <= wanted.max)
        {
            return with_party(sim.version, pkm);
        }
    }
    None
}

fn new_sim(rng: &mut Rng, now: u64) -> SimRow {
    let known = citrine_gb_pkm::names::languages();
    let spoken: Vec<(Language, u64)> = LANGUAGES
        .iter()
        .copied()
        .filter(|(l, _)| known.contains(l))
        .collect();
    let total: u64 = spoken.iter().map(|(_, w)| w).sum();
    let mut roll = rng.below(total.max(1));
    let language = spoken
        .iter()
        .find(|(_, w)| {
            let hit = roll < *w;
            roll = roll.saturating_sub(*w);
            hit
        })
        .map_or(Language::English, |(l, _)| *l);
    let female = rng.chance(35);
    let name = *rng.pick(names::names(language, female));
    let place = places::pick(rng, language);
    let days = rng.below(40) as i64;
    let tid = rng.next_u64() as u16;
    let mut sid = rng.next_u64() as u16;
    if sid == tid {
        sid = sid.wrapping_add(1);
    }
    let pid = 1 + (rng.next_u64() % 0x7FFF_FFFE) as i64;
    SimRow {
        pid,
        name: name.to_owned(),
        language: i64::from(language.to_byte()),
        version: i64::from(*rng.pick(&VERSIONS)),
        country: i64::from(place.country),
        region: i64::from(place.region),
        tid: i64::from(tid),
        sid: i64::from(sid),
        female,
        avatar: i64::from(places::avatar(rng, female)),
        utc_offset: i64::from(place.utc_offset),
        seed: (rng.next_u64() >> 1) as i64,
        started_at: now.saturating_sub(days as u64 * DAY + rng.below(DAY)),
        days,
        next_at: now + rng.below(DAY),
    }
}

fn delay(row: &GtsRow) -> u64 {
    let mut rng = Rng::new((row.pid as u64) ^ row.deposited_at.rotate_left(17));
    TAKE_AFTER + rng.below(TAKE_SPREAD)
}

pub async fn tick(db: &Db, now: u64) -> Result<Vec<i64>> {
    let mut rng = Rng::new(now.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let cap = cap(db
        .gts_active_players(now.saturating_sub(ACTIVE_FOR))
        .await?);
    let mut open = db.gts_sim_open().await?;
    if open > cap {
        let listed = db.gts_sim_listed(GENERATION, None).await?;
        for row in listed
            .iter()
            .rev()
            .take(usize::try_from(open - cap).unwrap_or_default())
        {
            db.gts_sim_withdraw(row.pid).await?;
            open -= 1;
        }
    }
    let mut sims: Vec<Sim> = db.gts_sims().await?.into_iter().map(Sim::new).collect();
    let wanted_sims = usize::try_from((cap * 3 + 1) / 2).unwrap_or_default();
    while sims.len() < wanted_sims {
        let row = new_sim(&mut rng, now);
        if !db.gts_sim_add(&row).await? {
            continue;
        }
        let sim = Sim::new(row);
        if open < cap && rng.chance(70) {
            let mut sim_rng = Rng::new(sim.row.seed as u64);
            if let Some(mut record) = offer(&sim, &mut sim_rng, now) {
                let at = now.saturating_sub(rng.below(BACKDATE_UP_TO));
                record.stamp(sim.row.pid as i32, at, None);
                if db
                    .gts_sim_deposit(sim.row.pid, GENERATION, &record.bytes, listing(&record), at)
                    .await?
                {
                    open += 1;
                }
            }
        }
        sims.push(sim);
    }
    for sim in sims.iter_mut().filter(|s| s.row.next_at <= now) {
        let mut sim_rng = Rng::new(sim.row.seed as u64 ^ now);
        sim.row.days += 1 + i64::from(sim_rng.chance(30));
        if let Some(row) = db.gts_sim_record(sim.row.pid).await? {
            let stale = row.deposited_at + GIVE_UP_AFTER <= now;
            let rethink = row.deposited_at + RETHINK_AFTER <= now && sim_rng.chance(RETHINK_CHANCE);
            if row.traded_at.is_some() || stale || rethink {
                db.gts_sim_withdraw(sim.row.pid).await?;
                if row.traded_at.is_none() {
                    open -= 1;
                }
            }
        }
        if db.gts_sim_record(sim.row.pid).await?.is_none()
            && open < cap
            && sim_rng.chance(DEPOSIT_CHANCE)
            && let Some(mut record) = offer(sim, &mut sim_rng, now)
        {
            record.stamp(sim.row.pid as i32, now, None);
            if db
                .gts_sim_deposit(
                    sim.row.pid,
                    GENERATION,
                    &record.bytes,
                    listing(&record),
                    now,
                )
                .await?
            {
                open += 1;
            }
        }
        sim.row.next_at = sim.next_evening(&mut sim_rng, now);
        sim.row.seed = (sim_rng.next_u64() >> 1) as i64;
        db.gts_sim_played(sim.row.pid, sim.row.days, sim.row.seed, sim.row.next_at)
            .await?;
    }
    let mut took = Vec::new();
    if !sims.is_empty() {
        for row in db
            .gts_waiting(GENERATION, now.saturating_sub(TAKE_AFTER))
            .await?
        {
            if took.len() >= MOST_TAKES {
                break;
            }
            if now < row.deposited_at + delay(&row) || !rng.chance(TAKE_CHANCE) {
                continue;
            }
            if take(db, &sims, &mut rng, &row, now).await? {
                took.push(row.pid);
            }
        }
    }
    Ok(took)
}

async fn take(db: &Db, sims: &[Sim], rng: &mut Rng, row: &GtsRow, now: u64) -> Result<bool> {
    let Some(theirs) = Record::new(Generation::Four, &row.record) else {
        return Ok(false);
    };
    let wanted = theirs.wanted();
    if legendary(wanted.species) {
        return Ok(false);
    }
    let asked_level = if wanted.max == 0 {
        wanted.min.max(1)
    } else {
        ((u16::from(wanted.min.max(1)) + u16::from(wanted.max)) / 2) as u8
    };
    if value(theirs.species(), theirs.level()) + FAIR < value(wanted.species, asked_level) {
        return Ok(false);
    }
    let start = rng.below(sims.len() as u64) as usize;
    for sim in sims.iter().cycle().skip(start).take(sims.len()) {
        let Some(pkm) = answer(sim, rng, wanted, now) else {
            continue;
        };
        let back = Wanted {
            species: theirs.species(),
            gender: EITHER,
            min: 0,
            max: 0,
        };
        let Some(mut given) = Record::compose(&pkm, back, &sim.trailer()) else {
            continue;
        };
        given.stamp(row.pid as i32, row.deposited_at, Some(now));
        return db
            .gts_sim_took(row.pid, &given.bytes, sim.row.pid, now)
            .await;
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gts::record::tests::pokemon_record;
    use citrine_gb_net::PublicKey;

    const START: u64 = 1_762_560_000;
    const STEP: u64 = 600;
    const PLAYERS: i64 = 5;

    struct Relay {
        db: Db,
        pids: Vec<i64>,
        _dir: tempfile::TempDir,
    }

    async fn relay(players: i64) -> Relay {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("relay.db")).await.unwrap();
        let mut pids = Vec::new();
        for n in 0..players {
            let key = [n as u8 + 1; 32];
            let account = db
                .create_account(&key, &PublicKey(key), &format!("P{n}"), 1)
                .await
                .unwrap()
                .id;
            let pid = 3_000_000 + n;
            assert!(db.gts_claim(pid, account, GENERATION, START).await.unwrap());
            pids.push(pid);
        }
        Relay {
            db,
            pids,
            _dir: dir,
        }
    }

    fn asking(species: u16, level: u8, wanted: Wanted) -> Record {
        let pokemon = pokemon_record(Generation::Four, species, level)
            .pokemon()
            .unwrap();
        let trailer = Trailer {
            name: "Ren",
            language: Language::English,
            female: false,
            tid: 1234,
            country: 0,
            region: 0,
            avatar: 0,
            version: PLATINUM,
        };
        Record::compose(&pokemon, wanted, &trailer).unwrap()
    }

    fn sound(record: &Record) {
        let pokemon = record.check().unwrap();
        assert_eq!(legality::check(&pokemon), Ok(()), "{pokemon:?}");
        assert!(!pokemon.is_shiny(), "{pokemon:?}");
        assert!(!legendary(pokemon.species));
    }

    #[tokio::test]
    async fn a_week_stays_under_the_cap_and_every_sim_pokemon_is_legal() {
        let r = relay(PLAYERS).await;
        let bidoof = Wanted {
            species: 399,
            gender: EITHER,
            min: 0,
            max: 0,
        };
        let starly = asking(396, 5, bidoof);
        let listed = listing(&starly);
        assert!(
            r.db.gts_deposit(r.pids[0], GENERATION, &starly.bytes, listed, START)
                .await
                .unwrap()
        );
        let mut seen = HashSet::new();
        let mut took = Vec::new();
        let mut most = 0;
        let mut now = START;
        while now < START + 7 * DAY {
            if now.is_multiple_of(DAY) {
                for &pid in &r.pids {
                    r.db.gts_searched(pid, now).await.unwrap();
                }
            }
            let took_now = tick(&r.db, now).await.unwrap();
            let open = r.db.gts_sim_open().await.unwrap();
            assert!(open <= cap(PLAYERS), "{open} open over {}", cap(PLAYERS));
            most = most.max(open);
            for row in r.db.gts_sim_listed(GENERATION, None).await.unwrap() {
                if seen.insert((row.pid, row.deposited_at)) {
                    sound(&Record::new(Generation::Four, &row.record).unwrap());
                }
            }
            took.extend(took_now);
            now += STEP;
        }
        assert!(most >= cap(PLAYERS) / 2, "only {most} open at most");
        assert!(
            seen.len() > cap(PLAYERS) as usize,
            "{} deposits",
            seen.len()
        );
        assert_eq!(took, vec![r.pids[0]]);
        let row = r.db.gts_record(r.pids[0]).await.unwrap().unwrap();
        assert!(row.traded_with_sim);
        let given = Record::new(Generation::Four, &row.received.unwrap()).unwrap();
        sound(&given);
        assert_eq!(given.species(), 399);
    }

    #[tokio::test]
    async fn no_sims_without_players_and_clearing_removes_them() {
        let r = relay(0).await;
        assert!(tick(&r.db, START).await.unwrap().is_empty());
        assert_eq!(r.db.gts_sim_open().await.unwrap(), 0);
        assert!(r.db.gts_sims().await.unwrap().is_empty());

        let r = relay(2).await;
        tick(&r.db, START).await.unwrap();
        assert_eq!(r.db.gts_sims().await.unwrap().len(), 9);
        r.db.gts_sims_clear().await.unwrap();
        assert!(r.db.gts_sims().await.unwrap().is_empty());
        assert_eq!(r.db.gts_sim_open().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn a_sim_pid_is_never_claimed_by_a_player() {
        let r = relay(1).await;
        tick(&r.db, START).await.unwrap();
        let sim = r.db.gts_sims().await.unwrap()[0].pid;
        assert!(!r.db.gts_claim(sim, 1, GENERATION, START).await.unwrap());
    }

    #[tokio::test]
    async fn an_unfair_ask_is_never_taken() {
        let r = relay(PLAYERS).await;
        let wanted = Wanted {
            species: 248,
            gender: EITHER,
            min: 0,
            max: 0,
        };
        let bidoof = asking(399, 3, wanted);
        let listed = listing(&bidoof);
        r.db.gts_deposit(r.pids[0], GENERATION, &bidoof.bytes, listed, START)
            .await
            .unwrap();
        let mut now = START;
        while now < START + 2 * DAY {
            assert!(tick(&r.db, now).await.unwrap().is_empty());
            now += STEP;
        }
    }

    #[test]
    fn asks_are_fair_and_never_legendary() {
        let mut rng = Rng::new(7);
        let mut fair = 0;
        let total = 400;
        for _ in 0..total {
            let sim = Sim::new(new_sim(&mut rng, START));
            let Some(record) = offer(&sim, &mut rng, START) else {
                continue;
            };
            let wanted = record.wanted();
            assert!(!legendary(wanted.species));
            assert_ne!(wanted.species, record.species());
            assert!(wanted.max == 0 || wanted.min <= wanted.max);
            let asked = if wanted.max == 0 {
                record.level()
            } else {
                ((u16::from(wanted.min.max(1)) + u16::from(wanted.max)) / 2) as u8
            };
            let gap =
                (value(wanted.species, asked) - value(record.species(), record.level())).abs();
            if gap <= FAIR {
                fair += 1;
            }
        }
        assert!(fair * 10 >= total * 9, "{fair} of {total} fair");
    }

    #[test]
    fn sims_speak_the_languages_we_have_names_for() {
        let mut rng = Rng::new(11);
        let known = citrine_gb_pkm::names::languages();
        for _ in 0..200 {
            let row = new_sim(&mut rng, START);
            let language = Language::from_byte(row.language as u8);
            assert!(known.contains(&language), "{language:?}");
            assert!(row.tid != row.sid);
            assert!(row.started_at <= START && row.next_at >= START);
        }
    }

    fn offers(count: usize, seed: u64) -> Vec<Pokemon> {
        let mut rng = Rng::new(seed);
        let mut made = Vec::new();
        while made.len() < count {
            let sim = Sim::new(new_sim(&mut rng, START));
            if let Some(record) = offer(&sim, &mut rng, START)
                && let Some(pkm) = record.pokemon()
            {
                made.push(pkm);
            }
        }
        made
    }

    fn answers(count: usize, seed: u64) -> Vec<Pokemon> {
        let mut rng = Rng::new(seed);
        let mut made = Vec::new();
        let mut tries = 0;
        while made.len() < count && tries < count * 50 {
            tries += 1;
            let sim = Sim::new(new_sim(&mut rng, START));
            let catches = generate::catches(sim.version);
            let species = rng.pick(&catches).species();
            let (min, max) = if rng.chance(50) {
                (0, 0)
            } else {
                bracket(1 + rng.below(40) as u8)
            };
            let wanted = Wanted {
                species,
                gender: EITHER,
                min,
                max,
            };
            if let Some(pkm) = answer(&sim, &mut rng, wanted, START) {
                made.push(pkm);
            }
        }
        made
    }

    #[test]
    fn answers_are_legal_and_fit_the_ask() {
        let made = answers(150, 9);
        assert!(made.len() >= 100, "{} answers", made.len());
        for pokemon in made {
            assert_eq!(legality::check(&pokemon), Ok(()), "{pokemon:?}");
            assert!(!pokemon.is_shiny());
        }
    }

    #[test]
    fn offers_are_legal() {
        for pokemon in offers(300, 3) {
            assert_eq!(legality::check(&pokemon), Ok(()), "{pokemon:?}");
            assert!(!pokemon.is_shiny());
            let level = pokemon.party.as_ref().unwrap().level;
            assert!(u32::from(level) <= LAST_CAP);
        }
    }

    #[test]
    #[ignore]
    fn dump_offers_for_the_lab() {
        let Ok(dir) = std::env::var("CITRINE_SIM_DUMP") else {
            return;
        };
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir.join("pk")).unwrap();
        let mut list = String::new();
        let made = offers(2000, 5)
            .into_iter()
            .map(|p| ("offer", p))
            .chain(answers(1000, 6).into_iter().map(|p| ("answer", p)));
        for (i, (kind, pokemon)) in made.enumerate() {
            let name = format!("sim{i:05}.pk4");
            std::fs::write(dir.join("pk").join(&name), pokemon.encode_box()).unwrap();
            list.push_str(&format!("{name}\t{kind}\n"));
        }
        std::fs::write(dir.join("files.tsv"), list).unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn seed_a_relay_for_the_real_game() {
        let Ok(path) = std::env::var("CITRINE_SIM_SEED") else {
            return;
        };
        let db = Db::open(std::path::Path::new(&path)).await.unwrap();
        db.set_setting(crate::settings::GTS_SIMS, "on")
            .await
            .unwrap();
        let mut rng = Rng::new(21);
        let now = crate::now();
        let mut row = new_sim(&mut rng, now);
        row.version = i64::from(HEARTGOLD);
        row.days = 30;
        let sim = Sim::new(row);
        let mut made = None;
        while made.is_none() {
            let hatched_at = sim.history_day(&mut rng, now - DAY);
            made = generate::hatched(
                &sim.trainer(),
                &mut rng,
                155,
                5,
                date_of(hatched_at - DAY),
                date_of(hatched_at),
            )
            .and_then(|pkm| with_party(sim.version, pkm));
        }
        let pkm = made.unwrap();
        assert_eq!(legality::check(&pkm), Ok(()));
        let wanted = Wanted {
            species: 155,
            gender: EITHER,
            min: 0,
            max: 0,
        };
        let mut record = Record::compose(&pkm, wanted, &sim.trailer()).unwrap();
        let at = now - 2 * HOUR;
        record.stamp(sim.row.pid as i32, at, None);
        assert!(db.gts_sim_add(&sim.row).await.unwrap());
        assert!(
            db.gts_sim_deposit(sim.row.pid, GENERATION, &record.bytes, listing(&record), at)
                .await
                .unwrap()
        );
        println!(
            "seeded sim {} {} lv {} for {}",
            sim.row.pid,
            sim.row.name,
            record.level(),
            record.trainer()
        );
    }
}
