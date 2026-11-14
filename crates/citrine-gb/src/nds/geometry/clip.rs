use super::polygon::Point;

pub const CAPACITY: usize = 16;

const Z: usize = 2;
const Y: usize = 1;
const X: usize = 0;
const W: usize = 3;
const COLOR_LOW: i32 = 0xFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Points {
    items: [Point; CAPACITY],
    len: usize,
}

impl Default for Points {
    fn default() -> Self {
        Self {
            items: [Point::default(); CAPACITY],
            len: 0,
        }
    }
}

impl Points {
    pub fn from_slice(points: &[Point]) -> Self {
        let mut list = Self::default();
        for &point in points {
            list.push(point);
        }
        list
    }

    pub fn push(&mut self, point: Point) {
        if self.len < CAPACITY {
            self.items[self.len] = point;
            self.len += 1;
        }
    }

    pub fn as_slice(&self) -> &[Point] {
        &self.items[..self.len]
    }

    pub fn as_mut_slice(&mut self) -> &mut [Point] {
        &mut self.items[..self.len]
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

pub fn clip(points: &mut Points, start: usize, keep_far: bool) {
    if points.as_slice()[start.min(points.len)..]
        .iter()
        .all(inside_every_plane)
    {
        mark_colors(points);
        return;
    }
    let mut near = Points::default();
    for axis in [Z, Y, X] {
        if !clip_side(points, &mut near, start, axis, 1, keep_far)
            || !clip_side(&near, points, start, axis, -1, true)
        {
            points.len = 0;
            return;
        }
        mark_colors(points);
    }
}

fn inside_every_plane(point: &Point) -> bool {
    let w = i64::from(point.position[W]);
    [X, Y, Z].into_iter().all(|axis| {
        let coordinate = i64::from(point.position[axis]);
        coordinate <= w && -coordinate <= w
    })
}

fn mark_colors(points: &mut Points) {
    for point in points.as_mut_slice() {
        for channel in &mut point.color {
            *channel = (*channel & !COLOR_LOW) | COLOR_LOW;
        }
    }
}

fn clip_side(
    points: &Points,
    out: &mut Points,
    start: usize,
    axis: usize,
    side: i64,
    keep_far: bool,
) -> bool {
    let all = points.as_slice();
    let count = all.len();
    let outside = |point: &Point| {
        let coordinate = i64::from(point.position[axis]) * side;
        coordinate > i64::from(point.position[W])
    };
    out.len = 0;
    for &point in &all[..start.min(count)] {
        out.push(point);
    }
    for index in start..count {
        let point = &all[index];
        if !outside(point) {
            out.push(*point);
            continue;
        }
        if axis == Z && side == 1 && !keep_far {
            return false;
        }
        let previous = &all[(index + count - 1) % count];
        let next = &all[(index + 1) % count];
        for neighbour in [previous, next] {
            if !outside(neighbour) {
                out.push(intersect(point, neighbour, axis, side));
            }
        }
    }
    true
}

fn intersect(outside: &Point, inside: &Point, axis: usize, side: i64) -> Point {
    let distance =
        |point: &Point| i64::from(point.position[W]) - side * i64::from(point.position[axis]);
    let numerator = distance(outside);
    let denominator = i64::from((numerator - distance(inside)) as i32);
    let lerp = |from: i32, to: i32| -> i32 {
        let delta = i64::from(to.wrapping_sub(from)) * numerator;
        let step = delta.checked_div(denominator).unwrap_or(0);
        (i64::from(from) + step) as i32
    };
    let mut point = *outside;
    for component in [X, Y, Z, W] {
        if component != axis {
            point.position[component] =
                lerp(outside.position[component], inside.position[component]);
        }
    }
    point.position[axis] = (side as i32).wrapping_mul(point.position[W]);
    for channel in 0..3 {
        point.color[channel] = lerp(outside.color[channel], inside.color[channel]);
    }
    for axis in 0..2 {
        let from = i32::from(outside.texcoord[axis]);
        point.texcoord[axis] = lerp(from, i32::from(inside.texcoord[axis])) as i16;
    }
    point.clipped = true;
    point
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: i32, y: i32, z: i32) -> Point {
        Point::new([x, y, z, 0x1000], [31, 0, 0], [0, 0])
    }

    fn clipped(points: &Points, start: usize, keep_far: bool) -> Points {
        let mut points = *points;
        clip(&mut points, start, keep_far);
        points
    }

    fn positions(points: &Points) -> Vec<[i32; 4]> {
        points
            .as_slice()
            .iter()
            .map(|point| point.position)
            .collect()
    }

    #[test]
    fn inside_polygons_pass_unchanged() {
        let points = Points::from_slice(&[point(0, 0, 0), point(0x800, 0, 0), point(0, 0x800, 0)]);
        let clipped = clipped(&points, 0, false);
        assert_eq!(clipped, points);
        assert!(clipped.as_slice().iter().all(|point| !point.clipped));
    }

    #[test]
    fn edges_on_the_volume_are_inside() {
        let points = Points::from_slice(&[
            point(-0x1000, 0, 0),
            point(0x1000, 0, 0),
            point(0, 0x1000, 0),
        ]);
        assert_eq!(clipped(&points, 0, false).len(), 3);
    }

    #[test]
    fn an_outside_vertex_becomes_two_on_the_plane_per_gbatek() {
        let points = Points::from_slice(&[point(0, 0, 0), point(0x2000, 0, 0), point(0, 0x800, 0)]);
        let clipped = clipped(&points, 0, false);
        assert_eq!(
            positions(&clipped),
            [
                [0, 0, 0, 0x1000],
                [0x1000, 0, 0, 0x1000],
                [0x1000, 0x400, 0, 0x1000],
                [0, 0x800, 0, 0x1000]
            ]
        );
        let flags: Vec<bool> = clipped
            .as_slice()
            .iter()
            .map(|point| point.clipped)
            .collect();
        assert_eq!(flags, [false, true, true, false]);
    }

    #[test]
    fn interpolation_truncates_toward_zero_like_melonds() {
        let points = Points::from_slice(&[
            point(0, 0, 0),
            point(0x3000, 0x0001, 0),
            point(0, -0x0001, 0),
        ]);
        let clipped = clipped(&points, 0, false);
        assert_eq!(clipped.as_slice()[1].position, [0x1000, 1, 0, 0x1000]);
        assert_eq!(clipped.as_slice()[2].position, [0x1000, 0, 0, 0x1000]);
    }

    #[test]
    fn w_interpolates_with_the_rest() {
        let near = Point::new([0, 0, 0, 0x1000], [0; 3], [0; 2]);
        let far = Point::new([0x3000, 0, 0, 0x2000], [0; 3], [0; 2]);
        let side = Point::new([0, 0x800, 0, 0x1000], [0; 3], [0; 2]);
        let clipped = clipped(&Points::from_slice(&[near, far, side]), 0, false);
        assert_eq!(clipped.as_slice()[1].position, [0x1800, 0, 0, 0x1800]);
    }

    #[test]
    fn colors_and_texcoords_interpolate_and_colors_keep_five_bits_like_melonds() {
        let a = Point::new([0, 0, 0, 0x1000], [0, 0, 0], [0, 0]);
        let b = Point::new([0x2000, 0, 0, 0x1000], [31, 10, 3], [0x100, -0x100]);
        let c = Point::new([0, 0x800, 0, 0x1000], [0, 0, 0], [0, 0]);
        let clipped = clipped(&Points::from_slice(&[a, b, c]), 0, false);
        let cut = clipped.as_slice()[1];
        assert_eq!(cut.color.map(|channel| channel >> 12), [16, 5, 2]);
        assert_eq!(cut.color.map(|channel| channel & 0xFFF), [0xFFF; 3]);
        assert_eq!(cut.texcoord, [0x80, -0x80]);
    }

    #[test]
    fn the_far_plane_drops_the_polygon_unless_bit_12_per_gbatek() {
        let points =
            Points::from_slice(&[point(0, 0, 0), point(0x800, 0, 0x2000), point(0, 0x800, 0)]);
        assert!(clipped(&points, 0, false).is_empty());
        assert_eq!(clipped(&points, 0, true).len(), 4);
    }

    #[test]
    fn the_near_plane_always_clips() {
        let points =
            Points::from_slice(&[point(0, 0, 0), point(0x800, 0, -0x2000), point(0, 0x800, 0)]);
        let clipped = clipped(&points, 0, false);
        assert_eq!(clipped.len(), 4);
        assert_eq!(clipped.as_slice()[1].position[2], -0x1000);
    }

    #[test]
    fn a_polygon_wholly_outside_is_gone() {
        let points = Points::from_slice(&[
            point(0x2000, 0, 0),
            point(0x3000, 0, 0),
            point(0x2000, 0x800, 0),
        ]);
        assert!(clipped(&points, 0, false).is_empty());
    }

    #[test]
    fn a_corner_cut_by_two_planes_adds_vertices() {
        let points = Points::from_slice(&[
            point(0, 0, 0),
            point(0x1800, 0, 0),
            point(0x1800, 0x1800, 0),
            point(0, 0x1800, 0),
        ]);
        let clipped = clipped(&points, 0, false);
        assert_eq!(clipped.len(), 4);
        assert!(
            clipped
                .as_slice()
                .iter()
                .all(|point| point.position[0] <= 0x1000 && point.position[1] <= 0x1000)
        );
    }

    #[test]
    fn shared_strip_vertices_before_start_stay_first() {
        let points = Points::from_slice(&[point(0, 0, 0), point(0, 0x800, 0), point(0x2000, 0, 0)]);
        let clipped = clipped(&points, 2, false);
        assert_eq!(
            &positions(&clipped)[..2],
            &[[0, 0, 0, 0x1000], [0, 0x800, 0, 0x1000]]
        );
        assert_eq!(clipped.len(), 4);
    }

    #[test]
    fn y_is_clipped_before_x_like_melonds() {
        let points =
            Points::from_slice(&[point(0, 0, 0), point(0x2000, 0x2000, 0), point(0, 0x800, 0)]);
        let clipped = clipped(&points, 0, false);
        assert_eq!(
            positions(&clipped),
            [
                [0, 0, 0, 0x1000],
                [0x1000, 0x1000, 0, 0x1000],
                [0x0AAB, 0x1000, 0, 0x1000],
                [0, 0x800, 0, 0x1000]
            ]
        );
    }
}
