use crate::gb::GbModel;
use crate::gb::ic::{ICInterface, Interrupt};
use crate::gb::ppu::colors::ColorCache;
use crate::gb::ppu::fetcher::{ObjectStep, PixelFetcher, PixelFetcherState};
use crate::gb::ppu::fifo::{FifoStart, POSITION_START, PixelFifo};
use crate::gb::ppu::scanner::{OamLines, OamScanner};
use crate::gb::ppu::types::theme::DmgTheme;
use crate::{ReadMemory, WriteMemory};
use types::color::{ColorCorrection, DmgColors, RGBA};
use types::framebuffer::Framebuffer;
use types::lcdc::LCDC;
use types::mode::PpuMode;
use types::stat::STAT;

mod colors;
pub mod compat;
mod fetcher;
mod fifo;
mod scanner;
pub mod types;

pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;
const VRAM_BANK_SIZE: usize = 0x2000; // 8KiB
const OAM_SIZE: usize = 160; // Bytes

#[derive(Clone)]
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
    #[cfg_attr(feature = "serde", serde(skip, default))]
    pub(crate) blind: bool,
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
    #[cfg_attr(feature = "serde", serde(default))]
    frame_dots: u32,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    held_frame: Option<Framebuffer>,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    ahead_end: u16,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    line_ahead: Option<LineStart>,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    pub(crate) per_dot: bool,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    colors: ColorCache,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    scan_lazy: bool,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    scan_done: u16,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    quiet_until: u16,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    access_behind: bool,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    access_done: u16,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    access_next: u16,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    access_clear: u16,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    oam_lines: OamLines,
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

#[derive(Debug, Clone)]
struct LineStart {
    dot: u16,
    fetcher: PixelFetcher,
    fifo: FifoStart,
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct AccessBlocks {
    oam_read: bool,
    oam_write: bool,
    vram_read: bool,
    vram_write: bool,
    palettes: bool,
}

impl AccessBlocks {
    #[inline(always)]
    fn pass(&mut self, from: u16, to: u16, cgb: bool, hblank: Option<(u16, bool)>) {
        let passed = |t: u16| from < t && t <= to;
        if passed(80) {
            self.vram_read = !cgb;
            self.vram_write = false;
            self.palettes = false;
            self.oam_write = cgb;
        }
        if passed(87) {
            self.palettes = true;
        }
        if let Some((hblank_dot, double_speed)) = hblank {
            if passed(hblank_dot + 3) {
                self.palettes = !double_speed;
            }
            if passed(hblank_dot + 5) {
                self.palettes = false;
            }
        }
    }
}

const BOOT_TRADEMARK: [u8; 8] = [0x3C, 0x42, 0xB9, 0xA5, 0xB9, 0xA5, 0x42, 0x3C];
const LINE_DOTS: u16 = 456;
const FRAME_DOTS: u32 = 70224;
const LCD_ON_FRAME_END_DOTS: u32 = 10 * LINE_DOTS as u32;
const LCD_ON_DOT: u16 = 7;
const MODE3_START: u16 = 89;
const FIRST_LINE_MODE3_START: u16 = 91;
const DMG_BOOT_LINE: u8 = 153;
const DMG_BOOT_DOT: u16 = 351;
const CGB_BOOT_LINE: u8 = 145;
const CGB_BOOT_DOT: u16 = 7;
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
            blind: false,
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
            frame_dots: 0,
            held_frame: None,
            ahead_end: 0,
            line_ahead: None,
            per_dot: false,
            colors: ColorCache::default(),
            scan_lazy: false,
            scan_done: 0,
            quiet_until: 0,
            access_behind: false,
            access_done: 0,
            access_next: 0,
            access_clear: 0,
            oam_lines: OamLines::default(),
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

    pub fn line(&self) -> u8 {
        self.line
    }

    #[inline(always)]
    pub fn cycle(&mut self, ic: &mut impl ICInterface, oam_dma: bool, double_speed: bool) -> bool {
        let dots = if double_speed { 2 } else { 4 };
        if self.line_dot_counter + dots <= self.quiet_until {
            self.skip_cached(dots, double_speed);
            return false;
        }
        self.cycle_slow(ic, oam_dma, double_speed)
    }

    #[inline(always)]
    fn skip_cached(&mut self, dots: u16, double_speed: bool) {
        debug_assert!(dots <= self.count_quiet_dots());
        debug_assert_eq!(self.double_speed, double_speed);
        self.frame_dots += dots as u32;
        self.line_dot_counter += dots;
    }

    #[inline(never)]
    fn cycle_slow(&mut self, ic: &mut impl ICInterface, oam_dma: bool, double_speed: bool) -> bool {
        let mode = self.stat.ppu_mode;
        self.catch_up_access();
        if self.cycle_dots(ic, oam_dma, double_speed) {
            self.cache_quiet_stepped();
        } else {
            self.cache_quiet();
        }
        self.hblank_started(mode)
    }

    fn hblank_started(&self, mode: PpuMode) -> bool {
        mode != PpuMode::HBlank && self.stat.ppu_mode == PpuMode::HBlank && self.ly < 144
    }

    #[inline(always)]
    fn cache_quiet(&mut self) {
        self.quiet_until =
            if self.lcdc.lcd_enabled && !self.wx_just_changed && !self.tile_sel_glitch {
                self.line_dot_counter + self.quiet_dots_now()
            } else {
                0
            };
        self.mark_access_behind(self.quiet_until);
    }

    #[inline(always)]
    fn cache_quiet_stepped(&mut self) {
        debug_assert!(self.lcdc.lcd_enabled && !self.wx_just_changed && !self.tile_sel_glitch);
        debug_assert!(self.timed_writes[0].is_none() && !self.skip_dot);
        self.quiet_until = self.line_dot_counter + self.quiet_line_dots();
        self.mark_access_behind(self.quiet_until);
    }

    fn mark_access_behind(&mut self, to: u16) {
        if self.access_behind || to <= self.access_clear {
            return;
        }
        match self.first_access_event(self.line_dot_counter, to) {
            Some(next) => {
                self.access_behind = true;
                self.access_done = self.line_dot_counter;
                self.access_next = next;
            }
            None => self.access_clear = to,
        }
    }

    fn first_access_event(&self, from: u16, to: u16) -> Option<u16> {
        if self.line >= 144 || self.first_line {
            return None;
        }
        let passed = |t: u16| from < t && t <= to;
        let mut next = u16::MAX;
        if from < 87 {
            if to < 80 {
                return None;
            }
            if from < 80 {
                next = 80;
            } else if to >= 87 {
                next = 87;
            }
        }
        if !self.drawing && to > MODE3_START {
            let (settling, settled) = (self.hblank_dot + 3, self.hblank_dot + 5);
            if passed(settling) {
                next = next.min(settling);
            } else if passed(settled) {
                next = next.min(settled);
            }
        }
        (next != u16::MAX).then_some(next)
    }

    fn hblank_access_events(&self, to: u16) -> Option<(u16, bool)> {
        (!self.drawing && to > MODE3_START).then_some((self.hblank_dot, self.double_speed))
    }

    fn access_after(&self, from: u16, to: u16) -> AccessBlocks {
        let mut access = self.access;
        if self.line < 144 && !self.first_line {
            access.pass(from, to, self.model.is_cgb(), self.hblank_access_events(to));
        }
        access
    }

    fn catch_up_access(&mut self) {
        if !self.access_behind {
            return;
        }
        self.access_behind = false;
        let to = self.line_dot_counter;
        if to < self.access_next || self.line >= 144 || self.first_line {
            return;
        }
        if self.access_next > 87 && !self.drawing && to >= self.hblank_dot + 5 {
            debug_assert!(self.access_done >= 87);
            self.access.palettes = false;
            return;
        }
        let (cgb, hblank) = (self.model.is_cgb(), self.hblank_access_events(to));
        self.access.pass(self.access_done, to, cgb, hblank);
    }

