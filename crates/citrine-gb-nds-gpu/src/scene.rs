use citrine_gb::nds::geometry::frame::{Frame, MAX_POLYGON_VERTICES, Polygon, SUBPIXEL_BITS};
use citrine_gb::nds::render::texture::{PALETTE_SIZE, TEXTURE_SIZE, TextureMemory};
use citrine_gb::nds::render::{Attribute, Pixel};
use citrine_gb::nds::video::{HEIGHT, WIDTH};

use crate::atlas::{self, Atlas};

pub const DATA_WIDTH: u32 = 1024;
pub const TILE: usize = 8;
pub const TILES_X: usize = WIDTH / TILE;
pub const TILES_Y: usize = HEIGHT / TILE;

const HEADER_WORDS: usize = 12;
const VERTEX_WORDS: usize = 8;
const PLACE_WORD: usize = 6;
const MASK_WORDS: usize = HEIGHT / 32;
const SHADOW: u32 = 3;
const DECAL: u32 = 1;
const FORMAT_SHIFT: u32 = 26;
const COLOR_0_CLEAR: u32 = 1 << 29;
const MODE_SHIFT: u32 = 4;
const ID_SHIFT: u32 = 24;
const ID_MASK: u32 = 0x3F;
const BOUNDS_MARGIN: i32 = 2;
const ENTRY_WORDS: usize = 4;
const ENTRY_TRANSLUCENT: u32 = 1 << 31;
const ENTRY_MASK: u32 = 1 << 30;
const DEPTH_EQUAL: u32 = 1 << 14;
const ALPHA_SHIFT: u32 = 16;
const ALPHA_MASK: u32 = 31;
const CLOCKWISE: u32 = 1 << 20;
const NO_AREA: u32 = 1 << 21;
const SLAB: u32 = 1 << 31;
const EXACT: u32 = 1 << 30;
const ROW_WORDS: usize = DATA_WIDTH as usize * 4;
const HEADS: usize = TILES_X * TILES_Y * 2;
const VRAM_AT: usize = HEADS;
const PALETTE_AT: usize = VRAM_AT + TEXTURE_SIZE as usize / 4;
const RECORDS_AT: usize = PALETTE_AT + PALETTE_SIZE as usize / 4;
const _: () = assert!(HEADS <= ROW_WORDS && RECORDS_AT.is_multiple_of(4));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offsets {
    pub bins: u32,
    pub vram: u32,
    pub palette: u32,
    pub tail: u32,
    pub atlas: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plain,
    Shadow,
    Mask,
    FarMask { clears: u8 },
}

#[derive(Debug, Default)]
pub struct Scene {
    pub words: Vec<u32>,
    pub hardware: Vec<[u32; 2]>,
    pub slabs: u32,
    pub smooth: bool,
    pub upscaled: bool,
    pub mipmaps: bool,
    pub atlased: bool,
    pub atlas_limit: u32,
    pub atlas: Atlas,
    places: Vec<(usize, usize)>,
    backs: Vec<[u32; 2]>,
    slabs_front: Vec<[u32; 2]>,
    slabs_back: Vec<[u32; 2]>,
    cut_slabs_front: Vec<[u32; 2]>,
    cut_slabs_back: Vec<[u32; 2]>,
    cut_fronts: Vec<[u32; 2]>,
    cut_backs: Vec<[u32; 2]>,
    bins: Vec<Vec<([u32; ENTRY_WORDS], Kind)>>,
    tails: Vec<u32>,
    filled: bool,
    textures_changed: bool,
}

struct Outline {
    points: [[u32; 2]; MAX_POLYGON_VERTICES],
    count: usize,
    bounds: [u32; 2],
    top: usize,
    bottom: usize,
    winding: u32,
    columns: std::ops::Range<usize>,
    rows: std::ops::Range<usize>,
}

