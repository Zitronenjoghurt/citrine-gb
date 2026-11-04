use crate::app::ui_state::keybinds::{GbButton, PadButton, Shoulder};
use crate::app::ui_state::settings::Settings;
use crate::media::{MediaKind, MediaStore};
use crate::storage::{SaveStore, SettingsStore};
use citrine_gb::console::{Console, ConsoleKind, Keys};
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::machine::{Firmware, Machine};
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::persistence::state::state_kind;
use citrine_gb::recording::{Button, Recording};
use citrine_gb::rom::Rom;
use citrine_gb::{ReadMemory, WriteMemory};
use std::path::Path;

const TOBU: &[u8] = include_bytes!("../homebrew/tobu.gb");
const SNAPSHOT: &[u8] = include_bytes!("../fixtures/slot0.snap");
const SNAPSHOT_META: &[u8] = include_bytes!("../fixtures/slot0.meta");
const SNAPSHOT_THUMB: &[u8] = include_bytes!("../fixtures/slot0.thumb");
const RESAVED: &[u8] = include_bytes!("../fixtures/slot0.resaved.snap");
const BATTERY: &[u8] = include_bytes!("../fixtures/battery.sav");
const SETTINGS: &[u8] = include_bytes!("../fixtures/settings.json");
const RECORDING: &str = include_str!("../fixtures/recording.json");
const SIDECAR: &[u8] = include_bytes!("../fixtures/screenshot.png.json");

const NOW: u64 = 1_790_000_000;
const KEY: &str = "pins";
const SNAPSHOT_SAVED_AT: u64 = 1_793_583_568;
const FRAMES_AFTER_SNAPSHOT: u64 = 14_468_992_638_221_170_530;
const FRAME_AFTER_600: u64 = 5_623_194_403_494_096_317;
const RECORDED_EVENTS: usize = 26;

fn script(frame: u32) -> Keys {
    match frame % 240 {
        30..=35 => Keys::START,
        60..=100 => Keys::RIGHT,
        101..=110 => Keys::A,
        150..=200 => Keys::LEFT | Keys::B,
        _ => Keys::empty(),
    }
}

