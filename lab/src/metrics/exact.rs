use crate::emulator::Frame;
use crate::metric::{FrameMetric, Polarity};

/// 1.0 if the frames are byte-for-byte identical, 0.0 otherwise.
pub struct ExactFrame;

impl FrameMetric for ExactFrame {
    fn name(&self) -> &str {
        "exact"
    }

    fn polarity(&self) -> Polarity {
        Polarity::HigherIsBetter
    }

    fn compare(&self, reference: &Frame, candidate: &Frame) -> f64 {
        if reference.rgba == candidate.rgba {
            1.0
        } else {
            0.0
        }
    }
}

/// Fraction of pixels (0.0..=1.0) that are exactly equal across all channels.
pub struct ExactPixelRatio;

impl FrameMetric for ExactPixelRatio {
    fn name(&self) -> &str {
        "px_match"
    }

    fn polarity(&self) -> Polarity {
        Polarity::HigherIsBetter
    }

    fn compare(&self, reference: &Frame, candidate: &Frame) -> f64 {
        let total = reference.width * reference.height;
        if total == 0 {
            return 1.0;
        }
        let matching = reference
            .rgba
            .chunks_exact(4)
            .zip(candidate.rgba.chunks_exact(4))
            .filter(|(a, b)| a == b)
            .count();
        matching as f64 / total as f64
    }
}
