//! Citrine Lab: an interfaced harness for experimenting with the Citrine Game Boy emulator.
//!
//! The first experiment is a frame-by-frame comparison of Citrine against the reference emulator
//! SameBoy, driven by recorded input traces and scored with image metrics.
//!
//! Everything is built around three traits so experiments can swap implementations freely:
//! - [`emulator::FrameEmulator`] — anything that can be stepped, fed input, and produce frames.
//! - [`metric::FrameMetric`] — a scalar similarity measure between two frames.
//! - [`report::Reporter`] — a way to render a comparison result.

pub mod emulator;
pub mod emulators;
pub mod metric;
pub mod metrics;
pub mod recording;
pub mod report;
pub mod runner;
