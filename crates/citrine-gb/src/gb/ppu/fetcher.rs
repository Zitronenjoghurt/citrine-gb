use crate::gb::ppu::colors::ColorCache;
use crate::gb::ppu::fifo::FifoPixel;
use crate::gb::ppu::types::attributes::BgAttributes;
use crate::gb::ppu::types::sprite::SpriteFlags;
use crate::gb::ppu::types::tile::TileLine;
use crate::gb::ppu::{Ppu, VRAM_BANK_SIZE};

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

#[derive(Debug, Copy, Clone)]
struct BgTile {
    map_address: u16,
    id: u8,
    attributes: BgAttributes,
    data_address: u16,
    line: TileLine,
}

impl BgTile {
    fn indices(&self) -> u64 {
        bg_row_indices(self.line, self.attributes)
    }
}

fn bg_row_indices(line: TileLine, attributes: BgAttributes) -> u64 {
    let indices = line.color_indices();
    if attributes.x_flip {
        indices.swap_bytes()
    } else {
        indices
    }
}

#[derive(Debug, Copy, Clone)]
struct BgRows {
    base: usize,
    flip: u8,
    row: u8,
    render_bg: bool,
}

impl BgRows {
    #[inline(always)]
    fn entry<const CGB: bool>(self, vram: &[[u8; VRAM_BANK_SIZE]; 2], map_address: u16) -> u16 {
        let at = (map_address & 0x1FFF) as usize;
        let attributes = if CGB { vram[1][at] } else { 0 };
        u16::from(vram[0][at]) | (u16::from(attributes) << 8)
    }

    #[inline(always)]
    fn put_tiles<const CGB: bool>(
        self,
        vram: &[[u8; VRAM_BANK_SIZE]; 2],
        colors: &ColorCache,
        map_row: u16,
        x0: u8,
        out: &mut [u8],
    ) {
        let row = usize::from(map_row & 0x1FE0);
        let (ids, attributes) = (&vram[0][row..row + 32], &vram[1][row..row + 32]);
        let x0 = usize::from(x0 & 0x1F);
        let spans = out.as_chunks_mut::<32>().0;
        let (head, tail) = spans.split_at_mut(spans.len().min(32 - x0));
        let mut last = u32::MAX;
        let mut pairs = [0; 4];
        for (spans, from) in [(head, x0), (tail, 0)] {
            let entries = ids[from..].iter().zip(&attributes[from..]);
            for (span, (&id, &attribute)) in spans.iter_mut().zip(entries) {
                let entry = u16::from(id) | (u16::from(if CGB { attribute } else { 0 }) << 8);
                if u32::from(entry) != last {
                    last = u32::from(entry);
                    let (indices, palette) = self.indices(vram, entry);
                    let indices = if CGB || self.render_bg { indices } else { 0 };
                    pairs = colors.bg_span(palette, indices);
                }
                for (chunk, pair) in span.as_chunks_mut::<8>().0.iter_mut().zip(pairs) {
                    *chunk = pair.to_ne_bytes();
                }
            }
        }
    }

