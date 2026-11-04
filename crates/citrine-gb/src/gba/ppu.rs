use super::interrupt::{Interrupts, Irq};
use crate::gb::ppu::types::color::ColorCorrection;
use affine::Affine;
use background::Background;
use blend::{Blend, Effect};
use sprite::{Layout, Sprites};
use window::Windows;

mod affine;
mod background;
mod blend;
mod fetch;
mod screen;
mod sprite;
mod window;

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
const BACKDROP_PRIORITY: u8 = 4;
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
            sprites: _,
            obj_fetch,
            frame,
            blind: _,
            correction: _,
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
        obj_fetch.forget();
        frame.copy_from_slice(&source.frame);
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
        for bg in 0..4 {
            if self.dispcnt & (BG0 << bg) == 0 {
                continue;
            }
            let background = self.backgrounds[bg];
            let mosaic = background.control() & MOSAIC != 0;
            let back = if mosaic { y % mosaic_height } else { 0 };
            let layer = &mut self.layers[bg];
            match (mode, bg) {
                (0, _) | (1, 0 | 1) => background.draw_text(y - back, vram, palette, layer),
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
        let compose = Compose {
            order,
            priorities: self.backgrounds.map(|background| background.priority()),
            layers: &self.layers,
            masks: &self.masks,
            sprites: &self.sprites,
            blend: &self.blend,
            backdrop: read16(palette, 0),
        };
        let pixels = self.frame[line]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate();
        let effect = self.blend.effect();
        let rgba = match self.correction {
            ColorCorrection::Raw => rgba,
            ColorCorrection::Screen => screen::rgba,
        };
        if effect == Effect::Alpha || self.sprites.any_semi {
            for (x, pixel) in pixels {
                *pixel = rgba(compose.pair(x, effect));
            }
        } else if effect == Effect::None && self.dispcnt & WINDOWS == 0 {
            for (x, pixel) in pixels {
                *pixel = rgba(compose.plain(x));
            }
        } else {
            for (x, pixel) in pixels {
                *pixel = rgba(compose.top(x, effect));
            }
        }
    }
}

struct Compose<'a> {
    order: &'a [usize],
    priorities: [u8; 4],
    layers: &'a [[u16; WIDTH]; 4],
    masks: &'a [u8; WIDTH],
    sprites: &'a Sprites,
    blend: &'a Blend,
    backdrop: u16,
}

impl Compose<'_> {
    fn plain(&self, x: usize) -> u16 {
        let mut color = self.backdrop;
        let mut priority = BACKDROP_PRIORITY;
        for &bg in self.order {
            let value = self.layers[bg][x];
            if value & OPAQUE != 0 {
                color = value;
                priority = self.priorities[bg];
                break;
            }
        }
        let sprite = self.sprites.color[x];
        if sprite & OPAQUE != 0 && self.sprites.priority[x] <= priority {
            sprite
        } else {
            color
        }
    }

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
                Effect::Alpha if self.blend.second(below_layer) => self.blend.alpha(top, below),
                Effect::Brighten => self.blend.brighten(top),
                Effect::Darken => self.blend.darken(top),
                _ => top,
            }
        }
    }
}

fn read16(memory: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([memory[offset], memory[offset + 1]])
}

pub(crate) fn rgba(color: u16) -> [u8; 4] {
    let channel = |shift: u16| {
        let value = ((color >> shift) & 0x1F) as u8;
        value << 3 | value >> 2
    };
    [channel(0), channel(5), channel(10), 0xFF]
}

#[cfg(test)]
mod tests;
