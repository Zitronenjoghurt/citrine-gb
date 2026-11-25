use super::interrupt::{Interrupts, Irq};
use crate::gb::ppu::types::color::ColorCorrection;
use crate::ppu2d::blend::{Blend, Effect};
use crate::ppu2d::window::{ALL_LAYERS, Windows};
use affine::Affine;
use background::Background;
use sprite::{Layout, Sprites};
use std::sync::OnceLock;

mod affine;
mod background;
mod fetch;
mod screen;
mod sprite;

pub const WIDTH: usize = 240;
pub const HEIGHT: usize = 160;
pub const LINE_CYCLES: u32 = 1232;
pub const HBLANK_START: u32 = 1007;
pub const LINES: u16 = 228;
pub const VBLANK_LINE: u16 = 160;
const LAST_VBLANK_FLAG_LINE: u16 = 226;
const POWER_ON_LINE: u16 = 225;
const VCOUNT_MATCH_DOT: u32 = 1;
const IRQ_DELAY: u32 = 1;
const VCOUNT_IRQ_DOT: u32 = VCOUNT_MATCH_DOT + IRQ_DELAY;
const CAPTURE_DOT: u32 = 3;
const HBLANK_IRQ_DOT: u32 = HBLANK_START + IRQ_DELAY;
const CAPTURE_LINES: std::ops::Range<u16> = 2..162;
const EVENTS: [u32; 6] = [
    VCOUNT_MATCH_DOT,
    VCOUNT_IRQ_DOT,
    CAPTURE_DOT,
    HBLANK_START,
    HBLANK_IRQ_DOT,
    LINE_CYCLES,
];
const MODE_MASK: u16 = 0b111;
const FORCED_BLANK: u16 = 1 << 7;
const BG0: u16 = 1 << 8;
const OBJ: u16 = 1 << 12;
const WINDOWS: u16 = 0b111 << 13;
const HBLANK_FREE: u16 = 1 << 5;
const OBJ_1D: u16 = 1 << 6;
const OPAQUE: u16 = 1 << 15;
const VBLANK_FLAG: u16 = 1 << 0;
const HBLANK_FLAG: u16 = 1 << 1;
const VCOUNT_FLAG: u16 = 1 << 2;
const VBLANK_IRQ: u16 = 1 << 3;
const HBLANK_IRQ: u16 = 1 << 4;
const VCOUNT_IRQ: u16 = 1 << 5;
const DISPSTAT_WRITABLE: u16 = 0xFF38;
const FIRST_BITMAP_MODE: u8 = 3;
const MOSAIC: u16 = 1 << 6;
const EFFECTS: u8 = 1 << 5;
const OBJ_LAYER: u8 = 4;
const BACKDROP_LAYER: u8 = 5;
const NO_RANK: u32 = 1 << 4;
const BACKDROP_RANK: usize = NO_RANK.trailing_zeros() as usize;
const AFFINE_FIRST: usize = 0x20;
const AFFINE_LAST: usize = 0x3F;
const WINDOW_FIRST: usize = 0x40;
const WINDOW_LAST: usize = 0x4B;
const MOSAIC_REGISTER: usize = 0x4C;
const BLEND_FIRST: usize = 0x50;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Signals {
    pub vblank: Option<u32>,
    pub hblank: Option<u32>,
    pub capture: Option<u32>,
    pub capture_end: bool,
    pub capture_arm: bool,
}

#[derive(Clone, Copy)]
pub struct Memory<'a> {
    pub vram: &'a [u8],
    pub palette: &'a [u8],
    pub oam: &'a [u8],
}

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ppu {
    dispcnt: u16,
    dispstat: u16,
    vcount: u16,
    vcount_match: bool,
    dot: u32,
    backgrounds: [Background; 4],
    affine: [Affine; 2],
    windows: Windows,
    blend: Blend,
    mosaic: u16,
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "fetch::OamWrites::is_empty")
    )]
    oam_writes: fetch::OamWrites,
    #[cfg_attr(feature = "serde", serde(skip, default = "blank_masks"))]
    masks: Box<[u8; WIDTH]>,
    #[cfg_attr(feature = "serde", serde(skip, default = "blank_layers"))]
    layers: Box<[[u16; WIDTH]; 4]>,
    #[cfg_attr(feature = "serde", serde(skip))]
    bg_colors: Box<background::Colors>,
    #[cfg_attr(feature = "serde", serde(skip))]
    sprites: Box<Sprites>,
    #[cfg_attr(feature = "serde", serde(skip))]
    obj_fetch: fetch::ObjFetch,
    #[cfg_attr(feature = "serde", serde(skip, default = "blank_frame"))]
    frame: Box<[u8]>,
    #[cfg_attr(feature = "serde", serde(skip))]
    blind: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    correction: ColorCorrection,
}

impl Default for Ppu {
    fn default() -> Self {
        Self {
            dispcnt: 0,
            dispstat: 0,
            vcount: 0,
            vcount_match: false,
            dot: 0,
            backgrounds: [Background::default(); 4],
            affine: [Affine::default(); 2],
            windows: Windows::default(),
            blend: Blend::default(),
            mosaic: 0,
            oam_writes: fetch::OamWrites::default(),
            masks: blank_masks(),
            layers: blank_layers(),
            bg_colors: Box::default(),
            sprites: Box::default(),
            obj_fetch: fetch::ObjFetch::default(),
            frame: blank_frame(),
            blind: false,
            correction: ColorCorrection::Raw,
        }
    }
}

fn blank_masks() -> Box<[u8; WIDTH]> {
    Box::new([0; WIDTH])
}

fn blank_layers() -> Box<[[u16; WIDTH]; 4]> {
    Box::new([[0; WIDTH]; 4])
}

fn blank_frame() -> Box<[u8]> {
    [0, 0, 0, 0xFF].repeat(WIDTH * HEIGHT).into_boxed_slice()
}

impl Ppu {
    pub(crate) fn clamp_loaded(&mut self) {
        self.dot %= LINE_CYCLES;
        self.vcount %= LINES;
    }

    pub fn copy_state_from(&mut self, source: &Ppu) {
        let Ppu {
            dispcnt,
            dispstat,
            vcount,
            vcount_match,
            dot,
            backgrounds,
            affine,
            windows,
            blend,
            mosaic,
            oam_writes,
            masks: _,
            layers: _,
            sprites,
            obj_fetch,
            frame,
            blind: _,
            correction: _,
            bg_colors,
        } = self;
        *dispcnt = source.dispcnt;
        *dispstat = source.dispstat;
        *vcount = source.vcount;
        *vcount_match = source.vcount_match;
        *dot = source.dot;
        *backgrounds = source.backgrounds;
        *affine = source.affine;
        *windows = source.windows;
        *blend = source.blend;
        *mosaic = source.mosaic;
        oam_writes.clone_from(&source.oam_writes);
        sprites.oam_written();
        obj_fetch.forget();
        frame.copy_from_slice(&source.frame);
        bg_colors.mark_stale();
    }

    pub(crate) fn palette_written(&mut self) {
        self.bg_colors.mark_stale();
    }

    pub fn set_blind(&mut self, blind: bool) {
        self.blind = blind;
    }

    pub fn dispcnt(&self) -> u16 {
        self.dispcnt
    }

    pub fn write_dispcnt(&mut self, value: u16, mask: u16) {
        let old = self.dispcnt;
        self.dispcnt = (self.dispcnt & !mask) | (value & mask);
        if (old ^ self.dispcnt) & (OBJ | HBLANK_FREE) != 0 {
            self.obj_fetch.forget();
        }
    }

    pub fn dispstat(&self) -> u16 {
        let mut flags = 0;
        if (VBLANK_LINE..=LAST_VBLANK_FLAG_LINE).contains(&self.vcount) {
            flags |= VBLANK_FLAG;
        }
        if self.dot >= HBLANK_START {
            flags |= HBLANK_FLAG;
        }
        if self.vcount_match {
            flags |= VCOUNT_FLAG;
        }
        self.dispstat | flags
    }

    pub fn write_dispstat(&mut self, value: u16, mask: u16) {
        let mask = mask & DISPSTAT_WRITABLE;
        self.dispstat = (self.dispstat & !mask) | (value & mask);
    }

    pub fn bgcnt(&self, index: usize) -> u16 {
        self.backgrounds[index].control()
    }

    pub fn write_bgcnt(&mut self, index: usize, value: u16, mask: u16) {
        self.backgrounds[index].write_control(index, value, mask);
    }

    pub fn write_bghofs(&mut self, index: usize, value: u16, mask: u16) {
        self.backgrounds[index].write_hofs(value, mask);
    }

    pub fn write_bgvofs(&mut self, index: usize, value: u16, mask: u16) {
        self.backgrounds[index].write_vofs(value, mask);
    }

    pub fn read_register(&self, offset: usize) -> u16 {
        match offset {
            WINDOW_FIRST..=WINDOW_LAST => self.windows.read(offset - WINDOW_FIRST),
            BLEND_FIRST.. => self.blend.read(offset - BLEND_FIRST),
            _ => 0,
        }
    }

