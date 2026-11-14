use super::{EngineVram, Memory, OPAQUE, PALETTE_BANK, WIDTH, read16};

const SPRITES: usize = 128;
const LINES: usize = 256;
const OBJ_PALETTE: usize = 0x200;
const TILE_BYTES: u32 = 32;
const MAP_2D_MASK: u32 = 0x7FFF;
const BITMAP_2D_MASK: u32 = 0x1_FFFF;
const NO_SPRITE: u8 = 4;
const AFFINE: u16 = 1 << 8;
const DISABLED_OR_DOUBLE: u16 = 1 << 9;
const COLORS_256: u16 = 1 << 13;
const HFLIP: u16 = 1 << 12;
const VFLIP: u16 = 1 << 13;
const MOSAIC: u16 = 1 << 12;
const MODE_SEMI: u16 = 1;
const MODE_WINDOW: u16 = 2;
const MODE_BITMAP: u16 = 3;
const PALETTE_BYTES: u32 = 0x200;
const NO_ROW: u32 = u32::MAX;
const SIZES: [[(usize, usize); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

#[derive(Debug, Clone)]
pub(super) struct Sprites {
    pub color: [u16; WIDTH],
    pub priority: [u8; WIDTH],
    pub semi: [bool; WIDTH],
    pub window: [bool; WIDTH],
    pub alpha: [u8; WIDTH],
    pub any_semi: bool,
    pub levels: u8,
    drawn: bool,
    scan: Scan,
}

#[derive(Debug, Clone)]
struct Scan {
    oam: Vec<u8>,
    lines: [u128; LINES],
}

impl Default for Scan {
    fn default() -> Self {
        Self {
            oam: Vec::new(),
            lines: [0; LINES],
        }
    }
}

impl Scan {
    fn refresh(&mut self, oam: &[u8]) {
        if self.oam == oam {
            return;
        }
        self.oam.clear();
        self.oam.extend_from_slice(oam);
        self.lines.fill(0);
        let entries = oam.as_chunks::<8>().0;
        for (index, entry) in entries[..entries.len().min(SPRITES)].iter().enumerate() {
            let attr0 = u16::from_le_bytes([entry[0], entry[1]]);
            let affine = attr0 & AFFINE != 0;
            let shape = usize::from(attr0 >> 14);
            if !affine && attr0 & DISABLED_OR_DOUBLE != 0 || shape == 3 {
                continue;
            }
            let attr1 = u16::from_le_bytes([entry[2], entry[3]]);
            let (_, height) = SIZES[shape][usize::from(attr1 >> 14)];
            let double = if affine && attr0 & DISABLED_OR_DOUBLE != 0 {
                2
            } else {
                1
            };
            let top = usize::from(attr0 as u8);
            for row in 0..height * double {
                self.lines[(top + row) % LINES] |= 1 << index;
            }
        }
    }
}

impl Default for Sprites {
    fn default() -> Self {
        Self {
            color: [0; WIDTH],
            priority: [NO_SPRITE; WIDTH],
            semi: [false; WIDTH],
            window: [false; WIDTH],
            alpha: [0; WIDTH],
            any_semi: false,
            levels: 0,
            drawn: false,
            scan: Scan::default(),
        }
    }
}

pub(super) struct Layout {
    pub boundary: Option<u32>,
    pub bitmap: Option<Bitmap>,
    pub extended: bool,
    pub mosaic: (usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Bitmap {
    OneDimensional { boundary: u32 },
    TwoDimensional { width: u32 },
}

impl Sprites {
    pub fn clear(&mut self) {
        self.levels = 0;
        if !self.drawn {
            return;
        }
        self.color.fill(0);
        self.priority.fill(NO_SPRITE);
        self.semi.fill(false);
        self.window.fill(false);
        self.alpha.fill(0);
        self.any_semi = false;
        self.drawn = false;
    }

    pub fn draw<V: EngineVram>(&mut self, y: usize, layout: &Layout, memory: &Memory<V>) {
        self.clear();
        let oam = memory.oam;
        self.scan.refresh(oam);
        let entries = oam.as_chunks::<8>().0;
        let mut on_line = self.scan.lines[y % LINES];
        while on_line != 0 {
            let index = on_line.trailing_zeros() as usize;
            on_line &= on_line - 1;
            let entry = &entries[index];
            let attr0 = u16::from_le_bytes([entry[0], entry[1]]);
            let top = usize::from(attr0 as u8);
            let row = (y + 256 - top) & 0xFF;
            let affine = attr0 & AFFINE != 0;
            let shape = usize::from(attr0 >> 14);
            let mode = attr0 >> 10 & 0b11;
            let attr2 = u16::from_le_bytes([entry[4], entry[5]]);
            let bitmap = mode == MODE_BITMAP;
            if bitmap && (layout.bitmap.is_none() || attr2 >> 12 == 0) {
                continue;
            }
            let attr1 = u16::from_le_bytes([entry[2], entry[3]]);
            let (width, height) = SIZES[shape][usize::from(attr1 >> 14)];
            let double = if affine && attr0 & DISABLED_OR_DOUBLE != 0 {
                2
            } else {
                1
            };
            let mosaic = attr0 & MOSAIC != 0;
            let row = if mosaic {
                let grid = y - y % layout.mosaic.1;
                let row = (grid + 256 - top) & 0xFF;
                if row >= height * double { 0 } else { row }
            } else {
                row
            };
            let matrix = affine.then(|| {
                let group = usize::from(attr1 >> 9 & 0x1F) * 32;
                [6, 14, 22, 30].map(|at| read16(oam, group + at) as i16)
            });
            let object = Object {
                x: usize::from(attr1 & 0x1FF),
                width,
                height,
                double,
                row,
                tile: u32::from(attr2 & 0x3FF),
                wide: !bitmap && attr0 & COLORS_256 != 0,
                hflip: !affine && attr1 & HFLIP != 0,
                vflip: !affine && attr1 & VFLIP != 0,
                priority: (attr2 >> 10 & 0b11) as u8,
                bank: u32::from(attr2 >> 12),
                mode,
                mosaic: if mosaic { layout.mosaic.0 } else { 1 },
                matrix,
            };
            if mode != MODE_WINDOW {
                self.levels |= 1 << object.priority;
            }
            self.draw_object(&object, layout, memory);
        }
    }

    fn draw_object<V: EngineVram>(&mut self, object: &Object, layout: &Layout, memory: &Memory<V>) {
        let palette = memory.palette[OBJ_PALETTE..]
            .first_chunk()
            .expect("a whole obj palette");
        let plain = object.matrix.is_none() && object.mosaic <= 1;
        if plain && object.mode != MODE_WINDOW && object.mode != MODE_BITMAP {
            self.draw_plain(object, layout, memory, palette);
            return;
        }
        let box_width = object.width * object.double;
        let box_height = object.height * object.double;
        let window = object.mode == MODE_WINDOW;
        let mut row = Row {
            key: NO_ROW,
            indices: 0,
        };
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
            let Some((source_x, source_y)) = object.source(column, box_width, box_height) else {
                continue;
            };
            let color = match layout.bitmap {
                Some(bitmap) if object.mode == MODE_BITMAP => {
                    let color = object.bitmap_color(source_x, source_y, bitmap, memory);
                    if color & OPAQUE == 0 {
                        continue;
                    }
                    color
                }
                _ => {
                    let index = object.index(source_x, source_y, layout, memory, &mut row);
                    if index == 0 {
                        continue;
                    }
                    if window {
                        self.drawn = true;
                        self.window[x] = true;
                        continue;
                    }
                    object.color(index, layout, memory, palette)
                }
            };
            self.drawn = true;
            self.color[x] = color;
            self.priority[x] = object.priority;
            let semi = object.mode == MODE_SEMI || object.mode == MODE_BITMAP;
            self.semi[x] = semi;
            self.alpha[x] = if object.mode == MODE_BITMAP {
                object.bank as u8
            } else {
                0
            };
            self.any_semi |= semi;
        }
    }

    fn draw_plain<V: EngineVram>(
        &mut self,
        object: &Object,
        layout: &Layout,
        memory: &Memory<V>,
        palette: &[u8; PALETTE_BANK],
    ) {
        let semi = object.mode == MODE_SEMI;
        let sy = if object.vflip {
            object.height - 1 - object.row
        } else {
            object.row
        };
        let mut row = Row {
            key: NO_ROW,
            indices: 0,
        };
        for column in 0..object.width {
            let x = (object.x + column) & 0x1FF;
            if x >= WIDTH || self.priority[x] <= object.priority {
                continue;
            }
            let sx = if object.hflip {
                object.width - 1 - column
            } else {
                column
            };
            let index = object.index(sx as u32, sy as u32, layout, memory, &mut row);
            if index == 0 {
                continue;
            }
            self.drawn = true;
            self.color[x] = object.color(index, layout, memory, palette);
            self.priority[x] = object.priority;
            self.semi[x] = semi;
            self.alpha[x] = 0;
            self.any_semi |= semi;
        }
    }
}

struct Row {
    key: u32,
    indices: u64,
}

struct Object {
    x: usize,
    width: usize,
    height: usize,
    double: usize,
    row: usize,
    tile: u32,
    wide: bool,
    hflip: bool,
    vflip: bool,
    priority: u8,
    bank: u32,
    mode: u16,
    mosaic: usize,
    matrix: Option<[i16; 4]>,
}

impl Object {
    fn source(&self, column: usize, box_width: usize, box_height: usize) -> Option<(u32, u32)> {
        match self.matrix {
            Some([pa, pb, pc, pd]) => {
                let dx = column as i32 - (box_width / 2) as i32;
                let dy = self.row as i32 - (box_height / 2) as i32;
                let tx = ((i32::from(pa) * dx + i32::from(pb) * dy) >> 8) + (self.width / 2) as i32;
                let ty =
                    ((i32::from(pc) * dx + i32::from(pd) * dy) >> 8) + (self.height / 2) as i32;
                if tx < 0 || ty < 0 || tx >= self.width as i32 || ty >= self.height as i32 {
                    return None;
                }
                Some((tx as u32, ty as u32))
            }
            None => {
                let sx = if self.hflip {
                    self.width - 1 - column
                } else {
                    column
                };
                let sy = if self.vflip {
                    self.height - 1 - self.row
                } else {
                    self.row
                };
                Some((sx as u32, sy as u32))
            }
        }
    }

    #[inline(always)]
    fn index<V: EngineVram>(
        &self,
        sx: u32,
        sy: u32,
        layout: &Layout,
        memory: &Memory<V>,
        row: &mut Row,
    ) -> u32 {
        let key = sy << 16 | sx >> 3;
        if row.key != key {
            row.key = key;
            row.indices = self.row_indices(sx, sy, layout, memory);
        }
        let column = sx & 7;
        if self.wide {
            (row.indices >> (column * 8) & 0xFF) as u32
        } else {
            (row.indices >> (column * 4) & 0xF) as u32
        }
    }

    fn row_indices<V: EngineVram>(
        &self,
        sx: u32,
        sy: u32,
        layout: &Layout,
        memory: &Memory<V>,
    ) -> u64 {
        let (row_bytes, step) = if self.wide { (8, 2) } else { (4, 1) };
        let within = (sy & 7) * row_bytes;
        let at = match layout.boundary {
            Some(boundary) => {
                let tiles_per_row = self.width as u32 / 8;
                let cell = (sy >> 3) * tiles_per_row + (sx >> 3);
                self.tile * boundary + cell * TILE_BYTES * step + within
            }
            None => {
                let first = if self.wide { self.tile & !1 } else { self.tile };
                let unit = first + (sy >> 3) * 32 + (sx >> 3) * step;
                ((unit * TILE_BYTES) & MAP_2D_MASK) + within
            }
        };
        let low = u64::from(memory.vram.obj_u32(at));
        if self.wide {
            low | u64::from(memory.vram.obj_u32(at + 4)) << 32
        } else {
            low
        }
    }

    fn bitmap_color<V: EngineVram>(
        &self,
        sx: u32,
        sy: u32,
        bitmap: Bitmap,
        memory: &Memory<V>,
    ) -> u16 {
        let at = match bitmap {
            Bitmap::OneDimensional { boundary } => {
                self.tile * boundary + (sy * self.width as u32 + sx) * 2
            }
            Bitmap::TwoDimensional { width } => {
                let mask = width / 8 - 1;
                let base = (self.tile & mask) * 0x10 + (self.tile & !mask) * 0x80;
                (base + (sy * width + sx) * 2) & BITMAP_2D_MASK
            }
        };
        memory.vram.obj_u16(at)
    }

    fn color<V: EngineVram>(
        &self,
        index: u32,
        layout: &Layout,
        memory: &Memory<V>,
        palette: &[u8; PALETTE_BANK],
    ) -> u16 {
        if self.wide && layout.extended {
            return memory
                .vram
                .obj_palette_u16(self.bank * PALETTE_BYTES + index * 2)
                | OPAQUE;
        }
        let entry = if self.wide {
            index
        } else {
            self.bank * 16 + index
        };
        let at = (entry as usize & 0xFF) * 2;
        u16::from_le_bytes([palette[at], palette[at + 1]]) | OPAQUE
    }
}

#[cfg(test)]
mod tests {
    use super::super::EngineKind;
    use super::super::tests::Scene;

    const DISPCNT: usize = 0x00;
    const BG0CNT: usize = 0x08;
    const MOSAIC: usize = 0x4C;
    const GRAPHICS: u32 = 1 << 16;
    const BG0: u32 = 1 << 8;
    const OBJ: u32 = 1 << 12;
    const ONE_DIMENSIONAL: u32 = 1 << 4;
    const EXTENDED: u32 = 1 << 31;
    const BITMAP_WIDE: u32 = 1 << 5;
    const BITMAP_1D: u32 = 1 << 6;
    const BITMAP: u16 = 3 << 10;
    const BLDCNT: usize = 0x50;
    const BLDALPHA: usize = 0x52;

    fn object(scene: &mut Scene, index: usize, attributes: [u16; 3]) {
        for (i, value) in attributes.into_iter().enumerate() {
            let at = index * 8 + i * 2;
            scene.oam[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn parameter(scene: &mut Scene, group: usize, matrix: [i16; 4]) {
        for (i, value) in matrix.into_iter().enumerate() {
            let at = group * 32 + 6 + i * 8;
            scene.oam[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn tile_4bpp(scene: &mut Scene, at: usize, pixel: impl Fn(usize, usize) -> u8) {
        for row in 0..8 {
            for pair in 0..4 {
                let low = pixel(pair * 2, row);
                let high = pixel(pair * 2 + 1, row);
                scene.vram.obj[at + row * 4 + pair] = low | high << 4;
            }
        }
    }

    fn colors(scene: &mut Scene) {
        for i in 1..256 {
            scene.color(0x100 + i, 0x400 + i as u16);
        }
    }

    fn hide_all(scene: &mut Scene) {
        for index in 0..128 {
            object(scene, index, [1 << 9, 0, 0]);
        }
    }

    fn ready(dispcnt: u32) -> Scene {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | OBJ | dispcnt);
        hide_all(&mut scene);
        colors(&mut scene);
        scene
    }

    #[test]
    fn a_sprite_moved_or_hidden_between_lines_follows_its_new_attributes() {
        let mut scene = ready(0);
        tile_4bpp(&mut scene, 2 * 32, |column, _| column as u8 + 1);
        object(&mut scene, 0, [5, 10, 2]);
        assert_eq!(scene.line(5)[10], 0x401);
        object(&mut scene, 0, [40, 10, 2]);
        assert_eq!(scene.line(5)[10], 0);
        assert_eq!(scene.line(40)[10], 0x401);
        object(&mut scene, 0, [1 << 9 | 40, 10, 2]);
        assert_eq!(scene.line(40)[10], 0);
    }

    #[test]
    fn a_16_colour_sprite_draws_at_its_position_with_its_palette_bank() {
        let mut scene = ready(0);
        tile_4bpp(&mut scene, 2 * 32, |column, _| column as u8 + 1);
        object(&mut scene, 0, [5, 10, 2 | 1 << 12]);
        let out = scene.line(5);
        assert_eq!(out[9], 0);
        assert_eq!(
            out[10..18],
            [0x411, 0x412, 0x413, 0x414, 0x415, 0x416, 0x417, 0x418]
        );
        assert_eq!(out[18], 0);
        assert_eq!(scene.line(4)[10], 0);
        assert_eq!(scene.line(13)[10], 0);
    }

    #[test]
    fn one_dimensional_mapping_steps_tile_numbers_by_the_boundary() {
        let mut scene = ready(ONE_DIMENSIONAL | 2 << 20);
        tile_4bpp(&mut scene, 3 * 128, |_, _| 1);
        tile_4bpp(&mut scene, 3 * 128 + 32, |_, _| 2);
        object(&mut scene, 0, [1 << 14, 0, 3]);
        let out = scene.line(0);
        assert_eq!(
            [out[0], out[7], out[8], out[15]],
            [0x401, 0x401, 0x402, 0x402]
        );
    }

    #[test]
    fn two_dimensional_mapping_rows_are_32_tiles_wide() {
        let mut scene = ready(0);
        tile_4bpp(&mut scene, 5 * 32, |_, _| 1);
        tile_4bpp(&mut scene, (5 + 32) * 32, |_, _| 2);
        object(&mut scene, 0, [2 << 14, 0, 5]);
        assert_eq!(scene.line(7)[0], 0x401);
        assert_eq!(scene.line(8)[0], 0x402);
    }

    #[test]
    fn a_256_colour_sprite_uses_the_extended_palette_when_enabled() {
        let mut scene = ready(ONE_DIMENSIONAL);
        scene.vram.obj[4 * 32..4 * 32 + 64].fill(9);
        object(&mut scene, 0, [1 << 13, 0, 4 | 2 << 12]);
        assert_eq!(scene.line(0)[0], 0x409);
        let at = 2 * 0x200 + 9 * 2;
        scene.vram.obj_palette[at..at + 2].copy_from_slice(&0x0123u16.to_le_bytes());
        scene.write32(DISPCNT, GRAPHICS | OBJ | ONE_DIMENSIONAL | EXTENDED);
        assert_eq!(scene.line(0)[0], 0x0123);
    }

    #[test]
    fn flips_mirror_the_sprite() {
        let mut scene = ready(0);
        tile_4bpp(&mut scene, 32, |column, row| {
            (column + row * 2) as u8 % 15 + 1
        });
        object(&mut scene, 0, [0, 1 << 12 | 1 << 13, 1]);
        let out = scene.line(0);
        assert_eq!(out[..3], [0x407, 0x406, 0x405]);
    }

    #[test]
    fn the_lower_priority_value_wins_then_the_lower_oam_index() {
        let mut scene = ready(0);
        tile_4bpp(&mut scene, 32, |_, _| 1);
        tile_4bpp(&mut scene, 64, |_, _| 2);
        tile_4bpp(&mut scene, 96, |_, _| 3);
        object(&mut scene, 0, [0, 0, 1 | 2 << 10]);
        object(&mut scene, 1, [0, 4, 2 | 1 << 10]);
        object(&mut scene, 2, [0, 6, 3 | 1 << 10]);
        let out = scene.line(0);
        assert_eq!(
            out[..8],
            [0x401, 0x401, 0x401, 0x401, 0x402, 0x402, 0x402, 0x402]
        );
        assert_eq!(out[12..14], [0x403, 0x403]);
    }

    #[test]
    fn a_sprite_is_in_front_of_bgs_of_its_own_priority_or_lower() {
        let mut scene = ready(BG0);
        for byte in &mut scene.vram.bg[32..64] {
            *byte = 0x11;
        }
        scene.vram.put_bg16(0x800, 1);
        scene.write(BG0CNT, 1 | 1 << 8);
        scene.color(1, 0x0001);
        tile_4bpp(&mut scene, 32, |_, _| 1);
        object(&mut scene, 0, [0, 0, 1 | 1 << 10]);
        assert_eq!(scene.line(0)[0], 0x401);
        object(&mut scene, 0, [0, 0, 1 | 2 << 10]);
        assert_eq!(scene.line(0)[0], 0x0001);
    }

    #[test]
    fn an_affine_sprite_scales_inside_its_double_size_box() {
        let mut scene = ready(0);
        tile_4bpp(&mut scene, 32, |column, row| {
            (row * 8 + column) as u8 % 15 + 1
        });
        parameter(&mut scene, 3, [0x80, 0, 0, 0x80]);
        object(&mut scene, 0, [1 << 8 | 1 << 9, 3 << 9, 1]);
        let out = scene.line(0);
        assert_eq!(out[..2], [0x401, 0x401]);
        assert_eq!(out[14..16], [0x408, 0x408]);
        assert_eq!(scene.line(15)[15], 0x400 + (7 * 8 + 7) % 15 + 1);
    }

    #[test]
    fn a_sprite_near_the_bottom_wraps_to_the_top() {
        let mut scene = ready(0);
        tile_4bpp(&mut scene, 32, |_, row| row as u8 + 1);
        tile_4bpp(&mut scene, (1 + 32) * 32, |_, row| row as u8 + 8);
        object(&mut scene, 0, [250 | 2 << 14, 0, 1]);
        assert_eq!(scene.line(0)[0], 0x407);
        assert_eq!(scene.line(9)[0], 0x40F);
        assert_eq!(scene.line(10)[0], 0);
    }

    #[test]
    fn every_oam_index_draws_on_exactly_its_own_lines() {
        let mut scene = ready(0);
        tile_4bpp(&mut scene, 32, |_, _| 1);
        tile_4bpp(&mut scene, 64, |_, _| 2);
        tile_4bpp(&mut scene, 96, |_, _| 3);
        object(&mut scene, 63, [20, 0, 1]);
        object(&mut scene, 64, [24, 0, 2]);
        object(&mut scene, 127, [26, 8, 3]);
        let rows: Vec<[u16; 2]> = (19..34)
            .map(|y| {
                let out = scene.line(y);
                [out[0], out[8]]
            })
            .collect();
        let mut expected = vec![[0, 0]];
        expected.extend([[0x401, 0]; 6]);
        expected.extend([[0x401, 0x403]; 2]);
        expected.extend([[0x402, 0x403]; 4]);
        expected.extend([[0, 0x403]; 2]);
        assert_eq!(rows, expected);
    }

    #[test]
    fn hidden_sprites_draw_nothing() {
        let mut scene = ready(0);
        tile_4bpp(&mut scene, 32, |_, _| 1);
        object(&mut scene, 1, [1 << 9, 8, 1]);
        assert_eq!(scene.line(0)[8], 0);
    }

    fn put_obj16(scene: &mut Scene, at: usize, value: u16) {
        scene.vram.obj[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn bitmap_row(scene: &mut Scene, at: usize, pixels: &[u16]) {
        for (i, &pixel) in pixels.iter().enumerate() {
            put_obj16(scene, at + i * 2, pixel);
        }
    }

    #[test]
    fn a_2d_bitmap_sprite_reads_direct_colour_rows_from_a_128_dot_wide_bitmap() {
        let mut scene = ready(0);
        let base = 2 * 0x10 + 0x10 * 0x80;
        bitmap_row(
            &mut scene,
            base + 128 * 2,
            &[0x8000 | 0x1234, 0x0042, 0xFFFF],
        );
        object(&mut scene, 0, [BITMAP, 0, 0x12 | 15 << 12]);
        let out = scene.line(1);
        assert_eq!(out[..4], [0x1234, 0, 0x7FFF, 0]);
    }

    #[test]
    fn a_2d_bitmap_sprite_in_a_256_dot_wide_bitmap_steps_rows_by_512_bytes() {
        let mut scene = ready(BITMAP_WIDE);
        let base = 0x10 + 0x20 * 0x80;
        bitmap_row(&mut scene, base + 3 * 512, &[0x8001, 0x8002]);
        object(&mut scene, 0, [BITMAP | 4, 7, 0x21 | 15 << 12]);
        let out = scene.line(7);
        assert_eq!(out[6..10], [0, 1, 2, 0]);
    }

    #[test]
    fn a_1d_bitmap_sprite_starts_at_tile_times_the_boundary_with_its_own_width_as_pitch() {
        let mut scene = ready(BITMAP_1D);
        bitmap_row(&mut scene, 3 * 128 + 2 * 32, &[0x8005]);
        bitmap_row(&mut scene, 3 * 256 + 2 * 32 + 30, &[0x8006]);
        object(&mut scene, 0, [BITMAP, 1 << 14, 3 | 15 << 12]);
        assert_eq!(scene.line(2)[0], 5);
        scene.write32(DISPCNT, GRAPHICS | OBJ | BITMAP_1D | 1 << 22);
        assert_eq!(scene.line(2)[15], 6);
    }

    #[test]
    fn engine_b_keeps_the_128_byte_bitmap_boundary() {
        let mut scene = Scene::new(EngineKind::B);
        scene.write32(DISPCNT, GRAPHICS | OBJ | BITMAP_1D | 1 << 22);
        hide_all(&mut scene);
        bitmap_row(&mut scene, 3 * 128, &[0x8005]);
        object(&mut scene, 0, [BITMAP, 0, 3 | 15 << 12]);
        assert_eq!(scene.line(0)[0], 5);
    }

    #[test]
    fn alpha_0_and_the_reserved_mapping_hide_bitmap_sprites() {
        let mut scene = ready(BITMAP_1D);
        bitmap_row(&mut scene, 0, &[0x8005]);
        object(&mut scene, 0, [BITMAP, 0, 0]);
        assert_eq!(scene.line(0)[0], 0);
        object(&mut scene, 0, [BITMAP, 0, 15 << 12]);
        assert_eq!(scene.line(0)[0], 5);
        scene.write32(DISPCNT, GRAPHICS | OBJ | BITMAP_1D | BITMAP_WIDE);
        assert_eq!(scene.line(0)[0], 0);
    }

    #[test]
    fn a_bitmap_sprite_blends_with_its_oam_alpha_over_a_second_target() {
        let mut scene = ready(BITMAP_1D);
        scene.color(0, 0x7C00);
        bitmap_row(&mut scene, 0, &[0x801F]);
        scene.write(BLDCNT, 1 << 13);
        scene.write(BLDALPHA, 16);
        object(&mut scene, 0, [BITMAP, 0, 7 << 12]);
        assert_eq!(scene.line(0)[0], 15 | 15 << 10);
        object(&mut scene, 0, [BITMAP, 0, 15 << 12]);
        assert_eq!(scene.line(0)[0], 0x001F);
        object(&mut scene, 0, [BITMAP, 0, 1 << 12]);
        assert_eq!(scene.line(0)[0], 4 | 27 << 10);
        scene.write(BLDCNT, 0);
        assert_eq!(scene.line(0)[0], 0x001F);
    }

    #[test]
    fn obj_mosaic_repeats_columns() {
        let mut scene = ready(0);
        tile_4bpp(&mut scene, 32, |column, _| column as u8 + 1);
        scene.write(MOSAIC, 3 << 8);
        object(&mut scene, 0, [1 << 12, 0, 1]);
        let out = scene.line(0);
        assert_eq!(
            out[..8],
            [0x401, 0x401, 0x401, 0x401, 0x405, 0x405, 0x405, 0x405]
        );
    }
}
