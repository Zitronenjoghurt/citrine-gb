use super::{OPAQUE, WIDTH};

const PRIORITY_MASK: u16 = 0b11;
const CHAR_BLOCK: usize = 0x4000;
const SCREEN_BLOCK: usize = 0x800;
const COLORS_256: u16 = 1 << 7;
const UNUSED_ON_BG0_BG1: u16 = 1 << 13;
const SCROLL_MASK: u16 = 0x1FF;
const BG_VRAM: usize = 0x1_0000;
const SIZES: [(usize, usize); 4] = [(256, 256), (512, 256), (256, 512), (512, 512)];
const TILE_4BPP: usize = 32;
const TILE_8BPP: usize = 64;
const PALETTE_BYTES: usize = 512;

#[derive(Debug, Default, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Background {
    control: u16,
    hofs: u16,
    vofs: u16,
}

impl Background {
    pub fn control(&self) -> u16 {
        self.control
    }

    pub fn write_control(&mut self, index: usize, value: u16, mask: u16) {
        let mask = if index < 2 {
            mask & !UNUSED_ON_BG0_BG1
        } else {
            mask
        };
        self.control = (self.control & !mask) | (value & mask);
    }

    pub fn write_hofs(&mut self, value: u16, mask: u16) {
        self.hofs = ((self.hofs & !mask) | (value & mask)) & SCROLL_MASK;
    }

    pub fn write_vofs(&mut self, value: u16, mask: u16) {
        self.vofs = ((self.vofs & !mask) | (value & mask)) & SCROLL_MASK;
    }

    #[cfg(feature = "debug")]
    pub fn offsets(&self) -> (u16, u16) {
        (self.hofs, self.vofs)
    }

    pub fn priority(&self) -> u8 {
        (self.control & PRIORITY_MASK) as u8
    }

    pub fn draw_text(&self, y: usize, vram: &[u8], colors: &Colors, out: &mut [u16; WIDTH]) {
        let vram: &[u8; BG_VRAM] = vram[..BG_VRAM].try_into().unwrap();
        let (width, height) = SIZES[usize::from(self.control >> 14)];
        let sy = (y + usize::from(self.vofs)) & (height - 1);
        let row = TextRow::new(self.control, sy, width, vram);
        let sx = usize::from(self.hofs) & (width - 1);
        let start = sx & 7;
        if start == 0 {
            row.paint(colors, sx >> 3, out.as_chunks_mut::<8>().0);
        } else {
            let mut tiles = [[0; 8]; WIDTH / 8 + 1];
            row.paint(colors, sx >> 3, &mut tiles);
            out.copy_from_slice(&tiles.as_flattened()[start..start + WIDTH]);
        }
    }
}

struct TextRow<'a> {
    vram: &'a [u8; BG_VRAM],
    map: [&'a [[u8; 2]; 32]; 2],
    rows: [usize; 2],
    columns: usize,
    wide: bool,
    checked: bool,
}

impl<'a> TextRow<'a> {
    fn new(control: u16, sy: usize, width: usize, vram: &'a [u8; BG_VRAM]) -> Self {
        let ty = sy >> 3;
        let block_row = (ty >> 5) * (width >> 8);
        let row_base = usize::from(control >> 8 & 0x1F) * SCREEN_BLOCK + ((ty & 31) << 6);
        let map: [&[[u8; 2]; 32]; 2] = [0, 1].map(move |block| {
            let at = (row_base + (block_row + block) * SCREEN_BLOCK) % BG_VRAM;
            vram[at..at + 64].as_chunks::<2>().0.try_into().unwrap()
        });
        let wide = control & COLORS_256 != 0;
        let char_base = usize::from(control >> 2 & 0b11) * CHAR_BLOCK;
        let tile_bytes = if wide { TILE_8BPP } else { TILE_4BPP };
        let pitch = tile_bytes / 8;
        let line = sy & 7;
        Self {
            vram,
            map,
            rows: [line, 7 - line].map(|row| char_base + row * pitch),
            columns: (width >> 3) - 1,
            wide,
            checked: char_base + 0x400 * tile_bytes > BG_VRAM,
        }
    }

