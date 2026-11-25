use std::collections::VecDeque;

const KEEP_SECS: f64 = 0.5;
const MAX_BEHIND_FRAMES: u64 = 2;
const CHUNK_DECAY: u64 = 64;
const LARGEST_CHUNK_DIVISOR: u32 = 20;
const LEVEL_SMOOTHING: f64 = 16.0;
const STRETCH_GAIN: f64 = 32.0;
const MOST_STRETCH_DIVISOR: u64 = 100;

#[derive(Debug, Clone)]
pub struct MicPacer {
    rate: u32,
    kept: VecDeque<i16>,
    end: u64,
    next: u64,
    chunk: u64,
    fraction: f64,
    level: f64,
    frame: Vec<i16>,
    resyncs: u64,
}

impl MicPacer {
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            kept: VecDeque::new(),
            end: 0,
            next: 0,
            chunk: 0,
            fraction: 0.0,
            level: 0.0,
            frame: Vec::new(),
            resyncs: 0,
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn push(&mut self, samples: &[f32]) {
        self.kept
            .extend(samples.iter().map(|&sample| to_i16(sample)));
        self.end += samples.len() as u64;
        let largest = u64::from(self.rate / LARGEST_CHUNK_DIVISOR);
        let chunk = (samples.len() as u64).min(largest);
        self.chunk = chunk.max(self.chunk - self.chunk / CHUNK_DECAY);
        let keep = (f64::from(self.rate) * KEEP_SECS) as usize;
        let excess = self.kept.len().saturating_sub(keep);
        self.kept.drain(..excess);
    }

    pub fn frame(&mut self, frame_secs: f64) -> &[i16] {
        let exact = frame_secs * f64::from(self.rate) + self.fraction;
        let count = exact.floor() as u64;
        self.fraction = exact - count as f64;
        let target = count + self.chunk + count / 4;
        let first = self.end - self.kept.len() as u64;
        let waiting = self.end.saturating_sub(self.next);
        let mut take = count;
        if waiting < count || waiting > count * MAX_BEHIND_FRAMES + target + self.chunk {
            self.next = self.end.saturating_sub(target).max(first);
            self.level = target as f64;
            self.resyncs += 1;
        } else {
            self.level += (waiting as f64 - self.level) / LEVEL_SMOOTHING;
            let off = self.level - target as f64;
            let band = (self.chunk / 2) as f64;
            if off.abs() > band {
                let most = (count / MOST_STRETCH_DIVISOR).max(1) as f64;
                let stretch = ((off.abs() - band) / STRETCH_GAIN).round().clamp(1.0, most);
                take = count.saturating_add_signed(stretch.copysign(off) as i64);
            }
        }
        self.frame.clear();
        self.frame
            .extend((self.next..self.next + take).map(|index| {
                index
                    .checked_sub(first)
                    .and_then(|at| self.kept.get(at as usize).copied())
                    .unwrap_or(0)
            }));
        self.next += take;
        &self.frame
    }

    pub fn behind(&self) -> u64 {
        self.end.saturating_sub(self.next)
    }

    pub fn kept(&self) -> usize {
        self.kept.len()
    }

    pub fn resyncs(&self) -> u64 {
        self.resyncs
    }
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nds::lcd::FRAME_CYCLES;
    use crate::nds::rtc::CYCLES_PER_SECOND;
    use crate::nds::spi::firmware::Calibration;
    use crate::nds::spi::touch::Touch;

    const HOST_RATE: u32 = 48_000;
    const CHUNK: usize = 480;
    const GAME_RATE: u64 = 16_384;
    const AUX: u8 = 0xE0;
    const MINUTE_FRAMES: u64 = 60 * CYCLES_PER_SECOND as u64 / FRAME_CYCLES as u64;
    const LATENCY_SECS: f64 = 0.064;

    struct Run {
        pacer: MicPacer,
        touch: Touch,
        pushed: u64,
        host_rate: f64,
        reads: Vec<(f64, u16)>,
        most_behind: u64,
        most_kept: usize,
    }

    impl Run {
        fn new(host_rate: f64) -> Self {
            Self {
                pacer: MicPacer::new(HOST_RATE),
                touch: Touch::new(Calibration::default()),
                pushed: 0,
                host_rate,
                reads: Vec::new(),
                most_behind: 0,
                most_kept: 0,
            }
        }

        fn capture_until(&mut self, secs: f64, signal: &impl Fn(u64) -> f32) {
            while (self.pushed + CHUNK as u64) as f64 <= secs * self.host_rate {
                let chunk: Vec<f32> = (self.pushed..self.pushed + CHUNK as u64)
                    .map(signal)
                    .collect();
                self.pacer.push(&chunk);
                self.pushed += CHUNK as u64;
            }
        }

