use super::tables::crystal_rom_with_pictures;
use super::*;
use crate::{Dvs, Gender, Genes, GrowthRate, Mon, Pokerus, RomTables, Side, Stats};

fn mon(species: u16, level: u8, exp: u32, dvs: Dvs, stat_exp: [u16; 5]) -> Mon {
    Mon {
        species,
        nickname: String::new(),
        ot_name: String::new(),
        ot_id: 0,
        level,
        exp,
        moves: Vec::new(),
        held_item: None,
        genes: Genes::Gb { dvs, stat_exp },
        hp: None,
        stats: None,
        status: None,
        friendship: None,
        pokerus: None,
        caught: None,
    }
}

#[test]
fn stats_follow_base_dvs_and_stat_exp() {
    let tables = RomTables::load(
        game(Version::Crystal, Language::English),
        &crystal_rom_with_pictures(),
    )
    .expect("tables");
    let dvs = Dvs::from_bytes(0xFE, 0xDC);
    assert_eq!(dvs.hp(), 10);
    let mon = mon(25, 50, 0, dvs, [0, 100, 65535, 1, 10000]);
    assert_eq!(
        tables.mon_stats(&mon),
        Some(Stats {
            max_hp: 110,
            attack: 71,
            defense: 110,
            speed: 88,
            special_attack: 109,
            special_defense: 119,
        })
    );
}

#[test]
fn exp_curves_match_the_growth_formulas() {
    let at_ten = [
        (GrowthRate::MediumFast, 1000),
        (GrowthRate::SlightlyFast, 1720),
        (GrowthRate::SlightlySlow, 2680),
        (GrowthRate::MediumSlow, 560),
        (GrowthRate::Fast, 800),
        (GrowthRate::Slow, 1250),
    ];
    for (rate, exp) in at_ten {
        assert_eq!(rate.exp_at(10), exp, "{rate:?}");
    }
    assert_eq!(GrowthRate::MediumSlow.exp_at(1), 0);
    assert_eq!(GrowthRate::MediumSlow.exp_at(2), 9);
    assert_eq!(GrowthRate::MediumFast.exp_at(100), 1_000_000);
    assert_eq!(GrowthRate::Slow.exp_at(100), 1_250_000);
    let tables = RomTables::load(
        game(Version::Crystal, Language::English),
        &crystal_rom_with_pictures(),
    )
    .expect("tables");
    let dvs = Dvs::from_bytes(0, 0);
    assert_eq!(
        tables.exp_to_next_level(&mon(25, 9, 900, dvs, [0; 5])),
        Some(100)
    );
    assert_eq!(
        tables.exp_to_next_level(&mon(25, 9, 1500, dvs, [0; 5])),
        Some(0)
    );
    assert_eq!(
        tables.exp_to_next_level(&mon(25, 100, 1_000_000, dvs, [0; 5])),
        None
    );
}

#[test]
fn gender_compares_attack_and_speed_dvs_with_the_ratio() {
    let tables = RomTables::load(
        game(Version::Crystal, Language::English),
        &crystal_rom_with_pictures(),
    )
    .expect("tables");
    let gender = |species: u16, high: u8, low: u8| {
        tables.mon_gender(&mon(species, 5, 0, Dvs::from_bytes(high, low), [0; 5]))
    };
    assert_eq!(gender(25, 0x00, 0x00), Some(Gender::Male));
    assert_eq!(gender(26, 0x1F, 0xF0), Some(Gender::Female));
    assert_eq!(gender(26, 0x20, 0x00), Some(Gender::Male));
    assert_eq!(gender(27, 0x70, 0xF0), Some(Gender::Female));
    assert_eq!(gender(27, 0x80, 0x00), Some(Gender::Male));
    assert_eq!(gender(28, 0xF0, 0xF0), Some(Gender::Female));
    assert_eq!(gender(29, 0x00, 0x00), None);
}

#[test]
fn shiny_mons_get_the_shiny_palette_and_unown_its_letter() {
    let rom = crystal_rom_with_pictures();
    let tables = RomTables::load(game(Version::Crystal, Language::English), &rom).expect("tables");
    let plain = mon(25, 5, 0, Dvs::from_bytes(0xAA, 0xAB), [0; 5]);
    let shiny = mon(25, 5, 0, Dvs::from_bytes(0xAA, 0xAA), [0; 5]);
    assert!(shiny.shiny() && !plain.shiny());
    assert_eq!(tables.mon_palette(&plain), tables.palette(25, false));
    assert_eq!(tables.mon_palette(&shiny), tables.palette(25, true));
    let unown = mon(201, 5, 0, Dvs::from_bytes(0x00, 0x44), [0; 5]);
    assert_eq!(unown.dvs().map(Dvs::unown_letter), Some('B'));
    let sprite = tables.mon_sprite(&rom, &unown, Side::Front).expect("unown");
    assert!(sprite.pixels.iter().all(|&p| p == 3 || p == 0));
    assert_eq!(sprite.pixel(0, 0), 3);
    assert_eq!(sprite.pixel(7, 0), 3);
    assert_eq!(sprite.pixel(1, 0), 0);
}

#[test]
fn pokerus_reads_strain_and_days() {
    let with = |byte| {
        let mut m = mon(1, 5, 0, Dvs::from_bytes(0, 0), [0; 5]);
        m.pokerus = byte;
        m.pokerus_status()
    };
    assert_eq!(with(None), None);
    assert_eq!(with(Some(0)), None);
    assert_eq!(
        with(Some(0x13)),
        Some(Pokerus::Infected { strain: 1, days: 3 })
    );
    assert_eq!(with(Some(0xA0)), Some(Pokerus::Cured { strain: 10 }));
}
