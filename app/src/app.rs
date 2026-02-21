use eframe::{Frame, Storage};
use egui::{CentralPanel, Context, FontDefinitions, Window};
use egui_notify::Toasts;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Citrine {
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
    fn update(&mut self, ctx: &Context, frame: &mut Frame) {
        Window::new("Hello World!").show(ctx, |ui| ui.label("Hello World!"));
        CentralPanel::default().show(ctx, |ui| ui.label("Hello World!"));
    }

    fn save(&mut self, storage: &mut dyn Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}