impl Outline {
    fn new(frame: &Frame, polygon: &Polygon) -> Self {
        let count = usize::from(polygon.count);
        let mut points = [[0u32; 2]; MAX_POLYGON_VERTICES];
        let (mut low, mut high) = ([u32::MAX; 2], [0u32; 2]);
        let (mut near, mut far) = ([i32::MAX; 2], [i32::MIN; 2]);
        let (mut top, mut bottom) = (0, 0);
        let (mut top_y, mut bottom_y) = (u32::MAX, 0);
        for (index, point) in points[..count].iter_mut().enumerate() {
            let vertex = frame.vertex(polygon, index);
            *point = [0, 1].map(|axis| {
                (u32::from(vertex.screen[axis]) << SUBPIXEL_BITS) + u32::from(vertex.subpixel[axis])
            });
            let y = (u32::from(vertex.screen[1]) << SUBPIXEL_BITS) | u32::from(vertex.subpixel[1]);
            if index == 0 || y < top_y {
                (top, top_y) = (index, y);
            }
            if index == 0 || y >= bottom_y {
                (bottom, bottom_y) = (index, y);
            }
            for axis in 0..2 {
                low[axis] = low[axis].min(point[axis]);
                high[axis] = high[axis].max(point[axis]);
                near[axis] = near[axis].min(i32::from(vertex.screen[axis]));
                far[axis] = far[axis].max(i32::from(vertex.screen[axis]));
            }
        }
        let area: i64 = (0..count)
            .map(|index| {
                let [ax, ay] = points[index].map(i64::from);
                let [bx, by] = points[(index + 1) % count].map(i64::from);
                ax * by - bx * ay
            })
            .sum();
        let winding = match area {
            0 => NO_AREA,
            area if area < 0 => CLOCKWISE,
            _ => 0,
        };
        let corner = |[x, y]: [u32; 2]| (x & 0xFFFF) | (y & 0xFFFF) << 16;
        let tile = |value: i32, limit: usize| (value.max(0) as usize / TILE).min(limit);
        let span = |axis: usize, tiles: usize| {
            tile(near[axis] - BOUNDS_MARGIN, tiles)..tile(far[axis] + BOUNDS_MARGIN, tiles - 1) + 1
        };
        Self {
            points,
            count,
            bounds: [corner(low), corner(high)],
            top,
            bottom,
            winding,
            columns: span(0, TILES_X),
            rows: span(1, TILES_Y),
        }
    }

    fn chain(&self, y: u32, forward: bool) -> [usize; 2] {
        let step = |index: usize| {
            if forward {
                (index + 1) % self.count
            } else {
                (index + self.count - 1) % self.count
            }
        };
        let (mut start, mut stop) = (self.top, step(self.top));
        for _ in 0..self.count {
            if y < self.points[stop][1] || start == self.bottom {
                break;
            }
            (start, stop) = (stop, step(stop));
        }
        [start, stop]
    }

    fn slabs(&self, front: bool, mut push: impl FnMut(u32)) {
        if self.count < 3 || self.winding == NO_AREA {
            return;
        }
        let mut order = [0usize; MAX_POLYGON_VERTICES];
        let order = &mut order[..self.count];
        for (index, slot) in order.iter_mut().enumerate() {
            *slot = index;
        }
        order.sort_unstable_by_key(|&index| self.points[index][1]);
        for pair in order.windows(2) {
            let [upper, lower] = [pair[0], pair[1]];
            let y = self.points[upper][1];
            if y == self.points[lower][1] {
                continue;
            }
            let [left, right] = [self.chain(y, front), self.chain(y, !front)];
            let ends = [left[0], left[1], right[0], right[1], upper, lower];
            let packed = ends.iter().enumerate().fold(SLAB, |word, (slot, &index)| {
                word | (index as u32) << (slot * 4)
            });
            push(packed);
        }
    }

