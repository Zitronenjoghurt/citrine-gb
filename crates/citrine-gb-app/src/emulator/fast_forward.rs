use std::fmt::Display;
use std::time::Duration;
use strum_macros::EnumIter;

const SHARE_OF_DISPLAY_FRAME: f64 = if cfg!(threaded) { 0.75 } else { 0.5 };
const LONGEST_DISPLAY_FRAME_SECS: f64 = 1.0 / 30.0;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum FastForwardSpeed {
    Two,
    #[default]
    Three,
    Five,
    Ten,
    Max,
}

impl FastForwardSpeed {
    fn times(self) -> Option<usize> {
        match self {
            Self::Two => Some(2),
            Self::Three => Some(3),
            Self::Five => Some(5),
            Self::Ten => Some(10),
            Self::Max => None,
        }
    }
}

impl Display for FastForwardSpeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.times() {
            Some(times) => write!(f, "{times}x"),
            None => f.write_str("Max"),
        }
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum FastForwardKey {
    #[default]
    Hold,
    Toggle,
}

impl Display for FastForwardKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Hold => "Hold",
            Self::Toggle => "Toggle",
        })
    }
}

pub(crate) fn budget(display_frame_secs: f64) -> Duration {
    Duration::from_secs_f64(
        display_frame_secs.clamp(0.0, LONGEST_DISPLAY_FRAME_SECS) * SHARE_OF_DISPLAY_FRAME,
    )
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct FastForwardClock {
    owed: f64,
}

impl FastForwardClock {
    pub(crate) fn due(&mut self, speed: FastForwardSpeed, dt: f64, frame_secs: f64) -> usize {
        let Some(times) = speed.times() else {
            return usize::MAX;
        };
        let dt = dt.clamp(0.0, LONGEST_DISPLAY_FRAME_SECS);
        self.owed += dt * times as f64 / frame_secs;
        let due = self.owed.floor();
        self.owed -= due;
        due as usize
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Spending {
    frames: usize,
    ran: usize,
    budget: Option<Duration>,
    spent: Duration,
    last_frame: Duration,
}

impl Spending {
    pub(crate) fn start(frames: usize, budget: Option<Duration>) -> Self {
        Self {
            frames,
            ran: 0,
            budget,
            spent: Duration::ZERO,
            last_frame: Duration::ZERO,
        }
    }

    pub(crate) fn next(&self) -> bool {
        self.ran < self.frames
            && (self.ran == 0
                || self
                    .budget
                    .is_none_or(|budget| self.spent + self.last_frame <= budget))
    }

    pub(crate) fn frame_took(&mut self, spent: Duration) {
        self.ran += 1;
        self.spent += spent;
        self.last_frame = spent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_read_as_clara_listed_them() {
        let names: Vec<String> = <FastForwardSpeed as strum::IntoEnumIterator>::iter()
            .map(|speed| speed.to_string())
            .collect();
        assert_eq!(names, ["2x", "3x", "5x", "10x", "Max"]);
    }

    const GBA_FRAME_SECS: f64 = 280_896.0 / 16_777_216.0;

    fn speed_reached(speed: FastForwardSpeed, hz: f64, frame: Duration) -> f64 {
        let refresh = 1.0 / hz;
        let refreshes = (hz * 10.0) as usize;
        let mut clock = FastForwardClock::default();
        let mut frames = 0;
        for _ in 0..refreshes {
            let due = clock.due(speed, refresh, GBA_FRAME_SECS);
            let mut spending = Spending::start(due, Some(budget(refresh)));
            while spending.next() {
                spending.frame_took(frame);
                frames += 1;
            }
        }
        frames as f64 * GBA_FRAME_SECS / (refreshes as f64 * refresh)
    }

    #[test]
    fn three_times_reaches_three_times_on_any_screen_when_frames_are_cheap() {
        let frame = Duration::from_micros(1500);
        for hz in [60.0, 120.0, 144.0, 180.0, 240.0] {
            let reached = speed_reached(FastForwardSpeed::Three, hz, frame);
            assert!((reached - 3.0).abs() < 0.01, "{hz} Hz: {reached:.2}x");
        }
    }

    #[test]
    fn every_fixed_speed_reaches_its_target_on_a_180_hz_screen() {
        for (speed, times) in [
            (FastForwardSpeed::Two, 2.0),
            (FastForwardSpeed::Three, 3.0),
            (FastForwardSpeed::Five, 5.0),
            (FastForwardSpeed::Ten, 10.0),
        ] {
            let reached = speed_reached(speed, 180.0, Duration::from_micros(1000));
            assert!((reached - times).abs() < 0.01, "{speed}: {reached:.2}x");
        }
    }

    #[test]
    fn max_spends_its_share_of_every_refresh() {
        let frame = Duration::from_micros(1000);
        let reached = speed_reached(FastForwardSpeed::Max, 180.0, frame);
        let share = SHARE_OF_DISPLAY_FRAME * GBA_FRAME_SECS / frame.as_secs_f64();
        assert!(reached > share * 0.9, "{reached:.2}x of {share:.2}x");
    }

    #[test]
    fn a_slow_console_runs_as_fast_as_its_budget_allows() {
        let frame = Duration::from_millis(6);
        let reached = speed_reached(FastForwardSpeed::Three, 60.0, frame);
        assert!(
            (reached - 2.0 * GBA_FRAME_SECS * 60.0).abs() < 0.01,
            "{reached:.2}x"
        );
    }
}
