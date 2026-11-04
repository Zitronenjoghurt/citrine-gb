use super::gen1::{international_save, red_rom, seal_international};
use super::gen2::crystal_rom;
use super::*;
use crate::layout::CRYSTAL_INTERNATIONAL;
use crate::{
    Encounter, Location, Method, Milestone, Position, Region, RomTables, Spot, Step, TimeOfDay,
    WildList, read_save,
};

pub(crate) fn at(bank: usize, addr: u16) -> usize {
    bank * 0x4000 + usize::from(addr) - 0x4000
}

fn text(name: &str) -> Vec<u8> {
    let mut bytes = en(name);
    bytes.push(0x50);
    bytes
}

fn gen1_places(rom: &mut [u8]) {
    let bank = 0x1C;
    put(
        rom,
        at(bank, 0x4000),
        &[
            0xFE, 0x25, 0x38, 0x0B, 0x01, 0x04, 0x00, 0x21, 0x00, 0x52, 0xBE, 0x38, 0x03, 0x09,
            0x18, 0xFA, 0x23, 0x18, 0x03, 0x21, 0x00, 0x50, 0x4F, 0x06, 0x00, 0x09, 0x09, 0x09,
        ],
    );
    put(rom, at(bank, 0x6000), &text("PALLET TOWN"));
    put(rom, at(bank, 0x6010), &text("ROUTE 1"));
    put(rom, at(bank, 0x6020), &text("PLACE"));
    put(rom, at(bank, 0x6030), &text("OAK LAB"));
    put(rom, at(bank, 0x6040), &text("MT MOON"));
    for map in 0..0x25u16 {
        let (coords, name): (u8, u16) = match map {
            0 => (0xB2, 0x6000),
            0x0C => (0x92, 0x6010),
            _ => (0x00, 0x6020),
        };
        let mut entry = vec![coords];
        entry.extend(name.to_le_bytes());
        put(rom, at(bank, 0x5000) + 3 * usize::from(map), &entry);
    }
    put(
        rom,
        at(bank, 0x5200),
        &[0x30, 0xB2, 0x30, 0x60, 0x40, 0x26, 0x40, 0x60, 0xFF],
    );
}

pub(crate) fn gen1_wild(rom: &mut [u8]) {
    let bank = 0x1D;
    put(
        rom,
        at(bank, 0x4000),
        &[
            0x21, 0x00, 0x41, 0xFA, 0x5E, 0xD3, 0x4F, 0x06, 0x00, 0x09, 0x09, 0x2A, 0x66, 0x6F,
            0x2A, 0xEA, 0x87, 0xD8, 0xA7, 0x28, 0x0A,
        ],
    );
    let mut pointers = Vec::new();
    for map in 0..0x30 {
        let pointer: u16 = if map == 0x0C { 0x4200 } else { 0x4300 };
        pointers.extend(pointer.to_le_bytes());
    }
    pointers.extend([0xFF, 0xFF]);
    put(rom, at(bank, 0x4100), &pointers);
    let mut grass = vec![25];
    for (level, species) in [
        (3, 0x24),
        (3, 0xA5),
        (4, 0x24),
        (4, 0xA5),
        (2, 0x24),
        (5, 0xA5),
        (5, 0x24),
        (6, 0xA5),
        (3, 0x24),
        (7, 0x24),
    ] {
        grass.extend([level, species]);
    }
    grass.push(0);
    put(rom, at(bank, 0x4200), &grass);
}

pub(crate) fn red_world_rom() -> Vec<u8> {
    let mut rom = red_rom();
    gen1_places(&mut rom);
    gen1_wild(&mut rom);
    rom
}

pub(crate) fn red_world() -> RomTables {
    RomTables::load(game(Version::Red, Language::English), &red_world_rom()).expect("tables")
}

fn gen2_places(rom: &mut [u8]) {
    let bank = 0x61;
    put(
        rom,
        at(bank, 0x4000),
        &[
            0x6B, 0x26, 0x00, 0x29, 0x29, 0x11, 0x02, 0x41, 0x19, 0x2A, 0x66, 0x6F,
        ],
    );
    put(rom, at(bank, 0x5000), &text("SPOT"));
    put(rom, at(bank, 0x5010), &text("NEW BARK TOWN"));
    put(rom, at(bank, 0x5020), &text("ROUTE 29"));
    put(rom, at(bank, 0x5030), &text("PALLET TOWN"));
    for i in 0..0x60u8 {
        let name: u16 = match i {
            1 => 0x5010,
            3 => 0x5020,
            0x2F => 0x5030,
            _ => 0x5000,
        };
        let mut entry = vec![8 + i, 16 + i];
        entry.extend(name.to_le_bytes());
        put(rom, at(bank, 0x4100) + 4 * usize::from(i), &entry);
    }
    put(
        rom,
        0x3800,
        &[
            0xC5, 0x05, 0x48, 0x06, 0x00, 0x21, 0x00, 0x40, 0x09, 0x09, 0x2A, 0x66, 0x6F, 0xC1,
            0x0D,
        ],
    );
    let bank = 0x62;
    for group in 0..26u16 {
        let start = 0x4100 + group * 0x40;
        put(
            rom,
            at(bank, 0x4000) + 2 * usize::from(group),
            &start.to_le_bytes(),
        );
        let landmarks: [u8; 3] = match group + 1 {
            24 => [1, 3, 0],
            1 => [2, 0x2F, 2],
            _ => [2, 2, 2],
        };
        for (i, landmark) in landmarks.into_iter().enumerate() {
            put(
                rom,
                at(bank, start) + 9 * i,
                &[0x25, 1, 1, 0x00, 0x40, landmark, 0x10, 0, 0],
            );
        }
    }
}

