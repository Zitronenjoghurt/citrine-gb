use crate::emulator::gimmicks::Motion;

const FULL_TURN: f32 = 3.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Level {
    rest: Option<[f32; 3]>,
}

impl Level {
    pub fn centre(&mut self) {
        self.rest = None;
    }

    #[cfg(test)]
    pub fn is_set(&self) -> bool {
        self.rest.is_some()
    }

    pub fn motion(&mut self, gravity: [f32; 3], gyro: [f32; 3]) -> Option<Motion> {
        let now = normalized(gravity)?;
        let rest = *self.rest.get_or_insert(now);
        let side = normalized(minus(
            [1.0, 0.0, 0.0],
            scaled(rest, dot([1.0, 0.0, 0.0], rest)),
        ))?;
        let forward = cross(side, rest);
        Some(
            Motion {
                x: -dot(now, side),
                y: -dot(now, forward),
                turn: -gyro[2] / FULL_TURN,
            }
            .clamped(),
        )
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn minus(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scaled(a: [f32; 3], by: f32) -> [f32; 3] {
    [a[0] * by, a[1] * by, a[2] * by]
}

fn normalized(a: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(a, a).sqrt();
    (length.is_finite() && length > 0.1).then(|| scaled(a, 1.0 / length))
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: f32 = 9.81;

    fn rolled(degrees: f32) -> [f32; 3] {
        let r = degrees.to_radians();
        [-G * r.sin(), G * r.cos(), 0.0]
    }

    #[test]
    fn holding_still_reads_level() {
        let mut level = Level::default();
        let motion = level.motion([0.0, G, 0.0], [0.0; 3]).expect("motion");
        assert!(motion.x.abs() < 1e-5 && motion.y.abs() < 1e-5);
        assert!(level.is_set());
    }

    #[test]
    fn rolling_right_tilts_right_from_any_rest() {
        for rest in [[0.0, G, 0.0], [0.0, 0.0, G], [0.0, G * 0.7, G * 0.7]] {
            let mut level = Level::default();
            level.motion(rest, [0.0; 3]);
            let right = [-G * 0.5, rest[1], rest[2]];
            let motion = level.motion(right, [0.0; 3]).expect("motion");
            assert!(motion.x > 0.3, "{rest:?} {motion:?}");
            assert!(motion.y.abs() < 0.2, "{rest:?} {motion:?}");
        }
        let mut level = Level::default();
        level.motion(rolled(0.0), [0.0; 3]);
        let motion = level.motion(rolled(-30.0), [0.0; 3]).expect("motion");
        assert!(motion.x < -0.4);
    }

    #[test]
    fn tipping_the_top_away_tilts_up() {
        let mut upright = Level::default();
        upright.motion([0.0, G, 0.0], [0.0; 3]);
        let away = upright
            .motion([0.0, G * 0.8, G * 0.6], [0.0; 3])
            .expect("motion");
        assert!(away.y < -0.5, "{away:?}");
        let mut flat = Level::default();
        flat.motion([0.0, 0.0, G], [0.0; 3]);
        let away = flat
            .motion([0.0, -G * 0.6, G * 0.8], [0.0; 3])
            .expect("motion");
        assert!(away.y < -0.5, "{away:?}");
    }

    #[test]
    fn centring_takes_the_next_reading_as_level() {
        let mut level = Level::default();
        level.motion([0.0, G, 0.0], [0.0; 3]);
        level.centre();
        assert!(!level.is_set());
        let motion = level.motion([0.0, 0.0, G], [0.0; 3]).expect("motion");
        assert!(motion.y.abs() < 1e-5);
    }

    #[test]
    fn turning_clockwise_reads_positive_and_bad_readings_none() {
        let mut level = Level::default();
        let motion = level
            .motion([0.0, G, 0.0], [0.0, 0.0, -1.5])
            .expect("motion");
        assert!((motion.turn - 0.5).abs() < 1e-5);
        assert!(level.motion([0.0; 3], [0.0; 3]).is_none());
        assert!(level.motion([f32::NAN, 0.0, 0.0], [0.0; 3]).is_none());
    }
}
