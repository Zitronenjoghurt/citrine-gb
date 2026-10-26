use crate::gb::GbModel;
use crate::gb::ic::{ICInterface, Interrupt};
use crate::gb::ppu::fetcher::{ObjectStep, PixelFetcher, PixelFetcherState};
use crate::gb::ppu::fifo::{POSITION_START, PixelFifo};
use crate::gb::ppu::scanner::OamScanner;
use crate::gb::ppu::types::theme::DmgTheme;
use crate::{ReadMemory, WriteMemory};
use types::color::{ColorCorrection, DmgColors, RGBA};
use types::framebuffer::Framebuffer;
use types::lcdc::LCDC;
use types::mode::PpuMode;
use types::stat::STAT;

pub mod compat;
mod fetcher;
mod fifo;
mod scanner;
pub mod types;

pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;
const VRAM_BANK_SIZE: usize = 0x2000; // 8KiB
const OAM_SIZE: usize = 160; // Bytes

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ppu {
    #[cfg_attr(feature = "serde", serde(skip, default))]
    frame: Framebuffer,
    model: GbModel,
    #[cfg_attr(feature = "serde", serde(default))]
    cgb_mode: bool,
    pub frame_ready: bool,
    pub dmg_theme: DmgTheme,
    #[cfg_attr(feature = "serde", serde(default))]
    pub dmg_colors: DmgColors,
    #[cfg_attr(feature = "serde", serde(default))]
    pub color_correction: ColorCorrection,
    #[cfg_attr(feature = "serde", serde(default))]
    pub fetcher: PixelFetcher,
    #[cfg_attr(feature = "serde", serde(default))]
    pub fifo: PixelFifo,
    #[cfg_attr(feature = "serde", serde(default))]
    pub scanner: OamScanner,
    stat_line_previous: bool,
    /// Dots in the current scanline
    pub line_dot_counter: u16,
    #[cfg_attr(feature = "serde", serde(default))]
    line: u8,
    #[cfg_attr(feature = "serde", serde(default))]
    ly_compare: Option<u8>,
    #[cfg_attr(feature = "serde", serde(default))]
    irq_mode: Option<PpuMode>,
    #[cfg_attr(feature = "serde", serde(default))]
    lyc_line: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    first_line: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    drawing: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    hblank_dot: u16,
    #[cfg_attr(feature = "serde", serde(default))]
    double_speed: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    stat_write_glitch: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    wx_just_changed: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    tile_sel_glitch: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    timed_writes: [Option<TimedWrite>; 2],
    #[cfg_attr(feature = "serde", serde(default))]
    skip_dot: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    access: AccessBlocks,
    #[cfg_attr(feature = "serde", serde(default))]
    lcd_on_frame: bool,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    held_frame: Option<Framebuffer>,
    // Memory
    /// Video RAM (2 banks on CGB)
    #[cfg_attr(feature = "serde", serde(with = "serde_vram"))]
    vram: [[u8; VRAM_BANK_SIZE]; 2],
    /// Sprite attribute table
    #[cfg_attr(feature = "serde", serde(with = "serde_oam"))]
    oam: [u8; OAM_SIZE],
    /// LCD control
    pub lcdc: LCDC,
    /// LCD status
    pub stat: STAT,
    /// BG scroll Y
    pub scy: u8,
    /// BG scroll X
    pub scx: u8,
    /// Current scanline
    pub ly: u8,
    /// Scanline compare
    pub lyc: u8,
    /// BG palette (DMG)
    bgp: u8,
    /// OBJ palette 0 (DMG)
    obp0: u8,
    /// OBJ palette 1 (DMG)
    obp1: u8,
    /// Window Y position
    pub wy: u8,
    /// Window X position (+7)
    pub wx: u8,
    /// VRAM bank select (CGB)
    vbk: u8,
    /// BG palette index (CGB)
    bcps: u8,
    /// Internal BG palette RAM, accessed via BCPS
    #[cfg_attr(feature = "serde", serde(with = "serde_64"))]
    bg_palette_ram: [u8; 64],
    /// OBJ palette index (CGB)
    ocps: u8,
    /// Internal OBJ palette RAM, accessed via OCPS
    #[cfg_attr(feature = "serde", serde(with = "serde_64"))]
    obj_palette_ram: [u8; 64],
    /// OBJ priority mode (CGB)
    opri: u8,
}

