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
        }
    }

    pub(crate) fn collect(&mut self, machine: &mut Machine) {
        match machine {
            Machine::Gb(gb) => {
                self.rumble = if gb.has_rumble() {
                    gb.take_rumble()
                } else {
                    0.0
                };
                if gb.printer_attached() {
                    self.prints.extend(gb.take_prints());
                }
            }
            #[cfg(feature = "gba")]
            Machine::Gba(gba) => {
                self.rumble = if gba.rumble() { 1.0 } else { 0.0 };
            }
        }
    }

    pub(crate) fn stop_rumble(&mut self) {
        self.rumble = 0.0;
    }
}

impl super::Emulator {
    pub fn cart(&self) -> Cart {
        if self.rom_key.is_none() {
            return Cart::default();
        }
        Cart::of(&self.machine)
    }

    pub fn gimmick_settings(&self) -> &GimmickSettings {
        &self.gimmick_settings
    }

    pub fn printer_free(&self) -> bool {
        self.link.is_none() && self.partner.is_none()
    }

    pub fn set_gimmick_settings(&mut self, settings: &GimmickSettings) {
        if *settings == self.gimmick_settings {
            return;
        }
        self.gimmick_settings = settings.clone();
        let Some(key) = self.rom_key.clone() else {
            return;
        };
        let offset = settings.clock_offset(&key);
        self.gimmicks
            .set_clock_offset(&mut self.machine, offset, crate::storage::now_unix());
        self.attach_printer();
    }

    pub(crate) fn start_gimmicks(&mut self) {
        self.gimmicks.reset();
        let Some(key) = self.rom_key.clone() else {
            return;
        };
        let offset = self.gimmick_settings.clock_offset(&key);
        self.gimmicks
            .start_clock(&mut self.machine, offset, crate::storage::now_unix());
        self.attach_printer();
    }

    pub(crate) fn attach_printer(&mut self) {
        let wanted = self
            .rom_key
            .as_deref()
            .is_some_and(|key| self.gimmick_settings.printer(key))
            && self.printer_free()
            && self.cart().printer;
        if let Some(gb) = self.machine.as_gb_mut()
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
mod tests;
