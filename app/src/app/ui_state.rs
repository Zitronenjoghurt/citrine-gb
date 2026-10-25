use crate::app::widgets::info::InfoState;
use crate::app::widgets::registers::RegistersState;
use crate::app::widgets::snapshots::SnapshotsState;
use crate::app::widgets::time_control::TimeControlState;
use crate::utils::avg_timer::AvgTimer;
use recent::RecentRoms;
use settings::Settings;

pub mod hotkeys;
pub mod keybinds;
pub mod palette;
pub mod recent;
pub mod settings;
pub mod touch;
pub mod ui_theme;

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UiState {
    pub e2e: E2E,
    pub info: InfoState,
    pub recent: RecentRoms,
    pub registers: RegistersState,
    #[serde(skip_serializing)]
    pub settings: Settings,
    #[serde(skip, default)]
    pub snapshots: SnapshotsState,
    pub time_ctrl: TimeControlState,
    #[serde(skip, default)]
    pub update_avg_timer: AvgTimer,
    #[serde(skip, default)]
    pub capture: keybinds::BindingCapture,
    #[serde(skip, default)]
    pub palette_editor: crate::app::widgets::settings::palette::PaletteEditorState,
    #[serde(skip, default)]
    pub audio_debug: crate::app::widgets::audio_debug::AudioDebugState,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct E2E {
    pub title: String,
    pub description: String,
}
