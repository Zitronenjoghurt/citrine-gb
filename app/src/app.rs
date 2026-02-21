use crate::emulator::Emulator;
use eframe::{Frame, Storage};
use egui::{CentralPanel, Context, FontDefinitions, TopBottomPanel};
use egui_notify::Toasts;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Citrine {
    #[serde(skip, default)]
    emulator: Emulator,
    #[serde(skip, default)]
    toasts: Toasts,
}

impl Citrine {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        cc.egui_ctx.set_pixels_per_point(1.5);
        Self::setup_fonts(&cc.egui_ctx);
        cc.storage
            .and_then(|storage| eframe::get_value::<Self>(storage, eframe::APP_KEY))
            .unwrap_or_default()
    }

    fn setup_fonts(ctx: &Context) {
        let mut fonts = FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        ctx.set_fonts(fonts);
    }
}

impl eframe::App for Citrine {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        self.emulator.update(ctx);
        TopBottomPanel::top("top_panel").show(ctx, |ui| self.top_panel(ui));
        CentralPanel::default().show(ctx, |ui| self.central_panel(ui));
    }

    fn save(&mut self, storage: &mut dyn Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}

// Rendering
impl Citrine {
    fn top_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Citrine");
            ui.separator();
        });
    }

    fn central_panel(&mut self, ui: &mut egui::Ui) {
        self.emulator.ui(ui);
    }
}
