use crate::fixtures::gen3::{
    FLASH, Gen3Save, Pocket, TestMon3, break_section, gen3_flash, gen3_name,
};
use crate::gen3::{Certainty, Growth, Layout, Nature, Pokemon, guess_layout, read_save, text};
use crate::{Gender, Language, Pokerus, ReadError, SaveCopy, Version};

const LAYOUTS: [Layout; 3] = [
    Layout::RubySapphire,
    Layout::Emerald,
    Layout::FireRedLeafGreen,
];

fn key_for(layout: Layout) -> u32 {
    match layout {
        Layout::RubySapphire => 0,
        _ => 0xA5C3_1E77,
    }
}

fn full_save(layout: Layout) -> Gen3Save {
    let mut save = Gen3Save::new(layout, key_for(layout));
    save.trainer("BRENDAN", false, (12345, 54321), (123, 45, 6));
    save.money(987_654, 1234);
    save.party(&[
        TestMon3::new(277, 0x1234_5678, 0xD431_3039).party(),
        TestMon3::new(280, 0x0BAD_F00D, 0xD431_3039).party(),
    ]);
    save.pocket(Pocket::Items, &[(13, 5), (20, 999)]);
    save.pocket(Pocket::KeyItems, &[(259, 1)]);
    save.pocket(Pocket::Balls, &[(4, 10)]);
    save.pocket(Pocket::TmsHms, &[(289, 1), (339, 1)]);
    save.pocket(Pocket::Berries, &[(133, 3)]);
    save.pocket(Pocket::Pc, &[(13, 1)]);
    save.dex(&[252, 386], &[1, 252, 386]);
    save
}

fn read(flash: &[u8], layout: Layout) -> crate::gen3::SaveState {
    read_save(flash, layout, Language::English).expect("save")
}

#[test]
fn decodes_names_from_their_bytes() {
    let brendan = [0xBC, 0xCC, 0xBF, 0xC8, 0xBE, 0xBB, 0xC8, 0xFF];
    assert_eq!(text::decode(&brendan, Language::English), "BRENDAN");
    let marks = [
        0xA1, 0xAA, 0xAB, 0xB5, 0xB6, 0x1B, 0x00, 0x53, 0x54, 0xFF, 0xBB,
    ];
    assert_eq!(text::decode(&marks, Language::German), "09!♂♀é PKMN");
    let haruka = [0x6A, 0x79, 0x56, 0xFF, 0xFF];
    assert_eq!(text::decode(&haruka, Language::Japanese), "ハルカ");
    assert_eq!(
        text::decode(&[0x01, 0x3C, 0xA1, 0xB7], Language::Japanese),
        "あざ０円"
    );
    assert_eq!(
        text::decode(&[0x0A, 0x40], Language::English),
        "\u{FFFD}\u{FFFD}"
    );
}

#[test]
fn encodes_what_it_decodes() {
    for (name, language) in [
        ("Zigzagoon", Language::English),
        ("Größe", Language::German),
        ("ミズゴロウ", Language::Japanese),
        ("ぽけもん", Language::Japanese),
    ] {
        let bytes = text::encode(name, language).expect("encodes");
        assert_eq!(text::decode(&bytes, language), name);
    }
    assert_eq!(text::encode("BRENDAN", Language::English).unwrap()[0], 0xBC);
    assert_eq!(text::encode("€", Language::English), None);
}