fn play(gb: &mut GameBoy, frames: std::ops::Range<u32>) {
    for frame in frames {
        let held = script(frame);
        gb.set_keys(held);
        gb.run_frame();
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325, |hash, &byte| {
        (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

fn rtc_rom() -> Vec<u8> {
    let mut data = vec![0u8; 0x8000];
    data[0x0147] = 0x10;
    data[0x0149] = 0x02;
    data
}

fn rtc_cart() -> GameBoy {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&Rom::new(&rtc_rom())).unwrap();
    gb
}

fn read_clock(gb: &mut GameBoy) -> [u8; 5] {
    gb.cartridge.write_naive(0x0000, 0x0A);
    gb.cartridge.write_naive(0x6000, 0x00);
    gb.cartridge.write_naive(0x6000, 0x01);
    [0x08, 0x09, 0x0A, 0x0B, 0x0C].map(|register| {
        gb.cartridge.write_naive(0x4000, register);
        gb.cartridge.read_naive(0xA000)
    })
}

fn read_ram(gb: &mut GameBoy, len: usize) -> Vec<u8> {
    gb.cartridge.write_naive(0x0000, 0x0A);
    gb.cartridge.write_naive(0x4000, 0x00);
    (0..len)
        .map(|i| gb.cartridge.read_naive(0xA000 + i as u16))
        .collect()
}

fn snapshot_store(dir: &Path) -> SaveStore {
    let store = SaveStore::at(dir.to_path_buf());
    store.put_file(KEY, "slot0.snap", SNAPSHOT).unwrap();
    store.put_file(KEY, "slot0.meta", SNAPSHOT_META).unwrap();
    store.put_file(KEY, "slot0.thumb", SNAPSHOT_THUMB).unwrap();
    store
}

#[test]
fn a_snapshot_from_today_shows_up_in_its_slot() {
    let dir = tempfile::tempdir().unwrap();
    let store = snapshot_store(dir.path());

    let info = store.snapshot_info(KEY, 0).unwrap();

    assert_eq!(info.console, Some(ConsoleKind::Gb));
    assert_eq!(info.saved_at, SNAPSHOT_SAVED_AT);
    assert_eq!(info.thumbnail.len(), 80 * 72 * 4);
}

#[test]
fn a_snapshot_from_today_saves_again_to_the_same_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let state = snapshot_store(dir.path()).load_snapshot(KEY, 0).unwrap();

    let mut gb = GameBoy::from_dump(&state, &Rom::new(TOBU), NOW).unwrap();
    assert!(gb.dump_full(NOW).unwrap() == RESAVED);

    let mut again = GameBoy::from_dump(RESAVED, &Rom::new(TOBU), NOW).unwrap();
    assert!(again.dump_full(NOW).unwrap() == RESAVED);
}

#[test]
fn a_snapshot_from_today_runs_600_frames_to_the_same_pictures() {
    let mut gb = GameBoy::from_dump(SNAPSHOT, &Rom::new(TOBU), NOW).unwrap();

    let mut frames = 0xCBF2_9CE4_8422_2325u64;
    for frame in 600..1200 {
        play(&mut gb, frame..frame + 1);
        frames = (frames ^ fnv(gb.frame().as_slice())).wrapping_mul(0x0000_0100_0000_01B3);
    }

    assert_eq!(frames, FRAMES_AFTER_SNAPSHOT);
    assert_eq!(fnv(gb.frame().as_slice()), FRAME_AFTER_600);
}

#[test]
fn a_battery_save_with_a_clock_from_today_loads_and_saves_again() {
    let mut gb = rtc_cart();
    gb.put_sram_dump(SramDump::from_slice(BATTERY), NOW + 3_661);

    assert_eq!(read_ram(&mut gb, 12), b"citrine pins");
    assert_eq!(read_clock(&mut gb), [13, 35, 6, 6, 0]);

    let mut again = rtc_cart();
    again.put_sram_dump(SramDump::from_slice(BATTERY), NOW);
    assert!(again.poll_sram_dump(true, NOW).unwrap().as_slice() == BATTERY);
}

#[test]
fn a_snapshot_from_today_loads_into_a_machine_and_saves_the_same_bytes() {
    assert_eq!(state_kind(SNAPSHOT), Some(ConsoleKind::Gb));
    let mut machine =
        Machine::from_state(SNAPSHOT, &Rom::new(TOBU), NOW, &Firmware::default()).unwrap();
    assert!(machine.save_state(NOW).unwrap() == RESAVED);

    let mut again =
        Machine::from_state(RESAVED, &Rom::new(TOBU), NOW, &Firmware::default()).unwrap();
    assert!(again.save_state(NOW).unwrap() == RESAVED);
}

#[test]
fn a_battery_save_from_today_loads_into_a_machine_and_saves_the_same_bytes() {
    let mut machine =
        Machine::load(&Rom::new(&rtc_rom()), ConsoleKind::Gb, &Firmware::default()).unwrap();
    let save = machine.battery_from_file(BATTERY);
    assert!(save.clock.is_some());
    assert_eq!(&save.ram[..12], b"citrine pins");
    assert!(save.to_file() == BATTERY);

    machine.put_battery(save, NOW);
    assert!(machine.battery(true, NOW).unwrap().to_file() == BATTERY);
}

#[test]
fn settings_from_today_keep_their_key_binds() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("settings")).unwrap();
    std::fs::write(dir.path().join("settings/settings.json"), SETTINGS).unwrap();

    let settings = SettingsStore::at(dir.path().to_path_buf())
        .load::<Settings>()
        .unwrap();

    let binds = &settings.bindings;
    assert_eq!(binds.button_for_key(egui::Key::J), Some(GbButton::A));
    assert_eq!(binds.button_for_key(egui::Key::Q), None);
    assert_eq!(binds.button_for_key(egui::Key::W), Some(GbButton::Up));
    assert_eq!(
        binds.button_for_pad(PadButton::LeftTrigger),
        Some(GbButton::Select)
    );
    assert_eq!(binds.shoulder_for_pad(PadButton::LeftTrigger), None);
    assert_eq!(
        binds.shoulder_pads(Shoulder::R),
        [PadButton::RightTrigger, PadButton::RightTrigger2]
    );
    assert_eq!(binds.shoulder_keys(Shoulder::L), [egui::Key::Num1]);
    assert_eq!(settings.channels, [true, false, true, true]);
    assert_eq!(settings.volume, 0.4);
}

#[test]
fn a_recording_from_today_loads() {
    let recording = Recording::from_json(RECORDING).unwrap();

    assert_eq!(
        recording.rom_sha256,
        Rom::new(TOBU).header().unwrap().sha256_hex_string()
    );
    assert_eq!(recording.model, GbModel::Dmg);
    assert_eq!(recording.events.len(), RECORDED_EVENTS);
    assert!(matches!(recording.events[0].button, Button::Start));
    assert!(recording.events[0].pressed);
    assert!(recording.events.is_sorted_by_key(|event| event.cycle));
}

#[test]
fn a_media_sidecar_from_today_names_its_game_and_console() {
    let dir = tempfile::tempdir().unwrap();
    let shots = dir.path().join("screenshots");
    let sidecars = dir.path().join(".library/screenshots");
    std::fs::create_dir_all(&shots).unwrap();
    std::fs::create_dir_all(&sidecars).unwrap();
    std::fs::write(shots.join("tobu.png"), b"png").unwrap();
    std::fs::write(sidecars.join("tobu.png.json"), SIDECAR).unwrap();

    let store = MediaStore::at(dir.path().to_path_buf());

    let info = &store.items()[0].info;
    assert_eq!(info.kind, MediaKind::Screenshot);
    assert_eq!(info.game, "Tobu Tobu Girl");
    assert_eq!(
        info.rom_key.as_deref(),
        Some(
            Rom::new(TOBU)
                .header()
                .unwrap()
                .sha256_hex_string()
                .as_str()
        )
    );
    assert_eq!(info.model, Some(GbModel::Dmg));
    assert_eq!(info.created_at, NOW);
    assert_eq!((info.width, info.height), (160, 144));
}
