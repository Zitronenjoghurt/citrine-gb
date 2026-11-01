use crate::app::tabs::TabViewer;
use crate::app::widgets::registers::Registers;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    Registers::new(&viewer.emulator.gb, &mut viewer.ui.registers).ui(ui);
}
