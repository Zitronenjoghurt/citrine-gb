use crate::app::tabs::Tab;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab};
use crate::media::MediaId;
use citrine_gb::gb::GbModel;

#[derive(Debug, PartialEq)]
pub enum GamesAction {
    Open,
    Files,
    Resume,
    Search,
    Homebrew,
    SecondScreens,
    #[cfg(feature = "nds")]
    Walker,
    #[cfg(feature = "nds")]
    Gts,
    Recent(std::path::PathBuf),
    Remove(std::path::PathBuf),
    AddFolder,
    RemoveFolder(String),
    Play(String),
    RemoveGame(String),
    CancelDownload(String),
    DeleteDownload(String),
    RemoveMissing,
    Undo,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    ShowInFolder(String),
    LibraryFilter,
    Folders,
    GameMenu(String),
    GameInfo(String),
    CloseInfo,
    AddTag(String, String),
    RemoveTag(String, String),
}

pub enum AppEvent {
    Record(crate::app::widgets::record_bar::RecordBarAction),
    Replay(crate::app::widgets::replay_bar::ReplayBarAction),
    Clip(crate::app::widgets::clip_panel::ClipAction),
    RemoveHud {
        id: u32,
    },
    RestoreHud {
        index: usize,
        hud: crate::app::hud::Hud,
    },
    Games(GamesAction),
    LoadRomData {
        data: Vec<u8>,
    },
    PlayShelfGame {
        game: &'static crate::homebrew::HomebrewGame,
    },
    PrefetchShelfGame {
        game: &'static crate::homebrew::HomebrewGame,
    },
    OpenTab {
        tab: Tab,
    },
    Notify {
        message: String,
        error: bool,
    },
    MediaSaved {
        id: MediaId,
    },
    MediaAdded {
        id: MediaId,
    },
    ShowMedia {
        id: MediaId,
    },
    PlayOn {
        model: GbModel,
    },
    PickSecondGame,
    LinkGame {
        data: std::sync::Arc<[u8]>,
        name: Option<String>,
    },
    EndLink,
    StartPartner {
        event: &'static citrine_gb_partner::Event,
    },
    OpenSettings {
        tab: SettingsTab,
    },
    FindSetting {
        row: crate::app::ui_state::settings_tree::rows::RowId,
    },
    UndoSnapshot {
        rom_key: String,
        slot: usize,
    },
    KeepPeerSaves(crate::emulator::peer_saves::PeerNotice),
    AskDiscardPeerSaves(crate::emulator::peer_saves::PeerNotice),
    SetPeerSavesAside(crate::emulator::peer_saves::PeerNotice),
    UndoKeptSaves(Box<crate::emulator::peer_saves::KeptSaves>),
    PickCameraPicture,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    PickCaptureFolder,
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
    DownloadCodec,
    CentreTilt,
    HardcoreOff,
    SaveGameReport,
    Screens(crate::app::ui_state::ds_screens::ScreensAction),
    EditLayout(crate::app::layout_edit::EditStart),
    ToggleLid,
    ScreenSettings {
        phone: bool,
    },
    FindGroup {
        group: GroupId,
    },
    PickFile(crate::app::system_files::SystemFile),
    RemoveFile(crate::app::system_files::SystemFile),
    #[cfg(feature = "nds")]
    FindDsBios,
    #[cfg(feature = "nds")]
    FindDsDirect,
}

#[derive(Default)]
pub struct AppEventQueue {
    queue: Vec<AppEvent>,
}

impl AppEventQueue {
    pub fn take(&mut self) -> Vec<AppEvent> {
        std::mem::take(&mut self.queue)
    }

    pub fn push(&mut self, event: AppEvent) {
        self.queue.push(event);
    }

    pub fn games(&mut self, action: GamesAction) {
        self.queue.push(AppEvent::Games(action));
    }

    pub fn load_rom_data(&mut self, data: Vec<u8>) {
        self.queue.push(AppEvent::LoadRomData { data });
    }

    pub fn play_shelf_game(&mut self, game: &'static crate::homebrew::HomebrewGame) {
        self.queue.push(AppEvent::PlayShelfGame { game });
    }

    pub fn prefetch_shelf_game(&mut self, game: &'static crate::homebrew::HomebrewGame) {
        self.queue.push(AppEvent::PrefetchShelfGame { game });
    }

    pub fn pick_system_file(&mut self, file: crate::app::system_files::SystemFile) {
        self.queue.push(AppEvent::PickFile(file));
    }

    pub fn remove_system_file(&mut self, file: crate::app::system_files::SystemFile) {
        self.queue.push(AppEvent::RemoveFile(file));
    }

    pub fn play_on(&mut self, model: GbModel) {
        self.queue.push(AppEvent::PlayOn { model });
    }

    pub fn pick_second_game(&mut self) {
        self.queue.push(AppEvent::PickSecondGame);
    }

    pub fn link_game(&mut self, data: std::sync::Arc<[u8]>, name: Option<String>) {
        self.queue.push(AppEvent::LinkGame { data, name });
    }

    pub fn start_partner(&mut self, event: &'static citrine_gb_partner::Event) {
        self.queue.push(AppEvent::StartPartner { event });
    }

    pub fn end_link(&mut self) {
        self.queue.push(AppEvent::EndLink);
    }

    pub fn open_settings(&mut self, tab: SettingsTab) {
        self.queue.push(AppEvent::OpenSettings { tab });
    }

    pub fn pick_camera_picture(&mut self) {
        self.queue.push(AppEvent::PickCameraPicture);
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    pub fn pick_capture_folder(&mut self) {
        self.queue.push(AppEvent::PickCaptureFolder);
    }

    #[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
    pub fn download_codec(&mut self) {
        self.queue.push(AppEvent::DownloadCodec);
    }

    pub fn screens(&mut self, action: crate::app::ui_state::ds_screens::ScreensAction) {
        self.queue.push(AppEvent::Screens(action));
    }

    pub fn remove_hud(&mut self, id: u32) {
        self.queue.push(AppEvent::RemoveHud { id });
    }

    pub fn edit_layout(&mut self, start: crate::app::layout_edit::EditStart) {
        self.queue.push(AppEvent::EditLayout(start));
    }

    pub fn toggle_lid(&mut self) {
        self.queue.push(AppEvent::ToggleLid);
    }

    pub fn screen_settings(&mut self, phone: bool) {
        self.queue.push(AppEvent::ScreenSettings { phone });
    }

    pub fn find_group(&mut self, group: GroupId) {
        self.queue.push(AppEvent::FindGroup { group });
    }

    pub fn centre_tilt(&mut self) {
        self.queue.push(AppEvent::CentreTilt);
    }

    pub fn hardcore_off(&mut self) {
        self.queue.push(AppEvent::HardcoreOff);
    }

    pub fn save_game_report(&mut self) {
        self.queue.push(AppEvent::SaveGameReport);
    }

    pub fn open_tab(&mut self, tab: Tab) {
        self.queue.push(AppEvent::OpenTab { tab });
    }

    pub fn notify(&mut self, message: impl Into<String>) {
        self.queue.push(AppEvent::Notify {
            message: message.into(),
            error: false,
        });
    }

    pub fn notify_error(&mut self, message: impl Into<String>) {
        self.queue.push(AppEvent::Notify {
            message: message.into(),
            error: true,
        });
    }
}
