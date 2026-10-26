use crate::gb::ppu::Ppu;
use crate::gb::ppu::fifo::FifoPixel;
use crate::gb::ppu::types::attributes::BgAttributes;
use crate::gb::ppu::types::sprite::SpriteFlags;
use crate::gb::ppu::types::tile::TileLine;

/// Responsible for loading data into the pixel FIFO
/// Continuously active throughout mode 3 (Drawing)
#[derive(Debug, Default, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct PixelFetcher {
    pub state: PixelFetcherState,
    pub tile_map_address: u16,
    pub tile_id: u8,
    pub tile_attributes: BgAttributes,
    pub tile_line: TileLine,
    pub tile_y: u8,
    pub tile_data_bank: u8,
    pub tile_data_address: u16,
    pub window_mode: bool,
    /// Whether WY condition has been met this frame
    pub wy_triggered: bool,
    /// Window line
    pub wl: u8,
    pub window_x: u8,
    pub object: ObjectFetch,
    pub last_tileset: bool,
    pub sel_glitch_data: u8,
}

#[derive(Debug, Default, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ObjectFetch {
    pub step: ObjectStep,
    pub oam_index: u8,
    pub y: u8,
    pub tile_id: u8,
    pub flags: u8,
    pub tile_line: TileLine,
}

#[derive(Debug, Default, Copy, Clone, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ObjectStep {
    #[default]
    Idle,
    WaitForBackground,
    ReadAttributes,
    ReadAttributesWait,
    ReadLow,
    ReadLowWait,
    ReadHigh,
    Overlay,
}

impl PixelFetcher {
    pub fn reset_scanline(&mut self) {
        self.state = PixelFetcherState::GetTile1;
        self.window_mode = false;
        self.object = ObjectFetch::default();
    }

    pub fn reset_frame(&mut self) {
        self.reset_scanline();
        self.wy_triggered = false;
        self.wl = 0xFF;
    }
}

/// What the pixel fetcher's task is during the next dot
#[derive(Debug, Default, Copy, Clone, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PixelFetcherState {
    #[default]
    GetTile1,
    GetTile2,
    GetTileDataLow1,
    GetTileDataLow2,
    GetTileDataHigh1,
    GetTileDataHigh2,
    Push,
}

impl Ppu {
    pub fn advance_fetcher(&mut self) {
        match self.fetcher.state {
            PixelFetcherState::GetTile1 => {
                if !self.lcdc.window_enable {
                    self.fetcher.window_mode = false;
                }
                let window = self.fetcher.window_mode;
                let map =
                    if (window && self.lcdc.window_tilemap) || (!window && self.lcdc.bg_tilemap) {
                        0x9C00
                    } else {
                        0x9800
                    };
                let y = self.bg_fetcher_y();
                let x = if window {
                    self.fetcher.window_x
                } else if self.fifo.position.wrapping_add(16) < 8 {
                    self.scx >> 3
                } else {
                    let cgb_shift = (self.model.is_cgb() && !self.fetching_object()) as u8;
                    (self
                        .scx
                        .wrapping_add(self.fifo.position)
                        .wrapping_add(8)
                        .wrapping_sub(cgb_shift)
                        >> 3)
                        & 0x1F
                };
                self.fetcher.tile_y = y;
                self.fetcher.tile_map_address = map + x as u16 + (y as u16 / 8) * 32;
                self.fetcher.state = PixelFetcherState::GetTile2;
            }
            PixelFetcherState::GetTile2 => {
                let addr = self.fetcher.tile_map_address;
                self.fetcher.tile_id = self.vram_read(0, addr);
                self.fetcher.tile_attributes = if self.cgb_mode {
                    self.vram_read(1, addr).into()
                } else {
                    BgAttributes::default()
                };
                self.fetcher.state = PixelFetcherState::GetTileDataLow1;
            }
            PixelFetcherState::GetTileDataLow1 => {
                self.latch_bg_tile_line_address(0);
                self.fetcher.state = PixelFetcherState::GetTileDataLow2;
            }
            PixelFetcherState::GetTileDataLow2 => {
                self.fetcher.tile_line.low = self.read_tile_data(false);
                self.fetcher.state = PixelFetcherState::GetTileDataHigh1;
            }
            PixelFetcherState::GetTileDataHigh1 => {
                self.latch_bg_tile_line_address(1);
                self.fetcher.state = PixelFetcherState::GetTileDataHigh2;
            }
            PixelFetcherState::GetTileDataHigh2 => {
                self.fetcher.tile_line.high = self.read_tile_data(true);
                if self.fetcher.window_mode {
                    self.fetcher.window_x = (self.fetcher.window_x + 1) & 0x1F;
                }
                self.push_bg_row();
            }
            PixelFetcherState::Push => self.push_bg_row(),
        }
    }

