mod affine;
mod capture;
pub mod color;
mod compose;
mod composite;
mod display;
mod fetch;
mod sprite;
mod text;

use crate::ppu2d::blend::{Blend, Effect};
use crate::ppu2d::window::Windows;
use affine::{Affine, Map};
use capture::{Capture, SourceA, SourceB};
use compose::Compose;
use display::Fifo;
use sprite::Sprites;
use text::Background;

pub use capture::CaptureLine;
pub use composite::{Composite, Fallback, Layered, Mix, Shade};

pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 192;
pub(crate) const PALETTE_BANK: usize = 0x200;

const DISPCNT: usize = 0x00;
const DISPCNT_HIGH: usize = 0x02;
const BGCNT_FIRST: usize = 0x08;
const BGCNT_LAST: usize = 0x0E;
const SCROLL_FIRST: usize = 0x10;
const SCROLL_LAST: usize = 0x1E;
const AFFINE_FIRST: usize = 0x20;
const AFFINE_LAST: usize = 0x3F;
const WINDOW_FIRST: usize = 0x40;
const WINDOW_LAST: usize = 0x4B;
const MOSAIC_REGISTER: usize = 0x4C;
const BLEND_FIRST: usize = 0x50;
const BLEND_LAST: usize = 0x55;
const CAPTURE_LOW: usize = 0x64;
const CAPTURE_HIGH: usize = 0x66;
const FIFO_LOW: usize = 0x68;
const FIFO_HIGH: usize = 0x6A;
const MASTER_BRIGHT: usize = 0x6C;

const THREE_D: u32 = 1 << 3;
const FORCED_BLANK: u32 = 1 << 7;
const OBJ_1D: u32 = 1 << 4;
const BITMAP_OBJ_WIDE: u32 = 1 << 5;
const BITMAP_OBJ_1D: u32 = 1 << 6;
const BITMAP_BOUNDARY: u32 = 128;
const BG0: u32 = 1 << 8;
const OBJ: u32 = 1 << 12;
const WINDOWS: u32 = 0b111 << 13;
const EXTENDED_BG_PALETTES: u32 = 1 << 30;
const EXTENDED_OBJ_PALETTES: u32 = 1 << 31;
const TILE_BOUNDARY: u32 = 32;
const MOSAIC: u16 = 1 << 6;
const BITMAP: u16 = 1 << 7;
const DIRECT: u16 = 1 << 2;
const BITMAP_BLOCK: u32 = 0x4000;
const BITMAP_SIZES: [(i32, i32); 4] = [(128, 128), (256, 256), (512, 256), (512, 512)];
const LARGE_SIZES: [(i32, i32); 2] = [(512, 1024), (1024, 512)];
const OPAQUE: u16 = 1 << 15;
const BASE_BLOCK: u32 = 0x1_0000;
const ENGINE_B_DISPCNT: u32 = 0xC0B1_FFF7;
const MASTER_WRITABLE: u16 = 0xC01F;
const MAX_FACTOR: u16 = 16;
const MASTER_DARKEN_BIAS: u32 = 0xF;
const MODE_OFF: u8 = 0;
const MODE_GRAPHICS: u8 = 1;
const MODE_VRAM: u8 = 2;
const MODE_FIFO: u8 = 3;
const COLOR: u16 = 0x7FFF;

pub trait EngineVram {
    fn bg(&self, address: u32) -> u8;
    fn obj(&self, address: u32) -> u8;
    fn bg_palette(&self, address: u32) -> u8;
    fn obj_palette(&self, address: u32) -> u8;

    fn bg_u16(&self, address: u32) -> u16 {
        u16::from_le_bytes([self.bg(address), self.bg(address.wrapping_add(1))])
    }

    fn obj_u16(&self, address: u32) -> u16 {
        u16::from_le_bytes([self.obj(address), self.obj(address.wrapping_add(1))])
    }

    fn bg_palette_u16(&self, address: u32) -> u16 {
        u16::from_le_bytes([
            self.bg_palette(address),
            self.bg_palette(address.wrapping_add(1)),
        ])
    }

    fn obj_palette_u16(&self, address: u32) -> u16 {
        u16::from_le_bytes([
            self.obj_palette(address),
            self.obj_palette(address.wrapping_add(1)),
        ])
    }

    fn bg_palette_bank(&self, _address: u32) -> Option<&[u8; PALETTE_BANK]> {
        None
    }

    fn bg_tile(&self, _address: u32) -> Option<&[u8; 64]> {
        None
    }

    fn bg_u32(&self, address: u32) -> u32 {
        u32::from_le_bytes(std::array::from_fn(|at| {
            self.bg(address.wrapping_add(at as u32))
        }))
    }

    fn obj_u32(&self, address: u32) -> u32 {
        u32::from_le_bytes(std::array::from_fn(|at| {
            self.obj(address.wrapping_add(at as u32))
        }))
    }
}

pub trait LcdcRead {
    fn lcdc_u16(&self, address: u32) -> u16;
}