#[test]
fn reads_the_trainer_bag_and_pokedex_of_each_layout() {
    for layout in LAYOUTS {
        let state = read(
            &gen3_flash(Some(&full_save(layout).slot(7, 0)), None),
            layout,
        );
        assert_eq!(state.layout, layout);
        assert_eq!(state.copy, SaveCopy::Main);
        assert_eq!(state.save_index, 7);
        let trainer = &state.trainer;
        assert_eq!(trainer.name, "BRENDAN");
        assert_eq!(trainer.gender, Gender::Male);
        assert_eq!((trainer.id, trainer.secret_id), (12345, 54321));
        assert_eq!(trainer.money, 987_654, "{layout:?}");
        assert_eq!(trainer.coins, 1234);
        let time = trainer.play_time;
        assert_eq!((time.hours, time.minutes, time.seconds), (123, 45, 6));
        let pairs = |stacks: &[crate::gen3::ItemStack]| {
            stacks.iter().map(|s| (s.item, s.count)).collect::<Vec<_>>()
        };
        assert_eq!(
            pairs(&state.bag.items),
            vec![(13, 5), (20, 999)],
            "{layout:?}"
        );
        assert_eq!(pairs(&state.bag.key_items), vec![(259, 1)]);
        assert_eq!(pairs(&state.bag.balls), vec![(4, 10)]);
        assert_eq!(pairs(&state.bag.tms_hms), vec![(289, 1), (339, 1)]);
        assert_eq!(pairs(&state.bag.berries), vec![(133, 3)]);
        assert_eq!(pairs(&state.pc_items), vec![(13, 1)]);
        assert_eq!(state.pokedex.caught, vec![252, 386]);
        assert_eq!(state.pokedex.seen, vec![1, 252, 386]);
        assert_eq!(state.party.len(), 2);
        assert_eq!(state.boxes.len(), 14);
        assert!(state.boxes.iter().all(|b| b.slots.len() == 30));
    }
}

