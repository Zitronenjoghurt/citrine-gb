use super::Action;
use crate::app::widgets::emoji;
use crate::icons;
use crate::online::Online;
use crate::online::chat::ChatEntry;
use citrine_gb_net::{OnlineId, Quote};
use egui::layers::ShapeIdx;
use egui::{
    Align, Color32, CornerRadius, CursorIcon, Layout, Rect, RichText, Sense, TextStyle, Ui, vec2,
};

pub(super) const FLASH_SECS: f64 = 1.5;
pub(super) const SWIPE_REPLY: f32 = 56.0;
const SWIPE_START: f32 = 12.0;
const BAR: f32 = 3.0;
const QUOTE_MARGIN: egui::Margin = egui::Margin {
    left: 9,
    right: 8,
    top: 3,
    bottom: 3,
};
pub(super) const QUOTE_GAP: f32 = 4.0;
pub const DELETED: &str = "Original message deleted";

#[derive(Debug, Clone, PartialEq)]
pub(super) struct QuoteView {
    pub author: String,
    pub line: String,
    pub gone: bool,
    pub target: Option<u64>,
}

impl QuoteView {
    pub(super) fn of(
        online: &Online,
        friend: OnlineId,
        entry: &ChatEntry,
        quote: &Quote,
        before: &[ChatEntry],
    ) -> Self {
        let author = author(online, friend, entry.quotes_mine() == Some(true));
        match before.iter().rev().find(|e| e.id == quote.id) {
            Some(original) if original.is_deleted() => Self {
                author,
                line: DELETED.to_owned(),
                gone: true,
                target: None,
            },
            Some(original) => Self {
                author,
                line: original.quote().line,
                gone: false,
                target: Some(original.id),
            },
            None => Self {
                author,
                line: quote.line.clone(),
                gone: false,
                target: None,
            },
        }
    }
}

pub(super) fn author(online: &Online, friend: OnlineId, mine: bool) -> String {
    if mine {
        return "You".to_owned();
    }
    online
        .friend(friend)
        .map_or_else(|| "Friend".to_owned(), |f| f.display_name.clone())
}

pub(super) fn quote_width(ui: &Ui, view: &QuoteView) -> f32 {
    let author = emoji::line_width(ui, &view.author, TextStyle::Small);
    let line = emoji::line_width(ui, &view.line, TextStyle::Small);
    author.max(line) + QUOTE_MARGIN.sum().x + 1.0
}

pub(super) fn quote_block(
    ui: &mut Ui,
    view: &QuoteView,
    mine: bool,
    ink: Color32,
    actions: &mut Vec<Action>,
) {
    let accent = if mine {
        ink
    } else {
        crate::app::ui_state::ui_theme::brand_mark(ui.visuals())
    };
    let shade = if ui.visuals().dark_mode ^ mine {
        Color32::from_white_alpha(14)
    } else {
        Color32::from_black_alpha(14)
    };
    let shown = egui::Frame::new()
        .fill(shade)
        .corner_radius(6.0)
        .inner_margin(QUOTE_MARGIN)
        .show(ui, |ui| {
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                emoji::line(ui, &view.author, TextStyle::Small, accent);
                if view.gone {
                    let weak = ink.gamma_multiply(0.7);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&view.line)
                                .text_style(TextStyle::Small)
                                .italics()
                                .color(weak),
                        )
                        .truncate(),
                    );
                } else {
                    emoji::line(ui, &view.line, TextStyle::Small, ink);
                }
            });
        });
    let rect = shown.response.rect;
    let bar = Rect::from_min_max(rect.min, egui::pos2(rect.min.x + BAR, rect.max.y));
    ui.painter().rect_filled(
        bar,
        CornerRadius {
            nw: 6,
            sw: 6,
            ne: 0,
            se: 0,
        },
        accent,
    );
    if let Some(target) = view.target {
        let response = ui
            .interact(rect, ui.id().with(("quote", target)), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Show the original");
        if response.clicked() {
            actions.push(Action::ShowOriginal(target));
        }
    }
    ui.add_space(QUOTE_GAP);
}

pub(super) fn flash(ui: &Ui, spot: ShapeIdx, top: f32, bubble: Rect, since: f64) {
    let fade = (1.0 - since / FLASH_SECS).clamp(0.0, 1.0) as f32;
    let row = Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=bubble.bottom());
    let color = ui.visuals().selection.bg_fill.gamma_multiply(0.35 * fade);
    ui.painter()
        .set(spot, egui::Shape::rect_filled(row.expand(2.0), 6.0, color));
    ui.ctx().request_repaint();
}

pub(super) fn swipe_of(input: &egui::InputState, bubble: Rect) -> Option<f32> {
    if !input.any_touches() || !input.pointer.primary_down() {
        return None;
    }
    let origin = input.pointer.press_origin()?;
    let moved = input.pointer.total_drag_delta()?;
    (bubble.contains(origin) && moved.x >= SWIPE_START && moved.x > moved.y.abs() * 2.0)
        .then_some(moved.x.min(SWIPE_REPLY))
}

pub(super) fn swipe_shift(ui: &Ui, first: usize, shift: f32) {
    if shift <= 0.0 {
        return;
    }
    let layer = ui.layer_id();
    ui.ctx().graphics_mut(|layers| {
        let list = layers.entry(layer);
        for index in first..list.next_idx().0 {
            list.mutate_shape(ShapeIdx(index), |clipped| {
                clipped.shape.translate(vec2(shift, 0.0));
            });
        }
    });
}

pub(super) fn swipe_mark(ui: &Ui, bubble: Rect, shift: f32) {
    let progress = shift / SWIPE_REPLY;
    let color = ui.visuals().weak_text_color().gamma_multiply(progress);
    let center = egui::pos2(bubble.left() + shift / 2.0, bubble.center().y);
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        icons::ARROW_BEND_UP_LEFT,
        TextStyle::Body.resolve(ui.style()),
        color,
    );
}

pub(super) fn reply_bar(ui: &mut Ui, author: &str, line: &str, side: f32) -> bool {
    let mut cancel = false;
    let view = QuoteView {
        author: author.to_owned(),
        line: line.to_owned(),
        gone: false,
        target: None,
    };
    let ink = ui.visuals().text_color();
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            cancel = super::composer::cancel_button(ui, side, "Cancel reply").clicked();
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.vertical(|ui| quote_block(ui, &view, false, ink, &mut Vec::new()));
            });
        });
    });
    cancel
}
