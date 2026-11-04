use super::video::matches_fixture;
use crate::debug::tiles::Depth;
use crate::debug::video::{SheetColors, Sprite};
use crate::gba::machine::Gba;
use crate::tests::skip;
use std::path::PathBuf;

const FRAMES: usize = 120;

fn blank() -> Gba {
    let mut gba = Gba::new(&[0; 0x200]);
    gba.run_frame();
    gba
}

fn field<'a>(sprite: &'a Sprite, name: &str) -> &'a str {
    sprite
        .fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.as_str())
        .unwrap_or_else(|| panic!("sprite {} has no {name}", sprite.index))
}

#[test]
fn the_gba_shows_512_palette_entries_in_rows_of_16() {
    let mut gba = blank();
    gba.bus_mut().write16(0x0500_0002, 0x001F);
    gba.bus_mut().write16(0x0500_03FE, 0x7C00);
    let mut palettes = Vec::new();
    gba.palettes(&mut palettes);
    assert_eq!(palettes.len(), 2);
    for palette in &palettes {
        assert_eq!(palette.colors.len(), 256);
        assert_eq!(palette.rows(), 16);
        assert_eq!(palette.row_colors(1, Depth::Four).len(), 16);
        assert_eq!(palette.row_colors(1, Depth::Eight).len(), 256);
    }
    assert_eq!(palettes[0].values[1], 0x001F);
    assert_eq!(palettes[0].colors[1], [0xFF, 0, 0, 0xFF]);
    assert_eq!(palettes[1].values[255], 0x7C00);
    assert_eq!(palettes[1].colors[255], [0, 0, 0xFF, 0xFF]);
}

#[test]
fn the_gba_sheets_cover_the_96_kb_of_vram_at_both_depths() {
    let gba = blank();
    let mut sheets = Vec::new();
    gba.tile_sheets(SheetColors::Grey, &mut sheets);
    let tiles: Vec<usize> = sheets.iter().map(|sheet| sheet.tiles()).collect();
    assert_eq!(tiles, [2048, 1024, 1024, 512]);
    let bytes: usize = sheets
        .iter()
        .filter(|sheet| sheet.depth == Depth::Four)
        .map(|sheet| sheet.tiles() * 32)
        .sum();
    assert_eq!(bytes, 96 * 1024);
    assert_eq!(sheets[2].start, 0x0601_0000);
    assert_eq!(sheets[2].palette_group, 1);
}

#[test]
fn gba_sprite_fields_equal_the_oam_halfwords() {
    let mut gba = blank();
    let entries: [[u16; 3]; 4] = [
        [0x0010, 0x0020, 0x0001],
        [0x40F0, 0x91F8, 0x7C42],
        [0x2300, 0x4605, 0x0C10],
        [0x8200, 0xC000, 0x0000],
    ];
    for (index, entry) in entries.iter().enumerate() {
        for (half, value) in entry.iter().enumerate() {
            let at = 0x0700_0000 + (index * 8 + half * 2) as u32;
            gba.bus_mut().write16(at, *value);
        }
    }
    let mut sprites = Vec::new();
    gba.sprites(&mut sprites);
    assert_eq!(sprites.len(), 128);
    for (sprite, [attr0, attr1, attr2]) in sprites.iter().zip(entries) {
        let bytes: Vec<u8> = [attr0, attr1, attr2]
            .iter()
            .flat_map(|half| half.to_le_bytes())
            .collect();
        assert_eq!(sprite.bytes, bytes);
        let affine = attr0 & 0x100 != 0;
        let yes = |on: bool| if on { "Yes" } else { "No" };
        assert!(field(sprite, "Y position").starts_with(&format!("{} ", attr0 & 0xFF)));
        assert!(field(sprite, "X position").starts_with(&format!("{} ", attr1 & 0x1FF)));
        assert_eq!(
            field(sprite, "Tile index"),
            format!("${:03X}", attr2 & 0x3FF)
        );
        assert_eq!(field(sprite, "Priority"), (attr2 >> 10 & 3).to_string());
        assert_eq!(field(sprite, "Affine"), yes(affine));
        assert_eq!(field(sprite, "Mosaic"), yes(attr0 & 0x1000 != 0));
        let wide = attr0 & 0x2000 != 0;
        assert_eq!(field(sprite, "Colours"), if wide { "256" } else { "16" });
        if affine {
            assert_eq!(
                field(sprite, "Affine group"),
                (attr1 >> 9 & 0x1F).to_string()
            );
        } else {
            assert_eq!(field(sprite, "X flip"), yes(attr1 & 0x1000 != 0));
            assert_eq!(field(sprite, "Y flip"), yes(attr1 & 0x2000 != 0));
            assert_eq!(field(sprite, "Hidden"), yes(attr0 & 0x200 != 0));
        }
        if !wide {
            assert_eq!(field(sprite, "Palette"), format!("Sprites {}", attr2 >> 12));
        }
    }
    assert_eq!(field(&sprites[1], "Size"), "32x16");
    assert_eq!(field(&sprites[3], "Size"), "32x64");
    assert_eq!((sprites[1].x, sprites[1].y), (-8, 240));
    assert!(!sprites[1].on_screen);
    assert!(!sprites[3].on_screen);
}

