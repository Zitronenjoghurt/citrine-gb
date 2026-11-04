use super::world::{at, crystal_world_rom, location, red_world_rom};
use super::*;
use crate::{Palette, Region, RomTables};

const PAPER: u16 = 0x7FFF;
const GEN2_PALETTE: Palette = Palette {
    colors: [PAPER, 0x03E0, 0x7C00, 0x0000],
};

fn tile(shade: u8) -> [u8; 16] {
    let low = if shade & 1 != 0 { 0xFF } else { 0x00 };
    let high = if shade & 2 != 0 { 0xFF } else { 0x00 };
    let mut bytes = [0u8; 16];
    for row in 0..8 {
        bytes[2 * row] = low;
        bytes[2 * row + 1] = high;
    }
    bytes
}

fn literal_lz(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(32) {
        out.push(chunk.len() as u8 - 1);
        out.extend_from_slice(chunk);
    }
    out.push(0xFF);
    out
}

fn gen1_town_map(rom: &mut [u8]) {
    let bank = 0x1E;
    let tiles: Vec<u8> = (0..16u8).flat_map(|t| tile(t % 4)).collect();
    put(rom, at(bank, 0x4000), &tiles);
    let mut code = vec![
        0x21, 0x00, 0x40, 0x11, 0x00, 0x96, 0x01, 0x00, 0x01, 0x3E, bank as u8,
    ];
    code.extend([0xCD, 0x00, 0x00]);
    code.extend([
        0x21, 0x00, 0x00, 0x11, 0x40, 0x80, 0x01, 0x40, 0x00, 0x3E, 0x00,
    ]);
    code.extend([0xCD, 0x00, 0x00, 0x21, 0xA0, 0xC3]);
    code.extend([
        0x11, 0x00, 0x60, 0x1A, 0xA7, 0x28, 0x0A, 0x47, 0xE6, 0x0F, 0x4F, 0x78, 0xCB, 0x37, 0xE6,
        0x0F, 0xC6, 0x60,
    ]);
    put(rom, at(bank, 0x5000), &code);
    let runs: Vec<u8> = (0..24)
        .map(|i| if i % 2 == 0 { 0x1F } else { 0x2F })
        .collect();
    put(rom, at(bank, 0x6000), &runs);
}

fn gen2_town_map(rom: &mut [u8]) {
    let bank = 0x70u8;
    let tiles: Vec<u8> = (0..48u8).flat_map(|t| tile(t % 4)).collect();
    put(rom, at(bank.into(), 0x4000), &literal_lz(&tiles));
    put(
        rom,
        0x3A00,
        &[
            0x21, 0x00, 0x40, 0x11, 0x00, 0x90, 0x01, 0x30, bank, 0xCD, 0x00, 0x00, 0xC9,
        ],
    );
    put(
        rom,
        at(bank.into(), 0x5000),
        &[
            0x11, 0x00, 0x51, 0x18, 0x03, 0x11, 0x00, 0x53, 0x21, 0x00, 0xC4, 0x1A, 0xFE, 0xFF,
            0xC8,
        ],
    );
    put(rom, at(bank.into(), 0x5100), &[1; 360]);
    put(rom, at(bank.into(), 0x5300), &[2; 360]);
    put(
        rom,
        at(bank.into(), 0x5600),
        &[0xFE, 0x60, 0x30, 0x05, 0x21, 0x00, 0x57, 0xCB, 0x3F, 0x38],
    );
    put(rom, at(bank.into(), 0x5700), &[0; 48]);
    put(
        rom,
        at(bank.into(), 0x5800),
        &[0x21, 0x00, 0x59, 0x11, 0x00, 0xD0, 0x01, 0x30, 0x00],
    );
    let palettes: Vec<u8> = (0..6)
        .flat_map(|_| GEN2_PALETTE.colors)
        .flat_map(u16::to_le_bytes)
        .collect();
    put(rom, at(bank.into(), 0x5900), &palettes);
    for (landmark, x, y) in [(1usize, 148u8, 116u8), (0x2F, 60, 124)] {
        put(rom, at(0x61, 0x4100) + 4 * landmark, &[x, y]);
    }
}

fn rgba(palette: &Palette, shade: u8) -> [u8; 4] {
    let [r, g, b] = palette.rgb(shade);
    [r, g, b, 0xFF]
}

#[test]
fn a_gen1_town_map_is_drawn_from_its_runs_and_marks_the_place() {
    let mut rom = red_world_rom();
    gen1_town_map(&mut rom);
    let tables = RomTables::load(game(Version::Red, Language::English), &rom).expect("tables");
    let map = tables.region_map(Region::Kanto).expect("town map");
    assert_eq!((map.width, map.height), (160, 144));
    assert_eq!(map.pixel(0, 0), rgba(&Palette::GRAY, 1));
    assert_eq!(map.pixel(15 * 8, 0), rgba(&Palette::GRAY, 2));
    let marker = tables.marker(&location(0, 0, None)).expect("marker");
    assert_eq!((marker.name, marker.x, marker.y), ("PALLET TOWN", 36, 100));
    assert!(std::ptr::eq(marker.map, map));
}

#[test]
fn a_gen2_town_map_has_both_regions_and_marks_each_place() {
    let mut rom = crystal_world_rom();
    gen2_town_map(&mut rom);
    let tables = RomTables::load(game(Version::Crystal, Language::English), &rom).expect("tables");
    let johto = tables.region_map(Region::Johto).expect("johto");
    let kanto = tables.region_map(Region::Kanto).expect("kanto");
    assert_eq!((johto.width, johto.height), (144, 120));
    assert_eq!(johto.pixel(80, 64), rgba(&GEN2_PALETTE, 1));
    assert_eq!(kanto.pixel(80, 64), rgba(&GEN2_PALETTE, 2));
    let home = tables
        .marker(&location(24, 3, Some((24, 1))))
        .expect("new bark");
    assert_eq!((home.name, home.x, home.y), ("NEW BARK TOWN", 132, 84));
    assert!(std::ptr::eq(home.map, johto));
    let pallet = tables.marker(&location(1, 2, None)).expect("pallet");
    assert_eq!((pallet.name, pallet.x, pallet.y), ("PALLET TOWN", 44, 92));
    assert!(std::ptr::eq(pallet.map, kanto));
}

#[test]
fn a_rom_without_a_town_map_has_no_marker() {
    let tables = RomTables::load(
        game(Version::Crystal, Language::English),
        &crystal_world_rom(),
    )
    .expect("tables");
    assert!(tables.region_map(Region::Johto).is_none());
    assert!(tables.marker(&location(24, 3, Some((24, 1)))).is_none());
}
