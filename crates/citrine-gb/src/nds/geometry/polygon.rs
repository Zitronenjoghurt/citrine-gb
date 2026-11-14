use super::clip::{Points, clip};
use super::frame::{
    Frame, MAX_POLYGON_VERTICES, MAX_POLYGONS, MAX_VERTICES, Polygon, SUBPIXEL_BITS, Vertex,
};

const COLOR_FRACTION: u32 = 12;
const COLOR_LOW: i32 = (1 << COLOR_FRACTION) - 1;
const RENDER_BACK: u32 = 1 << 6;
const RENDER_FRONT: u32 = 1 << 7;
const RENDER_FAR: u32 = 1 << 12;
const RENDER_DOT: u32 = 1 << 13;
const DOT_DEPTH_MASK: u32 = 0x7FFF;
const DOT_DEPTH_SHIFT: u32 = 9;
const DOT_DEPTH_LOW: i32 = 0x1FF;
const ALPHA_SHIFT: u32 = 16;
const ALPHA_MASK: u32 = 0x1F;
const FORMAT_SHIFT: u32 = 26;
const FORMAT_A3I5: u32 = 1;
const FORMAT_A5I3: u32 = 6;
const PALETTE_MASK: u32 = 0x1FFF;
const SCREEN_HEIGHT: i32 = 192;
const SCREEN_X_MASK: u32 = 0x1FF;
const SCREEN_Y_MASK: u32 = 0xFF;
const W_MASK: i32 = 0x00FF_FFFF;
const DIVIDER_LIMIT: u32 = 0xFFFF;
const DEPTH_MAX: i32 = 0x00FF_FFFF;
const DEPTH_NO_W: i32 = 0x007F_FE00;
const SORT_MANUAL: u32 = 1 << 0;
const W_BUFFER: u32 = 1 << 1;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Point {
    pub position: [i32; 4],
    pub color: [i32; 3],
    pub texcoord: [i16; 2],
    pub clipped: bool,
}

impl Point {
    pub fn new(position: [i32; 4], color: [u8; 3], texcoord: [i16; 2]) -> Self {
        Self {
            position,
            color: color.map(|channel| (i32::from(channel) << COLOR_FRACTION) | COLOR_LOW),
            texcoord,
            clipped: false,
        }
    }

    fn from_vertex(vertex: &Vertex) -> Self {
        let mut point = Self::new(
            vertex.position,
            vertex.color.map(|c| c >> 1),
            vertex.texcoord,
        );
        point.clipped = vertex.clipped;
        point
    }

