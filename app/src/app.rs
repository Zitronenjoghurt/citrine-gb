use crate::app::file_picker::{FileIntent, FilePicker};
use crate::app::panels::Panels;
use crate::emulator::Emulator;
use crate::icons;
use eframe::{Frame, Storage};
use egui::{CentralPanel, Context, FontDefinitions, TopBottomPanel};
use egui_notify::Toasts;

mod file_picker;
mod panels;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Citrine {
    pub panels: Panels,
    #[serde(skip, default)]
    pub emulator: Emulator,
    #[serde(skip, default)]
    pub file_picker: FilePicker,
    #[serde(skip, default)]
    pub toasts: Toasts,
}

impl Citrine {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        cc.egui_ctx.set_pixels_per_point(1.5);
        Self::setup_fonts(&cc.egui_ctx);
        let mut app = cc
            .storage
            .and_then(|storage| eframe::get_value::<Self>(storage, eframe::APP_KEY))
            .unwrap_or_default();
        app.file_picker.set_drop_intent(FileIntent::LoadRom);
        app
    }

    fn setup_fonts(ctx: &Context) {
        let mut fonts = FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        ctx.set_fonts(fonts);
    }
}

impl eframe::App for Citrine {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        if let Some(result) = self.file_picker.poll(ctx) {
            // ToDo: Handle result
        }

        self.emulator.update(ctx);
        TopBottomPanel::top("top_panel").show(ctx, |ui| self.top_panel(ui));
        CentralPanel::default().show(ctx, |ui| self.central_panel(ui));

        self.file_picker.show_drop_overlay(ctx);
        self.toasts.show(ctx);
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

            ui.menu_button(icons::FOLDER, |ui| {
                if ui.button("Load ROM").clicked() {
                    self.file_picker.open(FileIntent::LoadRom);
                    ui.close_kind(egui::UiKind::Menu);
                }
            });

            ui.separator();

            ui.label(format!("{:.02}ms", self.emulator.last_frame_secs * 1000.0))
        });
    }

    fn central_panel(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            self.emulator.ui(ui);
        });
    }
}
