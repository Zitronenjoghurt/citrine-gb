use super::{
    ColorValue, Field, Frame, MapCell, Palette, SheetColors, Sprite, TileMap, TileRef, TileSheet,
    address, sheet_colors, yes_no,
};
use crate::debug::tiles::{CLEAR, Depth, Flip, Picture, decode, sheet, wrapped};
use crate::gba::machine::Gba;
use crate::gba::ppu::{HEIGHT, WIDTH, rgba};

const VRAM_START: u32 = 0x0600_0000;
const OAM_START: u32 = 0x0700_0000;
const BG_VRAM: usize = 0x1_0000;
const OBJ_VRAM: usize = 0x8000;
const SHEET_COLUMNS: usize = 32;
const CHAR_BLOCK: usize = 0x4000;
const SCREEN_BLOCK: usize = 0x800;
const MAP_SIZES: [(usize, usize); 4] = [(256, 256), (512, 256), (256, 512), (512, 512)];
const SPRITE_SIZES: [[(usize, usize); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];
const SPRITES: usize = 128;
const BITMAP_PAGE: usize = 0xA000;

const SHEETS: [(&str, usize, usize, Depth, usize); 4] = [
    ("Background, 16 colours", 0, BG_VRAM, Depth::Four, 0),
    ("Background, 256 colours", 0, BG_VRAM, Depth::Eight, 0),
    ("Sprites, 16 colours", BG_VRAM, OBJ_VRAM, Depth::Four, 1),
    ("Sprites, 256 colours", BG_VRAM, OBJ_VRAM, Depth::Eight, 1),
];

fn read16(memory: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([memory[at], memory[at + 1]])
}

fn sheet_of(sprites: bool, depth: Depth) -> usize {
    usize::from(sprites) * 2 + usize::from(depth == Depth::Eight)
}

impl Gba {
    pub fn palettes(&self, out: &mut Vec<Palette>) {
        out.clear();
        let (_, palette, _) = self.bus().video_memory();
        for (name, start) in [("Background", 0), ("Sprites", 0x200)] {
            let values: Vec<u16> = (0..256)
                .map(|index| read16(palette, start + index * 2) & 0x7FFF)
                .collect();
            out.push(Palette {
                name,
                row: 16,
                kind: ColorValue::Rgb555,
                colors: values.iter().map(|value| rgba(*value)).collect(),
                values,
            });
        }
    }

    pub fn tile_sheets(&self, colors: SheetColors, out: &mut Vec<TileSheet>) {
        out.clear();
        let (vram, _, _) = self.bus().video_memory();
        let mut palettes = Vec::new();
        self.palettes(&mut palettes);
        for (name, start, size, depth, palette_group) in SHEETS {
            let colors = sheet_colors(&palettes, colors, depth);
            out.push(TileSheet {
                name: name.to_string(),
                start: VRAM_START + start as u32,
                bank: 0,
                depth,
                columns: SHEET_COLUMNS,
                palette_group,
                picture: sheet(&vram[start..start + size], depth, SHEET_COLUMNS, &colors),
            });
        }
    }

    pub fn tile_maps(&self, out: &mut Vec<TileMap>) {
        out.clear();
        let ppu = self.bus().ppu();
        let dispcnt = ppu.dispcnt();
        match ppu.mode() {
            0..=2 => {
                let text: &[usize] = match ppu.mode() {
                    0 => &[0, 1, 2, 3],
                    1 => &[0, 1],
                    _ => &[],
                };
                for &index in text {
                    out.push(self.text_map(index, dispcnt & (1 << (8 + index)) != 0));
                }
            }
            3 => out.push(self.bitmap("Bitmap", 0, false)),
            4 => {
                let shown = usize::from(dispcnt & (1 << 4) != 0);
                for page in 0..2 {
                    let name = if page == shown {
                        format!("Page {page}, shown")
                    } else {
                        format!("Page {page}")
                    };
                    out.push(self.bitmap(&name, page * BITMAP_PAGE, true));
                }
            }
            _ => {}
        }
    }

    fn text_map(&self, index: usize, enabled: bool) -> TileMap {
        let (vram, palette, _) = self.bus().video_memory();
        let ppu = self.bus().ppu();
        let control = ppu.bgcnt(index);
        let char_base = usize::from(control >> 2 & 3) * CHAR_BLOCK;
        let screen_base = usize::from(control >> 8 & 0x1F) * SCREEN_BLOCK;
        let depth = if control & (1 << 7) != 0 {
            Depth::Eight
        } else {
            Depth::Four
        };
        let (width, height) = MAP_SIZES[usize::from(control >> 14)];
        let columns = width / 8;
        let mut picture = Picture::new(width, height);
        let mut cells = Vec::with_capacity(columns * (height / 8));
        let colors: Vec<[u8; 4]> = (0..256).map(|at| rgba(read16(palette, at * 2))).collect();
        for row in 0..height / 8 {
            for column in 0..columns {
                let block = column / 32 + (row / 32) * (width / 256);
                let at = screen_base + block * SCREEN_BLOCK + ((row % 32) * 32 + column % 32) * 2;
                let entry = if at + 1 < BG_VRAM {
                    read16(vram, at)
                } else {
                    0
                };
                let tile = usize::from(entry & 0x3FF);
                let flip = Flip {
                    x: entry & (1 << 10) != 0,
                    y: entry & (1 << 11) != 0,
                };
                let bank = (entry >> 12) as u8;
                let data_at = char_base + tile * depth.tile_bytes();
                let pixels = vram
                    .get(data_at..data_at + depth.tile_bytes())
                    .filter(|_| data_at + depth.tile_bytes() <= BG_VRAM)
                    .map(|bytes| decode(bytes, depth));
                let row_colors = match depth {
                    Depth::Four => &colors[usize::from(bank) * 16..usize::from(bank) * 16 + 16],
                    _ => &colors[..],
                };
                match pixels {
                    Some(pixels) => {
                        picture.draw_tile(column * 8, row * 8, &pixels, row_colors, flip, false)
                    }
                    None => picture.draw_tile(column * 8, row * 8, &[0; 64], &[CLEAR], flip, false),
                }
                cells.push(MapCell {
                    address: VRAM_START + at as u32,
                    tile: tile as u16,
                    source: TileRef {
                        sheet: sheet_of(false, depth),
                        tile: data_at / depth.tile_bytes(),
                    },
                    flip,
                    palette: (depth == Depth::Four).then_some(bank),
                    bank: None,
                    priority: None,
                });
            }
        }
        let (hofs, vofs) = ppu.bg_offsets(index);
        let name = if enabled {
            format!("BG{index}")
        } else {
            format!("BG{index}, off")
        };
        TileMap {
            name,
            columns,
            cells,
            frames: vec![Frame {
                name: "Screen",
                rects: wrapped(
                    usize::from(hofs & 0x1FF),
                    usize::from(vofs & 0x1FF),
                    WIDTH,
                    HEIGHT,
                    (width, height),
                ),
            }],
            picture,
        }
    }

    fn bitmap(&self, name: &str, start: usize, indexed: bool) -> TileMap {
        let (vram, palette, _) = self.bus().video_memory();
        let mut picture = Picture::new(WIDTH, HEIGHT);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let color = if indexed {
                    let index = usize::from(vram[start + y * WIDTH + x]);
                    read16(palette, index * 2)
                } else {
                    read16(vram, (y * WIDTH + x) * 2)
                };
                picture.set(x, y, rgba(color));
            }
        }
        TileMap {
            name: name.to_string(),
            columns: 0,
            cells: Vec::new(),
            frames: Vec::new(),
            picture,
        }
    }

    pub fn sprites(&self, out: &mut Vec<Sprite>) {
        out.clear();
        let (vram, palette, oam) = self.bus().video_memory();
        let ppu = self.bus().ppu();
        let one_dimensional = ppu.dispcnt() & (1 << 6) != 0;
        let colors: Vec<[u8; 4]> = (0..256)
            .map(|at| rgba(read16(palette, 0x200 + at * 2)))
            .collect();
        for index in 0..SPRITES {
            let at = index * 8;
            let attr0 = read16(oam, at);
            let attr1 = read16(oam, at + 2);
            let attr2 = read16(oam, at + 4);
            let affine = attr0 & (1 << 8) != 0;
            let hidden = !affine && attr0 & (1 << 9) != 0;
            let shape = usize::from(attr0 >> 14).min(2);
            let (width, height) = SPRITE_SIZES[shape][usize::from(attr1 >> 14)];
            let wide = attr0 & (1 << 13) != 0;
            let depth = if wide { Depth::Eight } else { Depth::Four };
            let tile = usize::from(attr2 & 0x3FF);
            let bank = usize::from(attr2 >> 12);
            let flip = Flip {
                x: !affine && attr1 & (1 << 12) != 0,
                y: !affine && attr1 & (1 << 13) != 0,
            };
            let row_colors = if wide {
                &colors[..]
            } else {
                &colors[bank * 16..bank * 16 + 16]
            };
            let step = if wide { 2 } else { 1 };
            let stride = if one_dimensional {
                width / 8 * step
            } else {
                32
            };
            let mut picture = Picture::new(width, height);
            for tile_row in 0..height / 8 {
                for tile_column in 0..width / 8 {
                    let number = (tile + tile_row * stride + tile_column * step) & 0x3FF;
                    let data_at = BG_VRAM + number * 32;
                    let Some(bytes) = vram.get(data_at..data_at + depth.tile_bytes()) else {
                        continue;
                    };
                    let x = if flip.x {
                        width - 8 - tile_column * 8
                    } else {
                        tile_column * 8
                    };
                    let y = if flip.y {
                        height - 8 - tile_row * 8
                    } else {
                        tile_row * 8
                    };
                    picture.draw_tile(x, y, &decode(bytes, depth), row_colors, flip, true);
                }
            }
            let raw_x = i32::from(attr1 & 0x1FF);
            let raw_y = i32::from(attr0 & 0xFF);
            let x = if raw_x >= 256 { raw_x - 512 } else { raw_x };
            let y = if raw_y + height as i32 > 256 {
                raw_y - 256
            } else {
                raw_y
            };
            let mode = match attr0 >> 10 & 3 {
                0 => "Normal",
                1 => "Semi-transparent",
                2 => "Window",
                _ => "Prohibited",
            };
            let mut fields = vec![
                Field::new("Address", address(OAM_START + at as u32)),
                Field::new("Y position", format!("{raw_y} (screen {y})")),
                Field::new("X position", format!("{raw_x} (screen {x})")),
                Field::new("Size", format!("{width}x{height}")),
                Field::new("Tile index", format!("${tile:03X}")),
                Field::new("Colours", if wide { "256" } else { "16" }),
                Field::new("Priority", (attr2 >> 10 & 3).to_string()),
                Field::new("Mode", mode),
                Field::new("Affine", yes_no(affine)),
            ];
            if affine {
                fields.push(Field::new("Double size", yes_no(attr0 & (1 << 9) != 0)));
                fields.push(Field::new("Affine group", (attr1 >> 9 & 0x1F).to_string()));
            } else {
                fields.push(Field::new("Hidden", yes_no(hidden)));
                fields.push(Field::new("X flip", yes_no(flip.x)));
                fields.push(Field::new("Y flip", yes_no(flip.y)));
            }
            if !wide {
                fields.push(Field::new("Palette", format!("Sprites {bank}")));
            }
            fields.push(Field::new("Mosaic", yes_no(attr0 & (1 << 12) != 0)));
            out.push(Sprite {
                index,
                x,
                y,
                source: TileRef {
                    sheet: sheet_of(true, depth),
                    tile: tile * 32 / depth.tile_bytes(),
                },
                on_screen: !hidden
                    && x > -(width as i32)
                    && x < WIDTH as i32
                    && y > -(height as i32)
                    && y < HEIGHT as i32,
                bytes: oam[at..at + 6].to_vec(),
                fields,
                picture,
            });
        }
    }
}

#[cfg(all(test, all(feature = "debug", feature = "gba")))]
mod tests {
    use crate::debug::tiles::Depth;
    use crate::debug::video::{SheetColors, Sprite};
    use crate::gba::machine::Gba;
    use crate::test_support::matches_fixture;
    use citrine_gb_test_roms as test_roms;

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
        let path = test_roms::roms_dir()
            .join("tonc")
            .join(demo)
            .join(format!("{name}.gba"));
        let rom = test_roms::read(test, path)?;
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
}