    pub fn write_register(&mut self, offset: usize, value: u16, mask: u16) {
        match offset {
            AFFINE_FIRST..=AFFINE_LAST => {
                let index = (offset - AFFINE_FIRST) / 16;
                self.affine[index].write(offset & 0xF, value, mask);
            }
            WINDOW_FIRST..=WINDOW_LAST => self.windows.write(offset - WINDOW_FIRST, value, mask),
            MOSAIC_REGISTER => self.mosaic = (self.mosaic & !mask) | (value & mask),
            BLEND_FIRST.. => self.blend.write(offset - BLEND_FIRST, value, mask),
            _ => {}
        }
    }

    #[cfg(test)]
    pub fn affine_matrix(&self, index: usize) -> [i16; 4] {
        self.affine[index].matrix()
    }

    #[cfg(feature = "debug")]
    pub fn bg_offsets(&self, index: usize) -> (u16, u16) {
        self.backgrounds[index].offsets()
    }

    pub fn vcount(&self) -> u16 {
        self.vcount
    }

    pub(crate) fn power_on(&mut self) {
        self.vcount = POWER_ON_LINE;
    }

    pub fn mode(&self) -> u8 {
        (self.dispcnt & MODE_MASK) as u8
    }

    pub fn bitmap_mode(&self) -> bool {
        self.mode() >= FIRST_BITMAP_MODE
    }

    pub fn frame(&self) -> &[u8] {
        &self.frame
    }

    #[cfg(feature = "persistence")]
    pub(crate) fn show_frame(&mut self, pixels: &[u8]) {
        self.frame.copy_from_slice(pixels);
    }

    pub fn set_color_correction(&mut self, correction: ColorCorrection) {
        self.correction = correction;
    }

    pub fn skip(&mut self, cycles: u32) -> bool {
        let next = match self.dot {
            CAPTURE_DOT..HBLANK_START => HBLANK_START,
            HBLANK_START => return false,
            HBLANK_IRQ_DOT..LINE_CYCLES => LINE_CYCLES,
            _ => return false,
        };
        let quiet = self.dot + cycles < next;
        if quiet {
            self.dot += cycles;
        }
        quiet
    }

    pub fn until_event(&self) -> u32 {
        let next = EVENTS
            .into_iter()
            .find(|&dot| dot > self.dot)
            .unwrap_or(LINE_CYCLES);
        next - self.dot
    }

    pub fn until_effect(&self, ie: u16, capture: bool, limit: u32) -> u32 {
        let (mut dot, mut vcount, mut matched) = (self.dot, self.vcount, self.vcount_match);
        let lyc = self.lyc();
        let vblank_on = self.irq_on(VBLANK_IRQ, Irq::VBLANK, ie);
        let vcount_on = self.irq_on(VCOUNT_IRQ, Irq::VCOUNT, ie);
        let hblank_on = self.irq_on(HBLANK_IRQ, Irq::HBLANK, ie);
        let mut far = 0;
        while far < limit {
            if dot == 0 && !hblank_on && vcount > VBLANK_LINE {
                let after = (vcount + 1) % LINES;
                let dull = !(vcount == lyc && vcount_on)
                    && !(capture && CAPTURE_LINES.contains(&vcount))
                    && after != CAPTURE_LINES.end
                    && after != CAPTURE_LINES.end + 1;
                if dull {
                    far += LINE_CYCLES;
                    vcount = after;
                    continue;
                }
            }
            let next = EVENTS
                .into_iter()
                .find(|&event| event > dot)
                .unwrap_or(LINE_CYCLES);
            far += next - dot;
            dot = next % LINE_CYCLES;
            let effect = match next {
                VCOUNT_MATCH_DOT => {
                    matched = vcount == lyc;
                    vcount == VBLANK_LINE && vblank_on
                }
                VCOUNT_IRQ_DOT => matched && vcount_on,
                CAPTURE_DOT => capture && CAPTURE_LINES.contains(&vcount),
                HBLANK_START => vcount < VBLANK_LINE,
                HBLANK_IRQ_DOT => hblank_on,
                LINE_CYCLES => {
                    vcount = (vcount + 1) % LINES;
                    vcount == VBLANK_LINE
                        || vcount == CAPTURE_LINES.end
                        || vcount == CAPTURE_LINES.end + 1
                }
                _ => false,
            };
            if effect {
                return far.min(limit);
            }
        }
        limit
    }

    fn irq_on(&self, enable: u16, irq: Irq, ie: u16) -> bool {
        self.dispstat & enable != 0 && ie & irq.bits() != 0
    }

    pub fn until_dma_event(&self) -> u32 {
        let next = [CAPTURE_DOT, HBLANK_START]
            .into_iter()
            .find(|&dot| dot > self.dot)
            .unwrap_or(LINE_CYCLES);
        next - self.dot
    }

    pub fn step(
        &mut self,
        cycles: u32,
        start: u32,
        memory: Memory,
        interrupts: &mut Interrupts,
    ) -> Signals {
        let mut signals = Signals::default();
        let mut left = cycles;
        loop {
            let next = EVENTS
                .into_iter()
                .find(|&dot| dot > self.dot)
                .unwrap_or(LINE_CYCLES);
            let gap = next - self.dot;
            if left < gap {
                self.dot += left;
                return signals;
            }
            left -= gap;
            self.dot = next % LINE_CYCLES;
            let at = start + cycles - left;
            match next {
                VCOUNT_MATCH_DOT => {
                    self.match_vcount();
                    if self.vcount == VBLANK_LINE {
                        self.raise(VBLANK_IRQ, Irq::VBLANK, interrupts, at);
                    }
                }
                VCOUNT_IRQ_DOT if self.vcount_match => {
                    self.raise(VCOUNT_IRQ, Irq::VCOUNT, interrupts, at)
                }
                CAPTURE_DOT if CAPTURE_LINES.contains(&self.vcount) => {
                    signals.capture.get_or_insert(at);
                }
                HBLANK_START => self.enter_hblank(&mut signals, memory, at),
                LINE_CYCLES => self.next_line(&mut signals, at),
                HBLANK_IRQ_DOT => self.raise(HBLANK_IRQ, Irq::HBLANK, interrupts, at),
                _ => {}
            }
        }
    }

    fn lyc(&self) -> u16 {
        self.dispstat >> 8
    }

    fn raise(&self, enable: u16, irq: Irq, interrupts: &mut Interrupts, at: u32) {
        if self.dispstat & enable != 0 {
            interrupts.raise(irq, at);
        }
    }

    fn match_vcount(&mut self) {
        self.vcount_match = self.vcount == self.lyc();
    }

    fn enter_hblank(&mut self, signals: &mut Signals, memory: Memory, at: u32) {
        if self.vcount < VBLANK_LINE {
            self.render_line(memory.vram, memory.palette, memory.oam);
            for affine in &mut self.affine {
                affine.step();
            }
            signals.hblank.get_or_insert(at);
        }
    }

    fn next_line(&mut self, signals: &mut Signals, at: u32) {
        self.vcount = (self.vcount + 1) % LINES;
        self.windows.start_line(self.vcount);
        self.oam_writes.retire(self.vcount);
        if self.vcount == VBLANK_LINE {
            for affine in &mut self.affine {
                affine.latch();
            }
            signals.vblank.get_or_insert(at);
        }
        signals.capture_end |= self.vcount == CAPTURE_LINES.end;
        signals.capture_arm |= self.vcount == CAPTURE_LINES.end + 1;
    }

    fn render_line(&mut self, vram: &[u8], palette: &[u8], oam: &[u8]) {
        if self.blind {
            return;
        }
        let y = usize::from(self.vcount);
        let line = y * WIDTH * 4..(y + 1) * WIDTH * 4;
        if self.dispcnt & FORCED_BLANK != 0 {
            self.frame[line].fill(0xFF);
            return;
        }
        let mode = self.mode();
        let mosaic_width = usize::from(self.mosaic & 0xF) + 1;
        let mosaic_height = usize::from(self.mosaic >> 4 & 0xF) + 1;
        let mut order = [0usize; 4];
        let mut count = 0;
        if mode <= 1 {
            self.bg_colors.refresh(palette);
        }
        for bg in 0..4 {
            if self.dispcnt & (BG0 << bg) == 0 {
                continue;
            }
            let background = self.backgrounds[bg];
            let mosaic = background.control() & MOSAIC != 0;
            let back = if mosaic { y % mosaic_height } else { 0 };
            let layer = &mut self.layers[bg];
            match (mode, bg) {
                (0, _) | (1, 0 | 1) => background.draw_text(y - back, vram, &self.bg_colors, layer),
                (1, 2) | (2, 2 | 3) => {
                    self.affine[bg - 2].draw_tiled(background.control(), back, vram, palette, layer)
                }
                (3..=5, 2) => self.affine[0].draw_bitmap(self.dispcnt, back, vram, palette, layer),
                _ => continue,
            }
            if mosaic && mosaic_width > 1 {
                for x in 0..WIDTH {
                    layer[x] = layer[x - x % mosaic_width];
                }
            }
            order[count] = bg;
            count += 1;
        }
        let order = &mut order[..count];
        order.sort_by_key(|&bg| (self.backgrounds[bg].priority(), bg));
        let sprites = self.dispcnt & OBJ != 0;
        if sprites {
            let layout = Layout {
                one_dimensional: self.dispcnt & OBJ_1D != 0,
                hblank_free: self.dispcnt & HBLANK_FREE != 0,
                bitmap: mode >= FIRST_BITMAP_MODE,
                mosaic: (
                    usize::from(self.mosaic >> 8 & 0xF) + 1,
                    usize::from(self.mosaic >> 12) + 1,
                ),
            };
            self.sprites.draw(y, &layout, vram, palette, oam);
        } else {
            self.sprites.clear();
        }
        self.windows
            .line(self.dispcnt, &self.sprites.window, &mut self.masks);
        let backdrop = [read16(palette, 0); WIDTH];
        let mut compose = Compose {
            ranks: Ranks::new(
                order,
                self.backgrounds.map(|background| background.priority()),
            ),
            colors: Compose::colors(order, &self.layers, &backdrop),
            opaque: [0; WIDTH],
            masks: &self.masks,
            sprites: &self.sprites,
            blend: &self.blend,
        };
        let effect = self.blend.effect();
        let windows = self.dispcnt & WINDOWS != 0;
        let pixels = &mut self.frame[line];
        let table = match self.correction {
            ColorCorrection::Raw => rgba_table(),
            ColorCorrection::Screen => screen::table(),
        };
        compose.draw(pixels, effect, windows, |color| {
            table[usize::from(color & 0x7FFF)]
        });
    }
}

