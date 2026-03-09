use crate::app::tabs::TabViewer;
use crate::app::widgets::apu::ApuWidget;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ApuWidget::new(viewer.emulator, viewer.events).ui(ui);
        });
}
