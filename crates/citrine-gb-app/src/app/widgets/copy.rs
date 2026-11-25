use crate::app::big_screen::ring;
use crate::icons;
use egui::emath::TSTransform;
use egui::{
    Align2, FontId, Frame, Rect, RectAlign, Response, RichText, Sense, TextStyle, TextWrapMode,
    Tooltip, Ui, WidgetInfo, WidgetText, WidgetType, vec2,
};
use std::time::Duration;
use web_time::Instant;

pub const COPIED_FOR: Duration = Duration::from_secs(2);

pub fn copy_button(ui: &mut Ui, text: &str, copied_at: Option<Instant>, now: Instant) -> bool {
    let copied = recently(copied_at, now);
    let (icon, tip) = if copied {
        let accent = crate::app::ui_state::ui_theme::brand_mark(ui.visuals());
        (RichText::new(icons::CHECK_FAT).color(accent), "Copied")
    } else {
        (RichText::new(icons::COPY), "Copy")
    };
    let response = ui.button(icon);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tip));
    let clicked = response.clicked();
    let quiet_id = response.id.with("tip_quiet");
    let resting = response.hovered() || response.has_focus();
    let quiet = clicked || (resting && ui.data(|d| d.get_temp::<bool>(quiet_id)).unwrap_or(false));
    ui.data_mut(|d| d.insert_temp(quiet_id, quiet));
    if !quiet {
        let tooltip = if response.has_focus() && !response.hovered() {
            Tooltip::for_widget(&response)
        } else {
            Tooltip::for_enabled(&response)
        };
        beside(ui, tooltip, &response, tip).show(|ui| ui.label(tip));
    }
    if clicked {
        ui.ctx().copy_text(text.to_owned());
    }
    clicked
}