#[test]
fn mode_0_lists_four_text_maps_and_mode_4_its_two_pages() {
    let mut gba = blank();
    gba.bus_mut().write16(0x0400_0000, 0x0100);
    let mut maps = Vec::new();
    gba.tile_maps(&mut maps);
    let names: Vec<&str> = maps.iter().map(|map| map.name.as_str()).collect();
    assert_eq!(names, ["BG0", "BG1, off", "BG2, off", "BG3, off"]);
    assert_eq!(maps[0].picture.width, 256);
    gba.bus_mut().write16(0x0400_0008, 0xC000);
    gba.tile_maps(&mut maps);
    assert_eq!((maps[0].picture.width, maps[0].picture.height), (512, 512));
    assert_eq!(maps[0].cells.len(), 64 * 64);
    gba.bus_mut().write16(0x0400_0000, 0x0414);
    gba.tile_maps(&mut maps);
    let names: Vec<&str> = maps.iter().map(|map| map.name.as_str()).collect();
    assert_eq!(names, ["Page 0", "Page 1, shown"]);
}

fn tonc(test: &str, demo: &str) -> Option<Gba> {
    let name = demo.rsplit('/').next().unwrap_or(demo);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.gba-roms/src")
        .join(demo)
        .join(format!("{name}.gba"));
    let Ok(rom) = std::fs::read(&path) else {
        skip(test, "the libtonc-examples builds live in .gba-roms/src");
        return None;
    };
    let mut gba = Gba::new(&rom);
    for _ in 0..FRAMES {
        gba.run_frame();
    }
    Some(gba)
}

fn check_demo(test: &str, demo: &str, sheets: &[usize], maps: &[usize], sprites: &[usize]) {
    let Some(gba) = tonc(test, demo) else {
        return;
    };
    let name = demo.rsplit('/').next().unwrap_or(demo);
    for &index in sheets {
        let mut found = Vec::new();
        gba.tile_sheets(SheetColors::Grey, &mut found);
        let group = found[index].palette_group;
        found.clear();
        gba.tile_sheets(SheetColors::Palette { group, row: 0 }, &mut found);
        matches_fixture(&format!("tonc-{name}-tiles-{index}"), &found[index].picture);
    }
    let mut found = Vec::new();
    gba.tile_maps(&mut found);
    for &index in maps {
        matches_fixture(&format!("tonc-{name}-map-{index}"), &found[index].picture);
    }
    let mut found = Vec::new();
    gba.sprites(&mut found);
    for &index in sprites {
        matches_fixture(
            &format!("tonc-{name}-sprite-{index}"),
            &found[index].picture,
        );
    }
}

#[test]
fn tonc_brin_demo_tiles_and_map_equal_the_committed_pictures() {
    check_demo("tonc_brin_demo", "basic/brin_demo", &[0], &[0], &[]);
}

#[test]
fn tonc_cbb_demo_tiles_and_maps_equal_the_committed_pictures() {
    check_demo("tonc_cbb_demo", "basic/cbb_demo", &[0, 1], &[0, 1], &[]);
}

#[test]
fn tonc_sbb_reg_wide_map_equals_the_committed_picture() {
    check_demo("tonc_sbb_reg", "basic/sbb_reg", &[], &[0], &[]);
}

#[test]
fn tonc_obj_demo_sprite_and_tiles_equal_the_committed_pictures() {
    check_demo("tonc_obj_demo", "basic/obj_demo", &[2], &[], &[0]);
}

#[test]
fn tonc_m3_demo_bitmap_equals_the_committed_picture() {
    check_demo("tonc_m3_demo", "basic/m3_demo", &[], &[0], &[]);
}

#[test]
fn tonc_pageflip_pages_equal_the_committed_pictures() {
    check_demo("tonc_pageflip", "basic/pageflip", &[], &[0, 1], &[]);
}
