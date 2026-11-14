use super::fetch::Word;
use super::{EngineVram, Memory, OPAQUE, PALETTE_BANK, WIDTH};

const PRIORITY_MASK: u16 = 0b11;
const CHAR_BLOCK: u32 = 0x4000;
const SCREEN_BLOCK: u32 = 0x800;
const COLORS_256: u16 = 1 << 7;
const SECOND_SLOT: u16 = 1 << 13;
const SCROLL_MASK: u16 = 0x1FF;
const SIZES: [(usize, usize); 4] = [(256, 256), (512, 256), (256, 512), (512, 512)];
const TILE_4BPP: u32 = 32;
const TILE_8BPP: u32 = 64;
const SLOT_BYTES: u32 = 0x2000;
const PALETTE_BYTES: u32 = 0x200;

pub(super) struct Source {
    pub char_base: u32,
    pub screen_base: u32,
    pub extended: bool,
}

#[derive(Debug, Default, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(super) struct Background {
    control: u16,
    hofs: u16,
    vofs: u16,
}

impl Background {
    pub fn control(&self) -> u16 {
        self.control
    }

    pub fn write_control(&mut self, value: u16, mask: u16) {
        self.control = (self.control & !mask) | (value & mask);
    }

    pub fn write_hofs(&mut self, value: u16, mask: u16) {
        self.hofs = ((self.hofs & !mask) | (value & mask)) & SCROLL_MASK;
    }

    pub fn write_vofs(&mut self, value: u16, mask: u16) {
        self.vofs = ((self.vofs & !mask) | (value & mask)) & SCROLL_MASK;
    }

    pub fn hofs(&self) -> u16 {
        self.hofs
    }

    pub fn priority(&self) -> u8 {
        (self.control & PRIORITY_MASK) as u8
    }

    pub fn char_base(&self, source: &Source) -> u32 {
        source.char_base + u32::from(self.control >> 2 & 0xF) * CHAR_BLOCK
    }

    pub fn screen_base(&self, source: &Source) -> u32 {
        source.screen_base + u32::from(self.control >> 8 & 0x1F) * SCREEN_BLOCK
    }

    pub fn slot(&self, bg: usize) -> u32 {
        let second = bg < 2 && self.control & SECOND_SLOT != 0;
        bg as u32 + if second { 2 } else { 0 }
    }

    pub fn draw_text<V: EngineVram>(
        &self,
        bg: usize,
        y: usize,
        source: &Source,
        memory: &Memory<V>,
        out: &mut [u16; WIDTH],
    ) {
        let (width, height) = SIZES[usize::from(self.control >> 14)];
        let char_base = self.char_base(source);
        let screen_base = self.screen_base(source);
        let wide = self.control & COLORS_256 != 0;
        let slot = source.extended.then(|| self.slot(bg) * SLOT_BYTES);
        let sy = (y + usize::from(self.vofs)) & (height - 1);
        let ty = sy >> 3;
        let block_row = (ty >> 5) * (width >> 8);
        let row_base = screen_base + ((ty as u32 & 31) << 6);
        let mut sx = usize::from(self.hofs) & (width - 1);
        let mut x = 0;
        let mut map = Word::default();
        let mut shades = Shades::default();
        let rows: [Option<&[u8; 64]>; 2] = std::array::from_fn(|column| {
            let block = (block_row + column) as u32;
            memory.vram.bg_tile(row_base + block * SCREEN_BLOCK)
        });
        let row_bits = |entry: u16| {
            let tile = Tile::from_entry(entry, sy);
            if wide {
                let at = char_base + tile.number * TILE_8BPP + tile.row * 8;
                u64::from(memory.vram.bg_u32(at)) | u64::from(memory.vram.bg_u32(at + 4)) << 32
            } else {
                let at = char_base + tile.number * TILE_4BPP + tile.row * 4;
                u64::from(memory.vram.bg_u32(at))
            }
        };
        let mut last = (0, row_bits(0));
        while x < WIDTH {
            let tx = sx >> 3;
            let entry = match rows[tx >> 5 & 1] {
                Some(row) => u16::from_le_bytes([row[(tx & 31) * 2], row[(tx & 31) * 2 + 1]]),
                None => {
                    let block = (block_row + (tx >> 5)) as u32;
                    map.half(
                        row_base + block * SCREEN_BLOCK + ((tx as u32 & 31) << 1),
                        |at| memory.vram.bg_u32(at),
                    )
                }
            };
            let start = sx & 7;
            let count = (8 - start).min(WIDTH - x);
            if entry != last.0 {
                last = (entry, row_bits(entry));
            }
            let bits = last.1;
            let span = &mut out[x..x + count];
            if bits == 0 {
                clear(span);
            } else if wide {
                let tile = Tile::from_entry(entry, sy);
                let colors = Colors::of(memory, slot.map(|slot| slot + tile.bank * PALETTE_BYTES));
                draw_8bpp(memory, bits, colors, start, tile.hflip, span);
            } else {
                let tile = Tile::from_entry(entry, sy);
                let colors = shades.of(memory.palette, tile.bank);
                draw_4bpp(bits as u32, colors, start, tile.hflip, span);
            }
            x += count;
            sx = (sx + count) & (width - 1);
        }
    }
}

