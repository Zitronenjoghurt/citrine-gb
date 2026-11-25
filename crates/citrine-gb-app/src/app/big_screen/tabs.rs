use super::glyphs::{self, Family, Glyph};
use super::ring;
use crate::app::mobile::nav::Place;
use crate::app::ui_state::keybinds::PadButton;
use crate::app::ui_state::ui_theme::ink_on;
use crate::app::widgets::avatar;
use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Color32, FontId, Id, LayerId, Layout, Order, Painter, Rect, Sense, Ui, UiBuilder, pos2,
    vec2,
};

pub const HEIGHT: f32 = 56.0;
const TAB: f32 = 34.0;
const SIDE: f32 = 16.0;
const ICON: f32 = 18.0;
const ICON_GAP: f32 = 8.0;
const LABEL: f32 = 16.0;
const GAME_LABEL_MAX: f32 = 160.0;
const MARK: f32 = 20.0;
pub(super) const MARK_GAP: f32 = 10.0;
const GAP: f32 = ring::MARGIN + MARK_GAP;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Game,
    Place(Place),
}

pub struct Entry {
    pub tab: Tab,
    pub label: String,
    pub icon: &'static str,
    pub selected: bool,
    pub dot: Option<Color32>,
    pub face: Option<(String, u64)>,
}

pub fn bar(
    ui: &mut Ui,
    entries: &[Entry],
    marks: bool,
    family: Family,
    confirm: PadButton,
    lift: bool,
) -> Option<Tab> {
    let area = ui.available_rect_before_wrap();
    let rect = Rect::from_min_size(area.min, vec2(area.width(), HEIGHT));
    ui.allocate_rect(rect, Sense::hover());
    let galleys: Vec<_> = entries
        .iter()
        .map(|entry| {
            let most = if entry.tab == Tab::Game {
                GAME_LABEL_MAX
            } else {
                f32::INFINITY
            };
            label_galley(ui, &entry.label, most)
        })
        .collect();
    let widths: Vec<f32> = galleys
        .iter()
        .map(|galley| SIDE + ICON + ICON_GAP + galley.size().x + SIDE)
        .collect();
    let span =
        |widths: &[f32]| widths.iter().sum::<f32>() + GAP * widths.len().saturating_sub(1) as f32;
    let games = entries
        .iter()
        .take_while(|entry| entry.tab == Tab::Game)
        .count();
    let before = span(&widths[..games]) + if games > 0 { GAP } else { 0.0 };
    let places = span(&widths[games..]);
    let held = rect.center().x - places / 2.0 - before;
    let centred = rect.center().x - (before + places) / 2.0;
    let edge = rect.left() + SIDE + MARK + GAP;
    let mut left = held.max(edge.min(centred));
    let top = rect.center().y - TAB / 2.0;
    let mut picked = None;
    let mut cells = Vec::with_capacity(entries.len());
    for ((entry, galley), width) in entries.iter().zip(galleys).zip(&widths) {
        let cell = Rect::from_min_size(pos2(left, top), vec2(*width, TAB));
        if show(ui, cell, entry, galley, lift) {
            picked = Some(entry.tab);
        }
        cells.push(cell);
        left += width + GAP;
    }
    if marks && let Some([before, after]) = mark_rooms(rect, &cells) {
        let marks = [
            (
                before,
                Layout::right_to_left(Align::Center),
                PadButton::LeftTrigger,
            ),
            (
                after,
                Layout::left_to_right(Align::Center),
                PadButton::RightTrigger,
            ),
        ];
        for (room, layout, button) in marks {
            ui.scope_builder(UiBuilder::new().max_rect(room).layout(layout), |ui| {
                glyphs::paint(ui, family, Glyph::Shoulder(button), confirm, MARK);
            });
        }
    }
    picked
}

fn mark_rooms(bar: Rect, cells: &[Rect]) -> Option<[Rect; 2]> {
    let (first_cell, last_cell) = (cells.first()?, cells.last()?);
    let before = Rect::from_min_max(
        pos2(bar.left(), bar.top()),
        pos2(first_cell.left() - ring::MARGIN - MARK_GAP, bar.bottom()),
    );
    let after = Rect::from_min_max(
        pos2(last_cell.right() + ring::MARGIN + MARK_GAP, bar.top()),
        pos2(bar.right(), bar.bottom()),
    );
    Some([before, after])
}

fn label_galley(ui: &Ui, text: &str, most: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(
        text.to_owned(),
        FontId::proportional(LABEL),
        Color32::PLACEHOLDER,
    );
    job.wrap = TextWrapping::truncate_at_width(most);
    ui.painter().layout_job(job)
}

fn show(
    ui: &mut Ui,
    cell: Rect,
    entry: &Entry,
    galley: std::sync::Arc<egui::Galley>,
    lift: bool,
) -> bool {
    let response = ui.interact(cell, ui.id().with(("big_tab", entry.tab)), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, entry.selected, &entry.label)
    });
    if !ui.is_rect_visible(cell) {
        return response.clicked();
    }
    ring::mark(
        &response,
        ring::Mark::Ring {
            rect: cell,
            radius: TAB / 2.0,
        },
    );
    let painter = if lift && entry.selected {
        lifted_painter(ui)
    } else {
        ui.painter().clone()
    };
    let visuals = ui.visuals();
    let brand = visuals.selection.bg_fill;
    let ink = if entry.selected {
        painter.rect_filled(cell, TAB / 2.0, brand);
        ink_on(brand)
    } else {
        visuals.text_color()
    };
    let icon = Rect::from_min_size(
        pos2(cell.left() + SIDE, cell.center().y - ICON / 2.0),
        vec2(ICON, ICON),
    );
    match &entry.face {
        Some((name, seed)) => {
            let rect = icon.expand(2.0);
            let painter = painter.with_clip_rect(rect.expand(2.0).intersect(painter.clip_rect()));
            avatar::paint_with(&painter, ui.visuals(), rect, name, *seed, None);
        }
        None => {
            painter.text(
                icon.center(),
                egui::Align2::CENTER_CENTER,
                entry.icon,
                FontId::proportional(ICON),
                ink,
            );
        }
    }
    if let Some(color) = entry.dot {
        avatar::dot_in(ui, icon.expand(4.0), color);
    }
    let at = pos2(
        icon.right() + ICON_GAP,
        cell.center().y - galley.size().y / 2.0,
    );
    painter.galley(at, galley, ink);
    response.clicked()
}

fn lifted_painter(ui: &Ui) -> Painter {
    ui.ctx()
        .layer_painter(LayerId::new(Order::Tooltip, Id::new("big_tab_lifted")))
}
