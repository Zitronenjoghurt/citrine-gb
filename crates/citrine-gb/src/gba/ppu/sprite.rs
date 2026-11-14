use super::{OPAQUE, WIDTH, read16};
use std::cmp::Ordering;
use std::ops::Range;

const SPRITES: usize = 128;
const OBJ_VRAM: usize = 0x1_0000;
const OBJ_VRAM_MASK: usize = 0x7FFF;
const OBJ_PALETTE: usize = 0x200;
const OBJ_PALETTE_BYTES: usize = 0x200;
const FIRST_BITMAP_TILE: usize = 512;
const LINE_BUDGET: i32 = 1210;
const HBLANK_FREE_BUDGET: i32 = 954;
const NO_SPRITE: u8 = 4;
const AFFINE: u16 = 1 << 8;
const DISABLED_OR_DOUBLE: u16 = 1 << 9;
const COLORS_256: u16 = 1 << 13;
const HFLIP: u16 = 1 << 12;
const VFLIP: u16 = 1 << 13;
const MODE_SEMI: u16 = 1;
const MODE_WINDOW: u16 = 2;
const MOSAIC: u16 = 1 << 12;
const MODE_PROHIBITED: u16 = 3;
pub(super) const SIZES: [[(usize, usize); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

#[derive(Clone)]
pub struct Sprites {
    pub color: [u16; WIDTH],
    pub priority: [u8; WIDTH],
    pub semi: [bool; WIDTH],
    pub window: [bool; WIDTH],
    pub any_semi: bool,
    drawn: bool,
    keys: [u32; SPRITES],
    spans: [Span; SPRITES],
    lines: [u128; 256],
    oam_changed: bool,
}

impl Default for Sprites {
    fn default() -> Self {
        Self {
            color: [0; WIDTH],
            priority: [NO_SPRITE; WIDTH],
            semi: [false; WIDTH],
            window: [false; WIDTH],
            any_semi: false,
            drawn: false,
            keys: [UNSEEN; SPRITES],
            spans: [Span::default(); SPRITES],
            lines: [0; 256],
            oam_changed: true,
        }
    }
}

const SPAN_KEY: u32 = 0xC000_FFFF;
const UNSEEN: u32 = !SPAN_KEY;

#[derive(Clone, Copy, Default)]
struct Span {
    top: u8,
    rows: u8,
}

impl Span {
    fn key(entry: &[u8; 8]) -> u32 {
        u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]) & SPAN_KEY
    }

    fn of(key: u32) -> Self {
        let attr0 = key as u16;
        let affine = attr0 & AFFINE != 0;
        let shape = usize::from(attr0 >> 14);
        let rows = if (!affine && attr0 & DISABLED_OR_DOUBLE != 0) || shape == 3 {
            0
        } else {
            let height = SIZES[shape][(key >> 30) as usize].1;
            if affine && attr0 & DISABLED_OR_DOUBLE != 0 {
                2 * height
            } else {
                height
            }
        };
        Self {
            top: attr0 as u8,
            rows: rows as u8,
        }
    }

    fn lines(&self) -> impl Iterator<Item = usize> {
        let top = usize::from(self.top);
        (top..top + usize::from(self.rows)).map(|line| line & 0xFF)
    }
}

pub struct Layout {
    pub one_dimensional: bool,
    pub hblank_free: bool,
    pub bitmap: bool,
    pub mosaic: (usize, usize),
}

impl Sprites {
    pub fn drawn(&self) -> bool {
        self.drawn
    }

    pub fn clear(&mut self) {
        if !self.drawn {
            return;
        }
        self.color.fill(0);
        self.priority.fill(NO_SPRITE);
        self.semi.fill(false);
        self.window.fill(false);
        self.any_semi = false;
        self.drawn = false;
    }

    pub fn oam_written(&mut self) {
        self.oam_changed = true;
    }

    fn move_spans(&mut self, keys: &[u32; SPRITES]) {
        for (index, &key) in keys.iter().enumerate() {
            if self.keys[index] == key {
                continue;
            }
            let bit = 1u128 << index;
            for line in self.spans[index].lines() {
                self.lines[line] &= !bit;
            }
            self.spans[index] = Span::of(key);
            for line in self.spans[index].lines() {
                self.lines[line] |= bit;
            }
        }
        self.keys = *keys;
    }

