use egui::Ui;
use strum_macros::EnumIter;

mod debug;

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Panels {
    pub left: Option<PanelKind>,
    pub right: Option<PanelKind>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize, EnumIter)]
pub enum PanelKind {
    Debug,
}

impl PanelKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Debug => "Debug",
        }
    }

    pub fn ui(&self, ui: &mut Ui, app: &mut crate::Citrine) {
        match self {
            Self::Debug => debug::debug(ui, app),
        }
    }
}
