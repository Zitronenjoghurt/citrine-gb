use super::gen1::red_rom;
use super::gen2::crystal_rom;
use super::pics::{Planes, encode_gen1};
use super::*;
use crate::{Caught, Gender, Place, RomTables, Side, SpriteError, TimeOfDay};

fn offset(bank: usize, addr: u16) -> usize {
    bank * 0x4000 + usize::from(addr) - 0x4000
}

fn words(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn gen1_record(dex: u8, front: u16, back: u16) -> Vec<u8> {
    let mut record = vec![0u8; 28];
    record[0] = dex;
    record[1..6].copy_from_slice(&[40, 50, 60, 70, 80]);
    record[6] = 0x14;
    record[7] = 0x02;
    record[10] = 0x55;
    record[11..13].copy_from_slice(&front.to_le_bytes());
    record[13..15].copy_from_slice(&back.to_le_bytes());
    record[19] = 3;
    record
}

fn gen1_planes(bank: usize) -> (Planes, Planes) {
    (
        Planes::random(5, 5, bank as u32, 40),
        Planes::random(4, 4, 100 + bank as u32, 40),
    )
}

fn type_table(rom: &mut [u8], code_at: usize, table_addr: u16, count: usize) {
    let bank = code_at / 0x4000;
    let mut code = vec![0x87, 0x21];
    code.extend(table_addr.to_le_bytes());
    code.extend([0x5F, 0x16, 0x00, 0x19, 0x2A, 0x5F, 0x56, 0xE1]);
    put(rom, code_at, &code);
    let names_addr = table_addr + 0x100;
    let mut names = Vec::new();
    let mut pointers = Vec::new();
    for i in 0..count {
        pointers.push(names_addr + names.len() as u16);
        names.extend(en(&label("TYPE", i)));
        names.push(0x50);
    }
    put(rom, offset(bank, table_addr), &words(&pointers));
    put(rom, offset(bank, names_addr), &names);
}

fn red_rom_with_pictures() -> Vec<u8> {
    let mut rom = red_rom();
    let bank_code = [
        0xFA, 0x1E, 0xD0, 0x47, 0xFE, 0x01, 0x3E, 0x01, 0x28, 0x16, 0x78, 0xFE, 0x1F, 0x3E, 0x09,
        0x38, 0x10, 0x78, 0xFE, 0x4A, 0x3E, 0x0A, 0x38, 0x0A, 0x78, 0xFE, 0x74, 0x3E, 0x0B, 0x38,
        0x04, 0x3E, 0x0C, 0xC3, 0x00, 0x15,
    ];
    put(&mut rom, 0x2000, &bank_code);
    let mut mew_code = vec![0x21];
    mew_code.extend(0x7800u16.to_le_bytes());
    mew_code.extend([
        0x11, 0x00, 0xD0, 0x01, 0x1C, 0x00, 0x3E, 0x01, 0xCD, 0x00, 0x00,
    ]);
    put(&mut rom, 0x2100, &mew_code);
    put(&mut rom, 0x7800, &gen1_record(151, 0x6000, 0x6800));
    let table: Vec<u8> = (1..=150)
        .flat_map(|dex| gen1_record(dex, 0x6000, 0x6800))
        .collect();
    put(&mut rom, 0x38000, &table);
    for bank in [1, 0x0A, 0x0B, 0x0C] {
        let (front, back) = gen1_planes(bank);
        put(&mut rom, offset(bank, 0x6000), &encode_gen1(&front, 2, 1));
        put(&mut rom, offset(bank, 0x6800), &encode_gen1(&back, 1, 0));
    }
    let palette_bank = 0x1D;
    put(
        &mut rom,
        offset(palette_bank, 0x4000),
        &[0x5F, 0x16, 0x00, 0x21, 0x00, 0x50, 0x19, 0x7E, 0xC9],
    );
    put(
        &mut rom,
        offset(palette_bank, 0x4010),
        &[0x11, 0x00, 0x51, 0x21, 0x00, 0x52, 0xCD, 0x00, 0x00],
    );
    let ids: Vec<u8> = (0..152).map(|dex| (dex % 10) as u8).collect();
    put(&mut rom, offset(palette_bank, 0x5000), &ids);
    put(&mut rom, offset(palette_bank, 0x5100), &[0x59]);
    let colors: Vec<u16> = (0..10u16)
        .flat_map(|id| [0x7FFF, 0x100 + id, 0x200 + id, 0])
        .collect();
    put(&mut rom, offset(palette_bank, 0x5200), &words(&colors));
    type_table(&mut rom, offset(palette_bank, 0x4020), 0x6000, 0x1B);
    rom
}

#[test]
fn gen1_pictures_come_from_the_bank_the_game_picks() {
    let rom = red_rom_with_pictures();
    let tables = RomTables::load(game(Version::Red, Language::English), &rom).expect("tables");
    for (dex, bank) in [
        (1, 0x0C),
        (40, 0x0C),
        (80, 0x0B),
        (117, 0x0B),
        (118, 0x0A),
        (150, 0x0A),
        (151, 1),
    ] {
        let species = tables.species_for_dex(dex).expect("species");
        let (front, back) = gen1_planes(bank);
        let sprite = tables
            .sprite(&rom, species, None, Side::Front)
            .unwrap_or_else(|e| panic!("dex {dex}: {e}"));
        assert_eq!((sprite.width, sprite.height), (5, 5));
        for y in 0..40 {
            for x in 0..40 {
                assert_eq!(sprite.pixel(x, y), front.shade(x, y), "dex {dex}");
            }
        }
        let sprite = tables
            .sprite(&rom, species, None, Side::Back)
            .expect("back");
        assert_eq!((sprite.width, sprite.height), (4, 4));
        assert!((0..32).all(|y| (0..32).all(|x| sprite.pixel(x, y) == back.shade(x, y))));
    }
    assert_eq!(
        tables.sprite(&rom, 2, None, Side::Front),
        Err(SpriteError::NoSpecies)
    );
}

#[test]
fn gen1_palettes_types_and_base_stats_come_from_the_rom() {
    let rom = red_rom_with_pictures();
    let tables = RomTables::load(game(Version::Red, Language::English), &rom).expect("tables");
    let mew = tables.species_for_dex(151).expect("mew");
    let first = tables.species_for_dex(1).expect("first");
    assert_eq!(tables.base_stats(mew).map(|b| b.dex), Some(151));
    let base = tables.base_stats(first).expect("base");
    assert_eq!(
        (base.hp, base.special_attack, base.special_defense),
        (40, 80, 80)
    );
    assert_eq!(base.gender_ratio, None);
    let palette = tables.palette(first, false).expect("palette");
    assert_eq!(palette.colors, [0x7FFF, 0x101, 0x201, 0]);
    assert_eq!(tables.palette(mew, true).expect("mew").colors[1], 0x101);
    assert_eq!(
        tables
            .palette(tables.species_for_dex(27).unwrap(), false)
            .unwrap()
            .colors[2],
        0x207
    );
    assert_eq!(tables.type_name(0x14), Some("TYPEAU"));
    assert_eq!(tables.species_types(first), vec!["TYPEAU", "TYPEAC"]);
    assert_eq!(tables.type_name(0x1B), None);
}

pub(crate) fn crystal_rom_with_pictures() -> Vec<u8> {
    let mut rom = crystal_rom();
    let mut fix = vec![
        0xE5, 0xC5, 0xD6, 0x12, 0x4F, 0x06, 0x00, 0x21, 0x0F, 0x40, 0x09, 0x7E, 0xC1, 0xE1, 0xC9,
    ];
    fix.extend((0..24).map(|i| 0x48 + i as u8));
    put(&mut rom, offset(4, 0x4000), &fix);
    put(
        &mut rom,
        offset(4, 0x4100),
        &[0x78, 0x16, 0x48, 0xFE, 0xC9, 0x20, 0x03, 0x79, 0x16, 0x50],
    );
    let base: Vec<u8> = (1..=251usize)
        .flat_map(|dex| {
            let mut record = vec![0u8; 32];
            record[0] = dex as u8;
            record[1..7].copy_from_slice(&[40, 50, 60, 70, 80, 90]);
            record[7] = 0x14;
            record[8] = 0x14;
            record[13] = [0x00, 0x1F, 0x7F, 0xFE, 0xFF][dex % 5];
            record[15] = 20;
            record[17] = [0x55, 0x66, 0x77][dex % 3];
            record[22] = 0;
            record
        })
        .collect();
    put(&mut rom, 0x60000, &base);
    let mut pointers = Vec::new();
    for dex in 1..=251usize {
        let n = [5usize, 6, 7][dex % 3];
        let front = 0x4000 + dex as u16 * 8;
        let back = 0x6000 + dex as u16 * 8;
        pointers.extend([0x13, front as u8, (front >> 8) as u8, 0x13]);
        pointers.extend(back.to_le_bytes());
        let need = n * n * 16 - 1;
        put(
            &mut rom,
            offset(0x49, front),
            &[0xE4 | (need >> 8) as u8, need as u8, dex as u8, 0xFF],
        );
        put(
            &mut rom,
            offset(0x49, back),
            &[0xE6, 0x3F, !(dex as u8), 0xFF],
        );
    }
    put(&mut rom, offset(0x48, 0x4000), &pointers);
    let mut unown = Vec::new();
    for letter in 0..26u16 {
        let front = 0x4000 + letter * 8;
        let back = 0x6000 + letter * 8;
        unown.push(0x14);
        unown.extend(front.to_le_bytes());
        unown.push(0x14);
        unown.extend(back.to_le_bytes());
        put(
            &mut rom,
            offset(0x4A, front),
            &[0xE5, 0x8F, 0x80 + letter as u8, 0xFF],
        );
        put(
            &mut rom,
            offset(0x4A, back),
            &[0xE6, 0x3F, 0x40 + letter as u8, 0xFF],
        );
    }
    put(&mut rom, offset(0x50, 0x4000), &unown);
    let bank = 9;
    put(
        &mut rom,
        offset(bank, 0x4000),
        &[
            0x6F, 0x26, 0x00, 0x29, 0x29, 0x29, 0x01, 0x00, 0x50, 0x09, 0xC9,
        ],
    );
    let palettes: Vec<u16> = (0..=251u16)
        .flat_map(|dex| [0x1000 + dex, 0x2000 + dex, 0x3000 + dex, 0x4000 + dex])
        .collect();
    put(&mut rom, offset(bank, 0x5000), &words(&palettes));
    put(
        &mut rom,
        offset(bank, 0x4020),
        &[
            0x6B, 0x26, 0x00, 0x29, 0x29, 0x11, 0x02, 0x60, 0x19, 0x2A, 0x66, 0x6F,
        ],
    );
    let mut landmarks = Vec::new();
    let mut names = Vec::new();
    for i in 0..0x60 {
        landmarks.extend([0, 0]);
        landmarks.extend((0x6400 + names.len() as u16).to_le_bytes());
        let mut name = en(&label("PLACE", i));
        if i == 3 {
            name = [en("NEW"), vec![0x1F], en("TOWN"), vec![0x25]].concat();
        }
        names.extend(name);
        names.push(0x50);
    }
    put(&mut rom, offset(bank, 0x6000), &landmarks);
    put(&mut rom, offset(bank, 0x6400), &names);
    type_table(&mut rom, offset(bank, 0x4040), 0x6800, 0x1C);
    rom
}

fn uniform(sprite: &crate::Sprite, byte: u8) -> bool {
    (0..sprite.height_px()).all(|y| {
        (0..sprite.width_px()).all(|x| {
            let bit = (byte >> (7 - x % 8)) & 1;
            sprite.pixel(x, y) == bit * 3
        })
    })
}

#[test]
fn gen2_pictures_follow_the_bank_fix_and_the_unown_letter() {
    let rom = crystal_rom_with_pictures();
    let tables = RomTables::load(game(Version::Crystal, Language::English), &rom).expect("tables");
    for dex in [1u8, 2, 3, 150, 251] {
        let n = [5, 6, 7][usize::from(dex) % 3];
        let front = tables
            .sprite(&rom, dex, None, Side::Front)
            .unwrap_or_else(|e| panic!("{dex}: {e}"));
        assert_eq!((front.width, front.height), (n, n));
        assert!(uniform(&front, dex), "{dex}");
        let back = tables.sprite(&rom, dex, None, Side::Back).expect("back");
        assert_eq!((back.width, back.height), (6, 6));
        assert!(uniform(&back, !dex), "{dex}");
    }
    for (letter, value) in [('A', 0x80), ('C', 0x82), ('Z', 0x99)] {
        let sprite = tables
            .sprite(&rom, 201, Some(letter), Side::Front)
            .expect("unown");
        assert!(uniform(&sprite, value), "{letter}");
        let back = tables
            .sprite(&rom, 201, Some(letter), Side::Back)
            .expect("unown back");
        assert!(uniform(&back, value - 0x40), "{letter}");
    }
    let plain = tables.sprite(&rom, 201, None, Side::Front).expect("unown");
    assert!(uniform(&plain, 0x80));
    let ignored = tables
        .sprite(&rom, 200, Some('C'), Side::Front)
        .expect("200");
    assert!(uniform(&ignored, 200));
    assert_eq!(
        tables.sprite(&rom, 0, None, Side::Front),
        Err(SpriteError::NoSpecies)
    );
}

#[test]
fn gen2_palettes_names_and_formulas_come_from_the_rom() {
    let rom = crystal_rom_with_pictures();
    let tables = RomTables::load(game(Version::Crystal, Language::English), &rom).expect("tables");
    let normal = tables.palette(25, false).expect("normal");
    let shiny = tables.palette(25, true).expect("shiny");
    assert_eq!(normal.colors, [0x7FFF, 0x1019, 0x2019, 0]);
    assert_eq!(shiny.colors, [0x7FFF, 0x3019, 0x4019, 0]);
    assert_eq!(tables.type_name(0x14), Some("TYPEAU"));
    assert_eq!(tables.species_types(25), vec!["TYPEAU"]);
    let caught = |location| Caught {
        time_of_day: TimeOfDay::Day,
        level: 5,
        ot_gender: Gender::Female,
        location,
    };
    assert_eq!(
        tables.caught_place(&caught(1)),
        Some(Place::Landmark("PLACEAB"))
    );
    assert_eq!(
        tables.caught_place(&caught(3)),
        Some(Place::Landmark("NEW TOWN"))
    );
    assert_eq!(tables.caught_place(&caught(0x7E)), Some(Place::Gift));
    assert_eq!(tables.caught_place(&caught(0x7F)), Some(Place::Event));
    assert_eq!(tables.caught_place(&caught(0x70)), None);
    assert_eq!(
        tables.base_stats(25).map(|b| b.gender_ratio),
        Some(Some(0x00))
    );
    assert_eq!(tables.base_stats(25).map(|b| b.egg_cycles), Some(Some(20)));
}

#[test]
fn tables_missing_from_the_rom_leave_sprites_unavailable() {
    let rom = red_rom();
    let tables = RomTables::load(game(Version::Red, Language::English), &rom).expect("tables");
    assert_eq!(
        tables.sprite(&rom, 190, None, Side::Front),
        Err(SpriteError::NoTable)
    );
    assert_eq!(tables.palette(190, false), None);
    assert_eq!(tables.base_stats(190), None);
    assert_eq!(tables.type_name(0), None);
}

#[test]
fn base_stats_parse_from_a_single_record() {
    use crate::{BaseStats, Generation, GrowthRate};
    let record = gen1_record(151, 0x5500, 0x6600);
    let mew = BaseStats::from_record(Generation::One, &record).expect("gen 1 record");
    assert_eq!((mew.dex, mew.hp, mew.special_defense), (151, 40, 80));
    assert_eq!(mew.growth_rate, GrowthRate::MediumSlow);
    assert_eq!(mew.egg_cycles, None);
    assert!(BaseStats::from_record(Generation::One, &record[..27]).is_none());

    let mut record = [0u8; 32];
    record[0] = 172;
    record[15] = 10;
    record[22] = 0;
    let pichu = BaseStats::from_record(Generation::Two, &record).expect("gen 2 record");
    assert_eq!((pichu.dex, pichu.egg_cycles), (172, Some(10)));
    record[22] = 9;
    assert!(BaseStats::from_record(Generation::Two, &record).is_none());
}
