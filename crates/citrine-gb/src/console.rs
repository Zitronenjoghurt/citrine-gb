#[cfg(feature = "persistence")]
use crate::error::GbResult;
use crate::gb::joypad::JoypadState;
use crate::gb::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
#[cfg(feature = "persistence")]
use crate::persistence::battery_save::BatterySave;
#[cfg(feature = "persistence")]
use crate::persistence::state::StateBody;
use crate::rom::header::RomHeader;
#[cfg(feature = "rom-id")]
use crate::rom::id::RomId;
use bitflags::bitflags;
use std::path::Path;

const GB_CLOCK_HZ: u32 = 4_194_304;
const GB_DOTS_PER_FRAME: u32 = 70_224;
const GBA_WIDTH: usize = 240;
const GBA_HEIGHT: usize = 160;
const GBA_CLOCK_HZ: u32 = 16_777_216;
const GBA_CYCLES_PER_FRAME: u32 = 280_896;

bitflags! {
    #[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct Keys: u16 {
        const A = 1 << 0;
        const B = 1 << 1;
        const SELECT = 1 << 2;
        const START = 1 << 3;
        const RIGHT = 1 << 4;
        const LEFT = 1 << 5;
        const UP = 1 << 6;
        const DOWN = 1 << 7;
        const R = 1 << 8;
        const L = 1 << 9;
    }
}

impl From<JoypadState> for Keys {
    fn from(state: JoypadState) -> Self {
        Self::from_bits_retain(u16::from(state.bits()))
    }
}

