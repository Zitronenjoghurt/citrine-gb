use super::region_map::{Warp, rom_pointer};
use crate::wild::{Method, Slot, WildList, merge};

const HEADER: usize = 20;
const INFO_PADDING: [u8; 3] = [0, 0, 0];
const MIN_HEADERS: usize = 40;
const LAST_SPECIES: u16 = 411;
const MAX_LEVEL: u8 = 100;
const LAND_CHANCES: [u16; 12] = [20, 20, 10, 10, 10, 10, 5, 5, 4, 4, 1, 1];
const WATER_CHANCES: [u16; 5] = [60, 30, 5, 4, 1];
const FISHING_SLOTS: usize = 10;
const RODS: [(Method, usize, &[u16]); 3] = [
    (Method::OldRod, 0, &[70, 30]),
    (Method::GoodRod, 2, &[60, 20, 20]),
    (Method::SuperRod, 5, &[40, 40, 15, 4, 1]),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Info {
    rate: u8,
    slots: Vec<Slot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Header {
    group: u8,
    map: u8,
    land: Option<Info>,
    water: Option<Info>,
    rock_smash: Option<Info>,
    fishing: Option<Info>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct WildTables {
    headers: Vec<Header>,
}

impl WildTables {
    pub fn load(rom: &[u8]) -> Self {
        Self {
            headers: find_headers(rom).unwrap_or_default(),
        }
    }

    pub fn len(&self) -> usize {
        self.headers.len()
    }

    pub fn here(&self, warp: Warp) -> Vec<WildList> {
        let Some(header) = self
            .headers
            .iter()
            .find(|h| h.group == warp.group && h.map == warp.map)
        else {
            return Vec::new();
        };
        let mut lists = Vec::new();
        let mut push = |method: Method, slots: &[Slot], chances: &[u16]| {
            let encounters = merge(slots, chances);
            if !encounters.is_empty() {
                lists.push(WildList { method, encounters });
            }
        };
        if let Some(land) = live(&header.land) {
            push(Method::Grass, &land.slots, &LAND_CHANCES);
        }
        if let Some(water) = live(&header.water) {
            push(Method::Water, &water.slots, &WATER_CHANCES);
        }
        if let Some(fishing) = live(&header.fishing) {
            for (method, first, chances) in RODS {
                push(
                    method,
                    &fishing.slots[first..first + chances.len()],
                    chances,
                );
            }
        }
        if let Some(rock_smash) = live(&header.rock_smash) {
            push(Method::RockSmash, &rock_smash.slots, &WATER_CHANCES);
        }
        lists
    }
}

fn live(info: &Option<Info>) -> Option<&Info> {
    info.as_ref().filter(|info| info.rate > 0)
}

fn find_headers(rom: &[u8]) -> Option<Vec<Header>> {
    let mut best: Vec<Header> = Vec::new();
    for start in (0..rom.len().saturating_sub(HEADER)).step_by(4) {
        if start >= HEADER && read_header(rom, start - HEADER).is_some() {
            continue;
        }
        let headers: Vec<Header> = (start..)
            .step_by(HEADER)
            .map_while(|at| read_header(rom, at))
            .collect();
        if headers.len() > best.len() {
            best = headers;
        }
    }
    (best.len() >= MIN_HEADERS).then_some(best)
}

fn read_header(rom: &[u8], at: usize) -> Option<Header> {
    let bytes = rom.get(at..at + HEADER)?;
    if bytes[0] == 0xFF || bytes[2] != 0 || bytes[3] != 0 {
        return None;
    }
    let info = |index: usize, count: usize| -> Option<Option<Info>> {
        let field = at + 4 + index * 4;
        if rom.get(field..field + 4)? == [0, 0, 0, 0] {
            return Some(None);
        }
        read_info(rom, rom_pointer(rom, field)?, count).map(Some)
    };
    let header = Header {
        group: bytes[0],
        map: bytes[1],
        land: info(0, LAND_CHANCES.len())?,
        water: info(1, WATER_CHANCES.len())?,
        rock_smash: info(2, WATER_CHANCES.len())?,
        fishing: info(3, FISHING_SLOTS)?,
    };
    [
        &header.land,
        &header.water,
        &header.rock_smash,
        &header.fishing,
    ]
    .iter()
    .any(|info| info.is_some())
    .then_some(header)
}

fn read_info(rom: &[u8], at: usize, count: usize) -> Option<Info> {
    if !at.is_multiple_of(4) || rom.get(at + 1..at + 4)? != INFO_PADDING {
        return None;
    }
    let mons = rom_pointer(rom, at + 4)?;
    let slots = rom
        .get(mons..mons + count * 4)?
        .as_chunks::<4>()
        .0
        .iter()
        .map(|mon| {
            let species = u16::from_le_bytes([mon[2], mon[3]]);
            let valid = (1..=mon[1]).contains(&mon[0])
                && mon[1] <= MAX_LEVEL
                && (1..=LAST_SPECIES).contains(&species);
            valid.then_some((mon[0], mon[1], species))
        })
        .collect::<Option<Vec<Slot>>>()?;
    Some(Info {
        rate: rom[at],
        slots,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wild::Encounter;

    const BASE: u32 = 0x0800_0000;
    const TABLE: usize = 0x1000;
    const DATA: usize = 0x4000;

    fn put_u32(rom: &mut [u8], at: usize, value: u32) {
        rom[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_info(rom: &mut Vec<u8>, rate: u8, mons: &[(u8, u8, u16)]) -> u32 {
        let at = rom.len();
        rom.extend([rate, 0, 0, 0]);
        rom.extend((BASE + at as u32 + 8).to_le_bytes());
        for &(min, max, species) in mons {
            rom.extend([min, max]);
            rom.extend(species.to_le_bytes());
        }
        BASE + at as u32
    }

    fn rom() -> Vec<u8> {
        let mut rom = vec![0u8; DATA];
        let land = put_info(&mut rom, 20, &[(2, 3, 290); 12]);
        let mut fish = vec![(5, 10, 129), (5, 10, 129), (10, 30, 129), (10, 30, 72)];
        fish.extend([(10, 30, 318), (25, 30, 72), (30, 35, 72), (25, 30, 72)]);
        fish.extend([(20, 25, 72), (30, 35, 72)]);
        let fishing = put_info(&mut rom, 10, &fish);
        let mut water = vec![(5, 35, 72); 4];
        water.push((5, 35, 278));
        let water = put_info(&mut rom, 4, &water);
        let rock = put_info(&mut rom, 0, &[(5, 10, 74); 5]);
        for i in 0..MIN_HEADERS {
            let at = TABLE + i * HEADER;
            rom[at] = 0;
            rom[at + 1] = i as u8;
            put_u32(&mut rom, at + 4, land);
            if i == 3 {
                put_u32(&mut rom, at + 8, water);
                put_u32(&mut rom, at + 12, rock);
                put_u32(&mut rom, at + 16, fishing);
            }
        }
        rom[TABLE + MIN_HEADERS * HEADER] = 0xFF;
        rom[TABLE + MIN_HEADERS * HEADER + 1] = 0xFF;
        rom
    }

    fn warp(map: u8) -> Warp {
        Warp {
            group: 0,
            map,
            x: 0,
            y: 0,
        }
    }

    fn mon(species: u16, min_level: u8, max_level: u8, percent: u8) -> Encounter {
        Encounter {
            species,
            min_level,
            max_level,
            percent,
        }
    }

    #[test]
    fn reads_every_header_up_to_the_end_mark() {
        assert_eq!(WildTables::load(&rom()).len(), MIN_HEADERS);
    }

    #[test]
    fn lists_land_water_and_each_rod_but_not_a_closed_rock_smash() {
        let lists = WildTables::load(&rom()).here(warp(3));
        let methods: Vec<Method> = lists.iter().map(|list| list.method).collect();
        assert_eq!(
            methods,
            [
                Method::Grass,
                Method::Water,
                Method::OldRod,
                Method::GoodRod,
                Method::SuperRod
            ]
        );
        assert_eq!(lists[0].encounters, [mon(290, 2, 3, 100)]);
        assert_eq!(
            lists[1].encounters,
            [mon(72, 5, 35, 99), mon(278, 5, 35, 1)]
        );
        assert_eq!(lists[2].encounters, [mon(129, 5, 10, 100)]);
        assert_eq!(
            lists[3].encounters,
            [
                mon(129, 10, 30, 60),
                mon(72, 10, 30, 20),
                mon(318, 10, 30, 20)
            ]
        );
        assert_eq!(lists[4].encounters, [mon(72, 20, 35, 100)]);
    }

    #[test]
    fn a_map_without_a_header_has_no_list() {
        let tables = WildTables::load(&rom());
        assert!(tables.here(warp(MIN_HEADERS as u8)).is_empty());
        assert_eq!(tables.here(warp(0)).len(), 1);
    }

    #[test]
    fn a_short_run_is_not_the_table() {
        let mut rom = rom();
        rom[TABLE + 20 * HEADER] = 0xFF;
        assert!(WildTables::load(&rom).here(warp(0)).is_empty());
    }
}
