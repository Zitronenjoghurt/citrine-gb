use crate::gb::ppu::types::color::RGBA;
use crate::gb::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};

const FB_PIXELS: usize = SCREEN_WIDTH * SCREEN_HEIGHT;
const FB_SIZE: usize = FB_PIXELS * 4;

#[derive(Debug, Clone)]
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

    pub fn set_span(&mut self, index: usize, colors: [RGBA; 8]) {
        let Some(span) = self.0.get_mut(index * 4..index * 4 + 32) else {
            return;
        };
        for (pixel, color) in span.as_chunks_mut::<4>().0.iter_mut().zip(colors) {
            *pixel = [color.r(), color.g(), color.b(), color.a()];
        }
    }

    pub fn set_run(&mut self, index: usize, colors: &[RGBA]) {
        let Some(run) = self.0.get_mut(index * 4..(index + colors.len()) * 4) else {
            return;
        };
        for (pixel, color) in run.as_chunks_mut::<4>().0.iter_mut().zip(colors) {
            *pixel = [color.r(), color.g(), color.b(), color.a()];
        }
    }

    #[inline(always)]
    pub fn put_span(&mut self, row: usize, x: u8, colors: [RGBA; 8]) {
        if x <= 152 {
            self.set_span(row + x as usize, colors);
            return;
        }
        for (i, color) in colors.into_iter().enumerate() {
            let x = x.wrapping_add(i as u8);
            if x < 160 {
                self.set(row + x as usize, color);
            }
        }
    }

    pub fn bytes_mut(&mut self, at: usize, len: usize) -> Option<&mut [u8]> {
        self.0.get_mut(at..at + len)
    }

    #[inline(always)]
    pub fn put_pairs(&mut self, row: usize, x: u8, pairs: [u64; 4]) {
        if x <= 152 {
            let at = (row + x as usize) * 4;
            if let Some(span) = self.0.get_mut(at..at + 32) {
                for (chunk, pair) in span.as_chunks_mut::<8>().0.iter_mut().zip(pairs) {
                    *chunk = pair.to_ne_bytes();
                }
            }
            return;
        }
        let colors = std::array::from_fn(|i| {
            let bytes = pairs[i / 2].to_ne_bytes();
            let at = i % 2 * 4;
            RGBA::new(bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3])
        });
        self.put_span(row, x, colors);
    }

    pub fn set_xy(&mut self, x: usize, y: usize, color: RGBA) {
        self.set(y * 160 + x, color);
    }

    pub fn get_xy(&self, x: usize, y: usize) -> RGBA {
        let i = (y * 160 + x) * 4;
        RGBA::new(self.0[i], self.0[i + 1], self.0[i + 2], self.0[i + 3])
    }

    pub fn clear(&mut self) {
        self.0.fill(0);
    }

    pub fn fill(&mut self, color: RGBA) {
        self.0
            .as_chunks_mut::<4>()
            .0
            .fill([color.r(), color.g(), color.b(), color.a()]);
    }

    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn copy_from_slice(&mut self, pixels: &[u8]) {
        self.0.copy_from_slice(pixels);
    }

    #[cfg(feature = "png")]
    pub fn render_png(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut encoder =
                png::Encoder::new(&mut buf, SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&self.0[..]).unwrap();
        }
        buf
    }
}
