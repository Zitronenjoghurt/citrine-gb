use super::clock::{offset_label, shift_rtc};
use super::settings::{LightSource, lux_to_light};
use super::*;
use crate::emulator::Emulator;
use crate::storage::SaveStore;
use citrine_gb::gb::GbModel;
use citrine_gb::gb::cartridge::{CAMERA_PIXELS, CAMERA_WIDTH, Rtc};
use citrine_gb::rom::Rom;

const NOW: u64 = 1_761_400_000;

fn emulator(name: &str) -> Emulator {
    let root = std::env::temp_dir().join(format!(
        "citrine-gimmick-test-{}-{name}",
        std::process::id()
    ));
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

fn loaded(name: &str, cartridge_type: u8) -> Emulator {
    let mut emulator = emulator(name);
    emulator
        .load_rom(&rom(cartridge_type), GbModel::Dmg, None)
        .expect("load");
    emulator
}

fn rtc_seconds(emulator: &Emulator) -> i64 {
    let real = emulator.gb().cartridge.rtc().expect("clock").real;
    i64::from(real.days()) * 86_400
        + i64::from(real.hours) * 3600
        + i64::from(real.minutes) * 60
        + i64::from(real.seconds)
}

#[test]
fn each_cart_reports_the_gimmicks_it_has() {
    let rumble = loaded("rumble", 0x1C).cart();
    assert!(rumble.rumble && !rumble.tilt && !rumble.clock);
    let tilt = loaded("tilt", 0x22).cart();
    assert!(tilt.tilt && tilt.motion() && !tilt.rumble);
    let clock = loaded("clock", 0x10).cart();
    assert!(clock.clock && clock.clock_back && clock.printer);
    let huc3 = loaded("huc3", 0xFE).cart();
    assert!(huc3.clock && !huc3.clock_back && huc3.ir);
    let camera = loaded("camera", 0xFC).cart();
    assert!(camera.camera && camera.printer);
    let plain = loaded("plain", 0x00).cart();
    assert_eq!(
        plain,
        Cart {
            printer: true,
            ..Cart::default()
        }
    );
    assert_eq!(emulator("empty").cart(), Cart::default());
    assert!(!Cart::default().any());
}

#[test]
fn the_clock_moves_back_and_forward_by_whole_seconds() {
    let mut rtc = Rtc::default();
    shift_rtc(&mut rtc, 2 * 86_400 + 3 * 3600 + 61);
    assert_eq!(rtc.real.days(), 2);
    assert_eq!(
        (rtc.real.hours, rtc.real.minutes, rtc.real.seconds),
        (3, 1, 1)
    );
    shift_rtc(&mut rtc, -(86_400 + 3600 + 2));
    assert_eq!(rtc.real.days(), 1);
    assert_eq!(
        (rtc.real.hours, rtc.real.minutes, rtc.real.seconds),
        (2, 0, 59)
    );
    shift_rtc(&mut rtc, -10 * 86_400);
    assert_eq!(rtc.real.days(), 0);
    assert_eq!(
        (rtc.real.hours, rtc.real.minutes, rtc.real.seconds),
        (0, 0, 0)
    );
}

#[test]
fn moving_back_keeps_the_halt_and_carry_bits() {
    let mut rtc = Rtc::default();
    rtc.real.days_low = 0xFF;
    rtc.real.days_high = 0x81;
    shift_rtc(&mut rtc, -86_400);
    assert_eq!(rtc.real.days(), 510);
    assert_eq!(rtc.real.days_high & 0x80, 0x80);
    rtc.real.days_high |= 0x40;
    shift_rtc(&mut rtc, -86_400);
    assert_eq!(rtc.real.days(), 510);
}

#[test]
fn a_new_clock_offset_moves_the_game_clock_by_the_change() {
    let mut emulator = loaded("offset", 0x10);
    let key = emulator.rom_key().expect("key").to_owned();
    let before = rtc_seconds(&emulator);
    let mut settings = GimmickSettings::default();
    settings.set_clock_offset(&key, 5 * 3600);
    emulator.set_gimmick_settings(&settings);
    assert_eq!(rtc_seconds(&emulator), before + 5 * 3600);
    settings.set_clock_offset(&key, 3600);
    emulator.set_gimmick_settings(&settings);
    assert_eq!(rtc_seconds(&emulator), before + 3600);
    assert_eq!(emulator.gimmicks.clock_offset(), 3600);
}

#[test]
fn reloading_a_game_keeps_its_offset_without_moving_the_clock_again() {
    let mut emulator = loaded("reload", 0x10);
    let key = emulator.rom_key().expect("key").to_owned();
    let mut settings = GimmickSettings::default();
    settings.set_clock_offset(&key, 7200);
    emulator.set_gimmick_settings(&settings);
    emulator
        .load_rom(&rom(0x10), GbModel::Dmg, None)
        .expect("reload");
    assert_eq!(emulator.gimmicks.clock_offset(), 7200);
    assert!(rtc_seconds(&emulator) <= 7200);
}

#[test]
fn paused_time_catches_up_on_the_game_clock() {
    let mut emulator = loaded("paused", 0x10);
    let before = rtc_seconds(&emulator);
    emulator.gimmicks.follow_clock(&mut emulator.machine, NOW);
    emulator
        .gimmicks
        .follow_clock(&mut emulator.machine, NOW + 1);
    assert_eq!(rtc_seconds(&emulator), before);
    emulator
        .gimmicks
        .follow_clock(&mut emulator.machine, NOW + 601);
    assert_eq!(rtc_seconds(&emulator), before + 600);
}

#[test]
fn huc3_clocks_only_move_forward() {
    let mut emulator = loaded("huc3-clock", 0xFE);
    let tail = |emulator: &mut Emulator| {
        emulator
            .machine
            .battery(true, NOW)
            .map(|save| save.to_file())
            .expect("save")
    };
    let before = tail(&mut emulator);
    super::clock::shift(&mut emulator.machine, -3600, NOW);
    assert_eq!(tail(&mut emulator), before);
    super::clock::shift(&mut emulator.machine, 3600, NOW);
    assert_ne!(tail(&mut emulator), before);
}

#[test]
fn the_printer_follows_the_per_game_switch_and_the_link() {
    let mut emulator = loaded("printer", 0x00);
    let key = emulator.rom_key().expect("key").to_owned();
    assert!(!emulator.gb().printer_attached());
    let mut settings = GimmickSettings::default();
    settings.set_printer(&key, true);
    emulator.set_gimmick_settings(&settings);
    assert!(emulator.gb().printer_attached());
    emulator
        .start_link(&rom(0x00), GbModel::Dmg.into(), Some("two"))
        .expect("link");
    assert!(!emulator.gb().printer_attached());
    emulator.end_link().expect("end");
    assert!(emulator.gb().printer_attached());
    settings.set_printer(&key, false);
    emulator.set_gimmick_settings(&settings);
    assert!(!emulator.gb().printer_attached());
}

#[test]
fn the_camera_cart_takes_the_fed_frame() {
    let mut emulator = loaded("camera-feed", 0xFC);
    emulator.gimmicks.set_camera_frame(vec![1; 3]);
    assert!(emulator.gimmicks.camera_frame().is_none());
    emulator.gimmicks.set_camera_frame(vec![200; CAMERA_PIXELS]);
    emulator.gimmicks.feed(&mut emulator.machine);
    assert_eq!(
        emulator.gimmicks.camera_frame().map(<[u8]>::len),
        Some(CAMERA_PIXELS)
    );
}

#[test]
fn a_picture_fills_the_camera_frame_cropped_to_its_shape() {
    let width = 256;
    let height = 112;
    let luma: Vec<u8> = (0..width * height)
        .map(|i| if i % width < 128 { 0 } else { 255 })
        .collect();
    let frame = camera::from_luma(width, height, &luma, false).expect("frame");
    assert_eq!(frame.len(), CAMERA_PIXELS);
    assert_eq!(frame[0], 0);
    assert_eq!(frame[CAMERA_WIDTH - 1], 255);
    let mirrored = camera::from_luma(width, height, &luma, true).expect("frame");
    assert_eq!(mirrored[0], 255);
    assert!(camera::from_luma(0, 0, &[], false).is_none());
    let rgba = vec![255u8; 4 * 4 * 4];
    let white = camera::from_rgba(4, 4, &rgba, false).expect("frame");
    assert!(white.iter().all(|&v| v >= 254));
}

#[test]
fn a_png_picture_turns_into_a_camera_frame() {
    let png = crate::utils::screenshot::encode_png(2, 2, &[90; 16]).expect("png");
    let frame = camera::from_picture(&png).expect("frame");
    assert_eq!(frame.len(), CAMERA_PIXELS);
    assert!(camera::from_picture(b"not a picture").is_none());
}

#[test]
fn rumble_reaches_the_app_and_stops_with_the_game() {
    let mut emulator = loaded("rumble-stop", 0x1C);
    emulator.gimmicks.rumble = 0.6;
    emulator.running = false;
    emulator.advance(web_time::Instant::now(), None);
    assert_eq!(emulator.gimmicks.rumble(), 0.0);
    assert_eq!(rumble_step(0.0), 0);
    assert_eq!(rumble_step(0.01), 1);
    assert_eq!(rumble_step(0.6), 3);
    assert_eq!(rumble_step(4.0), 4);
    assert_eq!(rumble_step(f32::NAN), 0);
}

#[test]
fn motion_stays_within_one_g() {
    let motion = Motion {
        x: 3.0,
        y: f32::NAN,
        turn: -2.0,
    }
    .clamped();
    assert_eq!(
        motion,
        Motion {
            x: 1.0,
            y: 0.0,
            turn: -1.0
        }
    );
}

#[test]
fn light_comes_from_the_sensor_or_the_slider() {
    let mut settings = GimmickSettings::default();
    assert_eq!(settings.light_level(Some(0.9)), 0.9);
    assert_eq!(settings.light_level(None), settings.light);
    settings.light_source = LightSource::Slider;
    settings.light = 0.25;
    assert_eq!(settings.light_level(Some(0.9)), 0.25);
    assert_eq!(lux_to_light(0.0), 0.0);
    assert_eq!(lux_to_light(100_000.0), 1.0);
    assert!(lux_to_light(1000.0) > 0.5);
}

#[test]
fn offsets_read_as_days_and_hours() {
    assert_eq!(offset_label(0), "Device time");
    assert_eq!(offset_label(3 * 3600), "+3 h");
    assert_eq!(offset_label(-2 * 86_400), "-2 d");
    assert_eq!(offset_label(86_400 + 5 * 3600), "+1 d 5 h");
    let mut settings = GimmickSettings::default();
    settings.set_clock_offset("game", i64::MAX);
    assert_eq!(settings.clock_offset("game"), settings::MAX_CLOCK_OFFSET);
    settings.set_clock_offset("game", 0);
    assert!(settings.clock_offsets.is_empty());
}

#[test]
fn unloading_keeps_the_gimmick_settings() {
    let mut emulator = loaded("unload", 0x00);
    let settings = GimmickSettings {
        rumble: false,
        ..GimmickSettings::default()
    };
    emulator.set_gimmick_settings(&settings);
    emulator.unload();
    assert!(!emulator.gimmick_settings().rumble);
}

#[cfg(feature = "gba")]
mod gba {
    use super::*;
    use citrine_gb::gba::machine::Gba;
    use citrine_gb::machine::Machine;

    fn gba_rom(code: &[u8; 3]) -> Vec<u8> {
        let mut data = vec![0u8; 0x400];
        data[0xAC..0xAF].copy_from_slice(code);
        data
    }

    fn gba_emulator(name: &str, code: &[u8; 3]) -> Emulator {
        let mut emulator = emulator(name);
        emulator.machine = Machine::Gba(Box::new(Gba::new(&gba_rom(code))));
        emulator.rom_key = Some(format!("gba-{name}"));
        emulator.start_gimmicks();
        emulator
    }

    #[test]
    fn gba_carts_report_their_sensors() {
        let boktai = gba_emulator("boktai", b"U3I").cart();
        assert!(boktai.clock && boktai.light && !boktai.printer);
        let twisted = gba_emulator("twisted", b"RZW").cart();
        assert!(twisted.turn && twisted.rumble && twisted.motion());
        let yoshi = gba_emulator("yoshi", b"KYG").cart();
        assert!(yoshi.tilt && !yoshi.clock);
    }

    #[test]
    fn the_gba_clock_shows_device_time_plus_the_offset() {
        let mut emulator = gba_emulator("ruby", b"AXV");
        let key = emulator.rom_key().expect("key").to_owned();
        let Machine::Gba(gba) = &emulator.machine else {
            panic!("gba");
        };
        let start = gba.clock();
        let mut settings = GimmickSettings::default();
        settings.set_clock_offset(&key, 86_400);
        emulator.set_gimmick_settings(&settings);
        let Machine::Gba(gba) = &emulator.machine else {
            panic!("gba");
        };
        let moved = gba.clock();
        assert_ne!((start.day, start.month), (moved.day, moved.month));
        assert_eq!(start.hour, moved.hour);
    }

    #[test]
    fn gba_sensors_take_the_fed_values() {
        let mut emulator = gba_emulator("sensors", b"U3I");
        emulator.gimmicks.light = 0.8;
        emulator.gimmicks.motion = Motion {
            x: 0.5,
            y: -0.5,
            turn: 0.3,
        };
        emulator.gimmicks.feed(&mut emulator.machine);
        emulator.gimmicks.collect(&mut emulator.machine);
        assert_eq!(emulator.gimmicks.rumble(), 0.0);
        assert!(emulator.printer_free());
    }
}
