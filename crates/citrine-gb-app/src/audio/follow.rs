use std::collections::VecDeque;

const WINDOW_SECS: f64 = 2.0;
const SHORT_SECS: f64 = 0.5;
const MIN_SECS: f64 = 0.5;
const GAP_SECS: f64 = 0.25;
const HITCH_SECS: f64 = 0.1;
const SLACK_SECS: f64 = 0.03;
const ENTER: f64 = 0.99;
const LEAVE: f64 = 0.995;
const JUMP: f64 = 0.1;
const SLOWEST: f64 = 0.5;

#[derive(Debug, Default, Clone)]
struct Window {
    steps: VecDeque<(f64, f64)>,
    wall: f64,
    made: f64,
}

impl Window {
    fn add(&mut self, wall: f64, made: f64, span: f64) {
        self.steps.push_back((wall, made));
        self.wall += wall;
        self.made += made;
        while let Some(&(wall, made)) = self.steps.front() {
            if self.wall - wall < span {
                break;
            }
            self.steps.pop_front();
            self.wall -= wall;
            self.made -= made;
        }
    }

    fn speed(&self) -> f64 {
        self.made / self.wall.max(f64::EPSILON)
    }

    fn surely_below(&self, speed: f64) -> bool {
        (self.made + SLACK_SECS) / self.wall.max(f64::EPSILON) < speed
    }
}

#[derive(Debug, Default, Clone)]
pub struct SpeedFollow {
    long: Window,
    short: Window,
    slow: bool,
}

impl SpeedFollow {
    pub fn update(&mut self, wall_secs: f64, made_secs: f64) -> Option<f64> {
        let hitch = wall_secs > HITCH_SECS && made_secs < wall_secs * 0.5;
        if wall_secs <= GAP_SECS && !hitch {
            let wall = wall_secs.max(0.0);
            self.long.add(wall, made_secs, WINDOW_SECS);
            self.short.add(wall, made_secs, SHORT_SECS);
        }
        if self.long.wall < MIN_SECS {
            return self.speed();
        }
        let jumped = self.jumped();
        if !self.slow {
            self.slow = self.long.surely_below(ENTER) || (jumped && self.short.surely_below(ENTER));
        } else if self.estimate() >= LEAVE {
            self.slow = false;
        }
        self.speed()
    }

    pub fn speed(&self) -> Option<f64> {
        self.slow.then(|| self.estimate().clamp(SLOWEST, 1.0))
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn jumped(&self) -> bool {
        self.short.wall >= SHORT_SECS * 0.9 && (self.short.speed() - self.long.speed()).abs() > JUMP
    }

    fn estimate(&self) -> f64 {
        if self.jumped() {
            self.short.speed()
        } else {
            self.long.speed()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f64 = 1.0 / 60.0;

    fn run(follow: &mut SpeedFollow, speed: f64, secs: f64) -> Vec<Option<f64>> {
        let mut seen = Vec::new();
        let mut owed = 0.0;
        for _ in 0..(secs / (4.0 * FRAME)) as usize {
            owed += 4.0 * FRAME * speed;
            let frames = (owed / FRAME).floor();
            owed -= frames * FRAME;
            seen.push(follow.update(4.0 * FRAME, frames * FRAME));
        }
        seen
    }

    #[test]
    fn full_speed_never_counts_as_slow() {
        let mut follow = SpeedFollow::default();
        assert!(run(&mut follow, 1.0, 60.0).iter().all(Option::is_none));
        let mut lumpy = SpeedFollow::default();
        for i in 0..3000 {
            let frames = [0.0, 2.0, 1.0, 0.0, 3.0][i % 5];
            assert_eq!(lumpy.update(1.2 * FRAME, frames * FRAME), None);
        }
    }

    #[test]
    fn slow_consoles_report_their_speed() {
        for speed in [0.95, 0.85, 0.7, 0.6] {
            let mut follow = SpeedFollow::default();
            let seen = run(&mut follow, speed, 10.0);
            let last = seen.last().copied().flatten().expect("slow");
            assert!((last - speed).abs() < 0.01, "{speed} read as {last}");
        }
    }

    #[test]
    fn a_stall_is_not_slowness() {
        let mut follow = SpeedFollow::default();
        run(&mut follow, 1.0, 5.0);
        assert_eq!(follow.update(0.6, FRAME), None);
        assert!(run(&mut follow, 1.0, 5.0).iter().all(Option::is_none));
    }

    #[test]
    fn a_single_hitch_is_not_slowness() {
        let mut follow = SpeedFollow::default();
        run(&mut follow, 1.0, 5.0);
        assert_eq!(follow.update(0.2, FRAME), None);
        assert!(run(&mut follow, 1.0, 5.0).iter().all(Option::is_none));
    }

    #[test]
    fn back_to_full_speed_lets_go() {
        let mut follow = SpeedFollow::default();
        run(&mut follow, 0.7, 5.0);
        assert!(follow.speed().is_some());
        let seen = run(&mut follow, 1.0, 6.0);
        assert_eq!(seen.last().copied().flatten(), None);
    }
}
