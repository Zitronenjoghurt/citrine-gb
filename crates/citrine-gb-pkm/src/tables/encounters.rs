use super::{ExtractError, Game, record, u16_at, u32_at};
use citrine_gb_nds_files::Narc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EncounterMethod {
    Grass,
    GrassMorning,
    GrassDay,
    GrassNight,
    Swarm,
    Radar,
    DualSlot(u8),
    HoennSound,
    SinnohSound,
    Surf,
    RockSmash,
    OldRod,
    GoodRod,
    SuperRod,
    SurfSwarm,
    FishingSwarm,
    NightGoodRod,
    NightSuperRod,
    SafariGrass,
    SafariSurf,
    SafariOldRod,
    SafariGoodRod,
    SafariSuperRod,
    SafariGrassBlock,
    SafariSurfBlock,
    SafariOldRodBlock,
    SafariGoodRodBlock,
    SafariSuperRodBlock,
    BugContest,
    GreatMarsh,
    GreatMarshNational,
    TrophyGarden,
    DoubleGrass,
    ShakingGrass,
    SurfSpot,
    Fishing,
    FishingSpot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Encounter {
    pub place: u16,
    pub season: Option<u8>,
    pub method: EncounterMethod,
    pub slot: u8,
    pub species: u16,
    pub form: u8,
    pub min_level: u8,
    pub max_level: u8,
}

const DPPT_LEN: usize = 0x1A8;
const HGSS_LEN: usize = 0xC4;
const BW_SEASON_LEN: usize = 0xE8;

pub(super) fn read(game: Game, narc: &Narc<'_>) -> Result<Vec<Encounter>, ExtractError> {
    let mut out = Vec::new();
    for (index, file) in narc.files().enumerate() {
        let file = file?;
        let mut sink = Sink {
            out: &mut out,
            place: index as u16,
            season: None,
        };
        match game {
            Game::Diamond | Game::Pearl | Game::Platinum => {
                dppt(&mut sink, record("encounter", index, file, DPPT_LEN)?)
            }
            Game::HeartGold | Game::SoulSilver => {
                hgss(&mut sink, record("encounter", index, file, HGSS_LEN)?)
            }
            Game::Black | Game::White | Game::Black2 | Game::White2 => {
                let seasons = file.len() / BW_SEASON_LEN;
                for season in 0..seasons {
                    sink.season = (seasons == 4).then_some(season as u8);
                    let at = season * BW_SEASON_LEN;
                    bw(&mut sink, &file[at..at + BW_SEASON_LEN]);
                }
            }
        }
    }
    Ok(out)
}

struct Sink<'a> {
    out: &'a mut Vec<Encounter>,
    place: u16,
    season: Option<u8>,
}

impl Sink<'_> {
    fn push(
        &mut self,
        method: EncounterMethod,
        slot: usize,
        species: u16,
        form: u8,
        levels: (u8, u8),
    ) {
        if species == 0 {
            return;
        }
        self.out.push(Encounter {
            place: self.place,
            season: self.season,
            method,
            slot: slot as u8,
            species,
            form,
            min_level: levels.0.min(levels.1),
            max_level: levels.0.max(levels.1),
        });
    }
}

fn dppt(sink: &mut Sink<'_>, r: &[u8]) {
    use EncounterMethod::*;
    if u32_at(r, 0) != 0 {
        let level = |slot: usize| u32_at(r, 4 + slot * 8) as u8;
        let species = |at: usize| u32_at(r, at) as u16;
        for slot in 0..12 {
            sink.push(
                Grass,
                slot,
                species(8 + slot * 8),
                0,
                (level(slot), level(slot)),
            );
        }
        let replaced: [(EncounterMethod, usize, &[usize]); 4] = [
            (Swarm, 0x64, &[0, 1]),
            (GrassDay, 0x6C, &[2, 3]),
            (GrassNight, 0x74, &[2, 3]),
            (Radar, 0x7C, &[4, 5, 10, 11]),
        ];
        for (method, at, slots) in replaced {
            for (i, &slot) in slots.iter().enumerate() {
                sink.push(
                    method,
                    slot,
                    species(at + i * 4),
                    0,
                    (level(slot), level(slot)),
                );
            }
        }
        for (i, version) in [2u8, 1, 3, 4, 5].into_iter().enumerate() {
            for (j, slot) in [8usize, 9].into_iter().enumerate() {
                let at = 0xA4 + (i * 2 + j) * 4;
                sink.push(
                    DualSlot(version),
                    slot,
                    species(at),
                    0,
                    (level(slot), level(slot)),
                );
            }
        }
    }
    for (method, at) in [
        (Surf, 0xCC),
        (RockSmash, 0xF8),
        (OldRod, 0x124),
        (GoodRod, 0x150),
        (SuperRod, 0x17C),
    ] {
        if u32_at(r, at) == 0 {
            continue;
        }
        for slot in 0..5 {
            let e = at + 4 + slot * 8;
            sink.push(method, slot, u32_at(r, e + 4) as u16, 0, (r[e], r[e + 1]));
        }
    }
}