    fn push_bg_row(&mut self) {
        self.fetcher.state = PixelFetcherState::Push;
        if !self.fifo.bg.is_empty() {
            return;
        }
        if self.window_pixel_glitch() {
            self.fifo.bg.fill_one(FifoPixel::default());
            return;
        }
        let attributes = self.fetcher.tile_attributes;
        let line = self.fetcher.tile_line;
        self.fifo.bg.fill(std::array::from_fn(|i| {
            let i = if attributes.x_flip { 7 - i } else { i };
            FifoPixel {
                color_index: line.color_index(i as u8),
                palette: attributes.palette,
                sprite_priority: 0,
                obj_bg_priority: attributes.bg_priority,
            }
        }));
        self.fetcher.state = PixelFetcherState::GetTile1;
    }

    fn window_pixel_glitch(&self) -> bool {
        if !self.fetcher.wy_triggered
            || self.lcdc.window_enable
            || self.model.is_cgb()
            || self.fifo.window_glitch_disabled
        {
            return false;
        }
        let position = self.fifo.position.wrapping_add(7);
        self.wx == if position > 167 { 0 } else { position }
    }

    fn bg_fetcher_y(&self) -> u8 {
        if self.fetcher.window_mode {
            self.fetcher.wl
        } else {
            self.line.wrapping_add(self.scy)
        }
    }

    fn latch_bg_tile_line_address(&mut self, offset: u16) {
        let y = if self.model.is_cgb() {
            self.fetcher.tile_y
        } else {
            self.bg_fetcher_y()
        };
        let attributes = self.fetcher.tile_attributes;
        let y = if attributes.y_flip {
            7 - (y & 7)
        } else {
            y & 7
        };
        self.fetcher.last_tileset = self.lcdc.bg_window_tiles;
        self.fetcher.tile_data_bank = attributes.bank as u8;
        self.fetcher.tile_data_address =
            self.lcdc.bg_win_tile_line_address(self.fetcher.tile_id, y) + offset;
    }

    fn read_tile_data(&mut self, high: bool) -> u8 {
        let bank = self.fetcher.tile_data_bank;
        let addr = self.fetcher.tile_data_address;
        if !self.tile_sel_glitch {
            let value = self.vram_read(bank, addr);
            if high {
                self.fetcher.sel_glitch_data = value;
            }
            return value;
        }
        if !self.fetcher.last_tileset {
            return self.fetcher.sel_glitch_data;
        }
        let value = if self.fetcher.tile_id & 0x80 == 0 {
            self.fetcher.tile_id
        } else {
            self.vram_read(bank, addr)
        };
        self.fetcher.sel_glitch_data = self.vram_read(bank, addr);
        value
    }

    fn vram_read(&self, bank: u8, addr: u16) -> u8 {
        self.vram[(bank & 1) as usize][(addr & 0x1FFF) as usize]
    }

    pub fn fetching_object(&self) -> bool {
        self.fetcher.object.step != ObjectStep::Idle
    }

