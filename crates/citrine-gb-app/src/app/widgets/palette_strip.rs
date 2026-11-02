use citrine_gb::gb::ppu::types::color::RGBA;
use egui::{Color32, Rect, Sense, Ui, vec2};

pub fn palette_strip(ui: &mut Ui, rows: &[[RGBA; 4]]) {
    let height = 26.0;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let swatch = width / 4.0;
    let row_heights: &[f32] = if rows.len() == 3 {
        &[14.0, 6.0, 6.0]
    } else {
        &[height]
    };

    let mut top = 0.0;
    for (row, row_height) in rows.iter().zip(row_heights) {
        for (index, color) in row.iter().enumerate() {
            let cell = Rect::from_min_size(
                rect.min + vec2(swatch * index as f32, top),
                vec2(swatch, *row_height),
            );
            ui.painter().rect_filled(
                cell,
                0.0,
                Color32::from_rgb(color.r(), color.g(), color.b()),
            );
        }
        top += row_height;
    }
    ui.painter().rect_stroke(
        rect,
        2.0,
        ui.visuals().widgets.inactive.bg_stroke,
        egui::StrokeKind::Inside,
    );
}