pub(super) struct Tile {
    pub number: u32,
    pub row: u32,
    pub hflip: bool,
    pub bank: u32,
}

impl Tile {
    pub fn from_entry(entry: u16, sy: usize) -> Self {
        let row = if entry & (1 << 11) != 0 {
            7 - (sy & 7)
        } else {
            sy & 7
        };
        Self {
            number: u32::from(entry & 0x3FF),
            row: row as u32,
            hflip: entry & (1 << 10) != 0,
            bank: u32::from(entry >> 12),
        }
    }
}

#[derive(Default)]
struct Shades {
    bank: Option<u32>,
    colors: [u16; 16],
}

impl Shades {
    fn of(&mut self, palette: &[u8], bank: u32) -> &[u16; 16] {
        if self.bank != Some(bank) {
            let bytes: &[u8; 32] = palette[(bank * 32) as usize..][..32]
                .try_into()
                .expect("a 16-colour bank is 32 bytes");
            self.colors = std::array::from_fn(|index| match index {
                0 => 0,
                _ => u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]]) | OPAQUE,
            });
            self.bank = Some(bank);
        }
        &self.colors
    }
}

fn draw_4bpp(bits: u32, colors: &[u16; 16], start: usize, hflip: bool, span: &mut [u16]) {
    let bits = (if hflip { mirror_nibbles(bits) } else { bits }) >> (start * 4);
    paint(span, |i| colors[(bits >> (i * 4) & 0xF) as usize]);
}

fn draw_8bpp<V: EngineVram>(
    memory: &Memory<V>,
    bits: u64,
    colors: Colors,
    start: usize,
    hflip: bool,
    span: &mut [u16],
) {
    let bits = (if hflip { bits.swap_bytes() } else { bits }) >> (start * 8);
    paint(span, |i| colors.color(memory, (bits >> (i * 8)) as u8));
}

pub(super) fn clear(span: &mut [u16]) {
    match <&mut [u16; 8]>::try_from(&mut *span) {
        Ok(tile) => *tile = [0; 8],
        Err(_) => clear_part(span),
    }
}

#[inline(never)]
fn clear_part(span: &mut [u16]) {
    span.fill(0);
}

#[inline(always)]
pub(super) fn paint(span: &mut [u16], pixel: impl Fn(usize) -> u16) {
    match <&mut [u16; 8]>::try_from(&mut *span) {
        Ok(tile) => *tile = std::array::from_fn(pixel),
        Err(_) => {
            for (i, out) in span.iter_mut().enumerate() {
                *out = pixel(i);
            }
        }
    }
}

fn mirror_nibbles(bits: u32) -> u32 {
    let bits = bits.swap_bytes();
    (bits >> 4 & 0x0F0F_0F0F) | (bits & 0x0F0F_0F0F) << 4
}

