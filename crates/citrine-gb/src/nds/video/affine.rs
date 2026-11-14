use super::text::{Colors, Tile, clear, paint};
use super::{EngineVram, Memory, OPAQUE, WIDTH};

const ONE: i16 = 0x100;
const WRAP: u16 = 1 << 13;
const TILE_8BPP: u32 = 64;
const SLOT_BYTES: u32 = 0x2000;
const PALETTE_BYTES: u32 = 0x200;
const NO_GROUP: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Map {
    Bytes,
    Entries { extended: Option<u32> },
    Indexed,
    Direct,
}

pub(super) struct Layout {
    pub control: u16,
    pub size: (i32, i32),
    pub char_base: u32,
    pub screen_base: u32,
    pub map: Map,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(super) struct Affine {
    pa: i16,
    pb: i16,
    pc: i16,
    pd: i16,
    x: u32,
    y: u32,
    line_x: i32,
    line_y: i32,
}

impl Default for Affine {
    fn default() -> Self {
        Self {
            pa: ONE,
            pb: 0,
            pc: 0,
            pd: ONE,
            x: 0,
            y: 0,
            line_x: 0,
            line_y: 0,
        }
    }
}

impl Affine {
    pub fn write(&mut self, offset: usize, value: u16, mask: u16) {
        let merge = |old: i16| ((old as u16 & !mask) | (value & mask)) as i16;
        match offset {
            0 => self.pa = merge(self.pa),
            2 => self.pb = merge(self.pb),
            4 => self.pc = merge(self.pc),
            6 => self.pd = merge(self.pd),
            8 | 10 => {
                merge_reference(&mut self.x, offset == 10, value, mask);
                self.line_x = signed(self.x);
            }
            _ => {
                merge_reference(&mut self.y, offset == 14, value, mask);
                self.line_y = signed(self.y);
            }
        }
    }

    pub fn latch(&mut self) {
        self.line_x = signed(self.x);
        self.line_y = signed(self.y);
    }

    pub fn step(&mut self) {
        self.line_x = self.line_x.wrapping_add(i32::from(self.pb));
        self.line_y = self.line_y.wrapping_add(i32::from(self.pd));
    }

    pub fn draw<V: EngineVram>(
        &self,
        layout: &Layout,
        back: usize,
        memory: &Memory<V>,
        out: &mut [u16; WIDTH],
    ) {
        let back = back as i32;
        let x = self.line_x.wrapping_sub(back * i32::from(self.pb));
        let y = self.line_y.wrapping_sub(back * i32::from(self.pd));
        if self.pa == ONE && self.pc == 0 && layout.map != Map::Direct {
            layout.span(x >> 8, y >> 8, memory, out);
        } else {
            let step = (i32::from(self.pa), i32::from(self.pc));
            layout.stepped(x, y, step, memory, out);
        }
    }
}

struct Group<'a> {
    key: u32,
    indices: u64,
    colors: Colors<'a>,
}

fn group_key(px: u32, py: u32) -> u32 {
    py << 16 | px >> 3
}

struct Cell<'a> {
    key: u32,
    at: u32,
    pixels: Option<&'a [u8; 64]>,
    flip: u32,
    colors: Colors<'a>,
}

fn cell_key(px: u32, py: u32) -> u32 {
    (py >> 3) << 16 | px >> 3
}

fn inside(start: i32, step: i32, limit: i32) -> Option<std::ops::Range<usize>> {
    let pixels = WIDTH as i64;
    i32::try_from(i64::from(start) + i64::from(step) * (pixels - 1)).ok()?;
    let (start, step) = (i64::from(start), i64::from(step));
    let (low, high) = (0, i64::from(limit) * 256 - 1);
    let (start, step, low, high) = if step < 0 {
        (-start, -step, -high, -low)
    } else {
        (start, step, low, high)
    };
    if step == 0 {
        return Some(if (low..=high).contains(&start) {
            0..WIDTH
        } else {
            0..0
        });
    }
    let first = (-(start - low).div_euclid(step)).clamp(0, pixels);
    let end = ((high - start).div_euclid(step) + 1).clamp(first, pixels);
    Some(first as usize..end as usize)
}