fn hgss(sink: &mut Sink<'_>, r: &[u8]) {
    use EncounterMethod::*;
    if r[0] != 0 {
        for (method, at) in [(GrassMorning, 0x14), (GrassDay, 0x2C), (GrassNight, 0x44)] {
            for slot in 0..12 {
                let level = r[0x08 + slot];
                sink.push(method, slot, u16_at(r, at + slot * 2), 0, (level, level));
            }
        }
        for (method, at) in [(HoennSound, 0x5C), (SinnohSound, 0x60)] {
            for i in 0..2 {
                for slot in [2 + 2 * i, 3 + 2 * i] {
                    let level = r[0x08 + slot];
                    sink.push(method, slot, u16_at(r, at + i * 2), 0, (level, level));
                }
            }
        }
    }
    let water: [(EncounterMethod, usize, u8, usize); 4] = [
        (Surf, 0x64, r[1], 5),
        (OldRod, 0x80, r[3], 5),
        (GoodRod, 0x94, r[4], 5),
        (SuperRod, 0xA8, r[5], 5),
    ];
    for (method, at, rate, slots) in water {
        if rate == 0 {
            continue;
        }
        for slot in 0..slots {
            let e = at + slot * 4;
            sink.push(method, slot, u16_at(r, e + 2), 0, (r[e], r[e + 1]));
        }
    }
    if r[2] != 0 {
        for slot in 0..2 {
            let e = 0x78 + slot * 4;
            sink.push(RockSmash, slot, u16_at(r, e + 2), 0, (r[e], r[e + 1]));
        }
    }
    for (slot, method) in [(0, Swarm), (1, SurfSwarm), (3, FishingSwarm)] {
        sink.push(method, 0, u16_at(r, 0xBC + slot * 2), 0, (0, 0));
    }
    let night = u16_at(r, 0xBC + 4);
    for (method, rate, at, slot) in [
        (NightGoodRod, r[4], 0x94, 3),
        (NightSuperRod, r[5], 0xA8, 1),
    ] {
        if rate != 0 {
            let e = at + slot * 4;
            sink.push(method, slot, night, 0, (r[e], r[e + 1]));
        }
    }
}

const SAFARI_LEN: usize = 0x390;
const SAFARI_SLOTS: usize = 10;
const SAFARI_TIMES: usize = 3;

pub(super) fn read_safari(narc: &Narc<'_>) -> Result<Vec<Encounter>, ExtractError> {
    use EncounterMethod::*;
    let mut out = Vec::new();
    for (index, file) in narc.files().enumerate() {
        let r = record("safari", index, file?, SAFARI_LEN)?;
        let mut sink = Sink {
            out: &mut out,
            place: index as u16,
            season: None,
        };
        let mut at = 8;
        for (kind, (plain, block)) in [
            (SafariGrass, SafariGrassBlock),
            (SafariSurf, SafariSurfBlock),
            (SafariOldRod, SafariOldRodBlock),
            (SafariGoodRod, SafariGoodRodBlock),
            (SafariSuperRod, SafariSuperRodBlock),
        ]
        .into_iter()
        .enumerate()
        {
            let objects = usize::from(r[kind]);
            for (method, count) in [(plain, SAFARI_SLOTS), (block, objects)] {
                for _ in 0..SAFARI_TIMES {
                    for slot in 0..count {
                        let e = at + slot * 4;
                        let level = r[e + 2];
                        sink.push(method, slot, u16_at(r, e), 0, (level, level));
                    }
                    at += count * 4;
                }
            }
            at += objects * 4;
        }
        let mut seen = std::collections::HashSet::new();
        sink.out.retain(|e| seen.insert(*e));
    }
    Ok(out)
}