pub trait LcdcWrite {
    fn write_lcdc_u16(&mut self, address: u32, value: u16);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EngineKind {
    A,
    B,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line3d {
    pub color: [u32; WIDTH],
    pub alpha: [u8; WIDTH],
}

pub const BLANK_3D: Line3d = Line3d {
    color: [0; WIDTH],
    alpha: [0; WIDTH],
};

impl Default for Line3d {
    fn default() -> Self {
        Self {
            color: [0; WIDTH],
            alpha: [0; WIDTH],
        }
    }
}

pub struct Memory<'a, V: EngineVram> {
    pub vram: &'a V,
    pub palette: &'a [u8],
    pub oam: &'a [u8],
    pub three_d: Option<&'a Line3d>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Engine {
    kind: EngineKind,
    dispcnt: u32,
    backgrounds: [Background; 4],
    affine: [Affine; 2],
    windows: Windows,
    blend: Blend,
    mosaic: u16,
    master: u16,
    capture: Capture,
    fifo: Fifo,
    #[cfg_attr(feature = "serde", serde(skip, default = "blank_layers"))]
    layers: [[u16; WIDTH]; 4],
    #[cfg_attr(feature = "serde", serde(skip))]
    sprites: Sprites,
    #[cfg_attr(feature = "serde", serde(skip, default = "blank_alpha"))]
    alpha_3d: [u8; WIDTH],
    #[cfg_attr(feature = "serde", serde(skip, default = "blank_colors"))]
    color_3d: [u32; WIDTH],
    #[cfg_attr(feature = "serde", serde(skip, default = "blank_alpha"))]
    masks: [u8; WIDTH],
    #[cfg_attr(feature = "serde", serde(skip))]
    drawn: ([usize; 4], usize),
}

#[derive(Debug, Clone, Copy)]
pub struct Look {
    kind: EngineKind,
    dispcnt: u32,
    backgrounds: [Background; 4],
    affine: [Affine; 2],
    windows: Windows,
    blend: Blend,
    mosaic: u16,
    master: u16,
}

fn blank_layers() -> [[u16; WIDTH]; 4] {
    [[0; WIDTH]; 4]
}

fn blank_alpha() -> [u8; WIDTH] {
    [0; WIDTH]
}

fn blank_colors() -> [u32; WIDTH] {
    [0; WIDTH]
}

impl Engine {
    pub fn new(kind: EngineKind) -> Self {
        Self {
            kind,
            dispcnt: 0,
            backgrounds: [Background::default(); 4],
            affine: [Affine::default(); 2],
            windows: Windows::default(),
            blend: Blend::default(),
            mosaic: 0,
            master: 0,
            capture: Capture::default(),
            fifo: Fifo::default(),
            layers: blank_layers(),
            sprites: Sprites::default(),
            alpha_3d: blank_alpha(),
            color_3d: blank_colors(),
            masks: blank_alpha(),
            drawn: ([0; 4], 0),
        }
    }

    pub fn kind(&self) -> EngineKind {
        self.kind
    }

