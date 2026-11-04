use crate::game::{Game, Generation, Version};
use crate::names::landmark;
use crate::rom::in_rom_bank;
use crate::scan::{Pattern, bank_of, banked, byte, same_bank, word};
use crate::state::Location;

const GEN1_FIRST_INDOOR: u8 = 0x25;
const GEN2_MAP_GROUPS: usize = 26;
const GEN2_MAP_RECORD: usize = 9;
const MAX_MAPS_PER_GROUP: usize = 96;
const SPECIAL_LANDMARK: u8 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Kanto,
    Johto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot<'a> {
    pub name: &'a str,
    pub region: Region,
    pub x: u8,
    pub y: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Landmark {
    pub x: u8,
    pub y: u8,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Places {
    outdoor: Vec<(u8, String)>,
    indoor: Vec<(u8, u8, String)>,
    pub landmarks: Vec<Landmark>,
    map_landmarks: Vec<Vec<u8>>,
    kanto_from: u8,
}

impl Places {
    pub fn load(game: Game, rom: &[u8]) -> Self {
        match game.generation() {
            Generation::One => gen1(game, rom).unwrap_or_default(),
            Generation::Two => gen2(game, rom),
            Generation::Three => Self::default(),
        }
    }

    pub fn spot(&self, location: &Location) -> Option<Spot<'_>> {
        if self.map_landmarks.is_empty() {
            return self.gen1_spot(location.map);
        }
        let mut landmark = self.landmark_of(location.group, location.map)?;
        if landmark == SPECIAL_LANDMARK {
            let (group, map) = location.outside?;
            landmark = self.landmark_of(group, map)?;
        }
        if landmark == SPECIAL_LANDMARK {
            return None;
        }
        let entry = self.landmarks.get(usize::from(landmark))?;
        Some(Spot {
            name: &entry.name,
            region: if landmark >= self.kanto_from {
                Region::Kanto
            } else {
                Region::Johto
            },
            x: entry.x,
            y: entry.y,
        })
    }

    fn landmark_of(&self, group: u8, map: u8) -> Option<u8> {
        let group = self.map_landmarks.get(usize::from(group).checked_sub(1)?)?;
        group.get(usize::from(map).checked_sub(1)?).copied()
    }

    fn gen1_spot(&self, map: u8) -> Option<Spot<'_>> {
        let (coords, name) = if map < GEN1_FIRST_INDOOR {
            let (coords, name) = self.outdoor.get(usize::from(map))?;
            (*coords, name)
        } else {
            let (_, coords, name) = self.indoor.iter().find(|entry| map < entry.0)?;
            (*coords, name)
        };
        Some(Spot {
            name,
            region: Region::Kanto,
            x: coords & 0x0F,
            y: coords >> 4,
        })
    }
}

fn gen1(game: Game, rom: &[u8]) -> Option<Places> {
    let pattern = Pattern::new(
        "FE 25 38 ?? 01 04 00 21 ?? ?? BE 38 03 09 18 FA 23 18 ?? 21 ?? ?? 4F 06 00 09 09 09",
    );
    let at = pattern.find_all(rom).next()?;
    let bank = bank_of(at);
    let indoor_at = same_bank(rom, at, 8)?;
    let outdoor_at = same_bank(rom, at, 20)?;
    let name = |pointer: usize| -> Option<String> {
        landmark(game, rom, banked(rom, bank, word(rom, pointer)?)?)
    };
    let outdoor = (0..usize::from(GEN1_FIRST_INDOOR))
        .map(|i| {
            Some((
                byte(rom, outdoor_at + 3 * i)?,
                name(outdoor_at + 3 * i + 1)?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let mut indoor = Vec::new();
    let mut entry = indoor_at;
    while let Some(limit) = byte(rom, entry).filter(|&b| b != 0xFF) {
        indoor.push((limit, byte(rom, entry + 1)?, name(entry + 2)?));
        entry += 4;
        if indoor.len() > 0x100 {
            return None;
        }
    }
    Some(Places {
        outdoor,
        indoor,
        ..Places::default()
    })
}

fn gen2(game: Game, rom: &[u8]) -> Places {
    let (count, kanto_from) = match game.version {
        Version::Crystal => (0x60, 0x2F),
        _ => (0x5F, 0x2E),
    };
    let landmarks = gen2_landmarks(game, rom, count);
    let map_landmarks = if landmarks.is_empty() {
        Vec::new()
    } else {
        gen2_maps(rom, landmarks.len()).unwrap_or_default()
    };
    Places {
        landmarks,
        map_landmarks,
        kanto_from,
        ..Places::default()
    }
}

fn gen2_landmarks(game: Game, rom: &[u8], count: usize) -> Vec<Landmark> {
    Pattern::new("6B 26 00 29 29 11 ?? ?? 19 2A 66 6F")
        .find_all(rom)
        .find_map(|at| {
            let entries = same_bank(rom, at, 6)?.checked_sub(2)?;
            (0..count)
                .map(|i| {
                    let pointer = word(rom, entries + 4 * i + 2)?;
                    Some(Landmark {
                        x: byte(rom, entries + 4 * i)?,
                        y: byte(rom, entries + 4 * i + 1)?,
                        name: landmark(game, rom, banked(rom, bank_of(entries), pointer)?)?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn gen2_maps(rom: &[u8], landmarks: usize) -> Option<Vec<Vec<u8>>> {
    let pattern = Pattern::new("C5 05 48 06 00 21 ?? ?? 09 09 2A 66 6F C1 0D");
    let at = pattern.find_all(rom).next()?;
    let table = word(rom, at + 6)?;
    let banks = rom.len() / 0x4000;
    let record_ok = |at: usize| -> bool {
        rom.get(at..at + GEN2_MAP_RECORD).is_some_and(|r| {
            usize::from(r[0]) < banks
                && r[0] != 0
                && (1..=7).contains(&r[2])
                && in_rom_bank(u16::from_le_bytes([r[3], r[4]]))
                && usize::from(r[5]) < landmarks
        })
    };
    (1..banks).find_map(|bank| {
        let starts = (0..GEN2_MAP_GROUPS)
            .map(|g| {
                let pointer = word(rom, banked(rom, bank, table)? + 2 * g)?;
                in_rom_bank(pointer).then_some(())?;
                let start = banked(rom, bank, pointer)?;
                record_ok(start).then_some(start)
            })
            .collect::<Option<Vec<usize>>>()?;
        Some(
            starts
                .iter()
                .map(|&start| {
                    (0..MAX_MAPS_PER_GROUP)
                        .map(|i| start + i * GEN2_MAP_RECORD)
                        .take_while(|&at| record_ok(at) && (at == start || !starts.contains(&at)))
                        .map(|at| rom[at + 5])
                        .collect()
                })
                .collect(),
        )
    })
}