    fn access_now(&self) -> AccessBlocks {
        if !self.access_behind || self.line_dot_counter < self.access_next {
            return self.access;
        }
        self.access_after(self.access_done, self.line_dot_counter)
    }

    fn lazy_access(&self) -> bool {
        !self.per_dot && !self.first_line
    }

    #[inline(always)]
    fn cycle_dots(&mut self, ic: &mut impl ICInterface, oam_dma: bool, double_speed: bool) -> bool {
        if !self.lcdc.lcd_enabled {
            for write in std::mem::take(&mut self.timed_writes).into_iter().flatten() {
                self.write_naive(write.addr, write.value);
            }
            self.frame_dots += if double_speed { 2 } else { 4 };
            if self.frame_dots >= FRAME_DOTS {
                self.end_frame();
            }
            return false;
        }

        self.double_speed = double_speed;
        let mut dots = if double_speed { 2 } else { 4 };
        if self.skip_dot {
            self.skip_dot = false;
            dots -= 1;
        }
        if self.timed_writes[0].is_none() {
            let quiet = self.quiet_dots();
            if quiet >= dots as u16 {
                self.skip_quiet_dots(dots);
                return true;
            }
            self.skip_prefix(quiet as u8);
            for _ in quiet as u8..dots {
                self.dot(ic, oam_dma);
            }
            return true;
        }
        self.catch_up_line();
        for dot in 1..=dots {
            self.dot(ic, oam_dma);
            self.apply_timed_writes(dot);
        }
        for write in self.timed_writes.iter_mut().flatten() {
            write.dot -= dots;
        }
        false
    }

    #[inline(always)]
    pub fn cycle_split<I: ICInterface, R>(
        &mut self,
        ic: &mut I,
        oam_dma: bool,
        double_speed: bool,
        sample_dot: u8,
        at: impl FnOnce(&mut I) -> R,
    ) -> (R, bool) {
        let dots = if double_speed { 2 } else { 4 };
        if self.line_dot_counter + dots <= self.quiet_until {
            self.skip_cached(dots, double_speed);
            return (at(ic), false);
        }
        self.cycle_split_slow(ic, oam_dma, double_speed, sample_dot, at)
    }

    #[inline(never)]
    fn cycle_split_slow<I: ICInterface, R>(
        &mut self,
        ic: &mut I,
        oam_dma: bool,
        double_speed: bool,
        sample_dot: u8,
        at: impl FnOnce(&mut I) -> R,
    ) -> (R, bool) {
        let dots = if double_speed { 2 } else { 4 };
        if sample_dot > dots {
            let hblank = self.cycle_slow(ic, oam_dma, double_speed);
            return (at(ic), hblank);
        }
        let mode = self.stat.ppu_mode;
        self.catch_up_access();
        if !self.lcdc.lcd_enabled || self.timed_writes[0].is_some() || self.skip_dot {
            let hblank = self.cycle_slow(ic, oam_dma, double_speed);
            return (at(ic), hblank);
        }
        self.double_speed = double_speed;
        let quiet = self.quiet_dots();
        if quiet >= dots as u16 {
            self.skip_quiet_dots(dots);
            self.cache_quiet_stepped();
            return (at(ic), false);
        }
        let quiet = quiet as u8;
        self.skip_prefix(quiet);
        for _ in quiet..sample_dot {
            self.dot(ic, oam_dma);
        }
        let result = at(ic);
        for _ in quiet.max(sample_dot)..dots {
            self.dot(ic, oam_dma);
        }
        self.cache_quiet_stepped();
        (result, self.hblank_started(mode))
    }

    #[inline(always)]
    fn skip_prefix(&mut self, dots: u8) {
        if dots == 0 {
            return;
        }
        let to = self.line_dot_counter + u16::from(dots);
        if self.access_behind || to > self.access_clear {
            self.skip_and_catch_up(dots);
            return;
        }
        debug_assert!(self.lcdc.lcd_enabled);
        self.frame_dots += u32::from(dots);
        self.line_dot_counter = to;
        self.wx_just_changed = false;
        self.tile_sel_glitch = false;
    }

    #[inline(never)]
    fn skip_and_catch_up(&mut self, dots: u8) {
        self.skip_lit_dots(dots as u16);
        self.catch_up_access();
    }

    fn skip_quiet_dots(&mut self, dots: u8) {
        self.skip_lit_dots(dots as u16);
    }

    #[inline(always)]
    pub fn quiet_dots(&self) -> u16 {
        if self.quiet_until <= self.line_dot_counter.wrapping_sub(1) {
            return self.count_quiet_dots();
        }
        let quiet = self.quiet_until - self.line_dot_counter;
        debug_assert_eq!(quiet, self.count_quiet_dots());
        quiet
    }

    fn count_quiet_dots(&self) -> u16 {
        self.quiet_dots_now()
    }

    #[inline(always)]
    fn quiet_dots_now(&self) -> u16 {
        if self.timed_writes[0].is_some() {
            return 0;
        }
        if !self.lcdc.lcd_enabled {
            return FRAME_DOTS
                .saturating_sub(self.frame_dots + 1)
                .min(u16::MAX as u32) as u16;
        }
        if self.skip_dot {
            return 0;
        }
        self.quiet_line_dots()
    }

    #[inline(always)]
    fn quiet_line_dots(&self) -> u16 {
        let at = self.line_dot_counter;
        if self.drawing {
            return self.ahead_end.saturating_sub(at + 1);
        }
        let to_line_end = LINE_DOTS - 1 - at;
        let first = at + 1;
        let last_quiet = match self.line {
            0..144 if first > FIRST_LINE_MODE3_START => {
                let settle = if self.lazy_access() { 1 } else { 5 };
                if first > self.hblank_dot + settle {
                    return to_line_end;
                }
                if first <= self.hblank_dot {
                    self.hblank_dot.min(LINE_DOTS - 3)
                } else {
                    0
                }
            }
            0..144 if first > 4 => {
                let lazy = self.lazy_access();
                let mut last = 0;
                if self.scan_lazy {
                    last = if lazy { 83 } else { 79 };
                }
                if lazy && (85..MODE3_START).contains(&first) {
                    last = last.max(MODE3_START - 1);
                }
                last
            }
            144..153 if first > 5 => return to_line_end,
            153 if first > 12 => return to_line_end,
            _ => 0,
        };
        last_quiet.saturating_sub(at).min(to_line_end)
    }

    pub fn skip_quiet(&mut self, dots: u16) {
        if !self.lcdc.lcd_enabled {
            self.frame_dots += dots as u32;
            return;
        }
        self.skip_lit_dots(dots);
    }