const BUG_CONTEST_SLOTS: usize = 10;
const BUG_CONTEST_LEN: usize = 8;

pub(super) fn read_bug_contest(file: &[u8]) -> Vec<Encounter> {
    let mut out = Vec::new();
    for (index, set) in file
        .as_chunks::<{ BUG_CONTEST_SLOTS * BUG_CONTEST_LEN }>()
        .0
        .iter()
        .enumerate()
    {
        let mut sink = Sink {
            out: &mut out,
            place: index as u16,
            season: None,
        };
        for (slot, e) in set.as_chunks::<BUG_CONTEST_LEN>().0.iter().enumerate() {
            sink.push(
                EncounterMethod::BugContest,
                slot,
                u16_at(e, 0),
                0,
                (e[2], e[3]),
            );
        }
    }
    out
}

pub(super) const TROPHY_GARDEN: u16 = 68;
pub(super) const GREAT_MARSH: u16 = 52;
const SWAPPED_SLOTS: [u8; 2] = [6, 7];

pub(super) fn daily_swaps(
    encounters: &[Encounter],
    files: &[(u16, u16)],
    lists: &Narc<'_>,
) -> Result<Vec<Encounter>, ExtractError> {
    use EncounterMethod::*;
    let mut out = Vec::new();
    for (method, list, location) in [
        (TrophyGarden, 8, TROPHY_GARDEN),
        (GreatMarsh, 9, GREAT_MARSH),
        (GreatMarshNational, 10, GREAT_MARSH),
    ] {
        let species: Vec<u16> = lists
            .file(list)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|e| u16_at(e, 0))
            .collect();
        for &(file, _) in files.iter().filter(|&&(_, at)| at == location) {
            let mut seen = std::collections::HashSet::new();
            for base in encounters
                .iter()
                .filter(|e| e.place == file && e.method == Grass && SWAPPED_SLOTS.contains(&e.slot))
            {
                for &species in &species {
                    let swapped = Encounter {
                        method,
                        species,
                        ..*base
                    };
                    if species != 0 && seen.insert(swapped) {
                        out.push(swapped);
                    }
                }
            }
        }
    }
    Ok(out)
}

