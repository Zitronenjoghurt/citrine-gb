mod exact;
mod mse;
mod ssim;

pub use exact::{ExactFrame, ExactPixelRatio};
pub use mse::{Mse, Nmse, Psnr};
pub use ssim::Ssim;

use crate::metric::FrameMetric;

/// Resolve a metric by the short name used on the CLI.
pub fn by_name(name: &str) -> Option<Box<dyn FrameMetric>> {
    match name {
        "exact" => Some(Box::new(ExactFrame)),
        "px_match" => Some(Box::new(ExactPixelRatio)),
        "mse" => Some(Box::new(Mse)),
        "nmse" => Some(Box::new(Nmse)),
        "psnr" => Some(Box::new(Psnr)),
        "ssim" => Some(Box::new(Ssim::default())),
        _ => None,
    }
}