#[derive(Debug, Default, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct TimedWrite {
    addr: u16,
    value: u8,
    dot: u8,
    #[cfg_attr(feature = "serde", serde(default))]
    merge: bool,
}

#[derive(Debug, Default, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct AccessBlocks {
    oam_read: bool,
    oam_write: bool,
    vram_read: bool,
    vram_write: bool,
    palettes: bool,
}

const BOOT_TRADEMARK: [u8; 8] = [0x3C, 0x42, 0xB9, 0xA5, 0xB9, 0xA5, 0x42, 0x3C];
const LINE_DOTS: u16 = 456;
const LCD_ON_DOT: u16 = 7;
const MODE3_START: u16 = 89;
const FIRST_LINE_MODE3_START: u16 = 91;
const DMG_BOOT_LINE: u8 = 153;
const DMG_BOOT_DOT: u16 = 351;
const CGB_BOOT_LINE: u8 = 145;
const CGB_BOOT_DOT: u16 = 5;
const DOUBLE_SPEED_SWITCH_DOTS: u8 = 6;

impl Ppu {
    pub fn new(model: GbModel) -> Self {
        let mut ppu = Self {
            frame: Framebuffer::new(),
            model,
            cgb_mode: model.is_cgb(),
            frame_ready: false,
            dmg_theme: DmgTheme::default(),
            dmg_colors: if model.is_cgb() {
                DmgColors::PaletteRam
            } else {
                DmgColors::Theme
            },
            color_correction: ColorCorrection::default(),
            fetcher: PixelFetcher::default(),
            fifo: PixelFifo::default(),
            scanner: OamScanner::default(),
            stat_line_previous: false,
            line_dot_counter: 0,
            line: 0,
            ly_compare: None,
            irq_mode: None,
            lyc_line: false,
            first_line: false,
            drawing: false,
            hblank_dot: 0,
            double_speed: false,
            stat_write_glitch: false,
            wx_just_changed: false,
            tile_sel_glitch: false,
            timed_writes: [None; 2],
            skip_dot: false,
            access: AccessBlocks::default(),
            lcd_on_frame: false,
            held_frame: None,
            vram: [[0x00; VRAM_BANK_SIZE]; 2],
            oam: [0x00; OAM_SIZE],
            lcdc: 0x91.into(),
            stat: 0x80.into(),
            scy: 0x00,
            scx: 0x00,
            ly: 0x00,
            lyc: 0x00,
            bgp: 0xFC,
            obp0: 0x00,
            obp1: 0x00,
            wy: 0x00,
            wx: 0x00,
            vbk: 0xFE,
            bcps: 0x00,
            bg_palette_ram: if model.is_cgb() {
                [0xFF, 0x7F].repeat(32).try_into().unwrap()
            } else {
                [0x00; 64]
            },
            ocps: 0x00,
            obj_palette_ram: [0x00; 64],
            opri: 0x00,
        };
        ppu.fetcher.reset_frame();
        let (line, dot) = if model.is_cgb() {
            (CGB_BOOT_LINE, CGB_BOOT_DOT)
        } else {
            (DMG_BOOT_LINE, DMG_BOOT_DOT)
        };
        ppu.line = line;
        ppu.ly = if line == 153 { 0 } else { line };
        ppu.ly_compare = Some(ppu.ly);
        ppu.line_dot_counter = dot;
        let mode = if line >= 144 {
            PpuMode::VBlank
        } else {
            PpuMode::HBlank
        };
        ppu.stat.ppu_mode = mode;
        ppu.irq_mode = Some(mode);
        ppu.check_lyc();
        ppu
    }

    pub fn cycle(&mut self, ic: &mut impl ICInterface, oam_dma: bool, double_speed: bool) {
        if !self.lcdc.lcd_enabled {
            for write in std::mem::take(&mut self.timed_writes).into_iter().flatten() {
                self.write_naive(write.addr, write.value);
            }
            return;
        }

        self.double_speed = double_speed;
        let mut dots = if double_speed { 2 } else { 4 };
        if self.skip_dot {
            self.skip_dot = false;
            dots -= 1;
        }
        if self.timed_writes[0].is_none() {
            for _ in 0..dots {
                self.dot(ic, oam_dma);
            }
            return;
        }
        for dot in 1..=dots {
            self.dot(ic, oam_dma);
            self.apply_timed_writes(dot);
        }
        for write in self.timed_writes.iter_mut().flatten() {
            write.dot -= dots;
        }
    }