    #[inline(always)]
    fn indices(self, vram: &[[u8; VRAM_BANK_SIZE]; 2], entry: u16) -> (u64, u8) {
        let id = entry as u8;
        let attributes = BgAttributes::from((entry >> 8) as u8);
        let row = if attributes.y_flip {
            7 - self.row
        } else {
            self.row
        };
        let tile = self.base + usize::from(id ^ self.flip) * 16;
        let bank = &vram[attributes.bank as usize];
        let at = tile + row as usize * 2;
        let line = TileLine {
            low: bank[at & 0x1FFF],
            high: bank[(at + 1) & 0x1FFF],
        };
        (bg_row_indices(line, attributes), attributes.palette)
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
    #[inline(always)]
    pub fn advance_fetcher(&mut self) {
        match self.fetcher.state {
            PixelFetcherState::GetTile1 => self.fetch_tile_address(self.fifo.position),
            PixelFetcherState::GetTile2 => {
                self.read_tile_id();
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
                self.read_tile_high();
                self.push_bg_row();
            }
            PixelFetcherState::Push => {
                if self.fifo.bg.is_empty() {
                    self.fill_bg_row();
                }
            }
        }
    }

    #[inline(always)]
    pub fn fetch_tile_address(&mut self, position: u8) {
        if !self.lcdc.window_enable {
            self.fetcher.window_mode = false;
        }
        let window = self.fetcher.window_mode;
        let map = if (window && self.lcdc.window_tilemap) || (!window && self.lcdc.bg_tilemap) {
            0x9C00
        } else {
            0x9800
        };
        let y = self.bg_fetcher_y();
        let x = if window {
            self.fetcher.window_x
        } else if position.wrapping_add(16) < 8 {
            self.scx >> 3
        } else {
            let cgb_shift = (self.model.is_cgb() && !self.fetching_object()) as u8;
            (self
                .scx
                .wrapping_add(position)
                .wrapping_add(8)
                .wrapping_sub(cgb_shift)
                >> 3)
                & 0x1F
        };
        self.fetcher.tile_y = y;
        self.fetcher.tile_map_address = map + x as u16 + (y as u16 / 8) * 32;
        self.fetcher.state = PixelFetcherState::GetTile2;
    }

    pub fn fetch_tile_row(&mut self) {
        if self.tile_sel_glitch {
            self.read_tile_id();
            self.latch_bg_tile_line_address(0);
            self.fetcher.tile_line.low = self.read_tile_data(false);
            self.latch_bg_tile_line_address(1);
            self.read_tile_high();
            self.push_bg_row();
            return;
        }
        let y = if self.model.is_cgb() {
            self.fetcher.tile_y
        } else {
            self.bg_fetcher_y()
        };
        let tile = self.read_bg_tile(self.fetcher.tile_map_address, y);
        if self.fetcher.window_mode {
            self.fetcher.window_x = (self.fetcher.window_x + 1) & 0x1F;
        }
        self.keep_bg_tile(tile);
    }

    pub fn draw_bg_run(&mut self, render_bg: bool, next_object: u8) -> u16 {
        self.fifo.window_fetching = false;
        self.plain_span(render_bg);
        8 + self.draw_bg_tiles(render_bg, next_object, false)
    }

    fn draw_bg_tiles(&mut self, render_bg: bool, next_object: u8, lead: bool) -> u16 {
        if !self.lcdc.window_enable {
            self.fetcher.window_mode = false;
        }
        let window = self.fetcher.window_mode;
        let map = if (window && self.lcdc.window_tilemap) || (!window && self.lcdc.bg_tilemap) {
            0x9C00
        } else {
            0x9800
        };
        let y = self.bg_fetcher_y();
        let map_row = map + (y as u16 / 8) * 32;
        let scroll = self.scx.wrapping_add(9 - self.model.is_cgb() as u8);
        let start = self.fifo.position - 8;
        let first = u16::from(start) + 8;
        let wx = u16::from(self.wx);
        let mut limit = u16::from(next_object).saturating_sub(15).min(153);
        if self.wx_matters() && wx >= first + 6 {
            limit = limit.min(wx.saturating_sub(15));
        }
        let end = (first + limit.saturating_sub(first).next_multiple_of(8)) as u8;
        let tiles = (end - start) / 8;
        let window_x = self.fetcher.window_x;
        let x0 = if window {
            window_x
        } else {
            scroll.wrapping_add(start) >> 3
        };
        let tile_x = move |i: u8| x0.wrapping_add(i) & 0x1F;
        let lcd_x = self.fifo.lcd_x;
        if !self.blind {
            let (base, flip) = if self.lcdc.bg_window_tiles {
                (0, 0)
            } else {
                (0x800, 0x80)
            };
            debug_assert!(render_bg || !self.cgb_mode);
            let rows = BgRows {
                base,
                flip,
                row: y & 7,
                render_bg,
            };
            let lead = u8::from(lead);
            let from = lcd_x.wrapping_sub(lead * 8);
            let first_x = x0.wrapping_sub(lead);
            let drawn = tiles - 1 + lead;
            let pixels = usize::from(drawn) * 8;
            let at = (self.line as usize * 160 + usize::from(from)) * 4;
            let out = if usize::from(from) + pixels <= 160 {
                self.frame.bytes_mut(at, pixels * 4)
            } else {
                None
            };
            match out {
                Some(out) if self.cgb_mode => {
                    rows.put_tiles::<true>(&self.vram, &self.colors, map_row, first_x, out)
                }
                Some(out) => {
                    rows.put_tiles::<false>(&self.vram, &self.colors, map_row, first_x, out)
                }
                None => self.put_tiles_clipped(rows, map_row, first_x, drawn, from),
            }
        }
        let tile = self.read_bg_tile(map_row + tile_x(tiles - 1) as u16, y);
        if window {
            self.fetcher.window_x = window_x.wrapping_add(tiles) & 0x1F;
        }
        self.fifo.lcd_x = lcd_x.wrapping_add((tiles - 1) * 8);
        self.fetcher.tile_y = y;
        self.fifo.position = end;
        if end == 160 {
            self.keep_last_bg_tile(tile);
        } else {
            self.keep_bg_tile(tile);
        }
        self.scanner.skip_before(end + 7);
        (end - start - 8) as u16
    }

    fn put_tiles_clipped(&mut self, rows: BgRows, map_row: u16, x0: u8, tiles: u8, mut lcd_x: u8) {
        let line = self.line as usize * 160;
        let mut last = u32::MAX;
        let mut pairs = [0; 4];
        for i in 0..tiles {
            let x = x0.wrapping_add(i) & 0x1F;
            let entry = if self.cgb_mode {
                rows.entry::<true>(&self.vram, map_row + u16::from(x))
            } else {
                rows.entry::<false>(&self.vram, map_row + u16::from(x))
            };
            if u32::from(entry) != last {
                last = u32::from(entry);
                let (indices, palette) = rows.indices(&self.vram, entry);
                let indices = if rows.render_bg { indices } else { 0 };
                pairs = self.colors.bg_span(palette, indices);
            }
            self.frame.put_pairs(line, lcd_x, pairs);
            lcd_x = lcd_x.wrapping_add(8);
        }
    }

    pub fn start_plain_line(&mut self) -> u16 {
        if self.fetcher.window_mode
            || self.fifo.window_fetching
            || self.scanner.peek().is_some_and(|o| o.x < 16)
        {
            return 0;
        }
        if self.fetcher.wy_triggered && self.wx < 24 && self.window_acts_at_line_start() {
            return self.start_window_line();
        }
        let y = self.bg_fetcher_y();
        let map = if self.lcdc.bg_tilemap { 0x9C00 } else { 0x9800 };
        let map_row = map + (y as u16 / 8) * 32;
        let tile_x = self.scx >> 3;
        let fine = self.scx & 7;
        self.fetcher.tile_y = y;
        self.fifo.bg.clear();
        if fine == 0 {
            let render_bg = self.cgb_mode || self.lcdc.do_render_bg();
            self.fifo.lcd_x = self.fifo.lcd_x.wrapping_add(8);
            self.fifo.position = 8;
            let next_object = if self.objects_fetchable() {
                self.scanner.next_at_or_after(8).unwrap_or(u8::MAX)
            } else {
                u8::MAX
            };
            return 16 + self.draw_bg_tiles(render_bg, next_object, true);
        }
        let first = self.read_bg_tile(map_row + tile_x as u16, y);
        let render_bg = self.cgb_mode || self.lcdc.do_render_bg();
        if !self.blind {
            let indices = if render_bg { first.indices() } else { 0 };
            let row = self.line as usize * 160;
            for i in fine..8 {
                let color = self
                    .colors
                    .bg(first.attributes.palette, (indices >> (i * 8)) as u8);
                self.frame.set(row + (i - fine) as usize, color);
            }
        }
        self.fifo.lcd_x = 8 - fine;
        self.fifo.position = 8 - fine;
        let next_object = if self.objects_fetchable() {
            self.scanner.next_at_or_after(16 - fine).unwrap_or(u8::MAX)
        } else {
            u8::MAX
        };
        let wx_near = self.wx_matters_at(8 - fine) && (14 - fine..=23 - fine).contains(&self.wx);
        if next_object > 23 - fine && !wx_near {
            debug_assert!(!self.window_pixel_glitch());
            self.fifo.lcd_x = 16 - fine;
            self.fifo.position = 16 - fine;
            return 24 + self.draw_bg_tiles(render_bg, next_object, true);
        }
        let second = self.read_bg_tile(map_row + ((tile_x + 1) & 0x1F) as u16, y);
        self.keep_bg_tile(second);
        16
    }

    fn window_acts_at_line_start(&self) -> bool {
        self.lcdc.window_enable
            || (!self.model.is_cgb() && matches!(self.wx.wrapping_add(self.scx & 7), 7 | 15))
    }

    fn start_window_line(&mut self) -> u16 {
        if self.wx != 7 || !self.lcdc.window_enable {
            return 0;
        }
        self.fetcher.wl = self.fetcher.wl.wrapping_add(1);
        self.fetcher.window_mode = true;
        let y = self.fetcher.wl;
        let map = if self.lcdc.window_tilemap {
            0x9C00
        } else {
            0x9800
        };
        let map_row = map + (y as u16 / 8) * 32;
        let first = self.read_bg_tile(map_row, y);
        self.fetcher.tile_y = y;
        self.fetcher.window_x = 2;
        let render_bg = self.cgb_mode || self.lcdc.do_render_bg();
        self.put_window_tile(first, render_bg);
        self.fifo.bg.clear();
        self.fifo.position = 8;
        let second = self.read_bg_tile(map_row + 1, y);
        let next_object = if self.objects_fetchable() {
            self.scanner.next_at_or_after(16)
        } else {
            None
        };
        if next_object.is_some_and(|x| x <= 23) {
            self.keep_bg_tile(second);
            return 22 + (self.scx & 7) as u16;
        }
        self.put_window_tile(second, render_bg);
        self.fifo.position = 16;
        30 + (self.scx & 7) as u16
            + self.draw_bg_tiles(render_bg, next_object.unwrap_or(u8::MAX), false)
    }

    #[inline(always)]
    fn put_window_tile(&mut self, tile: BgTile, render_bg: bool) {
        if !self.blind && self.line < 144 {
            let indices = if render_bg { tile.indices() } else { 0 };
            let pairs = self.colors.bg_span(tile.attributes.palette, indices);
            self.frame
                .put_pairs(self.line as usize * 160, self.fifo.lcd_x, pairs);
        }
        self.fifo.lcd_x = self.fifo.lcd_x.wrapping_add(8);
    }

    #[inline(always)]
    fn read_bg_tile(&self, map_address: u16, y: u8) -> BgTile {
        let id = self.vram_read(0, map_address);
        let attributes = if self.cgb_mode {
            self.vram_read(1, map_address).into()
        } else {
            BgAttributes::default()
        };
        let row = if attributes.y_flip {
            7 - (y & 7)
        } else {
            y & 7
        };
        let data_address = self.lcdc.bg_win_tile_line_address(id, row);
        let bank = attributes.bank as u8;
        BgTile {
            map_address,
            id,
            attributes,
            data_address,
            line: TileLine {
                low: self.vram_read(bank, data_address),
                high: self.vram_read(bank, data_address + 1),
            },
        }
    }

    fn keep_bg_tile(&mut self, tile: BgTile) {
        self.store_bg_tile(tile);
        self.push_bg_row();
    }

    fn keep_last_bg_tile(&mut self, tile: BgTile) {
        self.store_bg_tile(tile);
        debug_assert!(self.fifo.bg.is_empty());
        if self.window_pixel_glitch() {
            self.fetcher.state = PixelFetcherState::Push;
            self.fifo.bg.fill_one(FifoPixel::default());
        } else {
            self.fetcher.state = PixelFetcherState::GetTile1;
            self.fifo.bg_row_pending = true;
        }
    }

    pub fn fill_pending_bg_row(&mut self) {
        if std::mem::take(&mut self.fifo.bg_row_pending) {
            self.fifo.bg.fill(self.bg_row_pixels());
        }
    }

    fn store_bg_tile(&mut self, tile: BgTile) {
        self.fetcher.tile_map_address = tile.map_address;
        self.fetcher.tile_id = tile.id;
        self.fetcher.tile_attributes = tile.attributes;
        self.fetcher.last_tileset = self.lcdc.bg_window_tiles;
        self.fetcher.tile_data_bank = tile.attributes.bank as u8;
        self.fetcher.tile_data_address = tile.data_address + 1;
        self.fetcher.tile_line = tile.line;
        if self.fetcher.last_tileset {
            self.fetcher.sel_glitch_data = tile.line.high;
        }
    }

    fn read_tile_id(&mut self) {
        let addr = self.fetcher.tile_map_address;
        self.fetcher.tile_id = self.vram_read(0, addr);
        self.fetcher.tile_attributes = if self.cgb_mode {
            self.vram_read(1, addr).into()
        } else {
            BgAttributes::default()
        };
    }

    fn read_tile_high(&mut self) {
        self.fetcher.tile_line.high = self.read_tile_data(true);
        if self.fetcher.window_mode {
            self.fetcher.window_x = (self.fetcher.window_x + 1) & 0x1F;
        }
    }

    #[inline(always)]
    fn push_bg_row(&mut self) {
        self.fetcher.state = PixelFetcherState::Push;
        if self.fifo.bg.is_empty() {
            self.fill_bg_row();
        }
    }

    fn fill_bg_row(&mut self) {
        if self.window_pixel_glitch() {
            self.fifo.bg.fill_one(FifoPixel::default());
            return;
        }
        self.fifo.bg.fill(self.bg_row_pixels());
        self.fetcher.state = PixelFetcherState::GetTile1;
    }

    fn bg_row_pixels(&self) -> [FifoPixel; 8] {
        let attributes = self.fetcher.tile_attributes;
        let indices = bg_row_indices(self.fetcher.tile_line, attributes);
        std::array::from_fn(|i| FifoPixel {
            color_index: (indices >> (i * 8)) as u8,
            palette: attributes.palette,
            sprite_priority: 0,
            obj_bg_priority: attributes.bg_priority,
        })
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
            if high && self.fetcher.last_tileset {
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
        let match_x = self.fifo.object_match_x();
        self.scanner.skip_before(match_x);
        let Some(&object) = self.scanner.peek() else {
            return false;
        };
        if object.x != match_x || !self.objects_fetchable() {
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

    pub fn finish_object_fetch(&mut self, limit: u16) -> u16 {
        let mut dots = 0;
        let state = self.fetcher.state;
        if self.fetcher.object.step == ObjectStep::WaitForBackground
            && !self.fifo.bg.is_empty()
            && state > PixelFetcherState::GetTile1
            && state < PixelFetcherState::GetTileDataHigh2
            && PixelFetcherState::Push as u16 - state as u16 <= limit
        {
            self.fetch_tile_row();
            self.fetcher.object.step = ObjectStep::ReadAttributes;
            dots = PixelFetcherState::Push as u16 - state as u16;
        }
        if self.fetcher.object.step == ObjectStep::WaitForBackground {
            while dots < limit {
                let waiting = self.fetcher.state < PixelFetcherState::GetTileDataHigh2
                    || self.fifo.bg.is_empty();
                self.advance_fetcher();
                dots += 1;
                if !waiting {
                    self.fetcher.object.step = ObjectStep::ReadAttributes;
                    break;
                }
            }
        }
        if self.fetcher.object.step == ObjectStep::ReadAttributes && dots + 5 <= limit {
            self.read_object_row();
            return dots + 5;
        }
        while dots < limit && self.fetcher.object.step != ObjectStep::Overlay {
            self.advance_object_fetch();
            dots += 1;
        }
        dots
    }

    fn read_object_row(&mut self) {
        self.advance_fetcher();
        self.read_object_tile();
    }

    pub fn read_object_tile(&mut self) {
        let i = self.fetcher.object.oam_index as usize * 4;
        self.fetcher.object.tile_id = self.oam[i + 2];
        self.fetcher.object.flags = self.oam[i + 3];
        let (bank, addr) = self.object_line_address();
        self.fetcher.object.tile_line.low = self.vram_read(bank, addr);
        self.fetcher.object.tile_line.high = self.vram_read(bank, addr + 1);
        self.fetcher.sel_glitch_data = self.fetcher.object.tile_line.high;
        self.fetcher.object.step = ObjectStep::Overlay;
    }

    pub fn overlay_object(&mut self) {
        let object = self.fetcher.object;
        let flags = SpriteFlags::from(object.flags);
        let palette = if self.cgb_mode {
            flags.cgb_palette
        } else {
            flags.dmg_palette as u8
        };
        let indices = object.tile_line.color_indices();
        let indices = if flags.x_flip {
            indices.swap_bytes()
        } else {
            indices
        };
        let pixel = FifoPixel {
            color_index: 0,
            palette,
            sprite_priority: object.oam_index,
            obj_bg_priority: flags.obj_bg_priority,
        };
        let oam_priority = self.model.is_cgb() && self.opri & 1 == 0;
        self.fifo.sprite.overlay(indices, pixel, oam_priority);
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