    fn skip_lit_dots(&mut self, dots: u16) {
        debug_assert!(self.lcdc.lcd_enabled);
        self.frame_dots += dots as u32;
        self.mark_access_behind(self.line_dot_counter + dots);
        self.line_dot_counter += dots;
        self.wx_just_changed = false;
        self.tile_sel_glitch = false;
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
                if write.addr == 0xFF40 && self.model.is_cgb() && (old ^ value) & 0x10 != 0 {
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
        if !(0xFF40..=0xFF4B).contains(&addr) || !self.lcdc.lcd_enabled {
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
            (0xFF40, true, false) if !old & value & 0x10 != 0 => [Some((value, 4)), None],
            _ => return false,
        };
        self.quiet_until = 0;
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
        self.quiet_until = 0;
        self.access_clear = 0;
        self.catch_up_access();
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

    #[inline(always)]
    pub fn dot(&mut self, ic: &mut impl ICInterface, _oam_dma: bool) {
        self.line_dot_counter += 1;
        self.frame_dots += 1;
        if self.line_dot_counter == LINE_DOTS {
            self.next_line();
        }
        let t = self.line_dot_counter;
        if self.drawing {
            if self.ahead_end == 0 {
                self.check_colors();
                if self.mode3_step() {
                    self.end_mode3(t);
                }
            } else if t == self.ahead_end {
                self.leave_line_ahead(t);
            }
        } else if (5..80).contains(&t) && self.line < 144 && !self.first_line {
            if self.per_dot {
                self.oam_scan_dot(t);
            } else if !self.scan_lazy {
                self.scan_lazy = true;
                self.scan_done = t - 1;
            }
        } else {
            self.idle_dot(t, ic);
        }
        self.wx_just_changed = false;
        self.tile_sel_glitch = false;
    }

    #[inline(never)]
    fn next_line(&mut self) {
        self.irq_mode = self.irq_mode_now();
        self.line_dot_counter = 0;
        self.access_clear = 79;
        self.first_line = false;
        self.line += 1;
        if self.line == 154 {
            self.line = 0;
            self.fetcher.reset_frame();
        }
    }

    #[inline(never)]
    fn idle_dot(&mut self, t: u16, ic: &mut impl ICInterface) {
        if self.line < 144 {
            self.visible_line_dot(t, ic);
        } else if self.line < 153 {
            self.vblank_line_dot(t, ic);
        } else {
            self.last_line_dot(t, ic);
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
                    self.oam_stat_update(ic);
                } else {
                    self.ly_compare = Some(0);
                    if !cgb {
                        self.stat.ppu_mode = PpuMode::HBlank;
                    }
                    self.stat_update(ic);
                }
            }
            4 => self.enter_oam_scan(ic),
            5..=84 => self.late_scan_dot(t, ic),
            85 | 86 | 88 => {}
            87 => {
                self.access.palettes = true;
                self.access_clear = MODE3_START - 1;
            }
            MODE3_START => self.start_mode3(t),
            _ => self.hblank_dot_events(t, ic),
        }
    }

    #[inline(never)]
    fn enter_oam_scan(&mut self, ic: &mut impl ICInterface) {
        self.access.oam_read = true;
        self.access.oam_write = true;
        self.stat.ppu_mode = PpuMode::OamScan;
        self.irq_mode = Some(PpuMode::OamScan);
        self.ly_compare = Some(self.line);
        self.check_window_condition();
        self.oam_stat_update(ic);
        self.irq_mode = None;
        if self.lcdc.lcd_enabled {
            self.stat_line_previous = self.stat.lyc_interrupt && self.lyc_line;
        }
        self.scanner.reset();
        if self.per_dot {
            if self.model.is_cgb() {
                self.scan_oam_entry(0);
            }
        } else {
            self.scan_lazy = true;
            self.scan_done = 3;
        }
    }

    #[inline(never)]
    fn late_scan_dot(&mut self, t: u16, ic: &mut impl ICInterface) {
        if self.scan_lazy && t == 84 && self.scan_done == 3 {
            self.scan_lazy = false;
            self.scan_oam_line();
        } else if self.scan_lazy {
            self.catch_up_scan();
        } else {
            self.oam_scan_dot(t);
        }
        if t == 80 {
            let cgb = self.model.is_cgb();
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
            if !self.stat_settled() {
                self.stat_update(ic);
            }
        }
    }

    #[inline(always)]
    fn oam_scan_dot(&mut self, t: u16) {
        if t & 1 == 0 {
            let index = (t - 4) / 2;
            if self.model.is_cgb() {
                if index < 40 {
                    self.scan_oam_entry(index as u8);
                }
            } else {
                self.scan_oam_entry(index as u8 - 1);
            }
        }
    }

    fn hblank_dot_events(&mut self, t: u16, ic: &mut impl ICInterface) {
        match t.wrapping_sub(self.hblank_dot) {
            1 => {
                self.stat.ppu_mode = PpuMode::HBlank;
                self.irq_mode = Some(PpuMode::HBlank);
                self.access = AccessBlocks::default();
                self.hblank_stat_update(ic);
            }
            3 => self.access.palettes = !self.double_speed,
            5 => self.access.palettes = false,
            _ if t == LINE_DOTS - 2 && self.line != 143 => {
                self.irq_mode = Some(PpuMode::OamScan);
            }
            _ => {}
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
            5 if self.line == 144 => self.enter_vblank(ic),
            _ => {}
        }
    }

    #[inline(never)]
    fn enter_vblank(&mut self, ic: &mut impl ICInterface) {
        self.stat.ppu_mode = PpuMode::VBlank;
        ic.request_interrupt(Interrupt::VBlank);
        if !self.stat_line_previous && self.stat.mode2_interrupt {
            ic.request_interrupt(Interrupt::Lcd);
        }
        self.irq_mode = Some(PpuMode::VBlank);
        self.stat_update(ic);
        self.fill_pending_bg_row();
        self.finish_lcd_on_frame();
        self.end_frame();
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
        self.access_clear = LINE_DOTS - 1;
        self.check_colors();
        if self.per_dot
            || self.timed_writes[0].is_some()
            || self.wx_just_changed
            || self.tile_sel_glitch
        {
            if self.mode3_step() {
                self.end_mode3(t);
            }
            return;
        }
        self.draw_line_ahead(t);
    }

    fn draw_line_ahead(&mut self, start: u16) {
        debug_assert!(self.scanner.is_rewound());
        self.line_ahead = Some(LineStart {
            dot: start,
            fetcher: self.fetcher,
            fifo: self.fifo.line_start(),
        });
        let mut dot = start + self.start_plain_line();
        if self.fifo.position == 160 && dot < LINE_DOTS - 1 {
            self.finish_line_pixels();
            self.ahead_end = dot - 1;
            return;
        }
        loop {
            if self.fetching_object() {
                if self.fetcher.object.step != ObjectStep::Overlay {
                    dot += self.finish_object_fetch(LINE_DOTS - 1 - dot);
                } else if let Some(dots) = self.overlay_step(LINE_DOTS - 1 - dot) {
                    dot += dots;
                } else {
                    break;
                }
            } else if self.span_ready() {
                dot += self.draw_spans();
                if self.fifo.position == 160 && dot < LINE_DOTS - 1 {
                    dot -= 1;
                    break;
                }
            } else if let Some(dots) = self.draw_queued_pixels(LINE_DOTS - 1 - dot) {
                dot += dots;
            } else if dot + 8 < LINE_DOTS - 1
                && let Some(dots) = self.draw_line_end()
            {
                dot += dots - 1;
                break;
            } else if self.mode3_step() {
                break;
            } else {
                dot += 1;
            }
            if dot >= LINE_DOTS - 1 {
                self.restore_line_start();
                if self.mode3_step() {
                    self.end_mode3(start);
                }
                return;
            }
        }
        self.finish_line_pixels();
        self.ahead_end = dot;
    }

    fn leave_line_ahead(&mut self, t: u16) {
        self.ahead_end = 0;
        self.line_ahead = None;
        self.enter_hblank(t);
    }

    fn restore_line_start(&mut self) -> u16 {
        self.ahead_end = 0;
        let start = self
            .line_ahead
            .take()
            .expect("a line drawn ahead keeps its start");
        self.fetcher = start.fetcher;
        self.fifo.restart_line(start.fifo);
        self.scanner.rewind();
        start.dot
    }