    pub fn draw(&mut self, y: usize, layout: &Layout, vram: &[u8], palette: &[u8], oam: &[u8]) {
        self.clear();
        let mut budget = if layout.hblank_free {
            HBLANK_FREE_BUDGET
        } else {
            LINE_BUDGET
        };
        let entries: &[[u8; 8]; SPRITES] = oam.as_chunks::<8>().0[..SPRITES].try_into().unwrap();
        if std::mem::take(&mut self.oam_changed) {
            let keys: [u32; SPRITES] = std::array::from_fn(|index| Span::key(&entries[index]));
            if keys != self.keys {
                self.move_spans(&keys);
            }
        }
        let mut visible = self.lines[y & 0xFF];
        while visible != 0 {
            let index = visible.trailing_zeros() as usize;
            visible &= visible - 1;
            let entry = &entries[index];
            let attr0 = u16::from_le_bytes([entry[0], entry[1]]);
            let affine = attr0 & AFFINE != 0;
            if !affine && attr0 & DISABLED_OR_DOUBLE != 0 {
                continue;
            }
            let shape = usize::from(attr0 >> 14);
            if shape == 3 {
                continue;
            }
            let attr1 = u16::from_le_bytes([entry[2], entry[3]]);
            let (width, height) = SIZES[shape][usize::from(attr1 >> 14)];
            let double = if affine && attr0 & DISABLED_OR_DOUBLE != 0 {
                2
            } else {
                1
            };
            let top = usize::from(attr0 & 0xFF);
            let row = (y + 256 - top) & 0xFF;
            if row >= height * double {
                continue;
            }
            if budget <= 0 {
                break;
            }
            budget -= if affine {
                10 + 2 * (width * double) as i32
            } else {
                width as i32
            };
            let mode = attr0 >> 10 & 0b11;
            if mode == MODE_PROHIBITED {
                continue;
            }
            let attr2 = u16::from_le_bytes([entry[4], entry[5]]);
            let tile = usize::from(attr2 & 0x3FF);
            if layout.bitmap && tile < FIRST_BITMAP_TILE {
                continue;
            }
            let mosaic = attr0 & MOSAIC != 0;
            let row = if mosaic {
                let grid = y - y % layout.mosaic.1;
                let row = (grid + 256 - top) & 0xFF;
                if row >= height * double { 0 } else { row }
            } else {
                row
            };
            let matrix = if affine {
                let group = usize::from(attr1 >> 9 & 0x1F) * 32;
                Some([6, 14, 22, 30].map(|at| read16(oam, group + at) as i16))
            } else {
                None
            };
            let object = Object {
                x: usize::from(attr1 & 0x1FF),
                width,
                height,
                double,
                row,
                tile,
                wide: attr0 & COLORS_256 != 0,
                hflip: !affine && attr1 & HFLIP != 0,
                vflip: !affine && attr1 & VFLIP != 0,
                priority: (attr2 >> 10 & 0b11) as u8,
                bank: usize::from(attr2 >> 12) * 16,
                mode,
                mosaic: if mosaic { layout.mosaic.0 } else { 1 },
                matrix,
            };
            self.draw_object(&object, layout, vram, palette);
        }
    }

    fn draw_object(&mut self, object: &Object, layout: &Layout, vram: &[u8], palette: &[u8]) {
        let step = if object.wide { 2 } else { 1 };
        let stride = if layout.one_dimensional {
            object.width / 8 * step
        } else {
            32
        };
        let first = if object.wide && !layout.one_dimensional {
            object.tile & !1
        } else {
            object.tile
        };
        match object.matrix {
            None if object.mosaic == 1 => self.draw_plain(object, first, stride, vram, palette),
            Some(matrix) if object.mosaic == 1 => {
                self.draw_affine(object, matrix, first, stride, vram, palette)
            }
            _ => self.draw_any(object, first, stride, vram, palette),
        }
    }

    fn draw_plain(
        &mut self,
        object: &Object,
        first: usize,
        stride: usize,
        vram: &[u8],
        palette: &[u8],
    ) {
        let (tile_bytes, step) = if object.wide { (8, 2) } else { (4, 1) };
        let source_y = if object.vflip {
            object.height - 1 - object.row
        } else {
            object.row
        };
        let row_tile = first + (source_y >> 3) * stride;
        for source_tile in 0..object.width / 8 {
            let tile = row_tile + source_tile * step;
            let at = OBJ_VRAM + ((tile * 32 + (source_y & 7) * tile_bytes) & OBJ_VRAM_MASK);
            let bits = if object.wide {
                u64::from_le_bytes(vram[at..at + 8].try_into().unwrap())
            } else {
                u64::from(u32::from_le_bytes(vram[at..at + 4].try_into().unwrap()))
            };
            if bits == 0 {
                continue;
            }
            for pixel in 0..8 {
                let index = if object.wide {
                    usize::from((bits >> (pixel * 8)) as u8)
                } else {
                    (bits >> (pixel * 4) & 0xF) as usize
                };
                if index == 0 {
                    continue;
                }
                let source_x = source_tile * 8 + pixel;
                let column = if object.hflip {
                    object.width - 1 - source_x
                } else {
                    source_x
                };
                self.put(object, (object.x + column) & 0x1FF, index, palette);
            }
        }
    }

