#[cfg(feature = "nds")]
use citrine_gb::nds::spi::firmware::UserSettings;
use serde::{Deserialize, Serialize};

pub const FORMAT: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub created_unix: u64,
    pub app: AppInfo,
    pub game: GameInfo,
    pub firmware: FirmwareInfo,
    pub replay: Option<ReplayInfo>,
    pub screens: Vec<ScreenInfo>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    pub version: String,
    pub core_version: String,
    pub features: Vec<String>,
    pub target: String,
    pub os: String,
    pub profile: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameInfo {
    pub console: String,
    pub title: String,
    pub code: Option<String>,
    pub crc32: String,
    pub sha1: String,
    pub frames_played: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirmwareInfo {
    pub gb_boot_rom_sha1: Option<String>,
    #[serde(default)]
    pub gbc_boot_rom_sha1: Option<String>,
    pub gba_bios_sha1: Option<String>,
    pub nds_arm9_bios_sha1: Option<String>,
    pub nds_arm7_bios_sha1: Option<String>,
    pub nds_firmware_sha1: Option<String>,
    #[cfg(feature = "nds")]
    #[serde(default)]
    pub nds_user: Option<UserSettings>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayInfo {
    pub state_unix: u64,
    pub frames: u64,
    pub end_digest: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenInfo {
    pub file: String,
    pub seconds_before: u32,
}
