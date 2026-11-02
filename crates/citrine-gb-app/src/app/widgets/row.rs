use egui::{Rect, Shape, Ui};

pub fn row_highlight(ui: &Ui, rect: Rect) -> Shape {
    Shape::rect_filled(
        rect.expand(2.0),
        6.0,
        ui.visuals()
            .widgets
            .hovered
            .weak_bg_fill
            .gamma_multiply(0.4),
    )
}
