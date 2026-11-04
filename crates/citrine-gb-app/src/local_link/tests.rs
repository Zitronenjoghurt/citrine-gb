use super::*;
use crate::emulator::Emulator;
use citrine_gb::ReadMemory;
use citrine_gb::gb::GbModel;
use egui::Key;
use std::collections::HashSet;
use strum::IntoEnumIterator;

const COUNT: u16 = 300;

fn trade_program(sc: u8, first: u8, step: u8) -> Vec<u8> {
    let [low, high] = COUNT.to_le_bytes();
    let mut code = vec![0x3E, 0x0A, 0xEA, 0x00, 0x00, 0xAF, 0xEA, 0x00, 0xDF];
    code.extend_from_slice(&[0x3E, 0x10, 0xE0, 0x00, 0xF0, 0x00, 0xCB, 0x47, 0x20, 0xFA]);
    code.extend_from_slice(&[0x21, 0x00, 0xA0, 0x11, low, high, 0x06, first]);
    let top = code.len();
    code.extend_from_slice(&[0x78, 0xE0, 0x01]);
    if sc == 0x81 {
        code.extend_from_slice(&[0x0E, 0x28, 0x0D, 0x20, 0xFD]);
    }
    code.extend_from_slice(&[0x3E, sc, 0xE0, 0x02, 0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA]);
    code.extend_from_slice(&[0xF0, 0x01, 0x22, 0x78, 0xC6, step, 0x47]);
    code.extend_from_slice(&[0x1B, 0x7A, 0xB3, 0x20]);
    let back = top as i32 - (code.len() as i32 + 1);
    code.push(back as i8 as u8);
    code.extend_from_slice(&[0x3E, 0xAA, 0xEA, 0x00, 0xDF]);
    code
}

fn cart(cgb_flag: u8, kind: u8, code: &[u8]) -> Rom {
    let mut data = vec![0u8; 0x8000];
    data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    data[0x0143] = cgb_flag;
    data[0x0147] = kind;
    data[0x0149] = 0x02;
    data[0x0150..0x0150 + code.len()].copy_from_slice(code);
    let end = 0x0150 + code.len();
    data[end..end + 2].copy_from_slice(&[0x18, 0xFE]);
    Rom::new(&data)
}

fn red() -> Rom {
    cart(0x00, 0x03, &trade_program(0x81, 0x10, 1))
}

fn silver() -> Rom {
    cart(0x80, 0x10, &trade_program(0x80, 0x40, 3))
}

fn sent(first: u8, step: u8) -> Vec<u8> {
    (0..COUNT)
        .map(|i| first.wrapping_add((i as u8).wrapping_mul(step)))
        .collect()
}

fn done(gb: &GameBoy) -> bool {
    gb.memory.read_naive(0xDF00) == 0xAA
}

