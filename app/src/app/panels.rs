use crate::app::widgets::registers::Registers;
use egui::{Ui, Widget};
use strum_macros::EnumIter;

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Panels {
    pub left: Option<PanelKind>,
    pub right: Option<PanelKind>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize, EnumIter)]
pub enum PanelKind {
    Registers,
}

impl PanelKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Registers => "Registers",
        }
    }

    pub fn ui(&self, ui: &mut Ui, app: &mut crate::Citrine) {
        match self {
            Self::Registers => Registers::new(&app.emulator.gb, &mut app.ui.registers).ui(ui),
        };
    }
}
