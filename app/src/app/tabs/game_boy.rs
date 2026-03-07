use crate::app::tabs::TabViewer;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        viewer.emulator.ui(ui);
    });
}
