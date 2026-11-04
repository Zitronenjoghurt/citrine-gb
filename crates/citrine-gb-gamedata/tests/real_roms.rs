use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use citrine_gb_gamedata::{
    Effect, Game, Generation, Language, Location, Palette, Place, Pokemon, Position, Region,
    RomTables, SaveCopy, Side, Sprite, Version, identify, read_live, read_save,
};
use common::{map_picture, png};
use std::path::{Path, PathBuf};

mod common;

fn rom_dir() -> PathBuf {
    std::env::var_os("CITRINE_ROMS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").expect("home")).join("roms/gb"))
}

fn pokemon_roms() -> Vec<(PathBuf, Vec<u8>, Game)> {
    let mut roms: Vec<_> = std::fs::read_dir(rom_dir())
        .expect("rom dir")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let data = std::fs::read(&path).ok()?;
            let game = identify(&data)?;
            Some((path, data, game))
        })
        .collect();
    roms.sort_by(|a, b| a.0.cmp(&b.0));
    roms
}

fn expected_live(game: Game) -> Vec<(u16, usize)> {
    match game.version {
        Version::Red | Version::Blue => {
            vec![
                (0xD158, 0x2598),
                (0xD2F7, 0x25A3),
                (0xD163, 0x2F2C),
                (0xDA80, 0x30C0),
            ]
        }
        Version::Yellow => {
            vec![
                (0xD157, 0x2598),
                (0xD2F6, 0x25A3),
                (0xD162, 0x2F2C),
                (0xDA7F, 0x30C0),
            ]
        }
        Version::Gold | Version::Silver => {
            vec![(0xD1A1, 0x2009), (0xD9EE, 0x2856), (0xDA22, 0x288A)]
        }
        Version::Crystal => vec![
            (0xD47B, 0x2009),
            (0xDCA5, 0x2833),
            (0xDCD7, 0x2865),
            (0xD472, 0x3E3D),
        ],
        Version::Green => vec![
            (0xD11D, 0x2598),
            (0xD27B, 0x259E),
            (0xD123, 0x2ED5),
            (0xD9B2, 0x302D),
        ],
        Version::Ruby
        | Version::Sapphire
        | Version::Emerald
        | Version::FireRed
        | Version::LeafGreen => Vec::new(),
    }
}

#[test]
#[ignore]
fn every_pokemon_rom_loads_its_tables() {
    let roms = pokemon_roms();
    assert!(!roms.is_empty());
    for (path, data, game) in roms {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.contains("Translated") {
            assert!(RomTables::load(game, &data).is_err(), "{name}");
            continue;
        }
        let tables = RomTables::load(game, &data).unwrap_or_else(|e| panic!("{name}: {e}"));
        let dex = tables.pokedex_size();
        let mut names = std::collections::HashSet::new();
        for number in 1..=dex {
            let species = tables
                .species_for_dex(number)
                .unwrap_or_else(|| panic!("{name}: no species for {number}"));
            assert_eq!(tables.dex_number(species), Some(number), "{name}");
            let species_name = tables.species_name(species).expect("species name");
            assert!(
                !species_name.is_empty() && !species_name.contains('\u{FFFD}'),
                "{name}: {species_name}"
            );
            names.insert(species_name.to_string());
        }
        assert_eq!(names.len(), usize::from(dex), "{name}");
        let moves = if game.generation() == Generation::One {
            165
        } else {
            251
        };
        for id in 1..=moves {
            let move_name = tables.move_name(id).expect("move name");
            assert!(!move_name.contains('\u{FFFD}'), "{name}: {move_name}");
        }
        for id in 1..=0xC3u16 {
            let item = tables.item_name(id);
            assert!(
                item.is_some() || game.generation() == Generation::One,
                "{name}: item {id}"
            );
        }
        let (machines, cut) = match game.generation() {
            Generation::One => ((0xC4..=0xFAu16).collect::<Vec<_>>(), 0xC4),
            Generation::Two => (
                (0xBF..=0xF9u16)
                    .filter(|&id| id != 0xC3 && id != 0xDC)
                    .collect(),
                0xF3,
            ),
            Generation::Three => continue,
        };
        for id in machines {
            assert!(tables.machine_move(id).is_some(), "{name}: machine {id:x}");
        }
        assert_eq!(tables.machine_move(cut), tables.move_name(15), "{name}");
        if game.language == Language::English && game.generation() == Generation::Two {
            assert_eq!(tables.machine_move(0xC4), Some("ROAR"), "{name}");
            assert_eq!(tables.item_name(0xC4).as_deref(), Some("TM05"), "{name}");
            assert_eq!(tables.item_name(0xF3).as_deref(), Some("HM01"), "{name}");
        }
        assert_eq!(tables.live_map(), expected_live(game), "{name}");
        println!(
            "{name}: {game:?}, {dex} species, live map {:x?}",
            tables.live_map()
        );
    }
}

