use crate::gb::ppu::fifo::FifoPixel;
use crate::gb::ppu::tile::TileLine;
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
    /// Whether WY condition has been met this frame
    pub wy_triggered: bool,
    /// Window line
    // ToDo: Check when to increment
    pub wl: u8,
}

impl PixelFetcher {
    pub fn reset(&mut self) {
        self.state = PixelFetcherState::GetTile1;
        self.x = 0;
        self.tile_id = 0;
        self.tile_line = TileLine::default();
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

// ToDo: Check initial delay, how does it influence accuracy? is it really relevant?
// jsgroth's: PPU fetches the first tile twice, and "renders the first copy offscreen before pushing any pixels to the display
// => This adds another 8 cycles as the PPU pops these 8 pixels off the queue without clocking the LCD.
impl Ppu {
    /// Returns true when done
    pub fn dot_fetcher(&mut self) -> bool {
        if self.fetcher.x == 160 {
            return true;
        };

        match self.fetcher.state {
            PixelFetcherState::GetTile1 => {
                self.fetcher.state = PixelFetcherState::GetTile2;
            }
            PixelFetcherState::GetTile2 => {
                let inside_window = self.fetcher_inside_window();

                let tilemap_addr = if (self.lcdc.bg_tilemap && !inside_window)
                    || (self.lcdc.window_tilemap && inside_window)
                {
                    0x9C00
                } else {
                    0x9800
                };

                let (tile_x, tile_y) = if inside_window {
                    let wx = self.fetcher.x.wrapping_sub(self.wx.wrapping_sub(7)) / 8;
                    let wy = self.fetcher.wl;
                    (wx & 0x1F, (wy / 8) & 0x1F)
                } else {
                    let tx = ((self.scx / 8).wrapping_add(self.fetcher.x / 8)) & 0x1F;
                    let ty = self.ly.wrapping_add(self.scy);
                    (tx, (ty / 8) & 0x1F)
                };

                let index = tile_x as u16 + (tile_y as u16 * 32);
                let addr = tilemap_addr + index;
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
                self.fetcher.tile_line.low = self.blocked_read(addr);
                self.fetcher.state = PixelFetcherState::GetTileDataHigh1;
            }
            PixelFetcherState::GetTileDataHigh1 => {
                self.fetcher.state = PixelFetcherState::GetTileDataHigh2;
            }
            PixelFetcherState::GetTileDataHigh2 => {
                let addr = self
                    .lcdc
                    .bg_win_tile_line_address(self.fetcher.tile_id, self.fetcher_y());
                self.fetcher.tile_line.high = self.blocked_read(addr + 1);

                self.try_push_to_fifo();
                self.fetcher.state = PixelFetcherState::Push;
            }
            PixelFetcherState::Push => {
                // ToDo: Check if 8 pixels are pushed at once or if they are pushed one by one (and how it's influencing the fetcher.x increment)
                if self.try_push_to_fifo() {
                    self.fetcher.x += 8;
                    if self.fetcher.x == 160 {
                        return true;
                    } else {
                        self.fetcher.state = PixelFetcherState::GetTile1;
                    }
                }
            }
        }

        false
    }

    // ToDo: Blocked PPU read => https://gbdev.io/pandocs/pixel_fifo.html#vram-access
    fn blocked_read(&self, addr: u16) -> u8 {
        self.read_naive(addr)
    }

    // ToDo: If fetcher finds window pixel, it actually resets the X-Position-Counter, etc. => see GBEDG Window Fetching
    // => Check relevance for this implementation
    fn fetcher_inside_window(&self) -> bool {
        self.lcdc.do_render_window()
            && self.fetcher.wy_triggered
            && self.fetcher.x >= self.wx.wrapping_sub(7)
    }

    fn fetcher_y(&self) -> u8 {
        if self.fetcher_inside_window() {
            self.fetcher.wl
        } else {
            self.ly.wrapping_add(self.scy)
        }
    }

    fn try_push_to_fifo(&mut self) -> bool {
        if !self.fifo.bg_empty() {
            return false;
        };

        // ToDo: CGB palette from attribute byte in VRAM bank 1
        let pixels = std::array::from_fn(|i| FifoPixel {
            x: self.fetcher.x + i as u8,
            y: self.ly,
            color_index: self.fetcher.tile_line.color_index(i as u8),
            palette: 0,
            sprite_priority: 0,
            obj_bg_priority: false,
        });
        self.fifo.push_bg(pixels);

        true
    }
}
