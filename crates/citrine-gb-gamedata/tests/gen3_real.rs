use citrine_gb_gamedata::gen3::{
    Certainty, Growth, Layout, Mon, Pokemon, RegionMaps, RomTables, Warp, Whereabouts,
    guess_layout, identify, read_save,
};
use citrine_gb_gamedata::{Game, Generation, Language, Pokemon as FoldedPokemon, Tables, Version};
use common::map_picture;
use std::path::PathBuf;

mod common;

fn dir(var: &str, fallback: &str) -> Option<PathBuf> {
    let path = std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").expect("home")).join(fallback));
    path.is_dir().then_some(path)
}

fn files(dir: &PathBuf) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("dir")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let data = std::fs::read(&path).ok()?;
            Some((path.file_name()?.to_string_lossy().to_string(), data))
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

fn gba_roms() -> Vec<(String, Vec<u8>, Game)> {
    let Some(dir) = dir("CITRINE_GBA_ROMS", "roms/gba") else {
        return Vec::new();
    };
    files(&dir)
        .into_iter()
        .filter_map(|(name, data)| {
            let game = identify(&data)?;
            Some((name, data, game))
        })
        .collect()
}

fn exp_table(rom: &[u8]) -> Option<usize> {
    let row: Vec<u8> = [0u32, 1, 8, 27, 64, 125, 216]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    rom.windows(row.len()).position(|w| w == row.as_slice())
}

#[test]
#[ignore]
fn every_gba_pokemon_rom_loads_its_tables() {
    let roms = gba_roms();
    if roms.is_empty() {
        return;
    }
    let mut versions: Vec<Version> = roms.iter().map(|(_, _, game)| game.version).collect();
    versions.sort_by_key(|v| *v as u8);
    assert_eq!(
        versions,
        vec![
            Version::Ruby,
            Version::Sapphire,
            Version::Emerald,
            Version::FireRed,
            Version::LeafGreen
        ]
    );
    for (name, data, game) in &roms {
        let tables = RomTables::load(data).unwrap_or_else(|e| panic!("{name}: {e}"));
        for national in 1..=386 {
            let species = tables
                .species_for_national(national)
                .unwrap_or_else(|| panic!("{name}: no species for {national}"));
            let species_name = tables.species_name(species).expect("name");
            assert!(!species_name.is_empty() && !species_name.contains('\u{FFFD}'));
            assert!(tables.base_stats(species).is_some(), "{name}: {national}");
        }
        for id in 1..355 {
            let move_name = tables.move_name(id).expect("move");
            assert!(!move_name.contains('\u{FFFD}'), "{name}: move {id}");
        }
        for id in 1..78 {
            assert!(tables.ability_name(id).is_some(), "{name}: ability {id}");
        }
        assert_eq!(tables.species_name(1), Some("BULBASAUR"));
        assert_eq!(tables.national_number(277), Some(252));
        assert_eq!(tables.national_number(411), Some(358));
        assert_eq!(tables.national_number(410), Some(386));
        assert_eq!(tables.species_name(410), Some("DEOXYS"));
        assert_eq!(tables.move_name(1), Some("POUND"));
        assert_eq!(tables.move_name(354), Some("PSYCHO BOOST"));
        assert_eq!(tables.ability_name(1), Some("STENCH"));
        assert_eq!(tables.ability_name(77), Some("AIR LOCK"));
        assert_eq!(tables.type_name(9), Some("???"));
        assert_eq!(tables.species_types(1), vec!["GRASS", "POISON"]);
        assert_eq!(tables.item_name(1), Some("MASTER BALL"));
        assert_eq!(tables.item_name(273), Some("POKéBLOCK CASE"));
        let bulbasaur = tables.base_stats(1).expect("bulbasaur");
        assert_eq!(
            [
                bulbasaur.hp,
                bulbasaur.attack,
                bulbasaur.defense,
                bulbasaur.speed,
                bulbasaur.special_attack,
                bulbasaur.special_defense
            ],
            [45, 49, 49, 45, 65, 65]
        );
        assert_eq!(bulbasaur.growth, Growth::MediumSlow);
        let at = exp_table(data).expect("exp table");
        for byte in 0..6u8 {
            let growth = Growth::from_byte(byte).expect("growth");
            for level in 2..=100u8 {
                let offset = at + (usize::from(byte) * 101 + usize::from(level)) * 4;
                let stored = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
                assert_eq!(
                    growth.exp(level),
                    stored,
                    "{name}: {growth:?} level {level}"
                );
            }
        }
        println!(
            "{name}: {game:?}, {} items, last {:?}, abilities 76 {:?} 77 {:?}",
            (1..400).filter(|&i| tables.item_name(i).is_some()).count(),
            (1..400).rev().find_map(|i| tables.item_name(i)),
            tables.ability_name(76),
            tables.ability_name(77)
        );
    }
}

