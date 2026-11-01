use crate::app::tabs::TabViewer;
use crate::app::widgets::performance::PerformanceWidget;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    PerformanceWidget::new(viewer.emulator, viewer.ui).ui(ui);
}
