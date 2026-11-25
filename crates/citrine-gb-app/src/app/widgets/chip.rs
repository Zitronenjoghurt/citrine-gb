use crate::app::ui_state::ui_theme::{BRAND, BRAND_DARK, BRAND_EDGE};
use egui::{Button, Color32, Galley, Rect, RichText, Stroke, StrokeKind, Ui, Visuals, pos2, vec2};
use std::sync::Arc;

pub const CHIP_HEIGHT: f32 = 20.0;

pub const TAG_PAD: f32 = 6.0;

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

pub fn tag_fill(visuals: &Visuals, own: bool) -> Color32 {
    match (own, visuals.dark_mode) {
        (true, true) => BRAND_DARK.gamma_multiply(0.22),
        (true, false) => BRAND.gamma_multiply(0.45),
        (false, true) => Color32::from_white_alpha(18),
        (false, false) => Color32::from_black_alpha(14),
    }
}

pub fn tag_stroke(visuals: &Visuals, own: bool) -> Stroke {
    match (own, visuals.dark_mode) {
        (true, true) => Stroke::new(1.0_f32, BRAND_DARK),
        (true, false) => Stroke::new(1.0_f32, BRAND_EDGE),
        (false, _) => Stroke::NONE,
    }
}

pub fn tag_width(galley: &Galley) -> f32 {
    galley.size().x + 2.0 * TAG_PAD
}

pub fn paint_tag(ui: &Ui, rect: Rect, galley: Arc<Galley>, own: bool) {
    let visuals = ui.visuals();
    ui.painter().rect(
        rect,
        rect.height() * 0.42,
        tag_fill(visuals, own),
        tag_stroke(visuals, own),
        StrokeKind::Inside,
    );
    let top = rect.center().y - galley.size().y / 2.0;
    ui.painter().galley(
        pos2(rect.center().x - galley.size().x / 2.0, top),
        galley,
        visuals.strong_text_color(),
    );
}