fn describe(tables: &RomTables, mon: &Mon) -> String {
    let national = tables.national_number(mon.species).expect("national");
    let level = tables.mon_level(mon).expect("level");
    format!(
        "#{national} {} Lv{level} \"{}\" {:?} {:?} {}",
        tables.species_name(mon.species).expect("name"),
        mon.nickname,
        mon.nature(),
        tables.mon_gender(mon),
        tables.mon_ability(mon).expect("ability")
    )
}

#[test]
#[ignore]
fn every_gba_save_reads_with_the_rom_tables() {
    let Some(saves) = dir("CITRINE_GBA_SAVES", "roms/gba/saves") else {
        return;
    };
    let roms = gba_roms();
    if roms.is_empty() {
        return;
    }
    let mut read = 0;
    for (name, flash) in files(&saves) {
        let guess = guess_layout(&flash);
        println!("{name}: {} bytes, {guess:?}", flash.len());
        let Some(guess) = guess else {
            assert!(read_save(&flash, Layout::Emerald, Language::English).is_err());
            continue;
        };
        assert_eq!(guess.certainty, Certainty::Sure);
        let layout = guess.layout;
        let (_, rom, _) = roms
            .iter()
            .find(|(_, _, game)| Layout::of(game.version) == Some(layout))
            .expect("rom for the layout");
        let tables = RomTables::load(rom).expect("tables");
        let expected = if name.contains("Emerald") {
            Layout::Emerald
        } else if name.contains("Fire") || name.contains("Leaf") {
            Layout::FireRedLeafGreen
        } else {
            Layout::RubySapphire
        };
        assert_eq!(layout, expected, "{name}");
        let state = read_save(&flash, layout, Language::English).expect("save");
        read += 1;
        if name.contains("scripted") {
            assert_eq!(state.trainer.money, 3000, "{name}");
            assert_eq!(state.trainer.name, "AAAAAAA", "{name}");
            assert!(state.party.is_empty(), "{name}");
            assert_eq!(state.pc_items.len(), 1, "{name}");
            assert_eq!(tables.item_name(state.pc_items[0].item), Some("POTION"));
            assert_eq!(state.boxes[13].name, "BOX14", "{name}");
        }
        let trainer = &state.trainer;
        println!(
            "trainer {} {:?} id {} secret {} money {} coins {} time {:?}, save {} {:?}",
            trainer.name,
            trainer.gender,
            trainer.id,
            trainer.secret_id,
            trainer.money,
            trainer.coins,
            trainer.play_time,
            state.save_index,
            state.copy
        );
        println!(
            "pokedex seen {} caught {}",
            state.pokedex.seen.len(),
            state.pokedex.caught.len()
        );
        for pokemon in &state.party {
            let (Pokemon::Hatched(mon) | Pokemon::Egg(mon)) = pokemon else {
                panic!("bad egg in the party");
            };
            let party = mon.party.expect("party stats");
            let growth = tables.base_stats(mon.species).expect("base").growth;
            assert_eq!(mon.level, Some(mon.level_with(growth)), "{mon:?}");
            let mut boxed = mon.as_ref().clone();
            boxed.party = None;
            boxed.level = None;
            assert_eq!(tables.mon_stats(&boxed), Some(party.stats), "{mon:?}");
            let moves: Vec<&str> = mon
                .moves
                .iter()
                .map(|m| tables.move_name(m.id).expect("move"))
                .collect();
            println!(
                "party {} hp {}/{} item {:?} moves {moves:?}",
                describe(&tables, mon),
                party.hp,
                party.stats.max_hp,
                mon.held_item.map(|i| tables.item_name(i).expect("item"))
            );
        }
        let mut stored = 0;
        for pc_box in &state.boxes {
            let mons: Vec<String> = pc_box
                .slots
                .iter()
                .flatten()
                .map(|pokemon| match pokemon {
                    Pokemon::Hatched(mon) => describe(&tables, mon),
                    Pokemon::Egg(mon) => format!("egg {}", describe(&tables, mon)),
                    Pokemon::BadEgg => panic!("bad egg in box {}", pc_box.number),
                })
                .collect();
            stored += mons.len();
            println!("box {} \"{}\": {}", pc_box.number, pc_box.name, mons.len());
            for mon in mons.iter().take(3) {
                println!("  {mon}");
            }
        }
        println!("current box {}, {stored} in the PC", state.current_box + 1);
        let bag = &state.bag;
        for (pocket, stacks) in [
            ("items", &bag.items),
            ("key", &bag.key_items),
            ("balls", &bag.balls),
            ("tms", &bag.tms_hms),
            ("berries", &bag.berries),
            ("pc", &state.pc_items),
        ] {
            let named: Vec<String> = stacks
                .iter()
                .map(|s| {
                    let item = tables.item_name(s.item).expect("item name");
                    assert!((1..=999).contains(&s.count), "{item} x{}", s.count);
                    format!("{item} x{}", s.count)
                })
                .collect();
            println!("{pocket} {}: {}", named.len(), named.join(", "));
        }
    }
    assert!(read > 0);
}

