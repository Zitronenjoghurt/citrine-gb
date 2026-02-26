//! Source: https://gbdev.io/pandocs/pixel_fifo.html and https://ashiepaws.github.io/GBEDG/ppu/

use crate::gb::ppu::color::RGBA;
use crate::gb::ppu::Ppu;
use std::collections::VecDeque;

/// Using the Game Boy Pocket color scheme
/// https://en.wikipedia.org/wiki/List_of_video_game_console_palettes
const COLOR_SCHEME: [[u8; 4]; 4] = [
    [0xC5, 0xCA, 0xA4, 0xFF],
    [0x8C, 0x92, 0x6B, 0xFF],
    [0x4A, 0x51, 0x38, 0xFF],
    [0x18, 0x18, 0x18, 0xFF],
];

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
    lcd_x: u8,
    scx_discard: u8,
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

    pub fn start_scanline(&mut self, scx: u8) {
        self.bg.clear();
        self.lcd_x = 0;
        self.scx_discard = scx % 8;
    }
}

impl Ppu {
    // Returns true if the FIFO is empty (done)
    pub fn dot_fifo(&mut self) -> bool {
        // ToDo: If sprite pixel but no bg pixel => sprite pixel is 'merged' with current bg pixel? (GBEDG) => revisit when implementing sprite rendering

        if let Some(bg) = self.fifo.pop_bg() {
            if self.fifo.scx_discard > 0 {
                self.fifo.scx_discard -= 1;
            } else {
                let color = self.apply_bg_palette(bg.color_index);
                self.frame
                    .set_xy(self.fifo.lcd_x as usize, self.ly as usize, color);
                self.fifo.lcd_x += 1;
            }
        };

        self.fifo.lcd_x == 160
    }

    // ToDo: CGB color palette
    fn apply_bg_palette(&self, color_index: u8) -> RGBA {
        let shade = ((self.bgp >> (color_index * 2)) & 0x03) as usize;
        RGBA::from(COLOR_SCHEME[shade])
    }
}
