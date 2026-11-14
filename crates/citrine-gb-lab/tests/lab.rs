use citrine_gb::console::{ConsoleKind, Screen, VideoSpec};
use citrine_gb::gb::GbModel;
use citrine_gb_test_roms as test_roms;
use citrine_lab::emulator::{Frame, FrameEmulator};
use citrine_lab::emulators::{
    CitrineEmulator, CitrineNdsEmulator, DS_CYCLES_PER_FRAME, DS_SAMPLE_RATE, DS_VIDEO, DsBios,
    MelonDsEmulator, MgbaEmulator, SameBoyEmulator,
};
use citrine_lab::metric::FrameMetric;
use citrine_lab::metrics::{ExactFrame, ExactPixelRatio, Mse, Nmse, Psnr, Ssim};
use citrine_lab::recording::Recording;
use citrine_lab::runner::{replay, run_alone, run_loaded};
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
    let test = "replay_produces_requested_frame_count_on_both_emulators";
    let (Some(rom), Some(boot)) = (
        test_roms::read(test, roms_dir().join("test/dmg-acid2.gb")),
        test_roms::read(test, roms_dir().join("boot/dmg_boot.bin")),
    ) else {
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
    const SCREEN: VideoSpec = VideoSpec::of(&[Screen::whole(160, 144)]);
    let screen = SCREEN;
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
    const SCREEN: VideoSpec = VideoSpec::of(&[Screen::whole(240, 160)]);
    let rom = idle_gba_rom();
    assert_eq!(
        MgbaEmulator::new()
            .load(&rom, None, ConsoleKind::Gba)
            .unwrap(),
        SCREEN
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

fn idle_nds_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x1000];
    let header = [
        (0x20, 0x200),
        (0x24, 0x0200_0000),
        (0x28, 0x0200_0000),
        (0x2C, 4),
        (0x30, 0x400),
        (0x34, 0x0238_0000),
        (0x38, 0x0238_0000),
        (0x3C, 4),
        (0x80, 0x1000),
        (0x84, 0x4000),
        (0x200, 0xEAFF_FFFE),
        (0x400, 0xEAFF_FFFE),
    ];
    for (at, value) in header {
        rom[at..at + 4].copy_from_slice(&u32::to_le_bytes(value));
    }
    rom
}

fn itcm_stack_nds_rom() -> Vec<u8> {
    let mut rom = idle_nds_rom();
    let code: [u32; 15] = [
        0xE3A0_3404,
        0xE283_3FC1,
        0xE3A0_0C80,
        0xE380_0003,
        0xE1C3_00B0,
        0xE3A0_3404,
        0xE3A0_0801,
        0xE583_0000,
        0xE3A0_2880,
        0xE382_2C3F,
        0xE3A0_001F,
        0xE582_0000,
        0xE592_1000,
        0xE3A0_3405,
        0xE1C3_10B0,
    ];
    let mut at = 0x200;
    for word in code.into_iter().chain([0xEAFF_FFFE]) {
        rom[at..at + 4].copy_from_slice(&word.to_le_bytes());
        at += 4;
    }
    rom[0x2C..0x30].copy_from_slice(&64u32.to_le_bytes());
    rom
}

fn booted_melonds() -> MelonDsEmulator {
    let mut melonds = MelonDsEmulator::new();
    assert_eq!(
        melonds.load_nds(&idle_nds_rom(), DsBios::FREE).unwrap(),
        DS_VIDEO
    );
    melonds
}

#[test]
fn melonds_boots_a_ds_game_with_freebios_onto_two_stacked_screens() {
    let mut melonds = booted_melonds();
    assert!(melonds.step());
    let frame = melonds.frame(DS_VIDEO);
    assert_eq!((frame.width, frame.height), (256, 384));
    assert_eq!(frame.rgba.len(), 256 * 384 * 4);
}

#[test]
fn melonds_frames_hold_only_rgb666_colours() {
    let mut melonds = booted_melonds();
    for _ in 0..3 {
        melonds.step();
    }
    let frame = melonds.frame(DS_VIDEO);
    for px in frame.rgba.as_chunks::<4>().0 {
        for &c in &px[..3] {
            assert_eq!(c, (c & 0xFC) | (c >> 6), "{px:?}");
        }
        assert_eq!(px[3], 0xFF);
    }
}

#[test]
fn melonds_refuses_every_citrine_console_and_a_short_bios() {
    let rom = idle_nds_rom();
    for console in [ConsoleKind::Gb, ConsoleKind::Gbc, ConsoleKind::Gba] {
        assert!(MelonDsEmulator::new().load(&rom, None, console).is_err());
    }
    let short = DsBios {
        arm9: &DsBios::FREE.arm9[..0x800],
        arm7: DsBios::FREE.arm7,
    };
    assert!(MelonDsEmulator::new().load_nds(&rom, short).is_err());
}

#[test]
fn melonds_finishes_a_frame_every_560190_cycles() {
    let mut melonds = booted_melonds();
    let mut ends = Vec::new();
    while ends.len() < 3 {
        if melonds.step() {
            ends.push(melonds.total_cycles());
        }
    }
    assert!(
        (ends[2] - ends[1]).abs_diff(DS_CYCLES_PER_FRAME) < 1_000,
        "{ends:?}"
    );
}

