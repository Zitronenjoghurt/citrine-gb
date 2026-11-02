use crate::app::widgets::avatar;
use egui::{Color32, Rect, Ui, vec2};

pub fn unread_badge(ui: &Ui, avatar: Rect, count: usize) {
    count_pill(ui, avatar.right_top() + vec2(-3.0, 5.0), count);
}

pub fn count_pill(ui: &Ui, center: egui::Pos2, count: usize) {
    let text = if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    };
    let galley =
        ui.painter()
            .layout_no_wrap(text, egui::FontId::proportional(11.0), Color32::WHITE);
    let size = vec2((galley.size().x + 8.0).max(16.0), 16.0);
    let rect = Rect::from_center_size(center, size);
    ui.painter()
        .rect_filled(rect.expand(1.5), 9.5, ui.visuals().panel_fill);
    ui.painter().rect_filled(rect, 8.0, avatar::DOT);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
}
