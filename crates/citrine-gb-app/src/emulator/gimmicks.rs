use citrine_gb::console::Console;
use citrine_gb::gb::GameBoy;
use citrine_gb::gb::printer::PrintedImage;
use citrine_gb::machine::Machine;

pub mod camera;
pub mod clock;
pub mod settings;

pub use settings::{CameraSource, GimmickSettings, LightSource};

const CATCH_UP_AFTER: u64 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cart {
    pub clock: bool,
    pub clock_back: bool,
    pub rumble: bool,
    pub tilt: bool,
    pub turn: bool,
    pub light: bool,
    pub camera: bool,
    pub ir: bool,
    pub printer: bool,
}

impl Cart {
    pub fn of(machine: &Machine) -> Self {
        match machine {
            Machine::Gb(gb) => Self::of_gb(gb),
            #[cfg(feature = "gba")]
            Machine::Gba(gba) => {
                let devices = gba.cart_devices();
                Self {
                    clock: devices.rtc,
                    clock_back: devices.rtc,
                    rumble: devices.rumble,
                    tilt: devices.tilt,
                    turn: devices.gyro,
                    light: devices.solar,
                    ..Self::default()
                }
            }
            #[cfg(feature = "nds")]
            Machine::Nds(_) => Self::default(),
            #[cfg(not(feature = "nds"))]
            #[allow(unreachable_patterns)]
            _ => Self::default(),
        }
    }

    fn of_gb(gb: &GameBoy) -> Self {
        if !gb.cartridge.has_rom_loaded {
            return Self::default();
        }
        Self {
            clock: gb.has_clock() || clock::has_huc3_clock(gb),
            clock_back: gb.has_clock(),
            rumble: gb.has_rumble(),
            tilt: gb.has_tilt(),
            camera: gb.has_camera(),
            ir: gb.cartridge.has_ir(),
            printer: true,
            ..Self::default()
        }
    }

    pub fn any(&self) -> bool {
        self.clock || self.rumble || self.motion() || self.light || self.camera || self.printer
    }

