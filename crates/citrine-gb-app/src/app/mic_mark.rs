use crate::icons;
use egui::{Align2, Color32, FontId, Painter, Rect, Stroke, Vec2};

pub const SIZE: Vec2 = Vec2::splat(20.0);

pub fn paint(painter: &Painter, rect: Rect) {
    let dark = painter.ctx().style().visuals.dark_mode;
    let (card, line, mark) = if dark {
        (
            Color32::from_rgba_unmultiplied(27, 27, 27, 217),
            Color32::from_white_alpha(40),
            Color32::from_rgb(255, 107, 94),
        )
    } else {
        (
            Color32::from_rgba_unmultiplied(250, 250, 250, 217),
            Color32::from_black_alpha(40),
            Color32::from_rgb(208, 52, 44),
        )
    };
    let radius = rect.width().min(rect.height()) / 2.0;
    painter.circle(rect.center(), radius, card, Stroke::new(1.0_f32, line));
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        icons::MICROPHONE,
        FontId::proportional(radius * 1.25),
        mark,
    );
}