    fn color(&self) -> [u8; 3] {
        self.color
            .map(|channel| match (channel >> COLOR_FRACTION) as u8 {
                0 => 0,
                level => level * 2 + 1,
            })
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Primitive {
    #[default]
    Triangles,
    Quads,
    TriangleStrip,
    QuadStrip,
}

impl Primitive {
    fn from_param(param: u32) -> Self {
        match param & 3 {
            0 => Self::Triangles,
            1 => Self::Quads,
            2 => Self::TriangleStrip,
            _ => Self::QuadStrip,
        }
    }

    fn strip(self) -> bool {
        matches!(self, Self::TriangleStrip | Self::QuadStrip)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Viewport {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

impl Viewport {
    pub fn from_param(param: u32) -> Self {
        let field = |shift: u32| (param >> shift) & 0xFF;
        let top = 191u32.wrapping_sub(field(24)) & 0xFF;
        let bottom = 191u32.wrapping_sub(field(8)) & 0xFF;
        Self {
            left: field(0),
            top,
            width: field(16).wrapping_sub(field(0)).wrapping_add(1) & 0x1FF,
            height: bottom.wrapping_sub(top).wrapping_add(1) & 0xFF,
        }
    }

    pub fn screen(&self, position: [i32; 4]) -> [u16; 2] {
        self.project(position).0
    }

    pub fn subpixel(&self, position: [i32; 4]) -> [u8; 2] {
        self.project(position).1
    }

    fn project(&self, position: [i32; 4]) -> ([u16; 2], [u8; 2]) {
        let w = position[3] as u32;
        if w == 0 {
            return ([0, 0], [0, 0]);
        }
        let mut x = (position[0] as u32).wrapping_add(w);
        let mut y = (position[1] as u32).wrapping_neg().wrapping_add(w);
        let mut divisor = w;
        if w > DIVIDER_LIMIT {
            x >>= 1;
            y >>= 1;
            divisor >>= 1;
        }
        divisor <<= 1;
        let (x, y) = (x.wrapping_mul(self.width), y.wrapping_mul(self.height));
        let fraction = |value: u32| (((value % divisor) << SUBPIXEL_BITS) / divisor) as u8;
        let screen_x = (x / divisor).wrapping_add(self.left);
        let screen_y = (y / divisor).wrapping_add(self.top);
        (
            [
                (screen_x & SCREEN_X_MASK) as u16,
                (screen_y & SCREEN_Y_MASK) as u16,
            ],
            [fraction(x), fraction(y)],
        )
    }
}

#[derive(Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Polygons {
    building: Frame,
    rendered: Frame,
    viewport: Viewport,
    pending_attributes: u32,
    attributes: u32,
    texture: u32,
    palette: u16,
    w_buffer: bool,
    swap: Option<u32>,
    primitive: Primitive,
    points: [Point; 4],
    count: usize,
    strip: usize,
    last: Option<usize>,
    overflow: bool,
    dot_limit: Option<i32>,
}

impl Clone for Polygons {
    fn clone(&self) -> Self {
        Self {
            building: self.building.clone(),
            rendered: self.rendered.clone(),
            viewport: self.viewport,
            pending_attributes: self.pending_attributes,
            attributes: self.attributes,
            texture: self.texture,
            palette: self.palette,
            w_buffer: self.w_buffer,
            swap: self.swap,
            primitive: self.primitive,
            points: self.points,
            count: self.count,
            strip: self.strip,
            last: self.last,
            overflow: self.overflow,
            dot_limit: self.dot_limit,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        let Self {
            building,
            rendered,
            viewport,
            pending_attributes,
            attributes,
            texture,
            palette,
            w_buffer,
            swap,
            primitive,
            points,
            count,
            strip,
            last,
            overflow,
            dot_limit,
        } = self;
        building.clone_from(&source.building);
        rendered.clone_from(&source.rendered);
        viewport.clone_from(&source.viewport);
        pending_attributes.clone_from(&source.pending_attributes);
        attributes.clone_from(&source.attributes);
        texture.clone_from(&source.texture);
        palette.clone_from(&source.palette);
        w_buffer.clone_from(&source.w_buffer);
        swap.clone_from(&source.swap);
        primitive.clone_from(&source.primitive);
        points.clone_from(&source.points);
        count.clone_from(&source.count);
        strip.clone_from(&source.strip);
        last.clone_from(&source.last);
        overflow.clone_from(&source.overflow);
        dot_limit.clone_from(&source.dot_limit);
    }
}

impl Polygons {
    pub fn frame(&self) -> &Frame {
        &self.rendered
    }

    pub fn building(&self) -> &Frame {
        &self.building
    }

    pub fn attributes(&self) -> u32 {
        self.attributes
    }

    pub fn set_attributes(&mut self, param: u32) {
        self.pending_attributes = param;
    }

    pub fn set_texture(&mut self, param: u32) {
        self.texture = param;
    }

    pub fn texture(&self) -> u32 {
        self.texture
    }

    pub fn set_palette(&mut self, param: u32) {
        self.palette = (param & PALETTE_MASK) as u16;
    }

    pub fn set_dot_depth(&mut self, param: u32) {
        let depth = (param & DOT_DEPTH_MASK) as i32;
        self.dot_limit = Some((depth << DOT_DEPTH_SHIFT) + DOT_DEPTH_LOW);
    }

    pub fn overflow(&self) -> bool {
        self.overflow
    }

    pub fn acknowledge_overflow(&mut self) {
        self.overflow = false;
    }

    pub fn set_viewport(&mut self, param: u32) {
        self.viewport = Viewport::from_param(param);
    }

    pub fn begin(&mut self, param: u32) {
        self.primitive = Primitive::from_param(param);
        self.attributes = self.pending_attributes;
        self.count = 0;
        self.strip = 0;
        self.last = None;
    }

    pub fn request_swap(&mut self, param: u32) {
        self.swap = Some(param);
        self.w_buffer = param & W_BUFFER != 0;
    }

    pub fn swap(&mut self) -> bool {
        let Some(param) = self.swap.take() else {
            return false;
        };
        self.building.sort(param & SORT_MANUAL != 0);
        std::mem::swap(&mut self.building, &mut self.rendered);
        self.building.clear();
        self.last = None;
        true
    }

    pub fn vertex(&mut self, point: Point) {
        self.points[self.count] = point;
        self.count += 1;
        let [a, b, c, d] = self.points;
        match self.primitive {
            Primitive::Triangles if self.count == 3 => {
                self.count = 0;
                self.submit(&[a, b, c]);
            }
            Primitive::Quads if self.count == 4 => {
                self.count = 0;
                self.submit(&[a, b, c, d]);
            }
            Primitive::TriangleStrip if self.count == 3 => {
                if self.strip % 2 == 1 {
                    self.submit(&[b, a, c]);
                } else {
                    self.submit(&[a, b, c]);
                }
                self.points[0] = b;
                self.points[1] = c;
                self.count = 2;
            }
            Primitive::QuadStrip if self.count == 4 => {
                self.submit(&[a, b, d, c]);
                self.points[0] = c;
                self.points[1] = d;
                self.count = 2;
            }
            _ => {}
        }
    }

    fn submit(&mut self, points: &[Point]) {
        let strip = self.strip;
        self.strip += 1;
        let front = match facing(&points[0], &points[1], &points[2]) {
            dot if dot < 0 => {
                if self.attributes & RENDER_FRONT == 0 {
                    self.last = None;
                    return;
                }
                true
            }
            dot if dot > 0 => {
                if self.attributes & RENDER_BACK == 0 {
                    self.last = None;
                    return;
                }
                false
            }
            _ => true,
        };

        let reused = self.reused(strip, points.len());
        let mut list = Points::from_slice(points);
        let mut screens = [[0u16; 2]; MAX_POLYGON_VERTICES];
        let mut subpixels = [[0u8; 2]; MAX_POLYGON_VERTICES];
        let start = match reused {
            Some(shared) => {
                for (slot, index) in shared.into_iter().enumerate() {
                    let vertex = &self.building.vertices[usize::from(index)];
                    list.as_mut_slice()[slot] = Point::from_vertex(vertex);
                    screens[slot] = vertex.screen;
                    subpixels[slot] = vertex.subpixel;
                }
                2
            }
            None => 0,
        };
        clip(&mut list, start, self.attributes & RENDER_FAR != 0);
        let count = list.len().min(MAX_POLYGON_VERTICES);
        if count == 0 {
            self.last = None;
            return;
        }
        if self.building.polygons.len() >= MAX_POLYGONS
            || self.building.vertices.len() + count > MAX_VERTICES
        {
            self.overflow = true;
            self.last = None;
            return;
        }

        for (index, point) in list
            .as_mut_slice()
            .iter_mut()
            .enumerate()
            .take(count)
            .skip(start)
        {
            point.position[3] &= W_MASK;
            screens[index] = self.viewport.screen(point.position);
            subpixels[index] = self.viewport.subpixel(point.position);
        }
        if self.hidden_dot(&list.as_slice()[..count], &screens[..count]) {
            self.last = None;
            return;
        }

        let mut polygon = Polygon {
            attributes: self.attributes,
            texture: self.texture,
            palette: self.palette,
            front,
            translucent: translucent(self.attributes, self.texture),
            w_buffer: self.w_buffer,
            ..Polygon::default()
        };
        if let Some(shared) = reused {
            if count == points.len() {
                polygon.vertices[..2].copy_from_slice(&shared);
            } else {
                for (slot, index) in shared.into_iter().enumerate() {
                    let copy = self.building.vertices[usize::from(index)];
                    polygon.vertices[slot] = self.building.vertices.len() as u16;
                    self.building.vertices.push(copy);
                }
            }
        }
        for (index, point) in list.as_slice().iter().enumerate().take(count).skip(start) {
            polygon.vertices[index] = self.building.vertices.len() as u16;
            self.building.vertices.push(Vertex {
                position: point.position,
                screen: screens[index],
                subpixel: subpixels[index],
                color: point.color(),
                texcoord: point.texcoord,
                clipped: point.clipped,
            });
        }
        polygon.count = count as u8;
        self.finish(&mut polygon);
        self.building.polygons.push(polygon);
        self.last = self
            .primitive
            .strip()
            .then(|| self.building.polygons.len() - 1);
    }

    fn hidden_dot(&self, points: &[Point], screens: &[[u16; 2]]) -> bool {
        let Some(limit) = self.dot_limit else {
            return false;
        };
        self.attributes & RENDER_DOT == 0
            && screens.iter().all(|&screen| screen == screens[0])
            && points.iter().all(|point| point.position[3] > limit)
    }

    fn reused(&self, strip: usize, count: usize) -> Option<[u16; 2]> {
        let last = &self.building.polygons[self.last?];
        let ids = match self.primitive {
            Primitive::TriangleStrip if strip % 2 == 1 => [2, 1],
            Primitive::TriangleStrip => [0, 2],
            Primitive::QuadStrip => [3, 2],
            _ => return None,
        };
        let shared = ids.map(|id| last.vertices[id]);
        let unclipped = shared
            .iter()
            .all(|&index| !self.building.vertices[usize::from(index)].clipped);
        (usize::from(last.count) == count && unclipped).then_some(shared)
    }

    fn finish(&self, polygon: &mut Polygon) {
        let vertices: Vec<Vertex> = polygon
            .vertices()
            .iter()
            .map(|&index| self.building.vertices[usize::from(index)])
            .collect();

        let (mut top_y, mut bottom_y, mut bottom_x) = (SCREEN_HEIGHT, 0, 0);
        let mut w_size = 0;
        for (index, vertex) in vertices.iter().enumerate() {
            let [x, y] = vertex.screen.map(i32::from);
            if y < top_y {
                top_y = y;
                polygon.top = index as u8;
            }
            if y > bottom_y || (y == bottom_y && x > bottom_x) {
                bottom_y = y;
                bottom_x = x;
                polygon.bottom = index as u8;
            }
            let w = vertex.position[3] as u32;
            polygon.degenerate |= w == 0;
            while w_size < 32 && w >> w_size != 0 {
                w_size += 4;
            }
        }
        polygon.degenerate |= bottom_y > SCREEN_HEIGHT;

        for (index, vertex) in vertices.iter().enumerate() {
            let [_, _, z, w] = vertex.position;
            let (normal, shifted) = if w_size < 16 {
                let normal = w << (16 - w_size);
                (normal, normal >> (16 - w_size))
            } else {
                let normal = w >> (w_size - 16);
                (normal, normal << (w_size - 16))
            };
            let depth = if self.w_buffer {
                shifted
            } else if w != 0 {
                ((i64::from(z) * 0x4000 / i64::from(w) + 0x3FFF) * 0x200) as i32
            } else {
                DEPTH_NO_W
            };
            polygon.depth[index] = depth.clamp(0, DEPTH_MAX);
            polygon.w[index] = normal;
        }
    }
}

fn facing(a: &Point, b: &Point, c: &Point) -> i64 {
    let delta = |p: &Point, axis: usize| i64::from(p.position[axis].wrapping_sub(b.position[axis]));
    let (ax, ay, aw) = (delta(a, 0), delta(a, 1), delta(a, 3));
    let (cx, cy, cw) = (delta(c, 0), delta(c, 1), delta(c, 3));
    let mut normal = [ay * cw - aw * cy, aw * cx - ax * cw, ax * cy - ay * cx];
    while normal.iter().any(|&n| i32::try_from(n).is_err()) {
        normal = normal.map(|n| n >> 4);
    }
    let [bx, by, _, bw] = b.position.map(i64::from);
    (bx * normal[0])
        .wrapping_add(by * normal[1])
        .wrapping_add(bw * normal[2])
}

fn translucent(attributes: u32, texture: u32) -> bool {
    let format = (texture >> FORMAT_SHIFT) & 7;
    let alpha = (attributes >> ALPHA_SHIFT) & ALPHA_MASK;
    format == FORMAT_A3I5 || format == FORMAT_A5I3 || (1..ALPHA_MASK).contains(&alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH_SIDES: u32 = RENDER_BACK | RENDER_FRONT;
    const SOLID: u32 = ALPHA_MASK << ALPHA_SHIFT;
    const FULL_VIEW: u32 = 0xBFFF_0000;

    fn assembler(attributes: u32, primitive: u32) -> Polygons {
        let mut polygons = Polygons::default();
        polygons.set_viewport(FULL_VIEW);
        polygons.set_attributes(attributes);
        polygons.begin(primitive);
        polygons
    }

    fn point(x: i32, y: i32) -> Point {
        Point::new([x, y, 0, 0x1000], [31, 0, 16], [0, 0])
    }

    fn feed(polygons: &mut Polygons, points: &[(i32, i32)]) {
        for &(x, y) in points {
            polygons.vertex(point(x, y));
        }
    }

    fn screens(polygons: &Polygons, polygon: usize) -> Vec<[u16; 2]> {
        let frame = polygons.building();
        let polygon = &frame.polygons[polygon];
        (0..usize::from(polygon.count))
            .map(|index| frame.vertex(polygon, index).screen)
            .collect()
    }

    #[test]
    fn viewport_maps_the_view_volume_to_the_screen_per_gbatek() {
        let viewport = Viewport::from_param(FULL_VIEW);
        assert_eq!(viewport.screen([-0x1000, 0x1000, 0, 0x1000]), [0, 0]);
        assert_eq!(viewport.screen([0, 0, 0, 0x1000]), [128, 96]);
        assert_eq!(viewport.screen([0x1000, -0x1000, 0, 0x1000]), [256, 192]);
    }

    #[test]
    fn subpixels_keep_sixteenths_of_the_pixel_the_screen_position_drops() {
        let viewport = Viewport::from_param(FULL_VIEW);
        assert_eq!(viewport.screen([0x10, -0x8, 0, 0x1000]), [128, 96]);
        assert_eq!(viewport.subpixel([0x10, -0x8, 0, 0x1000]), [8, 3]);
        assert_eq!(viewport.subpixel([0, 0, 0, 0x1000]), [0, 0]);
    }

    #[test]
    fn viewport_y_counts_up_from_the_bottom_per_gbatek() {
        let viewport = Viewport::from_param(0x5F7F_0000);
        assert_eq!(viewport.screen([-0x1000, 0x1000, 0, 0x1000]), [0, 96]);
        assert_eq!(viewport.screen([0x1000, -0x1000, 0, 0x1000]), [128, 192]);
    }

    #[test]
    fn viewport_halves_large_w_for_the_32_bit_divider_like_melonds() {
        let viewport = Viewport::from_param(FULL_VIEW);
        assert_eq!(viewport.screen([-0xF201, 0, 0, 0x10001]), [7, 96]);
        assert_eq!(viewport.screen([1, 0, 0, 0x10000]), [128, 96]);
    }

    #[test]
    fn zero_w_lands_on_the_origin_like_melonds() {
        let viewport = Viewport::from_param(FULL_VIEW);
        assert_eq!(viewport.screen([0x1000, 0x1000, 0, 0]), [0, 0]);
    }

    #[test]
    fn color_expands_five_bits_to_six_per_gbatek() {
        let point = Point::new([0; 4], [0, 1, 31], [0, 0]);
        assert_eq!(point.color(), [0, 3, 63]);
    }

    #[test]
    fn separate_triangles_take_three_vertices_each() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 0);
        feed(
            &mut polygons,
            &[(0, 0), (0x800, 0), (0, 0x800), (0, 0), (0x800, 0)],
        );
        assert_eq!(polygons.building().polygons.len(), 1);
        assert_eq!(polygons.building().vertices.len(), 3);
        assert_eq!(screens(&polygons, 0), [[128, 96], [192, 96], [128, 48]]);
    }

    #[test]
    fn separate_quads_take_four_vertices_each() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 1);
        feed(
            &mut polygons,
            &[(0, 0), (0x800, 0), (0x800, 0x800), (0, 0x800)],
        );
        assert_eq!(polygons.building().polygons[0].count, 4);
        assert_eq!(polygons.building().vertices.len(), 4);
    }

    #[test]
    fn triangle_strip_flips_every_second_triangle_and_shares_two_vertices() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 2);
        feed(
            &mut polygons,
            &[(0, 0), (0, -0x800), (0x400, 0), (0x400, -0x800), (0x800, 0)],
        );
        let frame = polygons.building();
        assert_eq!(frame.polygons.len(), 3);
        assert_eq!(frame.vertices.len(), 5);
        assert_eq!(frame.polygons[0].vertices(), [0, 1, 2]);
        assert_eq!(frame.polygons[1].vertices(), [2, 1, 3]);
        assert_eq!(frame.polygons[2].vertices(), [2, 3, 4]);
        assert!(
            frame
                .polygons
                .iter()
                .all(|polygon| polygon.front == frame.polygons[0].front)
        );
    }

