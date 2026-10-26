use crate::app::tabs::TabViewer;
use crate::app::widgets::apu::ApuWidget;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    ApuWidget::new(viewer.emulator, viewer.events).ui(ui);
}
