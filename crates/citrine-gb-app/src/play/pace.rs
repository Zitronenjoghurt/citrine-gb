use citrine_gb::console::Timing;

const MOST_FRAMES: f64 = 4.0;

pub(crate) fn frame_ms(timing: Timing) -> f64 {
    timing.frame_secs() * 1000.0
}

pub(crate) fn frame_us(timing: Timing) -> u32 {
    (timing.frame_secs() * 1_000_000.0).round() as u32
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Pace {
    due: f64,
}

impl Pace {
    pub(crate) fn frames(&mut self, secs: f64, timing: Timing) -> u32 {
        self.due = (self.due + secs / timing.frame_secs()).min(MOST_FRAMES);
        let whole = self.due.floor();
        self.due -= whole;
        whole as u32
    }
}

#[cfg(test)]
mod tests {
    use super::{Pace, frame_ms, frame_us};
    use citrine_gb::console::{ConsoleKind, Timing};

    const TEST_CONSOLE: Timing = Timing {
        clock_hz: 33_513_982,
        cycles_per_frame: 263 * 355 * 6,
    };

    fn frames_in_a_minute(timing: Timing, step_ms: u32) -> u32 {
        let mut pace = Pace::default();
        let steps = 60_000 / step_ms;
        (0..steps)
            .map(|_| pace.frames(f64::from(step_ms) / 1000.0, timing))
            .sum()
    }

    #[test]
    fn the_test_console_runs_at_59_83_frames_a_second() {
        assert!((1.0 / TEST_CONSOLE.frame_secs() - 59.826).abs() < 0.001);
        assert_eq!(frame_us(TEST_CONSOLE), 16_715);
        assert_eq!(frames_in_a_minute(TEST_CONSOLE, 10), 3589);
        assert_eq!(frames_in_a_minute(TEST_CONSOLE, 25), 3589);
    }

    #[test]
    fn game_boy_and_gba_keep_their_frame_length() {
        for kind in [ConsoleKind::Gb, ConsoleKind::Gbc, ConsoleKind::Gba] {
            let timing = kind.timing();
            assert_eq!(frame_ms(timing), 70_224.0 * 1000.0 / 4_194_304.0);
            assert_eq!(frame_us(timing), 16_743);
            assert_eq!(frames_in_a_minute(timing, 10), 3583);
        }
    }

    #[test]
    fn a_long_stall_runs_at_most_four_frames() {
        let mut pace = Pace::default();
        assert_eq!(pace.frames(2.0, TEST_CONSOLE), 4);
        assert_eq!(pace.frames(0.010, TEST_CONSOLE), 0);
    }
}
