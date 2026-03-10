use crate::app::tabs::TabViewer;
use crate::app::widgets::disassembly::DisassemblyView;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            DisassemblyView::new(
                &viewer.emulator.gb.cpu,
                &viewer.emulator.gb.cartridge,
                &viewer.emulator.gb.debugger.disassembly,
                &mut viewer.emulator.gb.debugger.breakpoints,
                &mut viewer.emulator.gb.debugger.static_analysis_enabled,
                &mut viewer.ui.settings.track_pc,
            )
            .ui(ui);
        });
}
