use super::queue::QueueStats;
#[cfg(not(target_arch = "wasm32"))]
use super::queue::QueueWriter;
use super::resampler::Resampler;
#[cfg(target_arch = "wasm32")]
use super::web::PortWriter as QueueWriter;
use std::sync::Arc;

const FRAME_TIME: f64 = 1.0 / 59.7275;
const MAX_FRAMES_PER_UPDATE: usize = 4;
const MAX_BEHIND_SECS: f64 = 0.25;

const MAX_ADJUST: f64 = 0.005;
const PROPORTIONAL: f64 = MAX_ADJUST / 0.04;
const INTEGRAL: f64 = PROPORTIONAL * PROPORTIONAL / 4.0;
const SMOOTHING_SECS: f64 = 0.5;

#[derive(Debug, Default, Clone)]
pub struct FrameClock {
    behind: f64,
}

impl FrameClock {
    pub fn advance(&mut self, dt: f64) -> usize {
        self.behind += dt.max(0.0);
        if self.behind > MAX_BEHIND_SECS {
            self.behind = FRAME_TIME;
        }
        let due = ((self.behind / FRAME_TIME) as usize).min(MAX_FRAMES_PER_UPDATE);
        self.behind -= due as f64 * FRAME_TIME;
        due
    }
}

#[derive(Debug, Default, Clone)]
pub struct RateControl {
    target_secs: f64,
    error: Option<f64>,
    integral: f64,
    adjust: f64,
}

impl RateControl {
    pub fn new(target_secs: f64) -> Self {
        Self {
            target_secs,
            ..Self::default()
        }
    }

    pub fn update(&mut self, queued_secs: f64, dt: f64, playing: bool) -> f64 {
        let raw = queued_secs - self.target_secs;
        let error = match self.error {
            Some(prev) => prev + (raw - prev) * (dt / SMOOTHING_SECS).min(1.0),
            None => raw,
        };
        self.error = Some(error);

        if !playing {
            self.adjust = 0.0;
            return self.adjust;
        }

        let limit = MAX_ADJUST / INTEGRAL;
        self.integral = (self.integral + error * dt).clamp(-limit, limit);
        self.adjust =
            (-(PROPORTIONAL * error + INTEGRAL * self.integral)).clamp(-MAX_ADJUST, MAX_ADJUST);
        self.adjust
    }

    pub fn adjust(&self) -> f64 {
        self.adjust
    }
}

pub struct AudioFeed {
    writer: QueueWriter,
    resampler: Resampler,
    rate: RateControl,
    sample_rate: u32,
    target_frames: usize,
    scratch: Vec<f32>,
}

