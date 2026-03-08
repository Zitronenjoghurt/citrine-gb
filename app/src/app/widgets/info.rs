use egui::{Response, Ui, Widget};
use std::fmt::Display;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

mod changelog;
mod features;
mod welcome;

#[derive(
    Debug, Default, Copy, Clone, PartialEq, serde::Deserialize, serde::Serialize, EnumIter,
)]
pub enum InfoTab {
    #[default]
    Welcome,
    Features,
    Changelog,
}

impl InfoTab {
    pub fn build_md(&self) -> String {
        match self {
            Self::Welcome => welcome::build(),
            Self::Features => features::build(),
            Self::Changelog => changelog::build(),
        }
    }
}

impl Display for InfoTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Welcome => write!(f, "Welcome"),
            Self::Features => write!(f, "Features & Plans"),
            Self::Changelog => write!(f, "Changelog"),
        }
    }
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct InfoState {
    pub tab: InfoTab,
}

pub struct InfoPanel<'a> {
    state: &'a mut InfoState,
    commonmark: &'a mut egui_commonmark::CommonMarkCache,
}

impl<'a> InfoPanel<'a> {
    pub fn new(
        state: &'a mut InfoState,
        commonmark: &'a mut egui_commonmark::CommonMarkCache,
    ) -> Self {
        Self { state, commonmark }
    }
}

impl Widget for InfoPanel<'_> {
    fn ui(mut self, ui: &mut Ui) -> Response {
        ui.style_mut().url_in_tooltip = true;
        ui.vertical_centered(|ui| {
            self.tab_bar(ui);

            ui.separator();

            egui_commonmark::CommonMarkViewer::new().show(
                ui,
                self.commonmark,
                &self.state.tab.build_md(),
            );
        })
        .response
    }
}

impl InfoPanel<'_> {
    fn tab_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for tab in InfoTab::iter() {
                let selected = tab == self.state.tab;
                if ui.selectable_label(selected, tab.to_string()).clicked() {
                    self.state.tab = tab;
                }
            }
        });
    }
}