    fn convex(&self) -> bool {
        let at = |index: usize| self.points[index % self.count].map(i64::from);
        let mut turns = (0..self.count).map(|index| {
            let [a, b, c] = [at(index), at(index + 1), at(index + 2)];
            (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
        });
        turns.clone().all(|turn| turn >= 0) || turns.all(|turn| turn <= 0)
    }
}

impl Scene {
    pub fn pack<M: TextureMemory>(&mut self, frame: &Frame, memory: &M) -> Offsets {
        self.words.truncate(RECORDS_AT);
        self.words.resize(RECORDS_AT, 0);
        self.words[..HEADS].fill(0);
        self.textures_changed = self.fill_textures(memory) || !self.filled;
        self.filled = true;
        self.bins.resize_with(TILES_X * TILES_Y, Vec::new);
        self.bins.iter_mut().for_each(Vec::clear);
        self.hardware.clear();
        self.backs.clear();
        self.slabs_front.clear();
        self.slabs_back.clear();
        self.cut_slabs_front.clear();
        self.cut_slabs_back.clear();
        self.cut_fronts.clear();
        self.cut_backs.clear();
        self.atlas.clear();
        self.places.clear();
        let mut depth_mode = None;
        let mut masked = [0u32; MASK_WORDS];
        let mut masks = false;
        for polygon in &frame.polygons {
            if !drawable(polygon) {
                continue;
            }
            let mask = is_mask(polygon);
            let mut clears = [0; MASK_WORDS];
            if mask || masks {
                let rows = row_bits(frame, polygon);
                for ((clears, masked), rows) in clears.iter_mut().zip(&mut masked).zip(rows) {
                    if mask {
                        *clears = rows & !*masked;
                        *masked |= rows;
                    } else {
                        *masked &= !rows;
                    }
                }
                masks |= mask;
            }
            let outline = Outline::new(frame, polygon);
            self.words.resize(self.words.len().next_multiple_of(4), 0);
            let record = self.words.len() as u32;
            self.push_record(frame, polygon, &outline, clears);
            if self.atlased
                && let Some(index) = self.atlas.add(polygon.texture, u32::from(polygon.palette))
            {
                self.places
                    .push((record as usize + HEADER_WORDS + PLACE_WORD, index));
            }
            let flags = if polygon.translucent {
                ENTRY_TRANSLUCENT
            } else {
                0
            } | if mask { ENTRY_MASK } else { 0 };
            let drawn = hardware(polygon)
                && outline.convex()
                && *depth_mode.get_or_insert(polygon.w_buffer) == polygon.w_buffer;
            if drawn {
                let cut = self.smooth && cutout(polygon);
                let (slabs, edges) = match (polygon.front, cut) {
                    (true, false) => (&mut self.slabs_front, &mut self.hardware),
                    (false, false) => (&mut self.slabs_back, &mut self.backs),
                    (true, true) => (&mut self.cut_slabs_front, &mut self.cut_fronts),
                    (false, true) => (&mut self.cut_slabs_back, &mut self.cut_backs),
                };
                edges.push([record, 0]);
                if self.upscaled && !polygon.degenerate {
                    let exact = if cut { EXACT } else { 0 };
                    outline.slabs(polygon.front, |slab| slabs.push([record, slab | exact]));
                }
            }
            let [low, high] = outline.bounds;
            let entry = [record | flags, low, high, u32::from(!drawn)];
            let plain = if mode(polygon) == SHADOW {
                Kind::Shadow
            } else {
                Kind::Plain
            };
            let columns = if mask {
                0..TILES_X
            } else {
                outline.columns.clone()
            };
            for row in outline.rows.clone() {
                let far = Kind::FarMask {
                    clears: (clears[row * TILE / 32] >> (row * TILE % 32)) as u8,
                };
                for column in columns.clone() {
                    let kind = if !mask {
                        plain
                    } else if outline.columns.contains(&column) {
                        Kind::Mask
                    } else {
                        far
                    };
                    self.bins[row * TILES_X + column].push((entry, kind));
                }
            }
        }
        if masks {
            self.bins.iter_mut().for_each(prune);
        }
        self.words
            .resize(self.words.len().next_multiple_of(ENTRY_WORDS), 0);
        let bins = self.words.len() as u32;
        let mut start = 0;
        for (tile, list) in self.bins.iter().enumerate() {
            self.words[tile * 2] = start;
            self.words[tile * 2 + 1] = list.len() as u32;
            start += list.len() as u32;
        }
        self.words
            .reserve(start as usize * ENTRY_WORDS * 2 + HEADS + ROW_WORDS);
        for (entry, _) in self.bins.iter().flatten() {
            self.words.extend_from_slice(entry);
        }
        self.slabs = (self.slabs_front.len()
            + self.slabs_back.len()
            + self.cut_slabs_front.len()
            + self.cut_slabs_back.len()) as u32;
        self.slabs_front.append(&mut self.slabs_back);
        self.slabs_front.append(&mut self.cut_slabs_front);
        self.slabs_front.append(&mut self.cut_slabs_back);
        self.slabs_front.append(&mut self.hardware);
        self.slabs_front.append(&mut self.backs);
        self.slabs_front.append(&mut self.cut_fronts);
        self.slabs_front.append(&mut self.cut_backs);
        std::mem::swap(&mut self.hardware, &mut self.slabs_front);
        self.tails.clear();
        for list in &self.bins {
            let before = self.words.len();
            for (entry, _) in list.iter().filter(|(entry, _)| entry[3] != 0) {
                self.words.extend_from_slice(entry);
            }
            let count = ((self.words.len() - before) / ENTRY_WORDS) as u32;
            self.tails.extend([start, count]);
            start += count;
        }
        let tail = self.words.len() as u32;
        self.words.extend_from_slice(&self.tails);
        self.atlas.place(self.atlas_limit);
        for &(at, index) in &self.places {
            self.words[at] = self.atlas.word(index);
        }
        self.words
            .resize(self.words.len().next_multiple_of(atlas::ENTRY_WORDS), 0);
        let atlas = self.words.len() as u32;
        for entry in self.atlas.entries() {
            self.words.extend_from_slice(&entry);
        }
        self.words
            .resize(self.words.len().div_ceil(ROW_WORDS) * ROW_WORDS, 0);
        Offsets {
            bins,
            vram: VRAM_AT as u32,
            palette: PALETTE_AT as u32,
            tail,
            atlas,
        }
    }

