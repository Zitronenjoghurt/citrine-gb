use crate::fixtures::gen3_rom::{
    ITEMS, MOVED, SHEDINJA, SPECIES, SPOTS, Spots, gen3_rom, national_for, species_name,
};
use crate::gen3::{Met, Mon, PartyStats, RomTables, Spread, identify};
use crate::{Game, Gender, Language, ReadError, Stats, Version};

fn tables(at: Spots) -> RomTables {
    RomTables::load(&gen3_rom(b"BPEE", 0, at)).expect("tables")
}

fn mon(species: u16, personality: u32, exp: u32) -> Mon {
    Mon {
        species,
        personality,
        ot_id: 1,
        ot_secret_id: 2,
        nickname: String::new(),
        ot_name: String::new(),
        language: Some(Language::English),
        held_item: None,
        exp,
        level: None,
        friendship: 70,
        moves: Vec::new(),
        ivs: Spread {
            hp: 31,
            attack: 0,
            defense: 15,
            speed: 7,
            special_attack: 20,
            special_defense: 1,
        },
        evs: Spread {
            hp: 4,
            attack: 252,
            defense: 0,
            speed: 252,
            special_attack: 0,
            special_defense: 0,
        },
        ability_slot: 0,
        pokerus: 0,
        met: Met {
            location: 0,
            level: 5,
            game: 3,
            ball: 4,
            ot_gender: Gender::Male,
        },
        party: None,
    }
}

#[test]
fn identifies_each_game_from_its_header() {
    let cases = [
        (b"AXVE", Version::Ruby, Language::English),
        (b"AXPD", Version::Sapphire, Language::German),
        (b"BPEJ", Version::Emerald, Language::Japanese),
        (b"BPRF", Version::FireRed, Language::French),
        (b"BPGS", Version::LeafGreen, Language::Spanish),
        (b"BPEI", Version::Emerald, Language::Italian),
    ];
    for (code, version, language) in cases {
        let rom = gen3_rom(code, 2, SPOTS);
        assert_eq!(
            identify(&rom),
            Some(Game {
                version,
                language,
                revision: 2,
            })
        );
    }
}

#[test]
fn other_roms_are_not_identified() {
    assert_eq!(identify(&gen3_rom(b"BPEX", 0, SPOTS)), None);
    assert_eq!(identify(&gen3_rom(b"AGBE", 0, SPOTS)), None);
    assert_eq!(identify(&[0; 0xBF]), None);
    let mut gb = vec![0; 0x8000];
    gb[0x134..0x13F].copy_from_slice(b"POKEMON RED");
    assert_eq!(identify(&gb), None);
}

#[test]
fn finds_the_name_tables_wherever_they_sit() {
    for at in [SPOTS, MOVED] {
        let tables = tables(at);
        assert_eq!(tables.species_name(1), Some("BULBASAUR"));
        assert_eq!(tables.species_name(277), Some("TREECKO"));
        assert_eq!(tables.species_name(411), Some("CHIMECHO"));
        assert_eq!(tables.move_name(1), Some("POUND"));
        assert_eq!(tables.move_name(354), Some("MOVE354"));
        assert_eq!(tables.move_name(0), None);
        assert_eq!(tables.move_name(355), None);
        assert_eq!(tables.ability_name(1), Some("STENCH"));
        assert_eq!(tables.ability_name(77), Some("SKILL77"));
        assert_eq!(tables.type_name(0), Some("NORMAL"));
        assert_eq!(tables.type_name(9), Some("???"));
        assert_eq!(tables.type_name(17), Some("DARK"));
        assert_eq!(tables.item_name(1), Some("MASTER BALL"));
        assert_eq!(tables.item_name(ITEMS as u16 - 1), Some("ITEM376"));
        assert_eq!(tables.item_name(ITEMS as u16), None);
    }
}

#[test]
fn maps_internal_species_to_national_numbers() {
    let tables = tables(SPOTS);
    for species in 0..SPECIES as u16 {
        let national = national_for(species);
        let expected = (1..=386).contains(&national).then_some(national);
        assert_eq!(tables.national_number(species), expected, "{species}");
        if let Some(national) = expected {
            assert_eq!(tables.species_for_national(national), Some(species));
        }
    }
    assert_eq!(tables.national_number(277), Some(252));
    assert_eq!(tables.national_number(411), Some(358));
    assert_eq!(tables.species_for_national(0), None);
    assert_eq!(tables.species_for_national(387), None);
    assert_eq!(tables.pokedex_size(), 386);
}

#[test]
fn placeholder_species_have_no_name_or_stats() {
    let tables = tables(SPOTS);
    for species in [0, 252, 260, 276, 412] {
        assert_eq!(tables.species_name(species), None, "{species}");
        assert_eq!(tables.base_stats(species), None, "{species}");
    }
    assert_eq!(species_name(260), "MON260");
}

