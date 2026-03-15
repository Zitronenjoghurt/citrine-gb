use crate::app::tabs::TabViewer;
use crate::app::widgets::debug_actions::DebugActions;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            DebugActions::new(viewer.emulator, viewer.file_picker).ui(ui);
        });
}
