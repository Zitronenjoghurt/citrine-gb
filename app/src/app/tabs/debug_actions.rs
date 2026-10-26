use crate::app::tabs::TabViewer;
use crate::app::widgets::debug_actions::DebugActions;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    DebugActions::new(viewer.emulator, viewer.files).ui(ui);
}