fn bw(sink: &mut Sink<'_>, r: &[u8]) {
    use EncounterMethod::*;
    let groups = [
        (Grass, 12),
        (DoubleGrass, 12),
        (ShakingGrass, 12),
        (Surf, 5),
        (SurfSpot, 5),
        (Fishing, 5),
        (FishingSpot, 5),
    ];
    let mut at = 8;
    for (rate_at, (method, slots)) in groups.into_iter().enumerate() {
        for slot in 0..slots {
            let packed = u16_at(r, at);
            if r[rate_at] != 0 {
                sink.push(
                    method,
                    slot,
                    packed & 0x7FF,
                    (packed >> 11) as u8,
                    (r[at + 2], r[at + 3]),
                );
            }
            at += 4;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use EncounterMethod::*;
    use citrine_gb_nds_files::fixtures::narc;

    fn put32(r: &mut [u8], at: usize, value: u32) {
        r[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put16(r: &mut [u8], at: usize, value: u16) {
        r[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn read_one(game: Game, file: Vec<u8>) -> Vec<Encounter> {
        let bytes = narc::build(&[vec![0; file.len()], file]);
        read(game, &Narc::parse(&bytes).unwrap()).unwrap()
    }

    fn route_201_dp() -> Vec<u8> {
        let mut r = vec![0u8; DPPT_LEN];
        put32(&mut r, 0, 30);
        for slot in 0..12 {
            put32(&mut r, 4 + slot * 8, 2 + (slot as u32 % 2));
            put32(&mut r, 8 + slot * 8, if slot < 6 { 396 } else { 399 });
        }
        put32(&mut r, 0x6C, 403);
        put32(&mut r, 0xA4, 263);
        put32(&mut r, 0xCC, 10);
        r[0xD0] = 30;
        r[0xD1] = 20;
        put32(&mut r, 0xD4, 54);
        r
    }

    #[test]
    fn reads_dppt_grass_replacements_and_water() {
        let table = read_one(Game::Diamond, route_201_dp());
        assert!(table.iter().all(|e| e.place == 1 && e.season.is_none()));
        let grass: Vec<_> = table.iter().filter(|e| e.method == Grass).collect();
        assert_eq!(grass.len(), 12);
        assert_eq!(
            (grass[0].species, grass[0].min_level, grass[0].max_level),
            (396, 2, 2)
        );
        assert_eq!((grass[11].species, grass[11].min_level), (399, 3));
        let day = table.iter().find(|e| e.method == GrassDay).unwrap();
        assert_eq!((day.slot, day.species, day.min_level), (2, 403, 2));
        let ruby = table.iter().find(|e| e.method == DualSlot(2)).unwrap();
        assert_eq!((ruby.slot, ruby.species), (8, 263));
        let surf = table.iter().find(|e| e.method == Surf).unwrap();
        assert_eq!((surf.species, surf.min_level, surf.max_level), (54, 20, 30));
        assert_eq!(table.len(), 12 + 1 + 1 + 1);
    }

    #[test]
    fn reads_hgss_times_of_day_and_rock_smash() {
        let mut r = vec![0u8; HGSS_LEN];
        r[0] = 30;
        r[2] = 5;
        for slot in 0..12 {
            r[0x08 + slot] = 4;
            put16(&mut r, 0x14 + slot * 2, 16);
            put16(&mut r, 0x2C + slot * 2, 19);
            put16(&mut r, 0x44 + slot * 2, 163);
        }
        put16(&mut r, 0x7A, 74);
        r[0x78] = 13;
        r[0x79] = 15;
        put16(&mut r, 0xBC, 206);
        let table = read_one(Game::HeartGold, r);
        let count = |m| table.iter().filter(|e| e.method == m).count();
        assert_eq!(
            (count(GrassMorning), count(GrassDay), count(GrassNight)),
            (12, 12, 12)
        );
        let night = table.iter().find(|e| e.method == GrassNight).unwrap();
        assert_eq!((night.species, night.min_level), (163, 4));
        let smash = table.iter().find(|e| e.method == RockSmash).unwrap();
        assert_eq!(
            (smash.species, smash.min_level, smash.max_level),
            (74, 13, 15)
        );
        assert_eq!(
            table.iter().find(|e| e.method == Swarm).unwrap().species,
            206
        );
        assert_eq!(count(Surf), 0);
    }

    #[test]
    fn reads_bw_seasons_and_forms() {
        let mut file = vec![0u8; BW_SEASON_LEN * 4];
        for season in 0..4 {
            let r = &mut file[season * BW_SEASON_LEN..];
            r[0] = 10;
            r[2] = 10;
            put16(r, 8, 585 | (season as u16) << 11);
            r[10] = 2;
            r[11] = 4;
            put16(r, 8 + 24 * 4, 506);
            r[8 + 24 * 4 + 2] = 5;
            r[8 + 24 * 4 + 3] = 5;
        }
        let table = read_one(Game::Black, file);
        let deerling: Vec<_> = table.iter().filter(|e| e.species == 585).collect();
        assert_eq!(deerling.len(), 4);
        assert_eq!((deerling[3].season, deerling[3].form), (Some(3), 3));
        assert_eq!((deerling[0].min_level, deerling[0].max_level), (2, 4));
        let shaking = table.iter().find(|e| e.method == ShakingGrass).unwrap();
        assert_eq!((shaking.species, shaking.slot), (506, 0));
    }

    #[test]
    fn a_single_season_file_has_no_season() {
        let mut r = vec![0u8; BW_SEASON_LEN];
        r[0] = 10;
        put16(&mut r, 8, 504);
        let table = read_one(Game::White2, r);
        assert_eq!(table[0].season, None);
    }
}