#[test]
fn reads_a_female_trainer_with_a_japanese_name() {
    let mut save = Gen3Save::new(Layout::Emerald, 1);
    save.trainer("A", true, (1, 2), (0, 0, 0));
    save.small[..8].copy_from_slice(&[0x6A, 0x79, 0x56, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    let flash = gen3_flash(Some(&save.slot(1, 3)), None);
    let state = read_save(&flash, Layout::Emerald, Language::Japanese).expect("save");
    assert_eq!(state.trainer.name, "ハルカ");
    assert_eq!(state.trainer.gender, Gender::Female);
}

#[test]
fn the_newer_valid_slot_wins() {
    let save = full_save(Layout::Emerald);
    let mut older = save.clone();
    older.money(1, 0);
    let (a, b) = (older.slot(4, 0), save.slot(5, 1));
    let state = read(&gen3_flash(Some(&a), Some(&b)), Layout::Emerald);
    assert_eq!(
        (state.copy, state.save_index, state.trainer.money),
        (SaveCopy::Main, 5, 987_654)
    );
    let state = read(&gen3_flash(Some(&b), Some(&a)), Layout::Emerald);
    assert_eq!((state.copy, state.save_index), (SaveCopy::Main, 5));
}

#[test]
fn a_slot_with_a_bad_checksum_is_skipped() {
    let save = full_save(Layout::FireRedLeafGreen);
    let mut older = save.clone();
    older.money(1, 0);
    for id in [0, 4, 9, 13] {
        let mut newer = save.slot(9, 2);
        break_section(&mut newer, id);
        let flash = gen3_flash(Some(&older.slot(8, 1)), Some(&newer));
        let state = read(&flash, Layout::FireRedLeafGreen);
        assert_eq!(
            (state.copy, state.save_index, state.trainer.money),
            (SaveCopy::Backup, 8, 1)
        );
    }
    let mut stale = older.slot(8, 1);
    break_section(&mut stale, 1);
    let state = read(
        &gen3_flash(Some(&stale), Some(&save.slot(9, 2))),
        Layout::FireRedLeafGreen,
    );
    assert_eq!((state.copy, state.save_index), (SaveCopy::Main, 9));
}

#[test]
fn no_valid_slot_is_an_error() {
    let save = full_save(Layout::RubySapphire);
    let (mut a, mut b) = (save.slot(1, 0), save.slot(2, 0));
    break_section(&mut a, 3);
    break_section(&mut b, 5);
    let error = read_save(
        &gen3_flash(Some(&a), Some(&b)),
        Layout::RubySapphire,
        Language::English,
    );
    assert_eq!(error, Err(ReadError::NoValidCopy));
    let blank = gen3_flash(None, None);
    assert_eq!(
        read_save(&blank, Layout::RubySapphire, Language::English),
        Err(ReadError::NoValidCopy)
    );
    assert_eq!(
        read_save(&blank[..0x1BFFF], Layout::RubySapphire, Language::English),
        Err(ReadError::TooShort)
    );
    let mut missing = save.slot(1, 0);
    missing[0xFF8] = 0;
    assert_eq!(
        read_save(
            &gen3_flash(Some(&missing), None),
            Layout::RubySapphire,
            Language::English
        ),
        Err(ReadError::NoValidCopy)
    );
}

#[test]
fn the_save_index_wraps() {
    let save = full_save(Layout::Emerald);
    let mut older = save.clone();
    older.money(1, 0);
    let flash = gen3_flash(Some(&older.slot(u32::MAX, 0)), Some(&save.slot(0, 1)));
    let state = read(&flash, Layout::Emerald);
    assert_eq!((state.save_index, state.trainer.money), (0, 987_654));
}

#[test]
fn every_section_rotation_reads_the_same() {
    let save = full_save(Layout::RubySapphire);
    let first = read(
        &gen3_flash(Some(&save.slot(3, 0)), None),
        Layout::RubySapphire,
    );
    for rotation in 1..14 {
        let state = read(
            &gen3_flash(None, Some(&save.slot(3, rotation))),
            Layout::RubySapphire,
        );
        assert_eq!(state, first, "rotation {rotation}");
    }
}

#[test]
fn reads_a_party_mon() {
    let mut mon = TestMon3::new(277, 0x1234_5678, 0xD431_3039);
    mon.held_item = 139;
    mon.pp_ups = 0b11_10;
    mon.ability = true;
    mon.pokerus = 0x21;
    mon.status = 0x08;
    let mut save = Gen3Save::new(Layout::Emerald, 77);
    save.party(&[mon.party()]);
    let state = read(&gen3_flash(Some(&save.slot(1, 0)), None), Layout::Emerald);
    let Pokemon::Hatched(read) = &state.party[0] else {
        panic!("not a mon: {:?}", state.party[0]);
    };
    assert_eq!(read.species, 277);
    assert_eq!(read.personality, 0x1234_5678);
    assert_eq!((read.ot_id, read.ot_secret_id), (0x3039, 0xD431));
    assert_eq!(read.nickname, "TREECKO");
    assert_eq!(read.ot_name, "MAY");
    assert_eq!(read.language, Some(Language::English));
    assert_eq!(read.held_item, Some(139));
    assert_eq!(read.exp, 1000);
    assert_eq!(read.level, Some(10));
    assert_eq!(read.friendship, 70);
    let moves: Vec<(u16, u8, u8)> = read.moves.iter().map(|m| (m.id, m.pp, m.pp_ups)).collect();
    assert_eq!(moves, vec![(33, 35, 2), (45, 40, 3)]);
    let ivs = read.ivs;
    assert_eq!(
        [
            ivs.hp,
            ivs.attack,
            ivs.defense,
            ivs.speed,
            ivs.special_attack,
            ivs.special_defense
        ],
        [31, 0, 15, 7, 20, 1]
    );
    assert_eq!(
        (read.evs.attack, read.evs.speed, read.evs.hp),
        (252, 252, 4)
    );
    assert_eq!(read.ability_slot, 1);
    assert_eq!(
        read.pokerus_status(),
        Some(Pokerus::Infected { strain: 2, days: 1 })
    );
    assert_eq!(
        (
            read.met.location,
            read.met.level,
            read.met.game,
            read.met.ball,
            read.met.ot_gender
        ),
        (16, 5, 2, 4, Gender::Male)
    );
    let party = read.party.expect("party stats");
    assert_eq!((party.status, party.hp), (0x08, 30));
    assert_eq!((party.stats.max_hp, party.stats.special_defense), (31, 19));
    assert_eq!(read.nature(), Nature::from_personality(0x1234_5678));
    assert_eq!(read.nature(), Nature::Gentle);
}

#[test]
fn reads_every_substructure_order() {
    for order in 0..24u32 {
        let personality = 0x5EED_0000 - 16 + order;
        let mut mon = TestMon3::new(300 + order as u16, personality, 0x0BB0_0FF1);
        mon.moves = [(1, 2), (3, 4), (5, 6), (7, 8)];
        mon.pokerus = 0x10;
        let mut save = Gen3Save::new(Layout::FireRedLeafGreen, 5);
        save.party(&[mon.party()]);
        save.put_box(0, 0, &mon.boxed());
        let state = read(
            &gen3_flash(Some(&save.slot(1, 0)), None),
            Layout::FireRedLeafGreen,
        );
        for found in [
            &state.party[0],
            state.boxes[0].slots[0].as_ref().expect("box mon"),
        ] {
            let Pokemon::Hatched(found) = found else {
                panic!("order {order}: {found:?}");
            };
            assert_eq!(found.species, 300 + order as u16, "order {order}");
            assert_eq!(found.moves.len(), 4);
            assert_eq!(found.moves[3].id, 7);
            assert_eq!(found.evs.attack, 252);
            assert_eq!(found.ivs.defense, 15);
            assert_eq!(found.pokerus_status(), Some(Pokerus::Cured { strain: 1 }));
        }
        assert_eq!(personality % 24, order);
    }
}

#[test]
fn tells_shiny_from_plain() {
    let shiny = TestMon3::new(1, 0x3039_D436, 0xD431_3039);
    let plain = TestMon3::new(1, 0x3039_D439, 0xD431_3039);
    let mut save = Gen3Save::new(Layout::RubySapphire, 0);
    save.party(&[shiny.party(), plain.party()]);
    let state = read(
        &gen3_flash(Some(&save.slot(1, 0)), None),
        Layout::RubySapphire,
    );
    let shiny: Vec<bool> = state
        .party
        .iter()
        .map(|p| matches!(p, Pokemon::Hatched(m) if m.shiny()))
        .collect();
    assert_eq!(shiny, vec![true, false]);
}

#[test]
fn a_mon_that_fails_its_checksum_is_a_bad_egg() {
    let mut broken = TestMon3::new(25, 99, 7);
    broken.break_checksum = true;
    let mut flagged = TestMon3::new(25, 98, 7);
    flagged.flags = 0x03;
    let mut egg = TestMon3::new(258, 97, 7);
    egg.egg = true;
    egg.flags = 0x06;
    egg.language = 1;
    egg.nickname = gen3_name(&text::encode("タマゴ", Language::Japanese).unwrap(), 10);
    let mut save = Gen3Save::new(Layout::Emerald, 3);
    save.party(&[broken.party(), flagged.party(), egg.party()]);
    save.put_box(13, 29, &broken.boxed());
    let state = read(&gen3_flash(Some(&save.slot(1, 0)), None), Layout::Emerald);
    assert_eq!(state.party[0], Pokemon::BadEgg);
    assert_eq!(state.party[1], Pokemon::BadEgg);
    let Pokemon::Egg(egg) = &state.party[2] else {
        panic!("not an egg: {:?}", state.party[2]);
    };
    assert_eq!((egg.species, egg.nickname.as_str()), (258, "タマゴ"));
    assert_eq!(egg.language, Some(Language::Japanese));
    assert_eq!(state.boxes[13].slots[29], Some(Pokemon::BadEgg));
}

#[test]
fn garbage_in_a_slot_never_panics() {
    let mut save = Gen3Save::new(Layout::RubySapphire, 0);
    let mut seed = 0x1234_5678u32;
    for slot in 0..30 {
        let bytes: Vec<u8> = (0..80)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (seed >> 16) as u8
            })
            .collect();
        save.put_box(2, slot, &bytes);
    }
    save.large[0x234] = 0xFF;
    let state = read(
        &gen3_flash(Some(&save.slot(1, 0)), None),
        Layout::RubySapphire,
    );
    assert!(state.party.len() <= 6);
    assert!(state.boxes[2].slots.iter().flatten().count() >= 25);
}