        fn play(&mut self, frames: u64, frames_per_host_frame: u64, signal: &impl Fn(u64) -> f32) {
            let frame_secs = f64::from(FRAME_CYCLES) / f64::from(CYCLES_PER_SECOND);
            let read_at = |read: u64| read * u64::from(CYCLES_PER_SECOND) / GAME_RATE;
            let mut read = 0;
            for frame in 0..frames {
                let start = frame * u64::from(FRAME_CYCLES);
                let wall = (frame / frames_per_host_frame) as f64 * frame_secs;
                self.capture_until(wall, signal);
                let samples = self.pacer.frame(frame_secs).to_vec();
                self.most_behind = self.most_behind.max(self.pacer.behind());
                self.most_kept = self.most_kept.max(self.pacer.kept());
                self.touch.set_mic(&samples, start);
                while read_at(read) < start + u64::from(FRAME_CYCLES) {
                    let at = read_at(read);
                    let secs = at as f64 / f64::from(CYCLES_PER_SECOND);
                    self.reads.push((secs, read_mic(&mut self.touch, at)));
                    read += 1;
                }
            }
        }
    }

    fn read_mic(touch: &mut Touch, now: u64) -> u16 {
        touch.transfer(AUX, now);
        let high = touch.transfer(0, now);
        let low = touch.transfer(0, now);
        touch.deselect();
        (u16::from(high) << 8 | u16::from(low)) >> 3
    }

    fn sawtooth(index: u64) -> f32 {
        ((index % 4096) as f32 * 16.0 - 32768.0) / f32::from(i16::MAX)
    }

    fn tone(secs: f64) -> f32 {
        (0.5 * (2.0 * std::f64::consts::PI * 440.0 * secs).sin()) as f32
    }

    fn heard_index(read: u16, host_now: u64) -> u64 {
        let behind = (host_now % 4096 + 4096 - u64::from(read)) % 4096;
        host_now - behind
    }

    #[test]
    fn the_game_hears_the_host_in_order_and_close_to_now() {
        let mut run = Run::new(f64::from(HOST_RATE));
        run.play(MINUTE_FRAMES, 1, &sawtooth);
        let warm = run.reads.len() / 60;
        let heard: Vec<(u64, u64)> = run.reads[warm..]
            .iter()
            .map(|&(secs, read)| {
                let host_now = (secs * f64::from(HOST_RATE)) as u64;
                (host_now, heard_index(read, host_now))
            })
            .collect();
        let latency = (LATENCY_SECS * f64::from(HOST_RATE)) as u64;
        assert!(heard.iter().all(|&(now, index)| now - index <= latency));
        let jumps = heard
            .windows(2)
            .filter(|pair| !(1..=4).contains(&(pair[1].1 as i64 - pair[0].1 as i64)))
            .count();
        assert_eq!(jumps, 0);
    }

    #[test]
    fn a_tone_reads_back_within_four_percent_over_a_minute() {
        let mut run = Run::new(f64::from(HOST_RATE));
        let signal = |index: u64| tone(index as f64 / f64::from(HOST_RATE));
        run.play(MINUTE_FRAMES, 1, &signal);
        let warm = run.reads.len() / 60;
        let reads = &run.reads[warm..];
        let error_at = |lag: u64| {
            let lag = lag as f64 / f64::from(HOST_RATE);
            reads
                .iter()
                .map(|&(secs, read)| {
                    let heard = (f64::from(read) - 2048.0) / 2048.0;
                    (heard - f64::from(tone(secs - lag))).abs()
                })
                .fold(0.0, f64::max)
        };
        let latency = (LATENCY_SECS * f64::from(HOST_RATE)) as u64;
        let best = (0..=latency).map(error_at).fold(f64::MAX, f64::min);
        assert!(best <= 0.04, "worst error {best}");
    }

    #[test]
    fn the_queue_never_grows_over_a_minute() {
        let mut run = Run::new(f64::from(HOST_RATE));
        run.play(MINUTE_FRAMES, 1, &sawtooth);
        let latency = (LATENCY_SECS * f64::from(HOST_RATE)) as u64;
        assert!(run.most_behind <= latency, "{} behind", run.most_behind);
        assert!(run.most_kept <= HOST_RATE as usize / 2);
        assert!(run.touch.mic_read());
    }

    #[test]
    fn a_host_clock_a_tenth_of_a_percent_off_stays_close() {
        for host_rate in [f64::from(HOST_RATE) * 1.001, f64::from(HOST_RATE) * 0.999] {
            let mut run = Run::new(host_rate);
            run.play(MINUTE_FRAMES, 1, &sawtooth);
            let latency = (LATENCY_SECS * f64::from(HOST_RATE)) as u64;
            assert!(run.most_behind <= latency, "{} behind", run.most_behind);
            assert!(run.pacer.resyncs() <= 4, "{} resyncs", run.pacer.resyncs());
        }
    }

