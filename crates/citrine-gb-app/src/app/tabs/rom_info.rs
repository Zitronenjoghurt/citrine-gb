use crate::app::tabs::TabViewer;
use crate::app::widgets::rom_info::RomInfo;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    if !viewer.emulator.has_game() {
        ui.small("No ROM loaded");
        return;
    }

    #[cfg(feature = "nds")]
    if viewer.emulator.kind() == citrine_gb::console::ConsoleKind::Nds {
        crate::app::widgets::rom_info::NdsRomInfo::new(&viewer.emulator.rom_bytes()).ui(ui);
        return;
    }

    #[cfg(feature = "gba")]
    if let Some(gba) = viewer.emulator.as_gba() {
        let header = citrine_gb::gba::header::GbaHeader::parse(gba.bus().rom());
        crate::app::widgets::rom_info::GbaRomInfo::new(&header, gba.bus().backup().kind()).ui(ui);
        return;
    }

    let model = viewer.emulator.console();
    let Some(gb) = viewer.emulator.as_gb() else {
        return;
    };
    RomInfo::new(&gb.cartridge.header)
        .console(model, viewer.events)
        .ui(ui);
}
