#[derive(Debug, Default, Copy, Clone)]
pub struct Sprite {
    /// Horizontal position + 8
    pub y: u8,
    pub x: u8,
    pub tile_id: u8,
    pub flags: SpriteFlags,
    pub oam_index: u8,
}

impl From<[u8; 4]> for Sprite {
    fn from(value: [u8; 4]) -> Self {
        Self {
            y: value[0],
            x: value[1],
            tile_id: value[2],
            flags: value[3].into(),
            oam_index: 0,
        }
    }
}

#[derive(Debug, Default, Copy, Clone)]
pub struct SpriteFlags {
    /// 0 = Sprite is always rendered above background
    /// 1 = Background colors 1-3 overlay sprite, sprite is still rendered above color 0
    pub obj_bg_priority: bool,
    /// 1 = Sprite flipped vertically
    pub y_flip: bool,
    /// 1 = Sprite flipped horizontally
    pub x_flip: bool,
    /// Only used in DMG mode
    /// 0 = OBP0 register used as palette
    /// 1 = 0BP1 register used as palette
    pub dmg_palette: bool,
    /// Only used in CGB mode
    /// 0 = Fetch tile from VRAM bank 0
    /// 1 = Fetch tile from VRAM bank 1
    pub bank: bool,
    /// 3 bits => CGB palette number (0-7)
    pub cgb_palette: u8,
}

impl From<u8> for SpriteFlags {
    fn from(value: u8) -> Self {
        Self {
            obj_bg_priority: (value & 0b1000_0000) != 0,
            y_flip: (value & 0b0100_0000) != 0,
            x_flip: (value & 0b0010_0000) != 0,
            dmg_palette: (value & 0b0001_0000) != 0,
            bank: (value & 0b0000_1000) != 0,
            cgb_palette: (value & 0b0000_0111),
        }
    }
}

impl From<SpriteFlags> for u8 {
    fn from(value: SpriteFlags) -> Self {
        ((value.obj_bg_priority as u8) << 7)
            | ((value.y_flip as u8) << 6)
            | ((value.x_flip as u8) << 5)
            | ((value.dmg_palette as u8) << 4)
            | ((value.bank as u8) << 3)
            | (value.cgb_palette & 0b111)
    }
}
