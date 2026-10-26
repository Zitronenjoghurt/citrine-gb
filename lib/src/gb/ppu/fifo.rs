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

    pub fn pop(&mut self) -> Option<FifoPixel> {
        if self.len == 0 {
            return None;
        }
        let pixel = self.pixels[self.head as usize];
        self.head = (self.head + 1) & 7;
        self.len -= 1;
        Some(pixel)
    }

    fn get_mut(&mut self, index: u8) -> &mut FifoPixel {
        &mut self.pixels[((self.head + index) & 7) as usize]
    }

    pub fn overlay(&mut self, pixels: [FifoPixel; 8], oam_priority: bool) {
        while self.len < 8 {
            *self.get_mut(self.len) = FifoPixel::default();
            self.len += 1;
        }
        for (i, new_pixel) in pixels.into_iter().enumerate() {
            let existing = self.get_mut(i as u8);
            let wins = existing.color_index == 0
                || (oam_priority && new_pixel.sprite_priority < existing.sprite_priority);
            if new_pixel.color_index != 0 && wins {
                *existing = new_pixel;
            }
        }
    }
}

#[derive(Debug, Default)]
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
}

pub const POSITION_START: u8 = (-16i8) as u8;

impl PixelFifo {
    pub fn start_scanline(&mut self) {
        self.bg.fill([FifoPixel::default(); 8]);
        self.sprite.clear();
        self.lcd_x = 0;
        self.insert_pixel = false;
        self.window_glitch_disabled = false;
    }

    pub fn object_match_x(&self) -> u8 {
        let x = self.position.wrapping_add(8);
        if x > POSITION_START { 0 } else { x }
    }
}

impl Ppu {
    pub fn render_pixel(&mut self) {
        if self.object_at_x0_pending() {
            return;
        }
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

        let bg_color_index = if self.cgb_mode || self.lcdc.do_render_bg() {
            bg.color_index
        } else {
            0
        };
        let bg_priority = !self.cgb_mode || self.lcdc.bg_window_enable;
        let sprite = sprite.filter(|s| s.color_index != 0 && self.lcdc.obj_enable);

        let color = match sprite {
            Some(sprite)
                if !(bg_priority
                    && (bg.obj_bg_priority || sprite.obj_bg_priority)
                    && bg_color_index != 0) =>
            {
                self.apply_sprite_palette(sprite.palette, sprite.color_index)
            }
            _ => self.apply_bg_palette(bg.palette, bg_color_index),
        };

        if self.fifo.lcd_x < 160 && self.line < 144 {
            self.frame
                .set_xy(self.fifo.lcd_x as usize, self.line as usize, color);
        }
        self.fifo.lcd_x = self.fifo.lcd_x.wrapping_add(1);
        self.fifo.position = self.fifo.position.wrapping_add(1);
    }

    fn apply_bg_palette(&self, palette: u8, color_index: u8) -> RGBA {
        if self.cgb_mode {
            return self.cgb_color(&self.bg_palette_ram, palette, color_index);
        }
        let shade = (self.bgp >> (color_index * 2)) & 0x03;
        match self.dmg_colors {
            DmgColors::Theme => self.dmg_theme.color_from_shade(shade),
            DmgColors::PaletteRam => self.cgb_color(&self.bg_palette_ram, 0, shade),
        }
    }

    fn apply_sprite_palette(&self, palette: u8, color_index: u8) -> RGBA {
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