impl From<Keys> for JoypadState {
    fn from(keys: Keys) -> Self {
        let [low, _] = keys.bits().to_le_bytes();
        Self::from_bits_retain(low)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ConsoleKind {
    Gb,
    Gbc,
    Gba,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoSpec {
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub clock_hz: u32,
    pub cycles_per_frame: u32,
}

impl VideoSpec {
    pub fn aspect(self) -> f32 {
        self.height as f32 / self.width as f32
    }
}

impl Timing {
    pub fn frame_secs(self) -> f64 {
        f64::from(self.cycles_per_frame) / f64::from(self.clock_hz)
    }
}

impl ConsoleKind {
    pub fn video(self) -> VideoSpec {
        match self {
            ConsoleKind::Gb | ConsoleKind::Gbc => VideoSpec {
                width: SCREEN_WIDTH,
                height: SCREEN_HEIGHT,
            },
            ConsoleKind::Gba => VideoSpec {
                width: GBA_WIDTH,
                height: GBA_HEIGHT,
            },
        }
    }

    pub fn timing(self) -> Timing {
        match self {
            ConsoleKind::Gb | ConsoleKind::Gbc => Timing {
                clock_hz: GB_CLOCK_HZ,
                cycles_per_frame: GB_DOTS_PER_FRAME,
            },
            ConsoleKind::Gba => Timing {
                clock_hz: GBA_CLOCK_HZ,
                cycles_per_frame: GBA_CYCLES_PER_FRAME,
            },
        }
    }
}

pub trait Console {
    fn kind(&self) -> ConsoleKind;
    fn video(&self) -> VideoSpec;
    fn timing(&self) -> Timing;
    fn run_frame(&mut self);
    fn frame(&self) -> &[u8];
    fn take_audio(&mut self, out: &mut Vec<f32>);
    fn sample_rate(&self) -> u32;
    fn set_sample_rate(&mut self, rate: u32);
    fn keys(&self) -> Keys;
    fn set_keys(&mut self, keys: Keys);
    fn soft_reset(&mut self);
    fn audio_channels(&self) -> &'static [&'static str];
    fn set_channel_enabled(&mut self, index: usize, enabled: bool);
    fn rom_bytes(&self) -> Vec<u8>;
    fn title(&self) -> String;
    fn has_battery(&self) -> bool;
    fn has_clock(&self) -> bool;
    #[cfg(feature = "persistence")]
    fn battery(&mut self, force: bool, now: u64) -> Option<BatterySave>;
    #[cfg(feature = "persistence")]
    fn battery_from_file(&self, data: &[u8]) -> BatterySave;
    #[cfg(feature = "persistence")]
    fn put_battery(&mut self, save: BatterySave, now: u64);
    #[cfg(feature = "persistence")]
    fn save_state(&mut self, now: u64) -> GbResult<Vec<u8>>;
    #[cfg(feature = "persistence")]
    fn state_body(&mut self, now: u64) -> GbResult<StateBody>;
    #[cfg(feature = "rom-id")]
    fn rom_id(&self) -> RomId;
}

#[cfg(feature = "persistence")]
pub trait Rollback: Console + Sized {
    fn clone_state(&self) -> Self;
    fn copy_state_from(&mut self, other: &Self);
    fn state_hash(&self, scratch: &mut Self) -> u64;
    fn encode_state(&self) -> GbResult<Vec<u8>>;
    fn decode_state(&self, data: &[u8]) -> GbResult<Self>;
    fn set_blind(&mut self, blind: bool);
    fn set_muted(&mut self, muted: bool);
}

pub fn detect(data: &[u8], file_name: Option<&str>) -> Option<ConsoleKind> {
    let extension = file_name
        .and_then(|name| Path::new(name).extension())
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("gba") => Some(ConsoleKind::Gba),
        Some("gb" | "gbc") => Some(game_boy_kind(data)),
        _ => detect_content(data),
    }
}

fn detect_content(data: &[u8]) -> Option<ConsoleKind> {
    if is_gba(data) {
        Some(ConsoleKind::Gba)
    } else if RomHeader::parse_valid_nintendo_logo(data).unwrap_or(false) {
        Some(game_boy_kind(data))
    } else {
        None
    }
}

fn is_gba(data: &[u8]) -> bool {
    data.get(3) == Some(&0xEA) && data.get(0xB2) == Some(&0x96)
}

fn game_boy_kind(data: &[u8]) -> ConsoleKind {
    match RomHeader::parse_cgb_mode(data) {
        Ok(mode) if mode.is_cgb() => ConsoleKind::Gbc,
        _ => ConsoleKind::Gb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gb::GbModel;
    use crate::rom::header::NINTENDO_LOGO;

    fn gba_header() -> Vec<u8> {
        let mut data = vec![0; 0x200];
        data[0..4].copy_from_slice(&[0x2E, 0x00, 0x00, 0xEA]);
        data[0xA0..0xAC].copy_from_slice(b"CITRINE TEST");
        data[0xB2] = 0x96;
        data
    }

    fn gb_header(cgb_flag: u8) -> Vec<u8> {
        let mut data = vec![0; 0x8000];
        data[0x104..0x134].copy_from_slice(&NINTENDO_LOGO);
        data[0x143] = cgb_flag;
        data
    }

    #[test]
    fn a_gba_header_without_a_name_is_a_gba_game() {
        assert_eq!(detect(&gba_header(), None), Some(ConsoleKind::Gba));
    }

    #[test]
    fn a_gba_header_with_an_unknown_extension_is_a_gba_game() {
        assert_eq!(
            detect(&gba_header(), Some("game.bin")),
            Some(ConsoleKind::Gba)
        );
    }

    #[test]
    fn the_gba_extension_decides_before_the_content() {
        assert_eq!(
            detect(&gb_header(0x00), Some("Game.GBA")),
            Some(ConsoleKind::Gba)
        );
    }

    #[test]
    fn a_game_boy_extension_decides_before_the_content() {
        assert_eq!(
            detect(&gba_header(), Some("game.gb")),
            Some(ConsoleKind::Gb)
        );
    }

    #[test]
    fn a_game_boy_header_reads_the_color_flag() {
        assert_eq!(detect(&gb_header(0x00), None), Some(ConsoleKind::Gb));
        assert_eq!(detect(&gb_header(0x80), None), Some(ConsoleKind::Gbc));
        assert_eq!(
            detect(&gb_header(0xC0), Some("x.gb")),
            Some(ConsoleKind::Gbc)
        );
    }

    #[test]
    fn a_game_boy_screen_is_160_by_144_at_70224_cycles_a_frame() {
        for kind in [ConsoleKind::Gb, ConsoleKind::Gbc] {
            assert_eq!(
                kind.video(),
                VideoSpec {
                    width: 160,
                    height: 144
                }
            );
            assert_eq!(
                kind.timing(),
                Timing {
                    clock_hz: 4_194_304,
                    cycles_per_frame: 70_224
                }
            );
        }
    }

    #[test]
    fn a_gba_screen_is_240_by_160_at_280896_cycles_a_frame() {
        assert_eq!(
            ConsoleKind::Gba.video(),
            VideoSpec {
                width: 240,
                height: 160
            }
        );
        assert_eq!(
            ConsoleKind::Gba.timing(),
            Timing {
                clock_hz: 16_777_216,
                cycles_per_frame: 280_896
            }
        );
    }

    #[test]
    fn each_game_boy_model_is_its_console() {
        assert_eq!(ConsoleKind::from(GbModel::Dmg), ConsoleKind::Gb);
        assert_eq!(ConsoleKind::from(GbModel::Cgb), ConsoleKind::Gbc);
    }

    #[test]
    fn data_without_a_known_header_is_no_console() {
        assert_eq!(detect(&[0xFF; 0x200], None), None);
        assert_eq!(detect(&[], Some("notes.txt")), None);
    }
}