    pub fn cycle_sampled(
        &mut self,
        ic: &mut impl ICInterface,
        oam_dma: bool,
        double_speed: bool,
        sample_dot: u8,
    ) -> bool {
        let dots = if double_speed { 2 } else { 4 };
        if !self.lcdc.lcd_enabled
            || self.timed_writes[0].is_some()
            || self.skip_dot
            || sample_dot >= dots
        {
            self.cycle(ic, oam_dma, double_speed);
            return ic.has_pending_interrupt();
        }
        self.double_speed = double_speed;
        for _ in 0..sample_dot {
            self.dot(ic, oam_dma);
        }
        let pending = ic.has_pending_interrupt();
        for _ in sample_dot..dots {
            self.dot(ic, oam_dma);
        }
        pending
    }

    fn apply_timed_writes(&mut self, dot: u8) {
        for i in 0..self.timed_writes.len() {
            if let Some(write) = self.timed_writes[i]
                && write.dot == dot
            {
                let value = if write.merge {
                    self.dmg_lcdc_conflict(write.value)
                } else {
                    write.value
                };
                let old = u8::from(self.lcdc);
                self.write_naive(write.addr, value);
                self.timed_writes[i] = None;
                if write.addr == 0xFF40 && self.model.is_cgb() && old & !value & 0x10 != 0 {
                    self.tile_sel_glitch = true;
                }
            }
        }
        if self.timed_writes[0].is_none() {
            self.timed_writes.swap(0, 1);
        }
    }

    fn dmg_lcdc_conflict(&self, value: u8) -> u8 {
        let mut old = u8::from(self.lcdc);
        let fetching = self.fetching_object() && self.fetcher.object.step < ObjectStep::ReadHigh;
        if value & 0x02 == 0 && (self.fifo.position == 0 || fetching) {
            old &= !0x02;
        }
        (value & !0x22) | (old & 0x02) | (old & value & 0x20)
    }

    pub fn schedule_write(&mut self, addr: u16, value: u8) -> bool {
        if !self.lcdc.lcd_enabled {
            return false;
        }
        let old = self.read_naive(addr);
        let writes: [Option<(u8, u8)>; 2] = match (addr, self.model.is_cgb(), self.double_speed) {
            (0xFF47..=0xFF49, false, _) => [Some((value | old, 2)), Some((value, 3))],
            (0xFF47..=0xFF49, true, false) => [Some((value, 2)), None],
            (0xFF40, false, _) if (old ^ value) & 0x80 == 0 => {
                [Some(((value & !0x01) | (old & 0x01), 2)), Some((value, 3))]
            }
            (0xFF42, false, _) => [Some((value, 2)), None],
            (0xFF43, false, _) => [Some((value, 2)), None],
            (0xFF43, true, true) => [Some((value, 1)), None],
            (0xFF4B, true, false) => [Some((value, 5)), None],
            (0xFF40, true, false) if old & !value & 0x10 != 0 => [Some((value, 4)), None],
            (0xFF40, true, false) if (old ^ value) & 0x04 != 0 => [Some((value, 5)), None],
            _ => return false,
        };
        let mut slots = self.timed_writes.iter_mut().filter(|w| w.is_none());
        for (value, dot) in writes.into_iter().flatten() {
            let Some(slot) = slots.next() else {
                return false;
            };
            let merge = addr == 0xFF40 && dot == 2;
            *slot = Some(TimedWrite {
                addr,
                value,
                dot,
                merge,
            });
        }
        true
    }

    pub fn speed_switched(&mut self, double_speed: bool, ic: &mut impl ICInterface) {
        if !self.lcdc.lcd_enabled {
            return;
        }
        if double_speed {
            for _ in 0..DOUBLE_SPEED_SWITCH_DOTS {
                self.dot(ic, false);
            }
        } else {
            self.skip_dot = true;
        }
    }

    pub fn dot(&mut self, ic: &mut impl ICInterface, _oam_dma: bool) {
        let wx_just_changed = self.wx_just_changed;
        let tile_sel_glitch = self.tile_sel_glitch;
        self.line_dot_counter += 1;
        if self.line_dot_counter == LINE_DOTS {
            self.line_dot_counter = 0;
            self.first_line = false;
            self.line += 1;
            if self.line == 154 {
                self.line = 0;
                self.fetcher.reset_frame();
            }
        }

        let t = self.line_dot_counter;
        if self.drawing {
            self.mode3_step(t);
        } else if self.line < 144 {
            self.visible_line_dot(t, ic);
        } else if self.line < 153 {
            self.vblank_line_dot(t, ic);
        } else {
            self.last_line_dot(t, ic);
        }
        if wx_just_changed {
            self.wx_just_changed = false;
        }
        if tile_sel_glitch {
            self.tile_sel_glitch = false;
        }
    }

