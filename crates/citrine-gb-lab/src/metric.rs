use crate::emulator::Frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Polarity {
    HigherIsBetter,
    LowerIsBetter,
}

/// A scalar measure of similarity between a reference frame and a candidate frame.
pub trait FrameMetric {
    /// Column header in reports, e.g. "exact", "mse", "ssim".
    fn name(&self) -> &str;

    fn polarity(&self) -> Polarity;

    /// Empty for dimensionless metrics.
    fn unit(&self) -> &str {
        ""
    }

    /// Both frames are guaranteed to have equal dimensions.
    fn compare(&self, reference: &Frame, candidate: &Frame) -> f64;
}