    fn paint(&self, colors: &Colors, first: usize, tiles: &mut [[u16; 8]]) {
        match (self.wide, self.checked) {
            (false, false) => self.paint_tiles::<false, false>(colors, first, tiles),
            (false, true) => self.paint_tiles::<false, true>(colors, first, tiles),
            (true, false) => self.paint_tiles::<true, false>(colors, first, tiles),
            (true, true) => self.paint_tiles::<true, true>(colors, first, tiles),
        }
    }

    #[inline(always)]
    fn paint_tiles<const WIDE: bool, const CHECKED: bool>(
        &self,
        colors: &Colors,
        first: usize,
        tiles: &mut [[u16; 8]],
    ) {
        let [upright, flipped] = self.rows;
        let mut tx = first;
        let mut done = 0;
        let mut last = (u32::MAX, [0; 8]);
        while done < tiles.len() {
            let column = tx & 31;
            let run = (32 - column).min(tiles.len() - done);
            let entries = &self.map[tx >> 5 & 1][column..column + run];
            for (pixels, entry) in tiles[done..done + run].iter_mut().zip(entries) {
                let entry = u16::from_le_bytes(*entry);
                if u32::from(entry) == last.0 {
                    *pixels = last.1;
                    continue;
                }
                let tile = usize::from(entry & 0x3FF);
                let row = if entry & (1 << 11) != 0 {
                    flipped
                } else {
                    upright
                };
                let hflip = entry & (1 << 10) != 0;
                if WIDE {
                    let at = row + tile * TILE_8BPP;
                    draw_8bpp::<CHECKED>(self.vram, &colors.wide, at, hflip, pixels);
                } else {
                    let at = row + tile * TILE_4BPP;
                    let bank = &colors.pairs[usize::from(entry >> 12)];
                    draw_4bpp::<CHECKED>(self.vram, bank, at, hflip, pixels);
                }
                last = (u32::from(entry), *pixels);
            }
            done += run;
            tx = (tx + run) & self.columns;
        }
    }
}

#[derive(Clone)]
pub struct Colors {
    source: [u8; PALETTE_BYTES],
    wide: [u16; 256],
    pairs: [[[u16; 2]; 256]; 16],
    stale: bool,
}

impl Default for Colors {
    fn default() -> Self {
        let mut colors = Self {
            source: [0; PALETTE_BYTES],
            wide: [0; 256],
            pairs: [[[0; 2]; 256]; 16],
            stale: true,
        };
        for index in 0..256 {
            colors.set(index, 0);
        }
        colors
    }
}

impl Colors {
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    pub fn refresh(&mut self, palette: &[u8]) {
        if !std::mem::take(&mut self.stale) {
            return;
        }
        let palette: &[u8; PALETTE_BYTES] = palette[..PALETTE_BYTES].try_into().unwrap();
        if *palette == self.source {
            return;
        }
        let old = std::mem::replace(&mut self.source, *palette);
        let entries = palette
            .as_chunks::<2>()
            .0
            .iter()
            .zip(old.as_chunks::<2>().0);
        for (index, (new, old)) in entries.enumerate() {
            if new != old {
                self.set(index, u16::from_le_bytes(*new));
            }
        }
    }

    fn set(&mut self, index: usize, raw: u16) {
        if index == 0 {
            return;
        }
        let color = raw | OPAQUE;
        self.wide[index] = color;
        let (bank, slot) = (index >> 4, index & 15);
        if slot == 0 {
            return;
        }
        let pairs = &mut self.pairs[bank];
        for other in 0..16 {
            pairs[slot | other << 4][0] = color;
            pairs[other | slot << 4][1] = color;
        }
    }
}

