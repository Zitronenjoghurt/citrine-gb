use crate::app::tabs::Tab;
use crate::app::ui_state::settings::SettingsTab;
use crate::media::MediaId;
use citrine_gb::gb::GbModel;

#[derive(Debug, PartialEq)]
pub enum GamesAction {
    Open,
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
    UndoSnapshot {
        rom_key: String,
        slot: usize,
    },
    PickCameraPicture,
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
    DownloadCodec,
    CentreTilt,
    SaveGameReport,
    Screens(crate::app::ui_state::ds_screens::ScreensAction),
    ScreenSettings {
        phone: bool,
    },
    #[cfg(feature = "gba")]
    PickGbaBios,
    #[cfg(feature = "gba")]
    UseBuiltInBios,
    #[cfg(feature = "nds")]
    PickDsFile(crate::app::ds_firmware::DsFile),
    #[cfg(feature = "nds")]
    RemoveDsFile(crate::app::ds_firmware::DsFile),
    #[cfg(feature = "nds")]
    FindDsBios,
    #[cfg(feature = "nds")]
    FindDsDirect,
    #[cfg(feature = "nds")]
    ClearJitCache,
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

    #[cfg(feature = "gba")]
    pub fn pick_gba_bios(&mut self) {
        self.queue.push(AppEvent::PickGbaBios);
    }

    #[cfg(feature = "gba")]
    pub fn use_built_in_bios(&mut self) {
        self.queue.push(AppEvent::UseBuiltInBios);
    }

    #[cfg(feature = "nds")]
    pub fn pick_ds_file(&mut self, file: crate::app::ds_firmware::DsFile) {
        self.queue.push(AppEvent::PickDsFile(file));
    }

    #[cfg(feature = "nds")]
    pub fn remove_ds_file(&mut self, file: crate::app::ds_firmware::DsFile) {
        self.queue.push(AppEvent::RemoveDsFile(file));
    }

    #[cfg(feature = "nds")]
    pub fn clear_jit_cache(&mut self) {
        self.queue.push(AppEvent::ClearJitCache);
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

    pub fn screen_settings(&mut self, phone: bool) {
        self.queue.push(AppEvent::ScreenSettings { phone });
    }

    pub fn centre_tilt(&mut self) {
        self.queue.push(AppEvent::CentreTilt);
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
