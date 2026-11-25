use crate::app::widgets::debugger::TimeControlState;
use crate::app::widgets::info::InfoState;
use crate::app::widgets::registers::RegistersState;
use crate::app::widgets::snapshots::SnapshotsState;
use crate::utils::avg_timer::AvgTimer;
use recent::RecentRoms;
use settings::Settings;

pub mod ds_3d;
pub mod ds_online;
pub mod ds_screens;
pub mod ds_user;
pub mod games;
pub mod hotkeys;
pub mod keybinds;
pub mod layout_picks;
pub mod library;
pub mod palette;
pub mod per_game;
pub mod recent;
pub mod screen_layouts;
pub mod settings;
pub mod settings_tree;
pub mod synced;
pub mod touch;
pub mod ui_theme;
pub mod walker_steps;

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UiState {
    pub e2e: E2E,
    pub games: games::GamesPane,
    pub info: InfoState,
    pub library: library::Library,
    pub recent: RecentRoms,
    #[serde(skip_serializing)]
    pub rom_folder: library::OldFolder,
    pub registers: RegistersState,
    #[serde(skip_serializing)]
    pub settings: Settings,
    #[serde(skip, default)]
    pub settings_list_open: bool,
    #[serde(skip, default)]
    pub snapshots: SnapshotsState,
    #[serde(skip, default)]
    pub second: crate::app::second_screens::PaneState,
    #[serde(skip, default)]
    pub media: crate::app::tabs::media::MediaTabState,
    #[serde(skip, default)]
    pub cheats: crate::app::tabs::cheats::CheatsPane,
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
    pub focus_mode: bool,
    #[serde(skip, default)]
    pub threads_debug: crate::app::widgets::threads_debug::ThreadsDebugState,
    #[serde(skip, default)]
    pub performance: crate::app::widgets::performance::PerformanceState,
    #[serde(skip, default)]
    pub frame_log: crate::frame_log::FrameLog,
    #[serde(skip, default)]
    pub hud: crate::app::hud::HudState,
    #[serde(skip, default)]
    pub record_watch: crate::app::recording::RecordWatch,
    #[serde(skip, default)]
    pub toast_area: Option<egui::Rect>,
    #[serde(skip, default)]
    pub toast_spare: Option<egui::Rect>,
    #[serde(skip, default)]
    pub layout_edit: Option<crate::app::layout_edit::LayoutEdit>,
    #[serde(skip, default)]
    pub(crate) layout_applied: Option<crate::app::layout_picks::LayoutApplied>,
    #[serde(skip, default)]
    pub(crate) layout_sheet: Option<crate::app::layout_sheet::LayoutSheet>,
    #[serde(skip, default)]
    pub(crate) layout_turn: Option<crate::app::layout_sheet::LayoutTurn>,
    #[serde(skip, default)]
    pub(crate) orientation_held: bool,
    #[serde(skip, default)]
    pub screen_marks: Vec<(crate::app::ui_state::screen_layouts::Item, egui::Vec2)>,
    #[cfg(test)]
    #[serde(skip, default)]
    pub touch_bounds: Option<egui::Rect>,
    #[cfg(test)]
    #[serde(skip, default)]
    pub replay_room: Option<(f32, egui::Rect)>,
    #[serde(skip, default)]
    pub clip: Option<crate::app::clip::ClipView>,
    #[cfg(all(test, feature = "nds"))]
    #[serde(skip, default)]
    pub touch_targets: Vec<egui::Rect>,
    #[cfg(feature = "nds")]
    #[serde(skip, default)]
    pub walker: crate::app::tabs::walker::WalkerPane,
    #[cfg(feature = "nds")]
    #[serde(skip, default)]
    pub gts: crate::app::tabs::gts::GtsPane,
    #[serde(skip, default)]
    pub toast_picture: Option<egui::Rect>,
    #[serde(skip, default)]
    pub toast_banner: Option<egui::Rect>,
    #[serde(skip, default)]
    pub toolbar_reach: f32,
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
    pub storage: crate::app::widgets::settings::storage::StorageUi,
    #[serde(skip, default)]
    pub viewer: crate::app::viewer::ViewerState,
    #[serde(skip, default)]
    pub memory: crate::app::widgets::memory::MemoryViewState,
    #[serde(skip, default)]
    pub video: crate::app::widgets::video::VideoViewState,
    #[serde(skip, default)]
    pub ds_files: crate::app::tabs::ds_files::DsFilesUi,
}

impl UiState {
    pub fn seed_library(&mut self) {
        let tree = self.rom_folder.tree.take();
        let opened: Vec<_> = self
            .recent
            .entries()
            .iter()
            .map(|entry| {
                let opened = library::Opened {
                    path: entry.path.to_string_lossy().into_owned(),
                    name: entry.display_name(),
                    console: entry.console,
                    rom_key: self.recent.game_key(&entry.path).map(str::to_owned),
                    sha1: None,
                    crc32: None,
                };
                (opened, entry.last_played)
            })
            .collect();
        self.library.seed(tree.as_deref(), opened);
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct E2E {
    pub title: String,
    pub description: String,
}
