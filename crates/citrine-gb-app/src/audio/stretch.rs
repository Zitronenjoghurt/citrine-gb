const SEGMENT_SECS: f64 = 0.04;
const SEEK_SECS: f64 = 0.012;
const COARSE: usize = 4;

pub struct Stretch {
    half: usize,
    seek: usize,
    window: Vec<f32>,
    input: Vec<f32>,
    pos: f64,
    natural: Option<usize>,
    tail: Vec<f32>,
}

impl Stretch {
    pub fn new(sample_rate: u32) -> Self {
        let half = ((sample_rate as f64 * SEGMENT_SECS / 2.0) as usize).max(COARSE * 4);
        let seek = (sample_rate as f64 * SEEK_SECS) as usize;
        let window = (0..half * 2)
            .map(|i| {
                let angle = std::f64::consts::TAU * i as f64 / (half * 2) as f64;
                (0.5 - 0.5 * angle.cos()) as f32
            })
            .collect();
        Self {
            half,
            seek,
            window,
            input: Vec::new(),
            pos: 0.0,
            natural: None,
            tail: vec![0.0; half * 2],
        }
    }

    pub fn process(&mut self, input: &[f32], tempo: f64, out: &mut Vec<f32>) {
        self.input.extend_from_slice(&input[..input.len() & !1]);
        let frames = self.input.len() / 2;
        let half = self.half;
        loop {
            let nominal = self.pos as usize;
            let start = match self.natural {
                None => {
                    if nominal + half * 2 > frames {
                        break;
                    }
                    out.extend_from_slice(&self.input[nominal * 2..(nominal + half) * 2]);
                    nominal
                }
                Some(natural) => {
                    if nominal + self.seek + half * 2 > frames || natural + half * 2 > frames {
                        break;
                    }
                    let start = self.best_start(natural, nominal);
                    for i in 0..half * 2 {
                        out.push(self.tail[i] + self.input[start * 2 + i] * self.window[i / 2]);
                    }
                    start
                }
            };
            for i in 0..half * 2 {
                self.tail[i] = self.input[(start + half) * 2 + i] * self.window[half + i / 2];
            }
            self.natural = Some(start + half);
            self.pos += half as f64 * tempo.clamp(0.25, 1.0);
        }

        let keep = (self.pos as usize)
            .saturating_sub(self.seek)
            .min(self.natural.unwrap_or(usize::MAX))
            .min(frames);
        self.input.drain(..keep * 2);
        self.pos -= keep as f64;
        if let Some(natural) = &mut self.natural {
            *natural -= keep;
        }
    }

    pub fn flush(&mut self, out: &mut Vec<f32>) {
        let from = match self.natural.take() {
            Some(natural) => natural * 2,
            None => self.pos as usize * 2,
        };
        out.extend_from_slice(&self.input[from.min(self.input.len())..]);
        self.input.clear();
        self.pos = 0.0;
    }

    fn best_start(&self, natural: usize, nominal: usize) -> usize {
        let low = nominal.saturating_sub(self.seek);
        let high = nominal + self.seek;
        let mut best = (self.similarity(natural, nominal), nominal);
        let look = |candidate: usize, best: &mut (f32, usize)| {
            let score = self.similarity(natural, candidate);
            if score > best.0 {
                *best = (score, candidate);
            }
        };
        for candidate in (low..=high).step_by(COARSE) {
            look(candidate, &mut best);
        }
        let around = best.1;
        for candidate in
            around.saturating_sub(COARSE - 1).max(low)..=(around + COARSE - 1).min(high)
        {
            look(candidate, &mut best);
        }
        best.1
    }

    fn similarity(&self, a: usize, b: usize) -> f32 {
        let mut dot = 0.0f32;
        let mut energy = 0.0f32;
        for i in (0..self.half).step_by(2) {
            let x = self.input[(a + i) * 2] + self.input[(a + i) * 2 + 1];
            let y = self.input[(b + i) * 2] + self.input[(b + i) * 2 + 1];
            dot += x * y;
            energy += y * y;
        }
        dot / (energy + 1e-9).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn tone(frames: usize, hz: f64) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (i as f64 * hz / RATE as f64 * std::f64::consts::TAU).sin() as f32 * 0.5;
                [v, v]
            })
            .collect()
    }

    fn crossings(samples: &[f32]) -> usize {
        samples
            .chunks(2)
            .map(|f| f[0])
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count()
    }

    #[test]
    fn slowing_down_keeps_the_pitch() {
        for tempo in [1.0, 0.85, 0.7] {
            let mut stretch = Stretch::new(RATE);
            let mut out = Vec::new();
            for chunk in tone(RATE as usize * 2, 440.0).chunks(1600) {
                stretch.process(chunk, tempo, &mut out);
            }
            let frames = out.len() / 2;
            let want = RATE as f64 * 2.0 / tempo;
            assert!(
                (frames as f64 - want).abs() < want * 0.03,
                "{tempo}: {frames} frames"
            );
            let hz = crossings(&out) as f64 * RATE as f64 / frames as f64;
            assert!((hz - 440.0).abs() < 3.0, "{tempo}: {hz} Hz");
            let peak = out[RATE as usize..]
                .iter()
                .fold(0.0f32, |m, v| m.max(v.abs()));
            assert!((peak - 0.5).abs() < 0.05, "{tempo}: peak {peak}");
        }
    }

    #[test]
    fn tempo_one_passes_the_input_through() {
        let input = tone(RATE as usize, 300.0);
        let mut stretch = Stretch::new(RATE);
        let mut out = Vec::new();
        for chunk in input.chunks(800) {
            stretch.process(chunk, 1.0, &mut out);
        }
        stretch.flush(&mut out);
        assert_eq!(out.len(), input.len());
        let worst = out
            .iter()
            .zip(&input)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-3, "worst {worst}");
    }
}
