mod interp;
mod shade;
mod slope;
pub mod texture;

use super::geometry::frame::{Frame, Polygon};
use super::video::{HEIGHT, Line3d, WIDTH, color};
use interp::{Axis, DepthLane, Interpolator, Lane};
use shade::{SHADOW, TOON};
use slope::{Coverage, FULL_COVERAGE, Slope};
use std::ops::Range;
use texture::TextureMemory;

pub const DISP3DCNT: usize = 0x060;
pub const ALPHA_TEST_REF: usize = 0x340;
pub const CLEAR_COLOR: usize = 0x350;
pub const CLEAR_DEPTH: usize = 0x354;
pub const EDGE_COLOR: usize = 0x330;
pub const FOG_COLOR: usize = 0x358;
pub const FOG_OFFSET: usize = 0x35C;
pub const FOG_TABLE: usize = 0x360;
pub const TOON_TABLE: usize = 0x380;

const TEXTURES: u32 = 1 << 0;
const ALPHA_TEST: u32 = 1 << 2;
const ALPHA_BLENDING: u32 = 1 << 3;
const ANTIALIASING: u32 = 1 << 4;
const EDGE_MARKING: u32 = 1 << 5;
const FOG_ALPHA_ONLY: u32 = 1 << 6;
const FOG_ENABLE: u32 = 1 << 7;
const FOG_SHIFT_SHIFT: u32 = 8;
const HIGHLIGHT: u32 = 1 << 1;
const FOG_COLOR_MASK: u32 = 0x001F_7FFF;
const FOG_OFFSET_MASK: u32 = 0x7FFF;
const FOG_DENSITY_MASK: u8 = 0x7F;
const COLOR_MASK: u16 = 0x7FFF;
const EDGE_COVERAGE: u8 = 16;
const DISP3DCNT_MASK: u32 = 0x4FFF;
const CLEAR_COLOR_MASK: u32 = 0x3F1F_FFFF;
const CLEAR_DEPTH_MASK: u32 = 0xFFFF_7FFF;
const CLEAR_ALPHA_SHIFT: u32 = 16;
const DEPTH_STEP: u32 = 0x200;
const DEPTH_FAR: u32 = 0x7FFF;
const DEPTH_FILL: u32 = 0x1FF;

const MODE_SHIFT: u32 = 4;
const MODE_MASK: u32 = 3;
const TRANSLUCENT_DEPTH: u32 = 1 << 11;
const DEPTH_EQUAL: u32 = 1 << 14;
const FOG: u32 = 1 << 15;
const ID_SHIFT: u32 = 24;
const ID_MASK: u32 = 0x3F;
const EQUAL_Z: i32 = 0x200;
const EQUAL_W: i32 = 0xFF;
const ALPHA_SHIFT: u32 = 16;
const ALPHA_MASK: u32 = 0x1F;
const OPAQUE: u8 = 31;
const STENCIL_TOP: u8 = 1;
const STENCIL_UNDER: u8 = 2;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Registers {
    disp3dcnt: u32,
    alpha_reference: u32,
    clear_color: u32,
    clear_depth: u32,
    edge: [u16; 8],
    fog_color: u32,
    fog_offset: u32,
    fog: [u8; 32],
    toon: [u16; 32],
}

impl Registers {
    pub fn read(&self, offset: usize) -> u32 {
        match offset & !3 {
            DISP3DCNT => self.disp3dcnt,
            ALPHA_TEST_REF => self.alpha_reference,
            CLEAR_COLOR => self.clear_color,
            CLEAR_DEPTH => self.clear_depth,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: usize, value: u32, mask: u32) {
        let word = offset & !3;
        let halves = |table: &mut [u16], base: usize| {
            let index = (word - base) / 2;
            for (half, slot) in table[index..index + 2].iter_mut().enumerate() {
                let shift = half * 16;
                let keep = (mask >> shift) as u16;
                *slot = (*slot & !keep | (value >> shift) as u16 & keep) & COLOR_MASK;
            }
        };
        match word {
            EDGE_COLOR..0x340 => return halves(&mut self.edge, EDGE_COLOR),
            TOON_TABLE..0x3C0 => return halves(&mut self.toon, TOON_TABLE),
            FOG_TABLE..TOON_TABLE => {
                let index = word - FOG_TABLE;
                for (byte, slot) in self.fog[index..index + 4].iter_mut().enumerate() {
                    let shift = byte * 8;
                    let keep = (mask >> shift) as u8;
                    *slot = (*slot & !keep | (value >> shift) as u8 & keep) & FOG_DENSITY_MASK;
                }
                return;
            }
            _ => {}
        }
        let (register, writable) = match word {
            DISP3DCNT => (&mut self.disp3dcnt, DISP3DCNT_MASK),
            ALPHA_TEST_REF => (&mut self.alpha_reference, ALPHA_MASK),
            CLEAR_COLOR => (&mut self.clear_color, CLEAR_COLOR_MASK),
            CLEAR_DEPTH => (&mut self.clear_depth, CLEAR_DEPTH_MASK),
            FOG_COLOR => (&mut self.fog_color, FOG_COLOR_MASK),
            FOG_OFFSET => (&mut self.fog_offset, FOG_OFFSET_MASK),
            _ => return,
        };
        let mask = mask & writable;
        *register = (*register & !mask) | (value & mask);
    }

    fn alpha_threshold(&self) -> u8 {
        if self.disp3dcnt & ALPHA_TEST != 0 {
            self.alpha_reference as u8
        } else {
            0
        }
    }

    pub fn textured(&self) -> bool {
        self.disp3dcnt & TEXTURES != 0
    }

    fn blending(&self) -> bool {
        self.disp3dcnt & ALPHA_BLENDING != 0
    }

    fn edge_color(&self, id: u8) -> [u8; 3] {
        let color = self.edge[usize::from(id >> 3) & 7];
        [0, 5, 10].map(|shift| expand(color >> shift))
    }

    fn fog_density(&self, depth: u32) -> u32 {
        let offset = self.fog_offset * DEPTH_STEP;
        let (index, fraction) = if depth < offset {
            (0, 0)
        } else {
            let shift = (self.disp3dcnt >> FOG_SHIFT_SHIFT) & 0xF;
            let z = ((depth - offset) >> 2).wrapping_shl(shift);
            match (z >> 17) as usize {
                index @ 0..32 => (index, z & 0x1FFFF),
                _ => (32, 0),
            }
        };
        let entry = |index: usize| u32::from(self.fog[index.saturating_sub(1).min(31)]);
        let density = (entry(index) * (0x20000 - fraction) + entry(index + 1) * fraction) >> 17;
        if density >= 127 { 128 } else { density }
    }

    pub fn rear_color(&self) -> Pixel {
        let color = self.clear_color as u16;
        Pixel {
            r: expand(color),
            g: expand(color >> 5),
            b: expand(color >> 10),
            alpha: ((self.clear_color >> CLEAR_ALPHA_SHIFT) & ALPHA_MASK) as u8,
        }
    }

    pub fn rear_attribute(&self) -> Attribute {
        Attribute::new(
            polygon_id(self.clear_color),
            None,
            self.clear_color & FOG != 0,
            false,
            0,
            0,
        )
    }

    pub fn marks_edges_or_fogs(&self) -> bool {
        self.disp3dcnt & (EDGE_MARKING | FOG_ENABLE) != 0
    }

    pub fn edge_colors(&self) -> [u16; 8] {
        self.edge
    }

    pub fn fog_color(&self) -> u32 {
        self.fog_color
    }

    pub fn fog_offset(&self) -> u32 {
        self.fog_offset
    }

    pub fn fog_table(&self) -> [u8; 32] {
        self.fog
    }

    pub fn rear_depth(&self) -> u32 {
        let depth = self.clear_depth & DEPTH_FAR;
        depth * DEPTH_STEP + (depth + 1) / (DEPTH_FAR + 1) * DEPTH_FILL
    }
}

fn mode(polygon: &Polygon) -> u32 {
    (polygon.attributes >> MODE_SHIFT) & MODE_MASK
}

fn polygon_alpha(polygon: &Polygon) -> u8 {
    match ((polygon.attributes >> ALPHA_SHIFT) & ALPHA_MASK) as u8 {
        0 => OPAQUE,
        alpha => alpha,
    }
}

fn polygon_id(attributes: u32) -> u8 {
    ((attributes >> ID_SHIFT) & ID_MASK) as u8
}

const EXPANDED: [u8; 32] = {
    let mut table = [0; 32];
    let mut level = 1;
    while level < 32 {
        table[level] = level as u8 * 2 + 1;
        level += 1;
    }
    table
};

fn expand(channel: u16) -> u8 {
    EXPANDED[usize::from(channel & 0x1F)]
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct Attribute(u32);

impl Attribute {
    pub const LEFT: u8 = 1 << 0;
    pub const RIGHT: u8 = 1 << 1;
    pub const TOP: u8 = 1 << 2;
    pub const BOTTOM: u8 = 1 << 3;

    const TRANSLUCENT_SHIFT: u32 = 8;
    const HAS_TRANSLUCENT: u32 = 1 << 14;
    const FOG: u32 = 1 << 15;
    const BACK: u32 = 1 << 16;
    const EDGES_SHIFT: u32 = 17;
    const EDGES_MASK: u32 = 0xF;
    const COVERAGE_SHIFT: u32 = 24;
    const COVERAGE: u32 = 0xFF << Self::COVERAGE_SHIFT;
    const EDGES: u32 = Self::EDGES_MASK << Self::EDGES_SHIFT;

    pub fn new(
        opaque_id: u8,
        translucent_id: Option<u8>,
        fog: bool,
        back: bool,
        edges: u8,
        coverage: u8,
    ) -> Self {
        let opaque = Self((u32::from(opaque_id) & ID_MASK) | Self::flags(fog, back))
            .with_edges_and_coverage(edges, coverage);
        match translucent_id {
            Some(id) => opaque.translucent(id, fog, back),
            None => opaque,
        }
    }

    #[inline(always)]
    fn flags(fog: bool, back: bool) -> u32 {
        (if fog { Self::FOG } else { 0 }) | (if back { Self::BACK } else { 0 })
    }

    #[inline(always)]
    pub fn with_edges_and_coverage(self, edges: u8, coverage: u8) -> Self {
        Self(
            (self.0 & !(Self::EDGES | Self::COVERAGE))
                | ((u32::from(edges) & Self::EDGES_MASK) << Self::EDGES_SHIFT)
                | (u32::from(coverage) << Self::COVERAGE_SHIFT),
        )
    }

    #[inline(always)]
    pub fn with_coverage(self, coverage: u8) -> Self {
        Self((self.0 & !Self::COVERAGE) | (u32::from(coverage) << Self::COVERAGE_SHIFT))
    }

    #[inline(always)]
    pub fn translucent(self, id: u8, fog: bool, back: bool) -> Self {
        Self(
            (self.0 & (ID_MASK | Self::EDGES | Self::COVERAGE))
                | ((u32::from(id) & ID_MASK) << Self::TRANSLUCENT_SHIFT)
                | Self::HAS_TRANSLUCENT
                | Self::flags(fog, back),
        )
    }

    #[inline(always)]
    pub fn opaque_id(self) -> u8 {
        (self.0 & ID_MASK) as u8
    }

    #[inline(always)]
    pub fn translucent_id(self) -> Option<u8> {
        (self.0 & Self::HAS_TRANSLUCENT != 0)
            .then_some(((self.0 >> Self::TRANSLUCENT_SHIFT) & ID_MASK) as u8)
    }

    #[inline(always)]
    pub fn fog(self) -> bool {
        self.0 & Self::FOG != 0
    }

    #[inline(always)]
    pub fn back(self) -> bool {
        self.0 & Self::BACK != 0
    }

    #[inline(always)]
    pub fn edges(self) -> u8 {
        ((self.0 >> Self::EDGES_SHIFT) & Self::EDGES_MASK) as u8
    }

    #[inline(always)]
    pub fn coverage(self) -> u8 {
        (self.0 >> Self::COVERAGE_SHIFT) as u8
    }

    #[inline(always)]
    fn back_without_translucent(self) -> bool {
        self.0 & (Self::BACK | Self::HAS_TRANSLUCENT) == Self::BACK
    }
}

impl std::fmt::Debug for Attribute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attribute")
            .field("opaque_id", &self.opaque_id())
            .field("translucent_id", &self.translucent_id())
            .field("fog", &self.fog())
            .field("back", &self.back())
            .field("edges", &self.edges())
            .field("coverage", &self.coverage())
            .finish()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub alpha: u8,
}

impl Pixel {
    fn rgb666(&self) -> u32 {
        color::from_channels(self.r, self.g, self.b)
    }

