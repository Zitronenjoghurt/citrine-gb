use crate::app::widgets::debugger::TimeControlState;
use crate::app::widgets::info::InfoState;
use crate::app::widgets::registers::RegistersState;
use crate::app::widgets::snapshots::SnapshotsState;
use crate::utils::avg_timer::AvgTimer;
use recent::RecentRoms;
use settings::Settings;

pub mod hotkeys;
pub mod keybinds;
pub mod palette;
pub mod recent;
#[cfg(not(target_arch = "wasm32"))]
pub mod rom_folder;
pub mod settings;
pub mod synced;
pub mod touch;
pub mod ui_theme;

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UiState {
    pub e2e: E2E,
    pub info: InfoState,
    pub recent: RecentRoms,
    #[cfg(not(target_arch = "wasm32"))]
    pub rom_folder: rom_folder::RomFolder,
    pub registers: RegistersState,
    #[serde(skip_serializing)]
    pub settings: Settings,
    #[serde(skip, default)]
    pub snapshots: SnapshotsState,
    #[serde(skip, default)]
    pub media: crate::app::tabs::media::MediaTabState,
    #[serde(skip, default)]
    pub stats: crate::app::tabs::stats::StatsUi,
    pub time_ctrl: TimeControlState,
    #[serde(skip, default)]
    pub update_avg_timer: AvgTimer,
    #[serde(skip, default)]
    pub capture: keybinds::BindingCapture,
    #[serde(skip, default)]
    pub update: crate::update::Updater,
    #[serde(skip, default)]
    pub palette_editor: crate::app::widgets::settings::palette::PaletteEditorState,
    #[serde(skip, default)]
    pub audio_debug: crate::app::widgets::audio_debug::AudioDebugState,
    #[serde(skip, default)]
    pub controller_debug: crate::controller_debug::ControllerDebugState,
    #[serde(skip, default)]
    pub toast_area: Option<egui::Rect>,
    #[serde(skip, default)]
    pub toast_picture: Option<egui::Rect>,
    #[serde(skip, default)]
    pub tab_fit: crate::app::tabs::TabFit,
    #[serde(skip, default)]
    pub join: crate::app::widgets::settings::online::JoinForm,
    #[serde(skip, default)]
    pub friends: crate::app::tabs::friends::FriendsUi,
    #[serde(skip, default)]
    pub playing: crate::app::tabs::playing::PlayingUi,
    #[serde(skip, default)]
    pub linked: crate::app::tabs::linked::LinkUi,
    #[serde(skip, default)]
    pub your_data: crate::app::widgets::settings::your_data::YourDataUi,
    #[serde(skip, default)]
    pub viewer: crate::app::viewer::ViewerState,
    #[serde(skip, default)]
    pub memory: crate::app::widgets::memory::MemoryViewState,
    #[serde(skip, default)]
    pub video: crate::app::widgets::video::VideoViewState,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct E2E {
    pub title: String,
    pub description: String,
}