    pub fn read(&self, offset: usize) -> u16 {
        match offset {
            DISPCNT => self.dispcnt as u16,
            DISPCNT_HIGH => (self.dispcnt >> 16) as u16,
            BGCNT_FIRST..=BGCNT_LAST => self.backgrounds[(offset - BGCNT_FIRST) / 2].control(),
            WINDOW_FIRST..=WINDOW_LAST => self.windows.read(offset - WINDOW_FIRST),
            BLEND_FIRST..=BLEND_LAST => self.blend.read(offset - BLEND_FIRST),
            CAPTURE_LOW if self.kind == EngineKind::A => self.capture.control() as u16,
            CAPTURE_HIGH if self.kind == EngineKind::A => (self.capture.control() >> 16) as u16,
            MASTER_BRIGHT => self.master,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: usize, value: u16, mask: u16) {
        match offset {
            DISPCNT => self.write_dispcnt(0, value, mask),
            DISPCNT_HIGH => self.write_dispcnt(16, value, mask),
            BGCNT_FIRST..=BGCNT_LAST => {
                self.backgrounds[(offset - BGCNT_FIRST) / 2].write_control(value, mask)
            }
            SCROLL_FIRST..=SCROLL_LAST => {
                let background = &mut self.backgrounds[(offset - SCROLL_FIRST) / 4];
                if offset & 2 == 0 {
                    background.write_hofs(value, mask);
                } else {
                    background.write_vofs(value, mask);
                }
            }
            AFFINE_FIRST..=AFFINE_LAST => {
                self.affine[(offset - AFFINE_FIRST) / 16].write(offset & 0xF, value, mask)
            }
            WINDOW_FIRST..=WINDOW_LAST => self.windows.write(offset - WINDOW_FIRST, value, mask),
            MOSAIC_REGISTER => self.mosaic = (self.mosaic & !mask) | (value & mask),
            BLEND_FIRST..=BLEND_LAST => self.blend.write(offset - BLEND_FIRST, value, mask),
            CAPTURE_LOW | CAPTURE_HIGH if self.kind == EngineKind::A => {
                self.capture
                    .write(if offset == CAPTURE_HIGH { 16 } else { 0 }, value, mask)
            }
            FIFO_LOW | FIFO_HIGH if self.kind == EngineKind::A => self.fifo.push(value & mask),
            MASTER_BRIGHT => {
                self.master = ((self.master & !mask) | (value & mask)) & MASTER_WRITABLE
            }
            _ => {}
        }
    }

    fn write_dispcnt(&mut self, shift: u32, value: u16, mask: u16) {
        merge32(&mut self.dispcnt, shift, value, mask);
        if self.kind == EngineKind::B {
            self.dispcnt &= ENGINE_B_DISPCNT;
        }
    }

    pub fn display_mode(&self) -> u8 {
        (self.dispcnt >> 16 & 0b11) as u8
    }

    pub fn feeds_the_game(&self, y: usize) -> bool {
        self.display_mode() == MODE_FIFO || self.capture.request(y).is_some()
    }

    pub fn fifo_room(&self) -> usize {
        self.fifo.room()
    }

    pub fn paints_alone(&self, y: usize) -> bool {
        self.display_mode() == MODE_GRAPHICS && self.capture.request(y).is_none()
    }

    pub fn look(&self) -> Look {
        Look {
            kind: self.kind,
            dispcnt: self.dispcnt,
            backgrounds: self.backgrounds,
            affine: self.affine,
            windows: self.windows,
            blend: self.blend,
            mosaic: self.mosaic,
            master: self.master,
        }
    }

    pub fn paint<V: EngineVram>(
        &mut self,
        look: &Look,
        y: usize,
        memory: &Memory<V>,
        layered: bool,
        out: &mut [u32; WIDTH],
    ) {
        self.kind = look.kind;
        self.dispcnt = look.dispcnt;
        self.backgrounds = look.backgrounds;
        self.affine = look.affine;
        self.windows = look.windows;
        self.blend = look.blend;
        self.mosaic = look.mosaic;
        self.master = look.master;
        if !self.shades_out(layered) {
            self.render_line(y, memory, out);
        }
        self.master_brightness(out);
    }

    pub fn start_line(&mut self, line: u16) {
        self.windows.start_line(line & 0xFF);
        let line = usize::from(line);
        self.capture.start_line(line);
        if line == HEIGHT {
            self.affine.iter_mut().for_each(Affine::latch);
        } else if (1..HEIGHT).contains(&line) {
            self.affine.iter_mut().for_each(Affine::step);
        }
    }

    pub fn render_line<V: EngineVram>(
        &mut self,
        y: usize,
        memory: &Memory<V>,
        out: &mut [u32; WIDTH],
    ) {
        if self.dispcnt & FORCED_BLANK != 0 {
            out.fill(color::WHITE);
            return;
        }
        let mut order = [0usize; 4];
        let mut count = 0;
        for bg in 0..4 {
            if self.dispcnt & (BG0 << bg) == 0 {
                continue;
            }
            if !self.draw_background(bg, y, memory) {
                continue;
            }
            order[count] = bg;
            count += 1;
        }
        order[..count].sort_by_key(|&bg| (self.backgrounds[bg].priority(), bg));
        self.drawn = (order, count);
        let order = &order[..count];
        if self.dispcnt & OBJ != 0 {
            let layout = sprite::Layout {
                boundary: (self.dispcnt & OBJ_1D != 0)
                    .then(|| TILE_BOUNDARY << (self.dispcnt >> 20 & 0b11)),
                bitmap: match (
                    self.dispcnt & BITMAP_OBJ_1D != 0,
                    self.dispcnt & BITMAP_OBJ_WIDE != 0,
                ) {
                    (false, wide) => Some(sprite::Bitmap::TwoDimensional {
                        width: if wide { 256 } else { 128 },
                    }),
                    (true, false) => Some(sprite::Bitmap::OneDimensional {
                        boundary: BITMAP_BOUNDARY << (self.dispcnt >> 22 & 1),
                    }),
                    (true, true) => None,
                },
                extended: self.dispcnt & EXTENDED_OBJ_PALETTES != 0,
                mosaic: (
                    usize::from(self.mosaic >> 8 & 0xF) + 1,
                    usize::from(self.mosaic >> 12) + 1,
                ),
            };
            self.sprites.draw(y, &layout, memory);
        } else {
            self.sprites.clear();
        }
        self.windows
            .line(self.dispcnt as u16, &self.sprites.window, &mut self.masks);
        let three_d = self.dispcnt & (THREE_D | BG0) == THREE_D | BG0;
        let compose = Compose {
            order,
            priorities: self.backgrounds.map(|background| background.priority()),
            layers: &self.layers,
            three_d: three_d.then_some(compose::ThreeD {
                alpha: &self.alpha_3d,
                color: &self.color_3d,
            }),
            masks: &self.masks,
            windowed: self.dispcnt & WINDOWS != 0,
            sprites: &self.sprites,
            blend: &self.blend,
            backdrop: read16(memory.palette, 0),
        };
        compose.line(self.blend.effect(), out);
    }

    pub fn display_line<V: EngineVram, L: LcdcRead>(
        &mut self,
        y: usize,
        memory: &Memory<V>,
        lcdc: &L,
        layered: bool,
        out: &mut [u32; WIDTH],
    ) -> Option<CaptureLine> {
        let mode = self.display_mode();
        let request = self.capture.request(y);
        let a = request.and_then(|request| request.a);
        let b = request.and_then(|request| request.b);
        let block = self.dispcnt >> 18 & 0b11;
        let graphics = (a == Some(SourceA::Graphics)).then(|| {
            let mut graphics = [0; WIDTH];
            self.render_line(y, memory, &mut graphics);
            graphics
        });
        let fifo = (mode == MODE_FIFO || b == Some(SourceB::Fifo)).then(|| self.fifo.take_line());
        let widen = |line: [u16; WIDTH]| line.map(|pixel| color::widen(pixel & COLOR));
        match (mode, graphics) {
            (MODE_OFF, _) => out.fill(color::WHITE),
            (MODE_GRAPHICS, Some(graphics)) => *out = graphics,
            (MODE_GRAPHICS, None) if self.shades_out(layered) => {}
            (MODE_GRAPHICS, None) => self.render_line(y, memory, out),
            (MODE_VRAM, _) => *out = widen(display::vram_line(lcdc, block, 0, y)),
            _ => *out = widen(fifo.unwrap_or([0; WIDTH])),
        }
        if mode != MODE_OFF {
            self.master_brightness(out);
        }
        let request = request?;
        let source_a = match a {
            Some(SourceA::Graphics) => graphics
                .unwrap_or([0; WIDTH])
                .map(|pixel| color::narrow(pixel) | OPAQUE),
            Some(SourceA::ThreeD) => match memory.three_d {
                Some(line) => std::array::from_fn(|x| {
                    let alpha = if line.alpha[x] != 0 { OPAQUE } else { 0 };
                    color::narrow(line.color[x]) | alpha
                }),
                None => [0; WIDTH],
            },
            None => [0; WIDTH],
        };
        let source_b = match b {
            Some(SourceB::Vram { offset }) => {
                let offset = if mode == MODE_VRAM { 0 } else { offset };
                display::vram_line(lcdc, block, offset, y)
            }
            Some(SourceB::Fifo) => fifo.unwrap_or([0; WIDTH]),
            None => [0; WIDTH],
        };
        Some(request.line(&source_a, &source_b))
    }

    fn shades_out(&self, layered: bool) -> bool {
        !layered && self.master_shade().fills()
    }

    pub fn draws_layers(&self, y: usize, layered: bool) -> bool {
        let graphics = self.capture.request(y).and_then(|request| request.a);
        self.dispcnt & FORCED_BLANK == 0
            && (graphics == Some(SourceA::Graphics)
                || self.display_mode() == MODE_GRAPHICS && !self.shades_out(layered))
    }

    pub fn reads_3d(&self, y: usize) -> bool {
        if self.capture.request(y).is_some() {
            return true;
        }
        self.display_mode() == MODE_GRAPHICS
            && self.dispcnt & FORCED_BLANK == 0
            && self.dispcnt & (THREE_D | BG0) == THREE_D | BG0
            && self.blend.second(0)
            && (self.blend.effect() == Effect::Alpha || self.dispcnt & OBJ != 0)
    }

    pub fn master_shade(&self) -> Shade {
        let factor = (self.master & 0x1F).min(MAX_FACTOR) as u8;
        match self.master >> 14 {
            1 => Shade::Brighten(factor),
            2 => Shade::Darken(factor),
            _ => Shade::None,
        }
    }

    pub fn master_brightness(&self, line: &mut [u32; WIDTH]) {
        self.master_shade().master_line(line);
    }

    pub fn layered_line(&self, palette: &[u8], out: &[u32; WIDTH], into: &mut [Layered]) -> u16 {
        for (pixel, &native) in into.iter_mut().zip(out) {
            *pixel = Layered {
                native,
                under: native,
                below: native,
                mix: Mix::Native,
                shade: Shade::None,
            };
        }
        let shown = self.display_mode() == MODE_GRAPHICS
            && self.dispcnt & FORCED_BLANK == 0
            && self.dispcnt & (THREE_D | BG0) == THREE_D | BG0;
        if !shown {
            return 0;
        }
        let (order, count) = self.drawn;
        let order = &order[..count];
        let priorities = self.backgrounds.map(|background| background.priority());
        let compose = Compose {
            order,
            priorities,
            layers: &self.layers,
            three_d: Some(compose::ThreeD {
                alpha: &self.alpha_3d,
                color: &self.color_3d,
            }),
            masks: &self.masks,
            windowed: self.dispcnt & WINDOWS != 0,
            sprites: &self.sprites,
            blend: &self.blend,
            backdrop: read16(palette, 0),
        };
        let mut beneath = [0usize; 4];
        let mut kept = 0;
        for &bg in order.iter().filter(|&&bg| bg != 0) {
            beneath[kept] = bg;
            kept += 1;
        }
        let without = Compose {
            order: &beneath[..kept],
            three_d: None,
            ..compose
        };
        let effect = self.blend.effect();
        let stacked = without.stacked();
        for (x, pixel) in into.iter_mut().enumerate() {
            if !compose.shows_3d_on_top(x) {
                continue;
            }
            let (under, below, layer) = without.mix(x, &stacked, effect);
            let (mix, shade) = compose.mixes_3d(x, layer, effect);
            pixel.under = under;
            pixel.below = below;
            pixel.mix = mix;
            pixel.shade = shade;
        }
        self.backgrounds[0].hofs()
    }

    fn draw_background<V: EngineVram>(&mut self, bg: usize, y: usize, memory: &Memory<V>) -> bool {
        let background = self.backgrounds[bg];
        let layer = &mut self.layers[bg];
        if bg == 0 && self.dispcnt & THREE_D != 0 {
            match memory.three_d {
                Some(line) => {
                    let scroll = usize::from(background.hofs()) & 0x1FF;
                    let (start, from, len) = if scroll < WIDTH {
                        (0, scroll, WIDTH - scroll)
                    } else {
                        (2 * WIDTH - scroll, 0, scroll - WIDTH)
                    };
                    let shown = start..start + len;
                    for (low, high) in [(0, start), (start + len, WIDTH)] {
                        layer[low..high].fill(0);
                        self.alpha_3d[low..high].fill(0);
                        self.color_3d[low..high].fill(0);
                    }
                    let targets = layer[shown.clone()]
                        .iter_mut()
                        .zip(&mut self.alpha_3d[shown.clone()])
                        .zip(&mut self.color_3d[shown]);
                    let sources = line.alpha[from..from + len]
                        .iter()
                        .zip(&line.color[from..from + len]);
                    for (((pixel, alpha), color), (&source_alpha, &source_color)) in
                        targets.zip(sources)
                    {
                        let visible = source_alpha != 0;
                        *alpha = source_alpha;
                        *color = if visible { source_color } else { 0 };
                        *pixel = if visible { OPAQUE } else { 0 };
                    }
                }
                None => layer.fill(0),
            }
            return true;
        }
        let control = background.control();
        let mosaic = control & MOSAIC != 0;
        let mosaic_height = usize::from(self.mosaic >> 4 & 0xF) + 1;
        let back = if mosaic { y % mosaic_height } else { 0 };
        let source = text::Source {
            char_base: (self.dispcnt >> 24 & 0b111) * BASE_BLOCK,
            screen_base: (self.dispcnt >> 27 & 0b111) * BASE_BLOCK,
            extended: self.dispcnt & EXTENDED_BG_PALETTES != 0,
        };
        let size = control >> 14;
        let square = 128i32 << size;
        let tiled = |map| affine::Layout {
            control,
            size: (square, square),
            char_base: background.char_base(&source),
            screen_base: background.screen_base(&source),
            map,
        };
        let bitmap = |map| affine::Layout {
            control,
            size: BITMAP_SIZES[usize::from(size)],
            char_base: 0,
            screen_base: u32::from(control >> 8 & 0x1F) * BITMAP_BLOCK,
            map,
        };
        let layout = match BgKind::of(self.kind, self.dispcnt as u8 & 0b111, bg) {
            BgKind::Text => {
                background.draw_text(bg, y - back, &source, memory, layer);
                None
            }
            BgKind::Affine => Some(tiled(Map::Bytes)),
            BgKind::Extended if control & BITMAP == 0 => Some(tiled(Map::Entries {
                extended: source.extended.then(|| background.slot(bg)),
            })),
            BgKind::Extended if control & DIRECT == 0 => Some(bitmap(Map::Indexed)),
            BgKind::Extended => Some(bitmap(Map::Direct)),
            BgKind::Large => Some(affine::Layout {
                control,
                size: LARGE_SIZES[usize::from(size & 1)],
                char_base: 0,
                screen_base: 0,
                map: Map::Indexed,
            }),
            BgKind::None => return false,
        };
        if let Some(layout) = layout {
            self.affine[bg - 2].draw(&layout, back, memory, layer);
        }
        let mosaic_width = usize::from(self.mosaic & 0xF) + 1;
        if mosaic && mosaic_width > 1 {
            for x in 0..WIDTH {
                layer[x] = layer[x - x % mosaic_width];
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BgKind {
    Text,
    Affine,
    Extended,
    Large,
    None,
}

impl BgKind {
    fn of(kind: EngineKind, mode: u8, bg: usize) -> Self {
        match (mode, bg) {
            (0..=5, 0 | 1) | (0, _) | (1 | 3, 2) => Self::Text,
            (1, 3) | (2, _) | (4, 2) => Self::Affine,
            (3 | 4, 3) | (5, _) => Self::Extended,
            (6, 2) if kind == EngineKind::A => Self::Large,
            _ => Self::None,
        }
    }
}

fn merge32(register: &mut u32, shift: u32, value: u16, mask: u16) {
    let mask = u32::from(mask) << shift;
    *register = (*register & !mask) | (u32::from(value) << shift & mask);
}

fn read16(memory: &[u8], offset: usize) -> u16 {
    let bytes = memory[offset..offset + 2].try_into().expect("two bytes");
    u16::from_le_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) struct Flat {
        pub bg: Vec<u8>,
        pub obj: Vec<u8>,
        pub bg_palette: Vec<u8>,
        pub obj_palette: Vec<u8>,
    }

    impl Flat {
        pub fn new(kind: EngineKind) -> Self {
            let (bg, obj) = match kind {
                EngineKind::A => (0x8_0000, 0x4_0000),
                EngineKind::B => (0x2_0000, 0x2_0000),
            };
            Self {
                bg: vec![0; bg],
                obj: vec![0; obj],
                bg_palette: vec![0; 0x8000],
                obj_palette: vec![0; 0x2000],
            }
        }

        pub fn put_bg16(&mut self, address: usize, value: u16) {
            self.bg[address..address + 2].copy_from_slice(&value.to_le_bytes());
        }
    }

    impl EngineVram for Flat {
        fn bg(&self, address: u32) -> u8 {
            self.bg[address as usize % self.bg.len()]
        }

        fn obj(&self, address: u32) -> u8 {
            self.obj[address as usize % self.obj.len()]
        }

        fn bg_palette(&self, address: u32) -> u8 {
            self.bg_palette[address as usize % self.bg_palette.len()]
        }

        fn obj_palette(&self, address: u32) -> u8 {
            self.obj_palette[address as usize % self.obj_palette.len()]
        }
    }

    pub(super) struct Tiled<'a>(pub &'a Flat);

    impl EngineVram for Tiled<'_> {
        fn bg(&self, address: u32) -> u8 {
            self.0.bg(address)
        }

        fn obj(&self, address: u32) -> u8 {
            self.0.obj(address)
        }

        fn bg_palette(&self, address: u32) -> u8 {
            self.0.bg_palette(address)
        }

        fn obj_palette(&self, address: u32) -> u8 {
            self.0.obj_palette(address)
        }

        fn bg_tile(&self, address: u32) -> Option<&[u8; 64]> {
            let at = address as usize % self.0.bg.len();
            self.0.bg.get(at..at + 64)?.try_into().ok()
        }
    }

    pub(super) struct Banks {
        pub halves: Vec<u16>,
    }

    impl Banks {
        pub fn new() -> Self {
            Self {
                halves: vec![0; 0x4_0000],
            }
        }

        pub fn at(&self, address: u32) -> u16 {
            self.halves[address as usize / 2]
        }

        pub fn put(&mut self, address: u32, value: u16) {
            self.halves[address as usize / 2] = value;
        }
    }

    impl LcdcRead for Banks {
        fn lcdc_u16(&self, address: u32) -> u16 {
            self.at(address)
        }
    }

    impl LcdcWrite for Banks {
        fn write_lcdc_u16(&mut self, address: u32, value: u16) {
            self.put(address, value);
        }
    }

    pub(super) struct Scene {
        pub engine: Engine,
        pub vram: Flat,
        pub palette: Vec<u8>,
        pub oam: Vec<u8>,
        pub three_d: Option<Line3d>,
    }

    impl Scene {
        pub fn new(kind: EngineKind) -> Self {
            Self {
                engine: Engine::new(kind),
                vram: Flat::new(kind),
                palette: vec![0; 0x400],
                oam: vec![0; 0x400],
                three_d: None,
            }
        }

        pub fn write(&mut self, offset: usize, value: u16) {
            self.engine.write(offset, value, 0xFFFF);
        }

        pub fn write32(&mut self, offset: usize, value: u32) {
            self.write(offset, value as u16);
            self.write(offset + 2, (value >> 16) as u16);
        }

        pub fn color(&mut self, index: usize, color: u16) {
            self.palette[index * 2..index * 2 + 2].copy_from_slice(&color.to_le_bytes());
        }

        pub fn line(&mut self, y: usize) -> [u16; WIDTH] {
            self.engine.start_line(HEIGHT as u16);
            for line in 0..=y {
                self.engine.start_line(line as u16);
            }
            self.render(y)
        }

        pub fn render(&mut self, y: usize) -> [u16; WIDTH] {
            self.render_wide(y).map(color::narrow)
        }

        pub fn render_wide(&mut self, y: usize) -> [u32; WIDTH] {
            let memory = Memory {
                vram: &self.vram,
                palette: &self.palette,
                oam: &self.oam,
                three_d: self.three_d.as_ref(),
            };
            let mut out = [0; WIDTH];
            self.engine.render_line(y, &memory, &mut out);
            out
        }

        pub fn line_wide(&mut self, y: usize) -> [u32; WIDTH] {
            self.engine.start_line(HEIGHT as u16);
            for line in 0..=y {
                self.engine.start_line(line as u16);
            }
            self.render_wide(y)
        }

        pub fn display(&mut self, y: usize, banks: &mut Banks) -> [u16; WIDTH] {
            self.display_wide(y, banks).map(color::narrow)
        }

        pub fn display_wide(&mut self, y: usize, banks: &mut Banks) -> [u32; WIDTH] {
            let memory = Memory {
                vram: &self.vram,
                palette: &self.palette,
                oam: &self.oam,
                three_d: self.three_d.as_ref(),
            };
            let mut out = [0; WIDTH];
            if let Some(capture) = self.engine.display_line(y, &memory, banks, false, &mut out) {
                capture.write(banks);
            }
            out
        }
    }

    #[test]
    fn engine_b_drops_the_dispcnt_bits_it_lacks() {
        let mut scene = Scene::new(EngineKind::B);
        scene.write32(DISPCNT, 0xFFFF_FFFF);
        assert_eq!(scene.engine.read(DISPCNT), 0xFFF7);
        assert_eq!(scene.engine.read(DISPCNT_HIGH), 0xC0B1);
        assert_eq!(scene.engine.display_mode(), 1);
    }

    #[test]
    fn engine_b_draws_bg0_as_text_even_with_the_3d_bit_written() {
        let mut scene = Scene::new(EngineKind::B);
        scene.write32(DISPCNT, 1 << 16 | THREE_D | BG0);
        scene.vram.bg[32..64].fill(0x11);
        scene.vram.put_bg16(0, 1);
        scene.color(1, 0x001F);
        let mut line = Line3d::default();
        line.alpha.fill(31);
        line.color.fill(0x7C00);
        scene.three_d = Some(line);
        assert_eq!(scene.line(0)[0], 0x001F);
    }

    #[test]
    fn engine_b_ignores_the_dispcnt_bases_and_has_no_mode_6() {
        let mut scene = Scene::new(EngineKind::B);
        scene.write32(DISPCNT, 1 << 16 | BG0 | 1 << 24 | 1 << 27);
        scene.vram.bg[32..64].fill(0x11);
        scene.vram.put_bg16(0, 1);
        scene.color(0, 0x1234);
        scene.color(1, 0x001F);
        assert_eq!(scene.line(0)[0], 0x001F);
        scene.write32(DISPCNT, 1 << 16 | 6 | BG0 | 1 << 10);
        assert_eq!(scene.line(0)[0], 0x1234);
    }

    fn noise(seed: &mut u32, bytes: &mut [u8]) {
        for byte in bytes {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 17;
            *seed ^= *seed << 5;
            *byte = (*seed >> 24) as u8;
        }
    }

    fn busy_scene(kind: EngineKind) -> Scene {
        let mut scene = Scene::new(kind);
        let mut seed = 0x2D2D_2D2D;
        noise(&mut seed, &mut scene.vram.bg);
        noise(&mut seed, &mut scene.vram.obj);
        noise(&mut seed, &mut scene.vram.bg_palette);
        noise(&mut seed, &mut scene.vram.obj_palette);
        noise(&mut seed, &mut scene.palette);
        noise(&mut seed, &mut scene.oam);
        scene.write32(
            DISPCNT,
            1 << 16 | 4 | THREE_D | 0x1F << 8 | 0b101 << 13 | 1 << 4 | 1 << 20 | 3 << 30,
        );
        let registers = [
            (0x08, 0x0001),
            (0x0A, 0x4446 | 1 << 13),
            (0x0C, 0x5A86 | 1 << 13),
            (0x0E, 0x9C0B),
            (0x10, 13),
            (0x12, 7),
            (0x14, 300),
            (0x16, 40),
            (0x20, 0x00F0),
            (0x22, 0x0040),
            (0x24, 0xFFC0),
            (0x26, 0x00F0),
            (0x28, 0x1200),
            (0x2C, 0x0800),
            (0x30, 0x0180),
            (0x36, 0x0100),
            (0x38, 0xF000),
            (0x3A, 0x0FFF),
            (0x40, 40 << 8 | 200),
            (0x44, 30 << 8 | 150),
            (0x48, 0x3F1B),
            (0x4A, 0x2E35),
            (0x4C, 0x2121),
            (0x50, 0x3F41),
            (0x52, 0x0A06),
            (0x54, 5),
            (0x6C, 1 << 14 | 3),
        ];
        for (offset, value) in registers {
            scene.write(offset, value);
        }
        let mut line = Line3d::default();
        for x in 0..WIDTH {
            line.color[x] = (x as u32 * 997) & color::WHITE;
            line.alpha[x] = (x % 32) as u8;
        }
        scene.three_d = Some(line);
        scene
    }

    fn line_hashes(scene: &mut Scene) -> Vec<u64> {
        scene.engine.start_line(HEIGHT as u16);
        (0..HEIGHT)
            .map(|y| {
                scene.engine.start_line(y as u16);
                let mut line = scene.render_wide(y);
                scene.engine.master_brightness(&mut line);
                line.iter().fold(0xCBF2_9CE4_8422_2325u64, |hash, &pixel| {
                    (hash ^ u64::from(pixel)).wrapping_mul(0x0100_0000_01B3)
                })
            })
            .collect()
    }

    #[test]
    fn a_busy_scene_keeps_its_pinned_line_hashes() {
        let picked = [EngineKind::A, EngineKind::B].map(|kind| {
            let hashes = line_hashes(&mut busy_scene(kind));
            let distinct = hashes.iter().collect::<std::collections::HashSet<_>>();
            assert!(distinct.len() > 150, "{kind:?}");
            [hashes[0], hashes[64], hashes[127], hashes[191]]
        });
        let pins = [
            [
                4851778740075188239,
                2854753504009097647,
                13012571805379194471,
                1217404545631438033,
            ],
            [
                5916938464756386629,
                11170862378735697342,
                16022885130074492622,
                15901246364615537043,
            ],
        ];
        assert_eq!(picked, pins);
    }

    #[test]
    fn fully_shaded_lines_leave_the_next_lines_as_drawn() {
        for kind in [EngineKind::A, EngineKind::B] {
            let mut shown = busy_scene(kind);
            let mut shaded = busy_scene(kind);
            let mut banks = Banks::new();
            shown.engine.start_line(HEIGHT as u16);
            shaded.engine.start_line(HEIGHT as u16);
            for y in 0..HEIGHT {
                shown.engine.start_line(y as u16);
                shaded.engine.start_line(y as u16);
                let drawn = shown.display_wide(y, &mut banks);
                if y % 3 != 0 {
                    assert_eq!(shaded.display_wide(y, &mut banks), drawn, "{kind:?} {y}");
                    continue;
                }
                let (master, fill) = if y % 2 == 0 {
                    (2 << 14 | 16, 0)
                } else {
                    (1 << 14 | 31, color::WHITE)
                };
                shaded.write(MASTER_BRIGHT, master);
                assert_eq!(shaded.display_wide(y, &mut banks), [fill; WIDTH]);
                shaded.write(MASTER_BRIGHT, 1 << 14 | 3);
            }
        }
    }

    fn busy_bitmap_scene(mode: u32) -> Scene {
        let mut scene = busy_scene(EngineKind::A);
        scene.write32(
            DISPCNT,
            1 << 16 | mode | THREE_D | 0x1F << 8 | 0b101 << 13 | 1 << 5 | 3 << 30,
        );
        let registers = [
            (0x0C, 0x4086 | 1 << 13 | 2 << 8),
            (0x0E, 0xC0C3),
            (0x20, 0x0110),
            (0x22, 0x0030),
            (0x24, 0xFFE0),
            (0x26, 0x00F0),
            (0x30, 0x00C0),
            (0x36, 0x0140),
        ];
        for (offset, value) in registers {
            scene.write(offset, value);
        }
        for sprite in 0..128 {
            if sprite % 3 == 0 {
                let at = sprite * 8;
                let attr0 = u16::from_le_bytes([scene.oam[at], scene.oam[at + 1]]);
                let attr0 = (attr0 & !(0b11 << 8 | 0b11 << 10 | 1 << 15)) | 3 << 10;
                scene.oam[at..at + 2].copy_from_slice(&attr0.to_le_bytes());
            }
        }
        scene
    }

    #[test]
    fn a_busy_bitmap_scene_keeps_its_pinned_line_hashes() {
        let picked = [5, 6].map(|mode| {
            let hashes = line_hashes(&mut busy_bitmap_scene(mode));
            let distinct = hashes.iter().collect::<std::collections::HashSet<_>>();
            assert!(distinct.len() > 150, "mode {mode}");
            [hashes[0], hashes[64], hashes[127], hashes[191]]
        });
        let pins = [
            [
                13425208052041658735,
                8370520795858776621,
                11930943399589359589,
                16527725244590770408,
            ],
            [
                14369245647457640437,
                16610385143717900174,
                4473322118145315821,
                17633416535010581037,
            ],
        ];
        assert_eq!(picked, pins);
    }

    #[test]
    fn dispcnt_reads_back_all_32_bits() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, 0xC123_4567);
        assert_eq!(scene.engine.read(DISPCNT), 0x4567);
        assert_eq!(scene.engine.read(DISPCNT_HIGH), 0xC123);
        assert_eq!(scene.engine.display_mode(), 3);
    }

