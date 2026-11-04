use super::nav::Place;
use crate::app::ui_state::ui_theme::ink_on;
use crate::app::widgets::avatar;
use egui::{Align2, CornerRadius, FontId, Rect, Sense, Ui, pos2, vec2};

pub const BAR_HEIGHT: f32 = 64.0;
pub const RAIL_WIDTH: f32 = 72.0;
const PILL: egui::Vec2 = vec2(52.0, 30.0);
const RAIL_ENTRY: f32 = 64.0;

pub fn bar_height(style: &egui::Style) -> f32 {
    BAR_HEIGHT.max(style.spacing.interact_size.y + 16.0)
}

pub fn rail_width(style: &egui::Style) -> f32 {
    RAIL_WIDTH.max(style.spacing.interact_size.y + 24.0)
}

fn rail_entry(style: &egui::Style) -> f32 {
    RAIL_ENTRY.max(style.spacing.interact_size.y + 16.0)
}

pub struct Entry {
    pub place: Place,
    pub selected: bool,
    pub dot: Option<egui::Color32>,
    pub face: Option<(String, u64)>,
}

pub fn bar(ui: &mut Ui, entries: &[Entry]) -> Option<Place> {
    let area = ui.available_rect_before_wrap();
    let height = bar_height(ui.style());
    let rect = Rect::from_min_size(area.min, vec2(area.width(), height));
    ui.allocate_rect(rect, Sense::hover());
    let width = rect.width() / entries.len().max(1) as f32;
    let mut picked = None;
    for (index, entry) in entries.iter().enumerate() {
        let cell = Rect::from_min_size(
            pos2(rect.left() + width * index as f32, rect.top()),
            vec2(width, height),
        );
        if show(ui, cell, entry) {
            picked = Some(entry.place);
        }
    }
    picked
}

pub fn rail(ui: &mut Ui, entries: &[Entry]) -> Option<Place> {
    let area = ui.available_rect_before_wrap();
    let step = rail_entry(ui.style());
    let total = step * entries.len() as f32;
    let top = area.top() + ((area.height() - total) / 2.0).max(8.0);
    let mut picked = None;
    for (index, entry) in entries.iter().enumerate() {
        let cell = Rect::from_min_size(
            pos2(area.left(), top + step * index as f32),
            vec2(area.width(), step),
        );
        if show(ui, cell, entry) {
            picked = Some(entry.place);
        }
    }
    picked
}

fn show(ui: &mut Ui, cell: Rect, entry: &Entry) -> bool {
    let response = ui.interact(cell, ui.id().with(("place", entry.place)), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            true,
            entry.selected,
            entry.place.label(),
        )
    });
    if !ui.is_rect_visible(cell) {
        return response.clicked();
    }
    let visuals = ui.visuals();
    let pill = Rect::from_center_size(pos2(cell.center().x, cell.top() + 22.0), PILL);
    let t = ui
        .ctx()
        .animate_bool_responsive(response.id, entry.selected);
    let brand = visuals.selection.bg_fill;
    if t > 0.0 {
        let grown = Rect::from_center_size(pill.center(), vec2(PILL.x * (0.6 + 0.4 * t), PILL.y));
        ui.painter()
            .rect_filled(grown, CornerRadius::same(15), brand.gamma_multiply(t));
    }
    let ink = if entry.selected {
        ink_on(brand)
    } else {
        visuals.text_color()
    };
    match &entry.face {
        Some((name, seed)) => {
            let face = Rect::from_center_size(pill.center(), vec2(24.0, 24.0));
            avatar::paint(ui, face, name, *seed, None);
        }
        None => {
            ui.painter().text(
                pill.center(),
                Align2::CENTER_CENTER,
                entry.place.icon(),
                FontId::proportional(21.0),
                ink,
            );
        }
    }
    if let Some(color) = entry.dot {
        avatar::dot_in(
            ui,
            Rect::from_center_size(pill.center(), vec2(30.0, 26.0)),
            color,
        );
    }
    let label = if entry.selected {
        visuals.strong_text_color()
    } else {
        visuals.weak_text_color()
    };
    ui.painter().text(
        pos2(cell.center().x, pill.bottom() + 4.0),
        Align2::CENTER_TOP,
        entry.place.label(),
        FontId::proportional(12.0),
        label,
    );
    response.clicked()
}