fn draw_4bpp<const CHECKED: bool>(
    vram: &[u8; BG_VRAM],
    pairs: &[[u16; 2]; 256],
    at: usize,
    hflip: bool,
    pixels: &mut [u16; 8],
) {
    if CHECKED && at >= BG_VRAM {
        *pixels = [0; 8];
        return;
    }
    let rows: &[[u8; 4]; BG_VRAM / 4] = vram.as_chunks::<4>().0.try_into().unwrap();
    let bits = u32::from_le_bytes(rows[at / 4 % (BG_VRAM / 4)]);
    if bits == 0 {
        *pixels = [0; 8];
        return;
    }
    let bits = if hflip { reverse_nibbles(bits) } else { bits };
    for (i, pair) in pixels.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        *pair = pairs[usize::from((bits >> (i * 8)) as u8)];
    }
}

fn reverse_nibbles(bits: u32) -> u32 {
    let bytes = bits.swap_bytes();
    ((bytes & 0x0F0F_0F0F) << 4) | ((bytes >> 4) & 0x0F0F_0F0F)
}

fn draw_8bpp<const CHECKED: bool>(
    vram: &[u8; BG_VRAM],
    colors: &[u16; 256],
    at: usize,
    hflip: bool,
    pixels: &mut [u16; 8],
) {
    if CHECKED && at >= BG_VRAM {
        *pixels = [0; 8];
        return;
    }
    let rows: &[[u8; 8]; BG_VRAM / 8] = vram.as_chunks::<8>().0.try_into().unwrap();
    let row = u64::from_le_bytes(rows[at / 8 % (BG_VRAM / 8)]);
    if row == 0 {
        *pixels = [0; 8];
        return;
    }
    let row = if hflip { row.swap_bytes() } else { row };
    for (i, pixel) in pixels.iter_mut().enumerate() {
        *pixel = colors[usize::from((row >> (i * 8)) as u8)];
    }
}

#[cfg(test)]
mod tests {
    use crate::gba::bus::Bus;
    use crate::gba::ppu::rgba;
    use crate::gba::ppu::tests::{bus, draw, pixel, to_line};

    const DISPCNT: u32 = 0x0400_0000;
    const BG0CNT: u32 = 0x0400_0008;
    const BG0HOFS: u32 = 0x0400_0010;
    const BG0VOFS: u32 = 0x0400_0012;
    const PALETTE: u32 = 0x0500_0000;
    const VRAM: u32 = 0x0600_0000;
    const SCREEN_BLOCK: u32 = 0x800;
    const RED: u16 = 0x001F;
    const GREEN: u16 = 0x03E0;
    const BLUE: u16 = 0x7C00;
    const BG0: u16 = 1 << 8;
    const MAP: u16 = 8;
    const MAP_BLOCK: u32 = 8 * SCREEN_BLOCK;

    fn show(bus: &mut Bus, mode: u16, layers: u16) {
        bus.write16(DISPCNT, mode | layers);
    }

    fn solid_tile_4bpp(bus: &mut Bus, at: u32, index: u8) {
        let nibbles = u16::from(index) * 0x1111;
        for half in 0..16 {
            bus.write16(at + half * 2, nibbles);
        }
    }

    fn solid_tile_8bpp(bus: &mut Bus, at: u32, index: u8) {
        for half in 0..32 {
            bus.write16(at + half * 2, u16::from(index) * 0x0101);
        }
    }

    fn map(bus: &mut Bus, block: u32, x: u32, y: u32, entry: u16) {
        bus.write16(VRAM + block * SCREEN_BLOCK + (y * 32 + x) * 2, entry);
    }

    fn color(bus: &mut Bus, index: u32, value: u16) {
        bus.write16(PALETTE + index * 2, value);
    }

