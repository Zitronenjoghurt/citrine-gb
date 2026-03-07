use crate::app::tabs::TabViewer;
use crate::app::widgets::rom_info::RomInfo;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    if !viewer.emulator.gb.cartridge.has_rom_loaded {
        ui.small("No ROM loaded");
        return;
    }

    let rom_header = &viewer.emulator.gb.cartridge.header;
    RomInfo::new(rom_header).ui(ui);
}
