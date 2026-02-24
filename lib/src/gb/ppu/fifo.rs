//! Source: https://gbdev.io/pandocs/pixel_fifo.html

use std::collections::VecDeque;

#[derive(Debug, Default, Copy, Clone)]
pub struct FifoPixel {
    /// A value between 0 and 3
    pub shade: u8,
    /// On CGB a value between 0 and 7 and on DMG this only applies to objects
    pub palette: u8,
    /// On CGB this is the OAM index for the object and on DMG this doesn’t exist
    pub sprite_priority: u8,
    /// Holds the value of the OBJ-to-BG Priority bit
    pub background_priority: bool,
}

#[derive(Debug, Default)]
pub struct PixelFifo {
    // Separated queues, but mixed when popping items
    // Can hold up to 16 pixels
    // Pixel fetcher works to keep it at least 8 pixels => thats required for pixel rendering to work
    // The queues are only manipulated while drawing (mode 3)
    bg: VecDeque<FifoPixel>,
    obj: VecDeque<FifoPixel>,
}

impl PixelFifo {}
