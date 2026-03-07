use crate::app::ui_state::UiState;
use crate::emulator::Emulator;
use egui::{Ui, WidgetText};
use strum_macros::EnumIter;

mod game_boy;
mod settings;

pub struct TabViewer<'a> {
    pub emulator: &'a mut Emulator,
    pub tab_queue: &'a mut TabQueue,
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
        }
    }

    fn is_closeable(&self, tab: &Self::Tab) -> bool {
        tab.closable()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter)]
pub enum Tab {
    GameBoy,
    Settings,
}

impl Tab {
    pub fn title(&self) -> &'static str {
        match self {
            Tab::GameBoy => "Game Boy",
            Tab::Settings => "Settings",
        }
    }

    pub fn closable(&self) -> bool {
        !matches!(self, Tab::GameBoy)
    }
}

#[derive(Debug, Default)]
pub struct TabQueue {
    to_open: Vec<Tab>,
}

impl TabQueue {
    pub fn take(&mut self) -> Vec<Tab> {
        std::mem::take(&mut self.to_open)
    }

    pub fn open(&mut self, tab: Tab) {
        self.to_open.push(tab);
    }
}