fn location(group: u8, map: u8) -> Location {
    Location {
        group,
        map,
        x: 0,
        y: 0,
        outside: None,
    }
}

#[test]
#[ignore]
fn moves_matchups_places_and_wild_read_in_every_pokemon_rom() {
    const POUND: u16 = 1;
    const WATER_GUN: u16 = 55;
    const NORMAL: u8 = 0x00;
    const GHOST: u8 = 0x08;
    const FIRE: u8 = 0x14;
    for (path, data, game) in pokemon_roms() {
        if game.language == Language::Japanese {
            continue;
        }
        let tables = RomTables::load(game, &data).expect("tables");
        let pound = tables.move_data(POUND).expect("pound");
        assert_eq!(
            (pound.type_id, pound.power, pound.pp),
            (NORMAL, 40, 35),
            "{path:?}"
        );
        assert_eq!(tables.effect(WATER_GUN, [FIRE, FIRE]), Some(Effect::Double));
        assert_eq!(tables.effect(POUND, [GHOST, GHOST]), Some(Effect::NoEffect));
        let maps: Vec<Location> = match game.generation() {
            Generation::One => (0..=0xF7).map(|map| location(0, map)).collect(),
            Generation::Two => (1..=26)
                .flat_map(|group| (1..=40).map(move |map| location(group, map)))
                .collect(),
            Generation::Three => continue,
        };
        let mut places = std::collections::BTreeSet::new();
        let mut wild = std::collections::BTreeSet::new();
        let mut lists = 0;
        for at in &maps {
            if let Some(spot) = tables.spot(at) {
                places.insert(spot.name.to_owned());
            }
            for list in tables.wild(at, None) {
                lists += 1;
                for encounter in list.encounters {
                    assert!(encounter.min_level <= encounter.max_level);
                    assert!(encounter.percent > 0);
                    wild.insert(
                        tables
                            .species_name(encounter.species)
                            .expect("name")
                            .to_owned(),
                    );
                }
            }
        }
        println!(
            "{path:?}: {} places, {lists} wild lists, {} wild species",
            places.len(),
            wild.len()
        );
        assert!(places.len() > 50 && wild.len() > 70, "{path:?}");
        if game.language != Language::English {
            continue;
        }
        match game.generation() {
            Generation::One => {
                assert_eq!(
                    tables.spot(&location(0, 0)).map(|s| s.name),
                    Some("PALLET TOWN")
                );
                let route_1: Vec<String> = tables
                    .wild(&location(0, 0x0C), None)
                    .iter()
                    .flat_map(|list| &list.encounters)
                    .filter_map(|e| tables.species_name(e.species).map(str::to_owned))
                    .collect();
                println!("  route 1: {route_1:?}");
                assert!(route_1.iter().any(|name| name == "PIDGEY"));
                assert!(places.contains("ROUTE 1") && places.contains("CERULEAN CITY"));
            }
            Generation::Two => {
                assert!(places.contains("NEW BARK TOWN") && places.contains("ROUTE 29"));
                assert!(wild.contains("SENTRET") && wild.contains("PIDGEY"));
            }
            Generation::Three => {}
        }
    }
}

fn silver_save() -> Option<Vec<u8>> {
    let path = std::env::var_os("CITRINE_SILVER_SAVE").map(PathBuf::from)?;
    std::fs::read(path).ok()
}