    #[inline(always)]
    fn put(&mut self, object: &Object, x: usize, index: usize, palette: &[u8]) {
        let window = object.mode == MODE_WINDOW;
        if x >= WIDTH || (!window && self.priority[x] <= object.priority) {
            return;
        }
        self.drawn = true;
        if window {
            self.window[x] = true;
            return;
        }
        let entry = if object.wide {
            index
        } else {
            object.bank + index
        };
        self.color[x] = read16(palette, OBJ_PALETTE + entry * 2) | OPAQUE;
        self.priority[x] = object.priority;
        self.semi[x] = object.mode == MODE_SEMI;
        self.any_semi |= object.mode == MODE_SEMI;
    }

    fn draw_any(
        &mut self,
        object: &Object,
        first: usize,
        stride: usize,
        vram: &[u8],
        palette: &[u8],
    ) {
        let box_width = object.width * object.double;
        let box_height = object.height * object.double;
        let window = object.mode == MODE_WINDOW;
        for column in 0..box_width {
            let x = (object.x + column) & 0x1FF;
            if x >= WIDTH || (!window && self.priority[x] <= object.priority) {
                continue;
            }
            let column = if object.mosaic > 1 {
                column.saturating_sub(x % object.mosaic)
            } else {
                column
            };
            let (source_x, source_y) = match object.matrix {
                Some([pa, pb, pc, pd]) => {
                    let dx = column as i32 - (box_width / 2) as i32;
                    let dy = object.row as i32 - (box_height / 2) as i32;
                    let tx = ((i32::from(pa) * dx + i32::from(pb) * dy) >> 8)
                        + (object.width / 2) as i32;
                    let ty = ((i32::from(pc) * dx + i32::from(pd) * dy) >> 8)
                        + (object.height / 2) as i32;
                    if tx < 0 || ty < 0 || tx >= object.width as i32 || ty >= object.height as i32 {
                        continue;
                    }
                    (tx as usize, ty as usize)
                }
                None => (
                    if object.hflip {
                        object.width - 1 - column
                    } else {
                        column
                    },
                    if object.vflip {
                        object.height - 1 - object.row
                    } else {
                        object.row
                    },
                ),
            };
            let index = source_index(object, first, stride, source_x, source_y, vram);
            if index != 0 {
                self.put(object, x, index, palette);
            }
        }
    }

    fn draw_affine(
        &mut self,
        object: &Object,
        [pa, pb, pc, pd]: [i16; 4],
        first: usize,
        stride: usize,
        vram: &[u8],
        palette: &[u8],
    ) {
        let box_width = object.width * object.double;
        let half_box = (box_width / 2) as i32;
        let dy = object.row as i32 - (object.height * object.double / 2) as i32;
        let (half_width, half_height) = ((object.width / 2) as i32, (object.height / 2) as i32);
        let (pa, pc) = (i32::from(pa), i32::from(pc));
        let x_sum = i32::from(pb) * dy - pa * half_box;
        let y_sum = i32::from(pd) * dy - pc * half_box;
        let across = inside(pa, x_sum, half_width, box_width);
        let down = inside(pc, y_sum, half_height, box_width);
        let columns = across.start.max(down.start)..across.end.min(down.end);
        if columns.is_empty() {
            return;
        }
        let start = columns.start as i32;
        let walk = AffineWalk {
            columns,
            source: [
                pa * start + x_sum + (half_width << 8),
                pc * start + y_sum + (half_height << 8),
            ],
            step: [pa, pc],
            first,
            stride,
        };
        let tiles = vram[OBJ_VRAM..][..OBJ_VRAM_MASK + 1].try_into().unwrap();
        let colors = palette[OBJ_PALETTE..][..OBJ_PALETTE_BYTES]
            .try_into()
            .unwrap();
        if object.wide {
            self.affine_columns::<true>(object, walk, tiles, colors);
        } else {
            self.affine_columns::<false>(object, walk, tiles, colors);
        }
    }

    #[inline(always)]
    fn affine_columns<const WIDE: bool>(
        &mut self,
        object: &Object,
        walk: AffineWalk,
        tiles: &[u8; OBJ_VRAM_MASK + 1],
        colors: &[u8; OBJ_PALETTE_BYTES],
    ) {
        let window = object.mode == MODE_WINDOW;
        let semi = object.mode == MODE_SEMI;
        let bank = if WIDE { 0 } else { object.bank };
        let [mut source_x, mut source_y] = walk.source;
        let [step_x, step_y] = walk.step;
        for column in walk.columns {
            let x = (object.x + column) & 0x1FF;
            let (tx, ty) = ((source_x >> 8) as usize, (source_y >> 8) as usize);
            source_x += step_x;
            source_y += step_y;
            if x >= WIDTH || (!window && self.priority[x] <= object.priority) {
                continue;
            }
            let row = walk.first + (ty >> 3) * walk.stride;
            let index = if WIDE {
                let at = (row + (tx >> 3) * 2) * 32 + (ty & 7) * 8 + (tx & 7);
                usize::from(tiles[at & OBJ_VRAM_MASK])
            } else {
                let at = (row + (tx >> 3)) * 32 + (ty & 7) * 4 + ((tx & 7) >> 1);
                usize::from((tiles[at & OBJ_VRAM_MASK] >> ((tx & 1) * 4)) & 0xF)
            };
            if index == 0 {
                continue;
            }
            self.drawn = true;
            if window {
                self.window[x] = true;
                continue;
            }
            let entry = ((bank + index) * 2) & (OBJ_PALETTE_BYTES - 2);
            self.color[x] = u16::from_le_bytes([colors[entry], colors[entry + 1]]) | OPAQUE;
            self.priority[x] = object.priority;
            self.semi[x] = semi;
            self.any_semi |= semi;
        }
    }
}

