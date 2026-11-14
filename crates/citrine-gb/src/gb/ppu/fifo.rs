//! Source: https://gbdev.io/pandocs/pixel_fifo.html and https://ashiepaws.github.io/GBEDG/ppu/

use crate::gb::ppu::Ppu;
use crate::gb::ppu::types::color::{DmgColors, RGBA};

#[derive(Debug, Default, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FifoPixel {
    /// A value between 0 and 3
    pub color_index: u8,
    /// On CGB a value between 0 and 7 and on DMG this only applies to objects
    pub palette: u8,
    /// On CGB this is the OAM index for the object and on DMG this doesn’t exist
    pub sprite_priority: u8,
    /// Holds the value of the OBJ-to-BG Priority bit
    pub obj_bg_priority: bool,
}

#[derive(Debug, Default, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PixelQueue {
    pixels: [FifoPixel; 8],
    head: u8,
    len: u8,
}

impl PixelQueue {
    pub fn len(&self) -> u8 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    pub fn fill(&mut self, pixels: [FifoPixel; 8]) {
        self.pixels = pixels;
        self.head = 0;
        self.len = 8;
    }

    pub fn fill_one(&mut self, pixel: FifoPixel) {
        self.pixels[0] = pixel;
        self.head = 0;
        self.len = 1;
    }

    pub fn take_full(&mut self) -> [FifoPixel; 8] {
        let head = self.head as usize;
        self.clear();
        if head == 0 {
            return self.pixels;
        }
        std::array::from_fn(|i| self.pixels[(head + i) & 7])
    }

    pub fn pop(&mut self) -> Option<FifoPixel> {
        if self.len == 0 {
            return None;
        }
        let pixel = self.pixels[self.head as usize];
        self.head = (self.head + 1) & 7;
        self.len -= 1;
        Some(pixel)
    }

    pub fn at(&self, index: u8) -> FifoPixel {
        debug_assert!(index < self.len);
        self.pixels[((self.head + index) & 7) as usize]
    }

    pub fn drop_front(&mut self, n: u8) {
        let n = n.min(self.len);
        self.head = (self.head + n) & 7;
        self.len -= n;
    }

    fn get_mut(&mut self, index: u8) -> &mut FifoPixel {
        &mut self.pixels[((self.head + index) & 7) as usize]
    }

