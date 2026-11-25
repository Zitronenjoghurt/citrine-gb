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
use crate::rom::nds::NdsHeader;
use bitflags::bitflags;
use std::path::Path;
use std::sync::Arc;

const GB_CLOCK_HZ: u32 = 4_194_304;
const GB_DOTS_PER_FRAME: u32 = 70_224;
const GBA_WIDTH: usize = 240;
const GBA_HEIGHT: usize = 160;
const GBA_CLOCK_HZ: u32 = 16_777_216;
const GBA_CYCLES_PER_FRAME: u32 = 280_896;
const NDS_WIDTH: usize = 256;
const NDS_HEIGHT: usize = 192;
const NDS_CLOCK_HZ: u32 = 33_513_982;
const NDS_CYCLES_PER_FRAME: u32 = 560_190;

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
        const X = 1 << 10;
        const Y = 1 << 11;
        const LID = 1 << 12;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Touch {
    pub x: u8,
    pub y: u8,
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
    Nds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub touch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoSpec {
    pub width: usize,
    pub height: usize,
    pub screens: &'static [Screen],
}

const GB_SCREENS: [Screen; 1] = [Screen::whole(SCREEN_WIDTH, SCREEN_HEIGHT)];
const GBA_SCREENS: [Screen; 1] = [Screen::whole(GBA_WIDTH, GBA_HEIGHT)];
const NDS_SCREENS: [Screen; 2] = [
    Screen::whole(NDS_WIDTH, NDS_HEIGHT),
    Screen {
        x: 0,
        y: NDS_HEIGHT,
        width: NDS_WIDTH,
        height: NDS_HEIGHT,
        touch: true,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub clock_hz: u32,
    pub cycles_per_frame: u32,
}

impl Screen {
    pub const fn whole(width: usize, height: usize) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
            touch: false,
        }
    }
}

impl VideoSpec {
    pub const fn of(screens: &'static [Screen]) -> Self {
        let (mut width, mut height, mut at) = (0, 0, 0);
        while at < screens.len() {
            let screen = screens[at];
            if screen.x + screen.width > width {
                width = screen.x + screen.width;
            }
            if screen.y + screen.height > height {
                height = screen.y + screen.height;
            }
            at += 1;
        }
        Self {
            width,
            height,
            screens,
        }
    }

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
    pub const ALL: [ConsoleKind; 4] = [
        ConsoleKind::Gb,
        ConsoleKind::Gbc,
        ConsoleKind::Gba,
        ConsoleKind::Nds,
    ];

    pub fn video(self) -> VideoSpec {
        match self {
            ConsoleKind::Gb | ConsoleKind::Gbc => VideoSpec::of(&GB_SCREENS),
            ConsoleKind::Gba => VideoSpec::of(&GBA_SCREENS),
            ConsoleKind::Nds => VideoSpec::of(&NDS_SCREENS),
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
            ConsoleKind::Nds => Timing {
                clock_hz: NDS_CLOCK_HZ,
                cycles_per_frame: NDS_CYCLES_PER_FRAME,
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
    fn set_touch(&mut self, _touch: Option<Touch>) {}
    fn set_mic(&mut self, _samples: &[i16]) {}
    fn mic_in_use(&self) -> bool {
        false
    }
    fn soft_reset(&mut self);
    fn audio_channels(&self) -> &'static [&'static str];
    fn set_channel_enabled(&mut self, index: usize, enabled: bool);
    fn rom_bytes(&self) -> Arc<[u8]>;
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
    #[cfg(feature = "rom-id")]
    fn game_sha1(&self) -> [u8; 20] {
        self.rom_id().sha1
    }
}

#[cfg(feature = "persistence")]
pub trait Rollback: Console + Sized + Send {
    fn clone_state(&self) -> Self;
    fn copy_state_from(&mut self, other: &Self);
    fn snapshot_into(&mut self, slot: &mut Self) {
        slot.copy_state_from(self);
    }
    fn snapshot(&mut self) -> Self {
        let mut slot = self.clone_state();
        self.snapshot_into(&mut slot);
        slot
    }
    fn scratch(&self) -> Self {
        self.clone_state()
    }
    fn state_hash(&self, scratch: &mut Self) -> u64;
    fn encode_state(&self) -> GbResult<Vec<u8>>;
    fn encode_state_into(&self, out: &mut Vec<u8>) -> GbResult<()> {
        out.extend_from_slice(&self.encode_state()?);
        Ok(())
    }
    fn decode_state(&self, data: &[u8]) -> GbResult<Self>;
    fn set_blind(&mut self, blind: bool);
    fn set_muted(&mut self, muted: bool);
}

pub fn detect(data: &[u8], file_name: Option<&str>) -> Option<ConsoleKind> {
    let extension = file_name
        .and_then(|name| Path::new(name).extension())
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("nds") => Some(ConsoleKind::Nds),
        Some("gba") => Some(ConsoleKind::Gba),
        Some("gb" | "gbc") => Some(game_boy_kind(data)),
        _ => detect_content(data),
    }
}

fn detect_content(data: &[u8]) -> Option<ConsoleKind> {
    if NdsHeader::is_nds(data) {
        Some(ConsoleKind::Nds)
    } else if is_gba(data) {
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
    fn a_ds_header_is_a_ds_game_before_the_gba_check() {
        let mut data = crate::rom::nds::tests::nds_rom(&[]);
        assert_eq!(detect(&data, None), Some(ConsoleKind::Nds));
        assert_eq!(detect(&data, Some("game.bin")), Some(ConsoleKind::Nds));
        data[0x15E] ^= 1;
        assert_eq!(detect(&data, None), None);
        assert_eq!(detect(&data, Some("Game.NDS")), Some(ConsoleKind::Nds));
    }

    #[test]
    fn a_ds_frame_holds_both_screens() {
        let video = ConsoleKind::Nds.video();
        assert_eq!((video.width, video.height), (256, 384));
        assert!(video.screens[1].touch);
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
            let video = kind.video();
            assert_eq!((video.width, video.height), (160, 144));
            assert_eq!(
                video.screens,
                [Screen {
                    x: 0,
                    y: 0,
                    width: 160,
                    height: 144,
                    touch: false
                }]
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
        let video = ConsoleKind::Gba.video();
        assert_eq!((video.width, video.height), (240, 160));
        assert_eq!(video.screens, [Screen::whole(240, 160)]);
        assert_eq!(
            ConsoleKind::Gba.timing(),
            Timing {
                clock_hz: 16_777_216,
                cycles_per_frame: 280_896
            }
        );
    }

    #[test]
    fn a_frame_spans_every_screen_it_holds() {
        const TWO: [Screen; 2] = [
            Screen::whole(256, 192),
            Screen {
                x: 0,
                y: 192,
                width: 256,
                height: 192,
                touch: true,
            },
        ];
        let video = VideoSpec::of(&TWO);
        assert_eq!((video.width, video.height), (256, 384));
        assert_eq!(VideoSpec::of(&[]).width, 0);
    }

    #[test]
    fn the_ds_keys_press_no_game_boy_button() {
        let ds = Keys::X | Keys::Y | Keys::LID;
        assert_eq!(JoypadState::from(ds), JoypadState::empty());
        assert_eq!(JoypadState::from(ds | Keys::A), JoypadState::A);
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