    pub fn motion(&self) -> bool {
        self.tilt || self.turn
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Motion {
    pub x: f32,
    pub y: f32,
    pub turn: f32,
}

impl Motion {
    pub fn clamped(self) -> Self {
        let unit = |v: f32| {
            if v.is_finite() {
                v.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        };
        Self {
            x: unit(self.x),
            y: unit(self.y),
            turn: unit(self.turn),
        }
    }
}

const TILT_KEYS: [(egui::Key, f32, f32); 4] = [
    (egui::Key::J, -1.0, 0.0),
    (egui::Key::L, 1.0, 0.0),
    (egui::Key::I, 0.0, -1.0),
    (egui::Key::K, 0.0, 1.0),
];
const KEY_TILT_PER_SECOND: f32 = 4.0;

#[derive(Default)]
pub struct Gimmicks {
    pub motion: Motion,
    keys: [bool; 4],
    key_tilt: [f32; 2],
    stick: [f32; 2],
    pub sensed: Option<Motion>,
    pub light: f32,
    camera: Option<Vec<u8>>,
    rumble: f32,
    prints: Vec<PrintedImage>,
    clock_seen: Option<u64>,
    clock_offset: i64,
}

impl Gimmicks {
    pub fn tilt_key(&mut self, key: egui::Key, down: bool) -> bool {
        let Some(index) = TILT_KEYS.iter().position(|(k, ..)| *k == key) else {
            return false;
        };
        self.keys[index] = down;
        true
    }

    pub fn stick(&mut self, x: Option<f32>, y: Option<f32>) {
        if let Some(x) = x {
            self.stick[0] = x;
        }
        if let Some(y) = y {
            self.stick[1] = y;
        }
    }

    pub fn tilting_by_hand(&self) -> bool {
        self.keys.iter().any(|&down| down) || self.key_tilt != [0.0; 2]
    }

    pub fn update_motion(&mut self, seconds: f32) {
        let mut target = [0.0f32; 2];
        for ((_, x, y), down) in TILT_KEYS.iter().zip(self.keys) {
            if down {
                target[0] += x;
                target[1] += y;
            }
        }
        let step = KEY_TILT_PER_SECOND * seconds.clamp(0.0, 0.25);
        for (tilt, target) in self.key_tilt.iter_mut().zip(target) {
            *tilt = if (*tilt - target).abs() <= step {
                target
            } else {
                *tilt + step * (target - *tilt).signum()
            };
        }
        let sensed = self.sensed.unwrap_or_default();
        self.motion = Motion {
            x: sensed.x + self.key_tilt[0] + self.stick[0],
            y: sensed.y + self.key_tilt[1] + self.stick[1],
            turn: sensed.turn,
        }
        .clamped();
    }

    pub fn set_camera_frame(&mut self, frame: Vec<u8>) {
        if frame.len() == citrine_gb::gb::cartridge::CAMERA_PIXELS {
            self.camera = Some(frame);
        }
    }

    pub fn camera_frame(&self) -> Option<&[u8]> {
        self.camera.as_deref()
    }

    pub fn clear_camera(&mut self) {
        self.camera = None;
    }

    pub fn rumble(&self) -> f32 {
        self.rumble
    }

    pub fn take_prints(&mut self) -> Vec<PrintedImage> {
        std::mem::take(&mut self.prints)
    }

    pub fn clock_offset(&self) -> i64 {
        self.clock_offset
    }

    pub(crate) fn reset(&mut self) {
        self.rumble = 0.0;
        self.prints.clear();
        self.clock_seen = None;
        self.clock_offset = 0;
    }

    pub(crate) fn start_clock(&mut self, machine: &mut Machine, offset: i64, now: u64) {
        self.clock_offset = offset;
        self.clock_seen = Some(now);
        if let Some(local) = clock::local_unix(now) {
            clock::set_absolute(machine, local + offset);
        }
    }

    pub(crate) fn set_clock_offset(&mut self, machine: &mut Machine, offset: i64, now: u64) {
        let delta = offset - self.clock_offset;
        self.clock_offset = offset;
        if delta == 0 {
            return;
        }
        if clock::is_absolute(machine) {
            if let Some(local) = clock::local_unix(now) {
                clock::set_absolute(machine, local + offset);
            }
        } else {
            clock::shift(machine, delta, now);
        }
    }

    #[cfg(feature = "nds")]
    pub(crate) fn keep_clock_offset(&mut self, offset: i64) {
        self.clock_offset = offset;
    }

    pub(crate) fn follow_clock(&mut self, machine: &mut Machine, now: u64) {
        let Some(seen) = self.clock_seen.replace(now) else {
            return;
        };
        let gap = now.saturating_sub(seen);
        if gap < CATCH_UP_AFTER {
            return;
        }
        if clock::is_absolute(machine) {
            if let Some(local) = clock::local_unix(now) {
                clock::set_absolute(machine, local + self.clock_offset);
            }
        } else {
            clock::shift(machine, gap as i64, now);
        }
    }

    pub(crate) fn feed(&self, machine: &mut Machine) {
        let motion = self.motion.clamped();
        match machine {
            Machine::Gb(gb) => {
                if gb.has_tilt() {
                    gb.set_tilt(motion.x, motion.y);
                }
                if gb.has_camera()
                    && let Some(frame) = &self.camera
                {
                    gb.set_camera_frame(frame);
                }
            }
            #[cfg(feature = "gba")]
            Machine::Gba(gba) => {
                gba.set_tilt(motion.x, motion.y);
                gba.set_turn(motion.turn);
                gba.set_sunlight(self.light);
            }
            #[cfg(feature = "nds")]
            Machine::Nds(_) => {}
            #[cfg(not(feature = "nds"))]
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    pub(crate) fn take_catch(&mut self, catch: &mut Catch) {
        if let Some(rumble) = catch.rumble.take() {
            self.rumble = rumble;
        }
        self.prints.append(&mut catch.prints);
    }

    pub(crate) fn stop_rumble(&mut self) {
        self.rumble = 0.0;
    }
}

#[derive(Default)]
pub(crate) struct Catch {
    rumble: Option<f32>,
    prints: Vec<PrintedImage>,
}

impl Catch {
    pub(crate) fn collect(&mut self, machine: &mut Machine) {
        match machine {
            Machine::Gb(gb) => {
                self.rumble = Some(if gb.has_rumble() {
                    gb.take_rumble()
                } else {
                    0.0
                });
                if gb.printer_attached() {
                    self.prints.extend(gb.take_prints());
                }
            }
            #[cfg(feature = "gba")]
            Machine::Gba(gba) => {
                self.rumble = Some(if gba.rumble() { 1.0 } else { 0.0 });
            }
            #[cfg(feature = "nds")]
            Machine::Nds(_) => {}
            #[cfg(not(feature = "nds"))]
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }
}

impl super::Emulator {
    pub fn cart(&self) -> Cart {
        if self.rom_key.is_none() {
            return Cart::default();
        }
        self.seat.facts().cart
    }

    pub fn gimmick_settings(&self) -> &GimmickSettings {
        &self.gimmick_settings
    }

    pub fn printer_free(&self) -> bool {
        !self.linked() && !self.partnered()
    }

    pub fn set_gimmick_settings(&mut self, settings: &GimmickSettings) {
        if *settings == self.gimmick_settings {
            return;
        }
        self.gimmick_settings = settings.clone();
        let Some(key) = self.rom_key.clone() else {
            return;
        };
        let offset = self.clock_offset(&key);
        self.gimmicks.set_clock_offset(
            &mut self.seat.core_mut().machine,
            offset,
            crate::storage::now_unix(),
        );
        self.attach_printer();
    }

    fn clock_offset(&self, rom_key: &str) -> i64 {
        #[cfg(feature = "nds")]
        if matches!(self.seat.core().machine, Machine::Nds(_)) {
            return self.ds_clock;
        }
        self.gimmick_settings.clock_offset(rom_key)
    }

    #[cfg(feature = "nds")]
    pub fn set_ds_clock(&mut self, offset: i64) {
        if offset == self.ds_clock {
            return;
        }
        self.ds_clock = offset;
        let machine = &mut self.seat.core_mut().machine;
        if matches!(machine, Machine::Nds(_)) && self.rom_key.is_some() {
            self.gimmicks
                .set_clock_offset(machine, offset, crate::storage::now_unix());
        }
    }

    #[cfg(all(test, feature = "nds"))]
    pub(crate) fn nds_mut(&mut self) -> Option<&mut citrine_gb::nds::machine::Nds> {
        match &mut self.seat.core_mut().machine {
            Machine::Nds(nds) => Some(nds),
            _ => None,
        }
    }

    #[cfg(feature = "nds")]
    pub fn take_ds_clock_set(&mut self) -> Option<i64> {
        if self.seat.is_home() {
            self.catch_ds_clock_set();
        }
        let offset = self.ds_clock_set.take()?;
        self.ds_clock = offset;
        self.gimmicks.keep_clock_offset(offset);
        Some(offset)
    }

    #[cfg(feature = "nds")]
    pub(super) fn catch_ds_clock_set(&mut self) {
        let Machine::Nds(nds) = &mut self.seat.core_mut().machine else {
            return;
        };
        let Some(set) = nds.take_clock_set() else {
            return;
        };
        if let Some(local) = clock::local_unix(crate::storage::now_unix()) {
            self.ds_clock_set = Some(set - local);
        }
    }

    pub(crate) fn start_gimmicks(&mut self) {
        self.gimmicks.reset();
        let Some(key) = self.rom_key.clone() else {
            return;
        };
        let offset = self.clock_offset(&key);
        self.gimmicks.start_clock(
            &mut self.seat.core_mut().machine,
            offset,
            crate::storage::now_unix(),
        );
        self.attach_printer();
    }

    pub(crate) fn attach_printer(&mut self) {
        let wanted = self
            .rom_key
            .as_deref()
            .is_some_and(|key| self.gimmick_settings.printer(key))
            && self.printer_free()
            && self.cart().printer;
        if let Some(gb) = self.seat.core_mut().machine.as_gb_mut()
            && gb.printer_attached() != wanted
        {
            gb.attach_printer(wanted);
        }
    }
}

pub fn rumble_step(level: f32) -> u8 {
    if !level.is_finite() || level <= 0.0 {
        return 0;
    }
    ((level.min(1.0) * 4.0).ceil() as u8).max(1)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::emulator::Emulator;
    use crate::storage::SaveStore;
    use citrine_gb::gb::GbModel;
    use citrine_gb::gb::cartridge::{CAMERA_PIXELS, CAMERA_WIDTH};
    use citrine_gb::rom::Rom;

    pub(super) const NOW: u64 = 1_761_400_000;

    pub(super) fn emulator(name: &str) -> Emulator {
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

    pub(super) fn rom(cartridge_type: u8) -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x0147] = cartridge_type;
        data[0x0149] = 0x02;
        Rom::new(&data)
    }

    pub(super) fn loaded(name: &str, cartridge_type: u8) -> Emulator {
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
        emulator
            .gimmicks
            .follow_clock(&mut emulator.seat.core_mut().machine, NOW);
        emulator
            .gimmicks
            .follow_clock(&mut emulator.seat.core_mut().machine, NOW + 1);
        assert_eq!(rtc_seconds(&emulator), before);
        emulator
            .gimmicks
            .follow_clock(&mut emulator.seat.core_mut().machine, NOW + 601);
        assert_eq!(rtc_seconds(&emulator), before + 600);
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
        emulator
            .gimmicks
            .feed(&mut emulator.seat.core_mut().machine);
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
            emulator.seat.core_mut().machine = Machine::Gba(Box::new(Gba::new(&gba_rom(code))));
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
            let Machine::Gba(gba) = emulator.debug() else {
                panic!("gba");
            };
            let start = gba.clock();
            let mut settings = GimmickSettings::default();
            settings.set_clock_offset(&key, 86_400);
            emulator.set_gimmick_settings(&settings);
            let Machine::Gba(gba) = emulator.debug() else {
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
            emulator
                .gimmicks
                .feed(&mut emulator.seat.core_mut().machine);
            {
                let core = emulator.seat.core_mut();
                core.catch.collect(&mut core.machine);
                emulator.gimmicks.take_catch(&mut core.catch);
            }
            assert_eq!(emulator.gimmicks.rumble(), 0.0);
            assert!(emulator.printer_free());
        }
    }

    #[cfg(feature = "nds")]
    pub(crate) mod nds {
        use super::*;
        use citrine_gb::console::ConsoleKind;
        use citrine_gb::machine::{Firmware, Machine};
        use citrine_gb::nds::machine::Nds;

        const SELECT: u16 = 1 << 2;
        const PIN_CLOCK: u16 = 1 << 1;
        const DATA_WRITE: u16 = 1 << 4;
        const WRITE_TIME: u8 = 0x66;
        const YEAR: i64 = 365 * 86_400;

        fn ds_emulator(name: &str, offset: i64) -> Emulator {
            let mut emulator = emulator(name);
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/nds/blocksds-console-both-screens.nds");
            let rom = Rom::new(&std::fs::read(path).expect("the DS fixture"));
            emulator.seat.core_mut().machine =
                Machine::load(&rom, ConsoleKind::Nds, &Firmware::default()).expect("a DS game");
            emulator.rom_key = Some(format!("nds-{name}"));
            emulator.set_ds_clock(offset);
            emulator.start_gimmicks();
            emulator
        }

        fn nds(emulator: &mut Emulator) -> &mut Nds {
            let Machine::Nds(nds) = &mut emulator.seat.core_mut().machine else {
                panic!("nds");
            };
            nds
        }

        fn ds_clock(emulator: &mut Emulator) -> i64 {
            nds(emulator).buses().rtc().unix()
        }

        fn device_time() -> i64 {
            clock::local_unix(crate::storage::now_unix()).expect("local time")
        }

        fn near(a: i64, b: i64) -> bool {
            (a - b).abs() <= 5
        }

        pub(crate) fn game_sets_time(nds: &mut Nds, bytes: [u8; 3]) {
            let rtc = nds.buses_mut().rtc_mut();
            rtc.write(PIN_CLOCK | DATA_WRITE, 0xFFFF);
            rtc.write(SELECT | PIN_CLOCK | DATA_WRITE, 0xFFFF);
            for byte in std::iter::once(WRITE_TIME).chain(bytes) {
                for bit in 0..8 {
                    let data = u16::from((byte >> bit) & 1);
                    rtc.write(SELECT | DATA_WRITE | data, 0xFFFF);
                    rtc.write(SELECT | DATA_WRITE | PIN_CLOCK | data, 0xFFFF);
                }
            }
            rtc.write(PIN_CLOCK | DATA_WRITE, 0xFFFF);
        }

        #[test]
        fn a_ds_game_reads_the_device_time() {
            let mut emulator = ds_emulator("system", 0);
            assert!(near(ds_clock(&mut emulator), device_time()));
        }

        #[test]
        fn an_own_time_sets_the_ds_clock_and_follows_changes() {
            let mut emulator = ds_emulator("own", -10 * YEAR);
            assert!(near(ds_clock(&mut emulator), device_time() - 10 * YEAR));
            emulator.set_ds_clock(3600);
            assert!(near(ds_clock(&mut emulator), device_time() + 3600));
        }

        #[test]
        fn an_own_time_runs_on_after_a_pause() {
            let mut emulator = ds_emulator("pause", -YEAR);
            let now = crate::storage::now_unix();
            let machine = &mut emulator.seat.core_mut().machine;
            emulator.gimmicks.follow_clock(machine, now + 600);
            let expected = clock::local_unix(now + 600).unwrap() - YEAR;
            assert!(near(ds_clock(&mut emulator), expected));
        }

        #[test]
        fn a_time_the_game_sets_becomes_the_own_time() {
            let mut emulator = ds_emulator("game", 0);
            assert_eq!(emulator.take_ds_clock_set(), None);
            game_sets_time(nds(&mut emulator), [0x08, 0x30, 0x00]);
            let set = ds_clock(&mut emulator);
            let offset = emulator
                .take_ds_clock_set()
                .expect("the game set the clock");
            assert!(near(offset, set - device_time()));
            assert_eq!(emulator.take_ds_clock_set(), None);
            let now = crate::storage::now_unix();
            let machine = &mut emulator.seat.core_mut().machine;
            emulator.gimmicks.follow_clock(machine, now + 600);
            assert!(near(ds_clock(&mut emulator), set + 600));
        }

        #[cfg(threaded)]
        #[test]
        fn a_time_the_game_set_reaches_the_app_while_the_console_runs_the_next_frame() {
            let threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
            let mut emulator = ds_emulator("lent", 0);
            emulator.run_on(&threads);
            game_sets_time(nds(&mut emulator), [0x08, 0x30, 0x00]);
            let set = ds_clock(&mut emulator);
            emulator.catch_ds_clock_set();
            let plan = crate::emulator::runner::Plan {
                frames: 1,
                held: emulator.held.held(),
                owed: emulator.held.take_owed(),
                effects: emulator.effects(),
                sound: emulator.sound,
                fast_forward: false,
                take: false,
                rewind: crate::emulator::rewind::RewindPlan::OFF,
                budget: None,
                defer_3d: false,
                pace: 1.0,
            };
            emulator.seat.lend(plan);
            let offset = emulator
                .take_ds_clock_set()
                .expect("the game set the clock");
            assert!(!emulator.seat.is_home());
            assert!(near(offset, set - device_time()));
            assert_eq!(emulator.take_ds_clock_set(), None);
            assert!(emulator.seat.wait(std::time::Duration::from_secs(5)));
        }

        #[test]
        fn a_loaded_state_keeps_its_clock_and_sets_nothing() {
            let mut emulator = ds_emulator("state", -YEAR);
            game_sets_time(nds(&mut emulator), [0x08, 0x30, 0x00]);
            let set = ds_clock(&mut emulator);
            let state = nds(&mut emulator).encode_state().unwrap();
            let mut loaded = nds(&mut emulator).decode_state(&state).unwrap();
            assert_eq!(loaded.buses().rtc().unix(), set);
            assert_eq!(loaded.take_clock_set(), None);
            let again = loaded.encode_state().unwrap();
            assert_eq!(again, state);
        }
    }
}