#[test]
fn reads_base_stats_by_species() {
    let tables = tables(MOVED);
    let base = tables.base_stats(6).expect("base");
    assert_eq!((base.hp, base.attack, base.special_defense), (51, 49, 65));
    assert_eq!(base.types, [6, 9]);
    assert_eq!(
        (base.catch_rate, base.exp_yield, base.gender_ratio),
        (45, 64, 31)
    );
    assert_eq!(base.growth, crate::gen3::Growth::MediumFast);
    assert_eq!(base.abilities, [7, 0]);
    assert_eq!(tables.species_types(6), vec!["TYPE6", "???"]);
    assert_eq!(tables.species_types(15), vec!["TYPE15", "NORMAL"]);
}

#[test]
fn box_mons_get_their_level_from_the_growth_rate() {
    let tables = tables(SPOTS);
    let mut boxed = mon(6, 3, 125_000);
    assert_eq!(tables.mon_level(&boxed), Some(50));
    assert_eq!(tables.exp_to_next_level(&boxed), Some(132_651 - 125_000));
    boxed.exp = 124_999;
    assert_eq!(tables.mon_level(&boxed), Some(49));
    boxed.level = Some(77);
    assert_eq!(tables.mon_level(&boxed), Some(77));
    let erratic = mon(7, 3, 600_000);
    assert_eq!(tables.mon_level(&erratic), Some(100));
    assert_eq!(tables.exp_to_next_level(&erratic), None);
    assert_eq!(tables.mon_level(&mon(260, 3, 1000)), None);
}

#[test]
fn computes_box_stats_with_the_nature() {
    let tables = tables(SPOTS);
    let adamant = mon(6, 3, 125_000);
    assert_eq!(adamant.nature(), crate::gen3::Nature::Adamant);
    assert_eq!(
        tables.mon_stats(&adamant),
        Some(Stats {
            max_hp: 127,
            attack: 93,
            defense: 61,
            speed: 85,
            special_attack: 72,
            special_defense: 70,
        })
    );
    let hardy = mon(6, 0, 125_000);
    let stats = tables.mon_stats(&hardy).expect("stats");
    assert_eq!((stats.attack, stats.special_attack), (85, 80));
    let shedinja = mon(SHEDINJA, 3, 1_000_000);
    assert_eq!(tables.national_number(SHEDINJA), Some(292));
    assert_eq!(tables.mon_stats(&shedinja).map(|s| s.max_hp), Some(1));
}

#[test]
fn party_mons_keep_their_stored_stats() {
    let tables = tables(SPOTS);
    let stats = Stats {
        max_hp: 1,
        attack: 2,
        defense: 3,
        speed: 4,
        special_attack: 5,
        special_defense: 6,
    };
    let mut party = mon(6, 3, 125_000);
    party.level = Some(12);
    party.party = Some(PartyStats {
        status: 0,
        hp: 1,
        stats,
    });
    assert_eq!(tables.mon_stats(&party), Some(stats));
    assert_eq!(tables.mon_level(&party), Some(12));
}

#[test]
fn gender_follows_the_ratio_and_personality() {
    let tables = tables(SPOTS);
    assert_eq!(tables.mon_gender(&mon(6, 30, 0)), Some(Gender::Female));
    assert_eq!(tables.mon_gender(&mon(6, 31, 0)), Some(Gender::Male));
    assert_eq!(tables.mon_gender(&mon(6, 0x100, 0)), Some(Gender::Female));
    assert_eq!(tables.mon_gender(&mon(5, 0, 0)), Some(Gender::Male));
    assert_eq!(tables.mon_gender(&mon(8, 0xFF, 0)), Some(Gender::Female));
    assert_eq!(tables.mon_gender(&mon(9, 0, 0)), None);
}

#[test]
fn the_ability_slot_picks_the_ability() {
    let tables = tables(SPOTS);
    let mut first = mon(7, 0, 0);
    assert_eq!(tables.mon_ability(&first), Some("SKILL8"));
    first.ability_slot = 1;
    assert_eq!(tables.mon_ability(&first), Some("SKILL2"));
    let mut single = mon(6, 0, 0);
    single.ability_slot = 1;
    assert_eq!(tables.mon_ability(&single), Some("SKILL7"));
}

#[test]
fn a_missing_table_fails_the_load() {
    let base = gen3_rom(b"AXVE", 0, SPOTS);
    let cuts = [
        (SPOTS.species, 11),
        (SPOTS.moves + 13, 13),
        (SPOTS.abilities, 13),
        (SPOTS.types + 9 * 7, 7),
        (SPOTS.items + 44 + 14, 2),
        (SPOTS.national, 2),
        (SPOTS.base + 28 + 26, 2),
    ];
    for (cut, len) in cuts {
        let mut rom = base.clone();
        rom[cut..cut + len].fill(0x11);
        assert_eq!(RomTables::load(&rom), Err(ReadError::RomTables), "{cut:#x}");
    }
    assert_eq!(
        RomTables::load(&gen3_rom(b"ABCD", 0, SPOTS)),
        Err(ReadError::RomTables)
    );
}

#[test]
fn truncated_roms_never_panic() {
    let rom = gen3_rom(b"BPRE", 1, MOVED);
    for len in [0, 0xC0, 0x1000, 0x2_1000, 0x3_1100, 0x3_3D00] {
        assert!(RomTables::load(&rom[..len]).is_err(), "{len:#x}");
    }
}
