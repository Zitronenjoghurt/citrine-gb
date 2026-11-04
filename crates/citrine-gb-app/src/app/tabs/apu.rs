use crate::app::tabs::TabViewer;
use crate::app::widgets::apu::ApuWidget;
use citrine_gb::console::Console;
use citrine_gb::debug::Debug;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let machine = viewer.emulator.debug();
    ApuWidget::new(
        machine.audio_channels(),
        machine.waves(),
        &mut viewer.ui.settings,
    )
    .ui(ui);
}