    fn over(self, target: Pixel, blending: bool) -> Pixel {
        if target.alpha == 0 {
            return self;
        }
        let mut out = self;
        if blending {
            let weight = u32::from(self.alpha) + 1;
            let mix = |source: u8, target: u8| {
                ((u32::from(source) * weight + u32::from(target) * (32 - weight)) >> 5) as u8
            };
            out.r = mix(self.r, target.r);
            out.g = mix(self.g, target.g);
            out.b = mix(self.b, target.b);
        }
        out.alpha = self.alpha.max(target.alpha);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stencil(Vec<u8>);

impl Default for Stencil {
    fn default() -> Self {
        Self(vec![0; WIDTH])
    }
}

#[derive(Debug)]
pub struct Renderer {
    registers: Registers,
    color: Vec<Pixel>,
    depth: Vec<u32>,
    attributes: Vec<Attribute>,
    under: Under,
    stencil: Vec<u8>,
    rows: Vec<(i32, i32)>,
    buckets: Buckets,
    sides: Vec<[Option<Side>; 2]>,
    setups: Vec<Option<Setup>>,
    serial: Option<u64>,
    begun: Option<u64>,
    banding: bool,
    ready: usize,
}

#[derive(Debug)]
struct Under {
    color: Vec<Pixel>,
    depth: Vec<u32>,
    attributes: Vec<Attribute>,
}

impl Under {
    fn blank() -> Self {
        Self {
            color: vec![Pixel::default(); WIDTH * HEIGHT],
            depth: vec![0; WIDTH * HEIGHT],
            attributes: vec![Attribute::default(); WIDTH * HEIGHT],
        }
    }

    fn none() -> Self {
        Self {
            color: Vec::new(),
            depth: Vec::new(),
            attributes: Vec::new(),
        }
    }
}

impl Clone for Renderer {
    fn clone(&self) -> Self {
        Self {
            registers: self.registers,
            color: Vec::new(),
            depth: Vec::new(),
            attributes: Vec::new(),
            under: Under::none(),
            stencil: self.stencil.clone(),
            rows: self.rows.clone(),
            buckets: self.buckets.clone(),
            sides: self.sides.clone(),
            setups: self.setups.clone(),
            serial: self.serial,
            begun: self.begun,
            banding: self.banding,
            ready: self.ready,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.registers = source.registers;
        self.stencil.clone_from(&source.stencil);
        self.rows.clone_from(&source.rows);
        self.buckets.clone_from(&source.buckets);
        self.sides.clone_from(&source.sides);
        self.setups.clone_from(&source.setups);
        self.serial = source.serial;
        self.begun = source.begun;
        self.banding = source.banding;
        self.ready = source.ready;
    }
}

impl Default for Renderer {
    fn default() -> Self {
        let Under {
            color,
            depth,
            attributes,
        } = Under::blank();
        Self {
            registers: Registers::default(),
            color,
            depth,
            attributes,
            under: Under::blank(),
            stencil: vec![0; WIDTH],
            rows: Vec::new(),
            buckets: Buckets::default(),
            sides: Vec::new(),
            setups: Vec::new(),
            serial: None,
            begun: None,
            banding: false,
            ready: 0,
        }
    }
}

impl Renderer {
    pub fn start_frame(&mut self, registers: Registers) {
        self.registers = registers;
        self.ready = 0;
    }

    pub fn share_frame(&mut self, serial: u64) {
        self.serial = Some(serial);
    }

    pub fn pixel(&self, x: usize, y: usize) -> Pixel {
        self.color[y * WIDTH + x]
    }

    pub fn depth(&self, x: usize, y: usize) -> u32 {
        self.depth[y * WIDTH + x]
    }

    pub fn attribute(&self, x: usize, y: usize) -> Attribute {
        self.attributes[y * WIDTH + x]
    }

    pub fn under(&self, x: usize, y: usize) -> (Pixel, u32, Attribute) {
        let index = y * WIDTH + x;
        (
            self.under.color[index],
            self.under.depth[index],
            self.under.attributes[index],
        )
    }

    pub fn render_line<M: TextureMemory>(
        &mut self,
        y: usize,
        frame: &Frame,
        memory: &M,
        out: &mut Line3d,
    ) {
        if y == 0 {
            self.ready = 0;
        }
        let ahead = if self.registers.disp3dcnt & EDGE_MARKING != 0 {
            (y + 1).min(HEIGHT - 1)
        } else {
            y
        };
        while self.ready <= ahead {
            self.rasterize(self.ready, frame, memory);
            self.ready += 1;
        }
        let row = y * WIDTH;
        if self.registers.disp3dcnt & EDGE_MARKING != 0 {
            self.mark_edges(y);
        }
        if self.registers.disp3dcnt & FOG_ENABLE != 0 {
            self.fog(row);
        }
        if self.registers.disp3dcnt & ANTIALIASING != 0 {
            self.antialias(row);
        }
        for (x, pixel) in self.color[row..row + WIDTH].iter().enumerate() {
            out.color[x] = pixel.rgb666();
            out.alpha[x] = pixel.alpha;
        }
    }

    pub fn render_band<M: TextureMemory>(
        &mut self,
        band: Range<usize>,
        frame: &Frame,
        memory: &M,
        carried: &Stencil,
        mut out: impl FnMut(usize, &Line3d),
    ) -> Option<Stencil> {
        let serial = self.serial.take();
        if band.is_empty() {
            return None;
        }
        let edges = self.registers.disp3dcnt & EDGE_MARKING != 0;
        let first = if edges {
            band.start.saturating_sub(1)
        } else {
            band.start
        };
        let last = if edges { band.end } else { band.end - 1 }.min(HEIGHT - 1);
        if serial.is_none() || serial != self.begun {
            self.begin(frame, memory);
        }
        self.begun = serial;
        self.banding = true;
        let shadows = Shadows::new(frame, &self.rows);
        self.carry(first, last, &shadows, frame, memory, carried);
        self.ready = first;
        let mut line = Line3d::default();
        for y in band.clone() {
            self.render_line(y, frame, memory, &mut line);
            out(y, &line);
        }
        self.banding = false;
        let end = shadows.masks.iter().rposition(|&mask| mask)?;
        band.contains(&end).then(|| Stencil(self.stencil.clone()))
    }

    pub fn end_stencil<M: TextureMemory>(
        &mut self,
        frame: &Frame,
        memory: &M,
        carried: &Stencil,
    ) -> Option<Stencil> {
        self.serial = None;
        self.begin(frame, memory);
        let end = Shadows::new(frame, &self.rows)
            .masks
            .iter()
            .rposition(|&mask| mask)?;
        self.render_band(end..end + 1, frame, memory, carried, |_, _| {})
    }

    fn carry<M: TextureMemory>(
        &mut self,
        first: usize,
        last: usize,
        shadows: &Shadows,
        frame: &Frame,
        memory: &M,
        carried: &Stencil,
    ) {
        let until = (first..=last).find(|&y| shadows.masks[y]).unwrap_or(last);
        if !(first..=until).any(|y| shadows.reads[y]) {
            return;
        }
        self.stencil.copy_from_slice(&carried.0);
        let mut chain = Vec::new();
        let mut below = first;
        while let Some(y) = (0..below).rev().find(|&y| shadows.masks[y]) {
            chain.push(y);
            if !shadows.reads[y] {
                break;
            }
            below = y;
        }
        for &y in chain.iter().rev() {
            self.rasterize(y, frame, memory);
        }
    }

    fn begin<M: TextureMemory>(&mut self, frame: &Frame, memory: &M) {
        self.begun = None;
        self.setups.clear();
        self.setups.extend(
            frame
                .polygons
                .iter()
                .map(|polygon| Setup::new(frame, polygon, self.registers.disp3dcnt, memory)),
        );
        self.rows.clear();
        self.rows
            .extend(self.setups.iter().map(|setup| match setup {
                Some(setup) if setup.top == setup.bottom => (setup.top, setup.top),
                Some(setup) => (setup.top, setup.bottom - 1),
                None => (1, 0),
            }));
        self.buckets.fill(&self.rows);
        self.sides.clear();
        self.sides.resize(frame.polygons.len(), [None; 2]);
    }

    fn rasterize<M: TextureMemory>(&mut self, y: usize, frame: &Frame, memory: &M) {
        if self.color.is_empty() {
            let Under {
                color,
                depth,
                attributes,
            } = Under::blank();
            (self.color, self.depth, self.attributes) = (color, depth, attributes);
            self.under = Under::blank();
        }
        let row = y * WIDTH;
        let rear = (
            self.registers.rear_color(),
            self.registers.rear_depth(),
            self.registers.rear_attribute(),
        );
        self.color[row..row + WIDTH].fill(rear.0);
        self.depth[row..row + WIDTH].fill(rear.1);
        self.attributes[row..row + WIDTH].fill(rear.2);
        self.under.color[row..row + WIDTH].fill(rear.0);
        self.under.depth[row..row + WIDTH].fill(rear.1);
        self.under.attributes[row..row + WIDTH].fill(rear.2);
        if y == 0 && !self.banding {
            self.begin(frame, memory);
        }
        let buckets = std::mem::take(&mut self.buckets);
        let mut sides = std::mem::take(&mut self.sides);
        let setups = std::mem::take(&mut self.setups);
        let mut after_mask = false;
        for &index in buckets.line(y) {
            let index = index as usize;
            let (Some(polygon), Some(side), Some(Some(setup))) = (
                frame.polygons.get(index),
                sides.get_mut(index),
                setups.get(index),
            ) else {
                continue;
            };
            let Some(scan) = Scan::new(
                frame,
                polygon,
                setup,
                y as i32,
                self.registers.disp3dcnt,
                side,
            ) else {
                continue;
            };
            if setup.mask && !after_mask {
                self.stencil.fill(0);
            }
            after_mask = setup.mask;
            if setup.mask {
                self.draw_mask(row, polygon, &scan);
            } else {
                self.draw(y, polygon, setup, &scan, memory);
            }
        }
        self.buckets = buckets;
        self.sides = sides;
        self.setups = setups;
    }

    fn mark_edges(&mut self, y: usize) {
        let row = y * WIDTH;
        let rear_id = self.registers.rear_attribute().opaque_id();
        let rear_depth = self.registers.rear_depth();
        let neighbour = |x: isize, y: isize| -> (u8, u32) {
            if x < 0 || y < 0 || x >= WIDTH as isize || y >= HEIGHT as isize {
                (rear_id, rear_depth)
            } else {
                let index = y as usize * WIDTH + x as usize;
                (self.attributes[index].opaque_id(), self.depth[index])
            }
        };
        let mut marked = [false; WIDTH];
        for (x, mark) in marked.iter_mut().enumerate() {
            let attribute = self.attributes[row + x];
            if attribute.edges() == 0 {
                continue;
            }
            let depth = self.depth[row + x];
            let (x, y) = (x as isize, y as isize);
            *mark = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                .into_iter()
                .any(|(nx, ny)| {
                    let (id, other) = neighbour(nx, ny);
                    id != attribute.opaque_id() && depth < other
                });
        }
        for (x, _) in marked.iter().enumerate().filter(|(_, mark)| **mark) {
            let index = row + x;
            let [r, g, b] = self
                .registers
                .edge_color(self.attributes[index].opaque_id());
            let pixel = &mut self.color[index];
            (pixel.r, pixel.g, pixel.b) = (r, g, b);
            self.attributes[index] = self.attributes[index].with_coverage(EDGE_COVERAGE);
        }
    }

    fn fog(&mut self, row: usize) {
        let color = self.registers.fog_color;
        let fog = [0, 5, 10].map(|shift| u32::from(expand((color >> shift) as u16)));
        let fog_alpha = (color >> CLEAR_ALPHA_SHIFT) & ALPHA_MASK;
        let alpha_only = self.registers.disp3dcnt & FOG_ALPHA_ONLY != 0;
        let registers = self.registers;
        let apply = |pixel: &mut Pixel, depth: u32| {
            let density = registers.fog_density(depth);
            let mix = |fog: u32, value: u8| {
                ((fog * density + u32::from(value) * (128 - density)) >> 7) as u8
            };
            if !alpha_only {
                pixel.r = mix(fog[0], pixel.r);
                pixel.g = mix(fog[1], pixel.g);
                pixel.b = mix(fog[2], pixel.b);
            }
            pixel.alpha = mix(fog_alpha, pixel.alpha);
        };
        for index in row..row + WIDTH {
            let attribute = self.attributes[index];
            if attribute.fog() {
                apply(&mut self.color[index], self.depth[index]);
            }
            if attribute.edges() != 0 && self.under.attributes[index].fog() {
                apply(&mut self.under.color[index], self.under.depth[index]);
            }
        }
    }

    fn antialias(&mut self, row: usize) {
        for x in 0..WIDTH {
            let index = row + x;
            let attribute = self.attributes[index];
            if attribute.edges() == 0 || attribute.coverage() == FULL_COVERAGE {
                continue;
            }
            let below = self.under.color[index];
            if attribute.coverage() == 0 {
                self.color[index] = below;
                continue;
            }
            let weight = u32::from(attribute.coverage()) + 1;
            let mix = |top: u8, below: u8| {
                ((u32::from(top) * weight + u32::from(below) * (32 - weight)) >> 5) as u8
            };
            let top = self.color[index];
            let mut out = top;
            if below.alpha > 0 {
                out.r = mix(top.r, below.r);
                out.g = mix(top.g, below.g);
                out.b = mix(top.b, below.b);
            }
            out.alpha = mix(top.alpha, below.alpha);
            self.color[index] = out;
        }
    }

    fn depth_test(polygon: &Polygon, depth: i32, target: u32, attribute: Attribute) -> bool {
        let target = target as i32;
        if polygon.attributes & DEPTH_EQUAL != 0 {
            let tolerance = if polygon.w_buffer { EQUAL_W } else { EQUAL_Z };
            (target - depth).abs() <= tolerance
        } else if polygon.front && attribute.back_without_translucent() {
            depth <= target
        } else {
            depth < target
        }
    }

    fn draw_mask(&mut self, row: usize, polygon: &Polygon, scan: &Scan) {
        if polygon_alpha(polygon) <= self.registers.alpha_threshold() {
            return;
        }
        let mut across = scan.across(polygon);
        let depth_lane = across.depth_lane(scan.left.depth, scan.right.depth);
        for (x, _) in scan.pixels() {
            across.place(x);
            let depth = across.depth_value(&depth_lane);
            let index = row + x as usize;
            let top = self.attributes[index];
            let stencil = &mut self.stencil[x as usize];
            if !Self::depth_test(polygon, depth, self.depth[index], top) {
                *stencil = STENCIL_TOP;
            }
            if top.edges() != 0
                && !Self::depth_test(
                    polygon,
                    depth,
                    self.under.depth[index],
                    self.under.attributes[index],
                )
            {
                *stencil |= STENCIL_UNDER;
            }
        }
    }

    fn draw<M: TextureMemory>(
        &mut self,
        y: usize,
        polygon: &Polygon,
        setup: &Setup,
        scan: &Scan,
        memory: &M,
    ) {
        let shadow = setup.mode == SHADOW;
        let textured = setup.sampler.is_some();
        let antialiasing = self.registers.disp3dcnt & ANTIALIASING != 0;
        if setup.decoded
            && let Some(texels) = memory.decoded(polygon.texture, polygon.palette)
        {
            let decoded = texture::DecodedTexture::new(texels);
            let draw = match (shadow, antialiasing) {
                (false, false) => Self::draw_as::<texture::DecodedTexture, false, true, false>,
                (false, true) => Self::draw_as::<texture::DecodedTexture, false, true, true>,
                (true, false) => Self::draw_as::<texture::DecodedTexture, true, true, false>,
                (true, true) => Self::draw_as::<texture::DecodedTexture, true, true, true>,
            };
            draw(self, y, polygon, setup, scan, &decoded);
            return;
        }
        let draw = match (shadow, textured, antialiasing) {
            (false, false, false) => Self::draw_as::<M, false, false, false>,
            (false, false, true) => Self::draw_as::<M, false, false, true>,
            (false, true, false) => Self::draw_as::<M, false, true, false>,
            (false, true, true) => Self::draw_as::<M, false, true, true>,
            (true, false, false) => Self::draw_as::<M, true, false, false>,
            (true, false, true) => Self::draw_as::<M, true, false, true>,
            (true, true, false) => Self::draw_as::<M, true, true, false>,
            (true, true, true) => Self::draw_as::<M, true, true, true>,
        };
        draw(self, y, polygon, setup, scan, memory);
    }

    fn draw_as<
        M: TextureMemory,
        const SHADOWS: bool,
        const TEXTURED: bool,
        const ANTIALIASED: bool,
    >(
        &mut self,
        y: usize,
        polygon: &Polygon,
        setup: &Setup,
        scan: &Scan,
        memory: &M,
    ) {
        let mut across = scan.across(polygon);
        let (left, right) = (&scan.left, &scan.right);
        let depth_lane = across.depth_lane(left.depth, right.depth);
        let color = |channel: usize| across.lane(left.color[channel], right.color[channel]);
        let colors = [color(0), color(1), color(2)];
        let texcoord = |axis: usize| across.lane(left.texcoord[axis], right.texcoord[axis]);
        let texcoords = [texcoord(0), texcoord(1)];
        let mode = setup.mode;
        let shadow = SHADOWS;
        let raw_alpha = ((polygon.attributes >> ALPHA_SHIFT) & ALPHA_MASK) as u8;
        let wireframe = raw_alpha == 0;
        let registers = &self.registers;
        let textured = TEXTURED;
        let antialiasing = ANTIALIASED;
        let highlight = registers.disp3dcnt & HIGHLIGHT != 0;
        let threshold = registers.alpha_threshold();
        let blending = registers.blending();
        let sampler = setup.sampler.unwrap_or_else(|| texture::Sampler::new(0, 0));
        let opaque = setup.opaque;
        let mut top = Layer {
            color: line(&mut self.color, y),
            depth: line(&mut self.depth, y),
            attributes: line(&mut self.attributes, y),
        };
        let mut below = Layer {
            color: line(&mut self.under.color, y),
            depth: line(&mut self.under.depth, y),
            attributes: line(&mut self.under.attributes, y),
        };
        for (pixels, edge, coverage) in scan.regions() {
            let edges = scan.rows | edge;
            let mut ramp = match coverage {
                Some((Coverage::Ramp { start, .. }, _)) => start,
                _ => 0,
            };
            for x in pixels {
                let at = x as usize & (WIDTH - 1);
                let stencil = if shadow {
                    self.stencil[at]
                } else {
                    STENCIL_TOP | STENCIL_UNDER
                };
                if stencil == 0 {
                    continue;
                }
                across.set(x);
                let depth = across.depth_value(&depth_lane);
                let above = top.attributes[at];
                let top_edges = if stencil & STENCIL_UNDER != 0 {
                    above.edges()
                } else {
                    0
                };
                let under = if stencil & STENCIL_TOP == 0 {
                    if Self::depth_test(polygon, depth, below.depth[at], above) {
                        true
                    } else {
                        continue;
                    }
                } else if Self::depth_test(polygon, depth, top.depth[at], above) {
                    false
                } else if top_edges != 0
                    && Self::depth_test(polygon, depth, below.depth[at], below.attributes[at])
                {
                    true
                } else {
                    continue;
                };
                let channel = |lane: &Lane| (across.value(lane) >> 3) as u8;
                let vertex = [
                    channel(&colors[0]),
                    channel(&colors[1]),
                    channel(&colors[2]),
                ];
                let texel = textured.then(|| {
                    let s = i32::from(across.value(&texcoords[0]) as i16);
                    let t = i32::from(across.value(&texcoords[1]) as i16);
                    sampler.sample(memory, s, t)
                });
                let mut pixel = if mode == TOON {
                    shade::shade_toon(vertex, raw_alpha, texel, &registers.toon, highlight)
                } else {
                    shade::shade(mode, vertex, raw_alpha, texel)
                };
                if wireframe {
                    pixel.alpha = OPAQUE;
                }
                if pixel.alpha <= threshold {
                    continue;
                }
                if pixel.alpha != OPAQUE {
                    let layer = if under { &mut below } else { &mut top };
                    layer.plot_translucent(at, pixel, depth, polygon, shadow, blending);
                    if !under && top_edges != 0 {
                        below.plot_translucent(at, pixel, depth, polygon, shadow, blending);
                    }
                    continue;
                }
                let coverage = if antialiasing {
                    if edges != 0 && !under {
                        below.color[at] = top.color[at];
                        below.depth[at] = top.depth[at];
                        below.attributes[at] = above;
                    }
                    match coverage {
                        Some((Coverage::Fixed(level), _)) => level,
                        Some((Coverage::Ramp { step, .. }, from_right)) => {
                            let level = (ramp >> 5).min(i32::from(FULL_COVERAGE));
                            ramp += step;
                            if from_right {
                                (i32::from(FULL_COVERAGE) - level).max(0) as u8
                            } else {
                                level as u8
                            }
                        }
                        None => FULL_COVERAGE,
                    }
                } else {
                    0
                };
                let attribute = opaque.with_edges_and_coverage(edges, coverage);
                if under {
                    below.plot(at, pixel, depth, attribute);
                } else {
                    top.plot(at, pixel, depth, attribute);
                }
            }
        }
    }
}

fn line<T>(buffer: &mut [T], y: usize) -> &mut [T; WIDTH] {
    &mut buffer.as_chunks_mut::<WIDTH>().0[y]
}

struct Layer<'a> {
    color: &'a mut [Pixel; WIDTH],
    depth: &'a mut [u32; WIDTH],
    attributes: &'a mut [Attribute; WIDTH],
}

impl Layer<'_> {
    #[inline(always)]
    fn plot(&mut self, at: usize, pixel: Pixel, depth: i32, attribute: Attribute) {
        self.depth[at] = depth as u32;
        self.color[at] = pixel;
        self.attributes[at] = attribute;
    }

    fn plot_translucent(
        &mut self,
        at: usize,
        pixel: Pixel,
        depth: i32,
        polygon: &Polygon,
        shadow: bool,
        blending: bool,
    ) {
        let target = self.attributes[at];
        let id = polygon_id(polygon.attributes);
        let same = match target.translucent_id() {
            Some(translucent) => translucent == id,
            None => shadow && target.opaque_id() == id,
        };
        if same {
            return;
        }
        self.color[at] = pixel.over(self.color[at], blending);
        if polygon.attributes & TRANSLUCENT_DEPTH != 0 {
            self.depth[at] = depth as u32;
        }
        self.attributes[at] = target.translucent(
            id,
            target.fog() && polygon.attributes & FOG != 0,
            !polygon.front,
        );
    }
}

#[derive(Debug, Clone, Copy)]
struct Edge {
    slope: Slope,
    to_x: i32,
    values: Values,
}

#[derive(Debug, Clone, Copy)]
struct Values {
    w: i32,
    depth: i32,
    color: [i32; 3],
    texcoord: [i32; 2],
}

#[derive(Debug, Clone, Copy)]
struct Lanes {
    w: Lane,
    depth: DepthLane,
    color: [Lane; 3],
    texcoord: [Lane; 2],
}

impl Lanes {
    fn new(along: &Interpolator, frame: &Frame, polygon: &Polygon, from: usize, to: usize) -> Self {
        let (a, b) = (frame.vertex(polygon, from), frame.vertex(polygon, to));
        let color = |channel: usize| along.lane(widen(a.color[channel]), widen(b.color[channel]));
        let texcoord =
            |axis: usize| along.lane(i32::from(a.texcoord[axis]), i32::from(b.texcoord[axis]));
        Self {
            w: along.lane(polygon.w[from], polygon.w[to]),
            depth: along.depth_lane(polygon.depth[from], polygon.depth[to]),
            color: [color(0), color(1), color(2)],
            texcoord: [texcoord(0), texcoord(1)],
        }
    }

