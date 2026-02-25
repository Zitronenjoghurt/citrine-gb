use crate::gb::ppu::Ppu;
use crate::ReadMemory;

#[derive(Debug, Default, Copy, Clone)]
pub struct PixelFetcher {
    pub state: PixelFetcherState,
    pub x: u8,
    pub tilemap_addr: u16,
    pub tile_id: u8,
    pub tile_data_low: u8,
    pub tile_data_high: u8,
    /// Whether WY condition has been met this frame
    pub wy_triggered: bool,
}

impl PixelFetcher {
    pub fn reset(&mut self) {
        self.state = PixelFetcherState::GetTile1;
        self.x = 0;
        self.tilemap_addr = 0;
        self.tile_id = 0;
        self.wy_triggered = false;
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
    // Unsure if the sleep states actually exist, they're in the pan docs but nowhere else??
    //Sleep1,
    //Sleep2,
    Push,
}

impl Ppu {
    pub fn dot_fetcher(&mut self) {
        match self.fetcher.state {
            PixelFetcherState::GetTile1 => {
                let inside_window = self.fetcher_inside_window();
                if (self.lcdc.bg_tilemap && !inside_window)
                    || (self.lcdc.window_tilemap && inside_window)
                {
                    self.fetcher.tilemap_addr = 0x9C00;
                } else {
                    self.fetcher.tilemap_addr = 0x9800;
                }
                self.fetcher.state = PixelFetcherState::GetTile2;
            }
            PixelFetcherState::GetTile2 => {
                let (tile_x, tile_y) = if self.fetcher_inside_window() {
                    let wx = self.fetcher.x.wrapping_sub(self.wx.wrapping_sub(7)) / 8;
                    let wy = self.wl;
                    (wx & 0x1F, (wy / 8) & 0x1F)
                } else {
                    let tx = ((self.scx / 8).wrapping_add(self.fetcher.x)) & 0x1F;
                    let ty = self.ly.wrapping_add(self.scy);
                    (tx, (ty / 8) & 0x1F)
                };

                let index = tile_x as u16 + (tile_y as u16 * 32);
                let addr = self.fetcher.tilemap_addr + index;
                self.fetcher.tile_id = self.blocked_read(addr);
                self.fetcher.state = PixelFetcherState::GetTileDataLow1;
            }
            PixelFetcherState::GetTileDataLow1 => {
                self.fetcher.state = PixelFetcherState::GetTileDataLow2;
            }
            PixelFetcherState::GetTileDataLow2 => {
                let addr = self
                    .lcdc
                    .bg_win_tile_line_address(self.fetcher.tile_id, self.fetcher_y());
                self.fetcher.tile_data_low = self.blocked_read(addr);
                self.fetcher.state = PixelFetcherState::GetTileDataHigh1;
            }
            PixelFetcherState::GetTileDataHigh1 => {
                self.try_push_to_fifo();
                self.fetcher.state = PixelFetcherState::GetTileDataHigh2;
            }
            PixelFetcherState::GetTileDataHigh2 => {
                let addr = self
                    .lcdc
                    .bg_win_tile_line_address(self.fetcher.tile_id, self.fetcher_y());
                self.fetcher.tile_data_high = self.blocked_read(addr + 1);

                self.try_push_to_fifo();
                self.fetcher.state = PixelFetcherState::Push;
                //self.fetcher.state = PixelFetcherState::Sleep1;
            }
            //PixelFetcherState::Sleep1 => {
            //    self.fetcher.state = PixelFetcherState::Sleep2;
            //}
            //PixelFetcherState::Sleep2 => {
            //    self.fetcher.state = PixelFetcherState::Push;
            //}
            PixelFetcherState::Push => {
                if self.try_push_to_fifo() {
                    self.fetcher.x += 1;
                    self.fetcher.state = PixelFetcherState::GetTile1;
                }
            }
        }
    }

    // ToDo: Blocked PPU read => https://gbdev.io/pandocs/pixel_fifo.html#vram-access
    fn blocked_read(&self, addr: u16) -> u8 {
        self.read_naive(addr)
    }

    fn fetcher_inside_window(&self) -> bool {
        self.lcdc.do_render_window()
            && self.fetcher.wy_triggered
            && self.fetcher.x >= self.wx.wrapping_sub(7)
    }

    fn fetcher_y(&self) -> u8 {
        if self.fetcher_inside_window() {
            self.wl
        } else {
            self.ly.wrapping_add(self.scy)
        }
    }

    // ToDo: Implement
    // Push 8 pixels at once
    fn try_push_to_fifo(&mut self) -> bool {
        todo!()
    }
}
