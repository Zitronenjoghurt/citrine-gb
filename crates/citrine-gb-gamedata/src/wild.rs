use crate::game::{Game, Generation};
use crate::rom::in_rom_bank;
use crate::scan::{Pattern, bank_of, banked, byte, same_bank, word};
use crate::state::{Location, TimeOfDay};

const GEN1_SLOTS: usize = 10;
const GEN1_CHANCES: [u16; GEN1_SLOTS] = [51, 51, 39, 25, 25, 25, 13, 13, 11, 3];
const GEN1_MAPS: usize = 0x100;
const GEN2_GRASS_SLOTS: usize = 7;
const GEN2_GRASS_CHANCES: [u16; GEN2_GRASS_SLOTS] = [30, 30, 20, 10, 5, 4, 1];
const GEN2_WATER_SLOTS: usize = 3;
const GEN2_WATER_CHANCES: [u16; GEN2_WATER_SLOTS] = [60, 30, 10];
const GEN2_GRASS_RECORD: usize = 2 + 3 + GEN2_GRASS_SLOTS * 2 * 3;
const GEN2_WATER_RECORD: usize = 2 + 1 + GEN2_WATER_SLOTS * 2;
const MAX_GEN2_RECORDS: usize = 0x100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Grass,
    Water,
    RockSmash,
    OldRod,
    GoodRod,
    SuperRod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encounter {
    pub species: u16,
    pub min_level: u8,
    pub max_level: u8,
    pub percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WildList {
    pub method: Method,
    pub encounters: Vec<Encounter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Slots {
    rate: u8,
    slots: Vec<(u8, u8)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Gen2Grass {
    group: u8,
    map: u8,
    rates: [u8; 3],
    slots: [Vec<(u8, u8)>; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct WildTables {
    gen1: Vec<(Option<Slots>, Option<Slots>)>,
    grass: Vec<Gen2Grass>,
    water: Vec<(u8, u8, Slots)>,
}

impl WildTables {
    pub fn load(game: Game, rom: &[u8]) -> Self {
        match game.generation() {
            Generation::One => Self {
                gen1: gen1(rom).unwrap_or_default(),
                ..Self::default()
            },
            Generation::Two => gen2(rom).unwrap_or_default(),
            Generation::Three => Self::default(),
        }
    }

    pub fn here(&self, location: &Location, time: Option<TimeOfDay>) -> Vec<WildList> {
        let mut lists = Vec::new();
        let mut push = |method: Method, slots: &[(u8, u8)], chances: &[u16]| {
            let slots: Vec<Slot> = slots
                .iter()
                .map(|&(level, species)| (level, level, u16::from(species)))
                .collect();
            let encounters = merge(&slots, chances);
            if !encounters.is_empty() {
                lists.push(WildList { method, encounters });
            }
        };
        if !self.gen1.is_empty() {
            if let Some((grass, water)) = self.gen1.get(usize::from(location.map)) {
                if let Some(grass) = grass {
                    push(Method::Grass, &grass.slots, &GEN1_CHANCES);
                }
                if let Some(water) = water {
                    push(Method::Water, &water.slots, &GEN1_CHANCES);
                }
            }
            return lists;
        }
        let at = |group: u8, map: u8| group == location.group && map == location.map;
        if let Some(grass) = self.grass.iter().find(|g| at(g.group, g.map)) {
            let index = match time {
                Some(TimeOfDay::Morning) => 0,
                Some(TimeOfDay::Night) => 2,
                _ => 1,
            };
            if grass.rates[index] > 0 {
                push(Method::Grass, &grass.slots[index], &GEN2_GRASS_CHANCES);
            }
        }
        if let Some((_, _, water)) = self.water.iter().find(|w| at(w.0, w.1))
            && water.rate > 0
        {
            push(Method::Water, &water.slots, &GEN2_WATER_CHANCES);
        }
        lists
    }
}

pub(crate) type Slot = (u8, u8, u16);

pub(crate) fn merge(slots: &[Slot], chances: &[u16]) -> Vec<Encounter> {
    let total: u16 = chances.iter().sum();
    let mut merged: Vec<(Encounter, u16)> = Vec::new();
    for (&(min_level, max_level, species), &chance) in slots.iter().zip(chances) {
        if species == 0 {
            continue;
        }
        match merged.iter_mut().find(|(e, _)| e.species == species) {
            Some((e, sum)) => {
                e.min_level = e.min_level.min(min_level);
                e.max_level = e.max_level.max(max_level);
                *sum += chance;
            }
            None => merged.push((
                Encounter {
                    species,
                    min_level,
                    max_level,
                    percent: 0,
                },
                chance,
            )),
        }
    }
    merged.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    merged
        .into_iter()
        .map(|(mut e, sum)| {
            e.percent =
                ((u32::from(sum) * 100 + u32::from(total) / 2) / u32::from(total)).max(1) as u8;
            e
        })
        .collect()
}

fn pairs(rom: &[u8], at: usize, count: usize) -> Option<Vec<(u8, u8)>> {
    let bytes = rom.get(at..at + count * 2)?;
    Some(
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[a, b]| (a, b))
            .collect(),
    )
}

fn gen1(rom: &[u8]) -> Option<Vec<(Option<Slots>, Option<Slots>)>> {
    let pattern = Pattern::new("21 ?? ?? FA ?? ?? 4F 06 00 09 09 2A 66 6F 2A EA ?? ?? A7 28");
    let at = pattern.find_all(rom).next()?;
    let bank = bank_of(at);
    let table = same_bank(rom, at, 1)?;
    let mut maps = Vec::new();
    for i in 0..GEN1_MAPS {
        let pointer = word(rom, table + 2 * i)?;
        if pointer == 0xFFFF {
            break;
        }
        if !in_rom_bank(pointer) {
            return None;
        }
        let mut at = banked(rom, bank, pointer)?;
        let mut read = || -> Option<Option<Slots>> {
            let rate = byte(rom, at)?;
            at += 1;
            if rate == 0 {
                return Some(None);
            }
            let slots = pairs(rom, at, GEN1_SLOTS)?;
            at += GEN1_SLOTS * 2;
            Some(Some(Slots { rate, slots }))
        };
        let grass = read()?;
        let water = read()?;
        maps.push((grass, water));
    }
    (maps.len() > 0x20).then_some(maps)
}

fn gen2(rom: &[u8]) -> Option<WildTables> {
    let grass_code = Pattern::new("21 ?? ?? 11 ?? ?? CD ?? ?? 01 2F 00")
        .find_all(rom)
        .next()?;
    let water_code = Pattern::new("21 ?? ?? 11 ?? ?? CD ?? ?? 01 09 00")
        .find_all(rom)
        .next()?;
    let mut grass = Vec::new();
    for operand in [1, 4] {
        let mut at = same_bank(rom, grass_code, operand)?;
        for _ in 0..MAX_GEN2_RECORDS {
            let group = byte(rom, at)?;
            if group == 0xFF {
                break;
            }
            let rates = [byte(rom, at + 2)?, byte(rom, at + 3)?, byte(rom, at + 4)?];
            let slot =
                |time: usize| pairs(rom, at + 5 + time * GEN2_GRASS_SLOTS * 2, GEN2_GRASS_SLOTS);
            grass.push(Gen2Grass {
                group,
                map: byte(rom, at + 1)?,
                rates,
                slots: [slot(0)?, slot(1)?, slot(2)?],
            });
            at += GEN2_GRASS_RECORD;
        }
    }
    let mut water = Vec::new();
    for operand in [1, 4] {
        let mut at = same_bank(rom, water_code, operand)?;
        for _ in 0..MAX_GEN2_RECORDS {
            let group = byte(rom, at)?;
            if group == 0xFF {
                break;
            }
            water.push((
                group,
                byte(rom, at + 1)?,
                Slots {
                    rate: byte(rom, at + 2)?,
                    slots: pairs(rom, at + 3, GEN2_WATER_SLOTS)?,
                },
            ));
            at += GEN2_WATER_RECORD;
        }
    }
    let valid = grass.iter().all(|g| (1..=26).contains(&g.group))
        && water.iter().all(|w| (1..=26).contains(&w.0));
    (valid && !grass.is_empty()).then_some(WildTables {
        grass,
        water,
        ..WildTables::default()
    })
}
