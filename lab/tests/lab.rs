use citrine_gb::gb::GbModel;
use citrine_lab::emulator::{Frame, SCREEN_HEIGHT, SCREEN_WIDTH};
use citrine_lab::emulators::{CitrineEmulator, SameBoyEmulator};
use citrine_lab::metric::FrameMetric;
use citrine_lab::metrics::{ExactFrame, ExactPixelRatio, Mse, Nmse, Psnr, Ssim};
use citrine_lab::recording::Recording;
use citrine_lab::runner::replay;
use std::path::PathBuf;

fn solid(width: usize, height: usize, rgb: [u8; 3]) -> Frame {
    let mut rgba = Vec::with_capacity(width * height * 4);
    for _ in 0..width * height {
        rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 0xFF]);
    }
    Frame::new(width, height, rgba)
}

#[test]
fn metrics_identical_frames() {
    let a = solid(8, 8, [10, 20, 30]);
    let b = a.clone();

    assert_eq!(ExactFrame.compare(&a, &b), 1.0);
    assert_eq!(ExactPixelRatio.compare(&a, &b), 1.0);
    assert_eq!(Mse.compare(&a, &b), 0.0);
    assert_eq!(Nmse.compare(&a, &b), 0.0);
    assert!(Psnr.compare(&a, &b) >= 100.0);
    assert!((Ssim::default().compare(&a, &b) - 1.0).abs() < 1e-9);
}

#[test]
fn metrics_fully_different_frames() {
    let black = solid(8, 8, [0, 0, 0]);
    let white = solid(8, 8, [255, 255, 255]);

    assert_eq!(ExactFrame.compare(&black, &white), 0.0);
    assert_eq!(ExactPixelRatio.compare(&black, &white), 0.0);
    assert!((Mse.compare(&black, &white) - 65025.0).abs() < 1e-6);
    assert!((Nmse.compare(&black, &white) - 1.0).abs() < 1e-9);
    assert!(Psnr.compare(&black, &white).abs() < 1e-9);
    assert!(Ssim::default().compare(&black, &white) < 0.1);
}

#[test]
fn px_match_counts_partial_overlap() {
    let a = solid(2, 1, [0, 0, 0]);
    let mut b = solid(2, 1, [0, 0, 0]);
    b.rgba[4] = 255;
    assert_eq!(ExactPixelRatio.compare(&a, &b), 0.5);
}

#[test]
fn canonical_greyscale_collapses_palette() {
    let yellow = solid(4, 4, [0xF2, 0xCE, 0x44]);
    let green = solid(4, 4, [0x88, 0xC0, 0x70]);
    let na = yellow.to_canonical_greyscale();
    let nb = green.to_canonical_greyscale();
    assert_eq!(na.rgba, nb.rgba);
}

fn roms_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../roms")
}

#[test]
fn replay_produces_requested_frame_count_on_both_emulators() {
    let rom = std::fs::read(roms_dir().join("test/dmg-acid2.gb")).expect("test ROM present");
    let boot = std::fs::read(roms_dir().join("boot/dmg_boot.bin")).expect("boot ROM present");
    let recording = Recording::new("", GbModel::Dmg);
    let frames = 30;

    let mut citrine = CitrineEmulator::new();
    let cf = replay(&mut citrine, &rom, Some(&boot), &recording, frames).unwrap();
    assert_eq!(cf.len(), frames);
    assert_eq!(cf[0].width, SCREEN_WIDTH);
    assert_eq!(cf[0].height, SCREEN_HEIGHT);

    let mut sameboy = SameBoyEmulator::new();
    let sf = replay(&mut sameboy, &rom, Some(&boot), &recording, frames).unwrap();
    assert_eq!(sf.len(), frames);
    assert_eq!(sf[0].rgba.len(), SCREEN_WIDTH * SCREEN_HEIGHT * 4);

    let c0 = cf[0].to_canonical_greyscale();
    let s0 = sf[0].to_canonical_greyscale();
    assert_eq!(c0.rgba, s0.rgba, "first boot frame should match");
}
