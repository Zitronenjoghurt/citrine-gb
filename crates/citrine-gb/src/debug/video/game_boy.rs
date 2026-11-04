use super::{
    ColorValue, Field, Frame, MapCell, Palette, SheetColors, Sprite, TileMap, TileRef, TileSheet,
    address, sheet_colors, yes_no,
};
use crate::debug::tiles::{Depth, Flip, Picture, decode, sheet, wrapped};
use crate::gb::GameBoy;
use crate::gb::ppu::types::color::RGBA;
use crate::gb::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};

const TILES_PER_BANK: usize = 384;
const SHEET_COLUMNS: usize = 16;
const MAP_SIZE: usize = 32;
const MAP_PIXELS: usize = MAP_SIZE * 8;
const SPRITES: usize = 40;

fn rgba(color: RGBA) -> [u8; 4] {
    [color.r(), color.g(), color.b(), 0xFF]
}

impl GameBoy {
    fn vram_banks(&self) -> usize {
        if self.model.is_cgb() { 2 } else { 1 }
    }

    pub fn palettes(&self, out: &mut Vec<Palette>) {
        out.clear();
        let ppu = &self.ppu;
        if ppu.cgb_mode() {
            for (name, sprites) in [("Background", false), ("Sprites", true)] {
                let ram = ppu.palette_ram(sprites);
                let values = ram
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .collect();
                let colors = (0..32)
                    .map(|at| rgba(ppu.palette_color(sprites, at / 4, at % 4)))
                    .collect();
                out.push(Palette {
                    name,
                    row: 4,
                    kind: ColorValue::Rgb555,
                    values,
                    colors,
                });
            }
            return;
        }
        let [bgp, obp0, obp1] = ppu.dmg_palettes();
        let shades = |register: u8| (0..4).map(move |at| u16::from((register >> (at * 2)) & 3));
        out.push(Palette {
            name: "Background",
            row: 4,
            kind: ColorValue::Shade,
            values: shades(bgp).collect(),
            colors: (0..4).map(|at| rgba(ppu.apply_bg_palette(0, at))).collect(),
        });
        out.push(Palette {
            name: "Sprites",
            row: 4,
            kind: ColorValue::Shade,
            values: shades(obp0).chain(shades(obp1)).collect(),
            colors: (0..8)
                .map(|at| rgba(ppu.apply_sprite_palette(at / 4, at % 4)))
                .collect(),
        });
    }

    pub fn tile_sheets(&self, colors: SheetColors, out: &mut Vec<TileSheet>) {
        out.clear();
        let mut palettes = Vec::new();
        self.palettes(&mut palettes);
        let colors = sheet_colors(&palettes, colors, Depth::Two);
        let banks = self.vram_banks();
        for bank in 0..banks {
            let Some(vram) = self.ppu.vram(bank) else {
                continue;
            };
            let name = if banks > 1 {
                format!("Bank {bank}")
            } else {
                "VRAM".to_string()
            };
            out.push(TileSheet {
                name,
                start: 0x8000,
                bank: bank as u16,
                depth: Depth::Two,
                columns: SHEET_COLUMNS,
                palette_group: 0,
                picture: sheet(
                    &vram[..TILES_PER_BANK * 16],
                    Depth::Two,
                    SHEET_COLUMNS,
                    &colors,
                ),
            });
        }
    }