    #[test]
    fn quad_strip_orders_each_quad_round_and_shares_an_edge() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 3);
        feed(
            &mut polygons,
            &[
                (0, 0x800),
                (0, 0),
                (0x400, 0x800),
                (0x400, 0),
                (0x800, 0x800),
                (0x800, 0),
            ],
        );
        let frame = polygons.building();
        assert_eq!(frame.polygons.len(), 2);
        assert_eq!(frame.vertices.len(), 6);
        assert_eq!(frame.polygons[0].vertices(), [0, 1, 2, 3]);
        assert_eq!(frame.polygons[1].vertices(), [3, 2, 4, 5]);
        assert_eq!(frame.vertex(&frame.polygons[0], 2).screen, [160, 96]);
    }

    #[test]
    fn counter_clockwise_triangles_face_the_viewer_per_gbatek() {
        let mut polygons = assembler(RENDER_FRONT | SOLID, 0);
        feed(&mut polygons, &[(0, 0), (0x800, 0), (0, 0x800)]);
        assert_eq!(polygons.building().polygons.len(), 1);
        assert!(polygons.building().polygons[0].front);
    }

    #[test]
    fn back_faces_are_dropped_unless_bit_6_is_set() {
        let mut polygons = assembler(RENDER_FRONT | SOLID, 0);
        feed(&mut polygons, &[(0, 0), (0, 0x800), (0x800, 0)]);
        assert!(polygons.building().polygons.is_empty());
        let mut polygons = assembler(RENDER_BACK | SOLID, 0);
        feed(&mut polygons, &[(0, 0), (0, 0x800), (0x800, 0)]);
        assert!(!polygons.building().polygons[0].front);
    }

    #[test]
    fn front_faces_are_dropped_unless_bit_7_is_set() {
        let mut polygons = assembler(RENDER_BACK | SOLID, 0);
        feed(&mut polygons, &[(0, 0), (0x800, 0), (0, 0x800)]);
        assert!(polygons.building().polygons.is_empty());
    }

    #[test]
    fn lines_draw_with_neither_side_set_per_gbatek() {
        let mut polygons = assembler(SOLID, 0);
        feed(&mut polygons, &[(0, 0), (0x800, 0x800), (0x800, 0x800)]);
        assert_eq!(polygons.building().polygons.len(), 1);
    }

    #[test]
    fn a_dropped_strip_triangle_stops_vertex_sharing_like_melonds() {
        let mut polygons = assembler(RENDER_FRONT | SOLID, 2);
        feed(
            &mut polygons,
            &[(0, 0), (0, -0x800), (0x400, 0), (0x400, 0x800), (0, 0x400)],
        );
        let frame = polygons.building();
        assert_eq!(frame.polygons.len(), 2);
        assert_eq!(frame.vertices.len(), 6);
        assert_eq!(frame.polygons[1].vertices(), [3, 4, 5]);
    }

    #[test]
    fn attributes_latch_at_begin_per_gbatek() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 0);
        polygons.set_attributes(BOTH_SIDES | 0x0010_0000);
        feed(&mut polygons, &[(0, 0), (0x800, 0), (0, 0x800)]);
        assert_eq!(
            polygons.building().polygons[0].attributes,
            BOTH_SIDES | SOLID
        );
        polygons.begin(0);
        feed(&mut polygons, &[(0, 0), (0x800, 0), (0, 0x800)]);
        assert_eq!(
            polygons.building().polygons[1].attributes,
            BOTH_SIDES | 0x0010_0000
        );
    }

    #[test]
    fn begin_drops_a_half_built_polygon() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 0);
        feed(&mut polygons, &[(0, 0), (0x800, 0)]);
        polygons.begin(0);
        feed(&mut polygons, &[(0, 0), (0x800, 0), (0, 0x800)]);
        assert_eq!(polygons.building().polygons.len(), 1);
        assert_eq!(polygons.building().vertices.len(), 3);
    }

    #[test]
    fn translucency_comes_from_alpha_or_the_alpha_formats() {
        assert!(!translucent(SOLID, 0));
        assert!(!translucent(0, 0));
        assert!(translucent(15 << ALPHA_SHIFT, 0));
        assert!(translucent(SOLID, FORMAT_A3I5 << FORMAT_SHIFT));
        assert!(translucent(SOLID, FORMAT_A5I3 << FORMAT_SHIFT));
        assert!(!translucent(SOLID, 7 << FORMAT_SHIFT));
    }

    #[test]
    fn polygon_keeps_the_texture_and_palette_set_before_it() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 0);
        polygons.set_texture(0x1234_5678);
        polygons.set_palette(0xFFFF);
        feed(&mut polygons, &[(0, 0), (0x800, 0), (0, 0x800)]);
        let polygon = polygons.building().polygons[0];
        assert_eq!(polygon.texture, 0x1234_5678);
        assert_eq!(polygon.palette, 0x1FFF);
    }

    #[test]
    fn top_and_bottom_pick_the_highest_and_lowest_vertices() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 1);
        feed(
            &mut polygons,
            &[(0, 0), (0x800, -0x800), (0x800, 0x800), (-0x800, -0x800)],
        );
        let polygon = polygons.building().polygons[0];
        assert_eq!(polygon.top, 2);
        assert_eq!(polygon.bottom, 1);
    }

    #[test]
    fn z_buffer_depth_maps_z_over_w_to_24_bits_like_melonds() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 0);
        for (x, z) in [(0, -0x1000), (0x800, 0), (0, 0x800)] {
            polygons.vertex(Point::new([x, x, z, 0x1000], [0; 3], [0; 2]));
        }
        let polygon = polygons.building().polygons[0];
        assert_eq!(&polygon.depth[..3], &[0, 0x7FFE00, 0xBFFE00]);
        assert!(!polygon.w_buffer);
    }

    #[test]
    fn w_is_normalised_to_16_bits_per_polygon_like_melonds() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 0);
        for (x, w) in [(0, 0x100), (0x10, 0x200), (0, 0x80)] {
            polygons.vertex(Point::new([x, x, 0, w], [0; 3], [0; 2]));
        }
        let polygon = polygons.building().polygons[0];
        assert_eq!(&polygon.w[..3], &[0x1000, 0x2000, 0x0800]);
    }

    #[test]
    fn w_buffer_depth_is_the_normalised_w_after_a_swap_asks_for_it() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 0);
        polygons.request_swap(W_BUFFER);
        polygons.swap();
        polygons.begin(0);
        for (x, w) in [(0, 0x1_2345), (0x100, 0x1000), (0, 0x2000)] {
            polygons.vertex(Point::new([x, x, 0, w], [0; 3], [0; 2]));
        }
        let polygon = polygons.building().polygons[0];
        assert!(polygon.w_buffer);
        assert_eq!(&polygon.w[..3], &[0x1234, 0x0100, 0x0200]);
        assert_eq!(&polygon.depth[..3], &[0x1_2340, 0x1000, 0x2000]);
    }

    #[test]
    fn zero_w_makes_the_polygon_degenerate() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 0);
        for (x, w) in [(0, 0x1000), (0x100, 0x1000), (0, 0)] {
            polygons.vertex(Point::new([x, x, 0, w], [0; 3], [0; 2]));
        }
        let polygon = polygons.building().polygons[0];
        assert!(polygon.degenerate);
        assert_eq!(polygon.depth[2], 0x7FFE00);
    }

    #[test]
    fn swap_hands_the_frame_over_and_starts_empty() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 0);
        feed(&mut polygons, &[(0, 0), (0x800, 0), (0, 0x800)]);
        assert!(!polygons.swap());
        polygons.request_swap(0);
        assert!(polygons.swap.is_some());
        assert!(polygons.swap());
        assert_eq!(polygons.frame().polygons.len(), 1);
        assert!(polygons.building().polygons.is_empty());
        assert!(polygons.building().vertices.is_empty());
    }

    #[test]
    fn a_strip_that_runs_on_past_a_swap_starts_its_next_polygon_fresh() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 2);
        feed(&mut polygons, &[(0, 0), (0x800, 0), (0, 0x800)]);
        polygons.request_swap(0);
        assert!(polygons.swap());
        feed(&mut polygons, &[(0x800, 0x800)]);
        let frame = polygons.building();
        assert_eq!(frame.polygons.len(), 1);
        assert_eq!(frame.vertices.len(), 3);
    }

    fn quads(polygons: &mut Polygons, count: usize) {
        for _ in 0..count {
            feed(polygons, &[(0, 0), (0x800, 0), (0x800, 0x800), (0, 0x800)]);
        }
    }

    #[test]
    fn clipped_polygons_store_the_cut_vertices_per_gbatek() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 0);
        feed(&mut polygons, &[(0, 0), (0x2000, 0), (0, 0x800)]);
        let frame = polygons.building();
        assert_eq!(frame.polygons[0].count, 4);
        let clipped: Vec<bool> = frame.vertices.iter().map(|vertex| vertex.clipped).collect();
        assert_eq!(clipped, [false, true, true, false]);
        assert_eq!(frame.vertices[1].screen, [256, 96]);
    }

    #[test]
    fn polygons_outside_the_view_are_not_stored_per_gbatek() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 0);
        feed(&mut polygons, &[(0x2000, 0), (0x3000, 0), (0x2000, 0x800)]);
        assert!(polygons.building().polygons.is_empty());
        assert!(polygons.building().vertices.is_empty());
        assert!(!polygons.overflow());
    }

    #[test]
    fn far_plane_crossing_needs_bit_12_per_gbatek() {
        for (attributes, stored) in [(0, 0), (RENDER_FAR, 1)] {
            let mut polygons = assembler(BOTH_SIDES | SOLID | attributes, 0);
            for (x, z) in [(0, 0), (0x800, 0x2000), (0, 0)] {
                polygons.vertex(Point::new([x, x, z, 0x1000], [0; 3], [0; 2]));
            }
            assert_eq!(polygons.building().polygons.len(), stored);
        }
    }

    #[test]
    fn strip_shares_no_clipped_vertex_like_melonds() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 2);
        feed(
            &mut polygons,
            &[(0, 0), (0, -0x800), (0x2000, 0), (0x400, -0x800)],
        );
        let frame = polygons.building();
        assert_eq!(frame.polygons.len(), 2);
        assert_eq!(frame.polygons[0].count, 4);
        assert_eq!(frame.vertices.len(), 4 + frame.polygons[1].count as usize);
    }

    #[test]
    fn strip_polygon_cut_into_more_vertices_copies_the_shared_pair_like_melonds() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 2);
        feed(
            &mut polygons,
            &[(0, 0), (0, -0x800), (0x400, 0), (0x2000, -0x800)],
        );
        let frame = polygons.building();
        assert_eq!(frame.polygons[1].vertices(), [3, 4, 5, 6]);
        assert_eq!(frame.vertices[3], frame.vertices[2]);
        assert_eq!(frame.vertices[4], frame.vertices[1]);
    }

    #[test]
    fn vertex_ram_holds_6144_vertices() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 1);
        quads(&mut polygons, 1536);
        assert_eq!(polygons.building().vertices.len(), 6144);
        assert!(!polygons.overflow());
        quads(&mut polygons, 1);
        assert_eq!(polygons.building().polygons.len(), 1536);
        assert!(polygons.overflow());
        polygons.acknowledge_overflow();
        assert!(!polygons.overflow());
    }

    #[test]
    fn polygon_ram_holds_2048_polygons() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 2);
        for index in 0..2050 {
            polygons.vertex(point((index % 2) * 0x100, (index / 2) % 4));
        }
        assert_eq!(polygons.building().polygons.len(), 2048);
        assert!(!polygons.overflow());
        polygons.vertex(point(0, 0x10));
        assert_eq!(polygons.building().polygons.len(), 2048);
        assert!(polygons.overflow());
    }

    #[test]
    fn one_dot_polygons_beyond_the_depth_are_hidden_per_gbatek() {
        let mut polygons = assembler(BOTH_SIDES | SOLID, 0);
        feed(&mut polygons, &[(0, 0); 3]);
        polygons.set_dot_depth(0);
        feed(&mut polygons, &[(0, 0); 3]);
        polygons.set_dot_depth(8);
        feed(&mut polygons, &[(0, 0); 3]);
        polygons.set_dot_depth(7);
        feed(&mut polygons, &[(0, 0); 3]);
        feed(&mut polygons, &[(0, 0), (0x40, 0), (0, 0)]);
        assert_eq!(polygons.building().polygons.len(), 3);
    }

    #[test]
    fn bit_13_shows_one_dot_polygons_per_gbatek() {
        let mut polygons = assembler(BOTH_SIDES | SOLID | RENDER_DOT, 0);
        polygons.set_dot_depth(0);
        feed(&mut polygons, &[(0, 0); 3]);
        assert_eq!(polygons.building().polygons.len(), 1);
    }
}