#[test]
fn reads_a_full_pc() {
    let mut save = Gen3Save::new(Layout::FireRedLeafGreen, 0x0102_0304);
    for number in 0..14 {
        save.box_name(number, &format!("BOX {}", number + 1));
        for slot in 0..30 {
            let n = (number * 30 + slot) as u32;
            let mon = TestMon3::new(1 + n as u16, 0x0100_0000 + n * 7919, 0x0042_0042);
            save.put_box(number, slot, &mon.boxed());
        }
    }
    save.current_box(13);
    save.box_name(5, "Größe");
    let state = read(
        &gen3_flash(Some(&save.slot(2, 5)), None),
        Layout::FireRedLeafGreen,
    );
    assert_eq!(state.current_box, 13);
    assert_eq!(state.boxes[0].name, "BOX 1");
    assert_eq!(state.boxes[5].name, "Größe");
    assert_eq!(state.boxes[13].number, 14);
    let species: Vec<u16> = state
        .boxes
        .iter()
        .flat_map(|b| b.slots.iter())
        .map(|slot| match slot {
            Some(Pokemon::Hatched(mon)) => mon.species,
            other => panic!("not a mon: {other:?}"),
        })
        .collect();
    assert_eq!(species, (1..=420).collect::<Vec<u16>>());
    let Some(Pokemon::Hatched(last)) = &state.boxes[13].slots[29] else {
        unreachable!()
    };
    assert_eq!((last.level, last.party), (None, None));
    assert_eq!(last.level_with(Growth::MediumFast), 10);
}