struct Ranks {
    count: usize,
    layers: [u8; 5],
    masks: [u8; 16],
    sprites: [u32; 5],
}

impl Ranks {
    fn new(order: &[usize], priorities: [u8; 4]) -> Self {
        let mut layers = [BACKDROP_LAYER; 5];
        let mut ranked = [0u8; 4];
        for (rank, &bg) in order.iter().enumerate() {
            layers[rank] = bg as u8;
            ranked[bg] = 1 << rank;
        }
        let mut masks = [0; 16];
        for mask in 1..16 {
            masks[mask] = masks[mask & (mask - 1)] | ranked[mask.trailing_zeros() as usize];
        }
        let sprites = std::array::from_fn(|priority| {
            order
                .iter()
                .filter(|&&bg| usize::from(priorities[bg]) < priority)
                .count() as u32
        });
        Self {
            count: order.len(),
            layers,
            masks,
            sprites,
        }
    }
}

struct Compose<'a> {
    ranks: Ranks,
    colors: [&'a [u16; WIDTH]; 5],
    opaque: [u8; WIDTH],
    masks: &'a [u8; WIDTH],
    sprites: &'a Sprites,
    blend: &'a Blend,
}

impl<'a> Compose<'a> {
    fn colors(
        order: &[usize],
        layers: &'a [[u16; WIDTH]; 4],
        backdrop: &'a [u16; WIDTH],
    ) -> [&'a [u16; WIDTH]; 5] {
        let mut colors = [backdrop; 5];
        for (rank, &bg) in order.iter().enumerate() {
            colors[rank] = &layers[bg];
        }
        colors
    }

    #[inline(always)]
    fn draw(
        &mut self,
        line: &mut [u8],
        effect: Effect,
        windows: bool,
        rgba: impl Fn(u16) -> [u8; 4],
    ) {
        if self.sprites.drawn() {
            self.draw_line::<true>(line, effect, windows, &rgba);
        } else {
            self.draw_line::<false>(line, effect, windows, &rgba);
        }
    }

    #[inline(always)]
    fn draw_line<const SPRITES: bool>(
        &mut self,
        line: &mut [u8],
        effect: Effect,
        windows: bool,
        rgba: &impl Fn(u16) -> [u8; 4],
    ) {
        let pixels: &mut [[u8; 4]; WIDTH] = line.as_chunks_mut::<4>().0.try_into().unwrap();
        let blends = effect == Effect::Alpha || self.sprites.any_semi;
        let mask = if windows {
            self.line_mask()
        } else {
            Some(ALL_LAYERS)
        };
        if let Some(mask) = mask
            && (!blends || mask & EFFECTS == 0)
            && self.keeps_top::<SPRITES>(mask, effect)
        {
            self.paint_top::<SPRITES>(pixels, mask, rgba);
            return;
        }
        self.rank_opaque();
        let pixels = pixels.iter_mut().enumerate();
        let Some(mask) = mask else {
            if blends {
                for (x, pixel) in pixels {
                    *pixel = rgba(self.pair::<SPRITES>(x, self.masks[x], effect));
                }
            } else {
                for (x, pixel) in pixels {
                    *pixel = rgba(self.top::<SPRITES>(x, self.masks[x], effect));
                }
            }
            return;
        };
        if blends && mask & EFFECTS != 0 {
            for (x, pixel) in pixels {
                *pixel = rgba(self.blend_pair::<SPRITES>(x, mask, effect));
            }
        } else {
            for (x, pixel) in pixels {
                *pixel = rgba(self.top::<SPRITES>(x, mask, effect));
            }
        }
    }

    fn rank_opaque(&mut self) {
        for (rank, colors) in self.colors[..self.ranks.count].iter().enumerate() {
            for (bits, &value) in self.opaque.iter_mut().zip(colors.iter()) {
                *bits |= ((value >> 15) as u8) << rank;
            }
        }
    }

    #[inline(always)]
    fn keeps_top<const SPRITES: bool>(&self, mask: u8, effect: Effect) -> bool {
        if mask & EFFECTS == 0 || !matches!(effect, Effect::Brighten | Effect::Darken) {
            return true;
        }
        let ranks = self.ranks.masks[usize::from(mask & 0xF)];
        let shown = (0..BACKDROP_RANK)
            .filter(|rank| ranks >> rank & 1 != 0)
            .map(|rank| self.ranks.layers[rank]);
        let sprites = (SPRITES && mask & (1 << OBJ_LAYER) != 0).then_some(OBJ_LAYER);
        !shown
            .chain(sprites)
            .chain([BACKDROP_LAYER])
            .any(|layer| self.blend.first(layer))
    }

    #[inline(always)]
    fn paint_top<const SPRITES: bool>(
        &self,
        pixels: &mut [[u8; 4]; WIDTH],
        mask: u8,
        rgba: &impl Fn(u16) -> [u8; 4],
    ) {
        let sprites = SPRITES && mask & (1 << OBJ_LAYER) != 0;
        let backdrop = self.colors[BACKDROP_RANK];
        let mut shown = [(backdrop, BACKDROP_RANK as u16); 4];
        let mut count = 0;
        let mut ranks = self.ranks.masks[usize::from(mask & 0xF)];
        while ranks != 0 {
            let rank = (u8::BITS - 1 - ranks.leading_zeros()) as usize;
            ranks ^= 1 << rank;
            shown[count] = (self.colors[rank], rank as u16);
            count += 1;
        }
        let mut top = [0; WIDTH];
        let mut first = [BACKDROP_RANK as u16; WIDTH];
        if sprites {
            cover::<true>(&mut top, &mut first, backdrop, &shown[..count]);
        } else {
            cover::<false>(&mut top, &mut first, backdrop, &shown[..count]);
        }
        if sprites {
            for (x, pixel) in pixels.iter_mut().enumerate() {
                let sprite = self.sprites.color[x];
                let shows = sprite & OPAQUE != 0 && u32::from(first[x]) >= self.sprite_rank(x);
                *pixel = rgba(if shows { sprite } else { top[x] });
            }
        } else {
            for (pixel, &color) in pixels.iter_mut().zip(&top) {
                *pixel = rgba(color);
            }
        }
    }

    #[inline(always)]
    fn line_mask(&self) -> Option<u8> {
        let first = self.masks[0];
        let differ = self
            .masks
            .iter()
            .fold(0, |differ, &mask| differ | (mask ^ first));
        (differ == 0).then_some(first)
    }

    #[inline(always)]
    fn entry(&self, rank: u32, x: usize) -> (u16, u8) {
        let rank = rank as usize;
        (self.colors[rank][x], self.ranks.layers[rank])
    }

    #[inline(always)]
    fn visible(&self, x: usize, mask: u8) -> u32 {
        u32::from(self.opaque[x] & self.ranks.masks[usize::from(mask & 0xF)])
    }

    #[inline(always)]
    fn sprite_rank(&self, x: usize) -> u32 {
        self.ranks.sprites[usize::from(self.sprites.priority[x])]
    }

    #[inline(always)]
    fn top<const SPRITES: bool>(&self, x: usize, mask: u8, effect: Effect) -> u16 {
        let first = (self.visible(x, mask) | NO_RANK).trailing_zeros();
        let (mut color, mut layer) = self.entry(first, x);
        let sprite = self.sprites.color[x];
        if SPRITES
            && mask & (1 << OBJ_LAYER) != 0
            && sprite & OPAQUE != 0
            && first >= self.sprite_rank(x)
        {
            color = sprite;
            layer = OBJ_LAYER;
        }
        if mask & EFFECTS == 0 || !self.blend.first(layer) {
            return color;
        }
        match effect {
            Effect::Brighten => self.blend.brighten(color),
            Effect::Darken => self.blend.darken(color),
            _ => color,
        }
    }

    #[inline(always)]
    fn pair<const SPRITES: bool>(&self, x: usize, mask: u8, effect: Effect) -> u16 {
        if mask & EFFECTS == 0 {
            return self.top::<SPRITES>(x, mask, effect);
        }
        self.blend_pair::<SPRITES>(x, mask, effect)
    }

