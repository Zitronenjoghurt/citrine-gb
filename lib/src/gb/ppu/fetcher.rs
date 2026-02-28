use crate::gb::ppu::fifo::FifoPixel;
use crate::gb::ppu::types::sprite::Sprite;
use crate::gb::ppu::types::tile::TileLine;
use crate::gb::ppu::Ppu;
use crate::ReadMemory;

// ToDo: Differentiate between bg/win pixel fetcher and sprite pixel fetcher
/// Responsible for loading data into the pixel FIFO
/// Continuously active throughout mode 3 (Drawing)
#[derive(Debug, Default, Copy, Clone)]
pub struct PixelFetcher {
    pub state: PixelFetcherState,
    pub x: u8,
    pub tile_id: u8,
    pub tile_line: TileLine,
    /// If in sprite mode this will be the sprite to fetch pixels for
    pub sprite_mode: Option<Sprite>,
    pub window_mode: bool,
    /// Whether WY condition has been met this frame
    pub wy_triggered: bool,
    /// Window line
    pub wl: u8,
}

impl PixelFetcher {
    pub fn reset_scanline(&mut self) {
        self.state = PixelFetcherState::GetTile1;
        self.x = 0;
        self.tile_id = 0;
        self.tile_line = TileLine::default();
        self.window_mode = false;
    }

    pub fn reset_frame(&mut self) {
        self.reset_scanline();
        self.wy_triggered = false;
        self.wl = 0;
    }
}

/// What the pixel fetcher's task is during the next dot
#[derive(Debug, Default, Copy, Clone)]
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
    pub fn dot_fetcher(&mut self) {
        if !self.fetcher.window_mode
            && self.lcdc.do_render_window()
            && self.fetcher.wy_triggered
            && self.fifo.lcd_x >= self.wx.saturating_sub(7)
        {
            self.fifo.reset_bg();
            self.fetcher.reset_scanline();
            self.fetcher.window_mode = true;
            return;
        }

        if self.fetcher.sprite_mode.is_none()
            && self.lcdc.do_render_obj()
            && let Some(sprite) = self.scanner.pop_sprite_for_x(self.fifo.lcd_x)
        {
            self.fetcher.state = PixelFetcherState::GetTile1;
            self.fetcher.sprite_mode = Some(sprite);
            return;
        }

        match self.fetcher.state {
            PixelFetcherState::GetTile1 => {
                self.fetcher.state = PixelFetcherState::GetTile2;
            }
            PixelFetcherState::GetTile2 => {
                if let Some(sprite) = &self.fetcher.sprite_mode {
                    if self.lcdc.obj_size {
                        let y_offset = self.fetcher_y();
                        if y_offset >= 8 {
                            self.fetcher.tile_id = sprite.tile_id | 0x01;
                        } else {
                            self.fetcher.tile_id = sprite.tile_id & 0xFE;
                        }
                    } else {
                        self.fetcher.tile_id = sprite.tile_id;
                    }
                } else {
                    let window_mode = self.fetcher.window_mode;

                    let tilemap_addr = if (self.lcdc.bg_tilemap && !window_mode)
                        || (self.lcdc.window_tilemap && window_mode)
                    {
                        0x9C00
                    } else {
                        0x9800
                    };

                    let (tile_x, tile_y) = if window_mode {
                        let tx = (self.fetcher.x / 8) & 0x1F;
                        let ty = (self.fetcher.wl / 8) & 0x1F;
                        (tx, ty)
                    } else {
                        let tx = ((self.scx / 8).wrapping_add(self.fetcher.x / 8)) & 0x1F;
                        let ty = self.ly.wrapping_add(self.scy);
                        (tx, (ty / 8) & 0x1F)
                    };

                    let index = tile_x as u16 + (tile_y as u16 * 32);
                    let addr = tilemap_addr + index;
                    self.fetcher.tile_id = self.blocked_read(addr);
                }

                self.fetcher.state = PixelFetcherState::GetTileDataLow1;
            }
            PixelFetcherState::GetTileDataLow1 => {
                self.fetcher.state = PixelFetcherState::GetTileDataLow2;
            }
            PixelFetcherState::GetTileDataLow2 => {
                let addr = if self.fetcher.sprite_mode.is_some() {
                    self.lcdc
                        .obj_tile_line_address(self.fetcher.tile_id, self.fetcher_y())
                } else {
                    self.lcdc
                        .bg_win_tile_line_address(self.fetcher.tile_id, self.fetcher_y())
                };
                self.fetcher.tile_line.low = self.blocked_read(addr);
                self.fetcher.state = PixelFetcherState::GetTileDataHigh1;
            }
            PixelFetcherState::GetTileDataHigh1 => {
                self.fetcher.state = PixelFetcherState::GetTileDataHigh2;
            }
            PixelFetcherState::GetTileDataHigh2 => {
                let addr = if self.fetcher.sprite_mode.is_some() {
                    self.lcdc
                        .obj_tile_line_address(self.fetcher.tile_id, self.fetcher_y())
                } else {
                    self.lcdc
                        .bg_win_tile_line_address(self.fetcher.tile_id, self.fetcher_y())
                };
                self.fetcher.tile_line.high = self.blocked_read(addr + 1);

                // ToDo: Check where exactly the push happens
                //self.try_push_to_fifo();
                self.fetcher.state = PixelFetcherState::Push;
            }
            PixelFetcherState::Push => {
                if self.try_push_to_fifo() {
                    if self.fetcher.sprite_mode.is_none() {
                        self.fetcher.x += 8;
                    }
                    self.fetcher.state = PixelFetcherState::GetTile1;
                    self.fetcher.sprite_mode = None;
                }
            }
        }
    }

    // ToDo: Blocked PPU read => https://gbdev.io/pandocs/pixel_fifo.html#vram-access
    fn blocked_read(&self, addr: u16) -> u8 {
        self.read_naive(addr)
    }

    fn fetcher_y(&self) -> u8 {
        if let Some(sprite) = &self.fetcher.sprite_mode {
            let mut sprite_line = self.ly.wrapping_add(16).wrapping_sub(sprite.y);
            if sprite.flags.y_flip {
                let height = self.lcdc.sprite_height();
                sprite_line = height.saturating_sub(1).wrapping_sub(sprite_line);
            }
            sprite_line
        } else if self.fetcher.window_mode {
            self.fetcher.wl
        } else {
            self.ly.wrapping_add(self.scy)
        }
    }

    fn try_push_to_fifo(&mut self) -> bool {
        if let Some(sprite) = &self.fetcher.sprite_mode {
            let pixels = std::array::from_fn(|i| {
                let i = if sprite.flags.x_flip { 7 - i } else { i };
                FifoPixel {
                    color_index: self.fetcher.tile_line.color_index(i as u8),
                    palette: if self.model.is_dmg() {
                        sprite.flags.dmg_palette as u8
                    } else {
                        sprite.flags.cgb_palette
                    },
                    sprite_priority: sprite.oam_index,
                    obj_bg_priority: sprite.flags.obj_bg_priority,
                }
            });

            // Important for when the sprite is halfway offscreen to the left
            let offset = 8u8.saturating_sub(sprite.x);
            self.fifo.push_sprite(pixels, offset);
        } else {
            if !self.fifo.bg_empty() {
                return false;
            };

            // ToDo: CGB palette from attribute byte in VRAM bank 1
            let pixels = std::array::from_fn(|i| FifoPixel {
                color_index: self.fetcher.tile_line.color_index(i as u8),
                palette: 0,
                sprite_priority: 0,
                obj_bg_priority: false,
            });
            self.fifo.push_bg(pixels);
        }

        true
    }
}
