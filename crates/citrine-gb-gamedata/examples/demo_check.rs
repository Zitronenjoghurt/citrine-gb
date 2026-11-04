use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::rom::Rom;
use citrine_gb_gamedata::{
    GameState, Pokemon, Position, RomTables, TimeOfDay, identify, read_live, read_save,
};

fn summary(tables: &RomTables, label: &str, state: &GameState) {
    let t = &state.trainer;
    println!(
        "{label}: copy {:?} trainer {} id {} {:?} money {} mom {:?} coins {} time {}h{}m badges {} {:?}",
        state.copy,
        t.name,
        t.id,
        t.gender,
        t.money,
        t.mom_money,
        t.coins,
        t.play_time.hours,
        t.play_time.minutes,
        state.badge_count(),
        t.badges
    );
    println!(
        "{label}: dex seen {} caught {} location {:?} next_goal {:?}",
        state.pokedex.seen.len(),
        state.pokedex.caught.len(),
        state.location,
        state.next_goal()
    );
    println!(
        "{label}: journey {:?}",
        state
            .journey
            .iter()
            .map(|s| format!("{:?}={}", s.milestone, s.done))
            .collect::<Vec<_>>()
    );
    for pokemon in &state.party {
        let Pokemon::Hatched(mon) = pokemon else {
            println!("{label}: party egg");
            continue;
        };
        let moves: Vec<String> = mon
            .moves
            .iter()
            .map(|slot| {
                let data = tables.move_data(slot.id);
                format!(
                    "{} {}/{} [{} pow {} acc {}]",
                    tables.move_name(slot.id).unwrap_or("?"),
                    slot.pp,
                    tables.max_pp(*slot).unwrap_or(0),
                    data.and_then(|d| tables.type_name(d.type_id))
                        .unwrap_or("?"),
                    data.map_or(0, |d| d.power),
                    data.map_or(0, |d| d.accuracy_percent()),
                )
            })
            .collect();
        let computed = tables.mon_stats(mon);
        println!(
            "{label}: {} '{}' L{} hp {:?}/{:?} status {:?} item {:?} exp {} next {:?} types {:?} stats_match {} caught {:?} | {}",
            tables.species_name(mon.species).unwrap_or("?"),
            mon.nickname,
            mon.level,
            mon.hp,
            mon.stats.map(|s| s.max_hp),
            mon.status,
            mon.held_item.and_then(|i| tables.item_name(i)),
            mon.exp,
            tables.exp_to_next_level(mon),
            tables.species_types(mon.species),
            computed == mon.stats,
            mon.caught.as_ref().and_then(|c| tables.caught_place(c)),
            moves.join(", ")
        );
    }
    for b in &state.boxes {
        if b.pokemon.is_empty() {
            continue;
        }
        let names: Vec<String> = b
            .pokemon
            .iter()
            .filter_map(|p| match p {
                Pokemon::Hatched(m) => Some(format!(
                    "{} L{}",
                    tables.species_name(m.species).unwrap_or("?"),
                    m.level
                )),
                Pokemon::Egg => None,
            })
            .collect();
        println!("{label}: box {:?} {}", b.name, names.join(", "));
    }
    let items = |stacks: &[citrine_gb_gamedata::ItemStack]| {
        stacks
            .iter()
            .map(|s| {
                let name = tables.item_name(s.item).unwrap_or_default();
                match tables.machine_move(s.item) {
                    Some(m) => format!("{name} {m} x{}", s.count),
                    None => format!("{name} x{}", s.count),
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!(
        "{label}: bag items [{}] keys [{}] balls [{}] tms [{}] pc [{}]",
        items(&state.bag.items),
        items(&state.bag.key_items),
        items(&state.bag.balls),
        items(&state.bag.tms_hms),
        items(&state.pc_items)
    );
}

fn main() {
    let path = std::env::args().nth(1).expect("rom path");
    let data = std::fs::read(&path).expect("read rom");
    let game = identify(&data).expect("identify");
    println!("== {path}: identify {game:?}");
    let tables = match RomTables::load(game, &data) {
        Ok(t) => {
            println!("tables: ok");
            t
        }
        Err(e) => {
            println!("tables: {e}");
            return;
        }
    };
    println!(
        "tables: live_map {:x?} dex_size {} types {:?}",
        tables.live_map(),
        tables.pokedex_size(),
        (0..0x1C)
            .filter_map(|i| tables.type_name(i))
            .collect::<Vec<_>>()
    );
    println!(
        "tables: species 1 {:?} 155 {:?} 251 {:?} move 1 {:?} 251 {:?} item 1 {:?} 0xF9 {:?}",
        tables.species_name(1),
        tables.species_name(155),
        tables.species_name(251),
        tables.move_name(1),
        tables.move_name(251),
        tables.item_name(1),
        tables.item_name(0xF9)
    );
    let rom = Rom::new(&data);
    let mut gb = GameBoy::new_empty(GbModel::for_rom(&rom).expect("model"));
    gb.load_rom(&rom).expect("load");
    for _ in 0..120 {
        gb.run_frame();
    }
    let sram = gb.cartridge.battery_ram().to_vec();
    let wram = [
        &gb.wram_bank(0).expect("bank 0")[..],
        &gb.wram_bank(1).expect("bank 1")[..],
    ]
    .concat();
    println!(
        "emulator: sram {} bytes, wram {} bytes",
        sram.len(),
        wram.len()
    );
    let saved = match read_save(game, &sram) {
        Ok(s) => s,
        Err(e) => {
            println!("read_save: {e}");
            return;
        }
    };
    summary(&tables, "save", &saved);
    let live = match read_live(&tables, &wram, &sram) {
        Ok(s) => s,
        Err(e) => {
            println!("read_live: {e}");
            return;
        }
    };
    summary(&tables, "live", &live);
    let mut same = live.clone();
    same.copy = saved.copy;
    same.battle = None;
    same.clock = None;
    println!(
        "live matches save (apart from copy, clock, battle): {}",
        same == saved
    );
    println!("clock: {:?}", live.clock);
    if let Some(location) = live.location.as_ref().and_then(Position::gb) {
        match tables.spot(location) {
            Some(spot) => println!(
                "spot: {} {:?} town map ({}, {})",
                spot.name, spot.region, spot.x, spot.y
            ),
            None => println!("spot: none"),
        }
        let time = live.clock.map(|c| c.time).unwrap_or(TimeOfDay::Unknown);
        for list in tables.wild(location, Some(time)) {
            let mons: Vec<String> = list
                .encounters
                .iter()
                .map(|e| {
                    format!(
                        "{} L{}-{} {}%{}",
                        tables.species_name(e.species).unwrap_or("?"),
                        e.min_level,
                        e.max_level,
                        e.percent,
                        if live
                            .pokedex
                            .caught
                            .contains(&tables.dex_number(e.species).unwrap_or(0))
                        {
                            " caught"
                        } else if live
                            .pokedex
                            .seen
                            .contains(&tables.dex_number(e.species).unwrap_or(0))
                        {
                            " seen"
                        } else {
                            " unseen"
                        }
                    )
                })
                .collect();
            println!("wild {:?} ({time:?}): {}", list.method, mons.join(", "));
        }
    }
    match &live.battle {
        None => println!("battle: none"),
        Some(battle) => {
            let fighter = |f: &citrine_gb_gamedata::Fighter| {
                format!(
                    "{} L{} hp {}/{} status {} types {:?}",
                    tables.species_name(f.species).unwrap_or("?"),
                    f.level,
                    f.hp,
                    f.max_hp,
                    f.status,
                    f.types.map(|t| tables.type_name(t).unwrap_or("?"))
                )
            };
            println!("battle: {:?} enemy {}", battle.kind, fighter(&battle.enemy));
            println!("battle: player {}", fighter(&battle.player));
            for slot in &battle.player.moves {
                println!(
                    "battle: {} {}/{} vs enemy {:?}",
                    tables.move_name(slot.id).unwrap_or("?"),
                    slot.pp,
                    tables.max_pp(*slot).unwrap_or(0),
                    tables.effect(slot.id, battle.enemy.types)
                );
            }
        }
    }
}
