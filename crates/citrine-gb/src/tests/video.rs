use super::e2e::LedgerRoms;
use crate::debug::code::{Address, BlockId};
use crate::debug::tiles::Picture;
use crate::debug::video::{SheetColors, Sprite};
use crate::gb::ppu::types::color::ColorCorrection;
use crate::gb::ppu::types::theme::DmgTheme;
use crate::gb::{GameBoy, GbModel};
use crate::rom::Rom;
use crate::tests::skip;
use std::path::PathBuf;

const DMG_ACID2: &str = "464E14B7D42E7FEEA0B7EDE42BE7071DC88913F75B9FFA444299424B63D1DFF1";
const CGB_ACID2: &str = "197FB0BCEC544F0400527FC707E0A94F55435974986E6986B424ACE5DE81720E";
const FRAMES: usize = 30;
const TOBU: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb");

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/video")
        .join(format!("{name}.png"))
}

fn read_png(path: &PathBuf) -> Picture {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .expect("png header");
    let mut rgba = vec![0; reader.output_buffer_size().expect("png size")];
    let info = reader.next_frame(&mut rgba).expect("png frame");
    rgba.truncate(info.buffer_size());
    Picture {
        width: info.width as usize,
        height: info.height as usize,
        rgba,
    }
}

pub(super) fn matches_fixture(name: &str, picture: &Picture) {
    let path = fixture(name);
    if std::env::var_os("CITRINE_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().expect("fixture dir")).expect("fixture dir");
        std::fs::write(&path, picture.png()).expect("write fixture");
    }
    let committed = read_png(&path);
    assert_eq!(
        (committed.width, committed.height),
        (picture.width, picture.height),
        "{name}"
    );
    assert!(
        committed.rgba == picture.rgba,
        "{name} differs from {path:?}"
    );
}

fn acid2(test: &str, sha: &str, model: GbModel) -> Option<GameBoy> {
    let rom = LedgerRoms::load().and_then(|mut roms| roms.find(sha));
    let rom = match rom {
        Ok(Some(rom)) => rom,
        Ok(None) => {
            skip(test, "no ledger archive holds this acid2 ROM");
            return None;
        }
        Err(reason) => {
            skip(test, &reason);
            return None;
        }
    };
    let mut gb = GameBoy::new_empty(model);
    gb.load_rom(&Rom::new(&rom)).expect("load");
    gb.ppu.dmg_theme = DmgTheme::GreyScale;
    gb.ppu.color_correction = ColorCorrection::Raw;
    for _ in 0..FRAMES {
        gb.run_frame();
    }
    Some(gb)
}

fn check_sheets_and_maps(gb: &GameBoy, prefix: &str) {
    let mut sheets = Vec::new();
    gb.tile_sheets(SheetColors::default(), &mut sheets);
    assert!(!sheets.is_empty());
    for sheet in &sheets {
        assert_eq!(sheet.tiles(), 384);
        let name = format!("{prefix}-tiles-{}", sheet.bank);
        matches_fixture(&name, &sheet.picture);
    }
    let mut maps = Vec::new();
    gb.tile_maps(&mut maps);
    assert_eq!(maps.len(), 2);
    for map in &maps {
        let name = format!("{prefix}-map-{}", map.name.trim_start_matches("Map $"));
        matches_fixture(&name, &map.picture);
    }
}

#[test]
fn dmg_acid2_tiles_and_maps_equal_the_committed_pictures() {
    let test = "dmg_acid2_tiles_and_maps_equal_the_committed_pictures";
    let Some(gb) = acid2(test, DMG_ACID2, GbModel::Dmg) else {
        return;
    };
    check_sheets_and_maps(&gb, "dmg-acid2");
}

#[test]
fn cgb_acid2_tiles_and_maps_equal_the_committed_pictures() {
    let test = "cgb_acid2_tiles_and_maps_equal_the_committed_pictures";
    let Some(gb) = acid2(test, CGB_ACID2, GbModel::Cgb) else {
        return;
    };
    let mut sheets = Vec::new();
    gb.tile_sheets(SheetColors::default(), &mut sheets);
    assert_eq!(sheets.len(), 2);
    check_sheets_and_maps(&gb, "cgb-acid2");
}

fn field<'a>(sprite: &'a Sprite, name: &str) -> &'a str {
    sprite
        .fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.as_str())
        .unwrap_or_else(|| panic!("sprite {} has no {name}", sprite.index))
}