#[test]
fn melonds_gives_a_second_of_stereo_audio_for_a_second_of_frames() {
    let mut melonds = booted_melonds();
    let mut samples = Vec::new();
    for _ in 0..60 {
        melonds.step();
        melonds.drain_audio(&mut samples);
    }
    let frames = samples.len() / 2;
    let expected = DS_SAMPLE_RATE as usize * 60 * 560_190 / 33_513_982;
    assert!(frames.abs_diff(expected) < 2_000, "{frames} vs {expected}");
}

#[test]
fn a_ds_run_records_melonds_frames_alone() {
    let mut melonds = booted_melonds();
    let mut sizes = Vec::new();
    let frames = run_loaded(
        &mut melonds,
        DS_VIDEO,
        &Recording::new("", GbModel::Dmg),
        5,
        |_, frame| {
            sizes.push((frame.width, frame.height, frame.rgba.len()));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(frames, 5);
    assert_eq!(sizes, vec![(256, 384, 256 * 384 * 4); 5]);
}

#[test]
fn melonds_direct_boot_keeps_a_stack_in_the_itcm() {
    let mut melonds = MelonDsEmulator::new();
    melonds
        .load_nds(&itcm_stack_nds_rom(), DsBios::FREE)
        .unwrap();
    for _ in 0..3 {
        melonds.step();
    }
    let frame = melonds.frame(DS_VIDEO);
    let red = frame
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[0] > 0xF0 && px[1] == 0 && px[2] == 0)
        .count();
    assert_eq!(red, 256 * 192);
}

#[test]
fn citrine_and_melonds_show_the_same_frames_of_an_idle_ds_game() {
    let mut melonds = booted_melonds();
    let mut citrine = CitrineNdsEmulator::new();
    assert_eq!(citrine.load_nds(&idle_nds_rom()).unwrap(), DS_VIDEO);
    let (mut expected, mut actual) = (Frame::new(0, 0, Vec::new()), Frame::new(0, 0, Vec::new()));
    for _ in 0..3 {
        melonds.step();
        citrine.step();
        melonds.frame(DS_VIDEO).rgb555_into(&mut expected);
        citrine.frame(DS_VIDEO).rgb555_into(&mut actual);
        assert!(expected.rgba == actual.rgba);
    }
    assert!(citrine.total_cycles() >= 2 * DS_CYCLES_PER_FRAME);
}

#[test]
fn sameboy_without_a_boot_rom_reads_the_game_where_the_boot_rom_was() {
    let rom = homebrew("tobu.gb");
    for console in [ConsoleKind::Gb, ConsoleKind::Gbc] {
        let mut sameboy = SameBoyEmulator::new();
        sameboy.load(&rom, None, console).unwrap();
        let read: Vec<u8> = (0..0x900).map(|addr| sameboy.peek(addr)).collect();
        assert_eq!(read, rom[..0x900], "{console:?}");
    }
}

#[test]
fn sameboy_without_a_boot_rom_sets_the_dmg_flags_from_the_header_checksum() {
    let mut rom = homebrew("tobu.gb");
    for (checksum, af) in [(0x2A, 0x01B0), (0x00, 0x0180)] {
        rom[0x14D] = checksum;
        let mut sameboy = SameBoyEmulator::new();
        sameboy.load(&rom, None, ConsoleKind::Gb).unwrap();
        assert_eq!(sameboy.registers().af, af, "checksum {checksum:02X}");
    }
}

#[test]
fn sameboy_without_a_boot_rom_matches_a_booted_run_from_the_hand_off() {
    let test = "sameboy_without_a_boot_rom_matches_a_booted_run_from_the_hand_off";
    let boot = |file: &str| test_roms::read(test, roms_dir().join("boot").join(file));
    let (Some(dmg), Some(cgb)) = (boot("dmg_boot.bin"), boot("cgb_boot.bin")) else {
        return;
    };
    const VIDEO: VideoSpec = VideoSpec::of(&[Screen::whole(160, 144)]);
    for (name, console, boot_rom) in [
        ("tobu.gb", ConsoleKind::Gb, &dmg),
        ("tobu.gb", ConsoleKind::Gbc, &cgb),
        ("ucity.gbc", ConsoleKind::Gbc, &cgb),
    ] {
        let rom = homebrew(name);
        let mut booted = SameBoyEmulator::new();
        booted.load(&rom, Some(boot_rom), console).unwrap();
        while booted.registers().pc != 0x100 {
            booted.step();
        }
        let mut cold = SameBoyEmulator::new();
        cold.load(&rom, None, console).unwrap();
        assert_eq!(booted.registers(), cold.registers(), "{name} {console:?}");
        for addr in (0x8000..0xA000).chain(0xFE00..0xFEA0).chain(0xFF00..0xFF80) {
            assert_eq!(
                booted.peek(addr),
                cold.peek(addr),
                "{name} {console:?} {addr:04X}"
            );
        }
        let start = booted.total_cycles();
        let frames = |e: &mut SameBoyEmulator, start: u64| {
            let mut out = Vec::new();
            while out.len() < 300 {
                if e.step() {
                    out.push((e.total_cycles() - start, e.frame(VIDEO).rgba));
                }
            }
            out
        };
        let (b, c) = (frames(&mut booted, start), frames(&mut cold, 0));
        for (i, ((bc, bf), (cc, cf))) in b.iter().zip(&c).enumerate() {
            assert!(
                bc.abs_diff(*cc) <= 16,
                "{name} {console:?} frame {i}: {bc} vs {cc}"
            );
            assert!(bf == cf, "{name} {console:?} frame {i}");
        }
    }
}