    fn visible_line_dot(&mut self, t: u16, ic: &mut impl ICInterface) {
        if self.first_line {
            match t {
                84 => self.access.oam_write = true,
                86 => {
                    self.stat.ppu_mode = PpuMode::Drawing;
                    self.irq_mode = Some(PpuMode::Drawing);
                    self.access.oam_write = true;
                    self.access.oam_read = true;
                    let blocked = !self.model.is_cgb() || self.double_speed;
                    self.access.vram_read = blocked;
                    self.access.vram_write = blocked;
                }
                88 => self.access.palettes = true,
                FIRST_LINE_MODE3_START => {
                    self.access.vram_read = true;
                    self.access.vram_write = true;
                    self.start_mode3(t);
                }
                _ => self.hblank_dot_events(t, ic),
            }
            return;
        }

        let cgb = self.model.is_cgb();
        match t {
            0 => {
                self.access.oam_write = cgb && !self.double_speed;
                self.check_window_condition();
            }
            2 => self.access.oam_write = cgb,
            3 => {
                self.ly = self.line;
                self.access.oam_read = !self.double_speed || cgb;
                if self.line != 0 {
                    self.ly_compare = None;
                    self.irq_mode = Some(PpuMode::OamScan);
                    self.stat.ppu_mode = PpuMode::HBlank;
                } else {
                    self.ly_compare = Some(0);
                    if !cgb {
                        self.stat.ppu_mode = PpuMode::HBlank;
                    }
                }
                self.stat_update(ic);
            }
            4 => {
                self.access.oam_read = true;
                self.access.oam_write = true;
                self.stat.ppu_mode = PpuMode::OamScan;
                self.irq_mode = Some(PpuMode::OamScan);
                self.ly_compare = Some(self.line);
                self.check_window_condition();
                self.stat_update(ic);
                self.irq_mode = None;
                self.stat_update(ic);
                self.scanner.reset();
                if cgb {
                    self.scan_oam_entry(0);
                }
            }
            5..=84 => {
                if t & 1 == 0 {
                    let index = (t - 4) / 2;
                    if cgb {
                        if index < 40 {
                            self.scan_oam_entry(index as u8);
                        }
                    } else {
                        self.scan_oam_entry(index as u8 - 1);
                    }
                }
                if t == 80 {
                    self.access.vram_read = !cgb;
                    self.access.vram_write = false;
                    self.access.palettes = false;
                    self.access.oam_write = cgb;
                } else if t == 84 {
                    self.stat.ppu_mode = PpuMode::Drawing;
                    self.irq_mode = Some(PpuMode::Drawing);
                    self.access = AccessBlocks {
                        oam_read: true,
                        oam_write: true,
                        vram_read: true,
                        vram_write: true,
                        palettes: false,
                    };
                    self.stat_update(ic);
                }
            }
            87 => self.access.palettes = true,
            MODE3_START => self.start_mode3(t),
            _ => self.hblank_dot_events(t, ic),
        }
    }

    fn hblank_dot_events(&mut self, t: u16, ic: &mut impl ICInterface) {
        if t == self.hblank_dot + 1 {
            self.stat.ppu_mode = PpuMode::HBlank;
            self.irq_mode = Some(PpuMode::HBlank);
            self.access = AccessBlocks::default();
            self.stat_update(ic);
        } else if t == self.hblank_dot + 3 {
            self.access.palettes = !self.double_speed;
        } else if t == self.hblank_dot + 5 {
            self.access.palettes = false;
        } else if t == LINE_DOTS - 2 && self.line != 143 {
            self.irq_mode = Some(PpuMode::OamScan);
        }
    }

