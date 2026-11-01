use crate::emulator::Frame;
use crate::metric::{FrameMetric, Polarity};

/// Structural Similarity Index over luminance in non-overlapping windows, roughly `-1.0..=1.0`
/// with `1.0` meaning structurally identical. Uses the standard 8-bit constants.
pub struct Ssim {
    window: usize,
}

impl Default for Ssim {
    fn default() -> Self {
        Self { window: 8 }
    }
}

const C1: f64 = (0.01 * 255.0) * (0.01 * 255.0);
const C2: f64 = (0.03 * 255.0) * (0.03 * 255.0);

#[inline]
fn luma(p: &[u8]) -> f64 {
    0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64
}

impl FrameMetric for Ssim {
    fn name(&self) -> &str {
        "ssim"
    }

    fn polarity(&self) -> Polarity {
        Polarity::HigherIsBetter
    }

    fn compare(&self, reference: &Frame, candidate: &Frame) -> f64 {
        let (w, h) = (reference.width, reference.height);
        let win = self.window;
        let mut total = 0.0f64;
        let mut windows = 0u64;

        let mut y = 0;
        while y < h {
            let mut x = 0;
            while x < w {
                let bw = win.min(w - x);
                let bh = win.min(h - y);
                let n = (bw * bh) as f64;

                // One streaming pass: accumulate Σa, Σb, Σa², Σb², Σab, deriving the moments from
                // them. Luma is computed inline, so no luma plane is allocated.
                let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0, 0.0, 0.0, 0.0, 0.0);
                for dy in 0..bh {
                    let row = (y + dy) * w + x;
                    for dx in 0..bw {
                        let off = (row + dx) * 4;
                        let av = luma(&reference.rgba[off..off + 4]);
                        let bv = luma(&candidate.rgba[off..off + 4]);
                        sa += av;
                        sb += bv;
                        saa += av * av;
                        sbb += bv * bv;
                        sab += av * bv;
                    }
                }

                let (ma, mb) = (sa / n, sb / n);
                let denom = (n - 1.0).max(1.0);
                let va = (saa - sa * ma) / denom;
                let vb = (sbb - sb * mb) / denom;
                let cov = (sab - sa * mb) / denom;

                let numerator = (2.0 * ma * mb + C1) * (2.0 * cov + C2);
                let denominator = (ma * ma + mb * mb + C1) * (va + vb + C2);
                total += numerator / denominator;
                windows += 1;

                x += win;
            }
            y += win;
        }

        if windows == 0 {
            1.0
        } else {
            total / windows as f64
        }
    }
}
