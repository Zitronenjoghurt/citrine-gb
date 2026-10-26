const TAPS: usize = 32;
const HALF: usize = TAPS / 2;
const PHASES: usize = 256;
const CUTOFF: f64 = 0.96;

pub struct Resampler {
    table: Vec<f32>,
    history: Vec<f32>,
    pos: f64,
}

impl Default for Resampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Resampler {
    pub fn new() -> Self {
        Self {
            table: build_table(),
            history: vec![0.0; (HALF - 1) * 2],
            pos: (HALF - 1) as f64,
        }
    }

    pub fn process(&mut self, input: &[f32], step: f64, out: &mut Vec<f32>) {
        self.history.extend_from_slice(&input[..input.len() & !1]);
        let frames = self.history.len() / 2;

        while (self.pos as usize) + HALF < frames {
            let base = self.pos as usize;
            let phase = (self.pos - base as f64) * PHASES as f64;
            let index = (phase as usize).min(PHASES - 1);
            let blend = (phase - index as f64) as f32;
            let a = &self.table[index * TAPS..(index + 1) * TAPS];
            let b = &self.table[(index + 1) * TAPS..(index + 2) * TAPS];
            let window = &self.history[(base + 1 - HALF) * 2..(base + 1 + HALF) * 2];

            let mut left = 0.0f32;
            let mut right = 0.0f32;
            for k in 0..TAPS {
                let w = a[k] + (b[k] - a[k]) * blend;
                left += window[k * 2] * w;
                right += window[k * 2 + 1] * w;
            }
            out.push(left);
            out.push(right);
            self.pos += step;
        }

        let keep_from = (self.pos as usize).saturating_sub(HALF - 1).min(frames);
        self.history.drain(..keep_from * 2);
        self.pos -= keep_from as f64;
    }

    pub fn pending_frames(&self) -> f64 {
        (self.history.len() / 2) as f64 - self.pos
    }
}

fn build_table() -> Vec<f32> {
    let mut table = vec![0.0f32; (PHASES + 1) * TAPS];
    for phase in 0..=PHASES {
        let frac = phase as f64 / PHASES as f64;
        let row = &mut table[phase * TAPS..(phase + 1) * TAPS];
        let mut sum = 0.0f64;
        let mut weights = [0.0f64; TAPS];
        for (k, weight) in weights.iter_mut().enumerate() {
            let x = k as f64 - (HALF - 1) as f64 - frac;
            let t = x / HALF as f64;
            let window = if t.abs() >= 1.0 {
                0.0
            } else {
                let angle = std::f64::consts::PI * t;
                0.42 + 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos()
            };
            *weight = sinc(x * CUTOFF) * window;
            sum += *weight;
        }
        for (slot, weight) in row.iter_mut().zip(weights) {
            *slot = (weight / sum) as f32;
        }
    }
    table
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frames: usize, hz: f64) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (2.0 * std::f64::consts::PI * hz * i as f64 / 48_000.0).sin() as f32 * 0.5;
                [v, -v]
            })
            .collect()
    }

    #[test]
    fn unit_step_keeps_the_signal() {
        let input = sine(4800, 1000.0);
        let mut out = Vec::new();
        Resampler::new().process(&input, 1.0, &mut out);
        assert_eq!(out.len(), input.len() - HALF * 2);
        let error = out
            .iter()
            .zip(&input)
            .skip(HALF * 4)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(error < 2e-3, "error {error}");
    }

    #[test]
    fn step_changes_the_length() {
        let input = sine(48_000, 440.0);
        let mut out = Vec::new();
        Resampler::new().process(&input, 1.0 / 1.005, &mut out);
        let frames = out.len() / 2;
        assert!((48_220..=48_240).contains(&frames), "frames {frames}");
    }

    #[test]
    fn chunks_match_one_pass() {
        let input = sine(9000, 3000.0);
        let mut whole = Vec::new();
        Resampler::new().process(&input, 0.997, &mut whole);

        let mut chunked = Vec::new();
        let mut resampler = Resampler::new();
        for chunk in input.chunks(2 * 737) {
            resampler.process(chunk, 0.997, &mut chunked);
        }
        assert_eq!(whole.len(), chunked.len());
        let error = whole
            .iter()
            .zip(&chunked)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(error < 1e-6, "error {error}");
    }
}
