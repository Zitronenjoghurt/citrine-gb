/// Source: https://gbdev.io/pandocs/LCDC.html#lcd-control
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LCDC {
    /// Controls whether the LCD is on and the PPU is active.
    pub lcd_enabled: bool,
    /// Controls which background map the WINDOW uses for rendering. When it’s clear (0), the $9800 tilemap is used, otherwise it’s the $9C00 one.
    pub window_tilemap: bool,
    /// Controls whether the window shall be displayed or not. This bit is overridden on DMG by bit 0 (bg_window_enable) if that bit is clear.
    pub window_enable: bool,
    /// Controls which addressing mode the BG and Window use to pick tiles.
    /// https://gbdev.io/pandocs/Tile_Data.html#vram-tile-data
    pub bg_window_tiles: bool,
    /// Controls which background map the BACKGROUND uses for rendering. When it’s clear (0), the $9800 tilemap is used, otherwise it’s the $9C00 one.
    pub bg_tilemap: bool,
    /// Controls the size of all objects (1 tile or 2 stacked vertically)
    pub obj_size: bool,
    /// Controls whether objects are displayed or not.
    pub obj_enable: bool,
    /// Controls whether the background and window shall be displayed or not. If false, it overrides bit 5 (window_enable).
    pub bg_window_enable: bool,
}

impl LCDC {
    // ToDo: x-flip, y-flip
    pub fn bg_win_tile_line_address(&self, tile_id: u8, y: u8) -> u16 {
        if self.bg_window_tiles {
            self.obj_tile_line_address(tile_id, y)
        } else {
            // Line within the tile (0-7), same as before
            let tile_line = (y % 8) as u16;

            // tile_id is reinterpreted as a signed byte (-128 to 127)
            // so tile 0x00 = 0, 0x7F = 127, 0x80 = -128, 0xFF = -1
            // multiplied by 16 (tile size) to get the byte offset from 0x9000
            let offset = (tile_id as i8) as i32 * 16;

            // 0x9000 is the base address for this addressing mode
            // tile ID 0  => 0x9000 + 0 = 0x9000
            // tile ID 127 => 0x9000 + 2032 = 0x97F0 (top of range)
            // tile ID -1 => 0x9000 - 16 = 0x8FF0
            // tile ID -128 => 0x9000 - 2048 = 0x8800 (bottom of range)
            (0x9000_i32 + offset) as u16 + tile_line * 2
        }
    }

    // ToDo: x-flip, y-flip
    pub fn obj_tile_line_address(&self, tile_id: u8, y: u8) -> u16 {
        // Line within the tile => tiles are 8x8
        let tile_line = (y % 8) as u16;

        // At 2 bytes per row (2 bit per pixel) one tile is 16 bytes long
        // To get the address of the line within a tile we have to skip tile_line * 2 bytes
        0x8000 + (tile_id as u16) * 16 + tile_line * 2
    }

    pub fn bg_tile_id_address(&self, tile_x: u8, tile_y: u8) -> u16 {
        // Each tilemap is 32x32 tiles (5 bit), there are 2 addressing modes
        let index = (tile_x & 0b11111) as u16 + ((tile_y & 0b11111) as u16) * 32;
        if !self.bg_tilemap {
            0x9800 + index
        } else {
            0x9C00 + index
        }
    }

    pub fn window_tile_id_address(&self, tile_x: u8, tile_y: u8) -> u16 {
        let index = (tile_x & 0b11111) as u16 + ((tile_y & 0b11111) as u16) * 32;
        if !self.window_tilemap {
            0x9800 + index
        } else {
            0x9C00 + index
        }
    }

    pub fn do_render_bg(&self) -> bool {
        self.bg_window_enable
    }

    pub fn do_render_window(&self) -> bool {
        self.bg_window_enable && self.window_enable
    }
}

impl From<u8> for LCDC {
    fn from(value: u8) -> Self {
        Self {
            lcd_enabled: (value & 0b1000_0000) != 0,
            window_tilemap: (value & 0b0100_0000) != 0,
            window_enable: ((value & 0b0010_0000) != 0),
            bg_window_tiles: (value & 0b0001_0000) != 0,
            bg_tilemap: (value & 0b0000_1000) != 0,
            obj_size: (value & 0b0000_0100) != 0,
            obj_enable: (value & 0b0000_0010) != 0,
            bg_window_enable: (value & 0b0000_0001) != 0,
        }
    }
}

impl From<LCDC> for u8 {
    fn from(value: LCDC) -> Self {
        (value.lcd_enabled as u8) << 7
            | (value.window_tilemap as u8) << 6
            | (value.window_enable as u8) << 5
            | (value.bg_window_tiles as u8) << 4
            | (value.bg_tilemap as u8) << 3
            | (value.obj_size as u8) << 2
            | (value.obj_enable as u8) << 1
            | (value.bg_window_enable as u8)
    }
}