fn gen2_wild(rom: &mut [u8]) {
    let bank = 0x63;
    put(
        rom,
        at(bank, 0x4000),
        &[
            0x21, 0x00, 0x41, 0x11, 0x00, 0x42, 0xCD, 0x00, 0x30, 0x01, 0x2F, 0x00,
        ],
    );
    put(
        rom,
        at(bank, 0x4010),
        &[
            0x21, 0x00, 0x43, 0x11, 0x00, 0x44, 0xCD, 0x00, 0x30, 0x01, 0x09, 0x00,
        ],
    );
    let mut johto = vec![24, 2, 10, 10, 5];
    let morning = [(2, 0x10); 7];
    let day = [
        (3, 0x10),
        (3, 0x13),
        (4, 0x10),
        (5, 0x13),
        (4, 0x10),
        (6, 0x10),
        (7, 0xA1),
    ];
    let night: Vec<(u8, u8)> = (3..10).map(|level| (level, 0xA3)).collect();
    for (level, species) in morning.iter().chain(&day).chain(&night) {
        johto.extend([*level, *species]);
    }
    johto.push(0xFF);
    put(rom, at(bank, 0x4100), &johto);
    let mut kanto = vec![1, 1, 10, 10, 10];
    kanto.extend([5, 0x13].repeat(21));
    kanto.push(0xFF);
    put(rom, at(bank, 0x4200), &kanto);
    put(
        rom,
        at(bank, 0x4300),
        &[24, 2, 4, 20, 0x81, 15, 0x81, 20, 0x48, 0xFF],
    );
    put(rom, at(bank, 0x4400), &[0xFF]);
}

pub(crate) fn crystal_world_rom() -> Vec<u8> {
    let mut rom = crystal_rom();
    gen2_places(&mut rom);
    gen2_wild(&mut rom);
    rom
}

pub(crate) fn crystal_world() -> RomTables {
    RomTables::load(
        game(Version::Crystal, Language::English),
        &crystal_world_rom(),
    )
    .expect("tables")
}

pub(crate) fn location(group: u8, map: u8, outside: Option<(u8, u8)>) -> Location {
    Location {
        group,
        map,
        x: 0,
        y: 0,
        outside,
    }
}

fn encounter(species: u16, min_level: u8, max_level: u8, percent: u8) -> Encounter {
    Encounter {
        species,
        min_level,
        max_level,
        percent,
    }
}

#[test]
fn gen1_places_come_from_the_town_map_entries() {
    let tables = red_world();
    let spot = |map| tables.spot(&location(0, map, None));
    assert_eq!(
        spot(0),
        Some(Spot {
            name: "PALLET TOWN",
            region: Region::Kanto,
            x: 2,
            y: 11
        })
    );
    assert_eq!(spot(0x0C).map(|s| s.name), Some("ROUTE 1"));
    assert_eq!(spot(0x28).map(|s| s.name), Some("OAK LAB"));
    assert_eq!(
        spot(0x30).map(|s| (s.name, s.x, s.y)),
        Some(("MT MOON", 6, 2))
    );
    assert_eq!(spot(0x40), None);
}

#[test]
fn gen1_wild_pokemon_come_from_the_map_table() {
    let tables = red_world();
    assert_eq!(
        tables.wild(&location(0, 0x0C, None), None),
        vec![WildList {
            method: Method::Grass,
            encounters: vec![encounter(0x24, 2, 7, 55), encounter(0xA5, 3, 6, 45)],
        }]
    );
    assert!(tables.wild(&location(0, 0x00, None), None).is_empty());
    assert!(tables.wild(&location(0, 0x80, None), None).is_empty());
}