    fn catch_up_line(&mut self) {
        if self.ahead_end == 0 {
            return;
        }
        self.catch_up_access();
        let start = self.restore_line_start();
        self.check_colors();
        for _ in start..=self.line_dot_counter {
            self.mode3_step();
        }
    }

    fn catch_up_scan(&mut self) {
        if !self.scan_lazy {
            return;
        }
        self.scan_lazy = false;
        let offset = if self.model.is_cgb() { 4 } else { 6 };
        let start = (self.scan_done + 1).max(offset);
        let end = self.line_dot_counter.min(84);
        if start > end {
            return;
        }
        let first = (start - offset).div_ceil(2);
        let last = ((end - offset) / 2 + 1).min(40);
        self.scan_oam_lines(first as u8..last as u8);
    }

    pub fn settle(&mut self) {
        self.quiet_until = 0;
        self.catch_up_access();
        self.catch_up_scan();
        self.catch_up_line();
        self.fill_pending_bg_row();
        self.irq_mode = self.irq_mode_now();
    }

    pub fn line_pending(&self) -> bool {
        self.ahead_end != 0
            || self.fifo.bg_row_pending
            || self.scan_lazy
            || self.access_behind
            || self.irq_mode_now() != self.irq_mode
    }

    fn irq_mode_now(&self) -> Option<PpuMode> {
        if self.line < 143 && self.line_dot_counter >= LINE_DOTS - 2 {
            Some(PpuMode::OamScan)
        } else {
            self.irq_mode
        }
    }

    fn span_ready(&self) -> bool {
        let p = self.fifo.position;
        if p > 151
            || self.fetcher.state != PixelFetcherState::GetTile1
            || self.fifo.bg.len() != 8
            || self.fetcher.object.step != ObjectStep::Idle
            || self.fifo.insert_pixel
        {
            return false;
        }
        if self.wx_matters() && (p + 6..=p + 15).contains(&self.wx) {
            return false;
        }
        !self.objects_fetchable()
            || self
                .scanner
                .next_at_or_after(p + 8)
                .is_none_or(|x| x > p + 15)
    }

    fn wx_matters(&self) -> bool {
        self.wx_matters_at(self.fifo.position)
    }

    fn wx_matters_at(&self, p: u8) -> bool {
        if self.fetcher.window_mode {
            return p >= 160 || self.wx > p + 7 || (self.wx == p + 7 && !self.fifo.window_fetching);
        }
        self.fetcher.wy_triggered && (self.lcdc.window_enable || !self.model.is_cgb())
    }

    fn draw_spans(&mut self) -> u16 {
        if self.fifo.sprite.is_empty() {
            return self.draw_plain_run();
        }
        let p = self.fifo.position;
        self.fifo.window_fetching = false;
        self.fetch_tile_address(p + 1);
        self.render_span_run(8);
        self.fetch_tile_row();
        self.scanner.skip_before(p + 15);
        8
    }

    fn draw_line_end(&mut self) -> Option<u16> {
        let p = self.fifo.position;
        if !(152..160).contains(&p)
            || self.fetcher.state != PixelFetcherState::GetTile1
            || self.fifo.bg.len() != 8
            || !self.fifo.sprite.is_empty()
            || self.fetcher.object.step != ObjectStep::Idle
            || self.fifo.insert_pixel
            || (self.wx_matters() && (p + 6..=167).contains(&self.wx))
            || (self.objects_fetchable()
                && self
                    .scanner
                    .next_at_or_after(p + 8)
                    .is_some_and(|x| x <= 167))
        {
            return None;
        }
        let render_bg = self.cgb_mode || self.lcdc.do_render_bg();
        if p == 152 {
            self.draw_bg_run(render_bg, u8::MAX);
            return Some(8);
        }
        let row = self.line as usize * 160;
        self.fifo.window_fetching = false;
        for x in p..160 {
            let bg = self.fifo.bg.pop().unwrap_or_default();
            if !self.blind && self.fifo.lcd_x < 160 {
                let color_index = if render_bg { bg.color_index } else { 0 };
                let color = self.colors.bg(bg.palette, color_index);
                self.frame.set(row + self.fifo.lcd_x as usize, color);
            }
            self.fifo.lcd_x = self.fifo.lcd_x.wrapping_add(1);
            self.fifo.position = x + 1;
            self.advance_fetcher();
        }
        self.scanner.skip_before(167);
        Some((160 - p) as u16)
    }

    fn draw_queued_pixels(&mut self, limit: u16) -> Option<u16> {
        let p = self.fifo.position;
        let queued = self.fifo.bg.len();
        if queued == 0
            || p >= 160 - queued
            || self.fetcher.object.step != ObjectStep::Idle
            || self.fifo.insert_pixel
        {
            return None;
        }
        let Some(n) = self.queued_run(p, queued) else {
            return self.start_queued_window(p, limit);
        };
        if n == 0 {
            return self.start_queued_object(p, limit);
        }
        self.fifo.window_fetching = false;
        if self.fetcher.state == PixelFetcherState::Push {
            self.render_span_run(n);
            self.advance_fetcher();
        } else {
            for _ in 0..n {
                self.render_span_pixel();
                self.advance_fetcher();
            }
        }
        self.scanner.skip_before(p + n + 7);
        Some(n as u16)
    }

    fn queued_run(&self, p: u8, queued: u8) -> Option<u8> {
        let n = match self.scanner.next_at_or_after(p + 8) {
            Some(x) if self.objects_fetchable() => queued.min(x - p - 8),
            _ => queued,
        };
        if n != 0 && self.wx_matters_at(p) && (p + 6..=p + n + 6).contains(&self.wx) {
            return None;
        }
        Some(n)
    }

    fn start_queued_object(&mut self, p: u8, limit: u16) -> Option<u16> {
        if (self.wx_matters_at(p) && (p + 6..=p + 7).contains(&self.wx))
            || !self.start_object_fetch()
        {
            return None;
        }
        let state = self.fetcher.state;
        let wait = (PixelFetcherState::Push as u16 - state as u16).max(1);
        if wait + 5 <= limit {
            debug_assert!(!self.fifo.bg.is_empty());
            if state == PixelFetcherState::GetTile1 {
                self.fetch_tile_address(p);
            }
            if state != PixelFetcherState::Push {
                self.fetch_tile_row();
            }
            self.read_object_tile();
            return Some(wait + 5);
        }
        self.advance_object_fetch();
        if limit > 1 {
            Some(1 + self.finish_object_fetch(limit - 1))
        } else {
            Some(1)
        }
    }

    #[inline(never)]
    fn start_queued_window(&mut self, p: u8, limit: u16) -> Option<u16> {
        if limit < 6
            || self.wx != p + 7
            || self.fetcher.window_mode
            || !self.fetcher.wy_triggered
            || !self.lcdc.window_enable
        {
            return None;
        }
        debug_assert!(self.wx < 166);
        self.scanner.skip_before(p + 8);
        debug_assert!(
            !self.objects_fetchable() || self.scanner.peek().is_none_or(|o| o.x != p + 8)
        );
        self.fetcher.wl = self.fetcher.wl.wrapping_add(1);
        self.fetcher.window_x = 0;
        self.fifo.bg.clear();
        self.fetcher.window_mode = true;
        self.fifo.window_fetching = true;
        self.fetch_tile_address(p);
        self.fetch_tile_row();
        Some(6)
    }

