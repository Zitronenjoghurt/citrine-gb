use super::clock::Clock;
use super::menu::{menu_id, message_menu};
use super::palette::{card_width, palette_card};
use super::share::share_card;
use super::{
    Action, BIG_EMOJI, BIG_EMOJI_GAP, BIG_EMOJI_MAX, BUBBLE_MARGIN, BUBBLE_SHARE, ChatUi,
    INLINE_EMOJI, LONG_PRESS_SECS, LONG_PRESS_SLOP, STAMP_GAP, TEXT_STAMP_GAP,
};
use crate::app::ui_state::palette::PaletteLibrary;
use crate::app::widgets::emoji;
use crate::app::widgets::problem::problem_text;
use crate::app::widgets::text::faint;
use crate::game_name::GameNames;
use crate::icons;
use crate::online::Online;
use crate::online::chat::{ChatEntry, Delivery, Failure};
use citrine_gb_net::{Body, OnlineId};
use egui::{Align, Color32, Label, Layout, Rect, RichText, Ui, vec2};

#[allow(clippy::too_many_arguments)]
pub(super) fn entry_ui(
    ui: &mut Ui,
    state: &mut ChatUi,
    online: &Online,
    friend: OnlineId,
    entry: &ChatEntry,
    clock: &Clock,
    rom: Option<&str>,
    names: &GameNames,
    palettes: &PaletteLibrary,
    actions: &mut Vec<Action>,
) -> Rect {
    if entry.is_deleted() {
        return deleted_ui(ui, online, entry, clock, actions);
    }
    let room = ui.available_width();
    let max = (room * BUBBLE_SHARE).max(120.0).min(room);
    let layout = if entry.mine {
        Layout::right_to_left(Align::Min)
    } else {
        Layout::left_to_right(Align::Min)
    };
    let mut own_tip = false;
    let bubble = ui.with_layout(layout, |ui| {
        ui.set_max_width(max);
        let big = match &entry.body {
            Body::Text(text) => emoji::only_emoji(&emoji::pieces(text))
                .filter(|n| *n <= BIG_EMOJI_MAX)
                .map(|count| BigRow::fit(count, max, stamp_width(ui, entry, clock))),
            _ => None,
        };
        let margin = if big.is_some() {
            egui::Margin::symmetric(0, BUBBLE_MARGIN.top)
        } else {
            BUBBLE_MARGIN
        };
        let fit_id = ui.id().with(("bubble_fit", entry.id));
        let wraps = match &entry.body {
            Body::Text(text) => {
                let needed = text_width(ui, text, big.as_ref(), entry, clock) + margin.sum().x;
                let fitted = ui
                    .data(|d| d.get_temp::<(f32, f32)>(fit_id))
                    .filter(|(at, _)| (at - max).abs() < 0.5)
                    .map(|(_, content)| content + margin.sum().x);
                let width = needed.min(fitted.unwrap_or(max));
                if width < max {
                    ui.set_max_width(width);
                }
                needed >= max
            }
            Body::Palette(_) => {
                ui.set_max_width((card_width(ui) + margin.sum().x).min(room));
                false
            }
            _ => false,
        };
        let fill = match big {
            Some(_) => Color32::TRANSPARENT,
            None => bubble_fill(ui.visuals(), entry.mine),
        };
        let first = paint_index(ui);
        let shown = egui::Frame::new()
            .fill(fill)
            .corner_radius(10.0)
            .inner_margin(margin)
            .show(ui, |ui| {
                let content =
                    ui.with_layout(Layout::top_down(Align::Min), |ui| match &entry.body {
                        Body::Text(text) => text_body(ui, text, big.as_ref(), entry, clock),
                        Body::Share(share) => {
                            own_tip = share_card(
                                ui, state, online, friend, entry, share, rom, names, actions,
                            );
                            stamp(ui, entry, clock);
                        }
                        Body::Palette(json) => {
                            palette_card(ui, entry, clock, json, palettes, actions);
                        }
                        Body::Receipt(_) | Body::Edit(_) | Body::Delete(_) => {}
                    });
                if wraps {
                    let width = content.response.rect.width();
                    let known = ui.data(|d| d.get_temp::<(f32, f32)>(fit_id));
                    if known.is_none_or(|(at, was)| {
                        (at - max).abs() >= 0.5 || (was - width).abs() >= 0.5
                    }) {
                        ui.data_mut(|d| d.insert_temp(fit_id, (max, width)));
                        ui.ctx().request_discard("chat bubble width");
                    }
                }
                content.response.rect
            });
        if wraps {
            hug_words(ui, first, shown.inner, entry.mine);
        }
        shown.response.rect
    });
    let bubble = bubble.inner;
    message_menu(ui, state, online, friend, entry, bubble, palettes, actions);
    if entry.mine
        && let Some(text) = receipt_text(entry.delivery, clock)
    {
        let delay = ui.style().interaction.tooltip_delay;
        let (held, resting) = ui.input(|i| {
            let resting = !i.any_touches() && i.pointer.time_since_last_movement() >= delay;
            (long_pressed(i, bubble), resting)
        });
        if held {
            state.receipt = Some(entry.id);
        }
        if resting
            && !own_tip
            && !egui::Popup::is_id_open(ui.ctx(), menu_id(entry.id))
            && ui.rect_contains_pointer(bubble)
        {
            let corner = bubble.right_bottom() - BUBBLE_MARGIN.right_bottom();
            let ticks = Rect::from_min_max(
                corner
                    - vec2(
                        stamp_width(ui, entry, clock),
                        ui.text_style_height(&egui::TextStyle::Small),
                    ),
                corner,
            );
            let mut tip = egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                egui::Id::new(("chat_receipt", entry.id)),
                ticks,
            )
            .gap(BUBBLE_MARGIN.bottom as f32 + 4.0);
            tip.popup = tip.popup.align(egui::RectAlign::BOTTOM_END);
            tip.show(|ui| ui.add(egui::Label::new(&text).extend()));
        }
        if state.receipt == Some(entry.id) {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.label(faint(text));
            });
        }
    }
    if let Delivery::Failed(failure) = entry.delivery {
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if failure == Failure::Gone {
                if ui.small_button("Remove").clicked() {
                    actions.push(Action::Forget(entry.id));
                }
            } else if ui
                .small_button(format!("{} Retry", icons::ARROW_CLOCKWISE))
                .clicked()
            {
                actions.push(Action::Retry(entry.id));
            }
            ui.label(problem_text(ui, failure.text()).small());
        });
    }
    change_failure(ui, online, entry, actions);
    if let Some(note) = state.notes.get(&entry.id) {
        let layout = if entry.mine {
            Layout::right_to_left(Align::Min)
        } else {
            Layout::left_to_right(Align::Min)
        };
        ui.with_layout(layout, |ui| {
            ui.label(problem_text(ui, note).small());
        });
    }
    ui.add_space(4.0);
    bubble
}