    fn vblank_line_dot(&mut self, t: u16, ic: &mut impl ICInterface) {
        match t {
            0 => {
                self.ly_compare = None;
                self.stat_update(ic);
            }
            2 => {
                self.ly = self.line;
                if self.line == 144 && !self.stat_line_previous && self.stat.mode2_interrupt {
                    ic.request_interrupt(Interrupt::Lcd);
                }
            }
            4 => {
                self.ly_compare = Some(self.line);
                self.stat_update(ic);
            }
            5 if self.line == 144 => {
                self.stat.ppu_mode = PpuMode::VBlank;
                ic.request_interrupt(Interrupt::VBlank);
                if !self.stat_line_previous && self.stat.mode2_interrupt {
                    ic.request_interrupt(Interrupt::Lcd);
                }
                self.irq_mode = Some(PpuMode::VBlank);
                self.stat_update(ic);
                self.finish_lcd_on_frame();
                self.frame_ready = true;
            }
            _ => {}
        }
    }

    fn last_line_dot(&mut self, t: u16, ic: &mut impl ICInterface) {
        let cgb = self.model.is_cgb();
        match t {
            0 => {
                self.ly_compare = None;
                self.stat_update(ic);
            }
            2 => self.ly = 153,
            4 if cgb => {
                self.ly_compare = Some(153);
                self.stat_update(ic);
            }
            6 if !cgb => {
                self.ly = 0;
                self.ly_compare = Some(153);
                self.stat_update(ic);
            }
            8 => {
                self.ly = 0;
                self.ly_compare = if cgb { Some(153) } else { None };
                self.stat_update(ic);
            }
            12 => {
                self.ly_compare = Some(0);
                self.stat_update(ic);
            }
            _ => {}
        }
    }

    fn start_mode3(&mut self, t: u16) {
        self.fifo.start_scanline();
        self.fifo.position = POSITION_START;
        self.fetcher.state = PixelFetcherState::GetTile1;
        self.fetcher.object = Default::default();
        self.drawing = true;
        self.mode3_step(t);
    }

    fn mode3_step(&mut self, t: u16) {
        match self.fetcher.object.step {
            ObjectStep::Idle => {
                if self.check_window_start() {
                    return;
                }
                self.check_pixel_insert();
                if self.start_object_fetch() {
                    self.advance_object_fetch();
                    return;
                }
            }
            ObjectStep::Overlay => {
                self.overlay_object();
                if self.start_object_fetch() {
                    self.advance_object_fetch();
                    return;
                }
            }
            _ => {
                self.advance_object_fetch();
                return;
            }
        }

        self.render_pixel();
        self.advance_fetcher();
        if self.fifo.position == 160 {
            self.end_mode3(t);
        }
    }

    fn check_window_start(&mut self) -> bool {
        if self.fetcher.window_mode || !self.fetcher.wy_triggered || !self.lcdc.window_enable {
            return false;
        }
        let cgb = self.model.is_cgb();
        let position = self.fifo.position;
        let activate = if self.wx == 0 {
            position == (-7i8) as u8
                || (position == POSITION_START && self.scx & 7 != 0)
                || ((-15i8) as u8..=(-8i8) as u8).contains(&position)
        } else if self.wx < 166 + cgb as u8 {
            self.wx == position.wrapping_add(7)
                || (!cgb && self.wx == position.wrapping_add(6) && !self.wx_just_changed)
        } else {
            false
        };

        if activate {
            self.fetcher.wl = self.fetcher.wl.wrapping_add(1);
            self.fetcher.window_x = 0;
            self.fifo.bg.clear();
            self.fetcher.window_mode = true;
            self.fetcher.state = PixelFetcherState::GetTile1;
            self.fifo.window_fetching = true;
            return self.wx == 0 && self.scx & 7 != 0;
        } else if !cgb && self.wx == 166 && self.wx == position.wrapping_add(7) {
            self.fetcher.wl = self.fetcher.wl.wrapping_add(1);
        }
        false
    }

    fn check_pixel_insert(&mut self) {
        if self.wx == self.fifo.position.wrapping_add(7)
            && self.fetcher.window_mode
            && !self.fifo.window_fetching
            && self.fetcher.state == PixelFetcherState::GetTile1
            && self.fifo.bg.len() == 8
        {
            self.fifo.insert_pixel = true;
        }
    }