fn standing(group: u8, map: u8, x: i16, y: i16) -> Whereabouts {
    Whereabouts {
        x,
        y,
        location: Warp { group, map, x, y },
        ..Whereabouts::default()
    }
}

#[test]
#[ignore]
fn gba_region_maps_decode_and_mark_known_places() {
    let sheets = std::env::var_os("CITRINE_SHEETS").map(PathBuf::from);
    let saves = dir("CITRINE_GBA_SAVES", "roms/gba/saves")
        .map(|dir| files(&dir))
        .unwrap_or_default();
    for (name, rom, game) in gba_roms() {
        let maps = RegionMaps::load(&rom).unwrap_or_else(|| panic!("{name}: no region map"));
        let kanto = matches!(game.version, Version::FireRed | Version::LeafGreen);
        println!(
            "{name}: {} pictures, groups {:?}",
            maps.pictures().len(),
            maps.maps_in_groups()
        );
        assert_eq!(maps.pictures().len(), if kanto { 4 } else { 1 }, "{name}");
        for picture in maps.pictures() {
            let size = if kanto { (192, 144) } else { (240, 160) };
            assert_eq!((picture.width, picture.height), size, "{name}");
            let colors: std::collections::BTreeSet<[u8; 4]> =
                picture.pixels.iter().copied().collect();
            assert!(colors.len() >= 8, "{name}: {} colours", colors.len());
        }
        let mut known: Vec<(Whereabouts, &str, usize, (u16, u16))> = if kanto {
            vec![
                (standing(3, 0, 10, 10), "PALLET TOWN", 0, (44, 108)),
                (standing(1, 47, 5, 5), "SILPH CO.", 0, (124, 68)),
            ]
        } else {
            vec![(standing(0, 9, 10, 10), "LITTLEROOT TOWN", 0, (44, 108))]
        };
        if kanto {
            assert_eq!(maps.section_name(0x58), Some("PALLET TOWN"));
            let one_island = maps.marker(&standing(3, 12, 10, 10)).expect("one island");
            assert!(std::ptr::eq(one_island.map, &maps.pictures()[1]), "{name}");
            println!(
                "  one island {:?} at {:?}",
                one_island.name,
                (one_island.x, one_island.y)
            );
        } else {
            assert_eq!(maps.section_name(0), Some("LITTLEROOT TOWN"));
        }
        for (save_name, flash) in &saves {
            let Some(guess) = guess_layout(flash) else {
                continue;
            };
            if Some(guess.layout) != Layout::of(game.version) {
                continue;
            }
            let state = read_save(flash, guess.layout, Language::English).expect("save");
            let marker = maps.marker(&state.whereabouts);
            println!(
                "  {save_name}: {:?} -> {:?}",
                state.whereabouts,
                marker.map(|m| (m.name, m.x, m.y))
            );
            if save_name.contains("Ruby") {
                assert!(marker.is_none(), "{save_name} starts inside the truck");
            } else if save_name.contains("scripted") {
                let (home, point) = if kanto {
                    ("PALLET TOWN", (44, 108))
                } else {
                    ("LITTLEROOT TOWN", (44, 108))
                };
                known.push((state.whereabouts, home, 0, point));
            }
        }
        if let Some(dir) = &sheets {
            std::fs::create_dir_all(dir).expect("sheets dir");
            for (i, picture) in maps.pictures().iter().enumerate() {
                let marks: Vec<(u16, u16)> = known
                    .iter()
                    .filter_map(|(at, _, _, _)| maps.marker(at))
                    .filter(|m| std::ptr::eq(m.map, picture))
                    .map(|m| (m.x, m.y))
                    .collect();
                let stem = name
                    .split('(')
                    .next()
                    .unwrap_or(&name)
                    .trim()
                    .replace(' ', "_");
                map_picture(&dir.join(format!("{stem}-{i}.png")), picture, &marks);
            }
        }
        for (at, place, picture, point) in known {
            let marker = maps
                .marker(&at)
                .unwrap_or_else(|| panic!("{name}: {place}"));
            assert_eq!(marker.name, place, "{name}");
            assert!(
                std::ptr::eq(marker.map, &maps.pictures()[picture]),
                "{name}"
            );
            assert_eq!((marker.x, marker.y), point, "{name} {place}");
        }
    }
}