#[derive(Clone, Copy)]
pub(super) enum Colors<'a> {
    Bytes(&'a [u8; PALETTE_BANK]),
    Vram(u32),
}

impl<'a> Colors<'a> {
    pub fn of<V: EngineVram>(memory: &Memory<'a, V>, extended: Option<u32>) -> Self {
        match extended {
            Some(address) => memory
                .vram
                .bg_palette_bank(address)
                .map_or(Self::Vram(address), Self::Bytes),
            None => Self::standard(memory),
        }
    }

    pub fn standard<V: EngineVram>(memory: &Memory<'a, V>) -> Self {
        Self::Bytes(memory.palette.first_chunk().expect("a whole bg palette"))
    }

    #[inline(always)]
    pub fn color<V: EngineVram>(self, memory: &Memory<V>, index: u8) -> u16 {
        match self {
            Self::Bytes(bytes) => {
                let at = usize::from(index) * 2;
                let color = u16::from_le_bytes([bytes[at], bytes[at + 1]]) | OPAQUE;
                if index == 0 { 0 } else { color }
            }
            Self::Vram(_) if index == 0 => 0,
            Self::Vram(address) => {
                memory.vram.bg_palette_u16(address + u32::from(index) * 2) | OPAQUE
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Scene, Tiled};
    use super::super::{EngineKind, Memory, WIDTH};
    use super::{Background, SCROLL_MASK, Source};

    const DISPCNT: usize = 0x00;
    const BG0CNT: usize = 0x08;
    const BG1CNT: usize = 0x0A;
    const BG0HOFS: usize = 0x10;
    const BG0VOFS: usize = 0x12;
    const MOSAIC: usize = 0x4C;
    const GRAPHICS: u32 = 1 << 16;
    const BG0: u32 = 1 << 8;
    const BG1: u32 = 1 << 9;
    const EXTENDED: u32 = 1 << 30;

    fn tile_4bpp(scene: &mut Scene, base: usize, tile: usize, pixels: [u8; 64]) {
        for (i, pair) in pixels.chunks(2).enumerate() {
            scene.vram.bg[base + tile * 32 + i] = pair[0] | pair[1] << 4;
        }
    }

    fn tile_8bpp(scene: &mut Scene, base: usize, tile: usize, pixels: [u8; 64]) {
        scene.vram.bg[base + tile * 64..base + tile * 64 + 64].copy_from_slice(&pixels);
    }

    fn ramp() -> [u8; 64] {
        std::array::from_fn(|i| (i % 8 + 1) as u8)
    }

    #[test]
    fn a_16_colour_tile_reads_its_palette_bank() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | BG0);
        scene.write(BG0CNT, 1 << 2 | 2 << 8);
        tile_4bpp(&mut scene, 0x4000, 1, ramp());
        scene.vram.put_bg16(0x1000, 1 | 3 << 12);
        for i in 0..16 {
            scene.color(3 * 16 + i, 0x100 + i as u16);
        }
        let out = scene.line(0);
        assert_eq!(
            out[..8],
            [0x101, 0x102, 0x103, 0x104, 0x105, 0x106, 0x107, 0x108]
        );
        assert_eq!(out[8], 0);
    }