    fn end_mode3(&mut self, t: u16) {
        self.drawing = false;
        self.hblank_dot = t;
        if self.line < 144 && self.fifo.lcd_x < 160 {
            let y = self.line as usize;
            while self.fifo.lcd_x < 160 {
                let x = self.fifo.lcd_x as usize;
                let color = if x == 0 {
                    self.backdrop_color()
                } else {
                    self.frame.get_xy(x - 1, y)
                };
                self.frame.set_xy(x, y, color);
                self.fifo.lcd_x += 1;
            }
        }
        if !self.model.is_cgb()
            && self.fetcher.wy_triggered
            && self.lcdc.window_enable
            && self.wx == 166
        {
            self.fetcher.window_mode = true;
            self.fetcher.window_x = 1;
            self.fetcher.wl = self.fetcher.wl.wrapping_add(1);
        } else {
            self.fetcher.window_mode = false;
        }
        if !self.double_speed {
            self.stat.ppu_mode = PpuMode::HBlank;
            self.irq_mode = Some(PpuMode::HBlank);
            self.access = AccessBlocks {
                oam_read: self.model.is_cgb(),
                ..AccessBlocks::default()
            };
        }
    }

    pub fn check_lyc(&mut self) {
        if !self.lcdc.lcd_enabled {
            return;
        }
        match self.ly_compare {
            Some(line) => {
                let equal = line == self.lyc;
                self.stat.lyc_equals_ly = equal;
                self.lyc_line = equal;
            }
            None if !self.model.is_cgb() => self.stat.lyc_equals_ly = false,
            None => {}
        }
    }

    fn current_stat_line(&self) -> bool {
        let mode_line = match self.irq_mode {
            Some(PpuMode::HBlank) => self.stat.mode0_interrupt,
            Some(PpuMode::VBlank) => self.stat.mode1_interrupt,
            Some(PpuMode::OamScan) => self.stat.mode2_interrupt,
            _ => false,
        };
        mode_line || (self.stat.lyc_interrupt && self.lyc_line)
    }

    pub fn evaluate_stat_interrupts(&mut self, ic: &mut impl ICInterface) {
        if !self.lcdc.lcd_enabled {
            return;
        }
        if std::mem::take(&mut self.stat_write_glitch) {
            let written = self.stat;
            self.stat = (u8::from(self.stat) | 0x78).into();
            self.raise_stat_line(ic);
            self.stat = written;
        }
        self.raise_stat_line(ic);
    }

    fn raise_stat_line(&mut self, ic: &mut impl ICInterface) {
        let current_line = self.current_stat_line();
        if !self.stat_line_previous && current_line {
            ic.request_interrupt(Interrupt::Lcd);
        }
        self.stat_line_previous = current_line;
    }

    fn stat_update(&mut self, ic: &mut impl ICInterface) {
        self.check_lyc();
        self.evaluate_stat_interrupts(ic);
    }

    pub fn check_window_condition(&mut self) {
        if !self.lcdc.lcd_enabled || !self.lcdc.window_enable {
            return;
        }
        let comparison = match self.ly_compare {
            Some(line) if !self.model.is_cgb() || self.double_speed => line,
            _ => self.line,
        };
        if self.wy == comparison {
            self.fetcher.wy_triggered = true;
        }
    }

    pub fn cpu_read_blocked(&self, addr: u16) -> bool {
        if !self.lcdc.lcd_enabled {
            return false;
        }
        match addr {
            0x8000..=0x9FFF => self.access.vram_read,
            0xFE00..=0xFE9F => self.access.oam_read,
            _ => false,
        }
    }

    pub fn cpu_write_blocked(&self, addr: u16) -> bool {
        if !self.lcdc.lcd_enabled {
            return false;
        }
        match addr {
            0x8000..=0x9FFF => self.access.vram_write,
            0xFE00..=0xFE9F => self.access.oam_write,
            _ => false,
        }
    }

    pub fn set_cgb_mode(&mut self, cgb_mode: bool) {
        if self.model.is_cgb() {
            self.cgb_mode = cgb_mode;
        }
    }

    pub fn cgb_mode(&self) -> bool {
        self.cgb_mode
    }

    pub fn backdrop_color(&self) -> RGBA {
        if self.cgb_mode {
            return RGBA::rgb(0xFF, 0xFF, 0xFF);
        }
        match self.dmg_colors {
            DmgColors::Theme => self.dmg_theme.palette()[0],
            DmgColors::PaletteRam => self.cgb_color(&self.bg_palette_ram, 0, 0),
        }
    }

    fn palettes_locked(&self) -> bool {
        self.lcdc.lcd_enabled && self.access.palettes
    }

    pub fn frame(&self) -> &Framebuffer {
        &self.frame
    }