struct AffineWalk {
    columns: Range<usize>,
    source: [i32; 2],
    step: [i32; 2],
    first: usize,
    stride: usize,
}

#[inline(always)]
fn source_index(
    object: &Object,
    first: usize,
    stride: usize,
    source_x: usize,
    source_y: usize,
    vram: &[u8],
) -> usize {
    let (tile_bytes, step) = if object.wide { (8, 2) } else { (4, 1) };
    let tile = first + (source_y >> 3) * stride + (source_x >> 3) * step;
    let at = OBJ_VRAM
        + ((tile * 32 + (source_y & 7) * tile_bytes) & OBJ_VRAM_MASK)
        + if object.wide {
            source_x & 7
        } else {
            (source_x & 7) >> 1
        };
    let byte = usize::from(vram[at]);
    if object.wide {
        byte
    } else if source_x & 1 == 0 {
        byte & 0xF
    } else {
        byte >> 4
    }
}

fn inside(step: i32, at_zero: i32, half: i32, columns: usize) -> Range<usize> {
    let low = i64::from(-half * 256) - i64::from(at_zero);
    let high = i64::from(half * 256) - i64::from(at_zero);
    let step = i64::from(step);
    let (start, end) = match step.cmp(&0) {
        Ordering::Greater => (-((-low).div_euclid(step)), -((-high).div_euclid(step))),
        Ordering::Less => ((-high).div_euclid(-step) + 1, (-low).div_euclid(-step) + 1),
        Ordering::Equal if low <= 0 && 0 < high => (0, columns as i64),
        Ordering::Equal => (0, 0),
    };
    let clamp = |column: i64| column.clamp(0, columns as i64) as usize;
    clamp(start)..clamp(end)
}

struct Object {
    x: usize,
    width: usize,
    height: usize,
    double: usize,
    row: usize,
    tile: usize,
    wide: bool,
    hflip: bool,
    vflip: bool,
    priority: u8,
    bank: usize,
    mode: u16,
    mosaic: usize,
    matrix: Option<[i16; 4]>,
}

#[cfg(test)]
mod tests {
    use crate::gba::bus::Bus;
    use crate::gba::ppu::rgba;
    use crate::gba::ppu::tests::{bus, draw, pixel};

    const DISPCNT: u32 = 0x0400_0000;
    const OBJ_PALETTE: u32 = 0x0500_0200;
    const OBJ_VRAM: u32 = 0x0601_0000;
    const OAM: u32 = 0x0700_0000;
    const RED: u16 = 0x001F;
    const GREEN: u16 = 0x03E0;
    const BLUE: u16 = 0x7C00;
    const OBJ: u16 = 1 << 12;
    const ONE_D: u16 = 1 << 6;
    const HBLANK_FREE: u16 = 1 << 5;
    const HIDDEN: u16 = 1 << 9;
    const WIDE: u16 = 1 << 13;

    fn sprite(bus: &mut Bus, index: u32, attr0: u16, attr1: u16, attr2: u16) {
        bus.write16(OAM + index * 8, attr0);
        bus.write16(OAM + index * 8 + 2, attr1);
        bus.write16(OAM + index * 8 + 4, attr2);
    }

    fn hide_all(bus: &mut Bus) {
        for index in 0..128 {
            bus.write16(OAM + index * 8, HIDDEN);
        }
    }

    fn solid_tile(bus: &mut Bus, tile: u32, index: u8) {
        for half in 0..16 {
            bus.write16(OBJ_VRAM + tile * 32 + half * 2, u16::from(index) * 0x1111);
        }
    }

    fn color(bus: &mut Bus, index: u32, value: u16) {
        bus.write16(OBJ_PALETTE + index * 2, value);
    }

    fn ready() -> Bus {
        let mut bus = bus();
        hide_all(&mut bus);
        color(&mut bus, 1, RED);
        bus
    }

