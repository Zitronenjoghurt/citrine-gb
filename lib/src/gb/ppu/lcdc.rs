/// https://gbdev.io/pandocs/LCDC.html#lcd-control
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
    pub fn get_bg_tilemap_address(&self) -> u16 {
        if self.bg_tilemap { 0x9C00 } else { 0x9800 }
    }

    pub fn get_tile_address(&self, tile_x: u16, tile_y: u16) -> u16 {
        self.get_bg_tilemap_address() + tile_x + tile_y * 32
    }

    pub fn get_tile_line_data_address(&self, tile_id: u8, y_pos: u16) -> u16 {
        let tile_line = (y_pos % 8) * 2;

        let tile_data_offset = if self.bg_window_tiles {
            tile_id as u16 * 16
        } else {
            (((tile_id as i8) as i16) + 128) as u16 * 16
        };

        if self.bg_window_tiles {
            0x8000 + tile_data_offset + tile_line
        } else {
            0x8800 + tile_data_offset + tile_line
        }
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
