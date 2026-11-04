use citrine_gb::console::{ConsoleKind, VideoSpec};
use citrine_gb::gb::GbModel;
use citrine_lab::emulator::{Frame, FrameEmulator};
use citrine_lab::emulators::{CitrineEmulator, MgbaEmulator, SameBoyEmulator};
use citrine_lab::metric::FrameMetric;
use citrine_lab::metrics::{ExactFrame, ExactPixelRatio, Mse, Nmse, Psnr, Ssim};
use citrine_lab::recording::Recording;
use citrine_lab::runner::{replay, run_alone};
use std::io::Write;
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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../roms")
}

#[test]
fn replay_produces_requested_frame_count_on_both_emulators() {
    let (Ok(rom), Ok(boot)) = (
        std::fs::read(roms_dir().join("test/dmg-acid2.gb")),
        std::fs::read(roms_dir().join("boot/dmg_boot.bin")),
    ) else {
        let _ = writeln!(
            std::io::stderr(),
            "\nskipped replay_produces_requested_frame_count_on_both_emulators: it needs roms/test/dmg-acid2.gb and your own DMG boot ROM at roms/boot/dmg_boot.bin"
        );
        return;
    };
    let recording = Recording::new("", GbModel::Dmg);
    let frames = 30;

    let mut citrine = CitrineEmulator::new();
    let cf = replay(&mut citrine, &rom, Some(&boot), &recording, frames).unwrap();
    assert_eq!(cf.len(), frames);
    assert_eq!(cf[0].width, 160);
    assert_eq!(cf[0].height, 144);

    let mut sameboy = SameBoyEmulator::new();
    let sf = replay(&mut sameboy, &rom, Some(&boot), &recording, frames).unwrap();
    assert_eq!(sf.len(), frames);
    assert_eq!(sf[0].rgba.len(), 160 * 144 * 4);

    let c0 = cf[0].to_canonical_greyscale();
    let s0 = sf[0].to_canonical_greyscale();
    assert_eq!(c0.rgba, s0.rgba, "first boot frame should match");
}

fn homebrew(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../citrine-gb-app/homebrew")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn both_emulators_report_the_game_boy_screen_when_they_load_a_game() {
    let rom = homebrew("tobu.gb");
    let screen = VideoSpec {
        width: 160,
        height: 144,
    };
    for console in [ConsoleKind::Gb, ConsoleKind::Gbc] {
        assert_eq!(
            CitrineEmulator::new().load(&rom, None, console).unwrap(),
            screen
        );
        assert_eq!(
            SameBoyEmulator::new().load(&rom, None, console).unwrap(),
            screen
        );
    }
}

#[test]
fn neither_emulator_loads_a_gba_game() {
    let rom = homebrew("tobu.gb");
    assert!(
        CitrineEmulator::new()
            .load(&rom, None, ConsoleKind::Gba)
            .is_err()
    );
    assert!(
        SameBoyEmulator::new()
            .load(&rom, None, ConsoleKind::Gba)
            .is_err()
    );
}

#[test]
fn rgb555_ignores_only_the_low_three_bits() {
    let a = solid(4, 4, [0xF8, 0x08, 0x10]);
    let b = solid(4, 4, [0xFF, 0x0F, 0x17]);
    let c = solid(4, 4, [0xF0, 0x08, 0x10]);
    let canonical = Frame::canonical_for(ConsoleKind::Gba);
    let (mut na, mut nb, mut nc) = (a.clone(), b.clone(), c.clone());
    canonical(&a, &mut na);
    canonical(&b, &mut nb);
    canonical(&c, &mut nc);
    assert_eq!(na.rgba, nb.rgba);
    assert_ne!(na.rgba, nc.rgba);
}

fn idle_gba_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x200];
    rom[0..4].copy_from_slice(&0xEA00_002Eu32.to_le_bytes());
    rom[0xB2] = 0x96;
    rom[0xC0..0xC4].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
    rom
}

#[test]
fn mgba_reports_the_gba_screen_and_refuses_game_boy_consoles() {
    let rom = idle_gba_rom();
    assert_eq!(
        MgbaEmulator::new()
            .load(&rom, None, ConsoleKind::Gba)
            .unwrap(),
        VideoSpec {
            width: 240,
            height: 160
        }
    );
    assert!(
        MgbaEmulator::new()
            .load(&rom, None, ConsoleKind::Gb)
            .is_err()
    );
}

#[test]
fn mgba_finishes_a_frame_every_280896_cycles() {
    let mut mgba = MgbaEmulator::new();
    mgba.load(&idle_gba_rom(), None, ConsoleKind::Gba).unwrap();
    let mut ends = Vec::new();
    while ends.len() < 3 {
        if mgba.step() {
            ends.push(mgba.total_cycles());
        }
    }
    assert!((ends[2] - ends[1]).abs_diff(280_896) < 1_000, "{ends:?}");
}

#[test]
fn a_gba_run_without_a_gba_core_records_mgba_frames_alone() {
    let rom = idle_gba_rom();
    assert!(
        CitrineEmulator::new()
            .load(&rom, None, ConsoleKind::Gba)
            .is_err()
    );
    let mut sizes = Vec::new();
    let frames = run_alone(
        &mut MgbaEmulator::new(),
        &rom,
        None,
        &Recording::new("", GbModel::Dmg),
        ConsoleKind::Gba,
        5,
        |_, frame| {
            sizes.push((frame.width, frame.height, frame.rgba.len()));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(frames, 5);
    assert_eq!(sizes, vec![(240, 160, 240 * 160 * 4); 5]);
}
