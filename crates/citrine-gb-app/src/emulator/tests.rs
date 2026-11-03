use super::*;
use crate::app::ui_state::keybinds::{BindingKind, BindingTarget, Captured, GbButton, PadButton};
use crate::held::InputSource;
use crate::pad_input::PadEvent;
use crate::pad_input::{PadAxis, PadInput, PadKey};
use strum::IntoEnumIterator;

const RAM_LEN: usize = 0x2000;
const KEYS: InputSource = InputSource::Keyboard;

fn emulator(name: &str) -> Emulator {
    let root =
        std::env::temp_dir().join(format!("citrine-import-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    Emulator {
        store: SaveStore::at(root),
        ..Emulator::default()
    }
}

fn rom(cartridge_type: u8) -> Rom {
    let mut data = vec![0u8; 0x8000];
    data[0x0147] = cartridge_type;
    data[0x0149] = 0x02;
    Rom::new(&data)
}

fn bytes(seed: u8) -> Vec<u8> {
    (0..RAM_LEN)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
        .collect()
}

fn running(emulator: &mut Emulator, cartridge_type: u8) {
    emulator
        .load_rom(&rom(cartridge_type), GbModel::Dmg, None)
        .expect("load");
    emulator.gb.run_cycles(100_000);
}

fn played(emulator: &mut Emulator, seconds: f64) {
    let start = web_time::Instant::now();
    emulator.advance(start, None);
    let mut at = 0.0;
    while at < seconds {
        at += 0.1;
        emulator.advance(start + std::time::Duration::from_secs_f64(at), None);
    }
}

fn write_sram(emulator: &mut Emulator, value: u8) {
    use citrine_gb::WriteMemory;
    emulator.gb.cartridge.write_naive(0x0000, 0x0A);
    emulator.gb.cartridge.write_naive(0xA000, value);
}

fn autosaves(emulator: &Emulator) -> Vec<usize> {
    emulator
        .store
        .autosave_ids(emulator.rom_key().expect("key"))
}

fn gb_held(emulator: &mut Emulator) -> u8 {
    use citrine_gb::{ReadMemory, WriteMemory};
    let joypad = &mut emulator.gb.joypad;
    joypad.write_naive(0xFF00, 0x10);
    let buttons = !joypad.read_naive(0xFF00) & 0x0F;
    joypad.write_naive(0xFF00, 0x20);
    let dpad = !joypad.read_naive(0xFF00) & 0x0F;
    buttons | (dpad << 4)
}

#[test]
fn the_game_boy_sees_what_is_held_after_a_load() {
    let mut emulator = emulator("held-sync");
    running(&mut emulator, 0x03);
    emulator.press(KEYS, JoypadState::A);
    emulator.save_snapshot(0).expect("save");
    emulator.release(KEYS, JoypadState::A);
    emulator.press(KEYS, JoypadState::RIGHT);

    assert!(emulator.load_snapshot(0).expect("load"));
    assert_eq!(
        gb_held(&mut emulator),
        JoypadState::RIGHT.bits(),
        "A was let go before the load and Right is still down"
    );
    assert!(emulator.release(KEYS, JoypadState::RIGHT));
    assert_eq!(gb_held(&mut emulator), 0);

    emulator.press(KEYS, JoypadState::START);
    running(&mut emulator, 0x03);
    assert_eq!(gb_held(&mut emulator), JoypadState::START.bits());
    assert!(
        emulator.release(KEYS, JoypadState::START),
        "the release reaches the game"
    );
    assert_eq!(gb_held(&mut emulator), 0);
}

#[test]
fn paused_time_does_not_count() {
    let mut emulator = emulator("auto-paused");
    running(&mut emulator, 0x03);
    emulator.running = false;
    played(&mut emulator, 3.0);
    assert_eq!(emulator.autosave_if_played().expect("autosave"), None);

    emulator.running = true;
    played(&mut emulator, 0.5);
    emulator.autosave_interval = AutosaveInterval::OneMinute;
    assert_eq!(emulator.autosave_if_due().expect("autosave"), None);
    emulator.play.play(std::time::Duration::from_secs(60));
    assert_eq!(emulator.autosave_if_due().expect("autosave"), Some(0));
    assert_eq!(emulator.autosave_if_due().expect("autosave"), None);
}

#[test]
fn closing_autosaves_only_after_play() {
    let mut emulator = emulator("auto-close");
    running(&mut emulator, 0x03);
    assert_eq!(emulator.autosave_if_played().expect("autosave"), None);
    played(&mut emulator, 0.3);
    assert_eq!(emulator.autosave_if_played().expect("autosave"), Some(0));
    assert_eq!(emulator.autosave_if_played().expect("autosave"), None);
    assert_eq!(autosaves(&emulator), vec![0]);
}

#[test]
fn switching_games_autosaves_the_old_one() {
    let mut emulator = emulator("auto-switch");
    running(&mut emulator, 0x03);
    let first = emulator.rom_key().expect("key").to_owned();
    played(&mut emulator, 0.3);

    let mut other = vec![0u8; 0x8000];
    other[0x0134] = b'B';
    other[0x0147] = 0x03;
    other[0x0149] = 0x02;
    emulator
        .load_rom(&Rom::new(&other), GbModel::Dmg, None)
        .expect("load");

    assert_ne!(emulator.rom_key(), Some(first.as_str()));
    assert_eq!(emulator.store.autosave_ids(&first), vec![0]);
    assert!(autosaves(&emulator).is_empty());
}

#[test]
fn the_keep_limit_drops_the_oldest() {
    let mut emulator = emulator("auto-keep");
    running(&mut emulator, 0x03);
    emulator.autosave_keep = 2;
    for _ in 0..4 {
        emulator.autosave().expect("autosave");
    }
    assert_eq!(autosaves(&emulator), vec![2, 3]);
}

#[test]
fn an_autosave_loads_back_without_touching_snapshots() {
    let mut emulator = emulator("auto-load");
    running(&mut emulator, 0x03);
    emulator.save_snapshot(0).expect("snapshot");
    let key = emulator.rom_key().expect("key").to_owned();
    let snapshot = emulator.store.load_snapshot(&key, 0);

    played(&mut emulator, 0.3);
    write_sram(&mut emulator, 0x11);
    let id = emulator.autosave().expect("autosave").expect("id");
    played(&mut emulator, 0.3);
    write_sram(&mut emulator, 0x22);
    assert!(emulator.load_autosave(id).expect("load"));

    assert_eq!(emulator.gb.cartridge.battery_ram()[0], 0x11);
    assert_eq!(emulator.autosave_if_played().expect("autosave"), None);
    assert_eq!(emulator.store.load_snapshot(&key, 0), snapshot);
    assert_eq!(emulator.store.used_slots(&key), vec![0]);
}

fn dual(emulator: &mut Emulator, model: GbModel) {
    let mut data = rom(0x03).data;
    data[0x0143] = 0x80;
    emulator
        .load_rom(&Rom::new(&data), model, None)
        .expect("load");
    emulator.gb.run_cycles(100_000);
}

#[test]
fn switching_consoles_keeps_the_battery_save() {
    let mut emulator = emulator("switch-battery");
    dual(&mut emulator, GbModel::Cgb);
    write_sram(&mut emulator, 0x5A);
    emulator.restart_on(GbModel::Dmg).expect("switch");
    assert_eq!(emulator.console(), GbModel::Dmg);
    assert_eq!(emulator.gb.cartridge.battery_ram()[0], 0x5A);

    write_sram(&mut emulator, 0x6B);
    emulator.restart_on(GbModel::Cgb).expect("switch back");
    assert_eq!(emulator.console(), GbModel::Cgb);
    assert_eq!(emulator.gb.cartridge.battery_ram()[0], 0x6B);
    assert_eq!(
        emulator.take_console_switch(),
        None,
        "a restart is no surprise"
    );
}

#[test]
fn switching_consoles_keeps_the_game_name() {
    let mut emulator = emulator("switch-name");
    dual(&mut emulator, GbModel::Cgb);
    emulator.name_game(Some("My Game (USA).gbc"));
    let name = emulator.game_name().to_owned();
    emulator.restart_on(GbModel::Dmg).expect("switch");
    assert_eq!(emulator.game_name(), name);
}

#[test]
fn a_known_game_names_its_battery_save_by_its_proper_name() {
    let mut emulator = emulator("proper-name");
    let rom =
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/homebrew/libbet.gb")).expect("libbet");
    emulator
        .load_rom(&Rom::new(&rom), GbModel::Dmg, None)
        .expect("load");
    emulator.name_game(Some("libbet.gb"));
    assert_ne!(
        emulator.gb.cartridge.header.title.trim(),
        "Libbet and the Magic Floor"
    );
    assert_eq!(emulator.game_name(), "Libbet and the Magic Floor");
    assert_eq!(emulator.save_status().name, "Libbet and the Magic Floor");
}

#[test]
fn a_snapshot_loads_on_the_console_it_was_made_on() {
    let mut emulator = emulator("snapshot-console");
    dual(&mut emulator, GbModel::Dmg);
    emulator.save_snapshot(0).expect("snapshot");
    let key = emulator.rom_key().expect("key").to_owned();
    let info = emulator.store.snapshot_info(&key, 0).expect("info");
    assert_eq!(info.console, Some(GbModel::Dmg));

    emulator.restart_on(GbModel::Cgb).expect("switch");
    assert!(emulator.load_snapshot(0).expect("load"));
    assert_eq!(emulator.console(), GbModel::Dmg);
    assert_eq!(emulator.take_console_switch(), Some(GbModel::Dmg));
    assert_eq!(emulator.take_console_switch(), None);

    assert!(emulator.load_snapshot(0).expect("load again"));
    assert_eq!(
        emulator.take_console_switch(),
        None,
        "same console, no notice"
    );
}

#[test]
fn an_autosave_records_its_console() {
    let mut emulator = emulator("autosave-console");
    dual(&mut emulator, GbModel::Cgb);
    played(&mut emulator, 0.3);
    let id = emulator.autosave().expect("autosave").expect("id");
    let key = emulator.rom_key().expect("key").to_owned();
    let infos = emulator.store.autosave_infos(&key);
    assert_eq!(infos[0].console, Some(GbModel::Cgb));

    emulator.restart_on(GbModel::Dmg).expect("switch");
    assert!(emulator.load_autosave(id).expect("load"));
    assert_eq!(emulator.console(), GbModel::Cgb);
    assert_eq!(emulator.take_console_switch(), Some(GbModel::Cgb));
}

#[test]
fn a_snapshot_from_before_consoles_were_recorded_still_loads() {
    let name = "old-snapshot";
    let mut emulator = emulator(name);
    dual(&mut emulator, GbModel::Dmg);
    write_sram(&mut emulator, 0x33);
    emulator.save_snapshot(0).expect("snapshot");
    let key = emulator.rom_key().expect("key").to_owned();
    let meta = std::env::temp_dir()
        .join(format!("citrine-import-test-{}-{name}", std::process::id()))
        .join(&key)
        .join("slot0.meta");
    std::fs::write(&meta, 1_700_000_000u64.to_le_bytes()).expect("old meta");

    let info = emulator.store.snapshot_info(&key, 0).expect("info");
    assert_eq!(info.saved_at, 1_700_000_000);
    assert_eq!(info.console, Some(GbModel::Dmg), "read from the snapshot");
    assert_eq!(std::fs::read(&meta).expect("meta").len(), 9, "noted once");

    write_sram(&mut emulator, 0x44);
    assert!(emulator.load_snapshot(0).expect("load"));
    assert_eq!(emulator.gb.cartridge.battery_ram()[0], 0x33);
    assert_eq!(emulator.console(), GbModel::Dmg);
}

#[test]
fn a_crash_saves_the_last_frame_boundary() {
    let mut emulator = emulator("auto-crash");
    running(&mut emulator, 0x03);
    write_sram(&mut emulator, 0x42);
    played(&mut emulator, 1.2);
    emulator.running = false;
    played(&mut emulator, 0.1);

    assert_eq!(emulator.last_good.write_out(), Ok(Some(0)));
    let key = emulator.rom_key().expect("key").to_owned();
    let state = emulator.store.load_autosave(&key, 0).expect("autosave");
    let rom = Rom::new(&emulator.gb.cartridge.rom_bytes());
    let restored = GameBoy::from_dump(&state, &rom, now_unix()).expect("restores");
    assert_eq!(restored.cpu.pc, emulator.gb.cpu.pc);
    assert_eq!(restored.cartridge.battery_ram()[0], 0x42);
    let battery = emulator.store.load_battery(&key).expect("battery");
    assert_eq!(battery[0], 0x42);
}

#[test]
fn an_imported_save_survives_the_restart() {
    let mut emulator = emulator("save");
    running(&mut emulator, 0x03);
    emulator.import_save(Some(&bytes(1)), None).expect("first");
    emulator.restart();
    emulator.gb.run_cycles(100_000);

    let result = emulator.import_save(Some(&bytes(2)), None).expect("second");
    assert_eq!(result, SaveImport::Imported);
    emulator.restart();
    emulator.gb.run_cycles(100_000);

    assert_eq!(emulator.gb.cartridge.battery_ram(), bytes(2));
    let key = emulator.rom_key().expect("key").to_owned();
    assert_eq!(emulator.store.load_battery(&key), Some(bytes(2)));
    assert!(emulator.running);
    assert!(!emulator.has_been_playing());
}

#[test]
fn an_imported_clock_survives_the_restart() {
    let mut emulator = emulator("clock");
    running(&mut emulator, 0x10);
    let mut footer = emulator.gb.cartridge.rtc_footer(now_unix()).expect("clock");
    footer.real.seconds = 12;
    footer.real.minutes = 34;
    footer.real.hours = 5;
    footer.real.days_low = 6;
    footer.real.days_high = 0x40;
    footer.latched = footer.real;

    let result = emulator
        .import_save(Some(&bytes(3)), Some(footer))
        .expect("import");
    assert_eq!(result, SaveImport::Imported);
    emulator.restart();
    emulator.gb.run_cycles(100_000);

    assert_eq!(emulator.gb.cartridge.battery_ram(), bytes(3));
    let clock = emulator.gb.cartridge.rtc_footer(now_unix()).expect("clock");
    assert_eq!(clock.real, footer.real);
}

#[test]
fn a_clock_needs_a_cart_with_one() {
    let mut emulator = emulator("no-clock");
    assert_eq!(emulator.import_blocker(false), Some(SaveImport::NoRom));
    running(&mut emulator, 0x03);
    assert_eq!(emulator.import_blocker(false), None);
    assert_eq!(emulator.import_blocker(true), Some(SaveImport::NoClock));
}

#[test]
fn a_screenshot_lands_in_the_media_store() {
    use crate::media::{MediaKind, MediaStore, save_screenshot};
    let root = std::env::temp_dir().join(format!("citrine-shot-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut media = MediaStore::at(root.clone());
    let mut emulator = emulator("screenshot");
    assert!(save_screenshot(&emulator, &mut media, 3).is_err());

    running(&mut emulator, 0x00);
    let item = save_screenshot(&emulator, &mut media, 3).expect("saved");
    assert_eq!(item.info.kind, MediaKind::Screenshot);
    assert_eq!((item.info.width, item.info.height), (480, 432));
    assert!(media.path(&item.id).starts_with(root.join("screenshots")));
    assert!(media.path(&item.id).is_file());
    assert!(item.thumbnail.is_some());
    assert_eq!(MediaStore::at(root.clone()).items().len(), 1);
    let _ = std::fs::remove_dir_all(root);
}

struct FakePads(std::collections::VecDeque<PadEvent>);

impl PadSource for FakePads {
    fn next_event(&mut self) -> Option<PadEvent> {
        self.0.pop_front()
    }

    fn pads(&self, _seen: &[usize]) -> Vec<crate::controller_debug::PadInfo> {
        Vec::new()
    }

    fn api(&self) -> String {
        "fake".to_string()
    }
}

fn fake(events: &[(usize, PadInput)]) -> FakePads {
    FakePads(
        events
            .iter()
            .map(|&(pad, input)| PadEvent {
                pad,
                input,
                code: None,
            })
            .collect(),
    )
}

#[test]
fn any_pad_source_drives_the_bindings() {
    let mut emulator = emulator("pad-source");
    let bindings = KeyBindings::default();
    let mut capture = BindingCapture::default();
    let mut log = PadLog::new(web_time::Instant::now());
    let mut pads = fake(&[
        (3, PadInput::Connected),
        (3, PadInput::Pressed(PadKey::Known(PadButton::East))),
        (3, PadInput::Moved(PadAxis::LeftStickX, -0.9)),
    ]);
    emulator.handle_pads(&mut pads, &bindings, &mut capture, Some(&mut log));
    assert!(emulator.is_held(GbButton::A.joypad()));
    assert!(emulator.is_held(GbButton::Left.joypad()));
    assert_eq!(log.len(), 3);
    assert_eq!(log.seen(), &[3]);

    let mut pads = fake(&[
        (3, PadInput::Released(PadKey::Known(PadButton::East))),
        (3, PadInput::Moved(PadAxis::LeftStickX, 0.0)),
    ]);
    emulator.handle_pads(&mut pads, &bindings, &mut capture, None);
    assert!(!emulator.is_held(GbButton::A.joypad()));
    assert!(!emulator.is_held(GbButton::Left.joypad()));
}

#[test]
fn any_pad_source_feeds_a_waiting_binding() {
    let mut emulator = emulator("pad-capture");
    let bindings = KeyBindings::default();
    let mut capture = BindingCapture::default();
    capture.start(BindingTarget::Button(GbButton::B), BindingKind::Gamepad);
    let mut pads = fake(&[(
        1,
        PadInput::Pressed(PadKey::Known(PadButton::RightTrigger2)),
    )]);
    emulator.handle_pads(&mut pads, &bindings, &mut capture, None);
    assert_eq!(
        capture.take(),
        Some((
            BindingTarget::Button(GbButton::B),
            Captured::Pad(PadButton::RightTrigger2)
        ))
    );
    assert!(!emulator.is_held(GbButton::B.joypad()));
}

fn d_pad(button: PadButton, pressed: bool) -> PadInput {
    if pressed {
        PadInput::Pressed(PadKey::Known(button))
    } else {
        PadInput::Released(PadKey::Known(button))
    }
}

fn stick_x(value: f32) -> PadInput {
    PadInput::Moved(PadAxis::LeftStickX, value)
}

fn stick_y(value: f32) -> PadInput {
    PadInput::Moved(PadAxis::LeftStickY, value)
}

fn noise(count: usize) -> Vec<(usize, PadInput)> {
    (0..count)
        .map(|i| {
            let value = if i % 2 == 0 { 0.02 } else { -0.02 };
            let input = if i % 3 == 0 {
                stick_y(value)
            } else {
                stick_x(value)
            };
            (0, input)
        })
        .collect()
}

fn feed(emulator: &mut Emulator, bindings: &KeyBindings, events: &[(usize, PadInput)]) {
    let mut capture = BindingCapture::default();
    emulator.handle_pads(&mut fake(events), bindings, &mut capture, None);
}

fn pads_in(emulator: &mut Emulator, events: &[(usize, PadInput)]) {
    feed(emulator, &KeyBindings::default(), events);
}

fn play_a_frame(emulator: &mut Emulator) {
    emulator.running = true;
    let start = web_time::Instant::now();
    emulator.advance(start, None);
    emulator.advance(start + std::time::Duration::from_millis(50), None);
}

fn holds(emulator: &Emulator, button: GbButton) -> bool {
    emulator.is_held(button.joypad())
}

#[test]
fn stick_noise_never_lets_go_of_a_held_d_pad() {
    let mut emulator = emulator("pad-noise");
    running(&mut emulator, 0x03);
    pads_in(&mut emulator, &[(0, d_pad(PadButton::DPadLeft, true))]);
    pads_in(&mut emulator, &noise(50));
    assert!(holds(&emulator, GbButton::Left));
    play_a_frame(&mut emulator);
    assert_eq!(gb_held(&mut emulator), JoypadState::LEFT.bits());
}

#[test]
fn a_d_pad_tap_reaches_the_game_for_a_frame() {
    let mut emulator = emulator("pad-tap");
    running(&mut emulator, 0x03);
    let mut tap = vec![(0, d_pad(PadButton::DPadLeft, true))];
    tap.extend(noise(6));
    tap.push((0, d_pad(PadButton::DPadLeft, false)));
    pads_in(&mut emulator, &tap);
    assert!(!holds(&emulator, GbButton::Left), "the player let go");
    assert_eq!(
        gb_held(&mut emulator),
        JoypadState::LEFT.bits(),
        "the game still sees the tap until a frame runs"
    );
    play_a_frame(&mut emulator);
    assert_eq!(gb_held(&mut emulator), 0);
}

#[test]
fn a_stick_flick_reaches_the_game_for_a_frame() {
    let mut emulator = emulator("pad-flick");
    running(&mut emulator, 0x03);
    pads_in(
        &mut emulator,
        &[(0, stick_x(0.95)), (0, stick_x(0.3)), (0, stick_x(0.01))],
    );
    assert!(!holds(&emulator, GbButton::Right));
    assert_eq!(gb_held(&mut emulator), JoypadState::RIGHT.bits());
    play_a_frame(&mut emulator);
    assert_eq!(gb_held(&mut emulator), 0);
}

#[test]
fn a_centred_stick_lets_go_of_only_what_it_held() {
    let mut emulator = emulator("pad-centre");
    running(&mut emulator, 0x03);
    pads_in(
        &mut emulator,
        &[(0, d_pad(PadButton::DPadUp, true)), (0, stick_x(-0.9))],
    );
    play_a_frame(&mut emulator);
    pads_in(&mut emulator, &[(0, stick_x(0.0))]);
    assert!(!holds(&emulator, GbButton::Left));
    assert!(holds(&emulator, GbButton::Up));
    assert_eq!(gb_held(&mut emulator), JoypadState::UP.bits());
}

#[test]
fn the_d_pad_and_the_stick_hold_the_same_button_together() {
    let mut emulator = emulator("pad-both");
    running(&mut emulator, 0x03);
    pads_in(
        &mut emulator,
        &[(0, d_pad(PadButton::DPadLeft, true)), (0, stick_x(-0.9))],
    );
    play_a_frame(&mut emulator);
    pads_in(&mut emulator, &[(0, stick_x(0.0))]);
    assert!(holds(&emulator, GbButton::Left), "the d-pad still holds it");
    pads_in(&mut emulator, &[(0, stick_x(-0.9))]);
    play_a_frame(&mut emulator);
    pads_in(&mut emulator, &[(0, d_pad(PadButton::DPadLeft, false))]);
    assert!(holds(&emulator, GbButton::Left), "the stick still holds it");
    assert_eq!(gb_held(&mut emulator), JoypadState::LEFT.bits());
    pads_in(&mut emulator, &[(0, stick_x(0.0))]);
    assert!(!holds(&emulator, GbButton::Left));
    assert_eq!(gb_held(&mut emulator), 0);
}

#[test]
fn the_keyboard_and_a_pad_do_not_let_go_of_each_other() {
    let mut emulator = emulator("pad-keys");
    running(&mut emulator, 0x03);
    emulator.press(KEYS, JoypadState::A);
    pads_in(&mut emulator, &[(0, d_pad(PadButton::East, true))]);
    play_a_frame(&mut emulator);
    pads_in(&mut emulator, &[(0, d_pad(PadButton::East, false))]);
    assert!(holds(&emulator, GbButton::A));
    pads_in(&mut emulator, &[(0, d_pad(PadButton::East, true))]);
    emulator.release(KEYS, JoypadState::A);
    assert!(holds(&emulator, GbButton::A));
}

#[test]
fn stick_diagonals_act_on_each_axis_alone() {
    let mut emulator = emulator("pad-diagonal");
    running(&mut emulator, 0x03);
    pads_in(&mut emulator, &[(0, stick_x(-0.8)), (0, stick_y(-0.8))]);
    assert!(holds(&emulator, GbButton::Left));
    assert!(holds(&emulator, GbButton::Down));
    play_a_frame(&mut emulator);
    pads_in(&mut emulator, &[(0, stick_y(0.02))]);
    assert!(holds(&emulator, GbButton::Left));
    assert!(!holds(&emulator, GbButton::Down));
    pads_in(&mut emulator, &[(0, stick_y(0.8)), (0, stick_x(0.01))]);
    assert!(holds(&emulator, GbButton::Up));
    assert!(!holds(&emulator, GbButton::Left));
}

#[test]
fn a_stick_resting_near_the_dead_zone_does_not_flicker() {
    let mut emulator = emulator("pad-edge");
    running(&mut emulator, 0x03);
    let mut log = PadLog::new(web_time::Instant::now());
    let bindings = KeyBindings::default();
    let mut capture = BindingCapture::default();
    let mut send = |emulator: &mut Emulator, log: &mut PadLog, values: &[f32]| {
        let events: Vec<(usize, PadInput)> = values.iter().map(|&v| (0, stick_x(v))).collect();
        emulator.handle_pads(&mut fake(&events), &bindings, &mut capture, Some(log));
    };
    send(&mut emulator, &mut log, &[0.46, 0.49, 0.47, 0.5, 0.48]);
    assert!(!holds(&emulator, GbButton::Right), "not past the dead zone");
    send(&mut emulator, &mut log, &[0.53]);
    assert!(holds(&emulator, GbButton::Right));
    let before = log.len();
    let jitter: Vec<f32> = (0..40)
        .map(|i| if i % 2 == 0 { 0.52 } else { 0.42 })
        .collect();
    send(&mut emulator, &mut log, &jitter);
    assert!(holds(&emulator, GbButton::Right), "held through the jitter");
    assert_eq!(log.len(), before + 1, "the jitter is one log line");
    send(&mut emulator, &mut log, &[0.38]);
    assert!(!holds(&emulator, GbButton::Right));
}

#[test]
fn the_dead_zone_setting_moves_where_the_stick_presses() {
    let mut emulator = emulator("pad-deadzone");
    running(&mut emulator, 0x03);
    let mut bindings = KeyBindings::default();
    bindings.stick_deadzone = 30;
    feed(&mut emulator, &bindings, &[(0, stick_x(-0.35))]);
    assert!(holds(&emulator, GbButton::Left));
    feed(&mut emulator, &bindings, &[(0, stick_x(-0.22))]);
    assert!(holds(&emulator, GbButton::Left), "lets go only below 0.20");
    feed(&mut emulator, &bindings, &[(0, stick_x(-0.15))]);
    assert!(!holds(&emulator, GbButton::Left));
}

#[test]
fn an_unplugged_pad_lets_go_of_what_it_held() {
    let mut emulator = emulator("pad-unplug");
    running(&mut emulator, 0x03);
    emulator.press(KEYS, JoypadState::A);
    pads_in(
        &mut emulator,
        &[
            (0, d_pad(PadButton::East, true)),
            (0, d_pad(PadButton::Start, true)),
            (0, stick_x(-0.9)),
            (1, d_pad(PadButton::South, true)),
        ],
    );
    play_a_frame(&mut emulator);
    pads_in(&mut emulator, &[(0, PadInput::Disconnected)]);
    assert!(holds(&emulator, GbButton::A), "the keyboard still holds it");
    assert!(holds(&emulator, GbButton::B), "pad 1 still holds it");
    assert!(!holds(&emulator, GbButton::Start));
    assert!(!holds(&emulator, GbButton::Left));
    assert_eq!(
        gb_held(&mut emulator),
        (JoypadState::A | JoypadState::B).bits()
    );
}

#[test]
fn letting_go_of_everything_clears_every_source() {
    let mut emulator = emulator("pad-release-all");
    running(&mut emulator, 0x03);
    emulator.press(KEYS, JoypadState::A);
    emulator.press(InputSource::Touch, JoypadState::B);
    pads_in(
        &mut emulator,
        &[(0, d_pad(PadButton::DPadUp, true)), (0, stick_x(0.9))],
    );
    emulator.release_all();
    assert!(GbButton::iter().all(|b| !holds(&emulator, b)));
    assert_eq!(gb_held(&mut emulator), 0, "taps owed a frame go too");
    pads_in(&mut emulator, &[(0, stick_x(0.95))]);
    assert!(
        holds(&emulator, GbButton::Right),
        "a stick still pushed holds again on its next move"
    );
}

#[test]
fn stick_noise_folds_into_one_log_line_per_axis() {
    let mut emulator = emulator("pad-log");
    let bindings = KeyBindings::default();
    let mut capture = BindingCapture::default();
    let mut log = PadLog::new(web_time::Instant::now());
    let mut events = vec![(0, d_pad(PadButton::DPadUp, true))];
    events.extend(noise(90));
    events.push((0, PadInput::Moved(PadAxis::RightStickX, 0.01)));
    events.push((0, d_pad(PadButton::DPadUp, false)));
    events.extend(noise(9));
    emulator.handle_pads(&mut fake(&events), &bindings, &mut capture, Some(&mut log));
    let lines: Vec<String> = log.entries().map(|e| e.event()).collect();
    assert_eq!(lines.len(), 7, "{lines:#?}");
    assert!(lines[0].contains("DPadUp pressed"));
    assert!(
        lines[1].contains("LeftStickY moved 30 times between -0.02 and +0.02"),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].contains("LeftStickX moved 60 times"),
        "{}",
        lines[2]
    );
    assert!(lines[3].contains("RightStickX at +0.01"), "{}", lines[3]);
    assert!(lines[4].contains("DPadUp released"));
}

#[test]
fn a_finished_session_hands_its_console_to_the_emulator() {
    use crate::play::MyGame;
    use crate::play::tests::{Pair, SNAKE, homebrew, snake};
    let mut emu = emulator("finish-session");
    let model = GbModel::for_rom(&Rom::new(SNAKE)).unwrap();
    emu.load_rom(&Rom::new(SNAKE), model, None).unwrap();
    let name = emu.game_name().to_owned();
    let mut pair = Pair::new(false, 0, 0, homebrew());
    let session = pair.invite(snake());
    pair.accept(session, MyGame::Own(snake()));
    pair.play_frames(90);

    let now = web_time::Instant::now();
    emu.last_update = Some(now - std::time::Duration::from_millis(50));
    let session = pair.host.play.session_mut().unwrap();
    emu.advance_rollback(now, session);
    let shown = session.console(true).unwrap().frame().as_slice().to_vec();
    assert!(emu.linked_frame.as_deref() == Some(shown.as_slice()));

    pair.host.play.leave(&mut pair.host.online);
    let screen = pair
        .host
        .session()
        .console(true)
        .unwrap()
        .frame()
        .as_slice()
        .to_vec();
    let session = pair.host.play.close().unwrap();
    emu.finish_session(session).unwrap();
    assert!(emu.gb.frame().as_slice() == screen.as_slice());
    assert!(emu.last_frame == screen);
    assert!(emu.linked_frame.is_none());
    assert!(emu.running);
    assert!(!emu.gb.link_plugged());
    assert_eq!(emu.game_name(), name);
}
