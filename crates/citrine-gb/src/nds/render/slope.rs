const FRACTION: u32 = 18;
pub const ONE: i32 = 1 << FRACTION;
const HALF: i32 = ONE >> 1;
const LOW_BITS: i32 = (1 << (FRACTION / 2)) - 1;
const COVERAGE_BITS: u32 = 10;
const COVERAGE_MASK: i32 = (1 << COVERAGE_BITS) - 1;
pub const FULL_COVERAGE: u8 = 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    Fixed(u8),
    Ramp { start: i32, step: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slope {
    x_length: i32,
    y_length: i32,
    edge_increment: i32,
    edge_step: i32,
    edge_negative: bool,
    x0: i32,
    y0: i32,
    increment: i32,
    negative: bool,
    x_major: bool,
}

impl Slope {
    pub fn new(top: [i32; 2], bottom: [i32; 2]) -> Self {
        let (top, bottom) = if bottom[1] < top[1] {
            (bottom, top)
        } else {
            (top, bottom)
        };
        let mut x0 = top[0] << FRACTION;
        let mut dx = bottom[0] - top[0];
        let dy = bottom[1] - top[1];
        let negative = dx < 0;
        if negative {
            x0 -= 1;
            dx = -dx;
        }
        let x_major = dx > dy;
        if x_major || dx == dy {
            x0 += if negative { -HALF } else { HALF };
        }
        let increment = if dx == dy && dy != 0 {
            ONE
        } else {
            dx.wrapping_mul(if dy == 0 { ONE } else { ONE / dy })
        };
        let x_length = (bottom[0] - top[0]).abs().max(1);
        let y_length = bottom[1] - top[1];
        let edge_increment = if y_length == 0 {
            0
        } else if y_length == x_length && x_length != 1 {
            ONE
        } else {
            ((bottom[0] - top[0]).wrapping_mul(ONE / y_length)).abs()
        };
        Self {
            x_length,
            y_length,
            edge_increment,
            edge_step: ((y_length << COVERAGE_BITS) / x_length) & COVERAGE_MASK,
            edge_negative: bottom[0] < top[0],
            x0,
            y0: top[1],
            increment,
            negative,
            x_major,
        }
    }

    pub fn increment(&self) -> i32 {
        self.increment
    }

    pub fn negative(&self) -> bool {
        self.negative
    }

    pub fn x_major(&self) -> bool {
        self.x_major
    }

    pub fn vertical(&self) -> bool {
        self.increment == 0
    }

    fn start(&self, y: i32) -> i32 {
        let displacement = (y - self.y0).wrapping_mul(self.increment);
        if self.negative {
            self.x0.wrapping_sub(displacement)
        } else {
            self.x0.wrapping_add(displacement)
        }
    }

    fn end(&self, y: i32) -> i32 {
        let start = self.start(y);
        match (self.x_major, self.negative) {
            (false, _) => start,
            (true, false) => (start & !LOW_BITS) + self.increment - ONE,
            (true, true) => (start | LOW_BITS) - self.increment + ONE,
        }
    }

    pub fn coverage(&self, y: i32, right: bool, swapped: bool) -> Coverage {
        let negative = self.edge_negative;
        let (x_length, y_length) = (self.x_length, self.y_length);
        let increment = self.edge_increment;
        let x_major = increment > ONE;
        let start = match (x_major, increment != 0, right, negative) {
            (true, _, true, true) => HALF + ONE,
            (true, _, true, false) => increment - HALF,
            (true, _, false, true) => increment - HALF + ONE,
            (true, _, false, false) => HALF,
            (false, true, _, true) => ONE,
            _ => 0,
        };
        let dx = start.wrapping_add((y - self.y0).wrapping_mul(increment));
        if x_major {
            let length = if right != negative {
                (dx >> FRACTION) - (dx.wrapping_sub(increment) >> FRACTION)
            } else {
                (dx.wrapping_add(increment) >> FRACTION) - (dx >> FRACTION)
            };
            let mut first = dx >> FRACTION;
            if negative {
                first = x_length - first;
            }
            if right {
                first = first - length + 1;
            }
            let start = ((first << COVERAGE_BITS) + 0x1FF).wrapping_mul(y_length) / x_length;
            let start = match start & COVERAGE_MASK {
                COVERAGE_MASK => 0,
                start => start,
            };
            Coverage::Ramp {
                start,
                step: self.edge_step,
            }
        } else if increment == 0 {
            Coverage::Fixed(if swapped { 0 } else { FULL_COVERAGE })
        } else {
            let mut coverage = ((dx >> 9) + (increment >> 10)) >> 4;
            if coverage >> 5 != dx >> FRACTION {
                coverage = i32::from(FULL_COVERAGE);
            }
            let coverage = (coverage & 0x1F) as u8;
            let invert = if swapped {
                right != negative
            } else {
                right == negative
            };
            Coverage::Fixed(if invert {
                FULL_COVERAGE - coverage
            } else {
                coverage
            })
        }
    }

    pub fn span(&self, y: i32) -> (i32, i32) {
        let (start, end) = (self.start(y) >> FRACTION, self.end(y) >> FRACTION);
        (start.min(end), start.max(end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gaps(slope: &Slope, rows: i32) -> Vec<i32> {
        (0..rows - 1)
            .filter(|&y| {
                let (low, high) = slope.span(y);
                let (next_low, next_high) = slope.span(y + 1);
                if slope.negative() {
                    low - next_high > 1
                } else {
                    next_low - high > 1
                }
            })
            .collect()
    }

    #[test]
    fn a_diagonal_edge_steps_exactly_one_pixel_per_line_like_melonds() {
        assert_eq!(Slope::new([127, 0], [31, 96]).increment(), ONE);
        assert_eq!(Slope::new([129, 0], [225, 96]).increment(), ONE);
        assert_eq!(Slope::new([0, 0], [97, 96]).increment(), 97 * (ONE / 96));
    }

    #[test]
    fn x_major_spans_start_half_a_pixel_in_per_nds_interp() {
        let slope = Slope::new([0, 0], [10, 2]);
        assert!(slope.x_major());
        assert_eq!(slope.span(0), (0, 4));
        assert_eq!(slope.span(1), (5, 9));
    }

    #[test]
    fn y_major_edges_take_one_pixel_without_the_bias_per_nds_interp() {
        let slope = Slope::new([0, 0], [2, 10]);
        assert!(!slope.x_major());
        assert_eq!(slope.span(4), (0, 0));
        assert_eq!(slope.span(9), (1, 1));
    }

    #[test]
    fn the_reciprocal_comes_first_and_loses_precision_per_nds_interp() {
        let slope = Slope::new([0, 0], [2, 10]);
        assert_eq!(slope.increment(), 2 * (ONE / 10));
        assert_eq!(slope.span(5), (0, 0));
    }

    #[test]
    fn clearing_nine_bits_leaves_a_gap_on_69x49_per_nds_interp() {
        assert_eq!(gaps(&Slope::new([0, 0], [69, 49]), 49), [37]);
        assert_eq!(gaps(&Slope::new([0, 0], [71, 49]), 49), [38]);
        assert_eq!(gaps(&Slope::new([0, 0], [70, 66]), 66), [57]);
    }

    #[test]
    fn negative_slopes_mirror_positive_ones_with_gaps_on_the_same_rows_per_nds_interp() {
        let slope = Slope::new([69, 0], [0, 49]);
        assert!(slope.negative());
        assert_eq!(slope.span(0), (68, 68));
        assert_eq!(gaps(&slope, 49), [37]);
        let positive = Slope::new([0, 0], [69, 49]);
        for y in 0..49 {
            let (low, high) = positive.span(y);
            assert_eq!(slope.span(y), (68 - high, 68 - low));
        }
    }

    #[test]
    fn the_endpoints_order_does_not_matter() {
        assert_eq!(Slope::new([10, 2], [0, 0]), Slope::new([0, 0], [10, 2]));
    }

    #[test]
    fn vertical_edges_have_no_increment() {
        let slope = Slope::new([7, 3], [7, 40]);
        assert!(slope.vertical());
        assert_eq!(slope.span(20), (7, 7));
    }

    #[test]
    fn vertical_edges_cover_fully_unless_swapped_like_melonds() {
        let slope = Slope::new([7, 3], [7, 40]);
        assert_eq!(slope.coverage(10, false, false), Coverage::Fixed(31));
        assert_eq!(slope.coverage(10, true, true), Coverage::Fixed(0));
    }

    #[test]
    fn y_major_coverage_follows_the_subpixel_position_like_melonds() {
        let slope = Slope::new([0, 0], [8, 32]);
        let left: Vec<_> = (0..4).map(|y| slope.coverage(y, false, false)).collect();
        assert_eq!(left, [27, 19, 11, 3].map(Coverage::Fixed));
        assert_eq!(slope.coverage(1, true, false), Coverage::Fixed(12));
    }

    #[test]
    fn x_major_coverage_ramps_along_the_row_like_melonds() {
        let slope = Slope::new([0, 0], [40, 10]);
        assert_eq!(
            slope.coverage(0, false, false),
            Coverage::Ramp {
                start: 127,
                step: 256
            }
        );
        assert_eq!(
            slope.coverage(1, true, false),
            Coverage::Ramp {
                start: 127,
                step: 256
            }
        );
    }

    #[test]
    fn diagonals_get_the_bias_but_stay_one_pixel_wide_per_nds_interp() {
        let slope = Slope::new([0, 0], [3, 3]);
        assert!(!slope.x_major());
        assert_eq!(slope.span(0), (0, 0));
        assert_eq!(slope.span(2), (2, 2));
    }
}