    #[test]
    fn a_4bpp_tile_reads_its_palette_bank() {
        let mut bus = bus();
        solid_tile_4bpp(&mut bus, VRAM + 32, 3);
        color(&mut bus, 2 * 16 + 3, GREEN);
        map(&mut bus, 8, 1, 0, 1 | 2 << 12);
        bus.write16(BG0CNT, MAP << 8);
        show(&mut bus, 0, BG0);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 8, 0), rgba(GREEN));
        assert_eq!(pixel(&bus, 15, 7), rgba(GREEN));
        assert_eq!(pixel(&bus, 16, 0), rgba(0));
    }

    #[test]
    fn a_palette_write_shows_on_the_lines_drawn_after_it() {
        let mut bus = bus();
        solid_tile_4bpp(&mut bus, VRAM, 1);
        solid_tile_8bpp(&mut bus, VRAM + 0x8000, 17);
        color(&mut bus, 1, GREEN);
        color(&mut bus, 17, GREEN);
        bus.write16(BG0CNT, MAP << 8);
        show(&mut bus, 0, BG0);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 150), rgba(GREEN));
        to_line(&mut bus, 80);
        color(&mut bus, 1, RED);
        to_line(&mut bus, 0);
        assert_eq!(pixel(&bus, 0, 10), rgba(GREEN));
        assert_eq!(pixel(&bus, 0, 150), rgba(RED));
        bus.write16(BG0CNT, MAP << 8 | 2 << 2 | 1 << 7);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 150), rgba(GREEN));
        color(&mut bus, 17, BLUE);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 150), rgba(BLUE));
    }

    #[test]
    fn index_0_is_transparent_and_shows_the_backdrop() {
        let mut bus = bus();
        color(&mut bus, 0, BLUE);
        solid_tile_4bpp(&mut bus, VRAM, 0);
        bus.write16(BG0CNT, MAP << 8);
        show(&mut bus, 0, BG0);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(BLUE));
    }

    #[test]
    fn an_8bpp_tile_reads_the_whole_palette_from_its_char_block() {
        let mut bus = bus();
        solid_tile_8bpp(&mut bus, VRAM + 0x8000 + 2 * 64, 200);
        color(&mut bus, 200, RED);
        map(&mut bus, 8, 0, 0, 2 | 15 << 12);
        bus.write16(BG0CNT, 2 << 2 | 1 << 7 | MAP << 8);
        show(&mut bus, 0, BG0);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(RED));
        assert_eq!(pixel(&bus, 7, 7), rgba(RED));
    }

    #[test]
    fn the_left_pixel_is_the_low_nibble_and_flips_mirror_the_tile() {
        let mut bus = bus();
        bus.write16(VRAM, 0x0001);
        color(&mut bus, 1, RED);
        map(&mut bus, 8, 0, 0, 0);
        map(&mut bus, 8, 1, 0, 1 << 10);
        map(&mut bus, 8, 2, 0, 1 << 11);
        map(&mut bus, 8, 3, 0, 3 << 10);
        bus.write16(BG0CNT, MAP << 8);
        show(&mut bus, 0, BG0);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(RED));
        assert_eq!(pixel(&bus, 1, 0), rgba(0));
        assert_eq!(pixel(&bus, 15, 0), rgba(RED));
        assert_eq!(pixel(&bus, 8, 0), rgba(0));
        assert_eq!(pixel(&bus, 16, 7), rgba(RED));
        assert_eq!(pixel(&bus, 16, 0), rgba(0));
        assert_eq!(pixel(&bus, 31, 7), rgba(RED));
    }

    #[test]
    fn an_8bpp_row_flips_byte_by_byte() {
        let mut bus = bus();
        bus.write16(VRAM, 0x0009);
        color(&mut bus, 9, BLUE);
        map(&mut bus, 8, 0, 0, 1 << 10);
        bus.write16(BG0CNT, 1 << 7 | MAP << 8);
        show(&mut bus, 0, BG0);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 7, 0), rgba(BLUE));
        assert_eq!(pixel(&bus, 0, 0), rgba(0));
    }

    #[test]
    fn scrolling_moves_the_map_and_wraps_at_256() {
        let mut bus = bus();
        solid_tile_4bpp(&mut bus, VRAM + 32, 1);
        color(&mut bus, 1, RED);
        map(&mut bus, 8, 31, 31, 1);
        bus.write16(BG0CNT, MAP << 8);
        bus.write16(BG0HOFS, 248 + 256);
        bus.write16(BG0VOFS, 252);
        show(&mut bus, 0, BG0);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(RED));
        assert_eq!(pixel(&bus, 7, 3), rgba(RED));
        assert_eq!(pixel(&bus, 8, 0), rgba(0));
        assert_eq!(pixel(&bus, 0, 4), rgba(0));
    }

    fn four_blocks(bus: &mut Bus) {
        for (block, index) in [(8, 1), (9, 2), (10, 3), (11, 4)] {
            solid_tile_4bpp(bus, VRAM + index * 32, index as u8);
            map(bus, block, 0, 0, index as u16);
        }
        for (index, value) in [(1, RED), (2, GREEN), (3, BLUE), (4, 0x7FFF)] {
            color(bus, index, value);
        }
    }

    fn corner(bus: &mut Bus, size: u16, hofs: u16, vofs: u16) -> [u8; 4] {
        bus.write16(BG0CNT, MAP << 8 | size << 14);
        bus.write16(BG0HOFS, hofs);
        bus.write16(BG0VOFS, vofs);
        draw(bus);
        pixel(bus, 0, 0)
    }

    #[test]
    fn the_four_screen_sizes_lay_out_their_blocks() {
        let mut bus = bus();
        four_blocks(&mut bus);
        show(&mut bus, 0, BG0);
        assert_eq!(corner(&mut bus, 0, 256, 256), rgba(RED));
        assert_eq!(corner(&mut bus, 1, 256, 0), rgba(GREEN));
        assert_eq!(corner(&mut bus, 1, 0, 256), rgba(RED));
        assert_eq!(corner(&mut bus, 2, 0, 256), rgba(GREEN));
        assert_eq!(corner(&mut bus, 2, 256, 0), rgba(RED));
        assert_eq!(corner(&mut bus, 3, 256, 0), rgba(GREEN));
        assert_eq!(corner(&mut bus, 3, 0, 256), rgba(BLUE));
        assert_eq!(corner(&mut bus, 3, 256, 256), rgba(0x7FFF));
    }

    #[test]
    fn tiles_past_the_64k_bg_area_are_transparent() {
        let mut bus = bus();
        color(&mut bus, 0, BLUE);
        solid_tile_4bpp(&mut bus, VRAM + 0x1_0000, 1);
        color(&mut bus, 1, RED);
        map(&mut bus, 8, 0, 0, 512);
        bus.write16(BG0CNT, 3 << 2 | MAP << 8);
        show(&mut bus, 0, BG0);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(BLUE));
    }

    #[test]
    fn mode_1_draws_bg0_and_bg1_as_text_and_mode_2_none() {
        let mut bus = bus();
        color(&mut bus, 0, BLUE);
        solid_tile_4bpp(&mut bus, VRAM, 1);
        color(&mut bus, 1, RED);
        for bg in 0..4 {
            bus.write16(BG0CNT + bg * 2, MAP << 8);
        }
        for (mode, layer, expected) in [
            (1, 0, RED),
            (1, 1, RED),
            (1, 3, BLUE),
            (2, 0, BLUE),
            (2, 1, BLUE),
        ] {
            show(&mut bus, mode, BG0 << layer);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 0), rgba(expected), "mode {mode} bg{layer}");
        }
    }

    #[test]
    fn bgcnt_reads_back_without_bit_13_on_bg0_and_bg1() {
        let mut bus = bus();
        for bg in 0..4 {
            bus.write16(BG0CNT + bg * 2, 0xFFFF);
        }
        assert_eq!(bus.read16(BG0CNT), 0xDFFF);
        assert_eq!(bus.read16(BG0CNT + 2), 0xDFFF);
        assert_eq!(bus.read16(BG0CNT + 4), 0xFFFF);
        assert_eq!(bus.read16(BG0CNT + 6), 0xFFFF);
    }

    #[test]
    fn scroll_registers_are_write_only_and_byte_writes_land() {
        let mut bus = bus();
        solid_tile_4bpp(&mut bus, VRAM + 32, 1);
        color(&mut bus, 1, RED);
        map(&mut bus, 8, 1, 0, 1);
        bus.write16(BG0CNT, MAP << 8);
        bus.write8(BG0HOFS, 8);
        show(&mut bus, 0, BG0);
        draw(&mut bus);
        assert_eq!(bus.read16(BG0HOFS), 0);
        assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    }

    #[test]
    fn map_entries_come_from_the_selected_screen_block() {
        let mut bus = bus();
        solid_tile_4bpp(&mut bus, VRAM + 32, 1);
        color(&mut bus, 1, GREEN);
        bus.write16(VRAM + 31 * SCREEN_BLOCK, 1);
        bus.write16(VRAM + MAP_BLOCK, 0);
        bus.write16(BG0CNT, 31 << 8);
        show(&mut bus, 0, BG0);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(GREEN));
    }

    fn reference_row(row: &[u8], bpp8: bool, palette: &[u8], hflip: bool) -> Vec<u16> {
        (0..8)
            .map(|i| {
                let column = if hflip { 7 - i } else { i };
                let index = if bpp8 {
                    usize::from(row[column])
                } else {
                    usize::from(row[column / 2] >> (column % 2 * 4) & 0xF)
                };
                palette_color(palette, index)
            })
            .collect()
    }

    fn palette_color(palette: &[u8], index: usize) -> u16 {
        if index == 0 {
            0
        } else {
            u16::from_le_bytes([palette[index * 2], palette[index * 2 + 1]]) | super::OPAQUE
        }
    }

    fn reference_text(
        background: &super::Background,
        x: usize,
        y: usize,
        vram: &[u8],
        palette: &[u8],
    ) -> u16 {
        let control = usize::from(background.control);
        let (width, height) = super::SIZES[control >> 14];
        let sx = (x + usize::from(background.hofs)) % width;
        let sy = (y + usize::from(background.vofs)) % height;
        let block = sx / 256 + sy / 256 * (width / 256);
        let cell = (sy / 8 % 32 * 32 + sx / 8 % 32) * 2;
        let map = ((control >> 8 & 0x1F) * 0x800 + block * 0x800 + cell) % 0x1_0000;
        let entry = usize::from(u16::from_le_bytes([vram[map], vram[map + 1]]));
        let column = if entry & (1 << 10) != 0 {
            7 - sx % 8
        } else {
            sx % 8
        };
        let line = if entry & (1 << 11) != 0 {
            7 - sy % 8
        } else {
            sy % 8
        };
        let char_base = (control >> 2 & 3) * 0x4000;
        let tile = entry & 0x3FF;
        if control & (1 << 7) != 0 {
            let at = char_base + tile * 64 + line * 8 + column;
            if at >= 0x1_0000 {
                return 0;
            }
            palette_color(palette, usize::from(vram[at]))
        } else {
            let at = char_base + tile * 32 + line * 4 + column / 2;
            if at >= 0x1_0000 {
                return 0;
            }
            let nibble = usize::from(vram[at] >> (column % 2 * 4) & 0xF);
            if nibble == 0 {
                return 0;
            }
            palette_color(palette, (entry >> 12) * 16 + nibble)
        }
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
    fn colors_follow_each_palette_write() {
        let mut rng = Xorshift(0x51F1_5EED);
        let mut palette = [0u8; 512];
        let mut colors = super::Colors::default();
        for round in 0..400 {
            let writes = if round % 50 == 0 { 512 } else { rng.below(6) };
            for _ in 0..writes {
                palette[rng.below(512) as usize] = rng.below(256) as u8;
            }
            colors.mark_stale();
            colors.refresh(&palette);
            let color = |index: usize| {
                u16::from_le_bytes([palette[2 * index], palette[2 * index + 1]]) | super::OPAQUE
            };
            let banked = |bank: usize, slot: usize| {
                if slot == 0 {
                    0
                } else {
                    color(bank * 16 + slot)
                }
            };
            for (index, &wide) in colors.wide.iter().enumerate() {
                let expected = if index == 0 { 0 } else { color(index) };
                assert_eq!(wide, expected, "round {round} wide {index}");
            }
            for (bank, pairs) in colors.pairs.iter().enumerate() {
                for (byte, &pair) in pairs.iter().enumerate() {
                    let expected = [banked(bank, byte & 15), banked(bank, byte >> 4)];
                    assert_eq!(pair, expected, "round {round} bank {bank} {byte:02X}");
                }
            }
        }
    }

    #[test]
    fn tile_rows_paint_like_walking_each_pixel() {
        let mut rng = Xorshift(0x2545_F491);
        let palette: Vec<u8> = (0..1024).map(|_| rng.below(256) as u8).collect();
        let mut colors = super::Colors::default();
        colors.refresh(&palette);
        let mut vram = [0u8; 0x1_0000];
        for _ in 0..4000 {
            let row: Vec<u8> = (0..8)
                .map(|_| match rng.below(3) {
                    0 => 0,
                    1 => rng.below(16) as u8,
                    _ => rng.below(256) as u8,
                })
                .collect();
            vram[..8].copy_from_slice(&row);
            let hflip = rng.below(2) == 1;
            let bank = rng.below(16) as usize;
            let mut pixels = [0xFFFF; 8];
            super::draw_4bpp::<false>(&vram, &colors.pairs[bank], 0, hflip, &mut pixels);
            let expected = reference_row(&row, false, &palette[bank * 32..], hflip);
            assert_eq!(pixels[..], expected[..], "4bpp {row:02X?} {hflip}");
            let mut pixels = [0xFFFF; 8];
            super::draw_8bpp::<false>(&vram, &colors.wide, 0, hflip, &mut pixels);
            let expected = reference_row(&row, true, &palette, hflip);
            assert_eq!(pixels[..], expected[..], "8bpp {row:02X?} {hflip}");
        }
    }

    #[test]
    fn text_lines_paint_like_walking_each_pixel() {
        let mut rng = Xorshift(0x9E37_79B9);
        let vram = random_vram(&mut rng);
        assert_text_lines(&mut rng, &vram);
    }

    #[test]
    fn repeated_map_entries_paint_like_walking_each_pixel() {
        let mut rng = Xorshift(0x2545_F491);
        let mut vram = random_vram(&mut rng);
        for at in (2..0x1_0000).step_by(2) {
            if rng.below(4) != 0 {
                vram.copy_within(at - 2..at, at);
            }
        }
        assert_text_lines(&mut rng, &vram);
    }

    fn random_vram(rng: &mut Xorshift) -> Vec<u8> {
        let mut vram = vec![0u8; 0x1_8000];
        for word in vram.chunks_mut(4) {
            if rng.below(3) != 0 {
                for byte in word {
                    *byte = rng.below(256) as u8;
                }
            }
        }
        vram
    }

    fn assert_text_lines(rng: &mut Xorshift, vram: &[u8]) {
        let palette: Vec<u8> = (0..512).map(|_| rng.below(256) as u8).collect();
        let mut colors = super::Colors::default();
        colors.refresh(&palette);
        for _ in 0..3000 {
            let background = super::Background {
                control: rng.below(0x1_0000) as u16,
                hofs: rng.below(0x200) as u16,
                vofs: rng.below(0x200) as u16,
            };
            let y = rng.below(160) as usize;
            let mut out = [0xFFFF; super::WIDTH];
            background.draw_text(y, vram, &colors, &mut out);
            let expected: Vec<u16> = (0..super::WIDTH)
                .map(|x| reference_text(&background, x, y, vram, &palette))
                .collect();
            assert_eq!(out[..], expected[..], "{background:?} {y}");
        }
    }
}