fn deleted_ui(
    ui: &mut Ui,
    online: &Online,
    entry: &ChatEntry,
    clock: &Clock,
    actions: &mut Vec<Action>,
) -> Rect {
    let layout = if entry.mine {
        Layout::right_to_left(Align::Min)
    } else {
        Layout::left_to_right(Align::Min)
    };
    let bubble = ui
        .with_layout(layout, |ui| {
            egui::Frame::new()
                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                .corner_radius(10.0)
                .inner_margin(BUBBLE_MARGIN)
                .show(ui, |ui| {
                    let text_max =
                        ui.available_width() - stamp_width(ui, entry, clock) - TEXT_STAMP_GAP;
                    ui.spacing_mut().interact_size.y = 0.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = TEXT_STAMP_GAP;
                        ui.scope(|ui| {
                            ui.set_max_width(text_max.max(0.0));
                            ui.add(
                                Label::new(
                                    RichText::new(format!("{} Deleted", icons::PROHIBIT))
                                        .italics()
                                        .color(ui.visuals().weak_text_color()),
                                )
                                .truncate(),
                            );
                        });
                        stamp_label(ui, entry, clock, false);
                    });
                })
                .response
                .rect
        })
        .inner;
    change_failure(ui, online, entry, actions);
    ui.add_space(4.0);
    bubble
}

fn change_failure(ui: &mut Ui, online: &Online, entry: &ChatEntry, actions: &mut Vec<Action>) {
    let Some(failure) = online.change_failure(entry.id) else {
        return;
    };
    let what = if entry.is_deleted() {
        "Not deleted for them"
    } else {
        "Edit not sent"
    };
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        if ui
            .small_button(format!("{} Retry", icons::ARROW_CLOCKWISE))
            .clicked()
        {
            actions.push(Action::Retry(entry.id));
        }
        ui.label(problem_text(ui, what).small())
            .on_hover_text(failure.text());
    });
}

pub(super) fn paint_index(ui: &Ui) -> usize {
    ui.ctx().graphics(|layers| {
        layers
            .get(ui.layer_id())
            .map_or(0, |list| list.next_idx().0)
    })
}

