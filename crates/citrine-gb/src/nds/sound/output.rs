use super::mixer::MIX_CYCLES;
use citrine_gb_blip::BlipBuf;

pub const CLOCK_RATE: f64 = 16_756_991.0;
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
const MIX_TICKS: u32 = MIX_CYCLES / 2;
const BLIP_SAMPLES: u32 = 8192;
const MAX_FRAME_SAMPLES: u64 = 2048;
const FULL_SCALE: f32 = 32768.0;

pub struct Output {
    blip: BlipBuf<2>,
    time: u32,
    max_time: u32,
    previous: [i16; 2],
    sample_rate: u32,
    buffer: Vec<i16>,
}

impl Default for Output {
    fn default() -> Self {
        Self::new(DEFAULT_SAMPLE_RATE)
    }
}

impl Output {
    pub fn new(sample_rate: u32) -> Self {
        let mut output = Self {
            blip: BlipBuf::new(BLIP_SAMPLES),
            time: 0,
            max_time: 0,
            previous: [0; 2],
            sample_rate: 0,
            buffer: Vec::new(),
        };
        output.set_sample_rate(sample_rate);
        output
    }

    pub fn resumed(sample_rate: u32, level: [i16; 2]) -> Self {
        let mut output = Self::new(sample_rate);
        output.previous = level;
        output
    }

    pub fn level(&self) -> [i16; 2] {
        self.previous
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        let rate = rate.max(1);
        self.sample_rate = rate;
        self.blip.set_rates(CLOCK_RATE, f64::from(rate));
        self.max_time = (CLOCK_RATE as u64 * MAX_FRAME_SAMPLES / u64::from(rate)) as u32;
        self.buffer.clear();
    }

    pub fn push(&mut self, sample: [i16; 2]) {
        self.time += MIX_TICKS;
        if sample != self.previous {
            let [left, right] = sample;
            let [was_left, was_right] = self.previous;
            self.blip.add_delta(
                self.time,
                [
                    i32::from(left) - i32::from(was_left),
                    i32::from(right) - i32::from(was_right),
                ],
            );
            self.previous = sample;
        }
        if self.time >= self.max_time {
            self.flush();
        }
    }

    pub fn hold(&mut self, sample: [i16; 2]) {
        self.previous = sample;
    }

    pub fn take(&mut self, out: &mut Vec<f32>) {
        self.flush();
        out.extend(
            self.buffer
                .drain(..)
                .map(|sample| f32::from(sample) / FULL_SCALE),
        );
    }

    pub fn take_raw(&mut self, out: &mut Vec<i16>) {
        self.flush();
        out.append(&mut self.buffer);
    }

    fn flush(&mut self) {
        self.blip.end_frame(self.time);
        self.time = 0;
        let available = self.blip.samples_avail() as usize;
        let start = self.buffer.len();
        self.buffer.resize(start + available * 2, 0);
        self.blip
            .read_samples(self.buffer[start..].as_chunks_mut::<2>().0);
        let max_len = self.sample_rate as usize * 2;
        if self.buffer.len() > max_len {
            let excess = self.buffer.len() - max_len;
            self.buffer.drain(..excess);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_comes_out_at_the_sample_rate_and_settles_back_through_the_high_pass() {
        let mut output = Output::new(DEFAULT_SAMPLE_RATE);
        for _ in 0..32_728 {
            output.push([0x1000, -0x1000]);
        }
        let mut out = Vec::new();
        output.take_raw(&mut out);
        assert!(out.len().abs_diff(2 * DEFAULT_SAMPLE_RATE as usize) <= 400);
        let [left, right] = [out[32], out[33]];
        assert!((0xF00..=0x1000).contains(&left), "{left:#x}");
        assert!((left + right).abs() <= 1, "{left} {right}");
        assert!(out[out.len() - 2].abs() < 0x10);
    }

    struct MonoPair {
        channels: [blip_buf::BlipBuf; 2],
        time: u32,
        previous: [i16; 2],
        out: Vec<i16>,
    }

    impl MonoPair {
        fn new() -> Self {
            let channels = std::array::from_fn(|_| {
                let mut blip = blip_buf::BlipBuf::new(BLIP_SAMPLES);
                blip.set_rates(CLOCK_RATE, f64::from(DEFAULT_SAMPLE_RATE));
                blip
            });
            Self {
                channels,
                time: 0,
                previous: [0; 2],
                out: Vec::new(),
            }
        }

        fn push(&mut self, sample: [i16; 2], max_time: u32) {
            self.time += MIX_TICKS;
            for (channel, (now, was)) in self
                .channels
                .iter_mut()
                .zip(sample.into_iter().zip(self.previous))
            {
                if now != was {
                    channel.add_delta(self.time, i32::from(now) - i32::from(was));
                }
            }
            self.previous = sample;
            if self.time >= max_time {
                self.flush();
            }
        }

        fn flush(&mut self) {
            let mut read = [Vec::new(), Vec::new()];
            for (channel, read) in self.channels.iter_mut().zip(&mut read) {
                channel.end_frame(self.time);
                *read = vec![0; channel.samples_avail() as usize];
                channel.read_samples(read, false);
            }
            self.time = 0;
            for (left, right) in read[0].iter().zip(&read[1]) {
                self.out.extend([*left, *right]);
            }
        }
    }

    #[test]
    fn both_channels_come_out_as_two_mono_blip_buffers_give_them() {
        let mut output = Output::new(DEFAULT_SAMPLE_RATE);
        let mut pair = MonoPair::new();
        let mut out = Vec::new();
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut sample = [0i16; 2];
        for push in 0..40_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            match seed & 7 {
                0 | 1 => sample[0] = (seed >> 16) as i16 >> 2,
                2 | 3 => sample[1] = (seed >> 32) as i16 >> 2,
                4 => sample = [(seed >> 16) as i16 >> 2, (seed >> 32) as i16 >> 2],
                _ => {}
            }
            output.push(sample);
            pair.push(sample, output.max_time);
            if push % 700 == 699 {
                output.take_raw(&mut out);
                pair.flush();
            }
        }
        output.take_raw(&mut out);
        pair.flush();
        assert!(out.len() > 50_000);
        assert_eq!(out, pair.out);
    }
}