    pub fn overlay(&mut self, indices: u64, object: FifoPixel, oam_priority: bool) {
        if self.len == 0 {
            for i in 0..8 {
                let color_index = (indices >> (i * 8)) as u8;
                *self.get_mut(i) = if color_index == 0 {
                    FifoPixel::default()
                } else {
                    FifoPixel {
                        color_index,
                        ..object
                    }
                };
            }
            self.len = 8;
            return;
        }
        while self.len < 8 {
            *self.get_mut(self.len) = FifoPixel::default();
            self.len += 1;
        }
        for i in 0..8 {
            let color_index = (indices >> (i * 8)) as u8;
            let existing = self.get_mut(i);
            let wins = existing.color_index == 0
                || (oam_priority && object.sprite_priority < existing.sprite_priority);
            if color_index != 0 && wins {
                *existing = FifoPixel {
                    color_index,
                    ..object
                };
            }
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct FifoStart {
    sprite: PixelQueue,
    window_fetching: bool,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct PixelFifo {
    pub bg: PixelQueue,
    pub sprite: PixelQueue,
    pub lcd_x: u8,
    pub position: u8,
    pub window_fetching: bool,
    pub insert_pixel: bool,
    pub window_glitch_disabled: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub bg_row_pending: bool,
}

pub const POSITION_START: u8 = (-16i8) as u8;

impl PixelFifo {
    pub fn start_scanline(&mut self) {
        self.bg.fill([FifoPixel::default(); 8]);
        self.bg_row_pending = false;
        self.sprite.clear();
        self.lcd_x = 0;
        self.insert_pixel = false;
        self.window_glitch_disabled = false;
    }

    pub fn line_start(&self) -> FifoStart {
        debug_assert!(
            self.bg.head == 0
                && self.bg.len == 8
                && self.bg.pixels.iter().all(|p| {
                    p.color_index == 0
                        && p.palette == 0
                        && p.sprite_priority == 0
                        && !p.obj_bg_priority
                })
                && self.lcd_x == 0
                && self.position == POSITION_START
                && !self.insert_pixel
                && !self.window_glitch_disabled
                && !self.bg_row_pending
        );
        FifoStart {
            sprite: self.sprite,
            window_fetching: self.window_fetching,
        }
    }

    pub fn restart_line(&mut self, start: FifoStart) {
        self.start_scanline();
        self.sprite = start.sprite;
        self.position = POSITION_START;
        self.window_fetching = start.window_fetching;
    }

    pub fn object_match_x(&self) -> u8 {
        let x = self.position.wrapping_add(8);
        if x > POSITION_START { 0 } else { x }
    }
}

impl Ppu {
    pub fn render_pixel(&mut self) {
        if self.fifo.bg.is_empty() {
            return;
        }
        let bg = if std::mem::take(&mut self.fifo.insert_pixel) {
            FifoPixel::default()
        } else {
            self.fifo.bg.pop().unwrap_or_default()
        };
        let sprite = self.fifo.sprite.pop();

        let position = self.fifo.position;
        if position.wrapping_add(16) < 8 {
            let fine = self.scx & 7;
            if position & 7 == fine || (self.fifo.window_fetching && position & 7 == 6 && fine == 7)
            {
                self.fifo.position = (-8i8) as u8;
            } else if position == (-9i8) as u8 {
                self.fifo.position = POSITION_START;
                return;
            }
        }
        self.fifo.window_fetching = false;

        if self.fifo.position >= 160 {
            self.fifo.position = self.fifo.position.wrapping_add(1);
            return;
        }

        self.push_lcd_pixel(bg, sprite);
    }

    pub fn render_span_pixel(&mut self) {
        let bg = self.fifo.bg.pop().unwrap_or_default();
        let sprite = self.fifo.sprite.pop();
        self.push_lcd_pixel(bg, sprite);
    }

    #[inline(always)]
    pub fn render_span_run(&mut self, n: u8) {
        let render_bg = self.cgb_mode || self.lcdc.do_render_bg();
        let bg_priority = !self.cgb_mode || self.lcdc.bg_window_enable;
        debug_assert!(n <= self.fifo.bg.len());
        let objects = n.min(self.fifo.sprite.len()) as usize;
        let mut colors = [RGBA::default(); 8];
        for (i, color) in colors[..objects].iter_mut().enumerate() {
            let bg = self.fifo.bg.at(i as u8);
            let sprite = self.fifo.sprite.at(i as u8);
            *color = self.pixel_color(bg, Some(sprite), render_bg, bg_priority);
        }
        for (i, color) in colors[..n as usize].iter_mut().enumerate().skip(objects) {
            let bg = self.fifo.bg.at(i as u8);
            let color_index = if render_bg { bg.color_index } else { 0 };
            *color = self.colors.bg(bg.palette, color_index);
        }
        self.fifo.bg.drop_front(n);
        self.fifo.sprite.drop_front(n);
        let n = n as usize;
        if !self.blind {
            let row = self.line as usize * 160;
            let x = self.fifo.lcd_x;
            if x as usize + n <= 160 {
                self.frame.set_run(row + x as usize, &colors[..n]);
            } else {
                for (i, &color) in colors[..n].iter().enumerate() {
                    let x = x.wrapping_add(i as u8);
                    if x < 160 {
                        self.frame.set(row + x as usize, color);
                    }
                }
            }
        }
        self.fifo.lcd_x = self.fifo.lcd_x.wrapping_add(n as u8);
        self.fifo.position = self.fifo.position.wrapping_add(n as u8);
    }

    pub fn plain_span(&mut self, render_bg: bool) {
        let pixels = self.fifo.bg.take_full();
        if !self.blind {
            let colors = pixels.map(|bg| {
                let color_index = if render_bg { bg.color_index } else { 0 };
                self.colors.bg(bg.palette, color_index)
            });
            self.put_span(colors);
        }
        self.skip_span();
    }

    #[inline(always)]
    fn put_span(&mut self, colors: [RGBA; 8]) {
        self.frame
            .put_span(self.line as usize * 160, self.fifo.lcd_x, colors);
    }

    #[inline(always)]
    fn skip_span(&mut self) {
        self.fifo.lcd_x = self.fifo.lcd_x.wrapping_add(8);
        self.fifo.position = self.fifo.position.wrapping_add(8);
    }

    #[inline(always)]
    fn push_lcd_pixel(&mut self, bg: FifoPixel, sprite: Option<FifoPixel>) {
        if self.blind {
            self.fifo.lcd_x = self.fifo.lcd_x.wrapping_add(1);
            self.fifo.position = self.fifo.position.wrapping_add(1);
            return;
        }

        let render_bg = self.cgb_mode || self.lcdc.do_render_bg();
        let bg_priority = !self.cgb_mode || self.lcdc.bg_window_enable;
        let color = self.pixel_color(bg, sprite, render_bg, bg_priority);

        if self.fifo.lcd_x < 160 {
            self.frame
                .set_xy(self.fifo.lcd_x as usize, self.line as usize, color);
        }
        self.fifo.lcd_x = self.fifo.lcd_x.wrapping_add(1);
        self.fifo.position = self.fifo.position.wrapping_add(1);
    }

    #[inline(always)]
    fn pixel_color(
        &self,
        bg: FifoPixel,
        sprite: Option<FifoPixel>,
        render_bg: bool,
        bg_priority: bool,
    ) -> RGBA {
        let bg_color_index = if render_bg { bg.color_index } else { 0 };
        let sprite = sprite.filter(|s| s.color_index != 0 && self.lcdc.obj_enable);
        match sprite {
            Some(sprite)
                if !(bg_priority
                    && (bg.obj_bg_priority || sprite.obj_bg_priority)
                    && bg_color_index != 0) =>
            {
                self.colors.obj(sprite.palette, sprite.color_index)
            }
            _ => self.colors.bg(bg.palette, bg_color_index),
        }
    }

    pub(crate) fn apply_bg_palette(&self, palette: u8, color_index: u8) -> RGBA {
        if self.cgb_mode {
            return self.cgb_color(&self.bg_palette_ram, palette, color_index);
        }
        let shade = (self.bgp >> (color_index * 2)) & 0x03;
        match self.dmg_colors {
            DmgColors::Theme => self.dmg_theme.color_from_shade(shade),
            DmgColors::PaletteRam => self.cgb_color(&self.bg_palette_ram, 0, shade),
        }
    }

    pub(crate) fn apply_sprite_palette(&self, palette: u8, color_index: u8) -> RGBA {
        if self.cgb_mode {
            return self.cgb_color(&self.obj_palette_ram, palette, color_index);
        }
        let p = if palette & 1 == 1 {
            self.obp1
        } else {
            self.obp0
        };
        let shade = (p >> (color_index * 2)) & 0x03;
        match self.dmg_colors {
            DmgColors::Theme => self.dmg_theme.color_from_shade(shade),
            DmgColors::PaletteRam => self.cgb_color(&self.obj_palette_ram, palette & 1, shade),
        }
    }

    pub(super) fn cgb_color(&self, palette_ram: &[u8; 64], palette: u8, color_index: u8) -> RGBA {
        let index = ((palette & 0x07) as usize * 4 + (color_index & 0x03) as usize) * 2;
        self.color_correction.apply(u16::from_le_bytes([
            palette_ram[index],
            palette_ram[index + 1],
        ]))
    }
}