pub(super) fn hug_words(ui: &Ui, first: usize, content: egui::Rect, mine: bool) {
    let layer = ui.layer_id();
    ui.ctx().graphics_mut(|layers| {
        let list = layers.entry(layer);
        let end = list.next_idx();
        let right = list
            .all_entries()
            .skip(first + 1)
            .map(|clipped| clipped.shape.visual_bounding_rect().right())
            .fold(f32::NEG_INFINITY, f32::max);
        let slack = content.right() - right;
        if !(0.5..content.width()).contains(&slack) {
            return;
        }
        list.mutate_shape(egui::layers::ShapeIdx(first), |clipped| {
            if let egui::Shape::Rect(bubble) = &mut clipped.shape {
                if mine {
                    bubble.rect.min.x += slack;
                } else {
                    bubble.rect.max.x -= slack;
                }
            }
        });
        if mine {
            for index in first + 1..end.0 {
                list.mutate_shape(egui::layers::ShapeIdx(index), |clipped| {
                    clipped.shape.translate(vec2(slack, 0.0));
                });
            }
        }
    });
}

pub(super) fn bubble_fill(visuals: &egui::Visuals, mine: bool) -> Color32 {
    if mine {
        visuals.selection.bg_fill
    } else {
        visuals.faint_bg_color
    }
}

pub(super) fn text_body(
    ui: &mut Ui,
    text: &str,
    big: Option<&BigRow>,
    entry: &ChatEntry,
    clock: &Clock,
) {
    let pieces = emoji::pieces(text);
    if let Some(big) = big {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = BIG_EMOJI_GAP;
            for piece in &pieces {
                if let emoji::Piece::Emoji(index) = piece {
                    emoji::image(ui, *index, big.size);
                }
            }
            if big.beside {
                ui.add_space(STAMP_GAP - BIG_EMOJI_GAP);
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), big.size),
                    Layout::bottom_up(Align::Max),
                    |ui| stamp_label(ui, entry, clock, false),
                );
            }
        });
        if !big.beside {
            let align = if entry.mine { Align::Max } else { Align::Min };
            ui.with_layout(Layout::top_down(align), |ui| {
                stamp_label(ui, entry, clock, false);
            });
        }
        return;
    }
    ui.spacing_mut().interact_size.y = 0.0;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for piece in &pieces {
            match piece {
                emoji::Piece::Text(part) => {
                    ui.label(RichText::new(*part).color(ui.visuals().strong_text_color()));
                }
                emoji::Piece::Emoji(index) => {
                    ui.add_space(1.0);
                    emoji::image(ui, *index, INLINE_EMOJI);
                    ui.add_space(1.0);
                }
            }
        }
        let stamp = stamp_width(ui, entry, clock);
        if ui.available_size_before_wrap().x >= TEXT_STAMP_GAP + stamp {
            ui.add_space(TEXT_STAMP_GAP);
        } else {
            let text = ui.min_rect().width();
            ui.end_row();
            ui.add_space((text - stamp).max(0.0));
        }
        stamp_label(ui, entry, clock, true);
    });
}

pub(super) fn save_button(ui: &mut Ui) -> egui::Response {
    let full = format!("{} Save to Media", icons::DOWNLOAD_SIMPLE);
    let padding = ui.spacing().button_padding.x * 2.0;
    if measure(ui, RichText::new(&full)) + padding <= ui.available_width() {
        return ui.add(egui::Button::new(full).wrap_mode(egui::TextWrapMode::Extend));
    }
    let short = format!("{} Save", icons::DOWNLOAD_SIMPLE);
    ui.add(egui::Button::new(short).wrap_mode(egui::TextWrapMode::Extend))
        .on_hover_text("Save to Media")
}

pub(super) fn row_with_stamp(
    ui: &mut Ui,
    entry: &ChatEntry,
    clock: &Clock,
    left: impl FnOnce(&mut Ui),
) {
    let width = ui.available_width();
    let stamp_room = stamp_width(ui, entry, clock);
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 0.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let start = ui.cursor().min.x;
            ui.scope(|ui| {
                ui.set_max_width((width - stamp_room - TEXT_STAMP_GAP).max(0.0));
                left(ui);
            });
            let used = ui.cursor().min.x - start;
            ui.add_space((width - used - stamp_room).max(TEXT_STAMP_GAP));
            stamp_label(ui, entry, clock, true);
        });
    });
}

pub(super) fn stamp(ui: &mut Ui, entry: &ChatEntry, clock: &Clock) {
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        stamp_label(ui, entry, clock, true);
    });
}

pub(super) fn measure(ui: &Ui, text: RichText) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x
}

pub(super) fn stamp_width(ui: &Ui, entry: &ChatEntry, clock: &Clock) -> f32 {
    measure(ui, RichText::new(stamp_text(entry, clock)).small())
}

pub(super) struct BigRow {
    size: f32,
    beside: bool,
    width: f32,
}

