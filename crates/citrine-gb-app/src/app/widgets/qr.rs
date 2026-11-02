use egui::{Color32, Rect, Response, Sense, Ui, Widget, WidgetInfo, WidgetType, vec2};
use qrcode::{Color, QrCode};

const QUIET_CELLS: usize = 2;

pub struct Qr<'a> {
    text: &'a str,
    side: f32,
}

impl<'a> Qr<'a> {
    pub fn new(text: &'a str, side: f32) -> Self {
        Self { text, side }
    }
}

impl Widget for Qr<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(vec2(self.side, self.side), Sense::hover());
        if ui.is_rect_visible(rect)
            && let Ok(code) = QrCode::new(self.text.as_bytes())
        {
            let width = code.width();
            let cell = self.side / (width + 2 * QUIET_CELLS) as f32;
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, Color32::WHITE);
            for (i, color) in code.to_colors().into_iter().enumerate() {
                if color != Color::Dark {
                    continue;
                }
                let (x, y) = (i % width + QUIET_CELLS, i / width + QUIET_CELLS);
                let min = rect.min + vec2(x as f32 * cell, y as f32 * cell);
                let square = Rect::from_min_size(min, vec2(cell, cell)).expand(0.25);
                painter.rect_filled(square, 0.0, Color32::BLACK);
            }
        }
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "QR code"));
        response
    }
}
