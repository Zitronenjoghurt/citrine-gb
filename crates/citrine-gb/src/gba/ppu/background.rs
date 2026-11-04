use super::{OPAQUE, WIDTH, read16};

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

    pub fn draw_text(&self, y: usize, vram: &[u8], palette: &[u8], out: &mut [u16; WIDTH]) {
        let (width, height) = SIZES[usize::from(self.control >> 14)];
        let char_base = usize::from(self.control >> 2 & 0b11) * CHAR_BLOCK;
        let screen_base = usize::from(self.control >> 8 & 0x1F) * SCREEN_BLOCK;
        let wide = self.control & COLORS_256 != 0;
        let sy = (y + usize::from(self.vofs)) & (height - 1);
        let ty = sy >> 3;
        let block_row = (ty >> 5) * (width >> 8);
        let row_base = screen_base + ((ty & 31) << 6);
        let mut sx = usize::from(self.hofs) & (width - 1);
        let mut x = 0;
        while x < WIDTH {
            let tx = sx >> 3;
            let block = block_row + (tx >> 5);
            let entry = read16(
                vram,
                (row_base + block * SCREEN_BLOCK + ((tx & 31) << 1)) % BG_VRAM,
            );
            let start = sx & 7;
            let count = (8 - start).min(WIDTH - x);
            let span = &mut out[x..x + count];
            let tile = usize::from(entry & 0x3FF);
            let row = if entry & (1 << 11) != 0 {
                7 - (sy & 7)
            } else {
                sy & 7
            };
            let hflip = entry & (1 << 10) != 0;
            if wide {
                let at = char_base + tile * TILE_8BPP + row * 8;
                draw_8bpp(vram, palette, at, start, hflip, span);
            } else {
                let at = char_base + tile * TILE_4BPP + row * 4;
                let bank = usize::from(entry >> 12) * 16;
                draw_4bpp(vram, palette, at, bank, start, hflip, span);
            }
            x += count;
            sx = (sx + count) & (width - 1);
        }
    }
}

fn draw_4bpp(
    vram: &[u8],
    palette: &[u8],
    at: usize,
    bank: usize,
    start: usize,
    hflip: bool,
    span: &mut [u16],
) {
    if at + 4 > BG_VRAM {
        span.fill(0);
        return;
    }
    let bits = u32::from_le_bytes([vram[at], vram[at + 1], vram[at + 2], vram[at + 3]]);
    for (i, pixel) in span.iter_mut().enumerate() {
        let column = if hflip { 7 - start - i } else { start + i };
        let index = (bits >> (column * 4) & 0xF) as usize;
        *pixel = if index == 0 {
            0
        } else {
            read16(palette, (bank + index) * 2) | OPAQUE
        };
    }
}

fn draw_8bpp(vram: &[u8], palette: &[u8], at: usize, start: usize, hflip: bool, span: &mut [u16]) {
    if at + 8 > BG_VRAM {
        span.fill(0);
        return;
    }
    for (i, pixel) in span.iter_mut().enumerate() {
        let column = if hflip { 7 - start - i } else { start + i };
        let index = usize::from(vram[at + column]);
        *pixel = if index == 0 {
            0
        } else {
            read16(palette, index * 2) | OPAQUE
        };
    }
}

#[cfg(test)]
mod tests;