fn emulator(name: &str) -> Emulator {
    let root =
        std::env::temp_dir().join(format!("citrine-link-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut emulator = Emulator::default();
    emulator.store = SaveStore::at(root);
    emulator
}

fn linked(name: &str) -> Emulator {
    let mut emulator = emulator(name);
    emulator
        .load_rom(&red(), GbModel::Dmg, None)
        .expect("load red");
    emulator
        .start_link(&silver(), ConsoleKind::Gbc, Some("Silver"))
        .expect("link silver");
    emulator
}

fn link(emulator: &mut Emulator) -> &mut LocalLink {
    emulator.link.as_mut().expect("a linked game")
}

fn second(link: &mut LocalLink) -> &mut GameBoy {
    link.machine.as_gb_mut().expect("a game boy")
}

fn trade(emulator: &mut Emulator) {
    let start = Instant::now();
    let finished = (1..=200).any(|step| {
        emulator.advance(start + Duration::from_millis(step * 50), None);
        let second = emulator
            .link
            .as_ref()
            .and_then(|link| link.machine.as_gb())
            .expect("a linked game");
        done(emulator.gb()) && done(second)
    });
    assert!(finished, "the trade did not finish in 10 seconds");
}

fn press_a_on_both(emulator: &mut Emulator) {
    emulator.press(InputSource::Keyboard, Keys::A);
    link(emulator).set_held(InputSource::Keyboard, Keys::A, Keys::A);
}

fn store_holds(emulator: &Emulator, key: &str, expected: &[u8]) {
    let battery = emulator.store.load_battery(key).expect("a battery save");
    assert_eq!(&battery[..COUNT as usize], expected);
}

#[test]
fn red_and_silver_trade_and_both_saves_hold_the_other_game_after_the_link_ends() {
    let mut emulator = linked("trade");
    assert_eq!(emulator.gb().model, GbModel::Dmg);
    assert_eq!(link(&mut emulator).kind(), ConsoleKind::Gbc);
    assert!(link(&mut emulator).plugged());
    press_a_on_both(&mut emulator);
    trade(&mut emulator);

    let red_key = emulator.rom_key().expect("red key").to_owned();
    let silver_key = link(&mut emulator).rom_key().to_owned();
    link(&mut emulator).started_at -= Duration::from_secs(10);
    emulator.end_link().expect("end the link");
    emulator.flush_save().expect("save red");

    assert!(emulator.link.is_none());
    assert!(!emulator.gb().link_plugged());
    store_holds(&emulator, &red_key, &sent(0x40, 3));
    store_holds(&emulator, &silver_key, &sent(0x10, 1));
    assert_eq!(emulator.store.autosave_ids(&silver_key).len(), 1);
}

#[test]
fn the_second_game_loads_its_battery_save_like_it_does_alone() {
    let mut emulator = linked("battery");
    let silver_key = link(&mut emulator).rom_key().to_owned();
    emulator.end_link().expect("end the link");
    let saved: Vec<u8> = (0..0x2000).map(|i| (i % 251) as u8).collect();
    emulator
        .store
        .store_battery(&silver_key, &saved)
        .expect("store");
    emulator
        .start_link(&silver(), ConsoleKind::Gbc, Some("Silver"))
        .expect("link again");
    let dump = second(link(&mut emulator))
        .poll_sram_dump(true, now_unix())
        .expect("a battery save");
    assert_eq!(&dump.as_slice()[..saved.len()], &saved[..]);
}

#[test]
fn a_copy_of_the_same_game_keeps_its_own_save_and_leaves_the_first_one_alone() {
    let mut emulator = emulator("copy");
    emulator
        .load_rom(&red(), GbModel::Dmg, None)
        .expect("load red");
    let red_key = emulator.rom_key().expect("red key").to_owned();
    let saved: Vec<u8> = (0..0x2000).map(|i| (i % 251) as u8).collect();
    emulator
        .store
        .store_battery(&red_key, &saved)
        .expect("store");
    emulator
        .start_link(&red(), ConsoleKind::Gb, Some("Red"))
        .expect("link a copy");
    assert_eq!(link(&mut emulator).rom_key(), red_key);
    let copy_key = link(&mut emulator).save_key().to_owned();
    assert_ne!(copy_key, red_key);
    let dump = second(link(&mut emulator)).poll_sram_dump(true, now_unix());
    assert!(dump.is_none_or(|d| d.as_slice()[..saved.len()] != saved[..]));
    link(&mut emulator).started_at -= Duration::from_secs(10);
    emulator.end_link().expect("end the link");
    assert_eq!(emulator.store.load_battery(&red_key), Some(saved));
}

#[test]
fn with_the_cable_pulled_the_first_game_trades_with_no_one() {
    let mut emulator = linked("cable");
    link(&mut emulator).set_plugged(false);
    press_a_on_both(&mut emulator);
    let start = Instant::now();
    for step in 1..=40 {
        emulator.advance(start + Duration::from_millis(step * 50), None);
    }
    assert!(done(emulator.gb()));
    assert!(!done(second(link(&mut emulator))));
    assert!(!emulator.gb().link_plugged());
}

#[test]
fn closing_the_second_game_keeps_the_first_running_unplugged() {
    let mut emulator = linked("close");
    let red_key = emulator.rom_key().expect("red key").to_owned();
    emulator.end_link().expect("end the link");
    assert_eq!(emulator.rom_key(), Some(red_key.as_str()));
    assert!(emulator.running);
    assert!(!emulator.gb().link_plugged());
}

#[test]
fn the_split_keyboard_halves_cover_every_button_without_sharing_a_key() {
    let settings = LinkSettings::default();
    let mut seen = HashSet::new();
    for half in Half::BOTH {
        for button in GbButton::iter() {
            let keys = settings.keys_for(half, button);
            assert!(!keys.is_empty(), "{} has no {button}", half.label());
            for key in keys {
                assert!(seen.insert(key), "{key:?} is in both halves");
            }
        }
    }
}

#[test]
fn binding_a_key_to_one_half_takes_it_from_the_other() {
    let mut settings = LinkSettings::default();
    settings.bind_key(Half::Right, Key::W, GbButton::B);
    assert!(settings.keys_for(Half::Left, GbButton::Up).is_empty());
    assert_eq!(
        settings.keys_for(Half::Right, GbButton::B),
        [Key::K, Key::W]
    );
    assert!(!settings.keys_are_default());
    settings.reset_keys();
    assert!(settings.keys_are_default());
}

fn link_with(controls: Controls, swapped: bool) -> LocalLink {
    let root = std::env::temp_dir().join(format!("citrine-link-routing-{}", std::process::id()));
    let store = SaveStore::at(root);
    let mut link = LocalLink::start(
        &silver(),
        ConsoleKind::Gbc,
        &Firmware::default(),
        Some("Silver"),
        &store,
        48_000,
        None,
    )
    .expect("start");
    link.set_settings(&LinkSettings {
        controls,
        swapped,
        ..LinkSettings::default()
    });
    link
}

#[test]
fn a_split_keyboard_sends_each_half_to_its_own_game() {
    let bindings = KeyBindings::default();
    let link = link_with(Controls::SplitKeyboard, false);
    assert_eq!(
        link.key(Key::W, &bindings),
        Some((Game::First, GbButton::Up))
    );
    assert_eq!(
        link.key(Key::L, &bindings),
        Some((Game::Second, GbButton::A))
    );
    assert_eq!(link.key(Key::Q, &bindings), None);
    let swapped = link_with(Controls::SplitKeyboard, true);
    assert_eq!(
        swapped.key(Key::W, &bindings),
        Some((Game::Second, GbButton::Up))
    );
}

#[test]
fn keyboard_and_controller_each_drive_one_game() {
    let bindings = KeyBindings::default();
    let key = bindings.keys_for(GbButton::A)[0];
    let mut link = link_with(Controls::KeyboardAndController, false);
    assert_eq!(link.key(key, &bindings), Some((Game::First, GbButton::A)));
    assert_eq!(link.game_for(Source::Pad(3)), Game::Second);
    assert_eq!(link.game_for(Source::Touch), Game::First);
    assert_eq!(link.driver(Game::First), Some("Keyboard"));
    assert_eq!(link.driver(Game::Second), Some("Controller"));
}

#[test]
fn two_controllers_take_the_games_in_the_order_they_are_first_used() {
    let mut link = link_with(Controls::TwoControllers, false);
    assert_eq!(link.game_for(Source::Pad(5)), Game::First);
    assert_eq!(link.game_for(Source::Pad(2)), Game::Second);
    assert_eq!(link.game_for(Source::Pad(5)), Game::First);
    assert_eq!(link.game_for(Source::Pad(7)), Game::First);
}

#[test]
fn one_input_for_both_follows_the_game_with_control() {
    let bindings = KeyBindings::default();
    let key = bindings.keys_for(GbButton::A)[0];
    let mut link = link_with(Controls::Shared, false);
    assert!(link.takes_clicks());
    assert_eq!(link.driver(Game::First), Some("Playing here"));
    assert_eq!(link.driver(Game::Second), None);
    assert!(link.set_control(Game::Second));
    assert!(!link.set_control(Game::Second));
    assert_eq!(link.key(key, &bindings), Some((Game::Second, GbButton::A)));
    assert_eq!(link.game_for(Source::Pad(0)), Game::Second);
    assert_eq!(link.game_for(Source::Touch), Game::Second);
    assert_eq!(link.driver(Game::Second), Some("Playing here"));
}

#[test]
fn moving_control_lets_go_of_what_the_old_game_held() {
    let mut emulator = linked("release");
    emulator.press(InputSource::Keyboard, Keys::A);
    assert!(emulator.is_held(Keys::A));
    emulator.set_link_control(Game::Second);
    assert!(!emulator.is_held(Keys::A));
    emulator.press(InputSource::Touch, Keys::B);
    assert!(link(&mut emulator).held().contains(Keys::B));
    assert!(!emulator.is_held(Keys::B));
}

#[test]
fn the_second_sound_mixes_in_quieter_or_not_at_all() {
    let mut link = link_with(Controls::Shared, false);
    second(&mut link).apu.audio_buffer = vec![1.0; 8];
    let mut samples = vec![0.0; 8];
    link.mix(&mut samples);
    assert!(samples.iter().all(|s| *s > 0.0 && *s < 1.0));
    link.set_settings(&LinkSettings {
        second_sound: false,
        ..LinkSettings::default()
    });
    second(&mut link).apu.audio_buffer = vec![1.0; 8];
    let mut samples = vec![0.0; 8];
    link.mix(&mut samples);
    assert!(samples.iter().all(|s| *s == 0.0));
}

mod real_trade;

#[test]
fn two_infrared_carts_see_each_others_light() {
    let shining = cart(
        0x00,
        0xFF,
        &[0x3E, 0x0E, 0xEA, 0x00, 0x00, 0x3E, 0x01, 0xEA, 0x00, 0xA0],
    );
    let watching = cart(
        0x00,
        0xFF,
        &[
            0x3E, 0x0E, 0xEA, 0x00, 0x00, 0xFA, 0x00, 0xA0, 0xEA, 0x00, 0xC0, 0x18, 0xF8,
        ],
    );
    let mut first = GameBoy::new_empty(GbModel::Dmg);
    first.load_rom_with_model(&shining, GbModel::Dmg).unwrap();
    let mut second = GameBoy::new_empty(GbModel::Dmg);
    second.load_rom_with_model(&watching, GbModel::Dmg).unwrap();
    for _ in 0..3 {
        infrared_frame([&mut first, &mut second], [Keys::empty(); 2]);
    }
    assert!(first.ir_led());
    assert_eq!(second.memory.read_naive(0xC000), 0xC1);
}
