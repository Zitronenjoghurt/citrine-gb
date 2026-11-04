use super::*;
use crate::gen1::checksum;
use crate::{Genes, Pokemon, ReadError, RomTables, SaveCopy, read_live, read_save};

pub(crate) fn mon(species: u8, level: u8, party: bool) -> Vec<u8> {
    let mut m = vec![0u8; if party { 44 } else { 33 }];
    m[0] = species;
    m[1..3].copy_from_slice(&35u16.to_be_bytes());
    m[3] = level;
    m[8] = 0x21;
    m[9] = 0x2D;
    m[12..14].copy_from_slice(&12345u16.to_be_bytes());
    m[14..17].copy_from_slice(&[0x00, 0x01, 0x2C]);
    m[17..19].copy_from_slice(&500u16.to_be_bytes());
    m[27] = 0xA5;
    m[28] = 0x3C;
    m[29] = 0x23;
    m[30] = 0xC5;
    if party {
        m[33] = level;
        m[34..36].copy_from_slice(&40u16.to_be_bytes());
        m[36..38].copy_from_slice(&21u16.to_be_bytes());
        m[38..40].copy_from_slice(&22u16.to_be_bytes());
        m[40..42].copy_from_slice(&23u16.to_be_bytes());
        m[42..44].copy_from_slice(&24u16.to_be_bytes());
    }
    m
}

fn entry(species: u8, level: u8, party: bool, nickname: &str) -> TestMon {
    TestMon {
        listed: species,
        mon: mon(species, level, party),
        ot: en("RED"),
        nickname: en(nickname),
    }
}

fn empty_banks(save: &mut [u8], boxes_per_bank: usize, box_size: usize) {
    for bank in [0x4000, 0x6000] {
        for i in 0..boxes_per_bank {
            save[bank + i * box_size] = 0;
            save[bank + i * box_size + 1] = 0xFF;
        }
    }
}

pub(crate) fn international_save() -> Vec<u8> {
    let mut s = vec![0u8; 0x8000];
    put(&mut s, 0x2598, &name(&en("RED"), 11));
    s[0x25A3] = 0b0000_0001;
    s[0x25A3 + 3] = 0b0000_0001;
    s[0x25B6] = 0b0000_1001;
    s[0x25B6 + 3] = 0b0000_0001;
    s[0x25B6 + 18] = 0b0100_0000;
    put(&mut s, 0x25C9, &[2, 0x04, 5, 0x14, 3, 0xFF]);
    put(&mut s, 0x25F3, &[0x01, 0x23, 0x45]);
    s[0x2602] = 0b0000_0101;
    put(&mut s, 0x2605, &[0x30, 0x39]);
    put(&mut s, 0x27E6, &[1, 0x0A, 1, 0xFF]);
    s[0x284C] = 0x80 | 2;
    put(&mut s, 0x2850, &[0x09, 0x99]);
    put(&mut s, 0x2CED, &[12, 0, 34, 56, 7]);
    put_list(
        &mut s,
        0x2F2C,
        6,
        11,
        &[entry(0x99, 5, true, "BULB"), entry(0x54, 7, true, "PIKA")],
    );
    put_list(&mut s, 0x30C0, 20, 11, &[entry(0x24, 3, false, "BIRD")]);
    empty_banks(&mut s, 6, 0x462);
    put_list(&mut s, 0x4000, 20, 11, &[entry(0x05, 9, false, "ONE")]);
    put_list(
        &mut s,
        0x6000 + 0x462,
        20,
        11,
        &[
            entry(0x06, 10, false, "TWO"),
            entry(0x07, 11, false, "THREE"),
        ],
    );
    seal_international(&mut s);
    s
}

pub(crate) fn seal_international(save: &mut [u8]) {
    save[0x3523] = checksum(&save[0x2598..0x3523]);
}