    pub fn tile_maps(&self, out: &mut Vec<TileMap>) {
        out.clear();
        let ppu = &self.ppu;
        let lcdc = &ppu.lcdc;
        let cgb = ppu.cgb_mode();
        let (Some(tiles), attributes) = (ppu.vram(0), ppu.vram(1)) else {
            return;
        };
        let mut palettes = Vec::new();
        self.palettes(&mut palettes);
        for map in 0..2 {
            let base = 0x1800 + map * 0x400;
            let mut picture = Picture::new(MAP_PIXELS, MAP_PIXELS);
            let mut cells = Vec::with_capacity(MAP_SIZE * MAP_SIZE);
            for index in 0..MAP_SIZE * MAP_SIZE {
                let id = tiles[base + index];
                let attribute = attributes
                    .filter(|_| cgb)
                    .map_or(0, |bytes| bytes[base + index]);
                let tile = if lcdc.bg_window_tiles {
                    usize::from(id)
                } else {
                    (256 + i32::from(id as i8)) as usize
                };
                let bank = usize::from(attribute >> 3 & 1);
                let flip = Flip {
                    x: attribute & 0x20 != 0,
                    y: attribute & 0x40 != 0,
                };
                let palette = attribute & 7;
                let data = ppu.vram(bank).unwrap_or(tiles);
                let pixels = decode(&data[tile * 16..], Depth::Two);
                let colors = match palettes.first() {
                    Some(group) if cgb => group.row_colors(usize::from(palette), Depth::Two),
                    Some(group) => &group.colors[..],
                    None => &[],
                };
                let (x, y) = ((index % MAP_SIZE) * 8, (index / MAP_SIZE) * 8);
                picture.draw_tile(x, y, &pixels, colors, flip, false);
                cells.push(MapCell {
                    address: 0x8000 + (base + index) as u32,
                    tile: u16::from(id),
                    source: TileRef { sheet: bank, tile },
                    flip,
                    palette: cgb.then_some(palette),
                    bank: cgb.then_some(bank as u8),
                    priority: cgb.then_some(attribute & 0x80 != 0),
                });
            }
            let mut frames = Vec::new();
            if usize::from(lcdc.bg_tilemap) == map {
                frames.push(Frame {
                    name: "Screen",
                    rects: wrapped(
                        usize::from(ppu.scx),
                        usize::from(ppu.scy),
                        SCREEN_WIDTH,
                        SCREEN_HEIGHT,
                        (MAP_PIXELS, MAP_PIXELS),
                    ),
                });
            }
            if usize::from(lcdc.window_tilemap) == map
                && lcdc.window_enable
                && ppu.wx <= 166
                && usize::from(ppu.wy) < SCREEN_HEIGHT
            {
                let left = i32::from(ppu.wx) - 7;
                let skipped = (-left).max(0) as usize;
                frames.push(Frame {
                    name: "Window",
                    rects: wrapped(
                        skipped,
                        0,
                        SCREEN_WIDTH - left.max(0) as usize,
                        SCREEN_HEIGHT - usize::from(ppu.wy),
                        (MAP_PIXELS, MAP_PIXELS),
                    ),
                });
            }
            out.push(TileMap {
                name: format!("Map ${:04X}", 0x8000 + base),
                columns: MAP_SIZE,
                cells,
                frames,
                picture,
            });
        }
    }

    pub fn sprites(&self, out: &mut Vec<Sprite>) {
        out.clear();
        let ppu = &self.ppu;
        let cgb = ppu.cgb_mode();
        let tall = ppu.lcdc.obj_size;
        let height = if tall { 16 } else { 8 };
        let mut palettes = Vec::new();
        self.palettes(&mut palettes);
        let Some(group) = palettes.get(1) else {
            return;
        };
        for (index, bytes) in ppu
            .oam()
            .as_chunks::<4>()
            .0
            .iter()
            .take(SPRITES)
            .enumerate()
        {
            let [y, x, id, attribute] = *bytes;
            let bank = if cgb {
                usize::from(attribute >> 3 & 1)
            } else {
                0
            };
            let palette = if cgb {
                attribute & 7
            } else {
                attribute >> 4 & 1
            };
            let flip = Flip {
                x: attribute & 0x20 != 0,
                y: attribute & 0x40 != 0,
            };
            let first = if tall { id & 0xFE } else { id };
            let data = ppu.vram(bank).unwrap_or_default();
            let colors = group.row_colors(usize::from(palette), Depth::Two);
            let mut picture = Picture::new(8, height);
            for part in 0..height / 8 {
                let tile = usize::from(first) + part;
                let drawn_at = if flip.y { height / 8 - 1 - part } else { part };
                if let Some(bytes) = data.get(tile * 16..tile * 16 + 16) {
                    picture.draw_tile(
                        0,
                        drawn_at * 8,
                        &decode(bytes, Depth::Two),
                        colors,
                        flip,
                        true,
                    );
                }
            }
            let (screen_x, screen_y) = (i32::from(x) - 8, i32::from(y) - 16);
            let mut fields = vec![
                Field::new("Address", address(0xFE00 + index as u32 * 4)),
                Field::new("Y position", format!("{y} (screen {screen_y})")),
                Field::new("X position", format!("{x} (screen {screen_x})")),
                Field::new("Tile index", format!("${id:02X}")),
                Field::new("Behind background", yes_no(attribute & 0x80 != 0)),
                Field::new("Y flip", yes_no(flip.y)),
                Field::new("X flip", yes_no(flip.x)),
            ];
            if cgb {
                fields.push(Field::new("Bank", bank.to_string()));
                fields.push(Field::new("Palette", format!("Sprites {palette}")));
            } else {
                fields.push(Field::new("Palette", format!("Sprites {palette}")));
            }
            out.push(Sprite {
                index,
                x: screen_x,
                y: screen_y,
                source: TileRef {
                    sheet: bank,
                    tile: usize::from(first),
                },
                on_screen: screen_x > -8
                    && screen_x < SCREEN_WIDTH as i32
                    && screen_y > -(height as i32)
                    && screen_y < SCREEN_HEIGHT as i32,
                bytes: bytes.to_vec(),
                fields,
                picture,
            });
        }
    }
}