#[test]
#[ignore]
fn every_gba_save_folds_through_the_tables() {
    let Some(saves) = dir("CITRINE_GBA_SAVES", "roms/gba/saves") else {
        return;
    };
    let roms = gba_roms();
    if roms.is_empty() {
        return;
    }
    for (_, rom, game) in &roms {
        let tables = Tables::load(rom).expect("tables");
        assert_eq!(tables.generation(), Generation::Three);
        let pound = tables.move_data(1).expect("pound");
        assert_eq!(
            (pound.type_id, pound.power, pound.pp),
            (0, 40, 35),
            "{game:?}"
        );
        assert_eq!(tables.machine_move(289), Some("FOCUS PUNCH"), "{game:?}");
        assert_eq!(tables.machine_move(346), Some("DIVE"), "{game:?}");
        assert!(tables.is_hm(339) && !tables.is_hm(338));
        for species in (1..=251).chain(277..=411) {
            for shiny in [false, true] {
                let picture = tables.front_picture(rom, species, shiny);
                let drawn = picture.map_or(0, |p| p.pixels.iter().filter(|&&i| i != 0).count());
                assert!(drawn > 64, "{game:?} species {species} shiny {shiny}");
            }
        }
    }
    let mut folded = 0;
    for (name, flash) in files(&saves) {
        let Some(guess) = guess_layout(&flash) else {
            continue;
        };
        let (_, rom, _) = roms
            .iter()
            .find(|(_, _, game)| Layout::of(game.version) == Some(guess.layout))
            .expect("rom for the layout");
        let tables = Tables::load(rom).expect("tables");
        let raw = read_save(&flash, guess.layout, Language::English).expect("save");
        let state = tables.read_save(&flash).expect("fold");
        folded += 1;
        assert_eq!(state.party.len(), raw.party.len(), "{name}");
        assert_eq!(state.trainer.name, raw.trainer.name, "{name}");
        assert_eq!(state.trainer.badges, raw.badges, "{name}");
        let marker = state.location.as_ref().and_then(|at| tables.marker(at));
        println!(
            "{name}: badges {}, dex {} champion {}, at {:?}",
            raw.badges.iter().filter(|&&b| b).count(),
            raw.pokedex_received,
            raw.champion,
            marker.map(|m| m.name)
        );
        let mons = state
            .party
            .iter()
            .chain(state.boxes.iter().flat_map(|b| b.pokemon.iter()));
        for pokemon in mons {
            let FoldedPokemon::Hatched(mon) = pokemon else {
                continue;
            };
            assert!(tables.mon_stats(mon).is_some(), "{name} {mon:?}");
            assert!(tables.mon_ability(mon).is_some(), "{name} {mon:?}");
            assert!(tables.hidden_power(mon).is_some(), "{name} {mon:?}");
            assert!(!tables.species_type_ids(mon.species).is_empty());
            for slot in &mon.moves {
                let data = tables.move_data(slot.id).expect("move data");
                assert!(
                    tables.type_name(data.type_id).is_some(),
                    "{name} move {}",
                    slot.id
                );
                let max = tables.max_pp(*slot).expect("max pp");
                assert!(
                    slot.pp <= max,
                    "{name} move {} pp {} of {max}",
                    slot.id,
                    slot.pp
                );
            }
        }
        for stack in &state.bag.tms_hms {
            assert!(
                tables.machine_move(stack.item).is_some(),
                "{name} {}",
                stack.item
            );
        }
        if let Some(FoldedPokemon::Hatched(lead)) = state.party.first() {
            let genes = lead.gen3().expect("genes");
            let (kind, power) = tables.hidden_power(lead).expect("hidden power");
            println!(
                "lead {} Lv{} {} {} hidden power {} {power}, moves {:?}",
                tables.species_name(lead.species).unwrap_or("?"),
                lead.level,
                genes.nature.name(),
                tables.mon_ability(lead).unwrap_or("?"),
                tables.type_name(kind).unwrap_or("?"),
                lead.moves
                    .iter()
                    .map(|slot| format!(
                        "{} {}/{}",
                        tables.move_name(slot.id).unwrap_or("?"),
                        slot.pp,
                        tables.max_pp(*slot).unwrap_or(0)
                    ))
                    .collect::<Vec<_>>()
            );
        }
    }
    assert!(folded > 0);
}

