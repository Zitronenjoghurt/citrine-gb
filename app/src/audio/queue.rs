use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

const FADE_SECS: f32 = 0.005;
const GAIN_SMOOTHING: f32 = 0.002;
const EXCESS_SECS: f64 = 0.15;
const HEADROOM_SECS: f64 = 0.1;

pub struct QueueStats {
    underruns: AtomicU64,
    underrun_frames: AtomicU64,
    overruns: AtomicU64,
    overrun_frames: AtomicU64,
    playing: AtomicBool,
    paused: AtomicBool,
    min_sample: AtomicU32,
    max_sample: AtomicU32,
}

impl Default for QueueStats {
    fn default() -> Self {
        Self {
            underruns: AtomicU64::new(0),
            underrun_frames: AtomicU64::new(0),
            overruns: AtomicU64::new(0),
            overrun_frames: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            min_sample: AtomicU32::new(f32::INFINITY.to_bits()),
            max_sample: AtomicU32::new(f32::NEG_INFINITY.to_bits()),
        }
    }
}

impl QueueStats {
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    pub fn underrun_frames(&self) -> u64 {
        self.underrun_frames.load(Ordering::Relaxed)
    }

    pub fn overruns(&self) -> u64 {
        self.overruns.load(Ordering::Relaxed)
    }

