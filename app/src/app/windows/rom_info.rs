use crate::app::widgets::rom_info::RomInfo;
use crate::app::windows::{ActiveWindows, AppWindow};
use crate::Citrine;
use egui::{Ui, Widget, WidgetText};

#[derive(Default)]
pub struct RomInfoWindow;

impl RomInfoWindow {
    pub fn new() -> Self {
        Self
    }
}

impl AppWindow for RomInfoWindow {
    const ID: ActiveWindows = ActiveWindows::ROM_INFO;

    fn title(_app: &mut Citrine) -> impl Into<WidgetText> {
        "Rom Info"
    }

    fn ui(&mut self, ui: &mut Ui, app: &mut Citrine) {
        if !app.emulator.gb.cartridge.has_rom_loaded {
            ui.small("No ROM loaded");
            return;
        }

        let rom_header = &app.emulator.gb.cartridge.header;
        RomInfo::new(rom_header).ui(ui);
    }
}