#[test]
fn reads_an_international_save() {
    let game = game(Version::Red, Language::English);
    let state = read_save(game, &international_save()).expect("save");
    assert_eq!(state.copy, SaveCopy::Main);
    let trainer = &state.trainer;
    assert_eq!(trainer.name, "RED");
    assert_eq!(trainer.id, 12345);
    assert_eq!(trainer.money, 12345);
    assert_eq!(trainer.coins, 999);
    assert_eq!(
        (
            trainer.play_time.hours,
            trainer.play_time.minutes,
            trainer.play_time.seconds
        ),
        (12, 34, 56)
    );
    assert_eq!(
        trainer.badges,
        vec![true, false, true, false, false, false, false, false]
    );
    assert_eq!(state.badge_count(), 2);
    assert_eq!(state.pokedex.caught, vec![1, 25]);
    assert_eq!(state.pokedex.seen, vec![1, 4, 25, 151]);
    assert_eq!(state.bag.items.len(), 2);
    assert_eq!(
        (state.bag.items[1].item, state.bag.items[1].count),
        (0x14, 3)
    );
    assert_eq!(state.pc_items.len(), 1);

    assert_eq!(state.party.len(), 2);
    let Pokemon::Hatched(first) = &state.party[0] else {
        panic!("egg in gen 1");
    };
    assert_eq!(first.species, 0x99);
    assert_eq!(first.nickname, "BULB");
    assert_eq!(first.ot_name, "RED");
    assert_eq!(first.ot_id, 12345);
    assert_eq!(first.level, 5);
    assert_eq!(first.exp, 300);
    assert_eq!(first.hp, Some(35));
    let Genes::Gb { dvs, stat_exp } = first.genes else {
        panic!("gb genes");
    };
    assert_eq!(stat_exp[0], 500);
    assert_eq!(dvs.attack, 0xA);
    assert_eq!(dvs.defense, 0x5);
    assert_eq!(dvs.hp(), 0b0110);
    assert_eq!(first.moves.len(), 2);
    assert_eq!(
        (first.moves[0].id, first.moves[0].pp, first.moves[0].pp_ups),
        (0x21, 35, 0)
    );
    assert_eq!((first.moves[1].pp, first.moves[1].pp_ups), (5, 3));
    let stats = first.stats.expect("party stats");
    assert_eq!(
        (stats.max_hp, stats.special_attack, stats.special_defense),
        (40, 24, 24)
    );

    assert_eq!(state.current_box, 2);
    assert_eq!(state.boxes.len(), 12);
    let counts: Vec<usize> = state.boxes.iter().map(|b| b.pokemon.len()).collect();
    assert_eq!(counts, vec![1, 0, 1, 0, 0, 0, 0, 2, 0, 0, 0, 0]);
    let Pokemon::Hatched(bird) = &state.boxes[2].pokemon[0] else {
        panic!("egg");
    };
    assert_eq!(
        (bird.level, bird.stats, bird.nickname.as_str()),
        (3, None, "BIRD")
    );
}

#[test]
fn a_wrong_checksum_is_refused() {
    let game = game(Version::Blue, Language::English);
    let mut save = international_save();
    save[0x25F3] = 0x99;
    assert_eq!(read_save(game, &save), Err(ReadError::NoValidCopy));
    assert_eq!(read_save(game, &save[..0x4000]), Err(ReadError::TooShort));
    assert_eq!(
        read_save(game, &[0xFF; 0x8000]),
        Err(ReadError::NoValidCopy)
    );
}