#[test]
#[ignore]
fn silver_save_reads_and_matches_the_live_game() {
    let Some(save) = silver_save() else {
        return;
    };
    let (_, data, game) = pokemon_roms()
        .into_iter()
        .find(|(_, _, game)| game.version == Version::Silver)
        .expect("silver rom");
    let tables = RomTables::load(game, &data).expect("tables");
    let saved = read_save(game, &save).expect("save");
    assert_eq!(saved.copy, SaveCopy::Main);
    assert!(!saved.party.is_empty());
    for pokemon in saved
        .party
        .iter()
        .chain(saved.boxes.iter().flat_map(|b| &b.pokemon))
    {
        if let Pokemon::Hatched(mon) = pokemon {
            assert!(tables.species_name(mon.species).is_some());
            for slot in &mon.moves {
                assert!(tables.move_name(slot.id).is_some());
            }
        }
    }
    for stack in saved
        .bag
        .items
        .iter()
        .chain(&saved.bag.balls)
        .chain(&saved.bag.key_items)
    {
        assert!(tables.item_name(stack.item).is_some());
    }
    println!(
        "{} badges shown, {} earned, party {}, boxes {}, dex {}/{}",
        saved.trainer.badges.len(),
        saved.badge_count(),
        saved.party.len(),
        saved.boxes.iter().map(|b| b.pokemon.len()).sum::<usize>(),
        saved.pokedex.caught.len(),
        saved.pokedex.seen.len()
    );

    let rom = Rom::new(&data);
    let mut gb = GameBoy::new_empty(GbModel::for_rom(&rom).expect("model"));
    gb.load_rom(&rom).expect("load");
    gb.put_sram_dump(SramDump::from_slice(&save), 0);
    let live = |gb: &GameBoy| {
        let wram = [
            &gb.wram_bank(0).expect("bank 0")[..],
            &gb.wram_bank(1).expect("bank 1")[..],
        ]
        .concat();
        read_live(&tables, &wram, &gb.cartridge.battery_ram())
    };
    for _ in 0..60 {
        gb.run_frame();
    }
    assert!(live(&gb).is_err());
    let mut state = None;
    for step in 0..120 {
        let button = if step % 2 == 0 {
            JoypadState::START
        } else {
            JoypadState::A
        };
        gb.press_button(button);
        for _ in 0..6 {
            gb.run_frame();
        }
        gb.release_button(button);
        for _ in 0..54 {
            gb.run_frame();
        }
        if let Ok(read) = live(&gb)
            && read.party == saved.party
        {
            state = Some(read);
            break;
        }
    }
    let state = state.expect("the live game never matched the save");
    let at = *saved
        .location
        .as_ref()
        .and_then(Position::gb)
        .expect("location");
    println!(
        "saved at {at:?}: {:?}, wild {:?}",
        tables.spot(&at),
        tables
            .wild(&at, None)
            .iter()
            .flat_map(|list| &list.encounters)
            .map(|e| (
                tables.species_name(e.species),
                e.min_level,
                e.max_level,
                e.percent
            ))
            .collect::<Vec<_>>()
    );
    println!("journey {:?}, next {:?}", saved.journey, saved.next_goal());
    println!("live clock {:?}, battle {:?}", state.clock, state.battle);
    assert!(state.clock.is_some());
    assert!(state.battle.is_none());
    assert_eq!(state.copy, SaveCopy::Live);
    assert_eq!(state.trainer.name, saved.trainer.name);
    assert_eq!(state.trainer.id, saved.trainer.id);
    assert_eq!(state.trainer.money, saved.trainer.money);
    assert_eq!(state.trainer.badges, saved.trainer.badges);
    assert_eq!(state.bag, saved.bag);
    assert_eq!(state.pokedex, saved.pokedex);
    assert_eq!(state.boxes, saved.boxes);
}

