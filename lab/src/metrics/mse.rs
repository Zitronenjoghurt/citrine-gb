use crate::emulator::Frame;
use crate::metric::{FrameMetric, Polarity};

const MAX_MSE: f64 = 255.0 * 255.0;

/// Stand-in for the infinite PSNR of identical frames. Sits just above the largest finite 8-bit
/// PSNR (~96 dB for a single 1-LSB difference), so averages stay meaningful.
const PSNR_IDENTICAL: f64 = 100.0;

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

/// Absolute error over R, G and B in squared 8-bit intensity units, `0.0` to `65025.0`.
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

/// [`Mse`] divided by the maximum possible MSE, `0.0` (identical) to `1.0`.
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

/// Peak signal-to-noise ratio in decibels, `10·log₁₀(255² / MSE)`.
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