#[test]
fn boxes_stay_empty_until_the_player_first_changes_box() {
    let game = game(Version::Yellow, Language::English);
    let mut save = international_save();
    save[0x284C] = 2;
    save[0x4000] = 0x37;
    seal_international(&mut save);
    let state = read_save(game, &save).expect("save");
    let counts: Vec<usize> = state.boxes.iter().map(|b| b.pokemon.len()).collect();
    assert_eq!(counts, vec![0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn reads_a_japanese_save() {
    let game = game(Version::Green, Language::Japanese);
    let mut s = vec![0u8; 0x8000];
    put(&mut s, 0x2598, &name(&[0xD7, 0xDE, 0xD8], 6));
    put(&mut s, 0x25C4, &[1, 0x04, 9, 0xFF]);
    put(&mut s, 0x25EE, &[0x00, 0x30, 0x00]);
    s[0x25F8] = 0xFF;
    put(&mut s, 0x25FB, &[0x00, 0x07]);
    put(&mut s, 0x27DC, &[0, 0xFF]);
    s[0x2842] = 0x80 | 5;
    put(&mut s, 0x2CA0, &[3, 0, 4, 5, 0]);
    let jp = |species: u8, nickname: &[u8]| TestMon {
        listed: species,
        mon: mon(species, 20, true),
        ot: vec![0x80, 0xD8],
        nickname: nickname.to_vec(),
    };
    put_list(&mut s, 0x2ED5, 6, 6, &[jp(0x54, &[0x41, 0xE5, 0x9B])]);
    empty_banks(&mut s, 4, 0x566);
    let boxed: Vec<TestMon> = (0..30)
        .map(|_| TestMon {
            listed: 0x99,
            mon: mon(0x99, 4, false),
            ot: vec![0x80],
            nickname: vec![0x85],
        })
        .collect();
    put_list(&mut s, 0x302D, 30, 6, &boxed);
    put_list(&mut s, 0x6000 + 2 * 0x566, 30, 6, &boxed[..1]);
    s[0x3594] = checksum(&s[0x2598..0x3594]);

    let state = read_save(game, &s).expect("save");
    assert_eq!(state.trainer.name, "らんり");
    assert_eq!(state.trainer.money, 3000);
    assert_eq!(state.trainer.id, 7);
    assert_eq!(state.badge_count(), 8);
    assert_eq!(state.boxes.len(), 8);
    assert_eq!(state.boxes[5].pokemon.len(), 30);
    assert_eq!(state.boxes[6].pokemon.len(), 1);
    let Pokemon::Hatched(pika) = &state.party[0] else {
        panic!("egg");
    };
    assert_eq!(pika.ot_name, "アリ");
    assert_eq!(pika.nickname, "ピ゛フ");
}

pub(crate) fn red_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x80000];
    let mut pointers = Vec::new();
    for word in [0x4000u16, 0x4000, 0x7000, 0x5000, 0xD273, 0xD9AC, 0x7100] {
        pointers.extend(word.to_le_bytes());
    }
    put(&mut rom, 0x1000, &pointers);
    put(&mut rom, 7 * 0x4000, &fixed_names("M", 190, 10));
    put(&mut rom, 10 * 0x4000, &name_list("MOVE", 165));
    put(&mut rom, 0x5000, &name_list("ITEM", 83));
    let order: Vec<u8> = (1..=190u16)
        .map(|i| match i {
            1 => 151,
            2..=40 => 0,
            _ => (191 - i) as u8,
        })
        .collect();
    put(&mut rom, 0x41024, &order);
    let mut code = Vec::new();
    for (hl, de, bc) in [
        (0xA5A3, 0xD2F7, 0x789),
        (0xD158, 0xA598, 0x0B),
        (0xD2F7, 0xA5A3, 0x26),
        (0xD2F7, 0xA5A3, 0x789),
        (0xD163, 0xAF2C, 0x194),
        (0xDA80, 0xB0C0, 0x462),
        (0xC100, 0xAD2C, 0x200),
    ] {
        code.extend(copy_routine(hl, de, bc));
    }
    put(&mut rom, 0x73652, &code);
    rom
}

#[test]
fn live_state_comes_from_work_ram_and_other_boxes_from_the_save() {
    let game = game(Version::Red, Language::English);
    let tables = RomTables::load(game, &red_rom()).expect("tables");
    assert_eq!(
        tables.live_map(),
        vec![
            (0xD158, 0x2598),
            (0xD2F7, 0x25A3),
            (0xD163, 0x2F2C),
            (0xDA80, 0x30C0)
        ]
    );
    let saved = international_save();
    let mut now = saved.clone();
    put(&mut now, 0x25F3, &[0x09, 0x87, 0x65]);
    put_list(
        &mut now,
        0x2F2C,
        6,
        11,
        &[
            entry(0x99, 6, true, "BULB"),
            entry(0x54, 7, true, "PIKA"),
            entry(0x24, 3, true, "NEW"),
        ],
    );
    seal_international(&mut now);
    let mut wram = vec![0u8; 0x2000];
    for (addr, at, len) in [
        (0xD158, 0x2598, 0x0B),
        (0xD2F7, 0x25A3, 0x789),
        (0xD163, 0x2F2C, 0x194),
        (0xDA80, 0x30C0, 0x462),
    ] {
        put(&mut wram, addr - 0xC000, &now[at..at + len]);
    }
    let live = read_live(&tables, &wram, &saved).expect("live");
    let mut expected = read_save(game, &now).expect("save");
    expected.copy = SaveCopy::Live;
    assert_eq!(live, expected);
    assert_eq!(live.trainer.money, 98765);
    assert_eq!(live.party.len(), 3);

    assert_eq!(
        read_live(&tables, &[0u8; 0x2000], &saved),
        Err(ReadError::NotRunning)
    );
}