    #[test]
    fn fast_forward_hears_the_newest_sound() {
        let mut run = Run::new(f64::from(HOST_RATE));
        run.play(MINUTE_FRAMES, 4, &sawtooth);
        let latency = (LATENCY_SECS * f64::from(HOST_RATE)) as u64;
        assert!(run.most_behind <= latency, "{} behind", run.most_behind);
    }

    #[test]
    fn after_a_pause_the_game_hears_now_not_the_past() {
        let mut pacer = MicPacer::new(HOST_RATE);
        pacer.push(&vec![0.5; HOST_RATE as usize * 2]);
        pacer.push(&vec![-0.5; HOST_RATE as usize / 10]);
        let samples = pacer.frame(1.0 / 60.0).to_vec();
        assert_eq!(samples.len(), 800);
        assert!(samples.iter().all(|&sample| sample < 0));
    }

    #[test]
    fn no_capture_is_silence() {
        let mut pacer = MicPacer::new(HOST_RATE);
        let samples = pacer.frame(1.0 / 60.0);
        assert_eq!(samples, vec![0; 800]);
    }

    mod homebrew {
        use crate::nds::lcd::FRAME_CYCLES;
        use crate::nds::machine::{Nds, NdsFirmware};
        use crate::nds::mic::MicPacer;
        use crate::nds::rtc::CYCLES_PER_SECOND;

        const MICTEST: &[u8] = include_bytes!("../../../citrine-gb-app/fixtures/nds/mictest.nds");
        const HOST_RATE: u32 = 48_000;

        #[derive(Debug, Clone, Copy)]
        struct Report {
            rate: u32,
            buffers: u32,
            samples: u32,
            level: u32,
            crossings: u32,
            quiet: u32,
            loud: u32,
        }

        struct Desk {
            nds: Nds,
            pacer: MicPacer,
            pushed: u64,
            frames: u64,
        }

        impl Desk {
            fn new() -> Self {
                Self {
                    nds: Nds::new(MICTEST.into(), NdsFirmware::default()).unwrap(),
                    pacer: MicPacer::new(HOST_RATE),
                    pushed: 0,
                    frames: 0,
                }
            }

            fn play(&mut self, frames: u64, signal: impl Fn(f64) -> f32) {
                let frame_secs = f64::from(FRAME_CYCLES) / f64::from(CYCLES_PER_SECOND);
                for _ in 0..frames {
                    self.frames += 1;
                    let due = (self.frames as f64 * frame_secs * f64::from(HOST_RATE)) as u64;
                    let chunk: Vec<f32> = (self.pushed..due)
                        .map(|index| signal(index as f64 / f64::from(HOST_RATE)))
                        .collect();
                    self.pushed = due;
                    self.pacer.push(&chunk);
                    let samples = self.pacer.frame(frame_secs).to_vec();
                    self.nds.set_mic(&samples);
                    self.nds.run_frame();
                    self.nds.take_audio(&mut Vec::new());
                }
            }

            fn report(&self) -> Report {
                let ram = self.nds.buses().main_ram();
                let at = ram
                    .windows(8)
                    .position(|bytes| bytes == b"MICTEST\0")
                    .expect("the report is in main RAM");
                let word = |index: usize| {
                    let start = at + 8 + index * 4;
                    u32::from_le_bytes(ram[start..start + 4].try_into().unwrap())
                };
                Report {
                    rate: word(0),
                    buffers: word(1),
                    samples: word(2),
                    level: word(3),
                    crossings: word(4),
                    quiet: word(5),
                    loud: word(6),
                }
            }
        }

        fn tone(secs: f64) -> f32 {
            (0.5 * (2.0 * std::f64::consts::PI * 440.0 * secs).sin()) as f32
        }

        #[test]
        fn a_recording_homebrew_hears_the_pitch_and_the_silence() {
            let mut desk = Desk::new();
            desk.play(120, tone);
            let loud = desk.report();
            assert_eq!(loud.rate, 16_384, "{loud:?}");
            assert!(desk.nds.mic_in_use(), "{loud:?}");
            assert!(loud.loud > 4 && loud.level > 256, "{loud:?}");
            let pitch =
                f64::from(loud.crossings) / 2.0 * f64::from(loud.rate) / f64::from(loud.samples);
            assert!((pitch - 440.0).abs() < 440.0 * 0.03, "{pitch} Hz, {loud:?}");

            desk.play(60, |_| 0.0);
            let quiet = desk.report();
            assert!(quiet.buffers > loud.buffers + 4, "{quiet:?}");
            assert!(quiet.quiet > 4 && quiet.level < 16, "{quiet:?}");
        }
    }
}