impl BigRow {
    pub(super) fn fit(count: usize, room: f32, stamp: f32) -> Self {
        let count = count as f32;
        let gaps = (count - 1.0) * BIG_EMOJI_GAP;
        let full = count * BIG_EMOJI + gaps;
        if full + STAMP_GAP + stamp + 1.0 <= room {
            return Self {
                size: BIG_EMOJI,
                beside: true,
                width: full + STAMP_GAP + stamp,
            };
        }
        let size = ((room - gaps - 1.0) / count)
            .floor()
            .clamp(INLINE_EMOJI, BIG_EMOJI);
        Self {
            size,
            beside: false,
            width: (count * size + gaps).max(stamp),
        }
    }
}

pub(super) fn text_width(
    ui: &Ui,
    text: &str,
    big: Option<&BigRow>,
    entry: &ChatEntry,
    clock: &Clock,
) -> f32 {
    if let Some(big) = big {
        return big.width + 1.0;
    }
    let stamp = stamp_width(ui, entry, clock);
    let body: f32 = emoji::pieces(text)
        .iter()
        .map(|piece| match piece {
            emoji::Piece::Text(part) => measure(ui, RichText::new(*part)),
            emoji::Piece::Emoji(_) => INLINE_EMOJI + 2.0,
        })
        .sum();
    body + TEXT_STAMP_GAP + stamp + 1.0
}

fn entry_mark(entry: &ChatEntry) -> Option<&'static str> {
    if entry.change.untold() {
        return Some(icons::CLOCK);
    }
    stamp_mark(entry.delivery)
}

pub(super) fn stamp_mark(delivery: Delivery) -> Option<&'static str> {
    match delivery {
        Delivery::Sending => Some(icons::CLOCK),
        Delivery::Sent => Some(icons::CHECK),
        Delivery::Delivered(_) | Delivery::Read(_) => Some(icons::CHECKS),
        Delivery::Failed(_) => Some(icons::WARNING),
        Delivery::Received | Delivery::Seen => None,
    }
}

pub(super) fn stamp_text(entry: &ChatEntry, clock: &Clock) -> String {
    let edited = if entry.is_edited() { EDITED } else { "" };
    match entry_mark(entry) {
        Some(mark) => format!("{edited}{} {mark}", clock.time(entry.at)),
        None => format!("{edited}{}", clock.time(entry.at)),
    }
}

const EDITED: &str = "edited ";

pub(super) fn stamp_label(ui: &mut Ui, entry: &ChatEntry, clock: &Clock, in_bubble: bool) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let base = if in_bubble && entry.mine {
        ui.visuals().strong_text_color().gamma_multiply(0.75)
    } else {
        ui.visuals().weak_text_color()
    };
    let mut job = egui::text::LayoutJob::default();
    let format = |color| egui::TextFormat::simple(font.clone(), color);
    if entry.is_edited() {
        job.append(EDITED, 0.0, format(base));
    }
    job.append(&clock.time(entry.at), 0.0, format(base));
    if let Some(mark) = entry_mark(entry) {
        let color = match entry.delivery {
            _ if entry.change.untold() => base,
            Delivery::Read(_) => read_color(ui.visuals()),
            Delivery::Sent | Delivery::Delivered(_) if in_bubble && entry.mine => {
                base.gamma_multiply(0.5)
            }
            _ => base,
        };
        job.append(&format!(" {mark}"), 0.0, format(color));
    }
    let label = ui.add(Label::new(job).extend());
    if let Delivery::Failed(failure) = entry.delivery {
        label.on_hover_text(failure.text());
    }
}

pub(super) fn read_color(visuals: &egui::Visuals) -> Color32 {
    visuals.selection.stroke.color
}

pub(super) fn receipt_text(delivery: Delivery, clock: &Clock) -> Option<String> {
    let when = |at: u64| match clock.day(at) {
        Some(day) if day != clock.today => clock
            .zoned(at)
            .map(|z| z.strftime("%-d %b, %H:%M").to_string())
            .unwrap_or_default(),
        _ => clock.time(at),
    };
    match delivery {
        Delivery::Sending => Some("Sending".to_owned()),
        Delivery::Sent => Some("Sent".to_owned()),
        Delivery::Delivered(at) => Some(format!("Delivered {}", when(at))),
        Delivery::Read(at) => Some(format!("Read {}", when(at))),
        Delivery::Failed(_) | Delivery::Received | Delivery::Seen => None,
    }
}

pub(super) fn long_pressed(input: &egui::InputState, rect: Rect) -> bool {
    input.any_touches()
        && input.pointer.primary_down()
        && input
            .pointer
            .press_origin()
            .is_some_and(|at| rect.contains(at))
        && input
            .pointer
            .total_drag_delta()
            .is_none_or(|moved| moved.length() < LONG_PRESS_SLOP)
        && input
            .pointer
            .press_start_time()
            .is_some_and(|start| input.time - start >= LONG_PRESS_SECS)
}