    pub fn soft_reset(&mut self) {
        let previous = std::mem::replace(self, Self::new(self.model));
        self.dmg_theme = previous.dmg_theme;
        self.dmg_colors = previous.dmg_colors;
        self.color_correction = previous.color_correction;
        if !previous.cgb_mode {
            self.cgb_mode = false;
            self.bg_palette_ram = previous.bg_palette_ram;
            self.obj_palette_ram = previous.obj_palette_ram;
            self.opri = previous.opri;
        }
    }

    pub fn load_boot_vram(&mut self, logo: &[u8], dmg_tilemap: bool) {
        let doubled = |nibble: u8| {
            (0..4).fold(0u8, |acc, bit| {
                let set = (nibble >> (3 - bit)) & 1;
                acc << 2 | set << 1 | set
            })
        };
        let rows = logo
            .iter()
            .flat_map(|&byte| {
                let high = doubled(byte >> 4);
                let low = doubled(byte & 0x0F);
                [high, high, low, low]
            })
            .chain(BOOT_TRADEMARK);
        for (i, row) in rows.enumerate() {
            self.vram[0][0x10 + i * 2] = row;
        }
        if dmg_tilemap {
            self.vram[0][0x1910] = 0x19;
            for tile in 1..=12u8 {
                self.vram[0][0x1903 + tile as usize] = tile;
                self.vram[0][0x1923 + tile as usize] = tile + 12;
            }
        }
    }

    pub fn clear_frame(&mut self) {
        self.frame.clear_with_test_pattern();
    }

    pub fn power_on(&mut self) {
        self.lcdc = 0x00.into();
        self.lcd_off();
        self.bgp = 0x00;
        if self.model.is_dmg() {
            self.obp0 = 0xFF;
            self.obp1 = 0xFF;
        }
    }

    fn lcd_off(&mut self) {
        self.ly = 0;
        self.line = 0;
        self.line_dot_counter = 0;
        self.stat.ppu_mode = PpuMode::HBlank;
        self.irq_mode = None;
        self.ly_compare = Some(0);
        self.access = AccessBlocks::default();
        self.drawing = false;
        self.first_line = false;
        self.fetcher.wy_triggered = false;
        let blank = self.backdrop_color();
        self.frame.fill(blank);
    }

    fn lcd_on(&mut self) {
        self.ly = 0;
        self.line = 0;
        self.line_dot_counter = LCD_ON_DOT;
        self.first_line = true;
        self.drawing = false;
        self.hblank_dot = 0;
        self.stat.ppu_mode = PpuMode::HBlank;
        self.irq_mode = None;
        self.ly_compare = Some(0);
        self.access = AccessBlocks::default();
        self.fetcher.reset_frame();
        self.scanner.reset();
        self.fifo.position = POSITION_START;
        self.lcd_on_frame = true;
        if self.model.is_cgb() && self.held_frame.is_none() {
            self.held_frame = Some(self.frame.clone());
        }
    }

    fn finish_lcd_on_frame(&mut self) {
        if !self.lcd_on_frame {
            return;
        }
        self.lcd_on_frame = false;
        if let Some(held) = self.held_frame.take() {
            self.frame = held;
        } else if !self.model.is_cgb() {
            let blank = self.backdrop_color();
            self.frame.fill(blank);
        }
    }
}

