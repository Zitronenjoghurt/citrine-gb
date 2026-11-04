use crate::gb::apu::charge_factor;
use blip_buf::BlipBuf;

const CLOCK_SHIFT: u32 = 6;
const BLIP_CLOCK_RATE: f64 = (super::CLOCK_RATE >> CLOCK_SHIFT) as f64;
const BLIP_SAMPLES: u32 = 8192;
const MAX_FRAME_SAMPLES: u64 = 2048;
const SCALE: i32 = 16;
const FULL_SCALE: f32 = (512 * SCALE) as f32;
pub const DEFAULT_SAMPLE_RATE: u32 = 44_100;

pub struct Output {
    left: BlipBuf,
    right: BlipBuf,
    cycles: u32,
    max_time: u32,
    previous: (i32, i32),
    sample_rate: u32,
    charge_factor: f32,
    capacitor: (f32, f32),
    buffer: Vec<f32>,
    silent: bool,
}

impl Clone for Output {
    fn clone(&self) -> Self {
        let mut output = Self::default();
        output.set_sample_rate(self.sample_rate);
        output.silent = self.silent;
        output
    }
}

impl Default for Output {
    fn default() -> Self {
        let mut output = Self {
            left: BlipBuf::new(BLIP_SAMPLES),
            right: BlipBuf::new(BLIP_SAMPLES),
            cycles: 0,
            max_time: 0,
            previous: (0, 0),
            sample_rate: 0,
            charge_factor: 0.0,
            capacitor: (0.0, 0.0),
            buffer: Vec::new(),
            silent: false,
        };
        output.set_sample_rate(DEFAULT_SAMPLE_RATE);
        output
    }
}

impl Output {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        let rate = rate.max(1);
        self.sample_rate = rate;
        self.left.set_rates(BLIP_CLOCK_RATE, f64::from(rate));
        self.right.set_rates(BLIP_CLOCK_RATE, f64::from(rate));
        self.max_time = (BLIP_CLOCK_RATE as u64 * MAX_FRAME_SAMPLES / u64::from(rate))
            .clamp(1, u64::from(u32::MAX >> CLOCK_SHIFT)) as u32;
        self.charge_factor = charge_factor(rate);
    }

    pub fn advance(&mut self, cycles: u32) {
        self.cycles += cycles;
        if self.cycles >> CLOCK_SHIFT >= self.max_time {
            self.flush();
        }
    }

    pub fn set(&mut self, left: i32, right: i32) {
        let time = self.cycles >> CLOCK_SHIFT;
        let (left, right) = (left * SCALE, right * SCALE);
        if left != self.previous.0 {
            self.left.add_delta(time, left - self.previous.0);
        }
        if right != self.previous.1 {
            self.right.add_delta(time, right - self.previous.1);
        }
        self.previous = (left, right);
    }

    pub fn set_silent(&mut self, silent: bool) {
        self.silent = silent;
    }

    pub fn take(&mut self, out: &mut Vec<f32>) {
        self.flush();
        if self.silent {
            self.buffer.clear();
        } else {
            out.append(&mut self.buffer);
        }
    }

    fn flush(&mut self) {
        let time = self.cycles >> CLOCK_SHIFT;
        self.left.end_frame(time);
        self.right.end_frame(time);
        self.cycles -= time << CLOCK_SHIFT;

        let available = self.left.samples_avail() as usize;
        let mut left = vec![0i16; available];
        let mut right = vec![0i16; available];
        self.left.read_samples(&mut left, false);
        self.right.read_samples(&mut right, false);

        self.buffer.reserve(available * 2);
        for (left, right) in left.into_iter().zip(right) {
            let left = high_pass(&mut self.capacitor.0, self.charge_factor, left);
            let right = high_pass(&mut self.capacitor.1, self.charge_factor, right);
            self.buffer.push(left);
            self.buffer.push(right);
        }

        let max_len = self.sample_rate as usize * 2;
        if self.buffer.len() > max_len {
            let excess = self.buffer.len() - max_len;
            self.buffer.drain(..excess);
        }
    }
}

fn high_pass(capacitor: &mut f32, charge_factor: f32, sample: i16) -> f32 {
    let raw = f32::from(sample) / FULL_SCALE;
    let filtered = raw - *capacitor;
    *capacitor = raw - filtered * charge_factor;
    filtered
}
