use super::camera::CameraView;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;

pub const MAX_CLOCK_OFFSET: i64 = 30 * 86_400;
pub const DEFAULT_LIGHT: f32 = 0.5;
pub const RECENTER: &str = "Recenter";
pub const PRINTER_HINT: &str = "Prints show up in Media.";
pub const PRINTER_LINKED: &str = "Off while a link cable is in use.";
pub const CLOCK_HINT: &str = "Hours the game's clock runs ahead of this device, or behind it.";
pub const CLOCK_FORWARD_HINT: &str =
    "Hours the game's clock runs ahead of this device. This cartridge can't set it back.";

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    strum_macros::EnumIter,
)]
pub enum CameraSource {
    #[default]
    Camera,
    Picture,
}

impl Display for CameraSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Camera => "Live camera",
            Self::Picture => "Picture",
        })
    }
}

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    strum_macros::EnumIter,
)]
pub enum LightSource {
    #[default]
    Sensor,
    Slider,
}

impl Display for LightSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Sensor => "Light sensor",
            Self::Slider => "Set by hand",
        })
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GimmickSettings {
    pub rumble: bool,
    pub light_source: LightSource,
    pub light: f32,
    pub camera: CameraSource,
    pub camera_view: CameraView,
    pub clock_offsets: BTreeMap<String, i64>,
    pub printers: BTreeSet<String>,
}

impl Default for GimmickSettings {
    fn default() -> Self {
        Self {
            rumble: true,
            light_source: LightSource::default(),
            light: DEFAULT_LIGHT,
            camera: CameraSource::default(),
            camera_view: CameraView::default(),
            clock_offsets: BTreeMap::new(),
            printers: BTreeSet::new(),
        }
    }
}

impl GimmickSettings {
    pub fn clock_offset(&self, rom_key: &str) -> i64 {
        self.clock_offsets.get(rom_key).copied().unwrap_or(0)
    }

    pub fn set_clock_offset(&mut self, rom_key: &str, seconds: i64) {
        let seconds = seconds.clamp(-MAX_CLOCK_OFFSET, MAX_CLOCK_OFFSET);
        if seconds == 0 {
            self.clock_offsets.remove(rom_key);
        } else {
            self.clock_offsets.insert(rom_key.to_owned(), seconds);
        }
    }

    pub fn printer(&self, rom_key: &str) -> bool {
        self.printers.contains(rom_key)
    }

    pub fn set_printer(&mut self, rom_key: &str, on: bool) {
        if on {
            self.printers.insert(rom_key.to_owned());
        } else {
            self.printers.remove(rom_key);
        }
    }

    pub fn light_level(&self, sensed: Option<f32>) -> f32 {
        match (self.light_source, sensed) {
            (LightSource::Sensor, Some(level)) => level.clamp(0.0, 1.0),
            _ => self.light.clamp(0.0, 1.0),
        }
    }
}

#[cfg(any(target_os = "android", test))]
pub fn lux_to_light(lux: f32) -> f32 {
    if !lux.is_finite() || lux <= 1.0 {
        return 0.0;
    }
    (lux.log10() / 5.0).clamp(0.0, 1.0)
}
