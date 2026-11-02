use egui::{Button, Color32, RichText, Ui, vec2};

pub const CHIP_HEIGHT: f32 = 20.0;

pub fn chip_text(label: impl Into<String>) -> RichText {
    RichText::new(label).size(11.0)
}

pub fn chip(ui: &Ui, text: RichText, fill: Color32) -> Button<'static> {
    Button::new(text)
        .fill(fill)
        .stroke(ui.visuals().widgets.inactive.bg_stroke)
        .corner_radius(4)
        .min_size(vec2(0.0, CHIP_HEIGHT))
}