#[test]
#[ignore]
fn every_sprite_decodes_in_every_pokemon_rom() {
    let roms = pokemon_roms();
    assert!(!roms.is_empty());
    let sheets = std::env::var_os("CITRINE_SHEETS").map(PathBuf::from);
    for (path, data, game) in roms {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        if name.contains("Translated") {
            continue;
        }
        let tables = RomTables::load(game, &data).unwrap_or_else(|e| panic!("{name}: {e}"));
        let gen2 = game.generation() == Generation::Two;
        let mut fronts = Vec::new();
        let mut backs = Vec::new();
        for dex in 1..=tables.pokedex_size() {
            let species = tables.species_for_dex(dex).expect("species");
            let front = tables
                .sprite(&data, species, None, Side::Front)
                .unwrap_or_else(|e| panic!("{name}: front {dex}: {e}"));
            assert!((1..=7).contains(&front.width), "{name}: {dex}");
            assert!((1..=7).contains(&front.height), "{name}: {dex}");
            let back = tables
                .sprite(&data, species, None, Side::Back)
                .unwrap_or_else(|e| panic!("{name}: back {dex}: {e}"));
            let back_size = if gen2 { 6 } else { 4 };
            assert_eq!(
                (back.width, back.height),
                (back_size, back_size),
                "{name}: {dex}"
            );
            let base = tables.base_stats(species).expect("base stats");
            assert_eq!(u16::from(base.dex), dex, "{name}");
            assert_eq!(base.pic_size >> 4, base.pic_size & 0x0F, "{name}: {dex}");
            if !gen2 {
                assert_eq!(
                    (front.width << 4) | front.height,
                    base.pic_size,
                    "{name}: {dex}"
                );
            }
            let types = tables.species_types(species);
            assert!(!types.is_empty() && types.len() <= 2, "{name}: {dex}");
            assert!(
                types.iter().all(|t| !t.contains('\u{FFFD}')),
                "{name}: {types:?}"
            );
            let palette = tables.palette(species, false).expect("palette");
            let shiny = tables.palette(species, true).expect("shiny palette");
            if gen2 {
                fronts.push((front.clone(), palette));
                fronts.push((front, shiny));
            } else {
                fronts.push((front, palette));
            }
            backs.push((back, palette));
        }
        let mut unown = 0;
        if gen2 {
            for letter in 'A'..='Z' {
                for side in [Side::Front, Side::Back] {
                    let sprite = tables
                        .sprite(&data, 201, Some(letter), side)
                        .unwrap_or_else(|e| panic!("{name}: unown {letter}: {e}"));
                    if side == Side::Front {
                        fronts.push((sprite, tables.palette(201, false).expect("palette")));
                        unown += 1;
                    }
                }
            }
            let a = tables.sprite(&data, 201, Some('A'), Side::Front).unwrap();
            let b = tables.sprite(&data, 201, Some('B'), Side::Front).unwrap();
            assert_ne!(a, b, "{name}: unown letters");
        }
        let mut places = 0;
        if game.version == Version::Crystal {
            for location in 1..0x60u8 {
                let caught = citrine_gb_gamedata::Caught {
                    time_of_day: citrine_gb_gamedata::TimeOfDay::Day,
                    level: 5,
                    ot_gender: citrine_gb_gamedata::Gender::Male,
                    location,
                };
                match tables.caught_place(&caught) {
                    Some(Place::Landmark(place)) => {
                        assert!(
                            !place.is_empty() && !place.contains('\u{FFFD}'),
                            "{name}: {place}"
                        );
                        places += 1;
                    }
                    other => panic!("{name}: landmark {location}: {other:?}"),
                }
            }
        }
        if let Some(dir) = &sheets {
            std::fs::create_dir_all(dir).expect("sheet dir");
            let columns = if gen2 { 18 } else { 16 };
            contact_sheet(&dir.join(format!("{name}-front.png")), &fronts, columns);
            contact_sheet(&dir.join(format!("{name}-back.png")), &backs, 16);
        }
        println!(
            "{name}: {} fronts, {} backs, {unown} unown, {places} places",
            tables.pokedex_size(),
            backs.len()
        );
    }
}

