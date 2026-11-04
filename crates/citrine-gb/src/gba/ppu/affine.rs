use super::{OPAQUE, WIDTH, read16};

const ONE: i16 = 0x100;
const CHAR_BLOCK: usize = 0x4000;
const SCREEN_BLOCK: usize = 0x800;
const BG_VRAM: usize = 0x1_0000;
const WRAP: u16 = 1 << 13;
const TILE_8BPP: usize = 64;
const FRAME_SELECT: u16 = 1 << 4;
const SECOND_FRAME: usize = 0xA000;
const FULL: (i32, i32) = (240, 160);
const SMALL: (i32, i32) = (160, 128);

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Affine {
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

    #[cfg(test)]
    pub fn matrix(&self) -> [i16; 4] {
        [self.pa, self.pb, self.pc, self.pd]
    }

    pub fn latch(&mut self) {
        self.line_x = signed(self.x);
        self.line_y = signed(self.y);
    }

    pub fn step(&mut self) {
        self.line_x = self.line_x.wrapping_add(i32::from(self.pb));
        self.line_y = self.line_y.wrapping_add(i32::from(self.pd));
    }

    fn start(&self, back: usize) -> (i32, i32) {
        let back = back as i32;
        (
            self.line_x.wrapping_sub(back * i32::from(self.pb)),
            self.line_y.wrapping_sub(back * i32::from(self.pd)),
        )
    }

    pub fn draw_tiled(
        &self,
        control: u16,
        back: usize,
        vram: &[u8],
        palette: &[u8],
        out: &mut [u16; WIDTH],
    ) {
        let size = 128i32 << (control >> 14);
        let tiles_per_row = (size >> 3) as usize;
        let wrap = control & WRAP != 0;
        let char_base = usize::from(control >> 2 & 0b11) * CHAR_BLOCK;
        let screen_base = usize::from(control >> 8 & 0x1F) * SCREEN_BLOCK;
        let (mut x, mut y) = self.start(back);
        let (dx, dy) = (i32::from(self.pa), i32::from(self.pc));
        for pixel in out.iter_mut() {
            let (mut px, mut py) = (x >> 8, y >> 8);
            x = x.wrapping_add(dx);
            y = y.wrapping_add(dy);
            if wrap {
                px &= size - 1;
                py &= size - 1;
            } else if px < 0 || py < 0 || px >= size || py >= size {
                *pixel = 0;
                continue;
            }
            let (px, py) = (px as usize, py as usize);
            let entry = (screen_base + (py >> 3) * tiles_per_row + (px >> 3)) % BG_VRAM;
            let at = char_base + usize::from(vram[entry]) * TILE_8BPP + (py & 7) * 8 + (px & 7);
            let index = if at < BG_VRAM {
                usize::from(vram[at])
            } else {
                0
            };
            *pixel = if index == 0 {
                0
            } else {
                read16(palette, index * 2) | OPAQUE
            };
        }
    }

    pub fn draw_bitmap(
        &self,
        dispcnt: u16,
        back: usize,
        vram: &[u8],
        palette: &[u8],
        out: &mut [u16; WIDTH],
    ) {
        let mode = dispcnt & 0b111;
        let base = if mode != 3 && dispcnt & FRAME_SELECT != 0 {
            SECOND_FRAME
        } else {
            0
        };
        let (width, height) = if mode == 5 { SMALL } else { FULL };
        if self.pa == ONE && self.pc == 0 {
            let (x, y) = self.start(back);
            draw_bitmap_row(
                mode,
                base,
                (x >> 8, y >> 8),
                (width, height),
                vram,
                palette,
                out,
            );
            return;
        }
        let (mut x, mut y) = self.start(back);
        let (dx, dy) = (i32::from(self.pa), i32::from(self.pc));
        for pixel in out.iter_mut() {
            let (px, py) = (x >> 8, y >> 8);
            x = x.wrapping_add(dx);
            y = y.wrapping_add(dy);
            if px < 0 || py < 0 || px >= width || py >= height {
                *pixel = 0;
                continue;
            }
            let at = (py * width + px) as usize;
            *pixel = if mode == 4 {
                let index = usize::from(vram[base + at]);
                if index == 0 {
                    0
                } else {
                    read16(palette, index * 2) | OPAQUE
                }
            } else {
                read16(vram, base + at * 2) | OPAQUE
            };
        }
    }
}

fn draw_bitmap_row(
    mode: u16,
    base: usize,
    (left, row): (i32, i32),
    (width, height): (i32, i32),
    vram: &[u8],
    palette: &[u8],
    out: &mut [u16; WIDTH],
) {
    if row < 0 || row >= height {
        out.fill(0);
        return;
    }
    let start = left.clamp(0, width);
    let end = (left + WIDTH as i32).clamp(0, width);
    out.fill(0);
    if start >= end {
        return;
    }
    let pixels = &mut out[(start - left) as usize..(end - left) as usize];
    let first = (row * width + start) as usize;
    if mode == 4 {
        let indices = &vram[base + first..base + first + pixels.len()];
        for (pixel, &index) in pixels.iter_mut().zip(indices) {
            *pixel = if index == 0 {
                0
            } else {
                read16(palette, usize::from(index) * 2) | OPAQUE
            };
        }
    } else {
        let colors = &vram[base + first * 2..base + (first + pixels.len()) * 2];
        for (pixel, color) in pixels.iter_mut().zip(colors.as_chunks::<2>().0) {
            *pixel = u16::from_le_bytes(*color) | OPAQUE;
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
mod tests;
