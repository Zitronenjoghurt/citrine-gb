//! The frame-comparison metric abstraction.

use crate::emulator::Frame;

/// Higher score = more similar, or lower = more similar?
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Polarity {
    /// Larger values mean the frames are more similar (e.g. SSIM, Exact-Frame).
    HigherIsBetter,
    /// Smaller values mean the frames are more similar (e.g. MSE).
    LowerIsBetter,
}

/// A scalar measure of similarity between a reference frame and a candidate frame.
///
/// Implement this to add a new metric; the comparison engine and reporters work against the trait
/// and never need to know the concrete metric.
pub trait FrameMetric {
    /// Short identifier used as a column header in reports (e.g. "exact", "mse", "ssim").
    fn name(&self) -> &str;

    /// Whether higher or lower scores indicate greater similarity.
    fn polarity(&self) -> Polarity;

    /// Unit of the score, for display (e.g. `"dB"` for PSNR). Empty for dimensionless metrics.
    fn unit(&self) -> &str {
        ""
    }

    /// Compare `reference` against `candidate`. Both are guaranteed to have equal dimensions.
    fn compare(&self, reference: &Frame, candidate: &Frame) -> f64;
}
