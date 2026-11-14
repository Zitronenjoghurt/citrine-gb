#[cfg(feature = "serde")]
mod packed;

pub const MAX_POLYGONS: usize = 2048;
pub const MAX_VERTICES: usize = 6144;
pub const MAX_POLYGON_VERTICES: usize = 10;
pub const SUBPIXEL_BITS: u32 = 4;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vertex {
    pub position: [i32; 4],
    pub screen: [u16; 2],
    pub subpixel: [u8; 2],
    pub color: [u8; 3],
    pub texcoord: [i16; 2],
    pub clipped: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Polygon {
    pub vertices: [u16; MAX_POLYGON_VERTICES],
    pub count: u8,
    pub depth: [i32; MAX_POLYGON_VERTICES],
    pub w: [i32; MAX_POLYGON_VERTICES],
    pub attributes: u32,
    pub texture: u32,
    pub palette: u16,
    pub front: bool,
    pub translucent: bool,
    pub w_buffer: bool,
    pub degenerate: bool,
    pub top: u8,
    pub bottom: u8,
}

impl Polygon {
    pub fn vertices(&self) -> &[u16] {
        &self.vertices[..usize::from(self.count)]
    }
}

#[derive(Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Frame {
    #[cfg_attr(feature = "serde", serde(with = "packed"))]
    pub polygons: Vec<Polygon>,
    #[cfg_attr(feature = "serde", serde(with = "packed"))]
    pub vertices: Vec<Vertex>,
}

impl Clone for Frame {
    fn clone(&self) -> Self {
        Self {
            polygons: self.polygons.clone(),
            vertices: self.vertices.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.polygons.clone_from(&source.polygons);
        self.vertices.clone_from(&source.vertices);
    }
}

impl Frame {
    pub fn vertex(&self, polygon: &Polygon, index: usize) -> &Vertex {
        &self.vertices[usize::from(polygon.vertices[index])]
    }

    pub fn clear(&mut self) {
        self.polygons.clear();
        self.vertices.clear();
    }

    pub fn sort(&mut self, manual: bool) {
        let vertices = &self.vertices;
        let y = |polygon: &Polygon, index: u8| {
            u64::from(vertices[usize::from(polygon.vertices[usize::from(index)])].screen[1])
        };
        let mut order: Vec<u64> = self
            .polygons
            .iter()
            .enumerate()
            .map(|(at, polygon)| {
                let key = if polygon.translucent && manual {
                    1 << 32
                } else {
                    u64::from(polygon.translucent) << 32
                        | y(polygon, polygon.bottom) << 16
                        | y(polygon, polygon.top)
                };
                key << 16 | at as u64
            })
            .collect();
        order.sort_unstable();
        for at in 0..order.len() {
            let mut from = (order[at] & 0xFFFF) as usize;
            while from < at {
                from = (order[from] & 0xFFFF) as usize;
            }
            order[at] = from as u64;
            self.polygons.swap(at, from);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(spans: &[(u16, u16, bool)]) -> Frame {
        let mut frame = Frame::default();
        for (index, &(top, bottom, translucent)) in spans.iter().enumerate() {
            let first = frame.vertices.len() as u16;
            for y in [top, bottom, bottom] {
                frame.vertices.push(Vertex {
                    screen: [index as u16, y],
                    ..Vertex::default()
                });
            }
            frame.polygons.push(Polygon {
                vertices: [first, first + 1, first + 2, 0, 0, 0, 0, 0, 0, 0],
                count: 3,
                translucent,
                top: 0,
                bottom: 1,
                texture: index as u32,
                ..Polygon::default()
            });
        }
        frame
    }

    fn order(frame: &Frame) -> Vec<u32> {
        frame
            .polygons
            .iter()
            .map(|polygon| polygon.texture)
            .collect()
    }

    #[test]
    fn sort_puts_opaque_first_by_bottom_then_top_like_melonds() {
        let mut frame = frame(&[
            (10, 50, false),
            (0, 40, false),
            (20, 40, false),
            (0, 10, true),
        ]);
        frame.sort(false);
        assert_eq!(order(&frame), [1, 2, 0, 3]);
    }

    #[test]
    fn sort_keeps_submission_order_on_ties() {
        let mut frame = frame(&[(5, 30, false), (5, 30, false), (5, 30, false)]);
        frame.sort(false);
        assert_eq!(order(&frame), [0, 1, 2]);
    }

    #[test]
    fn auto_sort_orders_translucent_polygons_by_y() {
        let mut frame = frame(&[(0, 90, true), (0, 20, true), (0, 50, false)]);
        frame.sort(false);
        assert_eq!(order(&frame), [2, 1, 0]);
    }

    #[test]
    fn manual_sort_keeps_translucent_polygons_in_order() {
        let mut frame = frame(&[(0, 90, true), (0, 20, true), (0, 50, false)]);
        frame.sort(true);
        assert_eq!(order(&frame), [2, 0, 1]);
    }

    #[test]
    fn sort_matches_a_stable_sort_by_translucency_bottom_and_top() {
        let mut seed = 0x2545_f491_u32;
        let mut next = |range: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 16) % range
        };
        let spans: Vec<(u16, u16, bool)> = (0..MAX_POLYGONS)
            .map(|_| {
                let top = next(8) as u16 * 24;
                (top, top + next(4) as u16 * 8, next(3) == 0)
            })
            .collect();
        for manual in [false, true] {
            let mut frame = frame(&spans);
            let mut expected: Vec<(usize, &(u16, u16, bool))> = spans.iter().enumerate().collect();
            expected.sort_by_key(|&(_, &(top, bottom, translucent))| {
                if translucent && manual {
                    (true, (0, 0))
                } else {
                    (translucent, (bottom, top))
                }
            });
            frame.sort(manual);
            let expected: Vec<u32> = expected.iter().map(|&(at, _)| at as u32).collect();
            assert_eq!(order(&frame), expected);
        }
    }
}