    #[inline(never)]
    fn overlay_step(&mut self, limit: u16) -> Option<u16> {
        self.overlay_object();
        if self.start_object_fetch() {
            if limit >= 6
                && self.fetcher.state == PixelFetcherState::Push
                && !self.fifo.bg.is_empty()
            {
                self.read_object_tile();
                return Some(6);
            }
            self.advance_object_fetch();
            return Some(1);
        }
        let p = self.fifo.position;
        let queued = self.fifo.bg.len();
        if limit > 1
            && queued >= 2
            && p < 160 - queued
            && !self.fifo.insert_pixel
            && self.fetcher.state == PixelFetcherState::Push
        {
            self.fifo.window_fetching = false;
            if let Some(n) = self.queued_run(p + 1, queued - 1)
                && n != 0
            {
                self.render_span_run(1 + n);
                self.advance_fetcher();
                self.scanner.skip_before(p + n + 8);
                return Some(1 + n as u16);
            }
        }
        self.render_pixel();
        self.advance_fetcher();
        (self.fifo.position != 160).then_some(1)
    }

    fn draw_plain_run(&mut self) -> u16 {
        let next_object = if self.objects_fetchable() {
            self.scanner
                .next_at_or_after(self.fifo.position + 8)
                .unwrap_or(u8::MAX)
        } else {
            u8::MAX
        };
        let render_bg = self.cgb_mode || self.lcdc.do_render_bg();
        self.draw_bg_run(render_bg, next_object)
    }

    fn mode3_step(&mut self) -> bool {
        match self.fetcher.object.step {
            ObjectStep::Idle => {
                if self.fetcher.window_mode {
                    self.check_pixel_insert();
                } else if self.check_window_start() {
                    return false;
                }
                if self.start_object_fetch() {
                    self.advance_object_fetch();
                    return false;
                }
            }
            ObjectStep::Overlay => {
                self.overlay_object();
                if self.start_object_fetch() {
                    self.advance_object_fetch();
                    return false;
                }
            }
            _ => {
                self.advance_object_fetch();
                return false;
            }
        }

        self.render_pixel();
        self.advance_fetcher();
        self.fifo.position == 160
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
        self.finish_line_pixels();
        self.enter_hblank(t);
    }