    fn values(&self, along: &Interpolator) -> Values {
        let [r, g, b] = &self.color;
        let [s, t] = &self.texcoord;
        Values {
            w: along.value(&self.w),
            depth: along.depth_value(&self.depth),
            color: [along.value(r), along.value(g), along.value(b)],
            texcoord: [along.value(s), along.value(t)],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Side {
    y: i32,
    from: usize,
    to: usize,
    to_x: i32,
    to_y: i32,
    slope: Slope,
    along: Interpolator,
    lanes: Lanes,
}

impl Side {
    fn new(frame: &Frame, polygon: &Polygon, from: usize, to: usize, left: bool) -> Self {
        let end = screen(frame, polygon, to);
        let slope = Slope::new(screen(frame, polygon, from), end);
        let offset = i32::from(slope.increment() >= slope::ONE && left == slope.negative());
        let along = Interpolator::new(
            Axis::Y,
            [from, to].map(|index| screen(frame, polygon, index)[1] - offset),
            [polygon.w[from], polygon.w[to]],
            polygon.w_buffer,
        );
        Self {
            y: 0,
            from,
            to,
            to_x: end[0],
            to_y: end[1],
            slope,
            along,
            lanes: Lanes::new(&along, frame, polygon, from, to),
        }
    }

    fn edge(&mut self, y: i32) -> Edge {
        self.y = y;
        self.along.set(y);
        Edge {
            slope: self.slope,
            to_x: self.to_x,
            values: self.lanes.values(&self.along),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Buckets {
    starts: Vec<u32>,
    polygons: Vec<u32>,
}

impl Buckets {
    fn fill(&mut self, rows: &[(i32, i32)]) {
        let lines = |&(first, last): &(i32, i32)| {
            first.max(0) as usize..(last + 1).clamp(0, HEIGHT as i32) as usize
        };
        self.starts.clear();
        self.starts.resize(HEIGHT + 1, 0);
        for y in rows.iter().flat_map(lines) {
            self.starts[y + 1] += 1;
        }
        for y in 0..HEIGHT {
            self.starts[y + 1] += self.starts[y];
        }
        self.polygons.clear();
        self.polygons.resize(self.starts[HEIGHT] as usize, 0);
        let mut next = [0; HEIGHT];
        next.copy_from_slice(&self.starts[..HEIGHT]);
        for (index, range) in rows.iter().map(lines).enumerate() {
            for y in range {
                self.polygons[next[y] as usize] = index as u32;
                next[y] += 1;
            }
        }
    }

    fn line(&self, y: usize) -> &[u32] {
        match self.starts.get(y..y + 2) {
            Some(&[start, end]) => &self.polygons[start as usize..end as usize],
            _ => &[],
        }
    }
}

fn widen(channel: u8) -> i32 {
    match i32::from(channel) {
        0 => 0,
        level => level << 3 | 7,
    }
}

impl Edge {
    fn walk(
        frame: &Frame,
        polygon: &Polygon,
        y: i32,
        left: bool,
        cache: &mut Option<Side>,
    ) -> Self {
        if let Some(side) = cache
            && side.y <= y
            && y < side.to_y
        {
            return side.edge(y);
        }
        let count = usize::from(polygon.count);
        let forward = left == polygon.front;
        let step = |index: usize| {
            if forward {
                (index + 1) % count
            } else {
                (index + count - 1) % count
            }
        };
        let bottom = usize::from(polygon.bottom);
        let (mut from, mut to) = match cache {
            Some(side) if side.y <= y => (side.from, side.to),
            _ => {
                let top = usize::from(polygon.top);
                (top, step(top))
            }
        };
        while y >= screen(frame, polygon, to)[1] && from != bottom {
            from = to;
            to = step(from);
        }
        if !matches!(cache, Some(side) if side.from == from && side.to == to) {
            *cache = None;
        }
        cache
            .get_or_insert_with(|| Side::new(frame, polygon, from, to, left))
            .edge(y)
    }

    fn flat(frame: &Frame, polygon: &Polygon, rightmost: bool) -> Self {
        let last = usize::from(polygon.count) - 1;
        let x = |index: usize| screen(frame, polygon, index)[0];
        let index = [1, last].into_iter().fold(0, |best, index| {
            let further = if rightmost {
                x(index) > x(best)
            } else {
                x(index) < x(best)
            };
            if further { index } else { best }
        });
        let point = screen(frame, polygon, index);
        let still = Interpolator::new(Axis::Y, [0, 0], [0, 0], polygon.w_buffer);
        Self {
            slope: Slope::new(point, point),
            to_x: point[0],
            values: Lanes::new(&still, frame, polygon, index, index).values(&still),
        }
    }
}

struct Shadows {
    masks: [bool; HEIGHT],
    reads: [bool; HEIGHT],
}

impl Shadows {
    fn new(frame: &Frame, rows: &[(i32, i32)]) -> Self {
        let mut shadows = Self {
            masks: [false; HEIGHT],
            reads: [false; HEIGHT],
        };
        for (polygon, &(first, last)) in frame.polygons.iter().zip(rows) {
            if mode(polygon) != SHADOW {
                continue;
            }
            let mask = polygon_id(polygon.attributes) == 0;
            for y in first.max(0)..=last.min(HEIGHT as i32 - 1) {
                let y = y as usize;
                if mask {
                    shadows.masks[y] = true;
                } else if !shadows.masks[y] {
                    shadows.reads[y] = true;
                }
            }
        }
        shadows
    }
}

#[derive(Debug, Clone, Copy)]
struct Setup {
    top: i32,
    bottom: i32,
    mode: u32,
    mask: bool,
    fill_all: bool,
    opaque: Attribute,
    decoded: bool,
    sampler: Option<texture::Sampler>,
}

impl Setup {
    fn new<M: TextureMemory>(
        frame: &Frame,
        polygon: &Polygon,
        disp3dcnt: u32,
        memory: &M,
    ) -> Option<Self> {
        if polygon.degenerate || polygon.count < 3 {
            return None;
        }
        let mode = mode(polygon);
        let alpha = (polygon.attributes >> ALPHA_SHIFT) & ALPHA_MASK;
        let format = texture::format(polygon.texture);
        let textured = disp3dcnt & TEXTURES != 0 && format != 0;
        let decoded = (textured && format == texture::COMPRESSED)
            .then(|| memory.decoded(polygon.texture, polygon.palette))
            .flatten();
        Some(Self {
            top: screen(frame, polygon, usize::from(polygon.top))[1],
            bottom: screen(frame, polygon, usize::from(polygon.bottom))[1],
            mode,
            mask: mode == SHADOW && polygon_id(polygon.attributes) == 0,
            fill_all: disp3dcnt & (ANTIALIASING | EDGE_MARKING) != 0
                || (alpha < u32::from(OPAQUE) && disp3dcnt & ALPHA_BLENDING != 0)
                || alpha == 0,
            opaque: Attribute::new(
                polygon_id(polygon.attributes),
                None,
                polygon.attributes & FOG != 0,
                !polygon.front,
                0,
                0,
            ),
            decoded: decoded.is_some(),
            sampler: textured.then(|| match decoded {
                Some(texels) => {
                    texture::DecodedTexture::new(texels).sampler(polygon.texture, polygon.palette)
                }
                None => memory.sampler(polygon.texture, polygon.palette),
            }),
        })
    }
}

fn screen(frame: &Frame, polygon: &Polygon, index: usize) -> [i32; 2] {
    frame.vertex(polygon, index).screen.map(i32::from)
}

type Region = (Range<i32>, u8, Option<(Coverage, bool)>);

#[derive(Debug, Clone, Copy)]
struct Scan {
    left: Values,
    right: Values,
    start: i32,
    end: i32,
    left_end: i32,
    right_start: i32,
    fill_left: bool,
    fill_right: bool,
    fill_inside: bool,
    rows: u8,
    left_coverage: Coverage,
    right_coverage: Coverage,
}

impl Scan {
    fn new(
        frame: &Frame,
        polygon: &Polygon,
        setup: &Setup,
        y: i32,
        disp3dcnt: u32,
        sides: &mut [Option<Side>; 2],
    ) -> Option<Self> {
        let (top, bottom) = (setup.top, setup.bottom);
        let flat = top == bottom;
        if y < top || (y >= bottom && !(flat && y == top)) {
            return None;
        }
        let (left, right) = if flat {
            (
                Edge::flat(frame, polygon, false),
                Edge::flat(frame, polygon, true),
            )
        } else {
            (
                Edge::walk(frame, polygon, y, true, &mut sides[0]),
                Edge::walk(frame, polygon, y, false, &mut sides[1]),
            )
        };

        let wireframe = (polygon.attributes >> ALPHA_SHIFT) & ALPHA_MASK == 0;
        let fill_all = setup.fill_all;

        let (left_low, left_high) = left.slope.span(y);
        let (right_low, right_high) = right.slope.span(y);
        let mut start = left_low;
        let mut end = right_high;
        if right.slope.vertical() && (!left.slope.vertical() || start != end) && end != 0 {
            end -= 1;
        }
        let last_row = y == bottom - 1;
        let apart = left.to_x != right.to_x;

        let swapped = start > end;
        let (left_length, right_length, fill_left, fill_right, left, right) = if swapped {
            std::mem::swap(&mut start, &mut end);
            let fill_left = right.slope.negative()
                || !right.slope.x_major()
                || (last_row && right.slope.x_major() && apart);
            let fill_right = (!left.slope.negative() && left.slope.x_major())
                || (!(left.slope.negative() && left.slope.x_major()) && right.slope.vertical())
                || (last_row && left.slope.x_major() && apart);
            (1, 1, fill_left, fill_right, &right, &left)
        } else {
            let length = |edge: &Edge, low: i32, high: i32| {
                if edge.slope.x_major() {
                    high - low + 1
                } else {
                    1
                }
            };
            let left_length = length(&left, left_low, left_high);
            let right_length = length(&right, right_low, right_high);
            let fill_left = left.slope.negative()
                || !left.slope.x_major()
                || (last_row && left.slope.x_major() && apart)
                || (left.slope.increment() == right.slope.increment()
                    && start + left_length == end + 1);
            let fill_right = (!right.slope.negative() && right.slope.x_major())
                || right.slope.vertical()
                || (last_row && right.slope.x_major() && apart);
            (
                left_length,
                right_length,
                fill_left,
                fill_right,
                &left,
                &right,
            )
        };

        let antialiasing = disp3dcnt & ANTIALIASING != 0;
        let coverage = |edge: &Edge, right: bool| {
            if antialiasing {
                edge.slope.coverage(y, right, swapped)
            } else {
                Coverage::Fixed(0)
            }
        };
        Some(Self {
            left_coverage: coverage(left, swapped),
            right_coverage: coverage(right, !swapped),
            left: left.values,
            right: right.values,
            start,
            end,
            left_end: (start + left_length).min(end + 1),
            right_start: end - right_length + 1,
            fill_left: fill_all || fill_left,
            fill_right: fill_all || fill_right,
            fill_inside: !wireframe || y == top || last_row,
            rows: if y == top {
                Attribute::TOP
            } else if last_row {
                Attribute::BOTTOM
            } else {
                0
            },
        })
    }

    fn across(&self, polygon: &Polygon) -> Interpolator {
        Interpolator::new(
            Axis::X,
            [self.start, self.end + 1],
            [self.left.w, self.right.w],
            polygon.w_buffer,
        )
    }

    fn regions(&self) -> [Region; 3] {
        let low = self.start.max(0);
        let high = self.end.min(WIDTH as i32 - 1) + 1;
        let clip = |from: i32, to: i32, fill: bool| {
            if fill {
                from.max(low)..to.min(high)
            } else {
                0..0
            }
        };
        [
            (
                clip(self.start, self.left_end, self.fill_left),
                Attribute::LEFT,
                Some((self.left_coverage, false)),
            ),
            (
                clip(self.left_end, self.right_start, self.fill_inside),
                0,
                None,
            ),
            (
                clip(
                    self.left_end.max(self.right_start),
                    self.end + 1,
                    self.fill_right,
                ),
                Attribute::RIGHT,
                Some((self.right_coverage, true)),
            ),
        ]
    }

    fn pixels(&self) -> impl Iterator<Item = (i32, u8)> {
        let rows = self.rows;
        self.regions()
            .into_iter()
            .flat_map(move |(pixels, edge, _)| pixels.map(move |x| (x, rows | edge)))
    }
}

#[cfg(test)]
mod tests {
    use super::super::geometry::frame::{MAX_POLYGON_VERTICES, Vertex};
    use super::texture::tests::{Memory, param};
    use super::*;

    const SOLID: u32 = 31 << ALPHA_SHIFT;

    #[test]
    fn edge_marking_and_fog_each_count_as_passes_the_upscale_lacks() {
        for (bits, expected) in [
            (0, false),
            (ANTIALIASING, false),
            (EDGE_MARKING, true),
            (FOG_ENABLE, true),
        ] {
            let mut registers = Registers::default();
            registers.write(DISP3DCNT, bits, u32::MAX);
            assert_eq!(registers.marks_edges_or_fogs(), expected, "bits {bits:#x}");
        }
    }

    fn add(frame: &mut Frame, points: &[[u16; 2]], color: [u8; 3], attributes: u32) {
        let first = frame.vertices.len() as u16;
        let mut polygon = Polygon {
            count: points.len() as u8,
            attributes,
            front: true,
            ..Polygon::default()
        };
        for (index, &screen) in points.iter().enumerate() {
            polygon.vertices[index] = first + index as u16;
            frame.vertices.push(Vertex {
                screen,
                color,
                position: [0, 0, 0, 0x1000],
                ..Vertex::default()
            });
        }
        let y = |index: usize| points[index][1];
        polygon.top = (0..points.len()).min_by_key(|&i| y(i)).unwrap() as u8;
        polygon.bottom = (0..points.len())
            .max_by_key(|&i| (y(i), points[i][0]))
            .unwrap() as u8;
        frame.polygons.push(polygon);
    }

    fn render(frame: &Frame, disp3dcnt: u32) -> Renderer {
        let mut renderer = Renderer::default();
        let mut registers = Registers::default();
        registers.write(DISP3DCNT, disp3dcnt, !0);
        registers.write(CLEAR_DEPTH, 0x7FFF, 0xFFFF);
        renderer.start_frame(registers);
        render_with(renderer, frame, &Memory::default())
    }

    fn render_with(mut renderer: Renderer, frame: &Frame, memory: &Memory) -> Renderer {
        let mut line = Line3d::default();
        for y in 0..HEIGHT {
            renderer.render_line(y, frame, memory, &mut line);
        }
        renderer
    }

    fn row(renderer: &Renderer, y: usize) -> Vec<usize> {
        (0..WIDTH)
            .filter(|&x| renderer.pixel(x, y).alpha != 0)
            .collect()
    }

    fn quad(left: u16, top: u16, right: u16, bottom: u16) -> [[u16; 2]; 4] {
        [[left, top], [left, bottom], [right, bottom], [right, top]]
    }

    #[test]
    fn a_rectangle_covers_its_left_and_top_edges_but_not_its_right_and_bottom_like_melonds() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(10, 20, 30, 40), [63, 0, 0], SOLID);
        let renderer = render(&frame, 0);
        assert_eq!(row(&renderer, 19), Vec::<usize>::new());
        assert_eq!(row(&renderer, 20), (10..30).collect::<Vec<_>>());
        assert_eq!(row(&renderer, 39), (10..30).collect::<Vec<_>>());
        assert_eq!(row(&renderer, 40), Vec::<usize>::new());
        assert_eq!(
            renderer.pixel(10, 20),
            Pixel {
                r: 63,
                g: 0,
                b: 0,
                alpha: 31
            }
        );
    }

    fn shadow_frame() -> Frame {
        let shadow = SOLID | SHADOW << MODE_SHIFT;
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 200, 100), [5, 5, 5], SOLID);
        add(
            &mut frame,
            &quad(0, 10, 250, 81),
            [60, 0, 0],
            shadow | 1 << ID_SHIFT | DEPTH_EQUAL,
        );
        add(&mut frame, &quad(50, 20, 150, 41), [0, 0, 0], shadow);
        add(
            &mut frame,
            &quad(205, 24, 245, 144),
            [0, 30, 0],
            SOLID | 2 << ID_SHIFT,
        );
        frame
    }

    #[test]
    fn bands_draw_the_frame_the_line_by_line_renderer_draws_with_the_stencil_carried_over() {
        let frame = shadow_frame();
        for disp3dcnt in [0, EDGE_MARKING, ANTIALIASING | EDGE_MARKING] {
            let mut registers = Registers::default();
            registers.write(DISP3DCNT, disp3dcnt, !0);
            registers.write(CLEAR_DEPTH, 0x7FFF, 0xFFFF);
            let mut sequential = Renderer::default();
            let mut line = Line3d::default();
            let expected: Vec<Vec<Line3d>> = (0..2)
                .map(|_| {
                    sequential.start_frame(registers);
                    (0..HEIGHT)
                        .map(|y| {
                            sequential.render_line(y, &frame, &Memory::default(), &mut line);
                            line.clone()
                        })
                        .collect()
                })
                .collect();
            assert_ne!(expected[0][12], expected[1][12]);
            assert_ne!(expected[0][60], expected[0][5]);
            if disp3dcnt & EDGE_MARKING != 0 {
                assert_ne!(expected[0][24], expected[0][25]);
                assert_ne!(expected[0][143], expected[0][142]);
            }
            for rows in [1, 5, 24, 64, HEIGHT] {
                let bands: Vec<_> = (0..HEIGHT)
                    .step_by(rows)
                    .map(|start| start..(start + rows).min(HEIGHT))
                    .collect();
                let mut renderers: Vec<Renderer> =
                    bands.iter().map(|_| Renderer::default()).collect();
                let mut carried = Stencil::default();
                for (number, wanted) in expected.iter().enumerate() {
                    let mut got = vec![Line3d::default(); HEIGHT];
                    let mut end = None;
                    for (index, band) in bands.iter().enumerate().rev() {
                        let renderer = &mut renderers[(index + number) % bands.len()];
                        renderer.start_frame(registers);
                        let last = renderer.render_band(
                            band.clone(),
                            &frame,
                            &Memory::default(),
                            &carried,
                            |y, line| got[y].clone_from(line),
                        );
                        end = end.or(last);
                    }
                    assert!(
                        got == *wanted,
                        "frame {number} in bands of {rows} rows, disp3dcnt {disp3dcnt:#x}"
                    );
                    carried = end.expect("the mask rows end in one band");
                }
            }
        }
    }

    #[test]
    fn the_end_stencil_of_a_skipped_frame_is_the_stencil_a_drawn_frame_leaves() {
        let shadow = shadow_frame();
        let mut solid = Frame::default();
        add(&mut solid, &quad(0, 0, 200, 100), [5, 5, 5], SOLID);
        for disp3dcnt in [0, EDGE_MARKING, ANTIALIASING | EDGE_MARKING] {
            let mut registers = Registers::default();
            registers.write(DISP3DCNT, disp3dcnt, !0);
            registers.write(CLEAR_DEPTH, 0x7FFF, 0xFFFF);
            let mut drawn = Renderer::default();
            let mut skipped = Renderer::default();
            let mut carried = Stencil::default();
            for (number, frame) in [&shadow, &shadow, &solid, &shadow].into_iter().enumerate() {
                drawn.start_frame(registers);
                let left =
                    drawn.render_band(0..HEIGHT, frame, &Memory::default(), &carried, |_, _| {});
                skipped.start_frame(registers);
                let end = skipped.end_stencil(frame, &Memory::default(), &carried);
                assert_eq!(end, left, "frame {number}, disp3dcnt {disp3dcnt:#x}");
                assert_eq!(left.is_some(), number != 2);
                if let Some(left) = left {
                    carried = left;
                }
            }
        }
    }

    #[test]
    fn a_full_screen_quad_covers_every_pixel() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 256, 192), [1, 1, 1], SOLID);
        let renderer = render(&frame, 0);
        assert_eq!(row(&renderer, 0).len(), WIDTH);
        assert_eq!(row(&renderer, 191).len(), WIDTH);
    }