    pub fn overrun_frames(&self) -> u64 {
        self.overrun_frames.load(Ordering::Relaxed)
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn min_sample(&self) -> f32 {
        f32::from_bits(self.min_sample.load(Ordering::Relaxed))
    }

    pub fn max_sample(&self) -> f32 {
        f32::from_bits(self.max_sample.load(Ordering::Relaxed))
    }

    pub fn reset(&self) {
        self.underruns.store(0, Ordering::Relaxed);
        self.underrun_frames.store(0, Ordering::Relaxed);
        self.overruns.store(0, Ordering::Relaxed);
        self.overrun_frames.store(0, Ordering::Relaxed);
        self.min_sample
            .store(f32::INFINITY.to_bits(), Ordering::Relaxed);
        self.max_sample
            .store(f32::NEG_INFINITY.to_bits(), Ordering::Relaxed);
    }

    fn add_underrun(&self, frames: usize, new_event: bool) {
        if new_event {
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
        self.underrun_frames
            .fetch_add(frames as u64, Ordering::Relaxed);
    }

    fn add_overrun(&self, frames: usize) {
        self.overruns.fetch_add(1, Ordering::Relaxed);
        self.overrun_frames
            .fetch_add(frames as u64, Ordering::Relaxed);
    }

    fn note_range(&self, min: f32, max: f32) {
        if min < self.min_sample() {
            self.min_sample.store(min.to_bits(), Ordering::Relaxed);
        }
        if max > self.max_sample() {
            self.max_sample.store(max.to_bits(), Ordering::Relaxed);
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub struct RemoteReport {
    pub playing: bool,
    pub underruns: u64,
    pub underrun_frames: u64,
    pub overruns: u64,
    pub overrun_frames: u64,
    pub min_sample: f32,
    pub max_sample: f32,
}

#[cfg(target_arch = "wasm32")]
impl QueueStats {
    pub fn mirror(&self, report: &RemoteReport) {
        self.playing.store(report.playing, Ordering::Relaxed);
        self.underruns.store(report.underruns, Ordering::Relaxed);
        self.underrun_frames
            .store(report.underrun_frames, Ordering::Relaxed);
        self.overruns.store(report.overruns, Ordering::Relaxed);
        self.overrun_frames
            .store(report.overrun_frames, Ordering::Relaxed);
        self.min_sample
            .store(report.min_sample.to_bits(), Ordering::Relaxed);
        self.max_sample
            .store(report.max_sample.to_bits(), Ordering::Relaxed);
    }
}

pub struct QueueWriter {
    producer: HeapProd<f32>,
    stats: Arc<QueueStats>,
}

impl QueueWriter {
    pub fn write(&mut self, samples: &[f32]) {
        let wanted = samples.len() & !1;
        let take = (self.producer.vacant_len() & !1).min(wanted);
        let pushed = self.producer.push_slice(&samples[..take]);
        if pushed < wanted {
            self.stats.add_overrun((wanted - pushed) / 2);
        }
    }

    pub fn queued_frames(&self) -> usize {
        self.producer.occupied_len() / 2
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn is_playing(&self) -> bool {
        self.stats.is_playing()
    }

    pub fn set_paused(&self, paused: bool) {
        self.stats.paused.store(paused, Ordering::Relaxed);
    }

    pub fn stats(&self) -> &Arc<QueueStats> {
        &self.stats
    }
}

pub struct QueueReader {
    consumer: HeapCons<f32>,
    stats: Arc<QueueStats>,
    start_frames: usize,
    settle_frames: usize,
    excess_frames: usize,
    fade_frames: usize,
    playing: bool,
    started: bool,
    fade_in: f32,
    gain: f32,
    blend: Vec<[f32; 2]>,
    blend_pos: usize,
}

impl QueueReader {
    pub fn queued_frames(&self) -> usize {
        self.consumer.occupied_len() / 2
    }

    pub fn read(&mut self, out: &mut [f32], channels: usize, gain: f32) {
        if channels == 0 {
            return;
        }
        let frames = out.len() / channels;

        if !self.playing {
            if self.queued_frames() < self.start_frames {
                out.fill(0.0);
                self.gain = gain;
                if self.started && !self.stats.paused.load(Ordering::Relaxed) {
                    self.stats.add_underrun(frames, false);
                }
                return;
            }
            self.playing = true;
            self.started = true;
            self.fade_in = 0.0;
            self.gain = gain;
            self.stats.playing.store(true, Ordering::Relaxed);
        }

        if self.blend_pos >= self.blend.len() && self.queued_frames() > self.excess_frames {
            self.skip_to(self.settle_frames);
        }

        let available = self.queued_frames();
        let starving = available < frames + self.fade_frames;
        let play = if starving {
            available.min(frames)
        } else {
            frames
        };
        let fade_out = play.min(self.fade_frames).max(1) as f32;
        let fade_step = 1.0 / self.fade_frames.max(1) as f32;

        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;

        for (i, slot) in out.chunks_mut(channels).enumerate() {
            let frame = if i < play {
                self.next_frame()
            } else {
                [0.0; 2]
            };
            min = min.min(frame[0]).min(frame[1]);
            max = max.max(frame[0]).max(frame[1]);

            let mut envelope = self.fade_in;
            if self.fade_in < 1.0 {
                self.fade_in = (self.fade_in + fade_step).min(1.0);
            }
            if starving {
                envelope *= ((play - i.min(play)) as f32 / fade_out).min(1.0);
            }
            self.gain += (gain - self.gain) * GAIN_SMOOTHING;
            let level = envelope * self.gain;

            match channels {
                1 => slot[0] = (frame[0] + frame[1]) * 0.5 * level,
                _ => {
                    slot[0] = frame[0] * level;
                    slot[1] = frame[1] * level;
                    for extra in &mut slot[2..] {
                        *extra = 0.0;
                    }
                }
            }
        }

        if play > 0 {
            self.stats.note_range(min, max);
        }

        if starving {
            self.playing = false;
            self.stats.playing.store(false, Ordering::Relaxed);
            if !self.stats.paused.load(Ordering::Relaxed) {
                self.stats.add_underrun(frames - play, true);
            }
        }
    }

    fn next_frame(&mut self) -> [f32; 2] {
        let mut frame = [0.0f32; 2];
        if self.consumer.pop_slice(&mut frame) < 2 {
            frame = [0.0; 2];
        }
        if self.blend_pos < self.blend.len() {
            let old = self.blend[self.blend_pos];
            self.blend_pos += 1;
            let w = self.blend_pos as f32 / (self.blend.len() + 1) as f32;
            frame[0] = old[0] + (frame[0] - old[0]) * w;
            frame[1] = old[1] + (frame[1] - old[1]) * w;
        }
        frame
    }

    fn skip_to(&mut self, keep_frames: usize) {
        let queued = self.queued_frames();
        let excess = queued.saturating_sub(keep_frames);
        if excess <= self.fade_frames {
            return;
        }
        for old in self.blend.iter_mut() {
            let mut frame = [0.0f32; 2];
            self.consumer.pop_slice(&mut frame);
            *old = frame;
        }
        self.consumer.skip((excess - self.fade_frames) * 2);
        self.blend_pos = 0;
        self.stats.add_overrun(excess);
    }
}

pub struct QueueConfig {
    pub sample_rate: u32,
    pub target_frames: usize,
    pub excess_frames: usize,
    pub capacity_frames: usize,
}

impl QueueConfig {
    pub fn new(sample_rate: u32, target_frames: usize) -> Self {
        let rate = sample_rate.max(1) as f64;
        let excess_frames = target_frames + (rate * EXCESS_SECS) as usize;
        Self {
            sample_rate,
            target_frames,
            excess_frames,
            capacity_frames: excess_frames + (rate * HEADROOM_SECS) as usize,
        }
        .normalized()
    }

    pub fn fade_frames(&self) -> usize {
        ((self.sample_rate as f32 * FADE_SECS) as usize).max(1)
    }

    fn normalized(self) -> Self {
        let fade = self.fade_frames();
        let target_frames = self.target_frames.max(fade * 2);
        let excess_frames = self.excess_frames.max(target_frames + fade * 2);
        Self {
            sample_rate: self.sample_rate,
            target_frames,
            excess_frames,
            capacity_frames: self.capacity_frames.max(excess_frames + fade * 2),
        }
    }
}

pub fn latency_frames(sample_rate: u32, latency_ms: u32) -> usize {
    ((sample_rate as u64 * latency_ms as u64 / 1000) as usize).max(1)
}

pub fn audio_queue(config: QueueConfig) -> (QueueWriter, QueueReader) {
    let config = config.normalized();
    let fade_frames = config.fade_frames();
    let target_frames = config.target_frames;
    let excess_frames = config.excess_frames;
    let (producer, consumer) = HeapRb::<f32>::new(config.capacity_frames * 2).split();
    let stats = Arc::new(QueueStats::default());

    let writer = QueueWriter {
        producer,
        stats: stats.clone(),
    };
    let reader = QueueReader {
        consumer,
        stats,
        start_frames: target_frames,
        settle_frames: target_frames,
        excess_frames,
        fade_frames,
        playing: false,
        started: false,
        fade_in: 0.0,
        gain: 0.0,
        blend: vec![[0.0; 2]; fade_frames],
        blend_pos: fade_frames,
    };
    (writer, reader)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn queue(target: usize) -> (QueueWriter, QueueReader) {
        audio_queue(QueueConfig {
            sample_rate: RATE,
            target_frames: target,
            excess_frames: target * 3,
            capacity_frames: target * 4,
        })
    }

    fn constant(frames: usize, value: f32) -> Vec<f32> {
        vec![value; frames * 2]
    }

    fn ramp(start: usize, frames: usize) -> Vec<f32> {
        (start..start + frames)
            .flat_map(|i| {
                let v = (i % 4800) as f32 / 4800.0;
                [v, v]
            })
            .collect()
    }

    fn largest_step(samples: &[f32]) -> f32 {
        samples
            .chunks(2)
            .map(|f| f[0])
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max)
    }

    #[test]
    fn waits_for_target_before_playing() {
        let (mut writer, mut reader) = queue(1000);
        let mut out = vec![1.0; 960];
        writer.write(&constant(600, 0.5));
        reader.read(&mut out, 2, 1.0);
        assert!(out.iter().all(|s| *s == 0.0));
        assert_eq!(writer.stats().underruns(), 0);
        assert!(!writer.is_playing());

        writer.write(&constant(600, 0.5));
        reader.read(&mut out, 2, 1.0);
        assert!(writer.is_playing());
        assert!(out[0] < 0.01);
        assert!(out[out.len() - 1] > 0.49);
        assert_eq!(writer.stats().underruns(), 0);
    }

    #[test]
    fn fades_out_when_running_dry() {
        let (mut writer, mut reader) = queue(1000);
        writer.write(&constant(1200, 0.5));
        let mut played = Vec::new();
        let mut out = vec![0.0; 960];
        for _ in 0..4 {
            reader.read(&mut out, 2, 1.0);
            played.extend_from_slice(&out);
        }
        assert_eq!(writer.stats().underruns(), 1);
        assert!(!writer.is_playing());
        assert!(largest_step(&played) < 0.01);
        assert_eq!(played[played.len() - 1], 0.0);
    }

    #[test]
    fn resumes_with_the_rest_of_the_sound() {
        let (mut writer, mut reader) = queue(1000);
        writer.write(&ramp(0, 1200));
        let mut out = vec![0.0; 960];
        let mut played = Vec::new();
        for _ in 0..3 {
            reader.read(&mut out, 2, 1.0);
            played.extend_from_slice(&out);
        }
        writer.write(&ramp(1200, 2000));
        for _ in 0..3 {
            reader.read(&mut out, 2, 1.0);
            played.extend_from_slice(&out);
        }
        assert_eq!(writer.stats().underruns(), 1);
        assert!(largest_step(&played) < 0.01);
    }

    #[test]
    fn skips_excess_with_a_crossfade() {
        let (mut writer, mut reader) = queue(1000);
        writer.write(&ramp(0, 3900));
        let mut out = vec![0.0; 960];
        let mut played = Vec::new();
        for _ in 0..2 {
            reader.read(&mut out, 2, 1.0);
            played.extend_from_slice(&out);
        }
        assert_eq!(writer.stats().overruns(), 1);
        assert!(writer.queued_frames() <= 1000);
        assert!(largest_step(&played) < 0.01);
    }

    #[test]
    fn counts_writes_that_do_not_fit() {
        let (mut writer, _reader) = queue(1000);
        writer.write(&constant(5000, 0.1));
        assert_eq!(writer.stats().overruns(), 1);
        assert_eq!(writer.stats().overrun_frames(), 1000);
    }

    #[test]
    fn running_dry_while_paused_is_not_an_underrun() {
        let (mut writer, mut reader) = queue(1000);
        writer.write(&constant(1200, 0.5));
        writer.set_paused(true);
        let mut out = vec![0.0; 960];
        for _ in 0..5 {
            reader.read(&mut out, 2, 1.0);
        }
        assert_eq!(writer.stats().underruns(), 0);
        assert_eq!(writer.stats().underrun_frames(), 0);
    }

    #[test]
    fn mixes_down_to_mono_and_clears_extra_channels() {
        let (mut writer, mut reader) = queue(1000);
        let frames: Vec<f32> = (0..2000).flat_map(|_| [0.2, 0.6]).collect();
        writer.write(&frames);
        let mut mono = vec![0.0; 480];
        reader.read(&mut mono, 1, 1.0);
        assert!((mono[479] - 0.4).abs() < 1e-3);
        let mut quad = vec![1.0; 480 * 4];
        reader.read(&mut quad, 4, 1.0);
        let last = &quad[quad.len() - 4..];
        assert!((last[0] - 0.2).abs() < 1e-3);
        assert!((last[1] - 0.6).abs() < 1e-3);
        assert_eq!(&last[2..], &[0.0, 0.0]);
    }
}
