use super::{Budget, FrameSample};
use std::collections::VecDeque;
use std::time::Duration;

pub const FPS_WINDOW: Duration = Duration::from_secs(2);
pub const WINDOW: Duration = Duration::from_secs(5);
pub const EVERY: Duration = Duration::from_secs(1);
pub const STRIP: usize = 60;
pub const LOW_SHARE: f64 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripFrame {
    pub shown: Duration,
    pub repeated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Readout {
    pub at: Duration,
    pub fps: f64,
    pub rate: f64,
    pub late: u32,
    pub game: Option<Duration>,
    pub app: Duration,
    pub repeated: f64,
    pub repeated_late: f64,
    pub dropped: f64,
    pub low: Option<f64>,
    pub refresh: Option<f64>,
    pub audio_queued: Option<Duration>,
    pub audio_underrun: Duration,
}

pub fn refreshes_per_frame(budget: Budget) -> Option<f64> {
    let refresh = budget.refresh?.as_secs_f64();
    let ratio = budget.game.as_secs_f64() / refresh;
    Some((ratio - 0.05).ceil().max(1.0))
}

pub fn rate_gap(budget: Budget) -> f64 {
    let (Some(fits), Some(refresh)) = (refreshes_per_frame(budget), budget.refresh) else {
        return 0.0;
    };
    let rate = 1.0 / budget.game.as_secs_f64();
    (1.0 / refresh.as_secs_f64() - fits * rate).max(0.0)
}

fn game_frames(samples: &VecDeque<FrameSample>) -> impl Iterator<Item = (Duration, Budget)> {
    let mut shown = Duration::ZERO;
    let mut first = true;
    samples.iter().filter_map(move |sample| {
        shown += sample.display;
        if sample.game_frames == 0 {
            return None;
        }
        let frame = (!first).then_some((shown, sample.budget));
        first = false;
        shown = Duration::ZERO;
        frame
    })
}

pub fn strip(samples: &VecDeque<FrameSample>) -> Vec<StripFrame> {
    let mut frames = VecDeque::with_capacity(STRIP + 1);
    for (shown, budget) in game_frames(samples) {
        let normal = refreshes_per_frame(budget)
            .zip(budget.refresh)
            .map(|(fits, refresh)| refresh.mul_f64(fits + 0.5));
        frames.push_back(StripFrame {
            shown,
            repeated: normal.is_some_and(|normal| shown > normal),
        });
        if frames.len() > STRIP {
            frames.pop_front();
        }
    }
    frames.into()
}

pub fn low(samples: &VecDeque<FrameSample>) -> Option<f64> {
    let mut shown: Vec<Duration> = game_frames(samples).map(|(shown, _)| shown).collect();
    if shown.is_empty() {
        return None;
    }
    shown.sort_unstable_by(|a, b| b.cmp(a));
    let slowest = ((shown.len() as f64 * LOW_SHARE).ceil() as usize).max(1);
    let mean = shown[..slowest].iter().sum::<Duration>() / slowest as u32;
    (!mean.is_zero()).then(|| 1.0 / mean.as_secs_f64())
}

pub fn readout(samples: &VecDeque<FrameSample>) -> Option<Readout> {
    let newest = samples.back()?;
    let within = |window: Duration| {
        samples
            .iter()
            .filter(move |sample| newest.at.saturating_sub(sample.at) < window)
    };
    let span = |window: Duration| {
        let oldest = within(window).next().map_or(newest.at, |sample| sample.at);
        newest.at.saturating_sub(oldest).as_secs_f64()
    };
    let per_second = |count: u64, window: Duration| {
        let span = span(window);
        if span > 0.0 { count as f64 / span } else { 0.0 }
    };
    let budget = newest.budget;
    let rate = 1.0 / budget.game.as_secs_f64();
    let fps = per_second(
        within(FPS_WINDOW).map(|sample| sample.game_frames).sum(),
        FPS_WINDOW,
    );
    let game_frames: u64 = within(WINDOW).map(|sample| sample.game_frames).sum();
    let game = u32::try_from(game_frames)
        .ok()
        .filter(|frames| *frames > 0)
        .map(|frames| within(WINDOW).map(|sample| sample.game).sum::<Duration>() / frames);
    let updates = u32::try_from(within(WINDOW).count())
        .unwrap_or(u32::MAX)
        .max(1);
    let app = within(WINDOW).map(|sample| sample.ui).sum::<Duration>() / updates;
    let repeated_count: u64 = within(WINDOW).map(|sample| sample.repeated).sum();
    let dropped_count: u64 = within(WINDOW).map(|sample| sample.dropped).sum();
    let gap = rate_gap(budget);
    let gap_count = (gap * span(WINDOW)).round() as u64;
    let late_repeats = repeated_count.saturating_sub(gap_count);
    let repeated = per_second(repeated_count, WINDOW);
    Some(Readout {
        at: newest.at,
        fps: fps.min(rate),
        rate,
        late: u32::try_from(late_repeats + dropped_count).unwrap_or(u32::MAX),
        game,
        app,
        repeated,
        repeated_late: (repeated - gap).max(0.0),
        dropped: per_second(dropped_count, WINDOW),
        low: low(samples).map(|low| low.min(rate)),
        refresh: budget.refresh.map(|refresh| 1.0 / refresh.as_secs_f64()),
        audio_queued: newest.audio.queued,
        audio_underrun: within(WINDOW).map(|sample| sample.audio.underrun).sum(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_log::AudioSample;

    const GAME: Duration = Duration::from_nanos(16_742_706);
    const REFRESH: Duration = Duration::from_nanos(5_555_556);

    fn budget() -> Budget {
        Budget {
            game: GAME,
            refresh: Some(REFRESH),
        }
    }

    fn sample(at: Duration, game_frames: u64, repeated: u64) -> FrameSample {
        FrameSample {
            at,
            display: REFRESH,
            ui: Duration::from_millis(1),
            game: Duration::from_millis(3) * game_frames as u32,
            game_frames,
            repeated,
            dropped: 0,
            gpu: None,
            gpu_frames: 0,
            gpu_full: None,
            gpu_passes: None,
            jit_compile: Duration::ZERO,
            jit_blocks: 0,
            console_wait: Duration::ZERO,
            audio: AudioSample {
                queued: Some(Duration::from_millis(60)),
                underrun: Duration::ZERO,
                overrun: Duration::ZERO,
            },
            budget: budget(),
        }
    }

    fn run(seconds: u32, extra_repeats_every: Option<u32>) -> VecDeque<FrameSample> {
        let mut samples = VecDeque::new();
        let mut owed = 0.0;
        let mut frame = 0u32;
        let mut wait = 3;
        for refresh in 0..seconds * 180 {
            let at = REFRESH * (refresh + 1);
            wait -= 1;
            if wait > 0 {
                samples.push_back(sample(at, 0, 0));
                continue;
            }
            frame += 1;
            owed += rate_gap(budget()) / (1.0 / GAME.as_secs_f64());
            let mut held: u32 = 0;
            if owed >= 1.0 {
                owed -= 1.0;
                held = 1;
            }
            if extra_repeats_every.is_some_and(|every| frame.is_multiple_of(every)) {
                held += 1;
            }
            wait = 3 + held;
            samples.push_back(sample(at, 1, held.into()));
        }
        samples
    }

    #[test]
    fn a_59_8_hz_game_on_a_180_hz_screen_repeats_from_the_rate_gap_alone() {
        assert_eq!(refreshes_per_frame(budget()), Some(3.0));
        let gap = rate_gap(budget());
        assert!((gap - 0.82).abs() < 0.02, "{gap}");
        let readout = readout(&run(10, None)).unwrap();
        assert!((readout.rate - 59.73).abs() < 0.01, "{}", readout.rate);
        assert!(
            readout.fps > 59.0 && readout.fps <= readout.rate,
            "{}",
            readout.fps
        );
        assert!((readout.repeated - gap).abs() < 0.5, "{}", readout.repeated);
        assert_eq!(readout.late, 0);
        assert_eq!(readout.game, Some(Duration::from_millis(3)));
        assert_eq!(readout.app, Duration::from_millis(1));
        assert_eq!(readout.audio_queued, Some(Duration::from_millis(60)));
        assert_eq!(readout.audio_underrun, Duration::ZERO);
        assert_eq!(readout.dropped, 0.0);
        let refresh = readout.refresh.unwrap();
        assert!((refresh - 180.0).abs() < 0.01, "{refresh}");
        let low = readout.low.unwrap();
        assert!((low - 45.0).abs() < 0.1, "{low}");
    }

    #[test]
    fn repeats_beyond_the_rate_gap_count_as_late() {
        let readout = readout(&run(10, Some(30))).unwrap();
        assert!(readout.late >= 8, "{}", readout.late);
        assert!(readout.repeated_late > 1.5, "{}", readout.repeated_late);
    }

    #[test]
    fn the_low_is_the_fps_of_the_slowest_one_percent_of_game_frames() {
        let mut samples = run(10, None);
        let stalls: Vec<usize> = samples
            .iter()
            .enumerate()
            .filter(|(_, sample)| sample.game_frames == 0)
            .map(|(index, _)| index)
            .skip(10)
            .step_by(150)
            .take(8)
            .collect();
        assert_eq!(stalls.len(), 8);
        for index in stalls {
            samples[index].display += Duration::from_millis(50);
        }
        let low = readout(&samples).unwrap().low.unwrap();
        assert!(low > 13.5 && low < 15.1, "{low}");
    }

    #[test]
    fn the_strip_holds_the_last_60_game_frames_and_marks_the_longer_ones() {
        let strip = strip(&run(3, Some(30)));
        assert_eq!(strip.len(), STRIP);
        assert!(strip.iter().any(|frame| frame.repeated));
        assert!(strip.iter().any(|frame| !frame.repeated));
        for frame in &strip {
            assert_eq!(frame.repeated, frame.shown > REFRESH * 3 + REFRESH / 2);
        }
    }

    #[test]
    fn the_log_works_the_numbers_out_once_a_second() {
        let mut log = crate::frame_log::FrameLog {
            samples: run(3, None),
            ..Default::default()
        };
        let first = log.readout().unwrap();
        let newest = log.samples.back().unwrap().at;
        log.samples.push_back(sample(newest + REFRESH, 1, 5));
        let held = log.readout().unwrap();
        assert_eq!(held.at, first.at);
        assert_eq!(held.repeated, first.repeated);
        log.samples.push_back(sample(newest + EVERY, 1, 0));
        let fresh = log.readout().unwrap();
        assert_eq!(fresh.at, newest + EVERY);
        assert!(fresh.repeated > first.repeated);
    }

    #[test]
    fn no_samples_no_readout() {
        assert_eq!(readout(&VecDeque::new()), None);
        assert_eq!(low(&VecDeque::new()), None);
    }
}