#[test]
fn gen2_places_follow_the_map_headers_and_landmarks() {
    let tables = crystal_world();
    assert_eq!(
        tables.spot(&location(24, 2, None)),
        Some(Spot {
            name: "ROUTE 29",
            region: Region::Johto,
            x: 11,
            y: 19
        })
    );
    assert_eq!(
        tables.spot(&location(24, 3, Some((24, 1)))).map(|s| s.name),
        Some("NEW BARK TOWN")
    );
    assert_eq!(tables.spot(&location(24, 3, None)), None);
    assert_eq!(
        tables
            .spot(&location(1, 2, None))
            .map(|s| (s.name, s.region)),
        Some(("PALLET TOWN", Region::Kanto))
    );
    assert_eq!(tables.spot(&location(27, 1, None)), None);
    assert_eq!(tables.spot(&location(24, 9, None)), None);
}

#[test]
fn gen2_wild_pokemon_follow_the_time_of_day() {
    let tables = crystal_world();
    let here = location(24, 2, None);
    let water = WildList {
        method: Method::Water,
        encounters: vec![encounter(0x81, 15, 20, 90), encounter(0x48, 20, 20, 10)],
    };
    assert_eq!(
        tables.wild(&here, Some(TimeOfDay::Day)),
        vec![
            WildList {
                method: Method::Grass,
                encounters: vec![
                    encounter(0x10, 3, 6, 59),
                    encounter(0x13, 3, 5, 40),
                    encounter(0xA1, 7, 7, 1)
                ],
            },
            water.clone()
        ]
    );
    assert_eq!(
        tables.wild(&here, Some(TimeOfDay::Night))[0].encounters,
        vec![encounter(0xA3, 3, 9, 100)]
    );
    assert_eq!(
        tables.wild(&here, Some(TimeOfDay::Morning))[0].encounters,
        vec![encounter(0x10, 2, 2, 100)]
    );
    assert_eq!(
        tables.wild(&here, None),
        tables.wild(&here, Some(TimeOfDay::Day))
    );
    assert_eq!(
        tables.wild(&location(1, 1, None), None)[0].encounters,
        vec![encounter(0x13, 5, 5, 100)]
    );
    assert!(tables.wild(&location(2, 1, None), None).is_empty());
}

#[test]
fn a_gen1_save_says_where_the_player_is_and_what_is_done() {
    let mut save = international_save();
    put(&mut save, 0x260A, &[0x0C, 0, 0, 5, 7]);
    save[0x29F3 + 4] |= 1 << 5;
    seal_international(&mut save);
    let state = read_save(game(Version::Red, Language::English), &save).expect("save");
    assert_eq!(
        state.location,
        Some(Position::Gb(Location {
            group: 0,
            map: 0x0C,
            x: 7,
            y: 5,
            outside: None
        }))
    );
    assert_eq!(state.hall_of_fame, Some(0));
    let done: Vec<bool> = state.journey.iter().map(|s| s.done).collect();
    assert_eq!(
        done,
        [
            true, true, false, true, false, false, false, false, false, false
        ]
    );
    assert_eq!(state.next_goal(), Some(Milestone::Badge(1)));

    save[0x284E] = 2;
    save[0x2602] = 0xFF;
    seal_international(&mut save);
    let state = read_save(game(Version::Red, Language::English), &save).expect("save");
    assert_eq!(state.hall_of_fame, Some(2));
    assert_eq!(state.next_goal(), None);
    assert_eq!(
        state.journey.last(),
        Some(&Step {
            milestone: Milestone::EliteFour,
            done: true
        })
    );
}

#[test]
fn a_gen2_save_says_where_the_player_is_and_what_is_done() {
    let layout = &CRYSTAL_INTERNATIONAL;
    let mut save = crystal_save();
    put(&mut save, 0x2843, &[24, 2, 7, 9]);
    put(&mut save, 0x283B, &[24, 1]);
    save[0x23DA] |= 1;
    seal(layout, &mut save);
    let state = read_save(game(Version::Crystal, Language::English), &save).expect("save");
    assert_eq!(
        state.location,
        Some(Position::Gb(Location {
            group: 24,
            map: 2,
            x: 9,
            y: 7,
            outside: Some((24, 1))
        }))
    );
    let milestones: Vec<Milestone> = state.journey.iter().map(|s| s.milestone).collect();
    assert_eq!(milestones.len(), 10);
    assert_eq!(milestones[0], Milestone::Pokedex);
    assert_eq!(milestones[9], Milestone::EliteFour);
    assert_eq!(state.next_goal(), Some(Milestone::Badge(4)));

    save[layout.johto_badges] = 0xFF;
    save[0x2600 + 8] |= 1 << 4;
    save[layout.visited_spawns] |= 0x80;
    seal(layout, &mut save);
    let state = read_save(game(Version::Crystal, Language::English), &save).expect("save");
    assert_eq!(state.journey.len(), 18);
    assert_eq!(state.next_goal(), Some(Milestone::Badge(8)));
}