impl Layout {
    fn span<V: EngineVram>(
        &self,
        mut px: i32,
        py: i32,
        memory: &Memory<V>,
        out: &mut [u16; WIDTH],
    ) {
        let (width, height) = self.size;
        let wrap = self.control & WRAP != 0;
        let py = if wrap { py & (height - 1) } else { py };
        if py < 0 || py >= height {
            out.fill(0);
            return;
        }
        let mut at = 0;
        while at < WIDTH {
            let column = if wrap { px & (width - 1) } else { px };
            let run = (8 - (column & 7) as usize).min(WIDTH - at);
            let pixels = &mut out[at..at + run];
            at += run;
            px += run as i32;
            if column < 0 || column >= width {
                clear(pixels);
                continue;
            }
            let group = self.group(memory, column as u32, py as u32);
            let indices = group.indices >> ((column & 7) * 8);
            paint(pixels, |i| {
                group.colors.color(memory, (indices >> (i * 8)) as u8)
            });
        }
    }

    fn stepped<V: EngineVram>(
        &self,
        x: i32,
        y: i32,
        step: (i32, i32),
        memory: &Memory<V>,
        out: &mut [u16; WIDTH],
    ) {
        match self.map {
            Map::Direct => {
                let width = self.size.0 as u32;
                self.walk(x, y, step, out, |px, py| {
                    let color = memory.vram.bg_u16(self.screen_base + (py * width + px) * 2);
                    if color & OPAQUE != 0 { color } else { 0 }
                });
            }
            Map::Indexed => {
                let mut group = Group {
                    key: NO_GROUP,
                    indices: 0,
                    colors: Colors::standard(memory),
                };
                self.walk(x, y, step, out, |px, py| {
                    if group.key != group_key(px, py) {
                        group = self.group(memory, px, py);
                    }
                    let index = (group.indices >> ((px & 7) * 8)) as u8;
                    group.colors.color(memory, index)
                });
            }
            Map::Bytes | Map::Entries { .. } => {
                let mut cell = Cell {
                    key: NO_GROUP,
                    at: 0,
                    pixels: None,
                    flip: 0,
                    colors: Colors::standard(memory),
                };
                self.walk(x, y, step, out, |px, py| {
                    if cell.key != cell_key(px, py) {
                        cell = self.cell(memory, px, py);
                    }
                    let at = ((py & 7) << 3 | (px & 7)) ^ cell.flip;
                    let index = match cell.pixels {
                        Some(pixels) => pixels[at as usize],
                        None => memory.vram.bg(cell.at + at),
                    };
                    cell.colors.color(memory, index)
                });
            }
        }
    }

    #[inline(always)]
    fn walk(
        &self,
        mut x: i32,
        mut y: i32,
        (dx, dy): (i32, i32),
        out: &mut [u16; WIDTH],
        mut pixel_at: impl FnMut(u32, u32) -> u16,
    ) {
        let (width, height) = self.size;
        let shown = if self.control & WRAP != 0 {
            0..WIDTH
        } else {
            match (inside(x, dx, width), inside(y, dy, height)) {
                (Some(across), Some(down)) => {
                    across.start.max(down.start)..across.end.min(down.end)
                }
                _ => 0..0,
            }
        };
        if shown.is_empty() {
            out.fill(0);
            return;
        }
        out[..shown.start].fill(0);
        out[shown.end..].fill(0);
        x = x.wrapping_add(shown.start as i32 * dx);
        y = y.wrapping_add(shown.start as i32 * dy);
        for pixel in &mut out[shown] {
            let (px, py) = ((x >> 8) & (width - 1), (y >> 8) & (height - 1));
            x = x.wrapping_add(dx);
            y = y.wrapping_add(dy);
            *pixel = pixel_at(px as u32, py as u32);
        }
    }

