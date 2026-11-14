use super::follow::SpeedFollow;
use super::queue::QueueStats;
#[cfg(not(target_arch = "wasm32"))]
use super::queue::QueueWriter;
use super::resampler::Resampler;
#[cfg(all(threaded, target_arch = "wasm32"))]
use super::shared::RingWriter as QueueWriter;
use super::stretch::Stretch;
#[cfg(all(not(threaded), target_arch = "wasm32"))]
use super::web::PortWriter as QueueWriter;
#[cfg(test)]
use citrine_gb::console::Timing;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_FRAMES_PER_UPDATE: usize = 4;
const MAX_BEHIND_SECS: f64 = 0.25;

const MAX_ADJUST: f64 = 0.005;
const PROPORTIONAL: f64 = MAX_ADJUST / 0.04;
const INTEGRAL: f64 = PROPORTIONAL * PROPORTIONAL / 4.0;
const SMOOTHING_SECS: f64 = 0.5;
const SLOW_SCALE: f64 = 12.0;
const SLOW_CUSHION_SECS: f64 = 0.05;
const CARRY_SHARE: usize = 20;

#[derive(Debug, Default, Clone)]
pub struct FrameClock {
    behind: f64,
    margin: f64,
}

impl FrameClock {
    #[cfg(test)]
    pub fn advance(&mut self, dt: f64, timing: Timing) -> usize {
        self.advance_by(dt, timing.frame_secs())
    }

    pub fn advance_by(&mut self, dt: f64, frame_secs: f64) -> usize {
        self.behind += dt.max(0.0);
        if self.behind > MAX_BEHIND_SECS {
            self.behind = frame_secs + self.margin;
        }
        let due = ((self.behind / frame_secs) as usize).min(MAX_FRAMES_PER_UPDATE);
        self.behind -= due as f64 * frame_secs;
        due
    }

    pub fn until_due(&self, frame_secs: f64) -> f64 {
        (frame_secs - self.behind).max(0.0)
    }

    pub fn center(&mut self, period: f64) {
        self.margin = period / 2.0;
        self.behind = (self.behind / period).floor() * period + self.margin;
    }

    pub fn uncenter(&mut self) {
        self.margin = 0.0;
    }
}

#[derive(Debug, Default, Clone)]
pub struct RateControl {
    target_secs: f64,
    error: Option<f64>,
    integral: f64,
    adjust: f64,
    scale: f64,
}

impl RateControl {
    pub fn new(target_secs: f64) -> Self {
        Self {
            target_secs,
            scale: 1.0,
            ..Self::default()
        }
    }

    pub fn set_slow(&mut self, slow: bool) {
        self.scale = if slow { SLOW_SCALE } else { 1.0 };
    }

    pub fn update(&mut self, queued_secs: f64, dt: f64, playing: bool) -> f64 {
        let cushion = if self.scale > 1.0 {
            SLOW_CUSHION_SECS
        } else {
            0.0
        };
        let raw = queued_secs - self.target_secs - cushion;
        let error = match self.error {
            Some(prev) => prev + (raw - prev) * (dt / SMOOTHING_SECS).min(1.0),
            None => raw,
        };
        self.error = Some(error);

        if !playing {
            self.adjust = 0.0;
            return self.adjust;
        }

        let most = MAX_ADJUST * self.scale;
        let proportional = PROPORTIONAL * self.scale;
        let integral = INTEGRAL * self.scale * self.scale;
        let limit = most / integral;
        self.integral = (self.integral + error * dt).clamp(-limit, limit);
        self.adjust = (-(proportional * error + integral * self.integral)).clamp(-most, most);
        self.adjust
    }

    pub fn adjust(&self) -> f64 {
        self.adjust
    }
}

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    strum_macros::EnumIter,
)]
pub enum SlowSound {
    #[default]
    Tempo,
    Pitch,
}

impl std::fmt::Display for SlowSound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Tempo => "Normal pitch",
            Self::Pitch => "Lower pitch",
        })
    }
}

pub struct AudioFeed {
    writer: QueueWriter,
    resampler: Resampler,
    rate: RateControl,
    follow: SpeedFollow,
    stretch: Stretch,
    stretching: bool,
    slow_sound: SlowSound,
    last_push: Option<web_time::Instant>,
    resumed: AtomicBool,
    stretched: Vec<f32>,
    carry: Vec<f32>,
    sample_rate: u32,
    target_frames: usize,
    scratch: Vec<f32>,
    pace: f64,
}

