#![cfg(feature = "persistence")]

use citrine_gb::ReadMemory;
use citrine_gb::console::{Console, Keys};
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::rom::Rom;
use citrine_gb_test_roms as test_roms;
use std::path::PathBuf;

const CHECKPOINTS: [(usize, [u8; 2], u8); 11] = [
    (94, [0x00, 0x00], 0),
    (300, [0xBD, 0x88], 0),
    (500, [0xB0, 0xD2], 64),
    (1000, [0x90, 0x31], 0),
    (1500, [0xE4, 0x82], 0),
    (1800, [0x12, 0x8A], 0),
    (2000, [0xCD, 0x43], 0),
    (2090, [0xB7, 0x1C], 16),
    (2093, [0x73, 0x5D], 16),
    (2200, [0x77, 0x7C], 16),
    (2399, [0x8D, 0xA8], 0),
];

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("home"))
}

fn env_path(var: &str, default: PathBuf) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or(default)
}

fn timeline() -> Vec<Keys> {
    let mut keys = Vec::new();
    keys.extend((0..1500).map(|i| if i % 20 < 4 { Keys::A } else { Keys::empty() }));
    keys.extend((0..240).map(|i| if i % 20 < 4 { Keys::B } else { Keys::empty() }));
    keys.extend(std::iter::repeat_n(Keys::empty(), 60));
    for direction in [
        Keys::RIGHT,
        Keys::LEFT,
        Keys::DOWN,
        Keys::UP,
        Keys::LEFT,
        Keys::RIGHT,
        Keys::UP,
        Keys::DOWN,
    ] {
        keys.extend(std::iter::repeat_n(direction, 96));
        keys.extend(std::iter::repeat_n(Keys::empty(), 40));
    }
    keys
}

#[test]
#[ignore]
fn red_rng_matches_sameboy_through_the_first_encounter() {
    let rom = env_path("CITRINE_ROMS", home().join("roms/gb"))
        .join("Pokemon - Red Version (USA, Europe).gb");
    let save = env_path("CITRINE_SAVES", home().join("roms/saves")).join("blue.sav");
    let boot = env_path("CITRINE_DMG_BOOT", home().join("roms/bios/dmg_boot.bin"));
    let test = "red_rng_matches_sameboy_through_the_first_encounter";
    let (Some(rom), Some(save), Some(boot)) = (
        test_roms::read(test, rom),
        test_roms::read(test, save),
        test_roms::read(test, boot),
    ) else {
        return;
    };

    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_boot_rom(&boot);
    gb.load_rom(&Rom::new(&rom)).expect("load");
    let battery = gb.battery_from_file(&save);
    gb.put_battery(battery, 1_700_000_000);

    let mut seen = Vec::new();
    for (frame, keys) in timeline().into_iter().enumerate().take(2400) {
        gb.set_keys(keys);
        gb.run_frame();
        if let Some(&(_, rng, scy)) = CHECKPOINTS.iter().find(|(at, _, _)| *at == frame) {
            let got = [gb.memory.read_naive(0xFFD3), gb.memory.read_naive(0xFFD4)];
            seen.push((frame, got, gb.ppu.scy, rng, scy));
        }
    }
    let wrong: Vec<_> = seen
        .iter()
        .filter(|(_, got, got_scy, rng, scy)| got != rng || got_scy != scy)
        .collect();
    assert_eq!(seen.len(), CHECKPOINTS.len());
    assert!(wrong.is_empty(), "{wrong:02X?}");
}