    fn finish_line_pixels(&mut self) {
        if self.blind {
            self.fifo.lcd_x = self.fifo.lcd_x.max(160);
        } else if self.line < 144 && self.fifo.lcd_x < 160 {
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
    }

    fn enter_hblank(&mut self, t: u16) {
        self.drawing = false;
        self.hblank_dot = t;
        if !self.double_speed {
            self.access_clear = 0;
            self.stat.ppu_mode = PpuMode::HBlank;
            self.irq_mode = Some(PpuMode::HBlank);
            self.access = AccessBlocks {
                oam_read: self.model.is_cgb(),
                ..AccessBlocks::default()
            };
        } else {
            self.access_clear = LINE_DOTS - 1;
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

    fn stat_settled(&self) -> bool {
        let Some(line) = self.ly_compare else {
            return false;
        };
        let equal = line == self.lyc;
        !self.stat_write_glitch
            && self.lyc_line == equal
            && self.stat.lyc_equals_ly == equal
            && self.stat_line_previous == (self.stat.lyc_interrupt && equal)
    }

    fn current_stat_line(&self) -> bool {
        let mode_line = match self.irq_mode_now() {
            Some(PpuMode::HBlank) => self.stat.mode0_interrupt,
            Some(PpuMode::VBlank) => self.stat.mode1_interrupt,
            Some(PpuMode::OamScan) => self.stat.mode2_interrupt,
            _ => false,
        };
        mode_line || (self.stat.lyc_interrupt && self.lyc_line)
    }

    #[inline(always)]
    pub fn evaluate_stat_interrupts(&mut self, ic: &mut impl ICInterface) {
        let stat = self.stat;
        let sources =
            stat.lyc_interrupt | stat.mode2_interrupt | stat.mode1_interrupt | stat.mode0_interrupt;
        if sources || self.stat_line_previous || self.stat_write_glitch {
            self.evaluate_stat_line(ic);
        }
    }

    #[inline(never)]
    fn evaluate_stat_line(&mut self, ic: &mut impl ICInterface) {
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

    #[inline(always)]
    fn oam_stat_update(&mut self, ic: &mut impl ICInterface) {
        debug_assert_eq!(self.irq_mode_now(), Some(PpuMode::OamScan));
        self.check_lyc();
        let stat = self.stat;
        let sources =
            stat.lyc_interrupt | stat.mode2_interrupt | stat.mode1_interrupt | stat.mode0_interrupt;
        if sources || self.stat_line_previous || self.stat_write_glitch {
            self.oam_stat_line(ic);
        }
    }

    #[inline(always)]
    fn hblank_stat_update(&mut self, ic: &mut impl ICInterface) {
        self.check_lyc();
        let stat = self.stat;
        let sources =
            stat.lyc_interrupt | stat.mode2_interrupt | stat.mode1_interrupt | stat.mode0_interrupt;
        if sources || self.stat_line_previous || self.stat_write_glitch {
            self.hblank_stat_line(ic);
        }
    }

    #[inline(never)]
    fn hblank_stat_line(&mut self, ic: &mut impl ICInterface) {
        if self.stat_write_glitch
            || !self.lcdc.lcd_enabled
            || self.line_dot_counter >= LINE_DOTS - 2
        {
            self.evaluate_stat_line(ic);
            return;
        }
        debug_assert_eq!(self.irq_mode_now(), Some(PpuMode::HBlank));
        let stat = self.stat;
        let current_line = stat.mode0_interrupt || (stat.lyc_interrupt && self.lyc_line);
        if !self.stat_line_previous && current_line {
            ic.request_interrupt(Interrupt::Lcd);
        }
        self.stat_line_previous = current_line;
    }

    #[inline(never)]
    fn oam_stat_line(&mut self, ic: &mut impl ICInterface) {
        if self.stat_write_glitch || !self.lcdc.lcd_enabled {
            self.evaluate_stat_line(ic);
            return;
        }
        let stat = self.stat;
        let current_line = stat.mode2_interrupt || (stat.lyc_interrupt && self.lyc_line);
        if !self.stat_line_previous && current_line {
            ic.request_interrupt(Interrupt::Lcd);
        }
        self.stat_line_previous = current_line;
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
            0x8000..=0x9FFF => self.access_now().vram_read,
            0xFE00..=0xFE9F => self.access_now().oam_read,
            _ => false,
        }
    }

    pub fn cpu_write_blocked(&self, addr: u16) -> bool {
        if !self.lcdc.lcd_enabled {
            return false;
        }
        match addr {
            0x8000..=0x9FFF => self.access_now().vram_write,
            0xFE00..=0xFE9F => self.access_now().oam_write,
            _ => false,
        }
    }

    pub fn set_cgb_mode(&mut self, cgb_mode: bool) {
        self.quiet_until = 0;
        self.catch_up_line();
        if self.model.is_cgb() {
            self.cgb_mode = cgb_mode;
        }
    }

    #[cfg(feature = "debug")]
    pub(crate) fn vram_bank(&self) -> u16 {
        if self.model.is_cgb() {
            u16::from(self.vbk & 1)
        } else {
            0
        }
    }

    #[cfg(feature = "debug")]
    pub(crate) fn vram(&self, bank: usize) -> Option<&[u8]> {
        self.vram.get(bank).map(|bytes| &bytes[..])
    }

    #[cfg(feature = "debug")]
    pub(crate) fn vram_mut(&mut self, bank: usize) -> Option<&mut [u8]> {
        self.quiet_until = 0;
        self.catch_up_line();
        self.vram.get_mut(bank).map(|bytes| &mut bytes[..])
    }

    #[cfg(feature = "debug")]
    pub(crate) fn oam(&self) -> &[u8] {
        &self.oam
    }

    #[cfg(feature = "debug")]
    pub(crate) fn oam_mut(&mut self) -> &mut [u8] {
        self.settle();
        self.oam_lines.invalidate();
        &mut self.oam
    }

    #[cfg(feature = "debug")]
    pub(crate) fn dmg_palettes(&self) -> [u8; 3] {
        [self.bgp, self.obp0, self.obp1]
    }

    #[cfg(feature = "debug")]
    pub(crate) fn palette_ram(&self, sprites: bool) -> &[u8; 64] {
        if sprites {
            &self.obj_palette_ram
        } else {
            &self.bg_palette_ram
        }
    }

    #[cfg(feature = "debug")]
    pub(crate) fn palette_color(&self, sprites: bool, palette: u8, index: u8) -> RGBA {
        self.cgb_color(self.palette_ram(sprites), palette, index)
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
        self.lcdc.lcd_enabled && self.access_now().palettes
    }

    fn write_changes_line(&self, addr: u16) -> bool {
        match addr {
            0xFF41 | 0xFF44 | 0xFF45 | 0xFF4F | 0xFF68 | 0xFF6A => false,
            0xFF69 | 0xFF6B => !self.palettes_locked(),
            0xFE00..=0xFE9F => addr & 3 >= 2 && self.scanner.holds(((addr - 0xFE00) / 4) as u8),
            _ => true,
        }
    }

    pub fn frame(&self) -> &Framebuffer {
        &self.frame
    }

    #[cfg(feature = "persistence")]
    pub(crate) fn show_frame(&mut self, pixels: &[u8]) {
        self.frame.copy_from_slice(pixels);
    }

    pub fn soft_reset(&mut self) {
        let previous = std::mem::replace(self, Self::new(self.model));
        self.dmg_theme = previous.dmg_theme;
        self.dmg_colors = previous.dmg_colors;
        self.color_correction = previous.color_correction;
        self.per_dot = previous.per_dot;
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
        self.colors.invalidate();
    }

    fn lcd_off(&mut self) {
        self.ly = 0;
        self.line = 0;
        self.line_dot_counter = 0;
        self.stat.ppu_mode = PpuMode::HBlank;
        self.irq_mode = None;
        self.ly_compare = Some(0);
        self.access = AccessBlocks::default();
        self.access_behind = false;
        self.access_clear = 0;
        self.drawing = false;
        self.first_line = false;
        self.fetcher.wy_triggered = false;
        let blank = self.backdrop_color();
        self.frame.fill(blank);
    }

    fn lcd_on(&mut self) {
        if self.frame_dots > LCD_ON_FRAME_END_DOTS {
            self.end_frame();
        }
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
        self.access_behind = false;
        self.access_clear = 0;
        self.fetcher.reset_frame();
        self.scanner.reset();
        self.fifo.position = POSITION_START;
        self.lcd_on_frame = true;
        if self.model.is_cgb() && self.held_frame.is_none() {
            self.held_frame = Some(self.frame.clone());
        }
    }

    fn end_frame(&mut self) {
        self.frame_ready = true;
        self.frame_dots = 0;
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
        if matches!(addr, 0xFF40 | 0xFF4B) {
            self.quiet_until = 0;
        }
        if self.ahead_end != 0 && self.write_changes_line(addr) {
            self.quiet_until = 0;
            self.catch_up_line();
        }
        if self.scan_lazy && (addr == 0xFF40 || (0xFE00..=0xFE9F).contains(&addr)) {
            self.quiet_until = 0;
            self.catch_up_scan();
        }
        match addr {
            0x8000..=0x9FFF => {
                if self.model.is_cgb() {
                    self.vram[(self.vbk & 1) as usize][(addr - 0x8000) as usize] = value
                } else {
                    self.vram[0][(addr - 0x8000) as usize] = value;
                }
            }
            0xFE00..=0xFE9F => {
                self.oam[(addr - 0xFE00) as usize] = value;
                if addr & 3 == 0 {
                    self.oam_lines.invalidate();
                }
            }
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
            0xFF47 => {
                self.bgp = value;
                self.dmg_palette_written(false);
            }
            0xFF48 => {
                self.obp0 = value;
                self.dmg_palette_written(true);
            }
            0xFF49 => {
                self.obp1 = value;
                self.dmg_palette_written(true);
            }
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
                    self.palette_ram_written(false, index);
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
                    self.palette_ram_written(true, index);
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
pub(crate) fn serialize_settled<S: serde::Serializer>(ppu: &Ppu, s: S) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    if !ppu.line_pending() {
        return ppu.serialize(s);
    }
    let mut settled = ppu.clone();
    settled.settle();
    settled.serialize(s)
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

#[cfg(test)]
mod tests {
    use super::LINE_DOTS;
    use crate::console::{Keys, Rollback};
    use crate::gb::ppu::types::color::RGBA;
    use crate::gb::{GameBoy, GbModel};
    use crate::rom::Rom;

    const TOBU: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb");
    const PETRIS: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/petris.gbc");
    const LIBBET: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/libbet.gb");
    const POSTIE: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/postie.gbc");
    const UCITY: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/ucity.gbc");
    const TOBU_DELUXE: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/tobu_deluxe.gb");
    const LOCKSTEP_FRAMES: usize = 600;

    fn div_to_ppu_phase(model: GbModel, rom: &[u8]) -> i64 {
        let mut gb = GameBoy::new_empty(model);
        gb.load_rom(&Rom::new(rom)).unwrap();
        let dots = gb.ppu.line as i64 * LINE_DOTS as i64 + gb.ppu.line_dot_counter as i64;
        (gb.timer.div as i64 - dots).rem_euclid(4)
    }

    #[test]
    fn without_a_boot_rom_the_ppu_starts_in_the_boot_roms_cycle_phase() {
        assert_eq!(div_to_ppu_phase(GbModel::Dmg, TOBU), 1);
        assert_eq!(div_to_ppu_phase(GbModel::Cgb, TOBU), 1);
        assert_eq!(div_to_ppu_phase(GbModel::Cgb, PETRIS), 1);
    }

    fn boot(model: GbModel, rom: &[u8], per_dot: bool) -> GameBoy {
        let mut gb = GameBoy::new_empty(model);
        gb.load_rom(&Rom::new(rom)).unwrap();
        gb.ppu.per_dot = per_dot;
        gb
    }

    fn next(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    fn assert_same_state(fast: &GameBoy, slow: &GameBoy, scratch: &mut GameBoy, at: &str) {
        assert_eq!(
            fast.state_hash(scratch),
            slow.state_hash(scratch),
            "state hash at {at}"
        );
        assert!(
            fast.encode_state().unwrap() == slow.encode_state().unwrap(),
            "encoded state at {at}"
        );
    }

    fn assert_quiet_skip_exact(fast: &GameBoy, skip: &mut GameBoy, step: &mut GameBoy, at: &str) {
        let quiet = fast.ppu.quiet_dots();
        if quiet == 0 || !fast.ppu.lcdc.lcd_enabled {
            return;
        }
        skip.copy_state_from(fast);
        skip.ppu.clone_from(&fast.ppu);
        skip.ppu.skip_quiet(quiet);
        step.copy_state_from(fast);
        step.ppu.per_dot = true;
        let flag = u8::from(step.ic.flag);
        for _ in 0..quiet {
            step.ppu.dot(&mut step.ic, false);
        }
        step.ppu.per_dot = false;
        assert_eq!(
            u8::from(step.ic.flag),
            flag,
            "interrupt in {quiet} quiet dots at {at}"
        );
        assert!(
            skip.encode_state().unwrap() == step.encode_state().unwrap(),
            "{quiet} quiet dots at {at}"
        );
    }

    fn lockstep(name: &str, model: GbModel, rom: &[u8], mut seed: u64) {
        let mut fast = boot(model, rom, false);
        let mut slow = boot(model, rom, true);
        let mut scratch = boot(model, rom, false);
        let mut spare = boot(model, rom, false);
        let mut ahead_steps = 0u64;
        for frame in 0..LOCKSTEP_FRAMES {
            if frame.is_multiple_of(8) {
                let keys = Keys::from_bits_truncate(next(&mut seed) as u16 & 0xFF);
                fast.set_keys(keys);
                slow.set_keys(keys);
            }
            fast.ppu.frame_ready = false;
            slow.ppu.frame_ready = false;
            let mut step = 0u32;
            while !slow.ppu.frame_ready {
                fast.step();
                slow.step();
                step += 1;
                assert_eq!(
                    (
                        fast.cpu.pc,
                        fast.cycle_counter,
                        fast.ppu.line_dot_counter,
                        u8::from(fast.ppu.stat),
                        u8::from(fast.ic.flag),
                    ),
                    (
                        slow.cpu.pc,
                        slow.cycle_counter,
                        slow.ppu.line_dot_counter,
                        u8::from(slow.ppu.stat),
                        u8::from(slow.ic.flag),
                    ),
                    "{name} frame {frame} step {step}"
                );
                assert_eq!(
                    fast.ppu.access_now(),
                    slow.ppu.access_now(),
                    "{name} frame {frame} step {step} access"
                );
                if next(&mut seed).is_multiple_of(2048) {
                    let at = format!("{name} frame {frame} step {step}");
                    assert_quiet_skip_exact(&fast, &mut scratch, &mut spare, &at);
                }
                if fast.ppu.line_pending() {
                    ahead_steps += 1;
                    let roll = next(&mut seed);
                    if roll.is_multiple_of(512) {
                        let at = format!("{name} frame {frame} step {step}");
                        assert_same_state(&fast, &slow, &mut scratch, &at);
                        if roll & 512 != 0 {
                            fast.ppu.settle();
                        }
                    }
                }
            }
            assert!(fast.ppu.frame_ready, "{name} frame {frame} ended late");
            for gb in [&mut fast, &mut slow] {
                if gb.cycle_counter >= gb.frame_cycles() {
                    gb.cycle_counter -= gb.frame_cycles();
                }
                gb.apu.flush_audio();
            }
            assert!(
                fast.frame().as_slice() == slow.frame().as_slice(),
                "{name} frame {frame} pixels"
            );
        }
        assert!(ahead_steps > 0, "{name} never drew a line ahead");
        assert_same_state(&fast, &slow, &mut scratch, name);
    }

    #[test]
    fn lines_drawn_ahead_match_the_per_dot_ppu_on_dmg() {
        lockstep("tobu", GbModel::Dmg, TOBU, 0x9E37_79B9_7F4A_7C15);
        lockstep("libbet", GbModel::Dmg, LIBBET, 0xD1B5_4A32_D192_ED03);
    }

    #[test]
    fn lines_drawn_ahead_match_the_per_dot_ppu_on_cgb() {
        lockstep("petris", GbModel::Cgb, PETRIS, 0x94D0_49BB_1331_11EB);
        lockstep("postie", GbModel::Cgb, POSTIE, 0xBF58_476D_1CE4_E5B9);
        lockstep("ucity", GbModel::Cgb, UCITY, 0x2545_F491_4F6C_DD1D);
        lockstep("deluxe", GbModel::Cgb, TOBU_DELUXE, 0x6A09_E667_F3BC_C909);
    }

    fn window_scene(cgb: bool, seed: &mut u64) -> Vec<(u16, u8)> {
        let mut writes = Vec::new();
        for bank in 0..1 + cgb as u8 {
            if cgb {
                writes.push((0xFF4F, bank));
            }
            for addr in 0x8000..0xA000u16 {
                writes.push((addr, next(seed) as u8));
            }
        }
        let low_x = next(seed).is_multiple_of(2);
        let objects = next(seed) % 41;
        for i in 0..40u16 {
            let shown = u64::from(i) < objects;
            let y = if shown { (next(seed) % 161) as u8 } else { 0 };
            let x = if low_x {
                (next(seed) % 169) as u8
            } else {
                16 + (next(seed) % 153) as u8
            };
            writes.push((0xFE00 + i * 4, y));
            writes.push((0xFE01 + i * 4, x));
            writes.push((0xFE02 + i * 4, next(seed) as u8));
            writes.push((0xFE03 + i * 4, next(seed) as u8));
        }
        if cgb {
            for (select, data) in [(0xFF68, 0xFF69), (0xFF6A, 0xFF6B)] {
                writes.push((select, 0x80));
                for _ in 0..64 {
                    writes.push((data, next(seed) as u8));
                }
            }
        }
        for addr in [0xFF47, 0xFF48, 0xFF49, 0xFF42, 0xFF43] {
            writes.push((addr, next(seed) as u8));
        }
        writes.push((0xFF4A, (next(seed) % 144) as u8 >> (next(seed) % 3)));
        let wx = match next(seed) % 8 {
            0 => (next(seed) % 24) as u8,
            1 => (next(seed) % 167) as u8,
            _ => 7,
        };
        writes.push((0xFF4B, wx));
        let window = if next(seed).is_multiple_of(8) {
            0
        } else {
            0x20
        };
        writes.push((0xFF40, 0x80 | window | (next(seed) as u8 & 0x5F)));
        writes
    }

    #[test]
    fn window_lines_drawn_ahead_match_the_per_dot_ppu() {
        use crate::WriteMemory;
        let mut seed = 0x3C6E_F372_FE94_F82B;
        for (model, rom) in [(GbModel::Dmg, TOBU), (GbModel::Cgb, PETRIS)] {
            let mut rom = rom.to_vec();
            rom[0x100] = 0x18;
            rom[0x101] = 0xFE;
            for round in 0..32 {
                let mut fast = boot(model, &rom, false);
                let mut slow = boot(model, &rom, true);
                let mut scratch = boot(model, &rom, false);
                for (addr, value) in window_scene(model.is_cgb(), &mut seed) {
                    fast.ppu.write_naive(addr, value);
                    slow.ppu.write_naive(addr, value);
                }
                for frame in 0..2 {
                    let at = format!("{model:?} round {round} frame {frame}");
                    fast.ppu.frame_ready = false;
                    slow.ppu.frame_ready = false;
                    while !slow.ppu.frame_ready {
                        fast.step();
                        slow.step();
                        assert_eq!(
                            (fast.ppu.line_dot_counter, u8::from(fast.ppu.stat)),
                            (slow.ppu.line_dot_counter, u8::from(slow.ppu.stat)),
                            "{at}"
                        );
                        if fast.ppu.line_pending() && next(&mut seed).is_multiple_of(64) {
                            assert_same_state(&fast, &slow, &mut scratch, &at);
                        }
                    }
                    assert!(fast.ppu.frame_ready, "{at} ended late");
                    for gb in [&mut fast, &mut slow] {
                        if gb.cycle_counter >= gb.frame_cycles() {
                            gb.cycle_counter -= gb.frame_cycles();
                        }
                        gb.apu.flush_audio();
                    }
                    assert!(
                        fast.frame().as_slice() == slow.frame().as_slice(),
                        "{at} pixels"
                    );
                }
                assert_same_state(&fast, &slow, &mut scratch, &format!("{model:?} {round}"));
            }
        }
    }

    #[test]
    fn lines_under_a_disabled_window_match_the_per_dot_ppu() {
        use crate::WriteMemory;
        let mut seed = 0x1F83_D9AB_FB41_BD6B;
        for (model, rom) in [(GbModel::Dmg, TOBU), (GbModel::Cgb, PETRIS)] {
            let mut rom = rom.to_vec();
            rom[0x100] = 0x18;
            rom[0x101] = 0xFE;
            for round in 0..32u8 {
                let mut fast = boot(model, &rom, false);
                let mut slow = boot(model, &rom, true);
                let mut scratch = boot(model, &rom, false);
                let lcdc = 0xA0 | (next(&mut seed) as u8 & 0x5F);
                let mut writes = window_scene(model.is_cgb(), &mut seed);
                writes.extend([(0xFF4B, round % 24), (0xFF4A, 0), (0xFF40, lcdc)]);
                for (addr, value) in writes {
                    fast.ppu.write_naive(addr, value);
                    slow.ppu.write_naive(addr, value);
                }
                for frame in 0..2 {
                    let at = format!("{model:?} round {round} frame {frame}");
                    let off_line = (next(&mut seed) % 120) as u8;
                    let poke_line =
                        off_line + 1 + (next(&mut seed) % (143 - off_line as u64)) as u8;
                    let poke_dot = 84 + (next(&mut seed) % 160) as u16;
                    let poke = match next(&mut seed) % 3 {
                        0 => (0xFF4B, (next(&mut seed) % 24) as u8),
                        1 => (0xFF43, next(&mut seed) as u8),
                        _ => (0xFF40, lcdc),
                    };
                    let mut window_on = true;
                    let mut poked = false;
                    fast.ppu.frame_ready = false;
                    slow.ppu.frame_ready = false;
                    while !slow.ppu.frame_ready {
                        if window_on && slow.ppu.line() == off_line {
                            fast.ppu.write_naive(0xFF40, lcdc & !0x20);
                            slow.ppu.write_naive(0xFF40, lcdc & !0x20);
                            window_on = false;
                        }
                        if !poked
                            && slow.ppu.line() == poke_line
                            && slow.ppu.line_dot_counter >= poke_dot
                        {
                            fast.ppu.write_naive(poke.0, poke.1);
                            slow.ppu.write_naive(poke.0, poke.1);
                            poked = true;
                        }
                        fast.step();
                        slow.step();
                        assert_eq!(
                            (
                                fast.ppu.line_dot_counter,
                                u8::from(fast.ppu.stat),
                                u8::from(fast.ic.flag),
                            ),
                            (
                                slow.ppu.line_dot_counter,
                                u8::from(slow.ppu.stat),
                                u8::from(slow.ic.flag),
                            ),
                            "{at}"
                        );
                        assert_eq!(fast.ppu.access_now(), slow.ppu.access_now(), "{at} access");
                        if fast.ppu.line_pending() {
                            let roll = next(&mut seed);
                            if roll.is_multiple_of(64) {
                                assert_same_state(&fast, &slow, &mut scratch, &at);
                                if roll & 64 != 0 {
                                    fast.ppu.settle();
                                }
                            }
                        }
                    }
                    assert!(poked || frame == 0, "{at} never wrote mid-line");
                    assert!(fast.ppu.frame_ready, "{at} ended late");
                    for gb in [&mut fast, &mut slow] {
                        if gb.cycle_counter >= gb.frame_cycles() {
                            gb.cycle_counter -= gb.frame_cycles();
                        }
                        gb.apu.flush_audio();
                        gb.ppu.write_naive(0xFF40, lcdc);
                    }
                    assert!(
                        fast.frame().as_slice() == slow.frame().as_slice(),
                        "{at} pixels"
                    );
                }
                assert_same_state(&fast, &slow, &mut scratch, &format!("{model:?} {round}"));
            }
        }
    }

    #[test]
    fn a_lazy_oam_scan_finds_the_objects_a_per_dot_scan_finds() {
        let mut seed = 0x243F_6A88_85A3_08D3;
        for model in [GbModel::Dmg, GbModel::Cgb] {
            for _ in 0..500 {
                let mut ppu = super::Ppu::new(model);
                ppu.line = (next(&mut seed) % 144) as u8;
                ppu.lcdc.obj_size = next(&mut seed) & 1 == 1;
                for entry in 0..40 {
                    ppu.oam[entry * 4] = (ppu.line + 16).wrapping_sub((next(&mut seed) % 20) as u8);
                    ppu.oam[entry * 4 + 1] = next(&mut seed) as u8;
                }
                ppu.scanner.reset();
                let first = if model.is_cgb() { 3 } else { 4 };
                let from = first + (next(&mut seed) % 82) as u16;
                let to = from + (next(&mut seed) % 12) as u16;
                let mut step = ppu.clone();
                for t in from + 1..=to.min(84) {
                    step.oam_scan_dot(t);
                }
                ppu.scan_lazy = true;
                ppu.scan_done = from;
                ppu.line_dot_counter = to;
                ppu.catch_up_scan();
                assert_eq!(
                    format!("{:?}", ppu.scanner),
                    format!("{:?}", step.scanner),
                    "{model:?} line {} dots {from} to {to}",
                    ppu.line
                );
            }
        }
    }

    const PROGRAM: [u8; 32] = [
        0xF0, 0x44, 0xFE, 0x90, 0x20, 0xFA, 0x3E, 0x00, 0xE0, 0x40, 0x3E, 0xFF, 0xE0, 0x47, 0x3E,
        0x80, 0xE0, 0x68, 0x3E, 0x00, 0xE0, 0x69, 0xE0, 0x69, 0x3E, 0x91, 0xE0, 0x40, 0x18, 0xFE,
        0x00, 0x00,
    ];

    fn lcd_on_rom(cgb: bool) -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
        if cgb {
            data[0x0143] = 0x80;
        }
        data[0x0150..0x0150 + PROGRAM.len()].copy_from_slice(&PROGRAM);
        Rom::new(&data)
    }

    fn next_frame(gb: &mut GameBoy) -> Vec<RGBA> {
        gb.ppu.frame_ready = false;
        while !gb.ppu.frame_ready {
            gb.step();
        }
        let frame = gb.ppu.frame();
        (0..144)
            .flat_map(|y| (0..160).map(move |x| (x, y)))
            .map(|(x, y)| frame.get_xy(x, y))
            .collect()
    }

    fn first_frames_after_lcd_on(model: GbModel) -> (RGBA, Vec<RGBA>, Vec<RGBA>) {
        let mut gb = GameBoy::new_empty(model);
        gb.load_rom(&lcd_on_rom(model.is_cgb())).expect("load");
        while gb.ppu.lcdc.lcd_enabled {
            gb.step();
        }
        let shown = gb.ppu.frame().get_xy(0, 0);
        while !gb.ppu.lcdc.lcd_enabled {
            gb.step();
        }
        let first = next_frame(&mut gb);
        let second = next_frame(&mut gb);
        (shown, first, second)
    }

    #[test]
    fn dmg_shows_the_first_frame_after_lcd_on_blank() {
        let (_, first, second) = first_frames_after_lcd_on(GbModel::Dmg);
        let gb = GameBoy::new_empty(GbModel::Dmg);
        let blank = gb.ppu.backdrop_color();
        assert!(first.iter().all(|&px| px == blank));
        assert!(second.iter().all(|&px| px != blank));
    }

    #[test]
    fn cgb_repeats_the_frame_before_lcd_on() {
        let (shown, first, second) = first_frames_after_lcd_on(GbModel::Cgb);
        let black = RGBA::rgb(0, 0, 0);
        assert_ne!(shown, black);
        assert!(first.iter().all(|&px| px == shown));
        assert!(second.iter().all(|&px| px == black));
    }
}
