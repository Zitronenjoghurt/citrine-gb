use crate::app::tabs::Tab;
use crate::app::ui_state::settings::SettingsTab;
use crate::media::MediaId;
use citrine_gb::gb::GbModel;

pub enum AppEvent {
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
        data: Vec<u8>,
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
    CentreTilt,
    #[cfg(feature = "gba")]
    PickGbaBios,
    #[cfg(feature = "gba")]
    UseBuiltInBios,
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

    pub fn play_on(&mut self, model: GbModel) {
        self.queue.push(AppEvent::PlayOn { model });
    }

    pub fn pick_second_game(&mut self) {
        self.queue.push(AppEvent::PickSecondGame);
    }

    pub fn link_game(&mut self, data: Vec<u8>, name: Option<String>) {
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

    pub fn centre_tilt(&mut self) {
        self.queue.push(AppEvent::CentreTilt);
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