    pub fn start_object_fetch(&mut self) -> bool {
        while let Some(object) = self.scanner.peek() {
            if object.x < self.fifo.object_match_x() {
                self.scanner.advance();
            } else {
                break;
            }
        }
        let Some(&object) = self.scanner.peek() else {
            return false;
        };
        if !self.objects_fetchable() || object.x != self.fifo.object_match_x() {
            return false;
        }
        self.scanner.advance();
        self.fetcher.object = ObjectFetch {
            step: ObjectStep::WaitForBackground,
            oam_index: object.oam_index,
            y: object.y,
            ..ObjectFetch::default()
        };
        true
    }

    pub fn abort_object_fetch(&mut self) {
        let step = self.fetcher.object.step;
        if step != ObjectStep::Idle && step < ObjectStep::ReadHigh {
            self.fetcher.object.step = ObjectStep::Idle;
            self.scanner.retreat();
        }
    }

    pub fn advance_object_fetch(&mut self) {
        match self.fetcher.object.step {
            ObjectStep::Idle => {}
            ObjectStep::WaitForBackground => {
                let waiting = self.fetcher.state < PixelFetcherState::GetTileDataHigh2
                    || self.fifo.bg.is_empty();
                self.advance_fetcher();
                if !waiting {
                    self.fetcher.object.step = ObjectStep::ReadAttributes;
                }
            }
            ObjectStep::ReadAttributes => {
                self.advance_fetcher();
                let i = self.fetcher.object.oam_index as usize * 4;
                self.fetcher.object.tile_id = self.oam[i + 2];
                self.fetcher.object.flags = self.oam[i + 3];
                self.fetcher.object.step = ObjectStep::ReadAttributesWait;
            }
            ObjectStep::ReadAttributesWait => self.fetcher.object.step = ObjectStep::ReadLow,
            ObjectStep::ReadLow => {
                let (bank, addr) = self.object_line_address();
                self.fetcher.object.tile_line.low = self.vram_read(bank, addr);
                self.fetcher.object.step = ObjectStep::ReadLowWait;
            }
            ObjectStep::ReadLowWait => self.fetcher.object.step = ObjectStep::ReadHigh,
            ObjectStep::ReadHigh => {
                let (bank, addr) = self.object_line_address();
                self.fetcher.object.tile_line.high = self.vram_read(bank, addr + 1);
                self.fetcher.sel_glitch_data = self.fetcher.object.tile_line.high;
                self.fetcher.object.step = ObjectStep::Overlay;
            }
            ObjectStep::Overlay => {}
        }
    }

    pub fn overlay_object(&mut self) {
        let object = self.fetcher.object;
        let flags = SpriteFlags::from(object.flags);
        let palette = if self.cgb_mode {
            flags.cgb_palette
        } else {
            flags.dmg_palette as u8
        };
        let pixels = std::array::from_fn(|i| {
            let i = if flags.x_flip { 7 - i } else { i };
            FifoPixel {
                color_index: object.tile_line.color_index(i as u8),
                palette,
                sprite_priority: object.oam_index,
                obj_bg_priority: flags.obj_bg_priority,
            }
        });
        let oam_priority = self.model.is_cgb() && self.opri & 1 == 0;
        self.fifo.sprite.overlay(pixels, oam_priority);
        self.fetcher.object.step = ObjectStep::Idle;
    }

    fn object_line_address(&self) -> (u8, u16) {
        let object = &self.fetcher.object;
        let flags = SpriteFlags::from(object.flags);
        let height = self.lcdc.sprite_height();
        let mut row = self.line.wrapping_add(16).wrapping_sub(object.y) & (height - 1);
        if flags.y_flip {
            row = height - 1 - row;
        }
        let tile_id = if height == 16 {
            (object.tile_id & 0xFE) | (row >> 3)
        } else {
            object.tile_id
        };
        (
            (self.cgb_mode && flags.bank) as u8,
            self.lcdc.obj_tile_line_address(tile_id, row & 7),
        )
    }
}