fn sprite_fields_equal_the_oam_bytes(gb: &GameBoy, cgb: bool) {
    let mut oam = vec![0; 160];
    gb.peek(Address::at(BlockId::Oam, 0, 0), &mut oam);
    let mut sprites = Vec::new();
    gb.sprites(&mut sprites);
    assert_eq!(sprites.len(), 40);
    for (sprite, bytes) in sprites.iter().zip(oam.as_chunks::<4>().0) {
        assert_eq!(sprite.bytes, bytes);
        let [y, x, tile, flags] = *bytes;
        let yes = |bit: u8| if flags & bit != 0 { "Yes" } else { "No" };
        assert!(field(sprite, "Y position").starts_with(&format!("{y} ")));
        assert!(field(sprite, "X position").starts_with(&format!("{x} ")));
        assert_eq!(field(sprite, "Tile index"), format!("${tile:02X}"));
        assert_eq!(field(sprite, "Behind background"), yes(0x80));
        assert_eq!(field(sprite, "Y flip"), yes(0x40));
        assert_eq!(field(sprite, "X flip"), yes(0x20));
        if cgb {
            assert_eq!(field(sprite, "Bank"), (flags >> 3 & 1).to_string());
            assert_eq!(field(sprite, "Palette"), format!("Sprites {}", flags & 7));
        } else {
            assert_eq!(
                field(sprite, "Palette"),
                format!("Sprites {}", flags >> 4 & 1)
            );
        }
        assert_eq!((sprite.x, sprite.y), (i32::from(x) - 8, i32::from(y) - 16));
    }
}

#[test]
fn sprite_fields_equal_the_oam_bytes_on_both_models() {
    for (model, cgb) in [(GbModel::Dmg, false), (GbModel::Cgb, false)] {
        let mut gb = GameBoy::new_empty(model);
        gb.load_rom(&Rom::new(TOBU)).expect("load");
        for _ in 0..FRAMES {
            gb.run_frame();
        }
        let flags = [0x00, 0xF0, 0x2B, 0xCF];
        for (index, flag) in flags.iter().enumerate() {
            let bytes = [
                16 + index as u8 * 9,
                8 + index as u8 * 20,
                0x40 + index as u8,
                *flag,
            ];
            gb.poke(Address::at(BlockId::Oam, 0, index as u32 * 4), &bytes);
        }
        sprite_fields_equal_the_oam_bytes(&gb, cgb || gb.ppu.cgb_mode());
    }
}

#[test]
fn acid2_sprite_fields_equal_the_oam_bytes() {
    let test = "acid2_sprite_fields_equal_the_oam_bytes";
    for (sha, model) in [(DMG_ACID2, GbModel::Dmg), (CGB_ACID2, GbModel::Cgb)] {
        let Some(gb) = acid2(test, sha, model) else {
            return;
        };
        sprite_fields_equal_the_oam_bytes(&gb, gb.ppu.cgb_mode());
    }
}

#[test]
fn a_dmg_shows_its_three_palette_registers_as_shades() {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&Rom::new(TOBU)).expect("load");
    gb.run_frame();
    let mut palettes = Vec::new();
    gb.palettes(&mut palettes);
    assert_eq!(palettes.len(), 2);
    assert_eq!(palettes[0].values.len(), 4);
    assert_eq!(palettes[1].values.len(), 8);
    assert_eq!(palettes[1].rows(), 2);
    let [bgp, ..] = gb.ppu.dmg_palettes();
    let shades: Vec<u16> = (0..4).map(|at| u16::from(bgp >> (at * 2) & 3)).collect();
    assert_eq!(palettes[0].values, shades);
}

#[test]
fn a_sprite_click_finds_its_tile_in_the_sheet() {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&Rom::new(TOBU)).expect("load");
    gb.run_frame();
    gb.poke(Address::at(BlockId::Oam, 0, 0), &[40, 40, 0x7C, 0]);
    let mut sprites = Vec::new();
    gb.sprites(&mut sprites);
    let mut sheets = Vec::new();
    gb.tile_sheets(SheetColors::Grey, &mut sheets);
    let source = sprites[0].source;
    let sheet = &sheets[source.sheet];
    assert_eq!(sheet.tile_address(source.tile), 0x8000 + 0x7C * 16);
    let rect = sheet.tile_rect(source.tile);
    assert_eq!(sheet.tile_at(rect.x + 3, rect.y + 3), Some(source.tile));
}