impl AudioFeed {
    pub fn new(writer: QueueWriter, sample_rate: u32, target_frames: usize) -> Self {
        Self {
            writer,
            resampler: Resampler::new(),
            rate: RateControl::new(target_frames as f64 / sample_rate.max(1) as f64),
            sample_rate,
            target_frames,
            scratch: Vec::new(),
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        let frames = samples.len() / 2;
        if frames == 0 {
            return;
        }
        let rate = self.sample_rate.max(1) as f64;
        let queued = self.writer.queued_frames() as f64
            + self.resampler.pending_frames()
            + frames as f64 * 0.5;
        let adjust = self.rate.update(
            queued / rate,
            frames as f64 / rate,
            self.writer.is_playing(),
        );

        self.scratch.clear();
        self.resampler
            .process(samples, 1.0 / (1.0 + adjust), &mut self.scratch);
        self.writer.write(&self.scratch);
    }

    pub fn set_paused(&self, paused: bool) {
        self.writer.set_paused(paused);
    }

    pub fn queued_frames(&self) -> usize {
        self.writer.queued_frames()
    }

    pub fn target_frames(&self) -> usize {
        self.target_frames
    }

    pub fn rate_adjust(&self) -> f64 {
        self.rate.adjust()
    }

    pub fn stats(&self) -> &Arc<QueueStats> {
        self.writer.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::queue::{QueueConfig, audio_queue, latency_frames};

    #[test]
    fn frame_clock_runs_what_is_due() {
        let mut clock = FrameClock::default();
        assert_eq!(clock.advance(FRAME_TIME * 0.5), 0);
        assert_eq!(clock.advance(FRAME_TIME * 0.6), 1);
        assert_eq!(clock.advance(FRAME_TIME * 2.0), 2);
    }

    #[test]
    fn frame_clock_catches_up_over_several_updates() {
        let mut clock = FrameClock::default();
        let behind = FRAME_TIME * 7.2;
        let first = clock.advance(behind);
        let second = clock.advance(0.0);
        assert_eq!(first, MAX_FRAMES_PER_UPDATE);
        assert_eq!(first + second, 7);
    }

    #[test]
    fn frame_clock_drops_a_long_stall() {
        let mut clock = FrameClock::default();
        assert_eq!(clock.advance(2.0), 1);
        assert_eq!(clock.advance(0.0), 0);
    }

    #[test]
    fn rate_control_slows_a_full_queue_and_speeds_an_empty_one() {
        let mut full = RateControl::new(0.08);
        let mut empty = RateControl::new(0.08);
        for _ in 0..10 {
            full.update(0.12, 0.016, true);
            empty.update(0.04, 0.016, true);
        }
        assert!(full.adjust() < 0.0);
        assert!(empty.adjust() > 0.0);
        assert!(full.adjust().abs() <= MAX_ADJUST);
    }

    #[test]
    fn rate_control_holds_still_while_waiting() {
        let mut control = RateControl::new(0.08);
        for _ in 0..100 {
            assert_eq!(control.update(0.01, 0.016, false), 0.0);
        }
        assert_eq!(control.integral, 0.0);
    }

    #[test]
    fn rate_control_absorbs_clock_drift() {
        let mut control = RateControl::new(0.08);
        let mut queued = 0.08;
        let dt = 0.016;
        for _ in 0..(120.0 / dt) as usize {
            let adjust = control.update(queued, dt, true);
            queued += (adjust - 0.002) * dt;
        }
        assert!((queued - 0.08).abs() < 0.002, "queued {queued}");
        assert!((control.adjust() - 0.002).abs() < 0.0002);
    }

    #[test]
    fn feed_keeps_a_jittery_loop_fed() {
        let rate = 48_000;
        let config = QueueConfig::new(rate, latency_frames(rate, 80));
        let target = config.target_frames;
        let (writer, mut reader) = audio_queue(config);
        let mut feed = AudioFeed::new(writer, rate, target);
        let mut clock = FrameClock::default();

        let callback = 480;
        let period = callback as f64 / (rate as f64 * 1.0002);
        let mut out = vec![0.0f32; callback * 2];
        let mut next_callback = 0.0;
        let mut now = 0.0;
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut phase = 0.0f64;
        let mut tick = 0;

        while now < 20.0 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let jitter = ((seed >> 40) as f64 / (1u64 << 24) as f64 - 0.5) * 0.008;
            let dt = 1.0 / 60.0 + if tick % 2 == 0 { jitter } else { -jitter };
            tick += 1;
            now += dt;
            while next_callback <= now {
                reader.read(&mut out, 2, 1.0);
                next_callback += period;
            }
            let frames = clock.advance(dt);
            let produced = (frames as f64 * FRAME_TIME * rate as f64) as usize;
            let samples: Vec<f32> = (0..produced)
                .flat_map(|_| {
                    phase += 440.0 / rate as f64;
                    let v = (phase * std::f64::consts::TAU).sin() as f32 * 0.3;
                    [v, v]
                })
                .collect();
            feed.push(&samples);
        }

        let stats = feed.stats();
        assert_eq!(stats.underruns(), 0);
        assert_eq!(stats.overruns(), 0);
        let queued_ms = feed.queued_frames() as f64 * 1000.0 / rate as f64;
        assert!((40.0..=120.0).contains(&queued_ms), "queued {queued_ms} ms");
    }
}
