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
