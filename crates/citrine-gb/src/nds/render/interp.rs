#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

#[derive(Debug, Clone, Copy)]
pub struct Interpolator {
    axis: Axis,
    start: i32,
    length: i64,
    divisor: i64,
    position: i64,
    shift: u32,
    linear: bool,
    w_buffer: bool,
    weighs: bool,
    reciprocal: i64,
    w0_numerator: u32,
    far: u32,
    w_step: u32,
    factor: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct Lane {
    from: i64,
    delta: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct DepthLane {
    from: i64,
    offset: i64,
    delta: i64,
    down: u32,
    back: u32,
}

impl Interpolator {
    pub fn new(axis: Axis, span: [i32; 2], w: [i32; 2], w_buffer: bool) -> Self {
        let length = span[1] - span[0];
        let low_bits = match axis {
            Axis::X => 0x7F,
            Axis::Y => 0x7E,
        };
        let [w0, w1] = w.map(|w| w as u32);
        let (w0_numerator, w0_denominator, w1_denominator, shift) = match axis {
            Axis::X => (w0, w0, w1, 8),
            Axis::Y => (w0 >> 1, (w0 + (w0 & !w1 & 1)) >> 1, w1 >> 1, 9),
        };
        let linear = w0 == w1 && w0 & low_bits == 0;
        Self {
            axis,
            start: span[0],
            length: i64::from(length),
            divisor: i64::from(if length == 0 { 1 } else { length }),
            position: 0,
            shift,
            linear,
            w_buffer,
            weighs: length != 0 && (!linear || w_buffer),
            reciprocal: if length != 0 {
                i64::from((1 << 22) / length)
            } else {
                0
            },
            w0_numerator,
            far: (length as u32).wrapping_mul(w1_denominator),
            w_step: w0_denominator.wrapping_sub(w1_denominator),
            factor: 0,
        }
    }

    #[inline(always)]
    pub fn set(&mut self, at: i32) {
        self.position = i64::from(at - self.start);
        if self.weighs {
            self.factor = self.weight();
        }
    }

    #[inline(always)]
    pub fn place(&mut self, at: i32) {
        self.position = i64::from(at - self.start);
        if self.weighs && self.w_buffer {
            self.factor = self.weight();
        }
    }

    #[inline(always)]
    fn weight(&self) -> i64 {
        let x = self.position as u32;
        let numerator = x.wrapping_mul(self.w0_numerator) << self.shift;
        let denominator = self.far.wrapping_add(x.wrapping_mul(self.w_step));
        i64::from(numerator.checked_div(denominator).unwrap_or(0))
    }

    pub fn lane(&self, a: i32, b: i32) -> Lane {
        let (a, b) = (i64::from(a), i64::from(b));
        Lane {
            from: a,
            delta: if self.length == 0 { 0 } else { b - a },
        }
    }

    pub fn depth_lane(&self, a: i32, b: i32) -> DepthLane {
        let lane = self.lane(a, b);
        if self.w_buffer || lane.delta == 0 {
            return DepthLane {
                from: lane.from,
                offset: 0,
                delta: lane.delta,
                down: self.shift,
                back: 0,
            };
        }
        let (low, rising) = (lane.from.min(lane.from + lane.delta), lane.delta > 0);
        let (delta, down, back) = match self.axis {
            Axis::Y => {
                let (mut delta, mut back) = (lane.delta.abs(), 0);
                while delta > 0x3FF {
                    delta >>= 1;
                    back += 1;
                }
                (delta * self.reciprocal, 22, back)
            }
            Axis::X => ((lane.delta.abs() >> 9) * self.reciprocal, 13, 0),
        };
        let (offset, delta) = if rising {
            (0, delta)
        } else {
            (delta * self.length, -delta)
        };
        DepthLane {
            from: low,
            offset,
            delta,
            down,
            back,
        }
    }

    #[inline(always)]
    pub fn value(&self, lane: &Lane) -> i32 {
        let value = if self.linear {
            if lane.delta == 0 {
                return lane.from as i32;
            }
            if lane.delta > 0 {
                lane.from + lane.delta * self.position / self.divisor
            } else {
                lane.from + lane.delta - lane.delta * (self.length - self.position) / self.divisor
            }
        } else {
            lane.from + ((lane.delta * self.factor) >> self.shift)
        };
        value as i32
    }

    #[inline(always)]
    pub fn depth_value(&self, lane: &DepthLane) -> i32 {
        let along = if self.w_buffer {
            self.factor
        } else {
            self.position
        };
        (lane.from + (((lane.offset + lane.delta * along) >> lane.down) << lane.back)) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Interpolator {
        fn attribute(&self, a: i32, b: i32) -> i32 {
            self.value(&self.lane(a, b))
        }

        fn depth(&self, a: i32, b: i32) -> i32 {
            self.depth_value(&self.depth_lane(a, b))
        }
    }

    fn at(axis: Axis, span: [i32; 2], w: [i32; 2], w_buffer: bool, x: i32) -> Interpolator {
        let mut interpolator = Interpolator::new(axis, span, w, w_buffer);
        interpolator.set(x);
        interpolator
    }

    #[test]
    fn equal_w_interpolates_linearly_like_melonds() {
        let middle = at(Axis::X, [0, 100], [0x1000, 0x1000], false, 25);
        assert_eq!(middle.attribute(0, 400), 100);
        assert_eq!(middle.attribute(400, 0), 300);
    }

    #[test]
    fn equal_w_with_low_bits_set_goes_through_the_perspective_path_like_melonds() {
        let x = at(Axis::X, [0, 100], [0x1001, 0x1001], false, 50);
        assert_eq!(x.attribute(0, 512), 256);
        let y = at(Axis::Y, [0, 100], [0x1001, 0x1001], false, 50);
        assert_eq!(y.attribute(0, 512), 256);
    }

    #[test]
    fn a_far_end_pulls_the_middle_towards_the_near_end_like_melonds() {
        let middle = at(Axis::X, [0, 100], [0x1000, 0x4000], false, 50);
        let value = middle.attribute(0, 1000);
        assert_eq!(value, 199);
        assert!(value < 500);
    }

    #[test]
    fn interpolation_meets_both_ends() {
        let start = at(Axis::Y, [10, 60], [0x1000, 0x3000], false, 10);
        assert_eq!(start.attribute(30, 90), 30);
        let end = at(Axis::Y, [10, 60], [0x1000, 0x3000], false, 60);
        assert_eq!(end.attribute(30, 90), 90);
    }

    #[test]
    fn z_buffer_depth_is_linear_in_screen_space_like_melonds() {
        let middle = at(Axis::X, [0, 128], [0x1000, 0x8000], false, 64);
        assert_eq!(middle.depth(0, 0x80_0000), 0x40_0000);
        let along_y = at(Axis::Y, [0, 128], [0x1000, 0x8000], false, 64);
        assert_eq!(along_y.depth(0, 0x80_0000), 0x40_0000);
    }

    #[test]
    fn w_buffer_depth_follows_the_perspective_factor_like_melonds() {
        let middle = at(Axis::X, [0, 100], [0x1000, 0x4000], true, 50);
        assert_eq!(
            middle.depth(0x1000, 0x4000),
            middle.attribute(0x1000, 0x4000)
        );
        assert!(middle.depth(0x1000, 0x4000) < 0x2800);
    }

    #[test]
    fn a_zero_length_span_keeps_the_first_value() {
        let point = at(Axis::X, [5, 5], [0x1000, 0x2000], false, 5);
        assert_eq!(point.attribute(7, 9), 7);
        assert_eq!(point.depth(7, 9), 7);
    }
}
