use super::*;
use citrine_gb::gba::save::SaveType;
use citrine_gb::persistence::state::GBA_STATE_MAGIC;

const FRAMES: usize = 60;
const SAVES: [(&str, SaveType); 5] = [
    ("sram", SaveType::Sram32K),
    ("flash64", SaveType::Flash64K),
    ("flash128", SaveType::Flash128K),
    ("eeprom512", SaveType::Eeprom512),
    ("eeprom8k", SaveType::Eeprom8K),
];

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/gba")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn save_rom(kind: &str) -> Rom {
    Rom::new(&fixture(&format!("save-{kind}.gba")))
}

fn mgba_save(kind: &str) -> Vec<u8> {
    crate::storage::decompress(&fixture(&format!("mgba-{kind}.sav.br"))).expect("brotli")
}

fn on_store(root: &std::path::Path) -> Emulator {
    Emulator {
        store: SaveStore::at(root.to_path_buf()),
        ..Emulator::default()
    }
}

fn store_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("citrine-gba-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn start(emulator: &mut Emulator, rom: &Rom) {
    emulator
        .load_game(rom, ConsoleKind::Gba, None)
        .expect("load");
    assert_eq!(emulator.machine.kind(), ConsoleKind::Gba);
}

fn run(emulator: &mut Emulator, frames: usize) {
    for _ in 0..frames {
        emulator.machine.run_frame();
    }
}

fn chip(emulator: &mut Emulator) -> Option<SaveType> {
    emulator.machine.battery(true, 0).and_then(|save| save.chip)
}

#[test]
fn each_save_type_comes_back_with_its_chip_after_a_restart() {
    for (kind, expected) in SAVES {
        let root = store_root(&format!("type-{kind}"));
        let mut emulator = on_store(&root);
        start(&mut emulator, &save_rom(kind));
        run(&mut emulator, FRAMES);
        emulator.flush_save().expect("flush");
        let written = emulator.export_save_bytes().expect("save");
        assert_eq!(chip(&mut emulator), Some(expected), "{kind}");

        let mut again = on_store(&root);
        start(&mut again, &save_rom(kind));
        assert!(again.save_loaded, "{kind}");
        assert_eq!(again.export_save_bytes(), Some(written), "{kind}");
        assert_eq!(chip(&mut again), Some(expected), "{kind}");
    }
}

#[test]
fn a_cartridge_without_a_library_string_takes_its_chip_from_the_store() {
    let root = store_root("undetected");
    let mut emulator = on_store(&root);
    start(&mut emulator, &save_rom("flash64"));
    run(&mut emulator, FRAMES);
    emulator.flush_save().expect("flush");
    let written = emulator.export_save_bytes().expect("save");

    let mut bare = save_rom("flash64").data;
    let at = bare
        .windows(8)
        .position(|window| window == b"FLASH512")
        .expect("library string");
    bare[at] = 0;
    let mut again = on_store(&root);
    start(&mut again, &Rom::new(&bare));
    let bare_key = again.rom_key().expect("key").to_owned();
    let original_key = emulator.rom_key().expect("key").to_owned();
    for name in ["battery.sav", "battery.meta"] {
        let data = emulator.store.read_file(&original_key, name).expect(name);
        again.store.put_file(&bare_key, name, &data).expect(name);
    }
    again.reload_battery().expect("reload");
    assert_eq!(again.machine.kind(), ConsoleKind::Gba);
    assert_eq!(chip(&mut again), Some(SaveType::Flash64K));
    assert_eq!(again.export_save_bytes(), Some(written));
}

#[test]
fn a_save_written_by_mgba_imports_for_each_type() {
    for (kind, expected) in SAVES {
        let file = mgba_save(kind);
        let mut emulator = on_store(&store_root(&format!("mgba-{kind}")));
        start(&mut emulator, &save_rom(kind));
        let result = emulator.import_save(Some(&file), None).expect("import");
        assert_eq!(result, SaveImport::Imported, "{kind}");
        assert_eq!(
            emulator.export_save_bytes().as_deref(),
            Some(&file[..]),
            "{kind}"
        );
        let key = emulator.rom_key().expect("key").to_owned();
        assert_eq!(
            emulator.store.load_battery(&key).as_deref(),
            Some(&file[..]),
            "{kind}"
        );
        assert_eq!(chip(&mut emulator), Some(expected), "{kind}");
    }
}