#[test]
fn guesses_the_layout_from_the_save() {
    for layout in LAYOUTS {
        let flash = gen3_flash(Some(&full_save(layout).slot(1, 0)), None);
        let guess = guess_layout(&flash).expect("guess");
        assert_eq!((guess.layout, guess.certainty), (layout, Certainty::Sure));
    }
    let zero_key = gen3_flash(Some(&Gen3Save::new(Layout::Emerald, 0).slot(1, 0)), None);
    let guess = guess_layout(&zero_key).expect("guess");
    assert_eq!(
        (guess.layout, guess.certainty),
        (Layout::Emerald, Certainty::Likely)
    );
    assert_eq!(guess_layout(&vec![0xFF; FLASH]), None);
    assert_eq!(guess_layout(&[0; 100]), None);
    assert_eq!(
        Layout::of(Version::LeafGreen),
        Some(Layout::FireRedLeafGreen)
    );
    assert_eq!(Layout::of(Version::Sapphire), Some(Layout::RubySapphire));
    assert_eq!(Layout::of(Version::Crystal), None);
}

#[test]
fn a_fresh_save_is_told_apart_by_its_game_code() {
    for layout in LAYOUTS {
        let mut save = Gen3Save::new(layout, key_for(layout) | 0x10).fresh();
        save.trainer("AAAAAAA", false, (19608, 23605), (0, 0, 37));
        save.money(3000, 0);
        save.pocket(Pocket::Pc, &[(13, 1)]);
        let flash = gen3_flash(None, Some(&save.slot(1, 1)));
        let guess = guess_layout(&flash).expect("guess");
        assert_eq!((guess.layout, guess.certainty), (layout, Certainty::Sure));
        let state = read(&flash, layout);
        assert_eq!((state.save_index, state.copy), (1, SaveCopy::Main));
        assert_eq!((state.trainer.money, state.trainer.coins), (3000, 0));
        assert_eq!(state.pc_items.len(), 1);
        assert!(state.party.is_empty() && state.bag.items.is_empty());
    }
}

#[test]
fn natures_follow_the_personality() {
    assert_eq!(Nature::from_personality(0), Nature::Hardy);
    assert_eq!(Nature::from_personality(24), Nature::Quirky);
    assert_eq!(Nature::from_personality(25 * 1000 + 3), Nature::Adamant);
    assert_eq!(Nature::from_personality(u32::MAX), Nature::Calm);
}

#[test]
fn growth_rates_match_the_level_100_totals() {
    let totals: Vec<u32> = [
        Growth::MediumFast,
        Growth::Erratic,
        Growth::Fluctuating,
        Growth::MediumSlow,
        Growth::Fast,
        Growth::Slow,
    ]
    .into_iter()
    .map(|growth| growth.exp(100))
    .collect();
    assert_eq!(
        totals,
        vec![1_000_000, 600_000, 1_640_000, 1_059_860, 800_000, 1_250_000]
    );
    assert_eq!(Growth::MediumSlow.exp(1), 0);
    assert_eq!(Growth::MediumSlow.exp(5), 135);
    assert_eq!(Growth::Erratic.exp(50), 125_000);
    assert_eq!(Growth::MediumFast.level(999_999), 99);
    assert_eq!(Growth::MediumFast.level(0), 1);
    assert_eq!(Growth::Slow.level(u32::MAX), 100);
    assert_eq!(Growth::from_byte(2), Some(Growth::Fluctuating));
    assert_eq!(Growth::from_byte(6), None);
}
