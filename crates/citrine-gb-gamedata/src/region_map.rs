use crate::game::{Game, Generation};
use crate::pic::decompress_lz;
use crate::places::{Region, Spot};
use crate::scan::{Pattern, banked, byte, same_bank, word};
use crate::sprite::{Palette, gen1_palette};

const TILE: usize = 8;
const GB_TILE_BYTES: usize = 16;
const SCREEN_WIDTH: usize = 20;
const SCREEN_HEIGHT: usize = 18;
const GEN1_TILES: usize = 16;
const GEN1_LOAD_WINDOW: usize = 64;
const GEN1_TOWN_MAP_PALETTE: u8 = 0x0C;
const GEN2_TILES: usize = 48;
const GEN2_PALETTES: usize = 6;
const GEN2_PALETTE_TILES: u8 = 0x60;
const GEN2_FIRST_ROW: usize = 2;
const GEN2_FRAME: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RegionMap {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[u8; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker<'a> {
    pub map: &'a RegionMap,
    pub name: &'a str,
    pub x: u16,
    pub y: u16,
}

impl RegionMap {
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        self.pixels[y * self.width + x]
    }

    pub(crate) fn blank(columns: usize, rows: usize) -> Self {
        Self {
            width: columns * TILE,
            height: rows * TILE,
            pixels: vec![[0, 0, 0, 0]; columns * rows * TILE * TILE],
        }
    }

    pub(crate) fn put_tile(
        &mut self,
        column: usize,
        row: usize,
        flip: (bool, bool),
        mut color: impl FnMut(usize, usize) -> Option<[u8; 4]>,
    ) {
        for y in 0..TILE {
            for x in 0..TILE {
                let from_x = if flip.0 { TILE - 1 - x } else { x };
                let from_y = if flip.1 { TILE - 1 - y } else { y };
                if let Some(rgba) = color(from_x, from_y) {
                    let at = (row * TILE + y) * self.width + column * TILE + x;
                    if let Some(pixel) = self.pixels.get_mut(at) {
                        *pixel = rgba;
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct RegionMaps {
    maps: Vec<(Region, RegionMap)>,
}

impl RegionMaps {
    pub fn load(game: Game, rom: &[u8]) -> Self {
        let maps = match game.generation() {
            Generation::One => gen1(game, rom)
                .map(|map| vec![(Region::Kanto, map)])
                .unwrap_or_default(),
            Generation::Two => gen2(rom).unwrap_or_default(),
            Generation::Three => Vec::new(),
        };
        Self { maps }
    }

    pub fn get(&self, region: Region) -> Option<&RegionMap> {
        self.maps
            .iter()
            .find(|(own, _)| *own == region)
            .map(|(_, map)| map)
    }

    pub fn marker<'a>(&'a self, game: Game, spot: Spot<'a>) -> Option<Marker<'a>> {
        let map = self.get(spot.region)?;
        let (x, y) = match game.generation() {
            Generation::One => (u16::from(spot.x) * 8 + 20, u16::from(spot.y) * 8 + 12),
            Generation::Two => (
                u16::from(spot.x).checked_sub(8 + 8 * GEN2_FRAME as u16)?,
                u16::from(spot.y).checked_sub(16 + 8 * GEN2_FIRST_ROW as u16)?,
            ),
            Generation::Three => return None,
        };
        (usize::from(x) < map.width && usize::from(y) < map.height).then_some(Marker {
            map,
            name: spot.name,
            x,
            y,
        })
    }
}

fn gb_tile_shade(tile: &[u8], x: usize, y: usize) -> u8 {
    let low = (tile[2 * y] >> (7 - x)) & 1;
    let high = (tile[2 * y + 1] >> (7 - x)) & 1;
    low | (high << 1)
}

fn opaque(palette: &Palette, shade: u8) -> [u8; 4] {
    let [r, g, b] = palette.rgb(shade);
    [r, g, b, 0xFF]
}

fn gen1(game: Game, rom: &[u8]) -> Option<RegionMap> {
    let code = Pattern::new("11 ?? ?? 1A A7 28 ?? 47 E6 0F 4F 78 CB 37 E6 0F C6 60")
        .find_all(rom)
        .next()?;
    let before = rom.get(code.checked_sub(GEN1_LOAD_WINDOW)?..code)?;
    let graphics = code - GEN1_LOAD_WINDOW
        + Pattern::new("21 ?? ?? 11 00 96 01 ?? ?? 3E ?? CD")
            .find_all(before)
            .last()?;
    let tiles_at = banked(
        rom,
        usize::from(byte(rom, graphics + 10)?),
        word(rom, graphics + 1)?,
    )?;
    let tiles = rom.get(tiles_at..tiles_at + GEN1_TILES * GB_TILE_BYTES)?;
    let mut at = same_bank(rom, code, 1)?;
    let mut ids = Vec::with_capacity(SCREEN_WIDTH * SCREEN_HEIGHT);
    while ids.len() < SCREEN_WIDTH * SCREEN_HEIGHT {
        let run = byte(rom, at)?;
        if run == 0 {
            break;
        }
        ids.extend(std::iter::repeat_n(
            usize::from(run >> 4),
            usize::from(run & 0x0F),
        ));
        at += 1;
    }
    ids.truncate(SCREEN_WIDTH * SCREEN_HEIGHT);
    let palette = gen1_palette(game, rom, GEN1_TOWN_MAP_PALETTE).unwrap_or(Palette::GRAY);
    let mut map = RegionMap::blank(SCREEN_WIDTH, SCREEN_HEIGHT);
    for (i, id) in ids.into_iter().enumerate() {
        let tile = &tiles[id * GB_TILE_BYTES..][..GB_TILE_BYTES];
        map.put_tile(
            i % SCREEN_WIDTH,
            i / SCREEN_WIDTH,
            (false, false),
            |x, y| Some(opaque(&palette, gb_tile_shade(tile, x, y))),
        );
    }
    Some(map)
}

fn gen2(rom: &[u8]) -> Option<Vec<(Region, RegionMap)>> {
    let load = Pattern::new("21 ?? ?? 11 00 90 01 30 ?? CD ?? ?? C9")
        .find_all(rom)
        .next()?;
    let tiles_at = banked(rom, usize::from(byte(rom, load + 8)?), word(rom, load + 1)?)?;
    let tiles = decompress_lz(rom, tiles_at, GEN2_TILES * GB_TILE_BYTES).ok()?;
    let fill = Pattern::new("11 ?? ?? 18 ?? 11 ?? ?? 21 ?? ?? 1A FE FF C8")
        .find_all(rom)
        .next()?;
    let johto = same_bank(rom, fill, 1)?;
    let kanto = same_bank(rom, fill, 6)?;
    let pal_code = Pattern::new("FE 60 30 ?? 21 ?? ?? CB 3F 38")
        .find_all(rom)
        .next()?;
    let pal_map = rom
        .get(same_bank(rom, pal_code, 5)?..)?
        .get(..usize::from(GEN2_PALETTE_TILES) / 2)?;
    let palettes = gen2_palettes(rom)?;
    let draw = |at: usize| -> Option<RegionMap> {
        let ids = rom.get(at..at + SCREEN_WIDTH * SCREEN_HEIGHT)?;
        let rows = SCREEN_HEIGHT - GEN2_FIRST_ROW - GEN2_FRAME;
        let columns = SCREEN_WIDTH - 2 * GEN2_FRAME;
        let mut map = RegionMap::blank(columns, rows);
        for (i, &id) in ids.iter().enumerate().skip(SCREEN_WIDTH * GEN2_FIRST_ROW) {
            let (column, row) = (i % SCREEN_WIDTH, i / SCREEN_WIDTH - GEN2_FIRST_ROW);
            if column < GEN2_FRAME || column >= GEN2_FRAME + columns || row >= rows {
                continue;
            }
            let tile = tiles
                .get(usize::from(id) * GB_TILE_BYTES..)?
                .get(..GB_TILE_BYTES)?;
            let palette = if id < GEN2_PALETTE_TILES {
                let pair = pal_map[usize::from(id) / 2];
                let nibble = if id % 2 == 0 { pair & 0x0F } else { pair >> 4 };
                usize::from(nibble & 7).min(GEN2_PALETTES - 1)
            } else {
                0
            };
            let palette = palettes[palette];
            map.put_tile(column - GEN2_FRAME, row, (false, false), |x, y| {
                Some(opaque(&palette, gb_tile_shade(tile, x, y)))
            });
        }
        Some(map)
    };
    Some(vec![
        (Region::Johto, draw(johto)?),
        (Region::Kanto, draw(kanto)?),
    ])
}

fn gen2_palettes(rom: &[u8]) -> Option<[Palette; GEN2_PALETTES]> {
    Pattern::new("21 ?? ?? 11 ?? ?? 01 30 00")
        .find_all(rom)
        .find_map(|at| {
            let table = same_bank(rom, at, 1)?;
            let palettes: Vec<Palette> = (0..GEN2_PALETTES)
                .map(|i| {
                    let mut colors = [0u16; 4];
                    for (c, color) in colors.iter_mut().enumerate() {
                        *color = word(rom, table + i * 8 + 2 * c).filter(|&c| c < 0x8000)?;
                    }
                    Some(Palette { colors })
                })
                .collect::<Option<_>>()?;
            let paper = palettes[0].colors[0];
            let plausible = paper != 0
                && palettes.iter().all(|p| p.colors[0] == paper)
                && palettes[0].colors[3] == 0
                && palettes.iter().any(|p| p.colors[1] != p.colors[2]);
            plausible.then(|| palettes.try_into().ok())?
        })
}
