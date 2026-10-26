use crate::app::tabs::TabViewer;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    viewer.ui.toast_area = Some(ui.max_rect());
    ui.vertical_centered(|ui| {
        viewer.emulator.ui(ui);
    });
}