impl ReadMemory for Ppu {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => {
                if self.model.is_cgb() {
                    self.vram[(self.vbk & 1) as usize][(addr - 0x8000) as usize]
                } else {
                    self.vram[0][(addr - 0x8000) as usize]
                }
            }
            0xFE00..=0xFE9F => self.oam[(addr - 0xFE00) as usize],
            0xFF40 => self.lcdc.into(),
            0xFF41 => self.stat.into(),
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 if self.line_dot_counter == 2 && self.lcdc.lcd_enabled && !self.first_line => {
                self.line.wrapping_sub(1) & self.line
            }
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            0xFF4F => {
                if self.model.is_cgb() {
                    self.vbk | 0xFE // only bit 0 readable
                } else {
                    0xFF
                }
            }
            0xFF68 => {
                if self.model.is_cgb() {
                    self.bcps | 0x40
                } else {
                    0xFF
                }
            }
            0xFF69 => {
                if self.model.is_cgb() && !self.palettes_locked() {
                    self.bg_palette_ram[(self.bcps & 0x3F) as usize]
                } else {
                    0xFF
                }
            }
            0xFF6A => {
                if self.model.is_cgb() {
                    self.ocps | 0x40
                } else {
                    0xFF
                }
            }
            0xFF6B => {
                if self.model.is_cgb() && !self.palettes_locked() {
                    self.obj_palette_ram[(self.ocps & 0x3F) as usize]
                } else {
                    0xFF
                }
            }
            0xFF6C if self.model.is_cgb() => self.opri | 0xFE,
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Ppu {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => {
                if self.model.is_cgb() {
                    self.vram[(self.vbk & 1) as usize][(addr - 0x8000) as usize] = value
                } else {
                    self.vram[0][(addr - 0x8000) as usize] = value;
                }
            }
            0xFE00..=0xFE9F => self.oam[(addr - 0xFE00) as usize] = value,
            0xFF40 => {
                let was_enabled = self.lcdc.lcd_enabled;
                if !self.model.is_cgb() && self.lcdc.obj_enable && value & 0x02 == 0 {
                    self.abort_object_fetch();
                }
                if self.lcdc.window_enable && value & 0x20 == 0 && self.fifo.window_fetching {
                    self.fifo.window_glitch_disabled = true;
                }

                self.lcdc = value.into();
                let is_enabled = self.lcdc.lcd_enabled;

                if was_enabled && !is_enabled {
                    self.lcd_off();
                } else if !was_enabled && is_enabled {
                    self.lcd_on();
                }
                self.check_window_condition();
            }
            0xFF41 => {
                self.stat = ((u8::from(self.stat) & 0x87) | (value & 0x78)).into();
                self.stat_write_glitch = !self.model.is_cgb();
            }
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {} // LY is read-only
            0xFF45 => self.lyc = value,
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
            0xFF4A => {
                self.wy = value;
                self.check_window_condition();
            }
            0xFF4B => {
                self.wx = value;
                self.wx_just_changed = !self.model.is_cgb();
            }
            0xFF4F => {
                if self.cgb_mode {
                    self.vbk = value & 0x01
                } else if self.model.is_dmg() {
                    self.vbk = value
                }
            }
            0xFF68 => self.bcps = value,
            0xFF69 => {
                let index = (self.bcps & 0x3F) as usize;
                if !self.palettes_locked() {
                    self.bg_palette_ram[index] = value;
                }
                if self.bcps & 0x80 != 0 {
                    self.bcps = 0x80 | ((self.bcps.wrapping_add(1)) & 0x3F);
                }
            }
            0xFF6A => self.ocps = value,
            0xFF6B => {
                let index = (self.ocps & 0x3F) as usize;
                if !self.palettes_locked() {
                    self.obj_palette_ram[index] = value;
                }
                if self.ocps & 0x80 != 0 {
                    self.ocps = 0x80 | ((self.ocps.wrapping_add(1)) & 0x3F);
                }
            }
            0xFF6C => self.opri = value & 0x01,
            _ => {}
        }
    }
}

#[cfg(feature = "serde")]
mod serde_oam {
    use super::OAM_SIZE;
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    pub fn serialize<S: Serializer>(oam: &[u8; OAM_SIZE], s: S) -> Result<S::Ok, S::Error> {
        oam.as_slice().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; OAM_SIZE], D::Error> {
        let vec: Vec<u8> = Vec::deserialize(d)?;
        vec.try_into()
            .map_err(|_| D::Error::custom("OAM size mismatch during deserialization"))
    }
}

#[cfg(feature = "serde")]
mod serde_vram {
    use super::VRAM_BANK_SIZE;
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    pub fn serialize<S: Serializer>(
        vram: &[[u8; VRAM_BANK_SIZE]; 2],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        (&vram[0][..], &vram[1][..]).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<[[u8; VRAM_BANK_SIZE]; 2], D::Error> {
        let (b0, b1): (Vec<u8>, Vec<u8>) = Deserialize::deserialize(d)?;

        let bank0 = b0
            .try_into()
            .map_err(|_| D::Error::custom("VRAM Bank 0 size mismatch"))?;
        let bank1 = b1
            .try_into()
            .map_err(|_| D::Error::custom("VRAM Bank 1 size mismatch"))?;

        Ok([bank0, bank1])
    }
}

#[cfg(feature = "serde")]
mod serde_64 {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    pub fn serialize<S: Serializer>(arr: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        arr.as_slice().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        let vec: Vec<u8> = Vec::deserialize(d)?;
        vec.try_into()
            .map_err(|_| D::Error::custom("Size mismatch (expected 64)"))
    }
}