fn contact_sheet(path: &Path, sprites: &[(Sprite, Palette)], columns: usize) {
    let cell = 60;
    let rows = sprites.len().div_ceil(columns);
    let (width, height) = (columns * cell, rows * cell);
    let mut rgb = vec![0x40u8; width * height * 3];
    for (index, (sprite, palette)) in sprites.iter().enumerate() {
        let left = (index % columns) * cell + (cell - sprite.width_px()) / 2;
        let top = (index / columns) * cell + cell - 2 - sprite.height_px();
        for y in 0..sprite.height_px() {
            for x in 0..sprite.width_px() {
                let at = ((top + y) * width + left + x) * 3;
                rgb[at..at + 3].copy_from_slice(&palette.rgb(sprite.pixel(x, y)));
            }
        }
    }
    std::fs::write(path, png(width, height, &rgb)).expect("write sheet");
}

#[test]
#[ignore]
fn region_maps_decode_and_mark_known_places() {
    let sheets = std::env::var_os("CITRINE_SHEETS").map(PathBuf::from);
    for (path, data, game) in pokemon_roms() {
        if path.to_string_lossy().contains("Translated") {
            continue;
        }
        let tables = RomTables::load(game, &data).expect("tables");
        let regions: &[Region] = match game.generation() {
            Generation::One => &[Region::Kanto],
            Generation::Two => &[Region::Johto, Region::Kanto],
            Generation::Three => continue,
        };
        let known: Vec<(Location, &str, Region, (u16, u16))> = match game.generation() {
            Generation::One => vec![
                (location(0, 0x00), "PALLET TOWN", Region::Kanto, (36, 100)),
                (location(0, 0x03), "CERULEAN CITY", Region::Kanto, (100, 28)),
            ],
            Generation::Two => vec![
                (location(24, 4), "NEW BARK TOWN", Region::Johto, (132, 84)),
                (location(22, 3), "CIANWOOD CITY", Region::Johto, (12, 84)),
                (location(13, 2), "PALLET TOWN", Region::Kanto, (44, 92)),
            ],
            Generation::Three => continue,
        };
        for &region in regions {
            let map = tables.region_map(region).expect("region map");
            let colors: std::collections::BTreeSet<[u8; 4]> = map.pixels.iter().copied().collect();
            println!(
                "{path:?} {region:?}: {}x{}, {} colours",
                map.width,
                map.height,
                colors.len()
            );
            if let Some(dir) = &sheets {
                let marks: Vec<(u16, u16)> = known
                    .iter()
                    .filter_map(|(at, _, _, _)| tables.marker(at))
                    .filter(|marker| std::ptr::eq(marker.map, map))
                    .map(|marker| (marker.x, marker.y))
                    .collect();
                let name = path
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .replace(' ', "_");
                std::fs::create_dir_all(dir).expect("sheets dir");
                map_picture(&dir.join(format!("{name}-{region:?}.png")), map, &marks);
            }
            assert!(
                [(160, 144), (144, 120)].contains(&(map.width, map.height)),
                "{path:?}"
            );
            assert!(colors.len() >= 4, "{path:?} {region:?}");
        }
        if game.language != Language::English {
            continue;
        }
        for (at, name, region, point) in known {
            let marker = tables.marker(&at).expect("marker");
            println!("  {name}: {:?} at {:?}", marker.name, (marker.x, marker.y));
            assert_eq!(marker.name, name, "{path:?}");
            assert!(std::ptr::eq(marker.map, tables.region_map(region).unwrap()));
            assert_eq!((marker.x, marker.y), point, "{path:?} {name}");
        }
    }
    let Some(save) = silver_save() else {
        return;
    };
    let (_, data, game) = pokemon_roms()
        .into_iter()
        .find(|(_, _, game)| game.version == Version::Silver && game.language == Language::English)
        .expect("silver rom");
    let tables = RomTables::load(game, &data).expect("tables");
    let saved = read_save(game, &save).expect("save");
    let at = *saved
        .location
        .as_ref()
        .and_then(Position::gb)
        .expect("saved location");
    let marker = tables.marker(&at).expect("lighthouse marker");
    println!(
        "  silver save: {:?} at {:?}",
        marker.name,
        (marker.x, marker.y)
    );
    assert_eq!(marker.name, "LIGHTHOUSE");
    assert!(std::ptr::eq(
        marker.map,
        tables.region_map(Region::Johto).unwrap()
    ));
}
