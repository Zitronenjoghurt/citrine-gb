use citrine_gb::gb::ppu::types::color::RGBA;
use egui::{Color32, Rect, Sense, Ui, vec2};

pub fn palette_strip(ui: &mut Ui, rows: &[[RGBA; 4]]) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    paint_strip(ui, rect, rows);
}

pub fn paint_strip(ui: &Ui, rect: Rect, rows: &[[RGBA; 4]]) {
    let height = rect.height();
    let swatch = rect.width() / 4.0;
    let row_heights: &[f32] = if rows.len() == 3 {
        &[height * 0.54, height * 0.23, height * 0.23]
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
        egui::Stroke::new(1.0_f32, ui.visuals().weak_text_color().gamma_multiply(0.35)),
        egui::StrokeKind::Inside,
    );
}
