use crate::gb::ppu::color::RGBA;
use crate::gb::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};

const FB_PIXELS: usize = SCREEN_WIDTH * SCREEN_HEIGHT;
const FB_SIZE: usize = FB_PIXELS * 4;

#[derive(Debug)]
pub struct Framebuffer(Box<[u8; FB_SIZE]>);

impl Default for Framebuffer {
    fn default() -> Self {
        Self(vec![0u8; FB_SIZE].into_boxed_slice().try_into().unwrap())
    }
}

impl Framebuffer {
    pub fn new() -> Self {
        Self::test_pattern()
    }

    pub fn clear_with_test_pattern(&mut self) {
        *self = Self::test_pattern();
    }

    pub fn test_pattern() -> Self {
        let mut fb = Self::default();
        for y in 0..144 {
            for x in 0..160 {
                let tile_x = x / 4;
                let tile_y = y / 4;
                let is_yellow = (tile_x + tile_y) % 2 == 0;

                let px_check = (x + y) % 2;
                let pair_check = (x / 2 + y / 2) % 2;

                let idx = (y * 160 + x) * 4;
                if is_yellow {
                    let base: u8 = 0x60 + (pair_check as u8) * 0x20;
                    let v = base + (px_check as u8) * 0x10;
                    fb.0[idx] = v;
                    fb.0[idx + 1] = v;
                    fb.0[idx + 2] = 0x04;
                } else {
                    let base: u8 = 0x15 + (pair_check as u8) * 0x12;
                    let v = base + (px_check as u8) * 0x08;
                    fb.0[idx] = v;
                    fb.0[idx + 1] = v;
                    fb.0[idx + 2] = v + 0x08;
                }
                fb.0[idx + 3] = 0xFF;
            }
        }
        fb
    }

    pub fn set(&mut self, index: usize, color: RGBA) {
        if index >= 23040 {
            return;
        }
        self.0[index * 4] = color.r();
        self.0[index * 4 + 1] = color.g();
        self.0[index * 4 + 2] = color.b();
        self.0[index * 4 + 3] = color.a();
    }

    pub fn set_xy(&mut self, x: usize, y: usize, color: RGBA) {
        self.set(y * 160 + x, color);
    }

    pub fn clear(&mut self) {
        self.0.fill(0);
    }

    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}
