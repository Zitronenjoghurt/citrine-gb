use crate::app::tabs::TabViewer;
use crate::app::widgets::time_control::TimeControl;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    TimeControl::new(viewer.emulator, &mut viewer.ui.time_ctrl).ui(ui);
}