fn wild_names(tables: &RomTables, group: u8, map: u8) -> Vec<String> {
    let at = Warp {
        group,
        map,
        x: 0,
        y: 0,
    };
    tables
        .wild(at)
        .iter()
        .map(|list| {
            let mons: Vec<String> = list
                .encounters
                .iter()
                .map(|e| {
                    let name = tables.species_name(e.species).unwrap_or("?");
                    format!("{name} {}-{} {}%", e.min_level, e.max_level, e.percent)
                })
                .collect();
            format!("{:?}: {}", list.method, mons.join(", "))
        })
        .collect()
}

fn lists_wild_pokemon(versions: &[Version]) {
    for (name, rom, game) in gba_roms() {
        if !versions.contains(&game.version) {
            continue;
        }
        let tables = RomTables::load(&rom).expect("tables");
        let mut methods = std::collections::BTreeSet::new();
        for group in 0..40 {
            for map in 0..=255 {
                let at = Warp {
                    group,
                    map,
                    x: 0,
                    y: 0,
                };
                for list in tables.wild(at) {
                    methods.insert(format!("{:?}", list.method));
                    let total: u32 = list.encounters.iter().map(|e| u32::from(e.percent)).sum();
                    assert!((99..=102).contains(&total), "{name}: {group}.{map} {total}");
                }
            }
        }
        println!("{name}: {} maps, {methods:?}", tables.wild_maps());
        let (route, first) = match game.version {
            Version::Emerald => (
                (0, 16),
                "Grass: WURMPLE 2-3 45%, POOCHYENA 2-3 45%, ZIGZAGOON 2-3 10%",
            ),
            Version::Ruby | Version::Sapphire => (
                (0, 16),
                "Grass: WURMPLE 2-3 45%, ZIGZAGOON 2-3 45%, POOCHYENA 2-3 10%",
            ),
            Version::FireRed | Version::LeafGreen => {
                ((3, 19), "Grass: PIDGEY 2-5 50%, RATTATA 2-4 50%")
            }
            _ => unreachable!(),
        };
        assert_eq!(wild_names(&tables, route.0, route.1)[0], first, "{name}");
        let (water, only) = match game.version {
            Version::Ruby => ((0, 17), "SEEDOT"),
            Version::Sapphire | Version::Emerald => ((0, 17), "LOTAD"),
            Version::FireRed => ((3, 22), "EKANS"),
            Version::LeafGreen => ((3, 22), "SANDSHREW"),
            _ => unreachable!(),
        };
        let lists = wild_names(&tables, water.0, water.1);
        assert_eq!(lists.len(), 5, "{name}: {lists:?}");
        assert!(lists[0].contains(only), "{name}: {lists:?}");
        assert!(tables.wild_maps() > 80, "{name}");
        assert_eq!(methods.len(), 6, "{name}: {methods:?}");
    }
}

#[test]
#[ignore]
fn emerald_lists_its_wild_pokemon() {
    lists_wild_pokemon(&[Version::Emerald]);
}

#[test]
#[ignore]
fn ruby_and_sapphire_list_their_wild_pokemon() {
    lists_wild_pokemon(&[Version::Ruby, Version::Sapphire]);
}

#[test]
#[ignore]
fn firered_and_leafgreen_list_their_wild_pokemon() {
    lists_wild_pokemon(&[Version::FireRed, Version::LeafGreen]);
}
