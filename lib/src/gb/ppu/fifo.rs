//! Source: https://gbdev.io/pandocs/pixel_fifo.html and https://ashiepaws.github.io/GBEDG/ppu/

use crate::gb::ppu::types::color::RGBA;
use crate::gb::ppu::Ppu;
use std::collections::VecDeque;

#[derive(Debug, Default, Copy, Clone)]
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

#[derive(Debug, Default)]
pub struct PixelFifo {
    // Separated queues, but mixed when popping items
    // Can hold up to 8 pixels each
    // Pixel fetcher works to keep it at least 8 pixels => thats required for pixel rendering to work
    // The queues are only manipulated while drawing (mode 3)
    bg: VecDeque<FifoPixel>,
    sprite: VecDeque<FifoPixel>,
    pub lcd_x: u8,
    pub scx_discard: u8,
}

impl PixelFifo {
    pub fn bg_empty(&self) -> bool {
        self.bg.is_empty()
    }

    pub fn sprite_empty(&self) -> bool {
        self.sprite.is_empty()
    }

    pub fn push_bg(&mut self, pixels: [FifoPixel; 8]) {
        for pixel in pixels {
            self.bg.push_back(pixel);
        }
    }

    pub fn pop_bg(&mut self) -> Option<FifoPixel> {
        self.bg.pop_front()
    }

    pub fn reset_bg(&mut self) {
        self.bg.clear();
    }

    pub fn push_sprite(&mut self, pixels: [FifoPixel; 8], offset: u8) {
        while self.sprite.len() < 8 {
            self.sprite.push_back(FifoPixel::default());
        }

        for i in offset..8 {
            let queue_index = (i - offset) as usize;
            let new_pixel = pixels[i as usize];

            let existing = &mut self.sprite[queue_index];

            // ToDo: Check if this is correct for CGB too
            if existing.color_index == 0 && new_pixel.color_index != 0 {
                *existing = new_pixel;
            }
        }
    }

    pub fn pop_sprite(&mut self) -> Option<FifoPixel> {
        self.sprite.pop_front()
    }

    pub fn start_scanline(&mut self, scx: u8) {
        self.bg.clear();
        self.sprite.clear();
        self.lcd_x = 0;
        self.scx_discard = scx % 8;
    }
}

impl Ppu {
    // Returns true if the FIFO is done
    pub fn dot_fifo(&mut self) -> bool {
        // ToDo: If sprite pixel but no bg pixel => sprite pixel is 'merged' with current bg pixel? (GBEDG)
        if self.fetcher.sprite_mode.is_some() {
            return false;
        }

        if let Some(bg) = self.fifo.pop_bg() {
            if self.fifo.scx_discard > 0 {
                self.fifo.scx_discard -= 1;
            } else {
                let sprite = self.fifo.pop_sprite();

                let bg_color_index = if self.lcdc.do_render_bg() {
                    bg.color_index
                } else {
                    0
                };

                let color = if let Some(sprite) = sprite {
                    if sprite.color_index == 0
                        || ((bg.obj_bg_priority || sprite.obj_bg_priority) && bg.color_index != 0)
                    {
                        self.apply_bg_palette(bg.palette, bg_color_index)
                    } else {
                        self.apply_sprite_palette(sprite.palette, sprite.color_index)
                    }
                } else {
                    self.apply_bg_palette(bg.palette, bg_color_index)
                };

                self.frame
                    .set_xy(self.fifo.lcd_x as usize, self.ly as usize, color);
                self.fifo.lcd_x += 1;
            }
        };

        self.fifo.lcd_x == 160
    }

    // ToDo: CGB color palette
    fn apply_bg_palette(&self, _palette: u8, color_index: u8) -> RGBA {
        let shade = (self.bgp >> (color_index * 2)) & 0x03;
        self.dmg_theme.color_from_shade(shade)
    }

    // ToDo: CGB color palette
    fn apply_sprite_palette(&self, palette: u8, color_index: u8) -> RGBA {
        let p = if palette & 1 == 1 {
            self.obp1
        } else {
            self.obp0
        };
        let shade = (p >> (color_index * 2)) & 0x03;
        self.dmg_theme.color_from_shade(shade)
    }
}