    fn cell<'a, V: EngineVram>(&self, memory: &Memory<'a, V>, px: u32, py: u32) -> Cell<'a> {
        let width = self.size.0 as u32;
        let cell = (py >> 3) * (width >> 3) + (px >> 3);
        let (number, flip, colors) = match self.map {
            Map::Entries { extended } => {
                let entry = memory.vram.bg_u16(self.screen_base + cell * 2);
                let tile = Tile::from_entry(entry, 0);
                let flip = tile.row << 3 | if tile.hflip { 7 } else { 0 };
                let palette = extended.map(|slot| slot * SLOT_BYTES + tile.bank * PALETTE_BYTES);
                (tile.number, flip, Colors::of(memory, palette))
            }
            _ => {
                let number = u32::from(memory.vram.bg(self.screen_base + cell));
                (number, 0, Colors::standard(memory))
            }
        };
        let at = self.char_base + number * TILE_8BPP;
        Cell {
            key: cell_key(px, py),
            at,
            pixels: memory.vram.bg_tile(at),
            flip,
            colors,
        }
    }

    fn group<'a, V: EngineVram>(&self, memory: &Memory<'a, V>, px: u32, py: u32) -> Group<'a> {
        let row = |at: u32| {
            u64::from(memory.vram.bg_u32(at)) | u64::from(memory.vram.bg_u32(at + 4)) << 32
        };
        let width = self.size.0 as u32;
        let cell = (py >> 3) * (width >> 3) + (px >> 3);
        let standard = Colors::standard(memory);
        let (indices, colors) = match self.map {
            Map::Bytes => {
                let tile = u32::from(memory.vram.bg(self.screen_base + cell));
                let at = self.char_base + tile * TILE_8BPP + (py & 7) * 8;
                (row(at), standard)
            }
            Map::Entries { extended } => {
                let entry = memory.vram.bg_u16(self.screen_base + cell * 2);
                let tile = Tile::from_entry(entry, py as usize);
                let indices = row(self.char_base + tile.number * TILE_8BPP + tile.row * 8);
                let palette = extended.map(|slot| slot * SLOT_BYTES + tile.bank * PALETTE_BYTES);
                let indices = if tile.hflip {
                    indices.swap_bytes()
                } else {
                    indices
                };
                (indices, Colors::of(memory, palette))
            }
            Map::Indexed | Map::Direct => {
                (row(self.screen_base + py * width + (px & !7)), standard)
            }
        };
        Group {
            key: group_key(px, py),
            indices,
            colors,
        }
    }
}

fn merge_reference(reference: &mut u32, high: bool, value: u16, mask: u16) {
    let shift = if high { 16 } else { 0 };
    *reference = (*reference & !(u32::from(mask) << shift)) | u32::from(value & mask) << shift;
}

