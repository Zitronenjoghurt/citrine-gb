use super::gen1::red_rom;
use super::*;
use crate::{ReadError, RomTables};

#[test]
fn gen1_names_come_from_the_rom() {
    let tables =
        RomTables::load(game(Version::Red, Language::English), &red_rom()).expect("tables");
    assert_eq!(tables.species_name(1), Some("MAA"));
    assert_eq!(tables.species_name(190), Some("MHH"));
    assert_eq!(tables.species_name(0), None);
    assert_eq!(tables.move_name(1), Some("MOVEAA"));
    assert_eq!(tables.move_name(165), Some("MOVEGI"));
    assert_eq!(tables.item_name(1).as_deref(), Some("ITEMAA"));
    assert_eq!(tables.item_name(0xC4).as_deref(), Some("HM01"));
    assert_eq!(tables.item_name(0xC9).as_deref(), Some("TM01"));
    assert_eq!(tables.item_name(0xFA).as_deref(), Some("TM50"));
    assert!(tables.is_hm(0xC4));
    assert!(tables.is_hm(0xC8));
    assert!(!tables.is_hm(0xC9));
}

#[test]
fn gen1_pokedex_order_maps_both_ways() {
    let tables =
        RomTables::load(game(Version::Red, Language::English), &red_rom()).expect("tables");
    assert_eq!(tables.dex_number(1), Some(151));
    assert_eq!(tables.dex_number(2), None);
    assert_eq!(tables.dex_number(190), Some(1));
    assert_eq!(tables.dex_number(191), None);
    assert_eq!(tables.species_for_dex(151), Some(1));
    assert_eq!(tables.species_for_dex(150), Some(41));
    assert_eq!(tables.species_for_dex(1), Some(190));
    assert_eq!(tables.species_for_dex(152), None);
    assert_eq!(tables.pokedex_size(), 151);
}

#[test]
fn a_rom_without_name_tables_is_refused() {
    let rom = vec![0u8; 0x80000];
    assert_eq!(
        RomTables::load(game(Version::Red, Language::English), &rom).err(),
        Some(ReadError::RomTables)
    );
    assert_eq!(
        RomTables::load(game(Version::Gold, Language::English), &rom).err(),
        Some(ReadError::RomTables)
    );
}

#[test]
fn a_japanese_rom_with_latin_names_is_refused() {
    let rom = japanese_cartridge_in_latin();
    let game = crate::identify(&rom).expect("identified");
    assert_eq!(game.language, Language::Japanese);
    assert_eq!(
        RomTables::load(game, &rom).err(),
        Some(ReadError::RomTables)
    );
}

#[test]
fn a_japanese_rom_loads_with_kana_names_and_its_own_checksum() {
    let mut rom = japanese_cartridge_in_latin();
    let kana: Vec<u8> = (0..251u8)
        .flat_map(|i| name(&[0x80 + i % 0x1A, 0x05 + i % 0x0F, 0xE3], 5))
        .collect();
    put(&mut rom, 0x53384, &kana);
    seal_cartridge(&mut rom);
    let game = crate::identify(&rom).expect("identified");
    assert!(RomTables::load(game, &rom).is_ok());

    rom[0x4000] ^= 1;
    assert_eq!(
        RomTables::load(game, &rom).err(),
        Some(ReadError::RomTables)
    );
}

#[test]
fn machines_name_their_moves() {
    let mut rom = crystal_rom();
    let machines: Vec<u8> = [223u8, 29, 174, 205, 46, 92]
        .into_iter()
        .chain(7..58)
        .collect();
    put(&mut rom, 0x1167A, &machines);
    let tables = RomTables::load(game(Version::Crystal, Language::English), &rom).expect("tables");
    assert_eq!(tables.machine_move(0xBF), tables.move_name(223));
    assert_eq!(tables.machine_move(0xC4), tables.move_name(46));
    assert_eq!(tables.machine_move(0xC3), None);
    assert_eq!(tables.machine_move(0xDC), None);
    assert_eq!(tables.machine_move(0xF3), tables.move_name(51));
    assert_eq!(tables.machine_move(0xF9), tables.move_name(57));
    assert_eq!(tables.machine_move(0xFA), None);
    assert!(tables.is_hm(0xF3));
    assert!(tables.is_hm(0xF9));
    assert!(!tables.is_hm(0xF2));
    assert!(!tables.is_hm(0xC4));
    assert_eq!(tables.machine_move(0x01), None);
}
