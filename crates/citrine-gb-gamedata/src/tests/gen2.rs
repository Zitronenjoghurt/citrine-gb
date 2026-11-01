use super::*;
pub(crate) use crate::fixtures::{crystal_rom, save, seal};
use crate::layout::{
    CRYSTAL_INTERNATIONAL, CRYSTAL_JAPANESE, GS_INTERNATIONAL, GS_JAPANESE, GS_KOREAN,
};
use crate::{Gender, Pokemon, ReadError, RomTables, SaveCopy, TimeOfDay, read_live, read_save};

#[test]
fn reads_a_gold_and_silver_save() {
    let game = game(Version::Gold, Language::English);
    let state = read_save(game, &save(&GS_INTERNATIONAL, &en("GOLD"), 11)).expect("save");
    assert_eq!(state.copy, SaveCopy::Main);
    let trainer = &state.trainer;
    assert_eq!(trainer.name, "GOLD");
    assert_eq!(trainer.id, 258);
    assert_eq!(trainer.gender, None);
    assert_eq!(trainer.money, 100_000);
    assert_eq!(trainer.mom_money, Some(1000));
    assert_eq!(trainer.coins, 500);
    assert_eq!(
        (
            trainer.play_time.hours,
            trainer.play_time.minutes,
            trainer.play_time.seconds
        ),
        (300, 30, 15)
    );
    assert_eq!(trainer.badges.len(), 8);
    assert_eq!(state.badge_count(), 4);
    let items: Vec<(u8, u8)> = state
        .bag
        .tms_hms
        .iter()
        .map(|s| (s.item, s.count))
        .collect();
    assert_eq!(items, vec![(0xBF, 1), (0xC4, 2), (0xF3, 1)]);
    assert_eq!(state.bag.items.len(), 1);
    assert_eq!(state.bag.key_items.len(), 2);
    assert_eq!(state.bag.balls[0].count, 10);
    assert!(state.pc_items.is_empty());
    assert_eq!(state.pokedex.caught, vec![152]);
    assert_eq!(state.pokedex.seen, vec![152, 155]);

    assert_eq!(state.party.len(), 2);
    assert_eq!(state.party[1], Pokemon::Egg);
    let Pokemon::Hatched(first) = &state.party[0] else {
        panic!("egg first");
    };
    assert_eq!(first.species, 0x9B);
    assert_eq!(first.held_item, Some(0x12));
    assert_eq!(first.ot_id, 54321);
    assert_eq!(first.exp, 4096);
    assert_eq!(first.level, 12);
    assert_eq!(first.hp, Some(30));
    assert_eq!(first.friendship, Some(70));
    assert_eq!(first.pokerus, Some(0x21));
    assert_eq!(first.caught, None);
    assert!(first.dvs.shiny());
    assert_eq!(
        (first.moves.len(), first.moves[0].pp, first.moves[0].pp_ups),
        (1, 30, 1)
    );
    let stats = first.stats.expect("stats");
    assert_eq!(
        (stats.max_hp, stats.special_attack, stats.special_defense),
        (31, 15, 16)
    );

    assert_eq!(state.boxes.len(), 14);
    assert_eq!(state.current_box, 1);
    assert_eq!(state.boxes[13].name.as_deref(), Some("BOX14"));
    let counts: Vec<usize> = state.boxes.iter().map(|b| b.pokemon.len()).collect();
    assert_eq!(counts, vec![1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let Pokemon::Hatched(boxed) = &state.boxes[0].pokemon[0] else {
        panic!("egg");
    };
    assert_eq!((boxed.hp, boxed.stats, boxed.status), (None, None, None));
}

#[test]
fn a_broken_main_copy_falls_back_to_the_backup() {
    let game = game(Version::Silver, Language::English);
    let mut s = save(&GS_INTERNATIONAL, &en("SILVER"), 11);
    s[0x23DB] = 0x00;
    let state = read_save(game, &s).expect("backup");
    assert_eq!(state.copy, SaveCopy::Backup);
    assert_eq!(state.trainer.money, 100_000);
    assert_eq!(state.trainer.name, "SILVER");
    assert_eq!(state.party.len(), 2);

    s[0x15C7 + 2] ^= 0xFF;
    assert_eq!(read_save(game, &s), Err(ReadError::NoValidCopy));

    let mut s = save(&GS_INTERNATIONAL, &en("SILVER"), 11);
    s[0x2D6B] = 0;
    assert_eq!(read_save(game, &s).expect("backup").copy, SaveCopy::Backup);
}

fn badges_shown(johto: u8, kanto: u8, spawns: [u8; 4]) -> (usize, usize) {
    let game = game(Version::Gold, Language::English);
    let mut s = save(&GS_INTERNATIONAL, &en("GOLD"), 11);
    s[0x23E4] = johto;
    s[0x23E5] = kanto;
    put(&mut s, 0x2856, &spawns);
    seal(&GS_INTERNATIONAL, &mut s);
    let state = read_save(game, &s).expect("save");
    (state.trainer.badges.len(), state.badge_count())
}

#[test]
fn kanto_badges_stay_hidden_until_the_player_reaches_kanto() {
    let johto_towns = [0x00, 0xC0, 0xFF, 0x03];
    assert_eq!(badges_shown(0xFF, 0x00, johto_towns), (8, 8));
    assert_eq!(badges_shown(0xFF, 0x00, [0x00, 0xE0, 0xFF, 0x03]), (8, 8));
    assert_eq!(badges_shown(0xFF, 0x00, [0x80, 0xC0, 0xFF, 0x03]), (16, 8));
    assert_eq!(badges_shown(0x7F, 0x00, [0x80, 0xC0, 0xFF, 0x03]), (8, 7));
    assert_eq!(badges_shown(0xFF, 0x05, johto_towns), (16, 10));
}

#[test]
fn reads_a_crystal_save_with_gender_and_caught_data() {
    let game = game(Version::Crystal, Language::English);
    let mut s = save(&CRYSTAL_INTERNATIONAL, &en("KRIS"), 11);
    let first = 0x2865 + 8;
    s[first + 29] = 0b1000_0101;
    s[first + 30] = 0x80 | 0x12;
    seal(&CRYSTAL_INTERNATIONAL, &mut s);
    let state = read_save(game, &s).expect("save");
    assert_eq!(state.trainer.name, "KRIS");
    assert_eq!(state.trainer.gender, Some(Gender::Female));
    let Pokemon::Hatched(mon) = &state.party[0] else {
        panic!("egg");
    };
    let caught = mon.caught.expect("caught");
    assert_eq!(caught.time_of_day, TimeOfDay::Day);
    assert_eq!(caught.level, 5);
    assert_eq!(caught.ot_gender, Gender::Female);
    assert_eq!(caught.location, 0x12);

    s[first + 29] = 0;
    s[first + 30] = 0x7E;
    seal(&CRYSTAL_INTERNATIONAL, &mut s);
    let state = read_save(game, &s).expect("gift");
    let Pokemon::Hatched(mon) = &state.party[0] else {
        panic!("egg");
    };
    let caught = mon.caught.expect("caught");
    assert_eq!((caught.time_of_day, caught.level), (TimeOfDay::Unknown, 0));
    assert_eq!(caught.location, 0x7E);

    s[0x2009] ^= 0xFF;
    let state = read_save(game, &s).expect("backup");
    assert_eq!((state.copy, state.trainer.id), (SaveCopy::Backup, 258));
}

#[test]
fn crystal_tables_and_live_state() {
    let game = game(Version::Crystal, Language::English);
    let tables = RomTables::load(game, &crystal_rom()).expect("tables");
    assert_eq!(tables.species_name(1), Some("PAA"));
    assert_eq!(tables.species_name(251), Some("PJQ"));
    assert_eq!(tables.species_name(0), None);
    assert_eq!(tables.dex_number(25), Some(25));
    assert_eq!(tables.species_for_dex(251), Some(251));
    assert_eq!(tables.move_name(251), Some("MOVEJQ"));
    assert_eq!(tables.item_name(0xBF).as_deref(), Some("THINGHI"));
    assert_eq!(
        tables.live_map(),
        vec![
            (0xD47B, 0x2009),
            (0xDCA5, 0x2833),
            (0xDCD7, 0x2865),
            (0xD472, 0x3E3D)
        ]
    );

    let saved = save(&CRYSTAL_INTERNATIONAL, &en("KRIS"), 11);
    let mut now = saved.clone();
    put(&mut now, 0x23DC, &[0x00, 0x00, 0x2A]);
    put(
        &mut now,
        CRYSTAL_INTERNATIONAL.play_time,
        &[0x00, 0x3C, 0x29, 0x07],
    );
    now[CRYSTAL_INTERNATIONAL.pokedex_caught] |= 0b0000_0110;
    now[CRYSTAL_INTERNATIONAL.pokedex_seen] |= 0b0000_1110;
    now[0x3E3D] = 0;
    let mut wram = vec![0u8; 0x2000];
    for (addr, at, len) in [
        (0xD47B, 0x2009, 0x82A),
        (0xDCA5, 0x2833, 0x32),
        (0xDCD7, 0x2865, 0x31E),
        (0xD472, 0x3E3D, 7),
    ] {
        put(&mut wram, addr - 0xC000, &now[at..at + len]);
    }
    let mut live_sram = saved.clone();
    put_list(&mut live_sram, 0x2D10, 20, 11, &[]);
    let live = read_live(&tables, &wram, &live_sram).expect("live");
    assert_eq!(live.copy, SaveCopy::Live);
    assert_eq!(live.trainer.money, 42);
    assert_eq!(
        live.trainer.play_time,
        crate::PlayTime {
            hours: 60,
            minutes: 41,
            seconds: 7
        }
    );
    assert_eq!(live.pokedex.caught, vec![2, 3, 152]);
    assert_eq!(live.pokedex.seen, vec![2, 3, 4, 152, 155]);
    assert_eq!(live.trainer.gender, Some(Gender::Male));
    assert_eq!(live.boxes[1].pokemon.len(), 0);
    assert_eq!(live.boxes[0].pokemon.len(), 1);
    assert_eq!(
        read_save(game, &live_sram).expect("save").boxes[1]
            .pokemon
            .len(),
        2
    );
    assert_eq!(
        read_live(&tables, &[0u8; 0x2000], &live_sram),
        Err(ReadError::NotRunning)
    );
}

#[test]
fn reads_japanese_gold_and_crystal_saves() {
    let hiragana = [0xB1, 0xB2, 0xB3];
    for (version, layout) in [
        (Version::Gold, &GS_JAPANESE),
        (Version::Crystal, &CRYSTAL_JAPANESE),
    ] {
        let game = game(version, Language::Japanese);
        let state = read_save(game, &save(layout, &hiragana, 6)).expect("save");
        assert_eq!(state.trainer.name, "あいう");
        assert_eq!(state.boxes.len(), 9);
        assert_eq!(state.trainer.money, 100_000);
        assert_eq!(state.party.len(), 2);
        assert_eq!(state.boxes[0].pokemon.len(), 1);
        assert_eq!(state.pokedex.seen, vec![152, 155]);
        assert_eq!(
            state.trainer.gender,
            (version == Version::Crystal).then_some(Gender::Female)
        );
    }
}

#[test]
fn reads_a_korean_save() {
    let game = game(Version::Silver, Language::Korean);
    let hangul = [0x01, 0x01, 0x0A, 0xFE, 0x85];
    let state = read_save(game, &save(&GS_KOREAN, &hangul, 11)).expect("save");
    assert_eq!(state.trainer.name, "가힝F");
    assert_eq!(state.boxes.len(), 14);
    assert_eq!(state.boxes[0].name.as_deref(), Some("BOX1"));
    assert_eq!(state.pokedex.caught, vec![152]);

    let mut s = save(&GS_KOREAN, &hangul, 11);
    s[0x2009] ^= 1;
    assert_eq!(read_save(game, &s), Err(ReadError::NoValidCopy));
}
