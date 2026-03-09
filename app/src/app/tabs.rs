use crate::app::events::AppEventQueue;
use crate::app::file_picker::FilePicker;
use crate::app::ui_state::UiState;
use crate::audio::Audio;
use crate::emulator::Emulator;
use egui::{Ui, WidgetText};
use strum_macros::EnumIter;

mod apu;
mod apu_waves;
mod audio_debug;
mod e2e;
mod game_boy;
mod homebrew;
mod info;
mod registers;
mod rom_info;
mod settings;
mod time_control;

pub struct TabViewer<'a> {
    pub audio: &'a mut Option<Audio>,
    pub commonmark: &'a mut egui_commonmark::CommonMarkCache,
    pub emulator: &'a mut Emulator,
    pub events: &'a mut AppEventQueue,
    pub file_picker: &'a mut FilePicker,
    pub ui: &'a mut UiState,
}

impl<'a> egui_dock::TabViewer for TabViewer<'a> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::GameBoy => game_boy::show(self, ui),
            Tab::Settings => settings::show(self, ui),
            Tab::TimeControl => time_control::show(self, ui),
            Tab::Registers => registers::show(self, ui),
            Tab::RomInfo => rom_info::show(self, ui),
            Tab::E2ETest => e2e::show(self, ui),
            Tab::Homebrew => homebrew::show(self, ui),
            Tab::Info => info::show(self, ui),
            Tab::AudioDebug => audio_debug::show(self, ui),
            Tab::Apu => apu::show(self, ui),
            Tab::ApuWaves => apu_waves::show(self, ui),
        }
    }

    fn is_closeable(&self, tab: &Self::Tab) -> bool {
        tab.closable()
    }

    fn allowed_in_windows(&self, tab: &mut Self::Tab) -> bool {
        tab.closable()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter)]
pub enum Tab {
    Apu,
    ApuWaves,
    AudioDebug,
    E2ETest,
    GameBoy,
    Homebrew,
    Info,
    Registers,
    RomInfo,
    Settings,
    TimeControl,
}

impl Tab {
    pub fn title(&self) -> &'static str {
        match self {
            Tab::GameBoy => "Game Boy",
            Tab::Settings => "Settings",
            Tab::TimeControl => "Time Control",
            Tab::Registers => "Registers",
            Tab::RomInfo => "ROM Info",
            Tab::E2ETest => "E2E Tests",
            Tab::Homebrew => "Homebrew",
            Tab::Info => "General Info",
            Tab::AudioDebug => "Audio Debug",
            Tab::Apu => "APU",
            Tab::ApuWaves => "APU Waves",
        }
    }

    pub fn closable(&self) -> bool {
        !matches!(self, Tab::GameBoy)
    }
}