    pub fn rows(&self) -> u32 {
        (self.words.len() / ROW_WORDS) as u32
    }

    pub fn unchanged_rows(&self) -> std::ops::Range<u32> {
        if self.textures_changed {
            0..0
        } else {
            1..(RECORDS_AT / ROW_WORDS) as u32
        }
    }

    pub fn atlas_changed(&self) -> bool {
        self.textures_changed || self.atlas.moved()
    }

    fn fill_textures<M: TextureMemory>(&mut self, memory: &M) -> bool {
        let (vram, palette) = self.words[VRAM_AT..RECORDS_AT].split_at_mut(PALETTE_AT - VRAM_AT);
        match memory.texture_bytes() {
            Some((image, colors))
                if image.len() == TEXTURE_SIZE as usize
                    && colors.len() == PALETTE_SIZE as usize =>
            {
                refill(vram, image) | refill(palette, colors)
            }
            _ => {
                refill_with(vram, |address| memory.texture(address))
                    | refill_with(palette, |address| memory.texture_palette(address))
            }
        }
    }

    fn push_record(
        &mut self,
        frame: &Frame,
        polygon: &Polygon,
        outline: &Outline,
        clears: [u32; MASK_WORDS],
    ) {
        let flags = outline.winding
            | u32::from(polygon.palette)
            | u32::from(polygon.front) << 16
            | u32::from(polygon.w_buffer) << 17
            | u32::from(polygon.degenerate) << 18
            | u32::from(polygon.translucent) << 19;
        let shape = outline.count as u32
            | u32::from(polygon.top) << 8
            | u32::from(polygon.bottom) << 16
            | (outline.top as u32) << 24
            | (outline.bottom as u32) << 28;
        self.words
            .extend_from_slice(&[polygon.attributes, polygon.texture, flags, shape]);
        self.words.extend_from_slice(&clears);
        self.words.extend_from_slice(&outline.bounds);
        debug_assert_eq!(6 + MASK_WORDS, HEADER_WORDS);
        for (index, point) in outline.points[..outline.count].iter().enumerate() {
            let vertex = frame.vertex(polygon, index);
            let [r, g, b] = vertex.color.map(u32::from);
            let [s, t] = vertex.texcoord.map(|value| u32::from(value as u16));
            let vertex_words: [u32; VERTEX_WORDS] = [
                u32::from(vertex.screen[0]) | u32::from(vertex.screen[1]) << 16,
                point[0] | point[1] << 16,
                polygon.depth[index] as u32,
                polygon.w[index] as u32,
                r | g << 8 | b << 16,
                s | t << 16,
                0,
                0,
            ];
            self.words.extend_from_slice(&vertex_words);
        }
    }
}

pub fn pack_pixel(pixel: Pixel) -> u32 {
    u32::from(pixel.r)
        | u32::from(pixel.g) << 8
        | u32::from(pixel.b) << 16
        | u32::from(pixel.alpha) << 24
}

pub fn unpack_pixel(value: u32) -> Pixel {
    Pixel {
        r: value as u8,
        g: (value >> 8) as u8,
        b: (value >> 16) as u8,
        alpha: (value >> 24) as u8,
    }
}

const NO_ID: u32 = 64;
const FOG_BIT: u32 = 1 << 13;
const BACK_BIT: u32 = 1 << 14;
const EDGES_SHIFT: u32 = 15;

pub fn pack_attribute(attribute: Attribute) -> u32 {
    u32::from(attribute.opaque_id())
        | attribute.translucent_id().map_or(NO_ID, u32::from) << 6
        | if attribute.fog() { FOG_BIT } else { 0 }
        | if attribute.back() { BACK_BIT } else { 0 }
        | u32::from(attribute.edges()) << EDGES_SHIFT
}

pub fn unpack_attribute(value: u32) -> Attribute {
    let translucent = (value >> 6) & 0x7F;
    Attribute::new(
        (value & ID_MASK) as u8,
        (translucent != NO_ID).then_some(translucent as u8),
        value & FOG_BIT != 0,
        value & BACK_BIT != 0,
        (value >> EDGES_SHIFT) as u8,
        0,
    )
}

fn refill(words: &mut [u32], bytes: &[u8]) -> bool {
    let mut changed = false;
    for (word, chunk) in words.iter_mut().zip(bytes.as_chunks::<4>().0) {
        let value = u32::from_le_bytes(*chunk);
        changed |= *word != value;
        *word = value;
    }
    changed
}

fn refill_with(words: &mut [u32], byte: impl Fn(u32) -> u8) -> bool {
    let mut changed = false;
    for (index, word) in words.iter_mut().enumerate() {
        let address = index as u32 * 4;
        let value = u32::from_le_bytes([
            byte(address),
            byte(address + 1),
            byte(address + 2),
            byte(address + 3),
        ]);
        changed |= *word != value;
        *word = value;
    }
    changed
}

fn drawable(polygon: &Polygon) -> bool {
    !polygon.degenerate && polygon.count >= 3
}

fn hardware(polygon: &Polygon) -> bool {
    !polygon.translucent
        && mode(polygon) != SHADOW
        && polygon.attributes & DEPTH_EQUAL == 0
        && (polygon.attributes >> ALPHA_SHIFT) & ALPHA_MASK == ALPHA_MASK
}

fn cutout(polygon: &Polygon) -> bool {
    let holed = match (polygon.texture >> FORMAT_SHIFT) & 7 {
        5 | 7 => true,
        2..=4 => polygon.texture & COLOR_0_CLEAR != 0,
        _ => false,
    };
    holed && mode(polygon) != DECAL
}

fn mode(polygon: &Polygon) -> u32 {
    (polygon.attributes >> MODE_SHIFT) & 3
}

fn is_mask(polygon: &Polygon) -> bool {
    mode(polygon) == SHADOW && (polygon.attributes >> ID_SHIFT) & ID_MASK == 0
}

fn prune(list: &mut Vec<([u32; ENTRY_WORDS], Kind)>) {
    let first_mask = list.iter().position(|(_, kind)| *kind == Kind::Mask);
    let last_shadow = list.iter().rposition(|(_, kind)| *kind == Kind::Shadow);
    let mut keep: Vec<bool> = (0..list.len())
        .map(|index| match list[index].1 {
            Kind::FarMask { clears } => {
                clears != 0
                    && first_mask.is_some_and(|first| first < index)
                    && last_shadow.is_some_and(|last| last > index)
            }
            _ => true,
        })
        .collect();
    let translucent = |index: usize| list[index].0[0] & ENTRY_TRANSLUCENT != 0;
    if let Some(first) = (0..list.len()).find(|&index| translucent(index))
        && !keep[first]
    {
        let next = (first + 1..list.len())
            .find(|&index| keep[index] && translucent(index))
            .unwrap_or(list.len());
        keep[first] = (first + 1..next).any(|index| keep[index]);
    }
    let mut kept = keep.into_iter();
    list.retain(|_| kept.next().unwrap_or(true));
}

fn row_bits(frame: &Frame, polygon: &Polygon) -> [u32; MASK_WORDS] {
    let y = |index: u8| usize::from(frame.vertex(polygon, usize::from(index)).screen[1]);
    let (top, bottom) = (y(polygon.top), y(polygon.bottom));
    let end = if top == bottom { top + 1 } else { bottom }.min(HEIGHT);
    let mut bits = [0; MASK_WORDS];
    for (word, bits) in bits.iter_mut().enumerate() {
        let low = top.max(word * 32);
        let high = end.min(word * 32 + 32);
        if low < high {
            *bits = (u32::MAX >> (32 - (high - low))) << (low - word * 32);
        }
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb::nds::geometry::frame::Vertex;

    struct Blank;

    impl TextureMemory for Blank {
        fn texture(&self, _: u32) -> u8 {
            0
        }

        fn texture_palette(&self, _: u32) -> u8 {
            0
        }
    }

    fn add(frame: &mut Frame, points: &[[u16; 2]], attributes: u32) {
        let first = frame.vertices.len() as u16;
        let mut polygon = Polygon {
            count: points.len() as u8,
            attributes,
            ..Polygon::default()
        };
        for (index, &screen) in points.iter().enumerate() {
            polygon.vertices[index] = first + index as u16;
            frame.vertices.push(Vertex {
                screen,
                ..Vertex::default()
            });
        }
        polygon.top = (0..points.len()).min_by_key(|&i| points[i][1]).unwrap() as u8;
        polygon.bottom = (0..points.len()).max_by_key(|&i| points[i][1]).unwrap() as u8;
        frame.polygons.push(polygon);
    }

    fn mask_clears(frame: &Frame) -> Vec<[u32; MASK_WORDS]> {
        let mut scene = Scene::default();
        scene.pack(frame, &Blank);
        let mut at = RECORDS_AT;
        frame
            .polygons
            .iter()
            .map(|polygon| {
                let clears = scene.words[at + 4..at + 4 + MASK_WORDS].try_into().unwrap();
                at += HEADER_WORDS + usize::from(polygon.count) * VERTEX_WORDS;
                clears
            })
            .collect()
    }

    fn tile_counts(scene: &Scene) -> Vec<u32> {
        (0..TILES_X * TILES_Y)
            .map(|tile| scene.words[tile * 2 + 1])
            .collect()
    }

    const MASK: u32 = SHADOW << MODE_SHIFT;
    const SHADOW_ID_1: u32 = SHADOW << MODE_SHIFT | 1 << ID_SHIFT;

    #[test]
    fn a_polygon_lands_only_in_the_tiles_its_bounds_touch() {
        let mut frame = Frame::default();
        add(&mut frame, &[[19, 19], [19, 27], [27, 27]], 0);
        let mut scene = Scene::default();
        scene.pack(&frame, &Blank);
        let counts = tile_counts(&scene);
        let touched: Vec<usize> = (0..counts.len())
            .filter(|&tile| counts[tile] == 1)
            .collect();
        assert_eq!(touched, [66, 67, 98, 99]);
        assert_eq!(counts.iter().sum::<u32>(), 4);
    }

    #[test]
    fn tile_lists_keep_the_frame_order() {
        let mut frame = Frame::default();
        add(&mut frame, &[[0, 0], [0, 4], [4, 4]], 0);
        add(&mut frame, &[[1, 1], [1, 5], [5, 5]], 0);
        let mut scene = Scene::default();
        let offsets = scene.pack(&frame, &Blank);
        let record = |entry: usize| {
            scene.words[offsets.bins as usize + entry * ENTRY_WORDS]
                & !(ENTRY_TRANSLUCENT | ENTRY_MASK)
        };
        assert!(record(0) < record(1));
    }

    #[test]
    fn a_tile_entry_carries_its_polygon_bounds_and_kind() {
        let mut frame = Frame::default();
        add(&mut frame, &[[0, 0], [0, 7], [7, 7]], MASK);
        add(&mut frame, &[[1, 2], [1, 5], [6, 5]], 0);
        frame.polygons[1].translucent = true;
        let mut scene = Scene::default();
        let offsets = scene.pack(&frame, &Blank);
        let entry = |index: usize| {
            let at = offsets.bins as usize + index * ENTRY_WORDS;
            [scene.words[at], scene.words[at + 1], scene.words[at + 2]]
        };
        let [mask, _, _] = entry(0);
        assert_eq!(mask & (ENTRY_TRANSLUCENT | ENTRY_MASK), ENTRY_MASK);
        let [translucent, low, high] = entry(1);
        assert_eq!(
            translucent & (ENTRY_TRANSLUCENT | ENTRY_MASK),
            ENTRY_TRANSLUCENT
        );
        assert_eq!(low, 16 | (2 * 16) << 16);
        assert_eq!(high, (6 * 16) | ((5 * 16) << 16));
        let record = (translucent & !ENTRY_TRANSLUCENT) as usize;
        assert_eq!(scene.words[record + 10..record + 12], [low, high]);
    }

    #[test]
    fn a_lone_mask_lands_only_in_the_tiles_its_bounds_touch() {
        let mut frame = Frame::default();
        add(&mut frame, &[[100, 0], [100, 7], [107, 7]], MASK);
        let mut scene = Scene::default();
        scene.pack(&frame, &Blank);
        let counts = tile_counts(&scene);
        assert_eq!(counts[12] + counts[13] + counts[44] + counts[45], 4);
        assert_eq!(counts.iter().sum::<u32>(), 4);
    }

    #[test]
    fn a_far_mask_stays_where_it_clears_a_stencil_a_later_shadow_reads() {
        let mut frame = Frame::default();
        add(&mut frame, &[[0, 0], [0, 7], [7, 7]], MASK);
        add(&mut frame, &[[50, 0], [50, 7], [57, 7]], 0);
        add(&mut frame, &[[100, 0], [100, 7], [107, 7]], MASK);
        add(&mut frame, &[[0, 0], [0, 7], [7, 7]], SHADOW_ID_1);
        let mut scene = Scene::default();
        scene.pack(&frame, &Blank);
        let counts = tile_counts(&scene);
        assert_eq!(counts[0], 3);
        assert_eq!(counts[6], 1);
        assert_eq!(counts[12], 1);
        assert_eq!(counts[20], 0);
    }

    #[test]
    fn a_far_mask_that_starts_the_translucent_entries_stays_before_an_opaque_one() {
        let mut frame = Frame::default();
        add(&mut frame, &[[100, 0], [100, 7], [107, 7]], MASK);
        frame.polygons[0].translucent = true;
        add(&mut frame, &[[0, 0], [0, 7], [7, 7]], 0);
        let mut scene = Scene::default();
        scene.pack(&frame, &Blank);
        assert_eq!(tile_counts(&scene)[0], 2);
    }

    #[test]
    fn a_far_mask_after_the_opaque_entries_drops_out() {
        let mut frame = Frame::default();
        add(&mut frame, &[[0, 0], [0, 7], [7, 7]], 0);
        add(&mut frame, &[[100, 0], [100, 7], [107, 7]], MASK);
        frame.polygons[1].translucent = true;
        let mut scene = Scene::default();
        scene.pack(&frame, &Blank);
        assert_eq!(tile_counts(&scene)[0], 1);
    }

    #[test]
    fn a_mask_clears_the_stencil_only_after_a_polygon_that_is_not_a_mask() {
        let mut frame = Frame::default();
        add(&mut frame, &[[0, 0], [0, 10], [10, 10]], MASK);
        add(&mut frame, &[[0, 0], [0, 10], [10, 10]], MASK);
        add(&mut frame, &[[0, 0], [0, 10], [10, 10]], SHADOW_ID_1);
        add(&mut frame, &[[0, 0], [0, 10], [10, 10]], MASK);
        let clears = mask_clears(&frame);
        assert_eq!(clears[0][0], 0x3FF);
        assert_eq!(clears[1][0], 0);
        assert_eq!(clears[3][0], 0x3FF);
    }

    #[test]
    fn polygons_off_a_row_do_not_break_a_run_of_masks() {
        let mut frame = Frame::default();
        add(&mut frame, &[[0, 0], [0, 10], [10, 10]], MASK);
        add(&mut frame, &[[0, 50], [0, 60], [10, 60]], 0);
        add(&mut frame, &[[0, 0], [0, 10], [10, 10]], MASK);
        assert_eq!(mask_clears(&frame)[2][0], 0);
    }

    #[test]
    fn the_shader_reads_the_data_texture_at_its_width() {
        let line = format!("const DATA_WIDTH: u32 = {DATA_WIDTH}u;");
        assert!(include_str!("raster.wgsl").contains(&line));
    }

    #[test]
    fn attributes_survive_packing() {
        let attribute = Attribute::new(
            63,
            Some(5),
            true,
            true,
            Attribute::LEFT | Attribute::BOTTOM,
            0,
        );
        assert_eq!(unpack_attribute(pack_attribute(attribute)), attribute);
        let rear = Attribute::default();
        assert_eq!(unpack_attribute(pack_attribute(rear)), rear);
    }

    struct Pattern {
        seed: u8,
        slices: Option<(Vec<u8>, Vec<u8>)>,
    }

    impl Pattern {
        fn new(seed: u8, slices: bool) -> Self {
            let slices = slices.then(|| {
                let image = (0..TEXTURE_SIZE)
                    .map(|address| byte(seed, address))
                    .collect();
                let palette = (0..PALETTE_SIZE)
                    .map(|address| byte(seed, address ^ 0x55))
                    .collect();
                (image, palette)
            });
            Self { seed, slices }
        }
    }

    fn byte(seed: u8, address: u32) -> u8 {
        (address.wrapping_mul(7) >> 3) as u8 ^ seed
    }

    impl TextureMemory for Pattern {
        fn texture(&self, address: u32) -> u8 {
            byte(self.seed, address % TEXTURE_SIZE)
        }

        fn texture_palette(&self, address: u32) -> u8 {
            byte(self.seed, (address % PALETTE_SIZE) ^ 0x55)
        }

        fn texture_bytes(&self) -> Option<(&[u8], &[u8])> {
            self.slices
                .as_ref()
                .map(|(image, palette)| (image.as_slice(), palette.as_slice()))
        }
    }

    fn texture_words(scene: &Scene, offsets: Offsets) -> (Vec<u8>, Vec<u8>) {
        let bytes = |from: u32, size: u32| -> Vec<u8> {
            scene.words[from as usize..(from + size / 4) as usize]
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect()
        };
        (
            bytes(offsets.vram, TEXTURE_SIZE),
            bytes(offsets.palette, PALETTE_SIZE),
        )
    }

    #[test]
    fn texture_memory_lands_in_the_data_the_same_through_bytes_or_slices() {
        let mut frame = Frame::default();
        add(&mut frame, &[[10, 10], [60, 10], [60, 60]], 0x1F << 16);
        for slices in [false, true] {
            let memory = Pattern::new(9, slices);
            let mut scene = Scene::default();
            let offsets = scene.pack(&frame, &memory);
            let (image, palette) = texture_words(&scene, offsets);
            assert!(
                (0..TEXTURE_SIZE).all(|address| image[address as usize] == memory.texture(address))
            );
            assert!(
                (0..PALETTE_SIZE)
                    .all(|address| palette[address as usize] == memory.texture_palette(address))
            );
        }
    }

    #[test]
    fn texture_rows_upload_again_only_after_the_memory_changes() {
        let mut frame = Frame::default();
        add(&mut frame, &[[10, 10], [60, 10], [60, 60]], 0x1F << 16);
        for slices in [false, true] {
            let mut scene = Scene::default();
            scene.pack(&frame, &Pattern::new(0, slices));
            assert!(
                scene.unchanged_rows().is_empty(),
                "a new scene uploads everything"
            );
            scene.pack(&frame, &Pattern::new(0, slices));
            let unchanged = scene.unchanged_rows();
            assert_eq!(unchanged, 1..(RECORDS_AT / ROW_WORDS) as u32);
            assert!(unchanged.start as usize * ROW_WORDS >= HEADS);
            assert!(unchanged.end as usize * ROW_WORDS <= RECORDS_AT);
            scene.pack(&frame, &Pattern::new(1, slices));
            assert!(scene.unchanged_rows().is_empty());
            let fresh = {
                let mut fresh = Scene::default();
                fresh.pack(&frame, &Pattern::new(1, slices));
                fresh.words
            };
            assert_eq!(scene.words, fresh);
        }
    }
}