    #[test]
    fn no_layers_draw_the_backdrop() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, 1 << 16);
        scene.color(0, 0x1234);
        assert_eq!(scene.line(0), [0x1234; WIDTH]);
    }

    #[test]
    fn forced_blank_draws_white() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, 1 << 16 | FORCED_BLANK | BG0);
        scene.color(0, 0x1234);
        assert_eq!(scene.line_wide(5), [color::WHITE; WIDTH]);
    }

    #[test]
    fn a_3d_pixel_reaches_the_screen_with_all_six_bits_of_each_channel() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, 1 << 16 | THREE_D | BG0);
        let mut line = Line3d::default();
        line.color[0] = color::from_channels(63, 33, 1);
        line.alpha[0] = 31;
        scene.three_d = Some(line);
        let out = scene.line_wide(0)[0];
        assert_eq!(out, color::from_channels(63, 33, 1));
        assert_eq!(color::rgba(out), [255, 134, 4, 255]);
    }

    #[test]
    fn bg0_in_3d_mode_takes_the_line_from_outside_scrolled_by_bg0hofs() {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, 1 << 16 | THREE_D | BG0);
        scene.color(0, 0x1234);
        let mut line = Line3d::default();
        line.color[3] = color::widen(0x03E0);
        line.alpha[3] = 31;
        line.color[4] = color::widen(0x001F);
        scene.three_d = Some(line);
        let out = scene.line(0);
        assert_eq!(out[3], 0x03E0);
        assert_eq!(out[4], 0x1234);
        scene.write(0x10, 0x1FE);
        let out = scene.line(0);
        assert_eq!(out[5], 0x03E0);
        assert_eq!(out[0], 0x1234);
        scene.write(0x10, 2);
        assert_eq!(scene.line(0)[1], 0x03E0);
        assert_eq!(scene.line(0)[WIDTH - 1], 0x1234);
        scene.three_d = None;
        assert_eq!(scene.line(0)[1], 0x1234);
    }
}