    #[inline(always)]
    fn blend_pair<const SPRITES: bool>(&self, x: usize, mask: u8, effect: Effect) -> u16 {
        let visible = self.visible(x, mask);
        let first = (visible | NO_RANK).trailing_zeros();
        let second = (visible & visible.wrapping_sub(1) | NO_RANK).trailing_zeros();
        let mut found = [self.entry(first, x), self.entry(second, x)];
        let sprite = self.sprites.color[x];
        if SPRITES && mask & (1 << OBJ_LAYER) != 0 && sprite & OPAQUE != 0 {
            let rank = self.sprite_rank(x);
            if first >= rank {
                found = [(sprite, OBJ_LAYER), found[0]];
            } else if second >= rank {
                found[1] = (sprite, OBJ_LAYER);
            }
        }
        let [(top, top_layer), (below, below_layer)] = found;
        let semi = SPRITES && top_layer == OBJ_LAYER && self.sprites.semi[x];
        if semi && self.blend.second(below_layer) {
            self.blend.alpha(top, below)
        } else if !(semi || self.blend.first(top_layer)) {
            top
        } else {
            match effect {
                Effect::Alpha if self.blend.second(below_layer) => self.blend.alpha(top, below),
                Effect::Brighten => self.blend.brighten(top),
                Effect::Darken => self.blend.darken(top),
                _ => top,
            }
        }
    }
}

#[inline(always)]
fn cover<const RANKED: bool>(
    top: &mut [u16; WIDTH],
    first: &mut [u16; WIDTH],
    backdrop: &[u16; WIDTH],
    shown: &[(&[u16; WIDTH], u16)],
) {
    match *shown {
        [] => cover_all::<0, RANKED>(top, first, backdrop, []),
        [a] => cover_all::<1, RANKED>(top, first, backdrop, [a]),
        [a, b] => cover_all::<2, RANKED>(top, first, backdrop, [a, b]),
        [a, b, c] => cover_all::<3, RANKED>(top, first, backdrop, [a, b, c]),
        [a, b, c, d, ..] => cover_all::<4, RANKED>(top, first, backdrop, [a, b, c, d]),
    }
}

#[inline(always)]
fn cover_all<const N: usize, const RANKED: bool>(
    top: &mut [u16; WIDTH],
    first: &mut [u16; WIDTH],
    backdrop: &[u16; WIDTH],
    shown: [(&[u16; WIDTH], u16); N],
) {
    let pixels = top.iter_mut().zip(first.iter_mut()).zip(backdrop);
    for (x, ((top, first), &backdrop)) in pixels.enumerate() {
        let mut color = backdrop;
        let mut rank = BACKDROP_RANK as u16;
        for (colors, layer) in shown {
            let hit = 0u16.wrapping_sub(colors[x] >> 15);
            color = colors[x] & hit | color & !hit;
            rank = layer & hit | rank & !hit;
        }
        *top = color;
        if RANKED {
            *first = rank;
        }
    }
}

fn read16(memory: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([memory[offset], memory[offset + 1]])
}

static RGBA_LOW: [u32; 256] = rgba_byte(0);
static RGBA_HIGH: [u32; 256] = rgba_byte(8);

const fn rgba_byte(shift: u32) -> [u32; 256] {
    let mut table = [0; 256];
    let mut byte = 0;
    while byte < 256 {
        let color = (byte as u32) << shift;
        let spread = color & 0x1F | (color & 0x3E0) << 3 | (color & 0x7C00) << 6;
        table[byte] = spread << 3 | spread >> 2 & 0x0007_0707;
        byte += 1;
    }
    table
}

pub(crate) fn rgba(color: u16) -> [u8; 4] {
    let color = usize::from(color);
    (RGBA_LOW[color & 0xFF] | RGBA_HIGH[color >> 8] | 0xFF00_0000).to_le_bytes()
}

fn rgba_table() -> &'static [[u8; 4]; 1 << 15] {
    static TABLE: OnceLock<Box<[[u8; 4]; 1 << 15]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let colors: Box<[[u8; 4]]> = (0..1 << 15).map(rgba).collect();
        colors.try_into().unwrap()
    })
}

#[cfg(test)]
mod tests {
    use super::{HBLANK_START, LINE_CYCLES, LINES, Ppu};
    use crate::gba::bus::Bus;
    use crate::gba::interrupt::Irq;

    #[test]
    fn a_loaded_dot_or_line_past_the_frame_wraps_into_it() {
        let mut ppu = Ppu {
            dot: u32::MAX,
            vcount: u16::MAX,
            ..Ppu::default()
        };
        ppu.clamp_loaded();
        assert!(ppu.dot < LINE_CYCLES && ppu.vcount < LINES);
        assert!(ppu.until_effect(0xFFFF, true, 4 * LINE_CYCLES) <= 4 * LINE_CYCLES);
    }

    #[test]
    fn a_real_dot_and_line_are_unchanged_by_the_load_clamp() {
        let mut bus = bus();
        bus.step_video(100 * LINE_CYCLES + 517);
        let (dot, vcount) = (bus.ppu().dot, bus.ppu().vcount);
        bus.ppu_mut().clamp_loaded();
        assert_eq!((bus.ppu().dot, bus.ppu().vcount), (dot, vcount));
    }

    mod blend {
        use crate::gba::bus::Bus;
        use crate::gba::ppu::rgba;
        use crate::gba::ppu::tests::{bus, draw, pixel, solid_layers};

        const DISPCNT: u32 = 0x0400_0000;
        const BG0CNT: u32 = 0x0400_0008;
        const BG1CNT: u32 = 0x0400_000A;
        const BLDCNT: u32 = 0x0400_0050;
        const BLDALPHA: u32 = 0x0400_0052;
        const BLDY: u32 = 0x0400_0054;
        const PALETTE: u32 = 0x0500_0000;
        const OAM: u32 = 0x0700_0000;
        const RED: u16 = 0x001F;
        const BLUE: u16 = 0x7C00;
        const LAYERS: u16 = 0b11 << 8;
        const OBJ: u16 = 1 << 12;
        const SEMI: u16 = 1 << 10;

        fn ready() -> Bus {
            let mut bus = bus();
            solid_layers(&mut bus);
            bus.write16(OAM, 1 << 9);
            bus.write16(BG0CNT, 8 << 8);
            bus.write16(BG1CNT, 9 << 8 | 1);
            bus.write16(BLDALPHA, 8 | 8 << 8);
            bus.write16(DISPCNT, LAYERS);
            bus
        }

        #[test]
        fn alpha_blends_a_first_target_over_a_second_target() {
            let mut bus = ready();
            for (control, expected) in [
                (1 | 1 << 6 | 2 << 8, 15 | 15 << 5),
                (1 | 1 << 6, RED),
                (2 | 1 << 6 | 2 << 8, RED),
                (1 | 2 << 8, RED),
            ] {
                bus.write16(BLDCNT, control);
                draw(&mut bus);
                assert_eq!(pixel(&bus, 0, 0), rgba(expected), "{control:#x}");
            }
        }