pub fn copy_icon(ui: &mut Ui, text: &str, copied_at: Option<Instant>, now: Instant) -> bool {
    let copied = recently(copied_at, now);
    let (glyph, name) = if copied {
        (icons::CHECK_FAT, "Copied")
    } else {
        (icons::COPY, "Copy")
    };
    let line = ui.text_style_height(&TextStyle::Body);
    let (slot, placed) = ui.allocate_exact_size(vec2(line, line), Sense::hover());
    let touch = crate::app::mobile::touch_square(ui).max(line);
    let rect = Rect::from_center_size(slot.center(), vec2(touch, touch));
    let response = ui.interact(rect, placed.id.with("copy"), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    ring::mark(
        &response,
        ring::Mark::Ring {
            rect: slot,
            radius: line / 2.0,
        },
    );
    let color = if copied {
        crate::app::ui_state::ui_theme::brand_mark(ui.visuals())
    } else {
        ui.style().interact(&response).fg_stroke.color
    };
    ui.painter().text(
        slot.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(line),
        color,
    );
    let clicked = response.on_hover_text(name).clicked();
    if clicked {
        ui.ctx().copy_text(text.to_owned());
    }
    clicked
}

const TIP_GAP: f32 = 4.0;
const TIP_SIDES: [RectAlign; 4] = [
    RectAlign::LEFT,
    RectAlign::RIGHT,
    RectAlign::TOP,
    RectAlign::BOTTOM,
];

fn beside<'a>(ui: &Ui, mut tooltip: Tooltip<'a>, button: &Response, tip: &str) -> Tooltip<'a> {
    let ctx = &button.ctx;
    let to_global = ctx
        .layer_transform_to_global(button.layer_id)
        .unwrap_or_default();
    let anchor = to_global * button.interact_rect;
    let text = WidgetText::from(tip)
        .into_galley(
            ui,
            Some(TextWrapMode::Extend),
            f32::INFINITY,
            TextStyle::Body,
        )
        .size();
    let size = (text + Frame::popup(ui.style()).total_margin().sum()).ceil() + vec2(1.0, 0.0);
    let side = TIP_SIDES
        .into_iter()
        .map(|side| (side, side.align_rect(&anchor, size, TIP_GAP)))
        .filter(|(_, tip)| ctx.content_rect().contains_rect(*tip))
        .map(|(side, tip)| (side, covered_area(button, anchor, to_global, tip)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(RectAlign::LEFT, |(side, _)| side);
    tooltip.popup = tooltip
        .popup
        .gap(TIP_GAP)
        .width(size.x)
        .align(side)
        .align_alternatives(&TIP_SIDES);
    tooltip
}

fn covered_area(button: &Response, anchor: Rect, to_global: TSTransform, tip: Rect) -> f32 {
    button.ctx.viewport(|viewport| {
        viewport
            .prev_pass
            .widgets
            .get_layer(button.layer_id)
            .filter(|widget| widget.id != button.id && widget.enabled)
            .filter(|widget| widget.sense.interactive() && widget.sense.is_focusable())
            .map(|widget| to_global * widget.interact_rect)
            .filter(|rect| !rect.contains_rect(anchor))
            .map(|rect| rect.intersect(tip))
            .filter(|cover| cover.is_positive())
            .map(|cover| cover.area())
            .sum()
    })
}

pub fn recently(copied_at: Option<Instant>, now: Instant) -> bool {
    copied_at.is_some_and(|at| now.duration_since(at) < COPIED_FOR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{
        Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape, pos2, vec2,
    };

    fn row(ctx: &Context, events: Vec<Event>) -> (Rect, bool) {
        let mut clicked = false;
        let mut rect = Rect::NOTHING;
        let now = Instant::now();
        let input = RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            ctx.style_mut(|style| style.spacing.interact_size.y = 44.0);
            egui::CentralPanel::default().show(ctx, |ui| {
                let line = ui.text_style_height(&TextStyle::Body);
                let row = egui::Layout::left_to_right(egui::Align::Center);
                let inner = ui.allocate_ui_with_layout(vec2(300.0, line), row, |ui| {
                    ui.label("0000-0001");
                    clicked = copy_icon(ui, "0000-0001", None, now);
                });
                rect = inner.response.rect;
            });
        });
        (rect, clicked)
    }

    #[test]
    fn the_copy_icon_keeps_its_row_one_line_high_with_a_touch_sized_tap() {
        let ctx = Context::default();
        row(&ctx, Vec::new());
        let (rect, _) = row(&ctx, Vec::new());
        let line = ctx.style().text_styles[&TextStyle::Body].size * 1.5;
        assert!(rect.height() < line, "the row is {} high", rect.height());
        let below = Pos2::new(rect.right() - 4.0, rect.bottom() + 12.0);
        let press = |pressed| Event::PointerButton {
            pos: below,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        row(&ctx, vec![Event::PointerMoved(below)]);
        row(&ctx, vec![press(true)]);
        let (_, clicked) = row(&ctx, vec![press(false)]);
        assert!(clicked, "a tap 12 pt under the row lands on the copy icon");
    }

    fn tip_after_keys(keys: &[Option<Key>]) -> Option<String> {
        let ctx = egui::Context::default();
        let mut copied_at = None;
        let mut tip = None;
        for key in keys {
            let now = Instant::now();
            let mut input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 400.0))),
                ..Default::default()
            };
            if let Some(key) = key {
                input.events.push(Event::Key {
                    key: *key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                });
            }
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(100.0);
                    if copy_button(ui, "cgb0-1234", copied_at, now) {
                        copied_at = Some(now);
                    }
                    ui.text_edit_singleline(&mut String::from("Clara"));
                });
            });
            tip = output
                .shapes
                .into_iter()
                .find_map(|clipped| match clipped.shape {
                    Shape::Text(text) if ["Copy", "Copied"].contains(&text.galley.text()) => {
                        Some(text.galley.text().to_owned())
                    }
                    _ => None,
                });
        }
        tip
    }

    #[test]
    fn the_copy_tip_goes_once_the_check_shows() {
        let tab = Some(Key::Tab);
        assert_eq!(tip_after_keys(&[None, tab, None]).as_deref(), Some("Copy"));
        assert_eq!(
            tip_after_keys(&[None, tab, None, Some(Key::Enter), None, None]),
            None
        );
    }

    #[test]
    fn the_focus_tip_of_a_copy_button_leaves_the_field_below_free() {
        let ctx = egui::Context::default();
        let now = Instant::now();
        let mut field = Rect::NOTHING;
        let mut tip = None;
        for pass in 0..4 {
            let mut input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 400.0))),
                ..Default::default()
            };
            if pass == 1 {
                input.events.push(Event::Key {
                    key: Key::Tab,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                });
            }
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(100.0);
                    copy_button(ui, "cgb0-1234", None, now);
                    field = ui.text_edit_singleline(&mut String::from("Clara")).rect;
                });
            });
            tip = output
                .shapes
                .into_iter()
                .find_map(|clipped| match clipped.shape {
                    Shape::Text(text) if text.galley.text() == "Copy" => {
                        Some(text.galley.rect.translate(text.pos.to_vec2()))
                    }
                    _ => None,
                });
        }
        let tip = tip.expect("no copy tip on keyboard focus");
        assert!(
            !tip.intersects(field),
            "tip {tip:?} covers the field {field:?}"
        );
    }

    struct Rows {
        copies: Vec<Rect>,
        marks: Vec<Rect>,
        field: Rect,
    }

    fn hovered_tip(column: f32) -> (Rows, Rect) {
        let ctx = egui::Context::default();
        let now = Instant::now();
        let mut rows = Rows {
            copies: Vec::new(),
            marks: Vec::new(),
            field: Rect::NOTHING,
        };
        let mut tip = None;
        for pass in 0..6 {
            let mut input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 400.0))),
                time: Some(pass as f64),
                ..Default::default()
            };
            if let Some(lower) = rows.copies.get(1).filter(|_| pass == 1) {
                input.events.push(Event::PointerMoved(lower.center()));
            }
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(100.0);
                    rows.copies.clear();
                    rows.marks.clear();
                    for (label, value) in
                        [("Relay ID", "cgb1-119y1j31"), ("Online ID", "0093-8NKR")]
                    {
                        ui.horizontal(|ui| {
                            let left = ui.max_rect().left();
                            ui.label(label);
                            rows.marks.push(ui.small_button("i").rect);
                            ui.add_space((left + column - ui.cursor().left()).max(0.0));
                            let start = ui.cursor().left();
                            copy_button(ui, value, None, now);
                            let end = ui.cursor().left() - ui.spacing().item_spacing.x;
                            let row = ui.min_rect();
                            rows.copies
                                .push(Rect::from_x_y_ranges(start..=end, row.y_range()));
                            ui.monospace(value);
                        });
                    }
                    rows.field = ui.text_edit_singleline(&mut String::from("Clara")).rect;
                });
            });
            tip = output
                .shapes
                .into_iter()
                .find_map(|clipped| match clipped.shape {
                    Shape::Text(text) if text.galley.text() == "Copy" => {
                        Some(text.galley.rect.translate(text.pos.to_vec2()))
                    }
                    _ => None,
                });
        }
        (rows, tip.expect("no copy tip on hover"))
    }

    #[test]
    fn the_hover_tip_of_a_copy_button_covers_no_other_control() {
        for column in [200.0, 90.0] {
            let (rows, tip) = hovered_tip(column);
            let others = [rows.copies[0], rows.marks[0], rows.marks[1], rows.field];
            for other in others {
                assert!(
                    !tip.intersects(other),
                    "with the column at {column} the tip {tip:?} covers {other:?}"
                );
            }
            if column == 200.0 {
                assert!(
                    tip.right() <= rows.copies[1].left(),
                    "with room the tip {tip:?} opens left of the button {:?}",
                    rows.copies[1]
                );
            }
        }
    }
}