    #[test]
    fn neighbouring_triangles_share_their_diagonal_without_gaps_or_overlap() {
        let mut frame = Frame::default();
        add(&mut frame, &[[0, 0], [0, 50], [70, 50]], [63, 0, 0], SOLID);
        add(&mut frame, &[[0, 0], [70, 50], [70, 0]], [0, 63, 0], SOLID);
        let renderer = render(&frame, 0);
        for y in 0..50 {
            assert_eq!(row(&renderer, y), (0..70).collect::<Vec<_>>(), "row {y}");
        }
    }

    #[test]
    fn x_major_left_edges_are_not_filled_when_opaque_like_melonds() {
        let mut frame = Frame::default();
        add(
            &mut frame,
            &[[0, 0], [100, 10], [120, 0]],
            [63, 63, 63],
            SOLID,
        );
        let opaque = render(&frame, 0);
        let filled = render(&frame, ANTIALIASING);
        let (opaque_row, filled_row) = (row(&opaque, 5), row(&filled, 5));
        assert!(opaque_row.first() > filled_row.first());
    }

    #[test]
    fn wireframe_polygons_draw_only_their_edges_and_the_top_and_bottom_rows() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(10, 10, 20, 20), [63, 63, 63], 0);
        let renderer = render(&frame, 0);
        assert_eq!(row(&renderer, 10), (10..20).collect::<Vec<_>>());
        assert_eq!(row(&renderer, 15), [10, 19]);
        assert_eq!(row(&renderer, 19), (10..20).collect::<Vec<_>>());
    }

    #[test]
    fn a_flat_polygon_draws_one_row_like_melonds() {
        let mut frame = Frame::default();
        add(
            &mut frame,
            &[[5, 30], [40, 30], [20, 30]],
            [63, 63, 63],
            SOLID,
        );
        let renderer = render(&frame, 0);
        assert_eq!(row(&renderer, 30), (5..40).collect::<Vec<_>>());
        assert_eq!(row(&renderer, 31), Vec::<usize>::new());
    }

    #[test]
    fn degenerate_polygons_are_skipped() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 10, 10), [63, 63, 63], SOLID);
        frame.polygons[0].degenerate = true;
        assert!(row(&render(&frame, 0), 5).is_empty());
    }

    #[test]
    fn back_facing_polygons_walk_their_vertices_the_other_way() {
        let mut frame = Frame::default();
        add(
            &mut frame,
            &[[10, 10], [10, 20], [20, 20], [20, 10]],
            [63, 63, 63],
            SOLID,
        );
        let front = row(&render(&frame, 0), 15);
        let mut frame = Frame::default();
        add(
            &mut frame,
            &[[20, 10], [20, 20], [10, 20], [10, 10]],
            [63, 63, 63],
            SOLID,
        );
        frame.polygons[0].front = false;
        assert_eq!(row(&render(&frame, 0), 15), front);
    }

    #[test]
    fn the_line_hands_the_2d_engine_six_bit_colour_and_alpha() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 4, 4), [63, 31, 1], SOLID);
        let mut renderer = Renderer::default();
        renderer.start_frame(far_registers());
        let mut line = Line3d::default();
        renderer.render_line(2, &frame, &Memory::default(), &mut line);
        assert_eq!(line.color[1], color::from_channels(63, 31, 1));
        assert_eq!(line.alpha[1], 31);
        assert_eq!(line.alpha[4], 0);
    }

    fn shade_across(frame: &mut Frame, left: [u8; 3], right: [u8; 3], w: [i32; 2]) {
        let polygon = &mut frame.polygons[0];
        for index in 0..usize::from(polygon.count) {
            let vertex = &mut frame.vertices[usize::from(polygon.vertices[index])];
            let side = usize::from(vertex.screen[0] != 0);
            vertex.color = [left, right][side];
            polygon.w[index] = w[side];
            polygon.depth[index] = [0, 0x10_0000][side];
        }
    }

    #[test]
    fn colours_interpolate_linearly_across_equal_w_like_melonds() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 64, 8), [0; 3], SOLID);
        shade_across(&mut frame, [63, 0, 0], [0, 0, 63], [0x1000, 0x1000]);
        let renderer = render(&frame, 0);
        assert_eq!(renderer.pixel(0, 4).r, 63);
        assert_eq!(renderer.pixel(32, 4).r, 31);
        assert_eq!(renderer.pixel(32, 4).b, 31);
        assert_eq!(renderer.pixel(63, 4).b, 62);
    }

    #[test]
    fn a_far_side_takes_less_of_the_span_with_perspective_like_melonds() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 64, 8), [0; 3], SOLID);
        shade_across(&mut frame, [63, 0, 0], [0, 0, 0], [0x1000, 0x4000]);
        let renderer = render(&frame, 0);
        assert!(renderer.pixel(32, 4).r > 40);
    }

    #[test]
    fn z_buffer_depth_interpolates_linearly_across_the_span_like_melonds() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 64, 8), [0; 3], SOLID);
        shade_across(&mut frame, [0; 3], [0; 3], [0x1000, 0x4000]);
        let renderer = render(&frame, 0);
        assert_eq!(renderer.depth(0, 4), 0);
        assert_eq!(renderer.depth(32, 4), 0x8_0000);
    }

    #[test]
    fn colours_interpolate_down_the_edges_too() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 8, 64), [0; 3], SOLID);
        for vertex in &mut frame.vertices {
            vertex.color = if vertex.screen[1] == 0 {
                [63, 0, 0]
            } else {
                [0; 3]
            };
        }
        let renderer = render(&frame, 0);
        assert_eq!(renderer.pixel(4, 0).r, 63);
        assert_eq!(renderer.pixel(4, 32).r, 31);
    }

    #[test]
    fn the_rear_plane_fills_the_line_with_the_clear_colour_and_alpha_per_gbatek() {
        let mut registers = Registers::default();
        registers.write(CLEAR_COLOR, 12 << 16 | 1 << 10 | 31, !0);
        registers.write(CLEAR_DEPTH, 0x1234, 0xFFFF);
        let mut renderer = Renderer::default();
        renderer.start_frame(registers);
        let mut line = Line3d::default();
        renderer.render_line(7, &Frame::default(), &Memory::default(), &mut line);
        let rear = Pixel {
            r: 63,
            g: 0,
            b: 3,
            alpha: 12,
        };
        assert_eq!(renderer.pixel(0, 7), rear);
        assert_eq!(renderer.pixel(255, 7), rear);
        assert_eq!(renderer.depth(9, 7), 0x1234 * 0x200);
        assert_eq!(line.alpha[3], 12);
    }

    #[test]
    fn clear_depth_7fff_expands_to_the_far_plane_per_gbatek() {
        let mut registers = Registers::default();
        registers.write(CLEAR_DEPTH, 0x7FFF, 0xFFFF);
        assert_eq!(registers.rear_depth(), 0xFF_FFFF);
        registers.write(CLEAR_DEPTH, 0x7FFE, 0xFFFF);
        assert_eq!(registers.rear_depth(), 0xFF_FC00);
    }

    #[test]
    fn a_clear_rear_plane_with_alpha_zero_shows_nothing() {
        let mut renderer = Renderer::default();
        let mut line = Line3d::default();
        renderer.render_line(0, &Frame::default(), &Memory::default(), &mut line);
        assert!(line.alpha.iter().all(|&alpha| alpha == 0));
    }

    fn flat_depth(frame: &mut Frame, polygon: usize, depth: i32) {
        frame.polygons[polygon].depth = [depth; 10];
    }

    fn far_registers() -> Registers {
        let mut registers = Registers::default();
        registers.write(CLEAR_DEPTH, 0x7FFF, 0xFFFF);
        registers
    }

    #[test]
    fn a_nearer_polygon_drawn_first_hides_a_farther_one_per_gbatek() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 16, 16), [63, 0, 0], SOLID);
        add(&mut frame, &quad(0, 0, 16, 16), [0, 63, 0], SOLID);
        flat_depth(&mut frame, 0, 0x1000);
        flat_depth(&mut frame, 1, 0x2000);
        let renderer = render(&frame, 0);
        assert_eq!(renderer.pixel(8, 8).r, 63);
        assert_eq!(renderer.depth(8, 8), 0x1000);
    }

    #[test]
    fn equal_depth_fails_the_less_test_per_gbatek() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 16, 16), [63, 0, 0], SOLID);
        add(&mut frame, &quad(0, 0, 16, 16), [0, 63, 0], SOLID);
        flat_depth(&mut frame, 0, 0x1000);
        flat_depth(&mut frame, 1, 0x1000);
        assert_eq!(render(&frame, 0).pixel(8, 8).r, 63);
    }

    #[test]
    fn the_equal_test_passes_within_0x200_in_z_buffer_mode_per_gbatek() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 16, 16), [63, 0, 0], SOLID);
        add(
            &mut frame,
            &quad(0, 0, 8, 16),
            [0, 63, 0],
            SOLID | DEPTH_EQUAL,
        );
        add(
            &mut frame,
            &quad(8, 0, 16, 16),
            [0, 0, 63],
            SOLID | DEPTH_EQUAL,
        );
        flat_depth(&mut frame, 0, 0x1000);
        flat_depth(&mut frame, 1, 0x1200);
        flat_depth(&mut frame, 2, 0x1201);
        let renderer = render(&frame, 0);
        assert_eq!(renderer.pixel(4, 8).g, 63);
        assert_eq!(renderer.pixel(12, 8).r, 63);
    }

    #[test]
    fn the_equal_test_passes_within_0xff_in_w_buffer_mode_like_melonds() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 16, 16), [63, 0, 0], SOLID);
        add(
            &mut frame,
            &quad(0, 0, 8, 16),
            [0, 63, 0],
            SOLID | DEPTH_EQUAL,
        );
        add(
            &mut frame,
            &quad(8, 0, 16, 16),
            [0, 0, 63],
            SOLID | DEPTH_EQUAL,
        );
        for polygon in &mut frame.polygons {
            polygon.w_buffer = true;
        }
        flat_depth(&mut frame, 0, 0x1000);
        flat_depth(&mut frame, 1, 0x10FF);
        flat_depth(&mut frame, 2, 0x1100);
        let renderer = render(&frame, 0);
        assert_eq!(renderer.pixel(4, 8).g, 63);
        assert_eq!(renderer.pixel(12, 8).r, 63);
    }

    #[test]
    fn a_front_face_wins_ties_over_an_opaque_back_face_like_melonds() {
        let mut frame = Frame::default();
        add(
            &mut frame,
            &[[16, 0], [16, 16], [0, 16], [0, 0]],
            [63, 0, 0],
            SOLID,
        );
        add(&mut frame, &quad(0, 0, 16, 16), [0, 63, 0], SOLID);
        frame.polygons[0].front = false;
        flat_depth(&mut frame, 0, 0x1000);
        flat_depth(&mut frame, 1, 0x1000);
        let renderer = render(&frame, 0);
        assert_eq!(renderer.pixel(8, 8).g, 63);
        assert!(!renderer.attribute(8, 8).back());
    }

    #[test]
    fn opaque_pixels_write_their_id_fog_and_edges_to_the_attribute_buffer() {
        let mut frame = Frame::default();
        add(
            &mut frame,
            &quad(10, 10, 20, 20),
            [63, 0, 0],
            SOLID | FOG | 9 << ID_SHIFT,
        );
        let renderer = render(&frame, 0);
        let inside = renderer.attribute(15, 15);
        assert_eq!(inside.opaque_id(), 9);
        assert_eq!(inside.translucent_id(), None);
        assert!(inside.fog());
        assert_eq!(inside.edges(), 0);
        assert_eq!(renderer.attribute(10, 15).edges(), Attribute::LEFT);
        assert_eq!(renderer.attribute(19, 15).edges(), Attribute::RIGHT);
        assert_eq!(
            renderer.attribute(10, 10).edges(),
            Attribute::TOP | Attribute::LEFT
        );
        assert_eq!(renderer.attribute(15, 19).edges(), Attribute::BOTTOM);
    }

    #[test]
    fn the_rear_plane_sets_the_clear_polygon_id_and_fog_per_gbatek() {
        let mut registers = far_registers();
        registers.write(CLEAR_COLOR, 5 << ID_SHIFT | FOG, !0);
        let mut renderer = Renderer::default();
        renderer.start_frame(registers);
        renderer.render_line(
            0,
            &Frame::default(),
            &Memory::default(),
            &mut Line3d::default(),
        );
        let rear = renderer.attribute(0, 0);
        assert_eq!(rear.opaque_id(), 5);
        assert!(rear.fog());
    }

    fn translucent(alpha: u32) -> u32 {
        alpha << ALPHA_SHIFT
    }

    fn layered(top: u32, disp3dcnt: u32) -> Renderer {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 16, 16), [63, 0, 0], SOLID | FOG);
        add(&mut frame, &quad(0, 0, 16, 16), [0, 63, 0], top);
        flat_depth(&mut frame, 0, 0x2000);
        flat_depth(&mut frame, 1, 0x1000);
        render(&frame, disp3dcnt)
    }

    #[test]
    fn translucent_pixels_blend_over_the_colour_buffer_per_gbatek() {
        let renderer = layered(translucent(15), ALPHA_BLENDING);
        assert_eq!(
            renderer.pixel(8, 8),
            Pixel {
                r: 31,
                g: 31,
                b: 0,
                alpha: 31
            }
        );
    }

    #[test]
    fn without_blending_translucent_pixels_replace_the_colour_but_keep_the_larger_alpha_per_gbatek()
    {
        let renderer = layered(translucent(15), 0);
        assert_eq!(
            renderer.pixel(8, 8),
            Pixel {
                r: 0,
                g: 63,
                b: 0,
                alpha: 31
            }
        );
    }

    #[test]
    fn translucent_pixels_over_a_clear_rear_plane_keep_their_own_colour_per_gbatek() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 16, 16), [0, 63, 0], translucent(9));
        let pixel = render(&frame, ALPHA_BLENDING).pixel(8, 8);
        assert_eq!(
            pixel,
            Pixel {
                r: 0,
                g: 63,
                b: 0,
                alpha: 9
            }
        );
    }

    #[test]
    fn translucent_pixels_keep_the_old_depth_unless_bit_11_is_set_per_gbatek() {
        assert_eq!(layered(translucent(15), 0).depth(8, 8), 0x2000);
        let updated = layered(translucent(15) | TRANSLUCENT_DEPTH, 0);
        assert_eq!(updated.depth(8, 8), 0x1000);
    }

    #[test]
    fn translucent_pixels_keep_the_opaque_id_and_and_the_fog_flag_per_gbatek() {
        let renderer = layered(translucent(15) | 7 << ID_SHIFT, 0);
        let attribute = renderer.attribute(8, 8);
        assert_eq!(attribute.opaque_id(), 0);
        assert_eq!(attribute.translucent_id(), Some(7));
        assert!(!attribute.fog());
        let fogged = layered(translucent(15) | FOG, 0).attribute(8, 8);
        assert!(fogged.fog());
    }

    #[test]
    fn a_translucent_polygon_id_blends_only_once_per_pixel_like_melonds() {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 16, 16), [0, 0, 0], SOLID);
        add(
            &mut frame,
            &quad(0, 0, 16, 16),
            [63, 0, 0],
            translucent(15) | 3 << ID_SHIFT,
        );
        add(
            &mut frame,
            &quad(0, 0, 16, 16),
            [63, 0, 0],
            translucent(15) | 3 << ID_SHIFT,
        );
        add(
            &mut frame,
            &quad(0, 0, 8, 16),
            [63, 0, 0],
            translucent(15) | 4 << ID_SHIFT,
        );
        flat_depth(&mut frame, 0, 0x2000);
        let renderer = render(&frame, ALPHA_BLENDING);
        assert_eq!(renderer.pixel(12, 8).r, 31);
        assert_eq!(renderer.pixel(4, 8).r, 47);
    }

    #[test]
    fn an_opaque_pixel_clears_the_translucent_id() {
        let mut frame = Frame::default();
        add(
            &mut frame,
            &quad(0, 0, 16, 16),
            [0, 0, 0],
            translucent(15) | 3 << ID_SHIFT,
        );
        add(&mut frame, &quad(0, 0, 16, 16), [63, 0, 0], SOLID);
        let attribute = render(&frame, 0).attribute(8, 8);
        assert_eq!(attribute.translucent_id(), None);
    }

    #[test]
    fn the_alpha_test_hides_polygons_at_or_below_the_reference_per_gbatek() {
        let draw = |alpha: u32, disp3dcnt: u32| {
            let mut frame = Frame::default();
            add(
                &mut frame,
                &quad(0, 0, 16, 16),
                [63, 0, 0],
                translucent(alpha),
            );
            let mut registers = far_registers();
            registers.write(ALPHA_TEST_REF, 15, !0);
            registers.write(DISP3DCNT, disp3dcnt, !0);
            let mut renderer = Renderer::default();
            renderer.start_frame(registers);
            renderer.render_line(8, &frame, &Memory::default(), &mut Line3d::default());
            renderer.pixel(8, 8).alpha
        };
        assert_eq!(draw(15, ALPHA_TEST), 0);
        assert_eq!(draw(16, ALPHA_TEST), 16);
        assert_eq!(draw(15, 0), 15);
    }

    const PALETTE_256: u32 = 4;
    const A5I3: u32 = 6;
    const DECAL: u32 = 1 << MODE_SHIFT;
    const SHADOW_MODE: u32 = 3 << MODE_SHIFT;

    fn textured(texture: u32, attributes: u32) -> Frame {
        let mut frame = Frame::default();
        add(&mut frame, &quad(0, 0, 8, 8), [63, 63, 63], attributes);
        frame.polygons[0].texture = texture;
        for vertex in &mut frame.vertices {
            vertex.texcoord = vertex.screen.map(|axis| (axis << 4) as i16);
        }
        frame
    }

    fn render_textured(frame: &Frame, memory: &Memory, disp3dcnt: u32) -> Renderer {
        let mut registers = far_registers();
        registers.write(DISP3DCNT, disp3dcnt, !0);
        let mut renderer = Renderer::default();
        renderer.start_frame(registers);
        render_with(renderer, frame, memory)
    }

    fn stripes() -> Memory {
        let mut memory = Memory::default();
        for (index, texel) in memory.texture[..64].iter_mut().enumerate() {
            *texel = (index % 8) as u8;
        }
        for index in 0..8u16 {
            memory.set_color(u32::from(index) * 2, index * 4);
        }
        memory
    }

    #[test]
    fn texel_colours_expand_to_six_bits_per_gbatek() {
        assert_eq!([expand(0), expand(1), expand(31)], [0, 3, 63]);
    }

    #[test]
    fn texture_coordinates_follow_the_span_one_texel_per_pixel() {
        let frame = textured(param(PALETTE_256, 0, 0), SOLID);
        let renderer = render_textured(&frame, &stripes(), TEXTURES);
        let reds: Vec<u8> = (0..8).map(|x| renderer.pixel(x, 3).r).collect();
        assert_eq!(reds, [0, 9, 17, 25, 33, 41, 49, 57]);
    }

    #[test]
    fn texturing_off_in_disp3dcnt_shows_the_vertex_colour_per_gbatek() {
        let frame = textured(param(PALETTE_256, 0, 0), SOLID);
        let renderer = render_textured(&frame, &stripes(), 0);
        assert_eq!(renderer.pixel(3, 3).r, 63);
    }

    #[test]
    fn transparent_texels_draw_nothing_per_gbatek() {
        let frame = textured(param(PALETTE_256, 0, 0) | 1 << 29, SOLID);
        let renderer = render_textured(&frame, &stripes(), TEXTURES);
        assert_eq!(renderer.pixel(0, 3).alpha, 0);
        assert_eq!(renderer.pixel(1, 3).alpha, 31);
    }

    #[test]
    fn texel_alpha_makes_a_solid_polygon_translucent_per_gbatek() {
        let mut memory = Memory::default();
        memory.texture[..64].fill(10 << 3);
        memory.set_color(0, 0x7FFF);
        let frame = textured(param(A5I3, 0, 0), SOLID | 2 << ID_SHIFT);
        let renderer = render_textured(&frame, &memory, TEXTURES);
        assert_eq!(renderer.pixel(4, 4).alpha, 10);
        assert_eq!(renderer.attribute(4, 4).translucent_id(), Some(2));
    }

    #[test]
    fn decal_keeps_the_polygon_alpha_per_gbatek() {
        let frame = textured(param(PALETTE_256, 0, 0), DECAL | translucent(20));
        let renderer = render_textured(&frame, &stripes(), TEXTURES);
        assert_eq!(renderer.pixel(5, 5).alpha, 20);
        assert_eq!(renderer.pixel(5, 5).r, 41);
    }

    fn shadow_scene(shadow_id: u32) -> Frame {
        let mut frame = Frame::default();
        add(
            &mut frame,
            &quad(0, 0, 16, 16),
            [63, 0, 0],
            SOLID | 1 << ID_SHIFT,
        );
        add(
            &mut frame,
            &[[8, 0], [8, 16], [0, 16], [0, 0]],
            [0; 3],
            SHADOW_MODE | translucent(15),
        );
        add(
            &mut frame,
            &quad(0, 0, 16, 16),
            [0, 63, 0],
            SHADOW_MODE | translucent(15) | shadow_id << ID_SHIFT,
        );
        frame.polygons[1].front = false;
        flat_depth(&mut frame, 0, 0x1000);
        flat_depth(&mut frame, 1, 0x2000);
        frame
    }

    #[test]
    fn shadows_draw_only_where_the_mask_is_behind_the_scene_like_melonds() {
        let renderer = render(&shadow_scene(2), 0);
        assert_eq!(renderer.pixel(4, 8).g, 63);
        assert_eq!(renderer.pixel(12, 8).r, 63);
        assert_eq!(renderer.depth(4, 8), 0x1000);
    }

    #[test]
    fn the_mask_itself_draws_no_colour_per_gbatek() {
        let mut frame = shadow_scene(2);
        frame.polygons.pop();
        let renderer = render(&frame, 0);
        assert_eq!(renderer.pixel(4, 8).r, 63);
    }

    #[test]
    fn a_shadow_skips_pixels_with_its_own_polygon_id_per_gbatek() {
        let renderer = render(&shadow_scene(1), 0);
        assert_eq!(renderer.pixel(4, 8).r, 63);
    }

    #[test]
    fn a_new_mask_after_a_shadow_clears_the_stencil_like_melonds() {
        let mut frame = shadow_scene(2);
        add(
            &mut frame,
            &[[16, 0], [16, 16], [8, 16], [8, 0]],
            [0; 3],
            SHADOW_MODE | translucent(15),
        );
        add(
            &mut frame,
            &quad(0, 0, 16, 16),
            [0, 0, 63],
            SHADOW_MODE | translucent(15) | 3 << ID_SHIFT,
        );
        frame.polygons[3].front = false;
        flat_depth(&mut frame, 3, 0x2000);
        let renderer = render(&frame, 0);
        assert_eq!(renderer.pixel(4, 8).g, 63);
        assert_eq!(renderer.pixel(12, 8).b, 63);
    }

    #[test]
    fn a_shadow_skips_the_layer_under_an_edge_unless_its_mask_marked_it_like_melonds() {
        let mut frame = Frame::default();
        slanted(&mut frame, [0, 63, 0], 0x1000);
        add(
            &mut frame,
            &[[0, 0], [0, 40], [60, 40], [60, 0]],
            [0, 0, 63],
            SOLID | 1 << ID_SHIFT,
        );
        flat_depth(&mut frame, 1, 0x6000);
        add(
            &mut frame,
            &[[0, 0], [0, 40], [60, 40], [60, 0]],
            [0; 3],
            SHADOW_MODE | translucent(15),
        );
        frame.polygons[2].front = false;
        flat_depth(&mut frame, 2, 0x3000);
        add(
            &mut frame,
            &[[0, 0], [0, 40], [60, 40], [60, 0]],
            [63, 63, 0],
            SHADOW_MODE | translucent(15) | 3 << ID_SHIFT,
        );
        flat_depth(&mut frame, 3, 0x2000);
        let renderer = render_over_red(&frame, ANTIALIASING);
        let x = first_left_edge(&renderer, 1);
        assert_eq!(renderer.under(x, 1).2.translucent_id(), None);
        assert_eq!(rgba(renderer.pixel(x, 1)), [0, 39, 23, 31]);
    }

    fn geometry_quad(attributes: u32, corners: [[i16; 2]; 4]) -> Renderer {
        use super::super::geometry::Geometry;
        use super::super::geometry::command::*;
        let mut geometry = Geometry::default();
        let mut send =
            |command: u8, param: u32| geometry.write(usize::from(command) << 2, param, !0);
        send(VIEWPORT, 0xBFFF_0000);
        send(POLYGON_ATTR, attributes);
        send(BEGIN_VTXS, 1);
        send(COLOR, 0x7FFF);
        for [x, y] in corners {
            send(VTX_16, u32::from(x as u16) | u32::from(y as u16) << 16);
            send(VTX_16, 0);
        }
        send(SWAP_BUFFERS, 0);
        geometry.step(u32::MAX);
        geometry.vblank();
        geometry.with_frame(|frame| render(frame, 0))
    }

    #[test]
    fn a_front_facing_quad_from_the_geometry_engine_covers_its_screen_rectangle() {
        let corners = [
            [-0x800, -0x800],
            [0x800, -0x800],
            [0x800, 0x800],
            [-0x800, 0x800],
        ];
        let renderer = geometry_quad(0x001F_0080, corners);
        assert_eq!(row(&renderer, 47), Vec::<usize>::new());
        assert_eq!(row(&renderer, 48), (64..192).collect::<Vec<_>>());
        assert_eq!(row(&renderer, 96), (64..192).collect::<Vec<_>>());
        assert_eq!(row(&renderer, 143), (64..192).collect::<Vec<_>>());
        assert_eq!(row(&renderer, 144), Vec::<usize>::new());
        assert_eq!(renderer.pixel(100, 100).r, 63);
        assert!(!renderer.attribute(100, 100).back());
    }

    #[test]
    fn a_back_facing_quad_from_the_geometry_engine_covers_the_same_pixels() {
        let corners = [
            [-0x800, -0x800],
            [-0x800, 0x800],
            [0x800, 0x800],
            [0x800, -0x800],
        ];
        let renderer = geometry_quad(0x001F_0040, corners);
        assert_eq!(row(&renderer, 48), (64..192).collect::<Vec<_>>());
        assert_eq!(row(&renderer, 143), (64..192).collect::<Vec<_>>());
        assert!(renderer.attribute(100, 100).back());
    }

    #[test]
    fn disp3dcnt_keeps_its_writable_bits() {
        let mut registers = Registers::default();
        registers.write(DISP3DCNT, 0xFFFF, 0xFFFF);
        assert_eq!(registers.read(DISP3DCNT), 0x4FFF);
    }

    fn render_over_red(frame: &Frame, disp3dcnt: u32) -> Renderer {
        let mut renderer = Renderer::default();
        let mut registers = Registers::default();
        registers.write(DISP3DCNT, disp3dcnt, !0);
        registers.write(CLEAR_COLOR, 31 | 31 << CLEAR_ALPHA_SHIFT, !0);
        registers.write(CLEAR_DEPTH, 0x7FFF, 0xFFFF);
        renderer.start_frame(registers);
        render_with(renderer, frame, &Memory::default())
    }

    fn slanted(frame: &mut Frame, color: [u8; 3], depth: i32) {
        add(frame, &[[0, 0], [8, 32], [40, 32], [40, 0]], color, SOLID);
        frame.polygons.last_mut().unwrap().depth = [depth; MAX_POLYGON_VERTICES];
    }

    fn first_left_edge(renderer: &Renderer, y: usize) -> usize {
        (0..WIDTH)
            .find(|&x| renderer.attribute(x, y).edges() & Attribute::LEFT != 0)
            .unwrap()
    }

    fn rgba(pixel: Pixel) -> [u8; 4] {
        [pixel.r, pixel.g, pixel.b, pixel.alpha]
    }

    #[test]
    fn antialiasing_blends_edge_pixels_with_the_pixel_under_them_like_melonds() {
        let mut frame = Frame::default();
        slanted(&mut frame, [0, 63, 0], 0x1000);
        let smooth = render_over_red(&frame, ANTIALIASING);
        let x = first_left_edge(&smooth, 1);
        assert_eq!(smooth.attribute(x, 1).coverage(), 19);
        assert_eq!(rgba(smooth.pixel(x, 1)), [23, 39, 0, 31]);
        let sharp = render_over_red(&frame, 0);
        assert_eq!(rgba(sharp.pixel(x, 1)), [0, 63, 0, 31]);
        assert_eq!(rgba(smooth.pixel(x + 4, 1)), [0, 63, 0, 31]);
    }

    #[test]
    fn a_polygon_behind_an_edge_pixel_draws_into_the_layer_under_it_like_melonds() {
        let mut frame = Frame::default();
        slanted(&mut frame, [0, 63, 0], 0x1000);
        add(
            &mut frame,
            &[[0, 0], [0, 40], [60, 40], [60, 0]],
            [0, 0, 63],
            SOLID,
        );
        frame.polygons.last_mut().unwrap().depth = [0x2000; MAX_POLYGON_VERTICES];
        let renderer = render_over_red(&frame, ANTIALIASING);
        let x = first_left_edge(&renderer, 1);
        assert_eq!(rgba(renderer.pixel(x, 1)), [0, 39, 23, 31]);
    }

    #[test]
    fn antialiasing_keeps_an_edge_pixel_over_a_transparent_one_in_its_color_like_melonds() {
        let mut frame = Frame::default();
        slanted(&mut frame, [0, 63, 0], 0x1000);
        let renderer = render(&frame, ANTIALIASING);
        let x = first_left_edge(&renderer, 1);
        assert_eq!(rgba(renderer.pixel(x, 1)), [0, 63, 0, 19]);
    }

    fn render_registers(frame: &Frame, registers: Registers) -> Renderer {
        let mut renderer = Renderer::default();
        renderer.start_frame(registers);
        render_with(renderer, frame, &Memory::default())
    }

    #[test]
    fn edge_marking_paints_polygon_borders_with_the_edge_table_like_melonds() {
        let mut frame = Frame::default();
        add(
            &mut frame,
            &[[10, 10], [10, 30], [40, 30], [40, 10]],
            [0, 63, 0],
            SOLID | 9 << ID_SHIFT,
        );
        let mut registers = Registers::default();
        registers.write(DISP3DCNT, EDGE_MARKING, !0);
        registers.write(CLEAR_DEPTH, 0x7FFF, !0);
        registers.write(EDGE_COLOR, (31 << 10) << 16, !0);
        let renderer = render_registers(&frame, registers);
        assert_eq!(rgba(renderer.pixel(10, 20)), [0, 0, 63, 31]);
        assert_eq!(rgba(renderer.pixel(20, 10)), [0, 0, 63, 31]);
        assert_eq!(renderer.attribute(10, 20).coverage(), EDGE_COVERAGE);
        assert_eq!(rgba(renderer.pixel(20, 20)), [0, 63, 0, 31]);
    }

    #[test]
    fn edge_marking_skips_edges_against_the_same_polygon_id_like_melonds() {
        let mut frame = Frame::default();
        add(
            &mut frame,
            &[[10, 10], [10, 30], [40, 30], [40, 10]],
            [0, 63, 0],
            SOLID,
        );
        let mut registers = Registers::default();
        registers.write(DISP3DCNT, EDGE_MARKING, !0);
        registers.write(CLEAR_DEPTH, 0x7FFF, !0);
        registers.write(EDGE_COLOR, 31 << 10, !0);
        let renderer = render_registers(&frame, registers);
        assert_eq!(rgba(renderer.pixel(10, 20)), [0, 63, 0, 31]);
    }

    #[test]
    fn dense_fog_replaces_the_colour_and_alpha_like_melonds() {
        let mut frame = Frame::default();
        add(
            &mut frame,
            &[[10, 10], [10, 30], [40, 30], [40, 10]],
            [0, 63, 0],
            SOLID | FOG,
        );
        add(
            &mut frame,
            &[[50, 10], [50, 30], [80, 30], [80, 10]],
            [0, 63, 0],
            SOLID,
        );
        let mut registers = Registers::default();
        registers.write(DISP3DCNT, FOG_ENABLE, !0);
        registers.write(CLEAR_DEPTH, 0x7FFF, !0);
        registers.write(FOG_COLOR, 31 | 7 << CLEAR_ALPHA_SHIFT, !0);
        for word in (0..32).step_by(4) {
            registers.write(FOG_TABLE + word, 0x7F7F_7F7F, !0);
        }
        let renderer = render_registers(&frame, registers);
        assert_eq!(rgba(renderer.pixel(20, 20)), [63, 0, 0, 7]);
        assert_eq!(rgba(renderer.pixel(60, 20)), [0, 63, 0, 31]);
        registers.write(DISP3DCNT, FOG_ENABLE | FOG_ALPHA_ONLY, !0);
        let renderer = render_registers(&frame, registers);
        assert_eq!(rgba(renderer.pixel(20, 20)), [0, 63, 0, 7]);
    }

    #[test]
    fn fog_density_interpolates_the_table_by_depth_like_melonds() {
        let mut registers = Registers::default();
        registers.write(FOG_TABLE, 0x0000_4020, !0);
        registers.write(FOG_OFFSET, 0x100, !0);
        registers.write(DISP3DCNT, 2 << FOG_SHIFT_SHIFT, !0);
        assert_eq!(registers.fog_density(0), 32);
        assert_eq!(registers.fog_density(0x100 * DEPTH_STEP), 32);
        assert_eq!(registers.fog_density(0x100 * DEPTH_STEP + 0x30000), 48);
        assert_eq!(registers.fog_density(0x7FFF * DEPTH_STEP), 0);
    }

    #[test]
    fn table_registers_take_byte_and_halfword_writes() {
        let mut registers = Registers::default();
        registers.write(TOON_TABLE + 4, 0xFFFF_1234, 0x0000_FFFF);
        registers.write(FOG_TABLE + 4, 0xFF80_7F01, !0);
        registers.write(EDGE_COLOR + 12, 0x8001_7FFF, !0);
        assert_eq!(registers.toon[2..4], [0x1234, 0]);
        assert_eq!(registers.fog[4..8], [1, 0x7F, 0, 0x7F]);
        assert_eq!(registers.edge[6..8], [0x7FFF, 1]);
    }

    #[test]
    fn toon_polygons_take_their_colour_from_the_toon_table() {
        let mut frame = Frame::default();
        add(
            &mut frame,
            &[[10, 10], [10, 30], [40, 30], [40, 10]],
            [9, 0, 0],
            SOLID | 2 << MODE_SHIFT,
        );
        let mut registers = Registers::default();
        registers.write(CLEAR_DEPTH, 0x7FFF, !0);
        registers.write(TOON_TABLE + 8, 31 << 5, !0);
        let renderer = render_registers(&frame, registers);
        assert_eq!(rgba(renderer.pixel(20, 20)), [0, 63, 0, 31]);
    }

    #[test]
    fn a_line_drawn_alone_or_backwards_matches_the_same_line_of_a_whole_frame() {
        let mut frame = Frame::default();
        let pentagon = [[120, 10], [40, 60], [60, 150], [180, 170], [220, 50]];
        add(&mut frame, &pentagon, [63, 0, 0], SOLID);
        add(
            &mut frame,
            &[[100, 0], [0, 140], [255, 191]],
            [0, 63, 0],
            SOLID,
        );
        let whole = render(&frame, 0);
        let mut registers = Registers::default();
        registers.write(CLEAR_DEPTH, 0x7FFF, 0xFFFF);
        let memory = Memory::default();
        let mut backwards = Renderer::default();
        backwards.start_frame(registers);
        backwards.begin(&frame, &memory);
        for y in (0..HEIGHT).rev() {
            let mut alone = Renderer::default();
            alone.start_frame(registers);
            alone.begin(&frame, &memory);
            alone.rasterize(y, &frame, &memory);
            backwards.rasterize(y, &frame, &memory);
            for x in 0..WIDTH {
                let expected = (whole.pixel(x, y), whole.attribute(x, y));
                assert_eq!((alone.pixel(x, y), alone.attribute(x, y)), expected);
                assert_eq!((backwards.pixel(x, y), backwards.attribute(x, y)), expected);
            }
        }
        assert!(row(&whole, 100).len() > 150);
    }
}
