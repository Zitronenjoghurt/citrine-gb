use crate::app::tabs::TabViewer;
use crate::app::widgets::apu_waves::ApuWaves;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    ApuWaves::new(viewer.emulator).ui(ui);
}