fn signed(reference: u32) -> i32 {
    ((reference << 4) as i32) >> 4
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Scene, Tiled};
    use super::super::{EngineKind, EngineVram, HEIGHT, Memory, WIDTH};
    use super::{Layout, Map};

    const DISPCNT: usize = 0x00;
    const BG2CNT: usize = 0x0C;
    const BG3CNT: usize = 0x0E;
    const BG2PA: usize = 0x20;
    const BG2PB: usize = 0x22;
    const BG2X: usize = 0x28;
    const BG2Y: usize = 0x2C;
    const BG3PA: usize = 0x30;
    const BG3X: usize = 0x38;
    const BG3Y: usize = 0x3C;
    const MOSAIC: usize = 0x4C;
    const BITMAP: u16 = 1 << 7;
    const DIRECT: u16 = 1 << 2;
    const MOSAIC_BIT: u16 = 1 << 6;
    const WRAP: u16 = 1 << 13;
    const BACKDROP: u16 = 0x0C63;
    const GRAPHICS: u32 = 1 << 16;
    const BG2: u32 = 1 << 10;
    const BG3: u32 = 1 << 11;
    const EXTENDED: u32 = 1 << 30;
    const CHAR: usize = 0x4000;

    fn tile(scene: &mut Scene, number: usize, pixels: impl Fn(usize, usize) -> u8) {
        for row in 0..8 {
            for column in 0..8 {
                scene.vram.bg[CHAR + number * 64 + row * 8 + column] = pixels(column, row);
            }
        }
    }

    fn colors(scene: &mut Scene) {
        for i in 1..256 {
            scene.color(i, i as u16);
        }
    }

    fn mode_2(scene: &mut Scene) {
        scene.write32(DISPCNT, GRAPHICS | 2 | BG2);
        scene.write(BG2CNT, 1 << 2);
        colors(scene);
    }

    #[test]
    fn an_identity_matrix_draws_the_byte_map_as_is() {
        let mut scene = Scene::new(EngineKind::A);
        mode_2(&mut scene);
        tile(&mut scene, 1, |column, _| column as u8 + 1);
        tile(&mut scene, 2, |_, row| row as u8 + 20);
        scene.vram.bg[0] = 1;
        scene.vram.bg[1] = 2;
        let out = scene.line(3);
        assert_eq!(out[..8], [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(out[8..16], [23; 8]);
        assert_eq!(out[16], 0);
    }

    #[test]
    fn pa_scales_each_line_and_the_reference_point_moves_it() {
        let mut scene = Scene::new(EngineKind::A);
        mode_2(&mut scene);
        tile(&mut scene, 1, |column, row| (row * 8 + column) as u8 + 1);
        scene.vram.bg[0] = 1;
        scene.write(BG2PA, 0x80);
        let out = scene.line(0);
        assert_eq!(out[..6], [1, 1, 2, 2, 3, 3]);
        scene.write(BG2X, 2 << 8);
        scene.write(BG2Y, 3 << 8);
        let out = scene.line(0);
        assert_eq!(out[..4], [27, 27, 28, 28]);
    }

    #[test]
    fn the_internal_reference_steps_by_pb_and_pd_each_line_and_reloads_at_vblank() {
        let mut scene = Scene::new(EngineKind::A);
        mode_2(&mut scene);
        tile(&mut scene, 1, |column, row| (row * 8 + column) as u8 + 1);
        scene.vram.bg[0] = 1;
        scene.write(BG2PB, 0x100);
        assert_eq!(scene.line(0)[0], 1);
        scene.engine.start_line(1);
        assert_eq!(scene.render(1)[0], 10);
        scene.engine.start_line(2);
        assert_eq!(scene.render(2)[0], 19);
        scene.engine.start_line(HEIGHT as u16);
        scene.engine.start_line(0);
        assert_eq!(scene.render(0)[0], 1);
    }

    #[test]
    fn outside_the_map_is_transparent_unless_wraparound_is_set() {
        let mut scene = Scene::new(EngineKind::A);
        mode_2(&mut scene);
        scene.color(0, 0x7FFF);
        tile(&mut scene, 1, |_, _| 5);
        scene.vram.bg[15] = 1;
        scene.write(BG2X, (-8i32 as u32) as u16);
        scene.write(BG2X + 2, ((-8i32 as u32) >> 16) as u16 & 0x0FFF);
        let out = scene.line(0);
        assert_eq!(out[0], 0x7FFF);
        scene.write(BG2CNT, 1 << 2 | 1 << 13);
        let out = scene.line(0);
        assert_eq!(out[0], 5);
        assert_eq!(out[8], 0x7FFF);
    }

    #[test]
    fn an_extended_bg_with_16_bit_entries_flips_and_uses_extended_palettes() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | 5 | BG3 | EXTENDED);
        scene.write(BG3CNT, 1 << 2 | 1 << 8);
        tile(
            &mut scene,
            3,
            |column, row| if row == 0 { column as u8 + 1 } else { 9 },
        );
        scene.vram.put_bg16(0x800, 3 | 1 << 10 | 1 << 11 | 4 << 12);
        for i in 1..10u16 {
            let at = 3 * 0x2000 + 4 * 0x200 + usize::from(i) * 2;
            scene.vram.bg_palette[at..at + 2].copy_from_slice(&(0x100 + i).to_le_bytes());
        }
        let out = scene.line(7);
        assert_eq!(
            out[..8],
            [0x108, 0x107, 0x106, 0x105, 0x104, 0x103, 0x102, 0x101]
        );
        scene.write32(DISPCNT, GRAPHICS | 5 | BG3);
        colors(&mut scene);
        assert_eq!(scene.line(7)[..3], [8, 7, 6]);
    }

    fn bitmap(scene: &mut Scene, control: u16) {
        scene.write32(DISPCNT, GRAPHICS | 3 | BG3);
        scene.write(BG3CNT, BITMAP | control);
        colors(scene);
        scene.color(0, BACKDROP);
    }

    #[test]
    fn an_8_bit_bitmap_reads_a_byte_per_pixel_from_its_16k_block() {
        let mut scene = Scene::new(EngineKind::A);
        bitmap(&mut scene, 1 << 14 | 2 << 8);
        for x in 0..256 {
            scene.vram.bg[0x8000 + 3 * 256 + x] = x as u8;
        }
        let out = scene.line(3);
        assert_eq!(out[..4], [BACKDROP, 1, 2, 3]);
        assert_eq!(out[255], 255);
        assert_eq!(scene.line(2)[1], BACKDROP);
    }

    #[test]
    fn a_direct_colour_bitmap_shows_pixels_with_bit_15_set() {
        let mut scene = Scene::new(EngineKind::A);
        bitmap(&mut scene, DIRECT | 1 << 8);
        scene.vram.put_bg16(0x4000, 0x8000 | 0x1234);
        scene.vram.put_bg16(0x4002, 0x0042);
        scene.vram.put_bg16(0x4000 + 127 * 2, 0x8000 | 0x7C00);
        let out = scene.line(0);
        assert_eq!(out[..3], [0x1234, BACKDROP, BACKDROP]);
        assert_eq!(out[127], 0x7C00);
        assert_eq!(out[128], BACKDROP);
        scene.write(BG3CNT, BITMAP | DIRECT | 1 << 8 | WRAP);
        assert_eq!(scene.line(0)[128], 0x1234);
    }

    #[test]
    fn bitmap_sizes_set_the_row_pitch_and_the_wrap() {
        let mut scene = Scene::new(EngineKind::A);
        bitmap(&mut scene, 2 << 14 | WRAP);
        scene.vram.bg[512 + 300] = 9;
        scene.write32(BG3X, 300 << 8);
        scene.write32(BG3Y, 1 << 8);
        assert_eq!(scene.line(0)[0], 9);
        scene.write32(BG3Y, 257 << 8);
        assert_eq!(scene.line(0)[0], 9);
        scene.write(BG3CNT, BITMAP | 3 << 14 | WRAP);
        assert_eq!(scene.line(0)[0], BACKDROP);
        scene.vram.bg[257 * 512 + 300] = 11;
        assert_eq!(scene.line(0)[0], 11);
    }

    #[test]
    fn a_bitmap_takes_the_affine_matrix() {
        let mut scene = Scene::new(EngineKind::A);
        bitmap(&mut scene, 1 << 14);
        for (x, byte) in scene.vram.bg.iter_mut().enumerate().take(256) {
            *byte = x as u8;
        }
        scene.write(BG3PA, 0x80);
        assert_eq!(scene.line(0)[..6], [BACKDROP, BACKDROP, 1, 1, 2, 2]);
        scene.write(BG3PA, 0x200);
        assert_eq!(scene.line(0)[1..4], [2, 4, 6]);
    }

    #[test]
    fn bitmap_mosaic_repeats_columns_and_rows() {
        let mut scene = Scene::new(EngineKind::A);
        bitmap(&mut scene, 1 << 14 | MOSAIC_BIT);
        for y in 0..4 {
            for x in 0..256 {
                scene.vram.bg[y * 256 + x] = (y * 16 + x + 1) as u8;
            }
        }
        scene.write(MOSAIC, 0x21);
        assert_eq!(scene.line(1)[..4], [1, 1, 3, 3]);
        assert_eq!(scene.line(2)[..2], [1, 1]);
        assert_eq!(scene.line(3)[..2], [49, 49]);
    }

    #[test]
    fn mode_6_draws_bg2_as_a_large_8_bit_bitmap_from_the_start_of_vram() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | 6 | BG2);
        scene.write(BG2CNT, 0x1F << 8);
        colors(&mut scene);
        scene.color(0, BACKDROP);
        scene.vram.bg[600 * 512 + 500] = 5;
        scene.vram.bg[300 * 1024 + 900] = 6;
        scene.write32(BG2X, 500 << 8);
        scene.write32(BG2Y, 600 << 8);
        assert_eq!(scene.line(0)[..2], [5, BACKDROP]);
        scene.write(BG2CNT, 1 << 14);
        scene.write32(BG2X, 900 << 8);
        assert_eq!(scene.line(0)[0], BACKDROP);
        scene.write32(BG2Y, 300 << 8);
        assert_eq!(scene.line(0)[0], 6);
        scene.write32(BG2Y, 0);
        scene.write32(BG2X, 1023 << 8);
        scene.vram.bg[1023] = 7;
        assert_eq!(scene.line(0)[..2], [7, BACKDROP]);
    }

    #[test]
    fn the_mode_decides_which_bgs_are_affine() {
        let mut scene = Scene::new(EngineKind::A);
        colors(&mut scene);
        tile(&mut scene, 1, |_, _| 6);
        scene.vram.bg[0] = 1;
        scene.vram.bg[2] = 1;
        scene.write(BG3CNT, 1 << 2 | 1 << 7);
        scene.write32(DISPCNT, GRAPHICS | 1 | BG3);
        let out = scene.line(0);
        assert_eq!([out[0], out[8], out[16]], [6, 0, 6]);
        scene.write32(DISPCNT, GRAPHICS | BG3);
        let out = scene.line(0);
        assert_eq!(out[..16], [6; 16]);
    }

    #[test]
    fn an_unturned_unscaled_line_matches_the_stepped_walk_for_every_map_and_offset() {
        let mut scene = Scene::new(EngineKind::A);
        let mut seed = 0x9E37_79B9u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let bytes = [
            &mut scene.vram.bg,
            &mut scene.vram.bg_palette,
            &mut scene.palette,
        ];
        for byte in bytes.into_iter().flat_map(|bytes| bytes.iter_mut()) {
            *byte = next() as u8;
        }
        let memory = Memory {
            vram: &scene.vram,
            palette: &scene.palette,
            oam: &scene.oam,
            three_d: None,
        };
        let maps = [
            Map::Bytes,
            Map::Entries { extended: None },
            Map::Entries { extended: Some(1) },
            Map::Indexed,
        ];
        let sizes = [(128, 128), (256, 256), (512, 256), (1024, 512)];
        for round in 0..512 {
            let layout = Layout {
                control: if round & 16 == 0 { 0 } else { WRAP },
                size: sizes[round / 4 % 4],
                char_base: next() & 0x3_C000,
                screen_base: next() & 0x1_F800,
                map: maps[round % 4],
            };
            let x = (next() % (1600 << 8)) as i32 - (300 << 8);
            let y = (next() % (700 << 8)) as i32 - (60 << 8);
            let (mut span, mut stepped) = ([1; WIDTH], [2; WIDTH]);
            layout.span(x >> 8, y >> 8, &memory, &mut span);
            layout.stepped(x, y, (0x100, 0), &memory, &mut stepped);
            assert_eq!(span, stepped, "round {round} x {x} y {y}");
        }
    }

    #[test]
    fn a_walk_past_the_i32_range_stays_transparent_outside_the_map() {
        let scene = Scene::new(EngineKind::A);
        let memory = Memory {
            vram: &scene.vram,
            palette: &scene.palette,
            oam: &scene.oam,
            three_d: None,
        };
        let layout = Layout {
            control: 0,
            size: (256, 256),
            char_base: 0,
            screen_base: 0,
            map: Map::Bytes,
        };
        let mut out = [1; WIDTH];
        layout.stepped(i32::MAX - 0x100, 0, (0x400, 0), &memory, &mut out);
        assert_eq!(out, [0; WIDTH]);
    }

    fn turned_lines_match_row_reads<V: EngineVram>(
        memory: &Memory<V>,
        mut next: impl FnMut() -> u32,
    ) {
        let maps = [
            Map::Bytes,
            Map::Entries { extended: None },
            Map::Entries { extended: Some(1) },
            Map::Indexed,
            Map::Direct,
        ];
        let sizes = [(128, 128), (256, 256), (512, 256), (1024, 512)];
        for round in 0..1000 {
            let layout = Layout {
                control: if round & 16 == 0 { 0 } else { super::WRAP },
                size: sizes[round / 5 % 4],
                char_base: next() & 0x3_C000,
                screen_base: next() & 0x1_F800,
                map: maps[round % 5],
            };
            let x = (next() % (1600 << 8)) as i32 - (300 << 8);
            let y = (next() % (700 << 8)) as i32 - (60 << 8);
            let turn = (
                (next() % 0x800) as i32 - 0x400,
                (next() % 0x800) as i32 - 0x400,
            );
            let step = match round % 7 {
                0 => (0, turn.1),
                1 => (turn.0, 0),
                2 => (0, 0),
                _ => turn,
            };
            let mut stepped = [1; WIDTH];
            layout.stepped(x, y, step, memory, &mut stepped);
            let (width, height) = layout.size;
            let wrap = layout.control & super::WRAP != 0;
            for (i, &pixel) in stepped.iter().enumerate() {
                let i = i as i32;
                let (mut px, mut py) = ((x + i * step.0) >> 8, (y + i * step.1) >> 8);
                if wrap {
                    px &= width - 1;
                    py &= height - 1;
                }
                let expected = if px < 0 || py < 0 || px >= width || py >= height {
                    0
                } else if layout.map == Map::Direct {
                    let at = layout.screen_base + (py * width + px) as u32 * 2;
                    let color = memory.vram.bg_u16(at);
                    if color & super::OPAQUE != 0 { color } else { 0 }
                } else {
                    let group = layout.group(memory, px as u32, py as u32);
                    let index = (group.indices >> ((px & 7) * 8)) as u8;
                    group.colors.color(memory, index)
                };
                assert_eq!(pixel, expected, "round {round} pixel {i} step {step:?}");
            }
        }
    }

    #[test]
    fn a_turned_line_reads_the_same_pixels_as_row_reads_with_or_without_whole_tiles() {
        let mut scene = Scene::new(EngineKind::A);
        let mut seed = 0x2545_F491u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let bytes = [
            &mut scene.vram.bg,
            &mut scene.vram.bg_palette,
            &mut scene.palette,
        ];
        for byte in bytes.into_iter().flat_map(|bytes| bytes.iter_mut()) {
            *byte = next() as u8;
        }
        let memory = Memory {
            vram: &scene.vram,
            palette: &scene.palette,
            oam: &scene.oam,
            three_d: None,
        };
        turned_lines_match_row_reads(&memory, &mut next);
        let tiled = Tiled(&scene.vram);
        let memory = Memory {
            vram: &tiled,
            palette: &scene.palette,
            oam: &scene.oam,
            three_d: None,
        };
        turned_lines_match_row_reads(&memory, &mut next);
    }
}