    #[test]
    fn a_4bpp_sprite_reads_its_bank_of_the_obj_palette() {
        let mut bus = ready();
        solid_tile(&mut bus, 1, 5);
        color(&mut bus, 3 * 16 + 5, GREEN);
        sprite(&mut bus, 0, 20, 16, 1 | 3 << 12);
        bus.write16(DISPCNT, OBJ);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 16, 20), rgba(GREEN));
        assert_eq!(pixel(&bus, 23, 27), rgba(GREEN));
        for (x, y) in [(24, 20), (15, 20), (16, 28), (16, 19)] {
            assert_eq!(pixel(&bus, x, y), rgba(0), "{x},{y}");
        }
    }

    #[test]
    fn every_shape_and_size_covers_its_area() {
        let mut bus = ready();
        for tile in 0..1024 {
            solid_tile(&mut bus, tile, 1);
        }
        let sizes = [
            [(8, 8), (16, 16), (32, 32), (64, 64)],
            [(16, 8), (32, 8), (32, 16), (64, 32)],
            [(8, 16), (8, 32), (16, 32), (32, 64)],
        ];
        bus.write16(DISPCNT, OBJ | ONE_D);
        for (shape, row) in sizes.iter().enumerate() {
            for (size, &(width, height)) in row.iter().enumerate() {
                sprite(&mut bus, 0, (shape as u16) << 14, (size as u16) << 14, 0);
                draw(&mut bus);
                let case = format!("shape {shape} size {size}");
                assert_eq!(pixel(&bus, width - 1, height - 1), rgba(RED), "{case}");
                assert_eq!(pixel(&bus, width, 0), rgba(0), "{case}");
                assert_eq!(pixel(&bus, 0, height), rgba(0), "{case}");
            }
        }
    }

    #[test]
    fn the_second_tile_row_follows_the_mapping_mode() {
        let mut bus = ready();
        solid_tile(&mut bus, 1, 1);
        solid_tile(&mut bus, 32, 2);
        color(&mut bus, 2, GREEN);
        sprite(&mut bus, 0, 2 << 14, 0, 0);
        bus.write16(DISPCNT, OBJ | ONE_D);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 8), rgba(RED));
        bus.write16(DISPCNT, OBJ);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 8), rgba(GREEN));
    }

    #[test]
    fn an_8bpp_sprite_uses_two_tile_numbers_per_tile() {
        let mut bus = ready();
        for half in 0..32 {
            bus.write16(OBJ_VRAM + 2 * 32 + half * 2, 0x4D4D);
        }
        color(&mut bus, 0x4D, BLUE);
        sprite(&mut bus, 0, WIDE, 0, 2);
        bus.write16(DISPCNT, OBJ | ONE_D);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(BLUE));
        assert_eq!(pixel(&bus, 7, 7), rgba(BLUE));
    }

    #[test]
    fn an_8bpp_sprite_in_2d_mapping_ignores_the_low_tile_bit() {
        let mut bus = ready();
        for half in 0..32 {
            bus.write16(OBJ_VRAM + 2 * 32 + half * 2, 0x0101);
        }
        sprite(&mut bus, 0, WIDE, 0, 3);
        bus.write16(DISPCNT, OBJ);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    }

    #[test]
    fn flips_mirror_the_whole_sprite() {
        let mut bus = ready();
        bus.write16(OBJ_VRAM, 0x0001);
        let cases = [
            (0, (0, 0)),
            (1 << 12, (15, 0)),
            (1 << 13, (0, 7)),
            (3 << 12, (15, 7)),
        ];
        bus.write16(DISPCNT, OBJ | ONE_D);
        for (flips, (x, y)) in cases {
            sprite(&mut bus, 0, 1 << 14, flips, 0);
            draw(&mut bus);
            assert_eq!(pixel(&bus, x, y), rgba(RED), "flips {flips:#x}");
            assert_eq!(pixel(&bus, 15 - x, 7 - y), rgba(0), "flips {flips:#x}");
        }
    }

    #[test]
    fn sprites_wrap_past_the_left_and_top_edges() {
        let mut bus = ready();
        solid_tile(&mut bus, 0, 1);
        sprite(&mut bus, 0, 252, 511, 0);
        bus.write16(DISPCNT, OBJ);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(RED));
        assert_eq!(pixel(&bus, 6, 3), rgba(RED));
        assert_eq!(pixel(&bus, 7, 0), rgba(0));
        assert_eq!(pixel(&bus, 0, 4), rgba(0));
    }

    #[test]
    fn hidden_window_and_switched_off_sprites_draw_nothing() {
        let mut bus = ready();
        solid_tile(&mut bus, 0, 1);
        for (attr0, dispcnt) in [(HIDDEN, OBJ), (2 << 10, OBJ), (3 << 10, OBJ), (0, 0)] {
            sprite(&mut bus, 0, attr0, 0, 0);
            bus.write16(DISPCNT, dispcnt);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 0), rgba(0), "attr0 {attr0:#x}");
        }
    }

    #[test]
    fn the_lower_oam_entry_wins_unless_the_other_has_higher_priority() {
        let mut bus = ready();
        solid_tile(&mut bus, 0, 1);
        solid_tile(&mut bus, 1, 2);
        color(&mut bus, 2, GREEN);
        sprite(&mut bus, 0, 0, 0, 1 << 10);
        sprite(&mut bus, 1, 0, 0, 1 | 1 << 10);
        bus.write16(DISPCNT, OBJ);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(RED));
        sprite(&mut bus, 1, 0, 0, 1);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(GREEN));
    }

    #[test]
    fn a_transparent_pixel_of_a_front_sprite_shows_the_one_behind() {
        let mut bus = ready();
        solid_tile(&mut bus, 1, 1);
        sprite(&mut bus, 0, 0, 0, 0);
        sprite(&mut bus, 1, 0, 0, 1 | 3 << 10);
        bus.write16(DISPCNT, OBJ);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    }

    fn line_budget_case(dispcnt: u16, wide_sprites: u32) -> [u8; 4] {
        let mut bus = ready();
        solid_tile(&mut bus, 0, 1);
        for index in 0..wide_sprites {
            sprite(&mut bus, index, 0, 300 | 3 << 14, 0);
        }
        sprite(&mut bus, wide_sprites, 0, 0, 0);
        bus.write16(DISPCNT, OBJ | dispcnt);
        draw(&mut bus);
        pixel(&bus, 0, 0)
    }

    #[test]
    fn a_line_draws_sprites_until_1210_cycles_are_spent() {
        assert_eq!(line_budget_case(0, 18), rgba(RED));
        assert_eq!(line_budget_case(0, 19), rgba(0));
    }

    #[test]
    fn hblank_free_leaves_954_cycles_for_sprites() {
        assert_eq!(line_budget_case(HBLANK_FREE, 14), rgba(RED));
        assert_eq!(line_budget_case(HBLANK_FREE, 15), rgba(0));
    }

    #[test]
    fn sprites_on_other_lines_cost_nothing() {
        let mut bus = ready();
        solid_tile(&mut bus, 0, 1);
        for index in 0..40 {
            sprite(&mut bus, index, 100, 300 | 3 << 14, 0);
        }
        sprite(&mut bus, 40, 0, 0, 0);
        bus.write16(DISPCNT, OBJ);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    }

    #[test]
    fn bitmap_modes_show_only_tiles_from_512() {
        let mut bus = ready();
        solid_tile(&mut bus, 0, 1);
        solid_tile(&mut bus, 512, 1);
        sprite(&mut bus, 0, 0, 0, 0);
        sprite(&mut bus, 1, 0, 16, 512);
        bus.write16(DISPCNT, 3 | OBJ);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(0));
        assert_eq!(pixel(&bus, 16, 0), rgba(RED));
    }

    const AFFINE: u16 = 1 << 8;
    const DOUBLE: u16 = 1 << 9;

    fn matrix(bus: &mut Bus, group: u32, values: [i16; 4]) {
        for (i, value) in values.into_iter().enumerate() {
            bus.write16(OAM + group * 32 + 6 + i as u32 * 8, value as u16);
        }
    }

    fn marked_tile(bus: &mut Bus, tile: u32) {
        for row in 0..8 {
            bus.write16(OBJ_VRAM + tile * 32 + row * 4, 0x2221);
            bus.write16(OBJ_VRAM + tile * 32 + row * 4 + 2, 0x2222);
        }
    }

    fn affine_ready() -> Bus {
        let mut bus = ready();
        color(&mut bus, 2, GREEN);
        marked_tile(&mut bus, 1);
        bus.write16(DISPCNT, OBJ);
        bus
    }

    #[test]
    fn an_affine_sprite_with_the_identity_matrix_draws_like_a_normal_one() {
        let mut bus = affine_ready();
        matrix(&mut bus, 0, [0x100, 0, 0, 0x100]);
        sprite(&mut bus, 0, 10 | AFFINE, 20, 1);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 20, 10), rgba(RED));
        assert_eq!(pixel(&bus, 21, 10), rgba(GREEN));
        assert_eq!(pixel(&bus, 27, 17), rgba(GREEN));
        assert_eq!(pixel(&bus, 28, 10), rgba(0));
        assert_eq!(pixel(&bus, 20, 18), rgba(0));
    }

    #[test]
    fn the_matrix_comes_from_the_parameter_group_not_the_flip_bits() {
        let mut bus = affine_ready();
        matrix(&mut bus, 0, [0x100, 0, 0, 0x100]);
        matrix(&mut bus, 24, [-0x100, 0, 0, 0x100]);
        sprite(&mut bus, 0, AFFINE, 24 << 9, 1);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(0));
        assert_eq!(pixel(&bus, 1, 0), rgba(GREEN));
    }

    #[test]
    fn double_size_centres_the_sprite_in_twice_its_area() {
        let mut bus = affine_ready();
        matrix(&mut bus, 0, [0x100, 0, 0, 0x100]);
        sprite(&mut bus, 0, AFFINE | DOUBLE, 0, 1);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 3, 4), rgba(0));
        assert_eq!(pixel(&bus, 4, 4), rgba(RED));
        assert_eq!(pixel(&bus, 11, 11), rgba(GREEN));
        assert_eq!(pixel(&bus, 12, 11), rgba(0));
        matrix(&mut bus, 0, [0x80, 0, 0, 0x80]);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(RED));
        assert_eq!(pixel(&bus, 1, 15), rgba(RED));
        assert_eq!(pixel(&bus, 2, 0), rgba(GREEN));
        assert_eq!(pixel(&bus, 15, 15), rgba(GREEN));
    }

    #[test]
    fn without_double_size_a_scaled_sprite_is_clipped_to_its_size() {
        let mut bus = affine_ready();
        matrix(&mut bus, 0, [0x80, 0, 0, 0x80]);
        sprite(&mut bus, 0, AFFINE, 0, 1);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(GREEN));
        assert_eq!(pixel(&bus, 7, 7), rgba(GREEN));
        assert_eq!(pixel(&bus, 8, 0), rgba(0));
    }

    #[test]
    fn rotation_maps_rows_to_columns() {
        let mut bus = affine_ready();
        matrix(&mut bus, 0, [0, 0x100, -0x100, 0]);
        sprite(&mut bus, 0, AFFINE, 0, 1);
        draw(&mut bus);
        for x in 1..8 {
            assert_eq!(pixel(&bus, x, 0), rgba(RED), "{x}");
            assert_eq!(pixel(&bus, x, 7), rgba(GREEN), "{x}");
        }
        assert_eq!(pixel(&bus, 0, 3), rgba(0));
    }

    struct Xorshift(u32);

    impl Xorshift {
        fn below(&mut self, n: u32) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0 % n
        }
    }

    #[test]
    fn plain_sprites_draw_like_walking_each_column() {
        let mut rng = Xorshift(0x9E37_79B9);
        let palette: Vec<u8> = (0..1024).map(|_| rng.below(256) as u8).collect();
        let mut vram: Vec<u8> = (0..0x1_8000)
            .map(|_| match rng.below(4) {
                0 => rng.below(256) as u8,
                _ => 0,
            })
            .collect();
        for _ in 0..3000 {
            let (width, height) = super::SIZES[rng.below(3) as usize][rng.below(4) as usize];
            let wide = rng.below(2) == 1;
            let one_dimensional = rng.below(2) == 1;
            let step = if wide { 2 } else { 1 };
            let object = super::Object {
                x: rng.below(512) as usize,
                width,
                height,
                double: 1,
                row: rng.below(height as u32) as usize,
                tile: rng.below(1024) as usize,
                wide,
                hflip: rng.below(2) == 1,
                vflip: rng.below(2) == 1,
                priority: rng.below(4) as u8,
                bank: rng.below(16) as usize * 16,
                mode: rng.below(3) as u16,
                mosaic: 1,
                matrix: None,
            };
            let stride = if one_dimensional {
                width / 8 * step
            } else {
                32
            };
            let first = if wide && !one_dimensional {
                object.tile & !1
            } else {
                object.tile
            };
            let at = 0x1_0000 + rng.below(0x8000) as usize;
            vram[at] = rng.below(256) as u8;
            let mut plain = super::Sprites::default();
            for priority in plain.priority.iter_mut().take(super::WIDTH) {
                *priority = rng.below(5) as u8;
            }
            let mut any = plain.clone();
            plain.draw_plain(&object, first, stride, &vram, &palette);
            any.draw_any(&object, first, stride, &vram, &palette);
            assert_eq!(plain.color, any.color);
            assert_eq!(plain.priority, any.priority);
            assert_eq!(plain.semi, any.semi);
            assert_eq!(plain.window, any.window);
            assert_eq!(plain.any_semi, any.any_semi);
            assert_eq!(plain.drawn, any.drawn);
        }
    }

    #[test]
    fn affine_sprites_draw_like_walking_each_column() {
        let mut rng = Xorshift(0x2545_F491);
        let palette: Vec<u8> = (0..1024).map(|_| rng.below(256) as u8).collect();
        let vram: Vec<u8> = (0..0x1_8000)
            .map(|_| match rng.below(3) {
                0 => 0,
                _ => rng.below(256) as u8,
            })
            .collect();
        let value = |rng: &mut Xorshift| match rng.below(6) {
            0 => 0,
            1 => 256,
            2 => -256,
            3 => [i16::MIN, i16::MAX, 1, -1][rng.below(4) as usize],
            4 => rng.below(1200) as i16 - 600,
            _ => rng.below(65536) as u16 as i16,
        };
        for _ in 0..20_000 {
            let (width, height) = super::SIZES[rng.below(3) as usize][rng.below(4) as usize];
            let double = 1 + rng.below(2) as usize;
            let wide = rng.below(2) == 1;
            let one_dimensional = rng.below(2) == 1;
            let step = if wide { 2 } else { 1 };
            let matrix: [i16; 4] = std::array::from_fn(|_| value(&mut rng));
            let object = super::Object {
                x: rng.below(512) as usize,
                width,
                height,
                double,
                row: rng.below((height * double) as u32) as usize,
                tile: rng.below(1024) as usize,
                wide,
                hflip: false,
                vflip: false,
                priority: rng.below(4) as u8,
                bank: rng.below(16) as usize * 16,
                mode: rng.below(3) as u16,
                mosaic: 1,
                matrix: Some(matrix),
            };
            let stride = if one_dimensional {
                width / 8 * step
            } else {
                32
            };
            let first = if wide && !one_dimensional {
                object.tile & !1
            } else {
                object.tile
            };
            let mut affine = super::Sprites::default();
            for priority in affine.priority.iter_mut().take(super::WIDTH) {
                *priority = rng.below(5) as u8;
            }
            let mut any = affine.clone();
            affine.draw_affine(&object, matrix, first, stride, &vram, &palette);
            any.draw_any(&object, first, stride, &vram, &palette);
            assert_eq!(affine.color, any.color, "{matrix:?}");
            assert_eq!(affine.priority, any.priority);
            assert_eq!(affine.semi, any.semi);
            assert_eq!(affine.window, any.window);
            assert_eq!(affine.any_semi, any.any_semi);
            assert_eq!(affine.drawn, any.drawn);
        }
    }

    #[test]
    fn a_moved_sprite_draws_at_its_new_rows() {
        let mut bus = ready();
        solid_tile(&mut bus, 1, 1);
        sprite(&mut bus, 0, 20, 16, 1);
        bus.write16(DISPCNT, OBJ);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 16, 20), rgba(RED));
        solid_tile(&mut bus, 33, 1);
        sprite(&mut bus, 0, 40 | 2 << 14, 16, 1);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 16, 20), rgba(0));
        assert_eq!(pixel(&bus, 16, 40), rgba(RED));
        assert_eq!(pixel(&bus, 23, 55), rgba(RED));
        assert_eq!(pixel(&bus, 24, 40), rgba(0));
        assert_eq!(pixel(&bus, 16, 56), rgba(0));
    }

    #[test]
    fn a_bus_state_copied_in_draws_the_sprites_of_its_oam() {
        let mut bus = ready();
        solid_tile(&mut bus, 1, 1);
        sprite(&mut bus, 0, 20, 16, 1);
        bus.write16(DISPCNT, OBJ);
        let mut moved = bus.clone();
        sprite(&mut moved, 0, 40, 16, 1);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 16, 20), rgba(RED));
        bus.copy_state_from(&moved);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 16, 20), rgba(0));
        assert_eq!(pixel(&bus, 16, 40), rgba(RED));
    }

    fn decode_skips(entry: &[u8; 8], y: usize) -> bool {
        let attr0 = u16::from_le_bytes([entry[0], entry[1]]);
        let attr1 = u16::from_le_bytes([entry[2], entry[3]]);
        let affine = attr0 & super::AFFINE != 0;
        let bit9 = attr0 & super::DISABLED_OR_DOUBLE != 0;
        let shape = usize::from(attr0 >> 14);
        (!affine && bit9) || shape == 3 || {
            let height = super::SIZES[shape][usize::from(attr1 >> 14)].1;
            let double = if affine && bit9 { 2 } else { 1 };
            (y + 256 - usize::from(attr0 & 0xFF)) & 0xFF >= height * double
        }
    }

    #[test]
    fn a_span_skips_the_lines_a_full_decode_skips() {
        let mut rng = Xorshift(0x1234_5677);
        for _ in 0..20_000 {
            let entry: [u8; 8] = std::array::from_fn(|_| rng.below(256) as u8);
            let lines: Vec<usize> = super::Span::of(super::Span::key(&entry)).lines().collect();
            for y in 0..228 {
                assert_eq!(
                    !lines.contains(&y),
                    decode_skips(&entry, y),
                    "{entry:02X?} line {y}"
                );
            }
        }
    }

    #[test]
    fn moved_spans_leave_each_line_with_the_sprites_a_full_decode_draws() {
        let mut rng = Xorshift(0x0BAD_5EED);
        let mut sprites = super::Sprites::default();
        let mut oam = [[0u8; 8]; super::SPRITES];
        for round in 0..200 {
            for _ in 0..if round == 0 { super::SPRITES } else { 6 } {
                let index = rng.below(super::SPRITES as u32) as usize;
                oam[index] = std::array::from_fn(|_| rng.below(256) as u8);
            }
            let keys = std::array::from_fn(|index| super::Span::key(&oam[index]));
            sprites.move_spans(&keys);
            for (y, &line) in sprites.lines.iter().enumerate().take(228) {
                for (index, entry) in oam.iter().enumerate() {
                    let drawn = line >> index & 1 != 0;
                    assert_eq!(
                        drawn,
                        !decode_skips(entry, y),
                        "round {round} sprite {index} line {y}"
                    );
                }
            }
        }
    }
}