        #[test]
        fn the_backdrop_can_be_the_second_target() {
            let mut bus = ready();
            bus.write16(PALETTE, BLUE);
            bus.write16(DISPCNT, 1 << 8);
            bus.write16(BLDCNT, 1 | 1 << 6 | 1 << 13);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 0), rgba(15 | 15 << 10));
        }

        #[test]
        fn brightness_applies_to_first_targets_only() {
            let mut bus = ready();
            bus.write16(BLDY, 16);
            bus.write16(BLDCNT, 1 | 3 << 6);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 0), rgba(0));
            bus.write16(BLDCNT, 2 | 3 << 6);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 0), rgba(RED));
        }

        #[test]
        fn a_semi_transparent_sprite_blends_with_a_second_target_whatever_the_effect() {
            let mut bus = ready();
            bus.write16(OAM, SEMI);
            bus.write16(OAM + 4, 0);
            bus.write16(DISPCNT, LAYERS | OBJ);
            bus.write16(BLDY, 16);
            for (control, expected) in [
                (1 << 8, 15 << 10 | 15),
                (1 << 8 | 2 << 6, 15 << 10 | 15),
                (2 << 6, 0x7FFF),
                (0, BLUE),
            ] {
                bus.write16(BLDCNT, control);
                draw(&mut bus);
                assert_eq!(pixel(&bus, 0, 0), rgba(expected), "{control:#x}");
            }
        }

        #[test]
        fn bldcnt_and_bldalpha_read_back_and_bldy_is_write_only() {
            let mut bus = ready();
            bus.write16(BLDCNT, 0xFFFF);
            bus.write16(BLDALPHA, 0xFFFF);
            bus.write16(BLDY, 0xFFFF);
            assert_eq!(bus.read16(BLDCNT), 0x3FFF);
            assert_eq!(bus.read16(BLDALPHA), 0x1F1F);
            assert_eq!(bus.read16(BLDY), 0);
        }
    }
    mod compose {
        use crate::gba::ppu::sprite::Sprites;
        use crate::gba::ppu::{
            BACKDROP_LAYER, Compose, EFFECTS, OBJ_LAYER, OPAQUE, Ranks, WIDTH, rgba,
        };
        use crate::ppu2d::blend::{Blend, Effect};

        const BACKDROP_PRIORITY: u8 = 4;

        struct Reference<'a> {
            order: &'a [usize],
            priorities: [u8; 4],
            layers: &'a [[u16; WIDTH]; 4],
            masks: &'a [u8; WIDTH],
            sprites: &'a Sprites,
            blend: &'a Blend,
            backdrop: u16,
        }

        impl Reference<'_> {
            fn top(&self, x: usize, effect: Effect) -> u16 {
                let mask = self.masks[x];
                let mut color = self.backdrop;
                let mut layer = BACKDROP_LAYER;
                let mut priority = BACKDROP_PRIORITY;
                for &bg in self.order {
                    let value = self.layers[bg][x];
                    if mask & (1 << bg) != 0 && value & OPAQUE != 0 {
                        color = value;
                        layer = bg as u8;
                        priority = self.priorities[bg];
                        break;
                    }
                }
                let sprite = self.sprites.color[x];
                if mask & (1 << OBJ_LAYER) != 0
                    && sprite & OPAQUE != 0
                    && self.sprites.priority[x] <= priority
                {
                    color = sprite;
                    layer = OBJ_LAYER;
                }
                if mask & EFFECTS == 0 || !self.blend.first(layer) {
                    return color;
                }
                match effect {
                    Effect::Brighten => self.blend.brighten(color),
                    Effect::Darken => self.blend.darken(color),
                    _ => color,
                }
            }

            fn pair(&self, x: usize, effect: Effect) -> u16 {
                let mask = self.masks[x];
                let mut found = [(self.backdrop, BACKDROP_LAYER); 2];
                let mut seen = 0;
                let sprite_color = self.sprites.color[x];
                let mut sprite = mask & (1 << OBJ_LAYER) != 0 && sprite_color & OPAQUE != 0;
                let sprite_priority = self.sprites.priority[x];
                for &bg in self.order {
                    if seen == 2 {
                        break;
                    }
                    let value = self.layers[bg][x];
                    if mask & (1 << bg) == 0 || value & OPAQUE == 0 {
                        continue;
                    }
                    if sprite && sprite_priority <= self.priorities[bg] {
                        found[seen] = (sprite_color, OBJ_LAYER);
                        seen += 1;
                        sprite = false;
                        if seen == 2 {
                            break;
                        }
                    }
                    found[seen] = (value, bg as u8);
                    seen += 1;
                }
                if sprite && seen < 2 {
                    found[seen] = (sprite_color, OBJ_LAYER);
                }
                let [(top, top_layer), (below, below_layer)] = found;
                let semi = top_layer == OBJ_LAYER && self.sprites.semi[x];
                if mask & EFFECTS == 0 {
                    top
                } else if semi && self.blend.second(below_layer) {
                    self.blend.alpha(top, below)
                } else if !(semi || self.blend.first(top_layer)) {
                    top
                } else {
                    match effect {
                        Effect::Alpha if self.blend.second(below_layer) => {
                            self.blend.alpha(top, below)
                        }
                        Effect::Brighten => self.blend.brighten(top),
                        Effect::Darken => self.blend.darken(top),
                        _ => top,
                    }
                }
            }
        }

        struct Xorshift(u32);

        impl Xorshift {
            fn word(&mut self) -> u32 {
                self.0 ^= self.0 << 13;
                self.0 ^= self.0 >> 17;
                self.0 ^= self.0 << 5;
                self.0
            }

            fn below(&mut self, n: u32) -> u32 {
                self.word() % n
            }
        }

        #[test]
        fn ranked_layers_compose_like_walking_the_priority_order() {
            let effects = [
                Effect::None,
                Effect::Alpha,
                Effect::Brighten,
                Effect::Darken,
            ];
            for seed in 1..=400u32 {
                let mut rng = Xorshift(seed.wrapping_mul(0x9E37_79B9));
                let density = 1 + rng.below(4);
                let mut layers = Box::new([[0u16; WIDTH]; 4]);
                for value in layers.iter_mut().flatten() {
                    let color = rng.word() as u16 & 0x7FFF;
                    *value = if rng.below(4) < density {
                        color | OPAQUE
                    } else {
                        color
                    };
                }
                let enabled = rng.below(16);
                let priorities: [u8; 4] = std::array::from_fn(|_| rng.below(4) as u8);
                let mut order: Vec<usize> = (0..4).filter(|bg| enabled >> bg & 1 != 0).collect();
                order.sort_by_key(|&bg| (priorities[bg], bg));
                let masks: [u8; WIDTH] = match rng.below(4) {
                    0 => [0x3F; WIDTH],
                    1 => [rng.below(0x40) as u8; WIDTH],
                    _ => std::array::from_fn(|_| rng.below(0x40) as u8),
                };
                let mut sprites = Sprites::default();
                let sprite_density = rng.below(5);
                for x in 0..WIDTH {
                    if rng.below(4) < sprite_density {
                        sprites.color[x] = rng.word() as u16 & 0x7FFF | OPAQUE;
                        sprites.priority[x] = rng.below(4) as u8;
                        sprites.semi[x] = rng.below(3) == 0;
                        sprites.any_semi |= sprites.semi[x];
                    }
                }
                let mut blend = Blend::default();
                blend.write(0, rng.word() as u16, 0xFFFF);
                blend.write(2, rng.word() as u16, 0xFFFF);
                blend.write(4, rng.word() as u16, 0xFFFF);
                let backdrop = rng.word() as u16 & 0x7FFF;
                let reference = Reference {
                    order: &order,
                    priorities,
                    layers: &layers,
                    masks: &masks,
                    sprites: &sprites,
                    blend: &blend,
                    backdrop,
                };
                let line = [backdrop; WIDTH];
                let mut compose = Compose {
                    ranks: Ranks::new(&order, priorities),
                    colors: Compose::colors(&order, &layers, &line),
                    opaque: [0; WIDTH],
                    masks: &masks,
                    sprites: &sprites,
                    blend: &blend,
                };
                let no_sprites = sprite_density == 0;
                for effect in effects {
                    let expected: Vec<[u8; 4]> = (0..WIDTH)
                        .map(|x| rgba(reference.pair(x, effect)))
                        .collect();
                    let all_layers = masks == [0x3F; WIDTH];
                    for (sprites, windows) in
                        [(true, true), (false, true), (true, false), (false, false)]
                    {
                        if (!sprites && !no_sprites) || (!windows && !all_layers) {
                            continue;
                        }
                        let mut drawn = [0u8; WIDTH * 4];
                        if sprites {
                            compose.draw_line::<true>(&mut drawn, effect, windows, &rgba);
                        } else {
                            compose.draw_line::<false>(&mut drawn, effect, windows, &rgba);
                        }
                        let drawn = drawn.as_chunks::<4>().0;
                        assert_eq!(
                            drawn, expected,
                            "seed {seed} {effect:?} {sprites} {windows}"
                        );
                    }
                }
                compose.rank_opaque();
                for (x, &mask) in masks.iter().enumerate() {
                    for effect in effects {
                        let (top, pair) = (reference.top(x, effect), reference.pair(x, effect));
                        let seen = [
                            compose.top::<true>(x, mask, effect),
                            compose.pair::<true>(x, mask, effect),
                        ];
                        assert_eq!(seen, [top, pair], "seed {seed} x {x} {effect:?}");
                        if no_sprites {
                            let seen = [
                                compose.top::<false>(x, mask, effect),
                                compose.pair::<false>(x, mask, effect),
                            ];
                            assert_eq!(seen, [top, pair], "seed {seed} x {x} {effect:?}");
                        }
                    }
                }
            }
        }
    }
    mod window {
        use crate::gba::bus::Bus;
        use crate::gba::ppu::rgba;
        use crate::gba::ppu::tests::{bus, draw, pixel, solid_layers, to_line};

        const DISPCNT: u32 = 0x0400_0000;
        const BG0CNT: u32 = 0x0400_0008;
        const BG1CNT: u32 = 0x0400_000A;
        const WIN0H: u32 = 0x0400_0040;
        const WIN1H: u32 = 0x0400_0042;
        const WIN0V: u32 = 0x0400_0044;
        const WIN1V: u32 = 0x0400_0046;
        const WININ: u32 = 0x0400_0048;
        const WINOUT: u32 = 0x0400_004A;
        const BLDCNT: u32 = 0x0400_0050;
        const BLDALPHA: u32 = 0x0400_0052;
        const BLDY: u32 = 0x0400_0054;
        const OAM: u32 = 0x0700_0000;
        const RED: u16 = 0x001F;
        const GREEN: u16 = 0x03E0;
        const BLUE: u16 = 0x7C00;
        const WHITE: u16 = 0x7FFF;
        const LAYERS: u16 = 0b11 << 8;
        const OBJ: u16 = 1 << 12;
        const WIN0: u16 = 1 << 13;
        const WIN1: u16 = 1 << 14;
        const OBJ_WINDOW: u16 = 1 << 15;
        const SHOW_BG0: u16 = 1;
        const SHOW_BG1: u16 = 2;
        const SHOW_OBJ: u16 = 1 << 4;
        const SHOW_EFFECTS: u16 = 1 << 5;

        fn ready() -> Bus {
            let mut bus = bus();
            solid_layers(&mut bus);
            bus.write16(OAM, 1 << 9);
            bus.write16(BG0CNT, 8 << 8);
            bus.write16(BG1CNT, 9 << 8 | 1);
            bus
        }

        #[test]
        fn without_windows_every_layer_shows() {
            let mut bus = ready();
            bus.write16(WINOUT, 0);
            bus.write16(DISPCNT, LAYERS);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 100, 100), rgba(RED));
        }

        #[test]
        fn win0_shows_its_layers_inside_its_rectangle_and_winout_outside() {
            let mut bus = ready();
            bus.write16(WIN0H, 8 << 8 | 16);
            bus.write16(WIN0V, 4 << 8 | 12);
            bus.write16(WININ, SHOW_BG1);
            bus.write16(WINOUT, SHOW_BG0);
            bus.write16(DISPCNT, LAYERS | WIN0);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 8, 4), rgba(GREEN));
            assert_eq!(pixel(&bus, 15, 11), rgba(GREEN));
            for (x, y) in [(7, 4), (16, 4), (8, 3), (8, 12)] {
                assert_eq!(pixel(&bus, x, y), rgba(RED), "{x},{y}");
            }
        }

        #[test]
        fn a_window_that_shows_nothing_shows_the_backdrop() {
            let mut bus = ready();
            bus.write16(WINOUT, 0);
            bus.write16(DISPCNT, LAYERS | WIN0);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 100, 100), rgba(0));
        }

        #[test]
        fn win0_wins_over_win1_where_they_overlap() {
            let mut bus = ready();
            bus.write16(WIN0H, 16);
            bus.write16(WIN0V, 16);
            bus.write16(WIN1H, 32);
            bus.write16(WIN1V, 32);
            bus.write16(WININ, SHOW_BG0 | SHOW_BG1 << 8);
            bus.write16(WINOUT, 0);
            bus.write16(DISPCNT, LAYERS | WIN0 | WIN1);
            draw(&mut bus);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 0), rgba(RED));
            assert_eq!(pixel(&bus, 20, 0), rgba(GREEN));
            assert_eq!(pixel(&bus, 40, 0), rgba(0));
        }

        #[test]
        fn a_right_edge_left_of_the_left_edge_or_past_240_reaches_the_screen_edge() {
            for h in [200 << 8 | 10, 200 << 8 | 250] {
                let mut bus = ready();
                bus.write16(WININ, SHOW_BG1);
                bus.write16(WINOUT, SHOW_BG0);
                bus.write16(DISPCNT, LAYERS | WIN0);
                bus.write16(WIN0H, h);
                bus.write16(WIN0V, 150 << 8 | 200);
                draw(&mut bus);
                assert_eq!(pixel(&bus, 239, 159), rgba(GREEN), "{h:#x}");
                assert_eq!(pixel(&bus, 5, 159), rgba(RED), "{h:#x}");
                assert_eq!(pixel(&bus, 239, 5), rgba(RED), "{h:#x}");
            }
        }

        #[test]
        fn a_window_opens_on_its_top_line_and_stays_open_until_its_bottom_line() {
            let mut bus = ready();
            bus.write16(WIN0H, 240);
            bus.write16(WIN0V, 150 << 8 | 20);
            bus.write16(WININ, SHOW_BG1);
            bus.write16(WINOUT, SHOW_BG0);
            bus.write16(DISPCNT, LAYERS | WIN0);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 5), rgba(RED));
            assert_eq!(pixel(&bus, 0, 150), rgba(GREEN));
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 5), rgba(GREEN));
            assert_eq!(pixel(&bus, 0, 19), rgba(GREEN));
            assert_eq!(pixel(&bus, 0, 20), rgba(RED));
            assert_eq!(pixel(&bus, 0, 149), rgba(RED));
        }

        #[test]
        fn a_top_line_that_has_already_passed_opens_the_window_only_next_frame() {
            let mut bus = ready();
            bus.write16(WIN0H, 240);
            bus.write16(WININ, SHOW_BG1);
            bus.write16(WINOUT, SHOW_BG0);
            bus.write16(DISPCNT, LAYERS | WIN0);
            to_line(&mut bus, 50);
            bus.write16(WIN0V, 40 << 8 | 100);
            to_line(&mut bus, 0);
            assert_eq!(pixel(&bus, 0, 60), rgba(RED));
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 39), rgba(RED));
            assert_eq!(pixel(&bus, 0, 60), rgba(GREEN));
            assert_eq!(pixel(&bus, 0, 100), rgba(RED));
        }

        #[test]
        fn the_object_window_is_the_opaque_pixels_of_window_sprites() {
            let mut bus = ready();
            bus.write16(OAM, 2 << 10);
            bus.write16(OAM + 2, 0);
            bus.write16(OAM + 4, 0);
            bus.write16(WINOUT, SHOW_BG0 | SHOW_BG1 << 8);
            bus.write16(DISPCNT, LAYERS | OBJ | OBJ_WINDOW);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 0), rgba(GREEN));
            assert_eq!(pixel(&bus, 7, 7), rgba(GREEN));
            assert_eq!(pixel(&bus, 8, 0), rgba(RED));
            bus.write16(DISPCNT, LAYERS | OBJ);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 0), rgba(RED));
        }

        #[test]
        fn windows_hide_sprites_through_the_obj_bit() {
            let mut bus = ready();
            bus.write16(OAM, 0);
            bus.write16(OAM + 4, 0);
            bus.write16(WINOUT, SHOW_BG0);
            bus.write16(DISPCNT, LAYERS | OBJ | WIN0);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 0), rgba(RED));
            bus.write16(WINOUT, SHOW_BG0 | SHOW_OBJ);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 0), rgba(BLUE));
        }

        #[test]
        fn the_effects_bit_switches_blending_per_region() {
            let mut bus = ready();
            bus.write16(BLDCNT, 1 | 2 << 6);
            bus.write16(BLDY, 16);
            bus.write16(WIN0H, 16);
            bus.write16(WIN0V, 16);
            bus.write16(WININ, SHOW_BG0);
            bus.write16(WINOUT, SHOW_BG0 | SHOW_EFFECTS);
            bus.write16(DISPCNT, LAYERS | WIN0);
            draw(&mut bus);
            draw(&mut bus);
            assert_eq!(pixel(&bus, 0, 0), rgba(RED));
            assert_eq!(pixel(&bus, 20, 0), rgba(WHITE));
        }

        #[test]
        fn a_window_over_whole_lines_blends_and_shows_layers_as_its_bits_say() {
            let mut bus = ready();
            bus.write16(BLDALPHA, 8 | 8 << 8);
            bus.write16(BLDCNT, 1 | 1 << 6 | 2 << 8);
            bus.write16(WIN0H, 240);
            bus.write16(WIN0V, 160);
            bus.write16(WINOUT, 0);
            bus.write16(DISPCNT, LAYERS | WIN0);
            for (inside, expected) in [
                (SHOW_BG0 | SHOW_BG1 | SHOW_EFFECTS, 15 | 15 << 5),
                (SHOW_BG0 | SHOW_BG1, RED),
                (SHOW_BG1 | SHOW_EFFECTS, GREEN),
            ] {
                bus.write16(WININ, inside);
                draw(&mut bus);
                draw(&mut bus);
                for x in [0, 119, 239] {
                    assert_eq!(pixel(&bus, x, 80), rgba(expected), "{inside:#x} {x}");
                }
            }
        }

        #[test]
        fn winin_and_winout_read_back_six_bits_per_region_and_the_edges_are_write_only() {
            let mut bus = ready();
            bus.write16(WININ, 0xFFFF);
            bus.write16(WINOUT, 0xFFFF);
            bus.write16(WIN0H, 0x1234);
            assert_eq!(bus.read16(WININ), 0x3F3F);
            assert_eq!(bus.read16(WINOUT), 0x3F3F);
            assert_eq!(bus.read16(WIN0H), 0);
            bus.write8(WINOUT + 1, 0x01);
            assert_eq!(bus.read16(WINOUT), 0x013F);
        }
    }

    const DISPCNT: u32 = 0x0400_0000;
    const DISPSTAT: u32 = 0x0400_0004;
    const VCOUNT: u32 = 0x0400_0006;
    const DMA0: u32 = 0x0400_00B0;
    const PALETTE: u32 = 0x0500_0000;
    const VRAM: u32 = 0x0600_0000;
    const FRAME_CYCLES: u32 = 280_896;
    const RED: u16 = 0x001F;
    const GREEN: u16 = 0x03E0;
    const BLUE: u16 = 0x7C00;
    const BG2: u16 = 1 << 10;

    pub(super) fn bus() -> Bus {
        Bus::new(&[])
    }

    pub(super) fn to_line(bus: &mut Bus, line: u32) {
        let now = u32::from(bus.ppu().vcount());
        bus.step_video((line + 228 - now) % 228 * LINE_CYCLES);
    }

    pub(super) fn pixel(bus: &Bus, x: usize, y: usize) -> [u8; 4] {
        let at = (y * 240 + x) * 4;
        bus.ppu().frame()[at..at + 4].try_into().unwrap()
    }

    pub(super) fn draw(bus: &mut Bus) {
        bus.step_video(FRAME_CYCLES);
    }

    #[test]
    fn vcount_counts_lines_of_1232_cycles_and_wraps_after_228() {
        let mut bus = bus();
        bus.step_video(LINE_CYCLES - 1);
        assert_eq!(bus.read16(VCOUNT), 0);
        bus.step_video(1);
        assert_eq!(bus.read16(VCOUNT), 1);
        bus.step_video(226 * LINE_CYCLES);
        assert_eq!(bus.read16(VCOUNT), 227);
        bus.step_video(LINE_CYCLES);
        assert_eq!(bus.read16(VCOUNT), 0);
    }

    #[test]
    fn a_frame_is_280896_cycles() {
        let mut bus = bus();
        bus.step_video(FRAME_CYCLES - 1);
        assert_eq!(bus.read16(VCOUNT), 227);
        bus.step_video(1);
        assert_eq!(bus.read16(VCOUNT), 0);
        assert_eq!(bus.read16(DISPSTAT) & 0b11, 0);
    }

    #[test]
    fn the_hblank_flag_rises_at_cycle_1007_and_falls_at_the_next_line() {
        let mut bus = bus();
        bus.step_video(HBLANK_START - 1);
        assert_eq!(bus.read16(DISPSTAT) & 0b10, 0);
        bus.step_video(1);
        assert_eq!(bus.read16(DISPSTAT) & 0b10, 0b10);
        bus.step_video(LINE_CYCLES - HBLANK_START - 1);
        assert_eq!(bus.read16(DISPSTAT) & 0b10, 0b10);
        bus.step_video(1);
        assert_eq!(bus.read16(DISPSTAT) & 0b10, 0);
    }

    #[test]
    fn the_hblank_irq_fires_on_every_line_when_enabled() {
        let mut quiet = bus();
        quiet.step_video(HBLANK_START);
        assert_eq!(quiet.interrupts().if_(), 0);
        let mut bus = bus();
        bus.write16(DISPSTAT, 1 << 4);
        to_line(&mut bus, 200);
        bus.write16(0x0400_0202, 0xFFFF);
        bus.step_video(HBLANK_START + 1);
        assert_eq!(bus.interrupts().if_(), 0);
        bus.step_video(1);
        assert_eq!(bus.interrupts().if_(), Irq::HBLANK.bits());
    }

    #[test]
    fn the_vblank_flag_covers_lines_160_to_226() {
        let mut bus = bus();
        to_line(&mut bus, 159);
        assert_eq!(bus.read16(DISPSTAT) & 1, 0);
        to_line(&mut bus, 160);
        assert_eq!(bus.read16(DISPSTAT) & 1, 1);
        to_line(&mut bus, 226);
        assert_eq!(bus.read16(DISPSTAT) & 1, 1);
        to_line(&mut bus, 227);
        assert_eq!(bus.read16(DISPSTAT) & 1, 0);
    }

    #[test]
    fn the_vblank_irq_fires_once_at_line_160_when_enabled() {
        let mut bus = bus();
        to_line(&mut bus, 159);
        bus.step_video(LINE_CYCLES);
        assert_eq!(bus.interrupts().if_(), 0);
        bus.write16(DISPSTAT, 1 << 3);
        to_line(&mut bus, 159);
        bus.write16(0x0400_0202, 0xFFFF);
        bus.step_video(LINE_CYCLES + 1);
        assert_eq!(bus.interrupts().if_(), 0);
        bus.step_video(1);
        assert_eq!(bus.interrupts().if_(), Irq::VBLANK.bits());
        bus.write16(0x0400_0202, 0xFFFF);
        to_line(&mut bus, 159);
        assert_eq!(bus.interrupts().if_(), 0);
    }

    #[test]
    fn the_vcount_flag_and_irq_follow_the_lyc_setting() {
        let mut bus = bus();
        bus.write16(DISPSTAT, (100 << 8) | (1 << 5));
        to_line(&mut bus, 99);
        assert_eq!(bus.read16(DISPSTAT) & 0b100, 0);
        assert_eq!(bus.interrupts().if_(), 0);
        to_line(&mut bus, 100);
        assert_eq!(bus.read16(DISPSTAT) & 0b100, 0);
        bus.step_video(1);
        assert_eq!(bus.read16(DISPSTAT) & 0b100, 0b100);
        bus.step_video(1);
        assert_eq!(bus.interrupts().if_(), 0);
        bus.step_video(1);
        assert_eq!(bus.interrupts().if_(), Irq::VCOUNT.bits());
        to_line(&mut bus, 101);
        bus.step_video(1);
        assert_eq!(bus.read16(DISPSTAT) & 0b100, 0);
    }

    #[test]
    fn dispstat_keeps_its_flags_read_only_and_vcount_ignores_writes() {
        let mut bus = bus();
        to_line(&mut bus, 170);
        bus.write16(DISPSTAT, 0xFFFF);
        assert_eq!(bus.read16(DISPSTAT), 0xFF39);
        bus.write16(DISPSTAT, 0);
        assert_eq!(bus.read16(DISPSTAT), 1);
        bus.write16(VCOUNT, 5);
        assert_eq!(bus.read16(VCOUNT), 170);
    }

    #[test]
    fn vblank_dma_starts_at_line_160() {
        let mut bus = bus();
        bus.write32(DMA0, 0x0300_0000);
        bus.write32(DMA0 + 4, 0x0300_0100);
        bus.write32(DMA0 + 8, 0x8000_0001 | (1 << 28));
        to_line(&mut bus, 159);
        assert!(!bus.dma_active());
        to_line(&mut bus, 160);
        assert!(bus.dma_active());
    }

    #[test]
    fn hblank_dma_starts_on_visible_lines_only() {
        let mut bus = bus();
        bus.write32(DMA0, 0x0300_0000);
        bus.write32(DMA0 + 4, 0x0300_0100);
        bus.write32(DMA0 + 8, 0x8000_0001 | (2 << 28) | (1 << 25));
        bus.step_video(HBLANK_START - 1);
        assert!(!bus.dma_active());
        bus.step_video(1);
        assert!(bus.dma_active());
        to_line(&mut bus, 160);
        bus.run_dma();
        bus.step_video(HBLANK_START);
        assert!(!bus.dma_active());
    }

    #[test]
    fn dma3_video_capture_runs_on_lines_2_to_161_once_armed_at_line_163() {
        let mut bus = bus();
        bus.write32(DMA0 + 36, 0x0300_0000);
        bus.write32(DMA0 + 40, 0x0300_0100);
        bus.write32(DMA0 + 44, 0x8000_0001 | (3 << 28) | (1 << 25));
        to_line(&mut bus, 2);
        bus.step_video(3);
        assert!(!bus.dma_active());
        bus.step_video(LINE_CYCLES - 3);
        to_line(&mut bus, 163);
        to_line(&mut bus, 2);
        bus.step_video(2);
        assert!(!bus.dma_active());
        bus.step_video(1);
        assert!(bus.dma_active());
        bus.run_dma();
        to_line(&mut bus, 161);
        bus.run_dma();
        to_line(&mut bus, 162);
        bus.step_video(3);
        assert!(!bus.dma_active());
        assert_eq!(bus.read16(DMA0 + 46) & 0x8000, 0);
    }

    #[test]
    fn bitmap_modes_drop_vram_past_0x1c000_mirror_below() {
        let mut bus = bus();
        bus.write16(DISPCNT, 3);
        bus.write32(VRAM + 0x1_0000, 0xABAD_1DEA);
        bus.write32(VRAM + 0x1_8000, 0xDEAD_BEEF);
        assert_eq!(bus.read32(VRAM + 0x1_0000), 0xABAD_1DEA);
        assert_eq!(bus.read32(VRAM + 0x1_8000), 0);
        bus.write32(VRAM + 0x1_C000, 0x1234_5678);
        assert_eq!(bus.read32(VRAM + 0x1_4000), 0x1234_5678);
        bus.write16(DISPCNT, 0);
        bus.write32(VRAM + 0x1_8000, 0xDEAD_BEEF);
        assert_eq!(bus.read32(VRAM + 0x1_0000), 0xDEAD_BEEF);
    }

    #[test]
    fn mode_3_draws_one_colour_per_halfword() {
        let mut bus = bus();
        bus.write16(DISPCNT, 3 | BG2);
        bus.write16(VRAM, RED);
        bus.write16(VRAM + 2 * (239 + 240 * 159), BLUE);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), [0xFF, 0, 0, 0xFF]);
        assert_eq!(pixel(&bus, 239, 159), [0, 0, 0xFF, 0xFF]);
        assert_eq!(pixel(&bus, 1, 0), [0, 0, 0, 0xFF]);
    }

    #[test]
    fn mode_4_reads_palette_indices_from_the_selected_frame() {
        let mut bus = bus();
        bus.write16(PALETTE + 2, GREEN);
        bus.write16(PALETTE + 4, BLUE);
        bus.write16(VRAM + 240 * 10, 0x0201);
        bus.write16(VRAM + 0xA000 + 240 * 10, 0x0102);
        bus.write16(DISPCNT, 4 | BG2);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 10), [0, 0xFF, 0, 0xFF]);
        assert_eq!(pixel(&bus, 1, 10), [0, 0, 0xFF, 0xFF]);
        bus.write16(DISPCNT, 4 | BG2 | (1 << 4));
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 10), [0, 0, 0xFF, 0xFF]);
        assert_eq!(pixel(&bus, 1, 10), [0, 0xFF, 0, 0xFF]);
    }

    #[test]
    fn mode_5_draws_160_by_128_and_the_backdrop_around_it() {
        let mut bus = bus();
        bus.write16(PALETTE, GREEN);
        bus.write16(VRAM + 2 * (159 + 160 * 127), RED);
        bus.write16(VRAM + 0xA000, BLUE);
        bus.write16(DISPCNT, 5 | BG2);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 159, 127), [0xFF, 0, 0, 0xFF]);
        assert_eq!(pixel(&bus, 160, 127), [0, 0xFF, 0, 0xFF]);
        assert_eq!(pixel(&bus, 0, 128), [0, 0xFF, 0, 0xFF]);
        bus.write16(DISPCNT, 5 | BG2 | (1 << 4));
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), [0, 0, 0xFF, 0xFF]);
    }

    #[test]
    fn forced_blank_draws_white() {
        let mut bus = bus();
        bus.write16(VRAM, RED);
        bus.write16(DISPCNT, 3 | BG2 | (1 << 7));
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), [0xFF; 4]);
    }

    #[test]
    fn tiled_modes_and_a_hidden_bg2_draw_the_backdrop() {
        let mut bus = bus();
        bus.write16(PALETTE, RED);
        bus.write16(VRAM, BLUE);
        bus.write16(DISPCNT, 3);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), [0xFF, 0, 0, 0xFF]);
        bus.write16(DISPCNT, BG2);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), [0xFF, 0, 0, 0xFF]);
    }

    #[test]
    fn byte_writes_to_bitmap_vram_reach_the_second_frame() {
        let mut bus = bus();
        bus.write16(DISPCNT, 4 | BG2);
        bus.write8(VRAM + 0x1_2000, 0x05);
        assert_eq!(bus.read16(VRAM + 0x1_2000), 0x0505);
    }

    const BG0CNT: u32 = 0x0400_0008;
    const OBJ_VRAM: u32 = 0x0601_0000;
    const OAM: u32 = 0x0700_0000;
    const BG0: u16 = 1 << 8;
    const OBJ: u16 = 1 << 12;

    pub(super) fn solid_layers(bus: &mut Bus) {
        for half in 0..16 {
            bus.write16(VRAM + 32 + half * 2, 0x1111);
            bus.write16(VRAM + 64 + half * 2, 0x2222);
            bus.write16(OBJ_VRAM + half * 2, 0x3333);
        }
        for (block, tile) in [(8, 1), (9, 2)] {
            for entry in 0..1024 {
                bus.write16(VRAM + block * 0x800 + entry * 2, tile);
            }
        }
        bus.write16(PALETTE + 2, RED);
        bus.write16(PALETTE + 4, GREEN);
        bus.write16(PALETTE + 0x200 + 6, BLUE);
        for index in 1..128 {
            bus.write16(OAM + index * 8, 1 << 9);
        }
    }

    fn top(bus: &mut Bus, bg0: u16, bg1: u16, obj: Option<u16>) -> [u8; 4] {
        bus.write16(BG0CNT, bg0 | 8 << 8);
        bus.write16(BG0CNT + 2, bg1 | 9 << 8);
        let layers = match obj {
            Some(priority) => {
                bus.write16(OAM + 4, priority << 10);
                BG0 | BG0 << 1 | OBJ
            }
            None => BG0 | BG0 << 1,
        };
        bus.write16(DISPCNT, layers);
        draw(bus);
        pixel(bus, 0, 0)
    }

    #[test]
    fn text_bg_colors_follow_palette_writes_and_state_copies() {
        let show_bg0 = |bus: &mut Bus| {
            bus.write16(BG0CNT, 8 << 8);
            bus.write16(DISPCNT, BG0);
            draw(bus);
            pixel(bus, 0, 0)
        };
        let mut first = bus();
        solid_layers(&mut first);
        assert_eq!(show_bg0(&mut first), super::rgba(RED));
        first.write16(PALETTE + 2, BLUE);
        assert_eq!(show_bg0(&mut first), super::rgba(BLUE));
        first.write8(PALETTE + 2, 0x1F);
        assert_eq!(show_bg0(&mut first), super::rgba(0x1F1F));
        let mut other = bus();
        solid_layers(&mut other);
        assert_eq!(show_bg0(&mut other), super::rgba(RED));
        other.copy_state_from(&first);
        assert_eq!(show_bg0(&mut other), super::rgba(0x1F1F));
    }

    #[test]
    fn the_bg_with_the_lower_priority_value_is_in_front() {
        let mut bus = bus();
        solid_layers(&mut bus);
        assert_eq!(top(&mut bus, 1, 0, None), super::rgba(GREEN));
        assert_eq!(top(&mut bus, 0, 1, None), super::rgba(RED));
    }

    #[test]
    fn on_equal_priority_the_lower_bg_number_is_in_front() {
        let mut bus = bus();
        solid_layers(&mut bus);
        assert_eq!(top(&mut bus, 2, 2, None), super::rgba(RED));
    }

    #[test]
    fn a_sprite_is_in_front_of_bgs_of_its_own_priority_or_lower() {
        let mut bus = bus();
        solid_layers(&mut bus);
        assert_eq!(top(&mut bus, 1, 1, Some(1)), super::rgba(BLUE));
        assert_eq!(top(&mut bus, 1, 3, Some(2)), super::rgba(RED));
        assert_eq!(top(&mut bus, 3, 1, Some(2)), super::rgba(GREEN));
        assert_eq!(top(&mut bus, 3, 3, Some(0)), super::rgba(BLUE));
    }

    #[test]
    fn a_sprite_shows_through_mode_4_index_0() {
        let mut bus = bus();
        solid_layers(&mut bus);
        for half in 0..16 {
            bus.write16(OBJ_VRAM + 0x4000 + half * 2, 0x3333);
        }
        bus.write16(OAM + 4, 512 | 3 << 10);
        bus.write16(VRAM + 2, 0x0101);
        bus.write16(DISPCNT, 4 | BG2 | OBJ);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), super::rgba(BLUE));
        assert_eq!(pixel(&bus, 2, 0), super::rgba(RED));
        assert_eq!(pixel(&bus, 8, 0), super::rgba(0));
    }

    const MOSAIC: u32 = 0x0400_004C;

    fn marked_tiles(bus: &mut Bus) {
        for row in 0..8 {
            let first = if row == 0 { 0x2221 } else { 0x2222 };
            bus.write16(VRAM + 96 + row * 4, first);
            bus.write16(VRAM + 96 + row * 4 + 2, 0x2222);
            bus.write16(OBJ_VRAM + 32 + row * 4, first);
            bus.write16(OBJ_VRAM + 32 + row * 4 + 2, 0x2222);
        }
        for entry in 0..1024 {
            bus.write16(VRAM + 10 * 0x800 + entry * 2, 3);
        }
        bus.write16(PALETTE + 0x200 + 2, BLUE);
        bus.write16(PALETTE + 0x200 + 4, GREEN);
    }

    #[test]
    fn bg_mosaic_repeats_the_top_left_pixel_of_each_block() {
        let mut bus = bus();
        solid_layers(&mut bus);
        marked_tiles(&mut bus);
        bus.write16(MOSAIC, 7 | 7 << 4);
        bus.write16(DISPCNT, BG0);
        bus.write16(BG0CNT, 10 << 8);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 1, 0), super::rgba(GREEN));
        bus.write16(BG0CNT, 10 << 8 | 1 << 6);
        draw(&mut bus);
        for (x, y) in [(0, 0), (7, 0), (0, 7), (7, 7)] {
            assert_eq!(pixel(&bus, x, y), super::rgba(RED), "{x},{y}");
        }
        bus.write16(MOSAIC, 3 | 1 << 4);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 3, 1), super::rgba(RED));
        assert_eq!(pixel(&bus, 4, 0), super::rgba(GREEN));
        assert_eq!(pixel(&bus, 0, 2), super::rgba(GREEN));
    }

    #[test]
    fn sprite_mosaic_uses_its_own_block_size() {
        let mut bus = bus();
        solid_layers(&mut bus);
        marked_tiles(&mut bus);
        bus.write16(OAM + 4, 1);
        bus.write16(MOSAIC, 3 << 8 | 3 << 12);
        bus.write16(DISPCNT, OBJ);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 1, 0), super::rgba(GREEN));
        bus.write16(OAM, 1 << 12);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 3, 3), super::rgba(BLUE));
        assert_eq!(pixel(&bus, 4, 0), super::rgba(GREEN));
        assert_eq!(pixel(&bus, 0, 4), super::rgba(GREEN));
    }

    #[test]
    fn rgba_widens_every_channel_of_every_color() {
        for color in 0..=u16::MAX {
            let channel = |shift: u16| {
                let value = ((color >> shift) & 0x1F) as u8;
                value << 3 | value >> 2
            };
            assert_eq!(
                super::rgba(color),
                [channel(0), channel(5), channel(10), 0xFF],
                "{color:#06x}"
            );
        }
    }

    #[test]
    fn rgba_widens_each_channel_by_repeating_its_top_bits() {
        for color in 0..=u16::MAX {
            let wide = |shift: u16| {
                let channel = (color >> shift & 0x1F) as u8;
                channel << 3 | channel >> 2
            };
            assert_eq!(
                super::rgba(color),
                [wide(0), wide(5), wide(10), 0xFF],
                "{color:#06x}"
            );
        }
    }

    #[test]
    fn the_rgba_table_matches_rgba_for_every_color() {
        let table = super::rgba_table();
        for color in 0..=u16::MAX {
            assert_eq!(table[usize::from(color & 0x7FFF)], super::rgba(color));
        }
    }
}
