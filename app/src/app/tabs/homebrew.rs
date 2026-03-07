use crate::app::tabs::TabViewer;
use crate::app::widgets::homebrew::HomebrewList;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    HomebrewList::new(viewer.events).ui(ui);
}
