use super::input::CablePace;
use citrine_gb_threads::{Split, Turnout};

const LATE_SHARE: f64 = 0.5;
const CAUGHT_UP_SHARE: f64 = 0.25;
const LATE_FRAMES: u32 = 30;
const PROBE_EVERY: u32 = 30;
const CAUGHT_UP_PROBES: u32 = 3;
const PROBE_ROUNDS: u32 = 64;
const PROBE_SPIN: u32 = 2_000;

pub(crate) struct Pacer {
    wish: CablePace,
    late_frames: u32,
    since_probe: u32,
    caught_up: u32,
    asked_at: Option<u32>,
}

impl Default for Pacer {
    fn default() -> Self {
        Self {
            wish: CablePace::Windows,
            late_frames: 0,
            since_probe: 0,
            caught_up: 0,
            asked_at: None,
        }
    }
}

pub(crate) fn missing_share(turnout: Turnout) -> f64 {
    let slots = turnout.rounds * turnout.lanes;
    if slots == 0 || turnout.lanes < 2 {
        return 0.0;
    }
    turnout.missing() as f64 / slots as f64
}

impl Pacer {
    pub(crate) fn wish(&self) -> CablePace {
        self.wish
    }

    pub(crate) fn asked(&mut self, frame: u32) {
        self.asked_at = Some(frame);
    }

    pub(crate) fn waiting_on(&self, frame: u32) -> bool {
        self.asked_at.is_some_and(|at| at >= frame)
    }

    pub(crate) fn windows_ran(&mut self, turnout: Turnout) {
        self.since_probe = 0;
        self.caught_up = 0;
        if missing_share(turnout) > LATE_SHARE {
            self.late_frames += 1;
            if self.late_frames >= LATE_FRAMES {
                self.wish = CablePace::Exact;
            }
        } else {
            self.late_frames = 0;
            self.wish = CablePace::Windows;
        }
    }

    pub(crate) fn exact_ran(&mut self, lanes: usize, split: &mut Split<'_>) {
        self.late_frames = 0;
        self.since_probe += 1;
        if self.since_probe < PROBE_EVERY {
            return;
        }
        self.since_probe = 0;
        if missing_share(probe(lanes, split)) > CAUGHT_UP_SHARE {
            self.caught_up = 0;
        } else {
            self.caught_up += 1;
            if self.caught_up >= CAUGHT_UP_PROBES {
                self.wish = CablePace::Windows;
            }
        }
    }
}

pub(crate) fn probe(lanes: usize, split: &mut Split<'_>) -> Turnout {
    let mut items = vec![0u64; lanes];
    let mut rounds = 0;
    split.rounds(
        "link speed check",
        &mut items,
        |sum| {
            for i in 0..PROBE_SPIN {
                *sum = std::hint::black_box(sum.wrapping_mul(31).wrapping_add(u64::from(i)));
            }
        },
        |_| {
            rounds += 1;
            rounds < PROBE_ROUNDS
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn late() -> Turnout {
        Turnout {
            rounds: 100,
            joined: 100,
            lanes: 4,
        }
    }

    fn on_time() -> Turnout {
        Turnout {
            rounds: 100,
            joined: 390,
            lanes: 4,
        }
    }

    #[test]
    fn a_share_of_missing_lanes_counts_only_with_two_lanes_or_more() {
        assert_eq!(missing_share(late()), 0.75);
        assert_eq!(missing_share(Turnout::default()), 0.0);
        let one = Turnout {
            rounds: 10,
            joined: 10,
            lanes: 1,
        };
        assert_eq!(missing_share(one), 0.0);
    }

    #[test]
    fn late_lanes_ask_for_exact_only_after_a_run_of_late_frames() {
        let mut pacer = Pacer::default();
        for _ in 1..LATE_FRAMES {
            pacer.windows_ran(late());
            assert_eq!(pacer.wish(), CablePace::Windows);
        }
        pacer.windows_ran(on_time());
        for _ in 1..LATE_FRAMES {
            pacer.windows_ran(late());
        }
        assert_eq!(pacer.wish(), CablePace::Windows);
        pacer.windows_ran(late());
        assert_eq!(pacer.wish(), CablePace::Exact);
    }

    #[test]
    fn exact_frames_probe_the_lanes_and_ask_for_windows_once_they_caught_up() {
        let mut pacer = Pacer::default();
        for _ in 0..LATE_FRAMES {
            pacer.windows_ran(late());
        }
        let starved = &mut Split::starved();
        for _ in 0..PROBE_EVERY * 5 {
            pacer.exact_ran(4, starved);
        }
        assert_eq!(pacer.wish(), CablePace::Exact);
        let free = &mut Split::inline();
        for _ in 0..PROBE_EVERY * CAUGHT_UP_PROBES - 1 {
            pacer.exact_ran(4, free);
        }
        assert_eq!(pacer.wish(), CablePace::Exact);
        pacer.exact_ran(4, free);
        assert_eq!(pacer.wish(), CablePace::Windows);
    }

    #[test]
    fn a_frame_asked_for_is_waited_on_until_it_runs() {
        let mut pacer = Pacer::default();
        assert!(!pacer.waiting_on(10));
        pacer.asked(12);
        assert!(pacer.waiting_on(12));
        assert!(!pacer.waiting_on(13));
    }
}