#[test]
fn citrine_writes_the_same_save_as_mgba() {
    for (kind, _) in SAVES {
        let mut emulator = on_store(&store_root(&format!("same-{kind}")));
        start(&mut emulator, &save_rom(kind));
        run(&mut emulator, FRAMES);
        let ours = emulator.export_save_bytes().expect("save");
        let theirs = mgba_save(kind);
        assert_eq!(ours.len(), theirs.len(), "{kind}");
        let fresh_bank = |at: usize| match kind {
            "flash128" => at >= 0x1_0000,
            "eeprom8k" => at >= 0x200,
            _ => false,
        };
        let first = ours
            .iter()
            .zip(&theirs)
            .enumerate()
            .position(|(at, (&a, &b))| a != b && !(fresh_bank(at) && a == 0xFF && b == 0));
        assert_eq!(first, None, "{kind}");
    }
}

#[test]
fn a_gba_snapshot_carries_the_magic_and_loads_back() {
    let mut emulator = on_store(&store_root("snapshot"));
    start(&mut emulator, &save_rom("sram"));
    run(&mut emulator, 10);
    emulator.save_snapshot(0).expect("snapshot");
    run(&mut emulator, 1);
    let frame = emulator.frame().to_vec();
    let key = emulator.rom_key().expect("key").to_owned();
    let state = emulator.store.load_snapshot(&key, 0).expect("stored");
    assert_eq!(state[..4], GBA_STATE_MAGIC);
    let info = emulator.store.snapshot_info(&key, 0).expect("info");
    assert_eq!(info.console, Some(ConsoleKind::Gba));
    assert_eq!(info.thumb_size(), [120, 80]);

    run(&mut emulator, FRAMES);
    assert!(emulator.load_snapshot(0).expect("load"));
    assert_eq!(emulator.machine.kind(), ConsoleKind::Gba);
    run(&mut emulator, 1);
    assert_eq!(emulator.frame(), &frame[..]);
    assert_eq!(emulator.take_console_switch(), None);
}

#[test]
fn a_gba_autosave_loads_back() {
    let mut emulator = on_store(&store_root("autosave"));
    start(&mut emulator, &save_rom("flash64"));
    run(&mut emulator, FRAMES);
    let id = emulator.autosave().expect("autosave").expect("id");
    let saved = emulator.export_save_bytes().expect("save");
    run(&mut emulator, 5);
    assert!(emulator.load_autosave(id).expect("load"));
    assert_eq!(emulator.machine.kind(), ConsoleKind::Gba);
    assert_eq!(emulator.export_save_bytes(), Some(saved));
}

#[test]
fn a_crash_during_a_gba_game_writes_its_state_and_battery() {
    let root = store_root("crash");
    let mut emulator = on_store(&root);
    start(&mut emulator, &save_rom("eeprom8k"));
    played(&mut emulator, 1.2);
    emulator.running = false;
    played(&mut emulator, 0.1);
    assert_eq!(emulator.last_good.write_out(), Ok(Some(0)));

    let key = emulator.rom_key().expect("key").to_owned();
    let state = emulator.store.load_autosave(&key, 0).expect("autosave");
    assert_eq!(state[..4], GBA_STATE_MAGIC);
    let mut again = on_store(&root);
    start(&mut again, &save_rom("eeprom8k"));
    assert_eq!(chip(&mut again), Some(SaveType::Eeprom8K));
    assert!(again.load_autosave(0).expect("load"));
    run(&mut again, 1);
    run(&mut emulator, 1);
    assert_eq!(again.frame(), emulator.frame());
}

#[test]
fn a_second_gba_game_runs_linked_and_keeps_its_own_save() {
    let root = store_root("link");
    let mut emulator = on_store(&root);
    let rom = save_rom("sram");
    start(&mut emulator, &rom);
    let first_key = emulator.rom_key().expect("key").to_owned();
    emulator
        .start_link(&rom, ConsoleKind::Gba, Some("save-sram.gba"))
        .expect("link");
    let link = emulator.link.as_ref().expect("a linked game");
    assert_eq!(link.kind(), ConsoleKind::Gba);
    assert_eq!(link.save_key(), format!("{first_key}-link2"));
    let started = web_time::Instant::now();
    for step in 1..=20 {
        emulator.advance(started + std::time::Duration::from_millis(step * 50), None);
    }
    let link = emulator.link.as_ref().expect("a linked game");
    assert_eq!(link.machine.frame().len(), 240 * 160 * 4);
    assert_eq!(link.machine.frame(), emulator.machine.frame());
    emulator.flush_save().expect("flush");
    assert!(
        emulator
            .store
            .load_battery(&format!("{first_key}-link2"))
            .is_some()
    );
    emulator.end_link().expect("end");
    assert!(emulator.link.is_none());
}

const KEYINPUT: u32 = 0x0400_0130;

