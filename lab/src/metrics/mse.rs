use crate::emulator::Frame;
use crate::metric::{FrameMetric, Polarity};

/// Largest possible MSE for 8-bit channels: every channel maximally apart (`255²`).
const MAX_MSE: f64 = 255.0 * 255.0; // 65025

/// PSNR reported for byte-identical frames, where the true value is infinite. Sits just above the
/// largest finite 8-bit PSNR (a single 1-LSB pixel difference tops out around 96 dB), so it reads
/// as "no measurable error" without poisoning the average with `inf`.
const PSNR_IDENTICAL: f64 = 100.0;

/// Mean of the squared per-channel differences over R, G and B (alpha ignored).
fn mean_squared_error(reference: &Frame, candidate: &Frame) -> f64 {
    let mut sum = 0.0f64;
    let mut count = 0u64;
    for (a, b) in reference
        .rgba
        .chunks_exact(4)
        .zip(candidate.rgba.chunks_exact(4))
    {
        for c in 0..3 {
            let diff = a[c] as f64 - b[c] as f64;
            sum += diff * diff;
        }
        count += 3;
    }
    if count == 0 { 0.0 } else { sum / count as f64 }
}

/// Mean squared error over the R, G and B channels (alpha ignored).
///
/// Range is `0.0` (identical) to `65025.0` (every channel maximally different). This is an absolute
/// error in squared 8-bit-intensity units, not a normalized ratio — see [`Nmse`] for a 0..1 version
/// and [`Psnr`] for the decibel form.
pub struct Mse;

impl FrameMetric for Mse {
    fn name(&self) -> &str {
        "mse"
    }

    fn polarity(&self) -> Polarity {
        Polarity::LowerIsBetter
    }

    fn compare(&self, reference: &Frame, candidate: &Frame) -> f64 {
        mean_squared_error(reference, candidate)
    }
}

/// Normalized mean squared error: [`Mse`] divided by the maximum possible MSE (`255²`).
///
/// Range is `0.0` (identical) to `1.0` (every channel maximally different).
pub struct Nmse;

impl FrameMetric for Nmse {
    fn name(&self) -> &str {
        "nmse"
    }

    fn polarity(&self) -> Polarity {
        Polarity::LowerIsBetter
    }

    fn compare(&self, reference: &Frame, candidate: &Frame) -> f64 {
        mean_squared_error(reference, candidate) / MAX_MSE
    }
}

/// Peak signal-to-noise ratio in decibels: `10·log₁₀(255² / MSE)`.
///
/// Higher is more similar. Byte-identical frames (MSE = 0, PSNR = ∞) are reported as
/// [`PSNR_IDENTICAL`] dB so the metric stays finite and averageable.
pub struct Psnr;

impl FrameMetric for Psnr {
    fn name(&self) -> &str {
        "psnr"
    }

    fn polarity(&self) -> Polarity {
        Polarity::HigherIsBetter
    }

    fn unit(&self) -> &str {
        "dB"
    }

    fn compare(&self, reference: &Frame, candidate: &Frame) -> f64 {
        let mse = mean_squared_error(reference, candidate);
        if mse <= 0.0 {
            PSNR_IDENTICAL
        } else {
            10.0 * (MAX_MSE / mse).log10()
        }
    }
}
