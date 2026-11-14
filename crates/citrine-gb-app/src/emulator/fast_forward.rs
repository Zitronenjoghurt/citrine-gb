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

    pub(crate) fn frames(self, due: usize) -> usize {
        self.times().map_or(usize::MAX, |times| due * times)
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

pub(crate) fn budget(display_frame_secs: f64) -> Duration {
    Duration::from_secs_f64(
        display_frame_secs.clamp(0.0, LONGEST_DISPLAY_FRAME_SECS) * SHARE_OF_DISPLAY_FRAME,
    )
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Spending {
    budget: Option<Duration>,
    started: web_time::Instant,
    last_frame: Duration,
}

impl Spending {
    pub(crate) fn start(budget: Option<Duration>) -> Self {
        Self {
            budget,
            started: web_time::Instant::now(),
            last_frame: Duration::ZERO,
        }
    }

    pub(crate) fn next_fits(&self) -> bool {
        self.budget
            .is_none_or(|budget| self.started.elapsed() + self.last_frame <= budget)
    }

    pub(crate) fn frame_took(&mut self, spent: Duration) {
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
}