impl AudioFeed {
    pub fn new(writer: QueueWriter, sample_rate: u32, target_frames: usize) -> Self {
        Self {
            writer,
            resampler: Resampler::new(),
            rate: RateControl::new(target_frames as f64 / sample_rate.max(1) as f64),
            follow: SpeedFollow::default(),
            stretch: Stretch::new(sample_rate),
            stretching: false,
            slow_sound: SlowSound::default(),
            last_push: None,
            resumed: AtomicBool::new(false),
            stretched: Vec::new(),
            carry: Vec::new(),
            sample_rate,
            target_frames,
            scratch: Vec::new(),
            pace: 1.0,
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        self.push_at(samples, web_time::Instant::now());
    }

    pub fn push_at(&mut self, samples: &[f32], now: web_time::Instant) {
        if samples.len() < 2 {
            return;
        }
        let wall = self
            .last_push
            .map_or(0.0, |last| now.duration_since(last).as_secs_f64());
        self.last_push = Some(now);
        self.push_after(samples, wall);
    }

    pub fn push_after(&mut self, samples: &[f32], wall_secs: f64) {
        let frames = samples.len() / 2;
        if frames == 0 {
            return;
        }
        let rate = self.sample_rate.max(1) as f64;
        if self.resumed.swap(false, Ordering::Relaxed) {
            self.follow.reset();
        }
        let speed = self.follow.update(wall_secs, frames as f64 / rate);
        self.rate.set_slow(speed.is_some());
        let slowed = speed.unwrap_or(1.0);
        let pending = self.resampler.pending_frames() + (self.carry.len() / 2) as f64;
        let queued = match speed {
            None => self.writer.queued_frames() as f64 + pending + frames as f64 * 0.5,
            Some(speed) => {
                self.writer.queued_frames() as f64 + (pending + frames as f64 * 0.5) / speed
            }
        };
        let adjust = self.rate.update(
            queued / rate,
            frames as f64 / rate / slowed,
            self.writer.is_playing(),
        );

        self.scratch.clear();
        match speed {
            Some(speed) => {
                self.stretching = true;
                self.stretched.clear();
                let (tempo, ratio) = match self.slow_sound {
                    SlowSound::Tempo => (speed / (1.0 + adjust), self.pace),
                    SlowSound::Pitch => (1.0, slowed * self.pace / (1.0 + adjust)),
                };
                if !self.carry.is_empty() {
                    self.stretch
                        .process(&self.carry, tempo, &mut self.stretched);
                    self.carry.clear();
                }
                self.stretch.process(samples, tempo, &mut self.stretched);
                self.resampler
                    .process(&self.stretched, ratio, &mut self.scratch);
            }
            None => {
                if self.stretching {
                    self.stretching = false;
                    self.stretch.flush(&mut self.carry);
                }
                let input = if self.carry.is_empty() {
                    samples
                } else {
                    self.carry.extend_from_slice(samples);
                    let take =
                        (samples.len() + samples.len() / CARRY_SHARE).min(self.carry.len()) & !1;
                    self.stretched.clear();
                    self.stretched.extend(self.carry.drain(..take));
                    &self.stretched[..]
                };
                self.resampler.process(
                    input,
                    slowed * self.pace / (1.0 + adjust),
                    &mut self.scratch,
                );
            }
        }
        self.writer.write(&self.scratch);
    }

    pub fn set_pace(&mut self, pace: f64) {
        self.pace = pace;
    }

    pub fn set_session(&self, session: bool) {
        self.writer.set_quick_resume(session);
    }

    pub fn set_slow_sound(&mut self, slow_sound: SlowSound) {
        self.slow_sound = slow_sound;
    }

    pub fn set_paused(&self, paused: bool) {
        if paused {
            self.resumed.store(true, Ordering::Relaxed);
        }
        self.writer.set_paused(paused);
    }

    pub fn queued_frames(&self) -> usize {
        self.writer.queued_frames()
    }

    pub fn target_frames(&self) -> usize {
        self.target_frames
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
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
    use crate::audio::even_clock::EvenClock;
    use crate::audio::queue::{QueueConfig, audio_queue, latency_frames};
    use citrine_gb::console::ConsoleKind;

    const GB: Timing = Timing {
        clock_hz: 4_194_304,
        cycles_per_frame: 70_224,
    };
    const FRAME_TIME: f64 = 70_224.0 / 4_194_304.0;

    #[test]
    fn frame_clock_paces_by_the_console_timing() {
        assert_eq!(ConsoleKind::Gb.timing(), GB);
        assert!((1.0 / FRAME_TIME - 59.7275).abs() < 1e-6);
        let gba = ConsoleKind::Gba.timing();
        let mut clock = FrameClock::default();
        assert_eq!(clock.advance(gba.frame_secs() * 1.5, gba), 1);
    }

    #[test]
    fn frame_clock_runs_what_is_due() {
        let mut clock = FrameClock::default();
        assert_eq!(clock.advance(FRAME_TIME * 0.5, GB), 0);
        assert_eq!(clock.advance(FRAME_TIME * 0.6, GB), 1);
        assert_eq!(clock.advance(FRAME_TIME * 2.0, GB), 2);
    }

    #[test]
    fn frame_clock_catches_up_over_several_updates() {
        let mut clock = FrameClock::default();
        let behind = FRAME_TIME * 7.2;
        let first = clock.advance(behind, GB);
        let second = clock.advance(0.0, GB);
        assert_eq!(first, MAX_FRAMES_PER_UPDATE);
        assert_eq!(first + second, 7);
    }

    #[test]
    fn frame_clock_drops_a_long_stall() {
        let mut clock = FrameClock::default();
        assert_eq!(clock.advance(2.0, GB), 1);
        assert_eq!(clock.advance(0.0, GB), 0);
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
        let mut clock = EvenClock::default();

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
                let before = feed.stats().underruns();
                reader.read(&mut out, 2);
                next_callback += period;
                if feed.stats().underruns() > before {
                    println!("UNDER at {now:.2}");
                }
            }
            let frames = clock.advance(dt, GB);
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

    #[test]
    fn silent_updates_between_frames_still_show_a_slow_session() {
        let rate = 48_000;
        let config = QueueConfig::new(rate, latency_frames(rate, 80));
        let target = config.target_frames;
        let (writer, _reader) = audio_queue(config);
        let mut feed = AudioFeed::new(writer, rate, target);
        let start = web_time::Instant::now();
        let frame = ConsoleKind::Nds.timing().frame_secs();
        let made = (frame * rate as f64) as usize;
        let samples = vec![0.1f32; made * 2];
        let mut wall = 0.0;
        while wall < 10.0 {
            wall += frame / 0.9;
            feed.push_at(&samples, start + std::time::Duration::from_secs_f64(wall));
            for gap in 1..3 {
                let at = wall + gap as f64 * 0.006;
                feed.push_at(&[], start + std::time::Duration::from_secs_f64(at));
            }
        }
        assert!(feed.stretching);
    }

    struct Load {
        underruns: u64,
        overruns: u64,
        repeated_per_sec: f64,
    }

    fn sixty_seconds_at_60_hz(timing: Timing, rate: u32, late_vsyncs: bool) -> Load {
        let config = QueueConfig::new(rate, latency_frames(rate, 80));
        let target = config.target_frames;
        let (writer, mut reader) = audio_queue(config);
        let mut feed = AudioFeed::new(writer, rate, target);
        let mut clock = EvenClock::default();

        let callback = rate as usize / 100;
        let period = callback as f64 / (rate as f64 * 1.0002);
        let mut out = vec![0.0f32; callback * 2];
        let mut next_callback = 0.0;

        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut uniform = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            ((seed >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        let vsyncs: Vec<f64> = (0..60 * 60)
            .map(|i| {
                let (a, b) = (uniform(), uniform());
                let jitter = (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos() * 0.0003;
                i as f64 / 60.0 + jitter
            })
            .collect();

        let mut last = vsyncs[0];
        let mut next = 1;
        let mut update = 0usize;
        let mut owed = 0.0;
        while next < vsyncs.len() {
            let now = vsyncs[next];
            let dt = now - last;
            last = now;
            update += 1;
            next += match update {
                _ if !late_vsyncs => 1,
                u if u % 60 == 0 => 3,
                u if u % 12 == 0 => 2,
                _ => 1,
            };
            while next_callback <= now {
                reader.read(&mut out, 2);
                next_callback += period;
            }
            let frames = clock.advance(dt, timing);
            owed += frames as f64 * timing.frame_secs() * rate as f64;
            let produced = owed as usize;
            owed -= produced as f64;
            feed.push(&vec![0.1f32; produced * 2]);
        }

        Load {
            underruns: feed.stats().underruns(),
            overruns: feed.stats().overruns(),
            repeated_per_sec: clock.count().repeated_per_sec(),
        }
    }

    #[test]
    fn late_vsyncs_keep_the_queue_fed() {
        let even = Timing {
            clock_hz: 6_000_000,
            cycles_per_frame: 100_000,
        };
        let consoles = [
            ("ds", ConsoleKind::Nds.timing()),
            ("gba", ConsoleKind::Gba.timing()),
            ("gb", GB),
            ("even", even),
        ];
        for (name, timing) in consoles {
            for rate in [44_100, 48_000] {
                for late in [false, true] {
                    let run = sixty_seconds_at_60_hz(timing, rate, late);
                    assert_eq!(run.underruns, 0, "{name} {rate} late {late}");
                    assert_eq!(run.overruns, 0, "{name} {rate} late {late}");
                }
            }
        }
        let run = sixty_seconds_at_60_hz(even, 48_000, false);
        assert_eq!(run.repeated_per_sec, 0.0);
    }

    struct Slow {
        silence: f64,
        steady_silence: f64,
        out: Vec<f32>,
        ever_slow: bool,
        achieved: f64,
        overruns: u64,
    }

    fn slow_run(
        timing: Timing,
        rate: u32,
        speed: impl Fn(f64) -> f64,
        secs: f64,
        slow_sound: impl Fn(f64) -> SlowSound,
        vsync_wait: bool,
        mut make: impl FnMut(usize) -> Vec<f32>,
    ) -> Slow {
        let config = QueueConfig::new(rate, latency_frames(rate, 80));
        let target = config.target_frames;
        let (writer, mut reader) = audio_queue(config);
        let mut feed = AudioFeed::new(writer, rate, target);
        let mut ever_slow = false;
        let mut clock = EvenClock::default();

        let callback = rate as usize / 100;
        let period = callback as f64 / rate as f64;
        let mut buf = vec![0.0f32; callback * 2];
        let mut out = Vec::new();
        let mut next_callback = 0.0;
        let mut started_at = None;
        let mut read = 0u64;
        let mut steady_from = None;
        let mut speak = |until: f64,
                         reader: &mut crate::audio::queue::QueueReader,
                         stats: &QueueStats,
                         out: &mut Vec<f32>| {
            while next_callback <= until {
                reader.read(&mut buf, 2);
                next_callback += period;
                if started_at.is_none() && stats.is_playing() {
                    started_at = Some(next_callback);
                }
                if started_at.is_some() {
                    read += callback as u64;
                    out.extend_from_slice(&buf);
                }
                if steady_from.is_none() && next_callback >= 5.0 && started_at.is_some() {
                    steady_from = Some((read, stats.underrun_frames()));
                }
            }
        };

        let vsync = 1.0 / 60.0;
        let mut now = 0.0f64;
        let mut last = 0.0f64;
        let mut last_push = 0.0f64;
        let mut owed = 0.0;
        let mut made = 0usize;
        while now < secs {
            let dt = now - last;
            last = now;
            let due = clock.advance(dt, timing);
            let cost = due as f64 * timing.frame_secs() / speed(now);
            let pushed_at = now + cost;
            let stats = feed.stats().clone();
            speak(pushed_at, &mut reader, &stats, &mut out);
            if due > 0 {
                owed += due as f64 * timing.frame_secs() * rate as f64;
                let produced = owed as usize;
                owed -= produced as f64;
                made += produced;
                feed.set_slow_sound(slow_sound(now));
                feed.push_after(&make(produced), pushed_at - last_push);
                ever_slow |= feed.follow.speed().is_some();
                last_push = pushed_at;
            }
            now = if vsync_wait {
                (pushed_at / vsync - 1e-9)
                    .ceil()
                    .max((now / vsync).round() + 1.0)
                    * vsync
            } else {
                pushed_at.max(now + vsync)
            };
            speak(now, &mut reader, &stats, &mut out);
        }

        let stats = feed.stats();
        let (steady_read, steady_under) = steady_from.unwrap_or((0, 0));
        Slow {
            silence: stats.underrun_frames() as f64 / read.max(1) as f64,
            steady_silence: (stats.underrun_frames() - steady_under) as f64
                / (read - steady_read).max(1) as f64,
            out,
            ever_slow,
            achieved: made as f64 / rate as f64 / now,
            overruns: stats.overruns(),
        }
    }

    fn tone(rate: u32) -> impl FnMut(usize) -> Vec<f32> {
        let mut phase = 0.0f64;
        move |frames| {
            (0..frames)
                .flat_map(|_| {
                    phase += 440.0 / rate as f64;
                    let v = (phase * std::f64::consts::TAU).sin() as f32 * 0.3;
                    [v, v]
                })
                .collect()
        }
    }

    fn pitch_range(out: &[f32], rate: u32, from_secs: f64) -> (f64, f64) {
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        let window = rate as usize / 2;
        let start = (from_secs * rate as f64) as usize;
        let mut range = (f64::INFINITY, 0.0f64);
        let mut at = start;
        while at + window <= left.len() {
            let ups = left[at..at + window]
                .windows(2)
                .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
                .count() as f64
                * 2.0;
            range = (range.0.min(ups), range.1.max(ups));
            at += window;
        }
        range
    }

    const CONSOLES: [ConsoleKind; 3] = [ConsoleKind::Nds, ConsoleKind::Gba, ConsoleKind::Gb];

    #[test]
    fn slow_consoles_play_without_gaps() {
        for slow_sound in [SlowSound::Pitch, SlowSound::Tempo] {
            for kind in CONSOLES {
                for speed in [0.95, 0.85, 0.7] {
                    let run = slow_run(
                        kind.timing(),
                        48_000,
                        |_| speed,
                        20.0,
                        move |_| slow_sound,
                        false,
                        tone(48_000),
                    );
                    let what = format!("{slow_sound:?} {kind:?} {speed}x");
                    assert!(
                        (run.achieved - speed).abs() < 0.01,
                        "{what}: ran at {}",
                        run.achieved
                    );
                    assert!(
                        run.steady_silence < 0.0005,
                        "{what}: {:.2}% silent",
                        run.steady_silence * 100.0
                    );
                    assert!(
                        run.silence < 0.01,
                        "{what}: {:.2}% silent from the start",
                        run.silence * 100.0
                    );
                    assert_eq!(run.overruns, 0, "{what}");
                    let (low, high) = pitch_range(&run.out, 48_000, 10.0);
                    let want = match slow_sound {
                        SlowSound::Pitch => 440.0 * speed,
                        SlowSound::Tempo => 440.0,
                    };
                    assert!(
                        low >= want - 6.0 && high <= want + 6.0,
                        "{what}: {low}-{high} Hz"
                    );
                }
            }
        }
    }

    #[test]
    fn a_frame_locked_loop_below_full_speed_keeps_playing() {
        for slow_sound in [SlowSound::Pitch, SlowSound::Tempo] {
            for speed in [0.95, 0.7] {
                let run = slow_run(
                    ConsoleKind::Nds.timing(),
                    48_000,
                    |_| speed,
                    20.0,
                    move |_| slow_sound,
                    true,
                    tone(48_000),
                );
                assert!(
                    run.achieved < 0.8,
                    "{slow_sound:?} {speed}x ran at {}",
                    run.achieved
                );
                assert!(
                    run.steady_silence < 0.001,
                    "{slow_sound:?} {speed}x: {:.2}% silent",
                    run.steady_silence * 100.0
                );
            }
        }
    }

    #[test]
    fn full_speed_never_slows_the_sound() {
        for slow_sound in [SlowSound::Pitch, SlowSound::Tempo] {
            for kind in CONSOLES {
                for (speed, vsync_wait) in [(1.0, false), (1.5, false), (1.5, true)] {
                    let run = slow_run(
                        kind.timing(),
                        48_000,
                        |_| speed,
                        20.0,
                        move |_| slow_sound,
                        vsync_wait,
                        tone(48_000),
                    );
                    let what = format!("{slow_sound:?} {kind:?} {speed}x vsync {vsync_wait}");
                    assert!(!run.ever_slow, "{what}");
                    assert_eq!(run.silence, 0.0, "{what}");
                    assert_eq!(run.overruns, 0, "{what}");
                }
            }
        }
    }

    #[test]
    fn swinging_speed_keeps_most_of_the_sound() {
        let swing = |t: f64| {
            if ((t / 6.0) as usize).is_multiple_of(2) {
                1.5
            } else {
                0.7
            }
        };
        for slow_sound in [SlowSound::Pitch, SlowSound::Tempo] {
            let run = slow_run(
                ConsoleKind::Nds.timing(),
                48_000,
                swing,
                36.0,
                move |_| slow_sound,
                false,
                tone(48_000),
            );
            assert!(
                run.steady_silence < 0.02,
                "{slow_sound:?}: {:.2}% silent",
                run.steady_silence * 100.0
            );
        }
    }

    #[test]
    fn switching_the_slowdown_sound_while_playing_keeps_it_smooth() {
        let flip = |t: f64| {
            if ((t / 3.0) as usize).is_multiple_of(2) {
                SlowSound::Tempo
            } else {
                SlowSound::Pitch
            }
        };
        let run = slow_run(
            ConsoleKind::Nds.timing(),
            48_000,
            |_| 0.7,
            24.0,
            flip,
            false,
            tone(48_000),
        );
        assert!(
            run.steady_silence < 0.0005,
            "{:.2}% silent",
            run.steady_silence * 100.0
        );
        assert_eq!(run.overruns, 0);
        let left: Vec<f32> = run.out.chunks(2).map(|f| f[0]).collect();
        let jump = left[5 * 48_000..]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(jump < 0.05, "a jump of {jump}");
    }
}
