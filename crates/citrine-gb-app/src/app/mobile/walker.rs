use super::kit;
use egui::{Color32, CornerRadius, Frame, Ui};

pub fn card(ui: &Ui) -> Frame {
    Frame::new()
        .fill(kit::card_fill(ui.visuals()))
        .corner_radius(CornerRadius::same(kit::RADIUS))
}

pub fn tile_fill(visuals: &egui::Visuals, hovered: bool) -> Color32 {
    if hovered {
        kit::card_fill(visuals).lerp_to_gamma(visuals.strong_text_color(), 0.06)
    } else {
        kit::card_fill(visuals)
    }
}

pub fn use_here(ui: &mut Ui) -> bool {
    kit::pill(ui, "Use here", true).clicked()
}