    #[test]
    fn neighbouring_tiles_switch_palette_banks_and_back() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | BG0);
        scene.write(BG0CNT, 1 << 2 | 2 << 8);
        tile_4bpp(&mut scene, 0x4000, 1, ramp());
        for (cell, bank) in [3u16, 5, 3, 0].into_iter().enumerate() {
            scene.vram.put_bg16(0x1000 + cell * 2, 1 | bank << 12);
        }
        for i in 0..16 {
            scene.color(i, 0x200 + i as u16);
            scene.color(3 * 16 + i, 0x100 + i as u16);
            scene.color(5 * 16 + i, 0x300 + i as u16);
        }
        let out = scene.line(0);
        assert_eq!(
            [out[0], out[7], out[8], out[15], out[16], out[24], out[31]],
            [0x101, 0x108, 0x301, 0x308, 0x101, 0x201, 0x208]
        );
    }

    #[test]
    fn runs_of_one_map_entry_draw_every_tile_with_its_own_flip_and_bank() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | BG0);
        scene.write(BG0CNT, 1 << 2 | 2 << 8);
        tile_4bpp(&mut scene, 0x4000, 0, [2; 64]);
        tile_4bpp(&mut scene, 0x4000, 1, ramp());
        let entries = [0u16, 0, 1, 1, 1 | 1 << 10, 1 | 1 << 10, 1 | 1 << 12, 0];
        for (cell, entry) in entries.into_iter().enumerate() {
            scene.vram.put_bg16(0x1000 + cell * 2, entry);
        }
        for i in 1..16 {
            scene.color(i, i as u16);
            scene.color(16 + i, 0x100 + i as u16);
        }
        let ramp: [u16; 8] = std::array::from_fn(|i| i as u16 + 1);
        let mirrored = ramp.map(|color| 9 - color);
        let banked = ramp.map(|color| color + 0x100);
        let tiles = [
            [2; 8], [2; 8], ramp, ramp, mirrored, mirrored, banked, [2; 8],
        ];
        assert_eq!(scene.line(0)[..64], tiles.concat()[..]);
    }

    #[test]
    fn flips_mirror_the_tile_and_scroll_moves_it() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | BG0);
        let pixels: [u8; 64] =
            std::array::from_fn(|i| if i / 8 == 0 { (i % 8 + 1) as u8 } else { 9 });
        tile_4bpp(&mut scene, 0, 1, pixels);
        scene.vram.put_bg16(0, 1 | 1 << 10 | 1 << 11);
        for i in 1..16 {
            scene.color(i, i as u16);
        }
        let out = scene.line(7);
        assert_eq!(out[..8], [8, 7, 6, 5, 4, 3, 2, 1]);
        scene.write(BG0HOFS, 2);
        scene.write(BG0VOFS, 1);
        let out = scene.line(6);
        assert_eq!(out[..6], [6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn dispcnt_bases_add_64k_blocks_to_bgcnt_bases() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | BG0 | 2 << 24 | 3 << 27);
        scene.write(BG0CNT, 1 << 2 | 1 << 8);
        tile_4bpp(&mut scene, 0x2_0000 + 0x4000, 5, [1; 64]);
        scene.vram.put_bg16(0x3_0000 + 0x800, 5);
        scene.color(1, 0x7C00);
        assert_eq!(scene.line(0)[0], 0x7C00);
    }

    #[test]
    fn a_256_colour_tile_uses_the_standard_palette_without_extended_palettes() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | BG0);
        scene.write(BG0CNT, 1 << 7);
        tile_8bpp(&mut scene, 0, 1, [200; 64]);
        scene.vram.put_bg16(0, 1 | 5 << 12);
        scene.color(200, 0x1234);
        assert_eq!(scene.line(0)[0], 0x1234);
    }

    #[test]
    fn a_flipped_256_colour_tile_mirrors_and_scroll_moves_it() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | BG0);
        scene.write(BG0CNT, 1 << 7);
        tile_8bpp(&mut scene, 0, 1, ramp());
        scene.vram.put_bg16(0, 1 | 1 << 10);
        for i in 1..9 {
            scene.color(i, i as u16);
        }
        assert_eq!(scene.line(0)[..8], [8, 7, 6, 5, 4, 3, 2, 1]);
        scene.write(BG0HOFS, 3);
        assert_eq!(scene.line(0)[..5], [5, 4, 3, 2, 1]);
    }

    #[test]
    fn extended_palettes_pick_the_slot_and_the_sub_palette() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | BG0 | BG1 | EXTENDED);
        scene.write(BG0CNT, 1 << 7 | 1 << 2 | 1);
        scene.write(BG1CNT, 1 << 7 | 1 << 2 | 1 << 13 | 2 << 8);
        tile_8bpp(&mut scene, 0x4000, 1, [7; 64]);
        scene.vram.put_bg16(0, 1 | 5 << 12);
        scene.vram.put_bg16(0x1000 + 2, 1 | 6 << 12);
        let put = |scene: &mut Scene, slot: usize, palette: usize, color: u16| {
            let at = slot * 0x2000 + palette * 0x200 + 7 * 2;
            scene.vram.bg_palette[at..at + 2].copy_from_slice(&color.to_le_bytes());
        };
        put(&mut scene, 0, 5, 0x0011);
        put(&mut scene, 3, 6, 0x0022);
        let out = scene.line(0);
        assert_eq!(out[0], 0x0011);
        assert_eq!(out[8], 0x0022);
    }

    #[test]
    fn a_512_wide_map_continues_in_the_next_screen_block() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | BG0);
        scene.write(BG0CNT, 1 << 14 | 4 << 8);
        tile_4bpp(&mut scene, 0, 2, [1; 64]);
        scene.vram.put_bg16(4 * 0x800 + 0x800, 2);
        scene.write(BG0HOFS, 256);
        scene.color(1, 0x03E0);
        let out = scene.line(0);
        assert_eq!(out[0], 0x03E0);
        assert_eq!(out[8], 0);
    }

    #[test]
    fn the_lower_priority_value_is_in_front_then_the_lower_bg() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | BG0 | BG1);
        tile_4bpp(&mut scene, 0, 1, [1; 64]);
        tile_4bpp(&mut scene, 0, 2, [2; 64]);
        scene.write(BG0CNT, 1 | 1 << 8);
        scene.write(BG1CNT, 2 << 8);
        scene.vram.put_bg16(0x800, 1);
        scene.vram.put_bg16(0x1000, 2);
        scene.color(1, 0x0001);
        scene.color(2, 0x0002);
        assert_eq!(scene.line(0)[0], 0x0002);
        scene.write(BG1CNT, 1 | 2 << 8);
        assert_eq!(scene.line(0)[0], 0x0001);
    }

    #[test]
    fn mosaic_repeats_pixels_and_rows() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | BG0);
        scene.write(BG0CNT, 1 << 6);
        scene.write(MOSAIC, 3 | 3 << 4);
        let pixels: [u8; 64] = std::array::from_fn(|i| ((i % 8 + i / 8) % 15 + 1) as u8);
        tile_4bpp(&mut scene, 0, 1, pixels);
        scene.vram.put_bg16(0, 1);
        for i in 1..16 {
            scene.color(i, i as u16);
        }
        let out = scene.line(2);
        assert_eq!(out[..8], [1, 1, 1, 1, 5, 5, 5, 5]);
        let out = scene.line(5);
        assert_eq!(out[..4], [5, 5, 5, 5]);
        assert_eq!(out.len(), WIDTH);
    }

    #[test]
    fn whole_map_rows_draw_the_same_lines_as_entry_reads() {
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
        let tiled = Tiled(&scene.vram);
        let entries = Memory {
            vram: &scene.vram,
            palette: &scene.palette,
            oam: &scene.oam,
            three_d: None,
        };
        let rows = Memory {
            vram: &tiled,
            palette: &scene.palette,
            oam: &scene.oam,
            three_d: None,
        };
        for round in 0..2000 {
            let background = Background {
                control: next() as u16,
                hofs: next() as u16 & SCROLL_MASK,
                vofs: next() as u16 & SCROLL_MASK,
            };
            let source = Source {
                char_base: next() % 8 * 0x1_0000,
                screen_base: next() % 8 * 0x1_0000,
                extended: round % 2 == 0,
            };
            let bg = round / 2 % 4;
            let y = next() as usize % 192;
            let mut by_entry = [1; WIDTH];
            let mut by_row = [2; WIDTH];
            background.draw_text(bg, y, &source, &entries, &mut by_entry);
            background.draw_text(bg, y, &source, &rows, &mut by_row);
            assert_eq!(by_entry, by_row, "round {round}");
        }
    }
}