fn shoulders_down(emulator: &mut Emulator) -> (bool, bool) {
    emulator.machine.run_frame();
    let keyinput = emulator.as_gba().expect("gba").bus().read16(KEYINPUT);
    let owed = emulator.held.take_owed();
    emulator.apply(crate::held::Change {
        pressed: Keys::empty(),
        released: owed,
    });
    (
        keyinput & Keys::L.bits() == 0,
        keyinput & Keys::R.bits() == 0,
    )
}

fn gba_emulator(name: &str) -> Emulator {
    let mut emulator = on_store(&store_root(name));
    start(&mut emulator, &save_rom("flash64"));
    emulator
}

fn feed(emulator: &mut Emulator, events: Vec<PadEvent>) {
    let mut capture = BindingCapture::default();
    let mut pads = FakePads(events.into());
    emulator.handle_pads(&mut pads, &KeyBindings::default(), &mut capture, None);
}

fn pad_buttons(button: PadButton, down: bool) -> Vec<PadEvent> {
    let key = PadKey::Known(button);
    let input = if down {
        PadInput::Pressed(key)
    } else {
        PadInput::Released(key)
    };
    vec![PadEvent {
        pad: 0,
        input,
        code: None,
    }]
}

#[test]
fn the_keyboard_presses_l_and_r_on_the_gba() {
    let mut emulator = gba_emulator("shoulder-keys");
    let bindings = KeyBindings::default();
    let mut capture = BindingCapture::default();
    let ctx = egui::Context::default();
    let mut key = |emulator: &mut Emulator, key: egui::Key, pressed: bool| {
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            emulator.handle_input(
                ctx,
                &mut FakePads(Default::default()),
                &bindings,
                &mut capture,
                None,
            );
        });
    };
    for _ in 0..3 {
        key(&mut emulator, egui::Key::Num1, true);
        assert_eq!(shoulders_down(&mut emulator), (true, false));
        key(&mut emulator, egui::Key::Num3, true);
        assert_eq!(shoulders_down(&mut emulator), (true, true));
        key(&mut emulator, egui::Key::Num1, false);
        key(&mut emulator, egui::Key::Num3, false);
        assert_eq!(shoulders_down(&mut emulator), (false, false));
    }
}

#[test]
fn bumpers_and_triggers_both_press_l_and_r() {
    let mut emulator = gba_emulator("shoulder-pads");
    for (button, expected) in [
        (PadButton::LeftTrigger, (true, false)),
        (PadButton::LeftTrigger2, (true, false)),
        (PadButton::RightTrigger, (false, true)),
        (PadButton::RightTrigger2, (false, true)),
    ] {
        for _ in 0..3 {
            feed(&mut emulator, pad_buttons(button, true));
            assert_eq!(shoulders_down(&mut emulator), expected, "{button:?}");
            feed(&mut emulator, pad_buttons(button, false));
            assert_eq!(shoulders_down(&mut emulator), (false, false), "{button:?}");
        }
    }
}

#[test]
fn an_analog_trigger_presses_l_past_its_threshold_and_lets_go_below_the_lower_one() {
    use crate::pad_input::sdl_map::{SdlInput, Translator};
    let mut emulator = gba_emulator("shoulder-axis");
    let mut translator = Translator::default();
    let mut pull = |emulator: &mut Emulator, raw: i16| {
        let events = translator.translate(0, SdlInput::Axis(4, raw));
        feed(emulator, events);
        shoulders_down(emulator).0
    };
    for _ in 0..3 {
        assert!(!pull(&mut emulator, 16_000));
        assert!(pull(&mut emulator, 26_000));
        assert!(
            pull(&mut emulator, 22_000),
            "a hand that eases off a little keeps L"
        );
        assert!(pull(&mut emulator, 25_000));
        assert!(!pull(&mut emulator, 20_000));
        assert!(
            !pull(&mut emulator, 23_000),
            "below the press line L stays up"
        );
        assert!(!pull(&mut emulator, 0));
    }
}

#[test]
fn a_shoulder_held_on_the_keyboard_and_a_pad_stays_down_until_both_let_go() {
    let mut emulator = gba_emulator("shoulder-both");
    emulator.press(InputSource::Keyboard, Keys::L);
    feed(&mut emulator, pad_buttons(PadButton::LeftTrigger, true));
    assert_eq!(shoulders_down(&mut emulator), (true, false));
    feed(&mut emulator, pad_buttons(PadButton::LeftTrigger, false));
    assert_eq!(shoulders_down(&mut emulator), (true, false));
    emulator.release(InputSource::Keyboard, Keys::L);
    assert_eq!(shoulders_down(&mut emulator), (false, false));
}
