use super::{OPAQUE, WIDTH, read16};

const SPRITES: usize = 128;
const OBJ_VRAM: usize = 0x1_0000;
const OBJ_VRAM_MASK: usize = 0x7FFF;
const OBJ_PALETTE: usize = 0x200;
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
        }
    }
}

pub struct Layout {
    pub one_dimensional: bool,
    pub hblank_free: bool,
    pub bitmap: bool,
    pub mosaic: (usize, usize),
}

impl Sprites {
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

    pub fn draw(&mut self, y: usize, layout: &Layout, vram: &[u8], palette: &[u8], oam: &[u8]) {
        self.clear();
        let mut budget = if layout.hblank_free {
            HBLANK_FREE_BUDGET
        } else {
            LINE_BUDGET
        };
        for sprite in 0..SPRITES {
            let attr0 = read16(oam, sprite * 8);
            let affine = attr0 & AFFINE != 0;
            if !affine && attr0 & DISABLED_OR_DOUBLE != 0 {
                continue;
            }
            let shape = usize::from(attr0 >> 14);
            if shape == 3 {
                continue;
            }
            let attr1 = read16(oam, sprite * 8 + 2);
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
            let attr2 = read16(oam, sprite * 8 + 4);
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
        let (tile_bytes, step) = if object.wide { (8, 2) } else { (4, 1) };
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
            let tile = first + (source_y >> 3) * stride + (source_x >> 3) * step;
            let at = OBJ_VRAM
                + ((tile * 32 + (source_y & 7) * tile_bytes) & OBJ_VRAM_MASK)
                + if object.wide {
                    source_x & 7
                } else {
                    (source_x & 7) >> 1
                };
            let byte = usize::from(vram[at]);
            let index = if object.wide {
                byte
            } else if source_x & 1 == 0 {
                byte & 0xF
            } else {
                byte >> 4
            };
            if index == 0 {
                continue;
            }
            self.drawn = true;
            if window {
                self.window[x] = true;
                continue;
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
    }
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
mod tests;
