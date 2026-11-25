use super::kit;
use crate::app::big_screen::scroll::Following;
use egui::{
    Context, CornerRadius, Frame, Id, Margin, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2,
};

const CLOSE_DRAG: f32 = 80.0;
const MAX_WIDTH: f32 = 560.0;
const TALLEST: f32 = 0.85;
const LINE_GAP: f32 = 12.0;
const HEAD_GAP: f32 = 20.0;
pub const INSET: f32 = 12.0;
pub const BAR_EDGE: f32 = 2.0;
pub const SLIDER_KNOB: f32 = 13.0;
const TITLE_GAP: f32 = 8.0;
const SIDE_SHARE: f32 = 0.5;
const SIDE_MIN: f32 = 300.0;
const SIDE_MAX: f32 = 460.0;
const AREA: &str = "sheet_area";
const ABOVE: &str = "sheet_above";
const LIFT: &str = "sheet_lift";

pub struct SheetResponse<R> {
    pub inner: R,
    pub closed: bool,
    pub outside: bool,
}

#[derive(Clone, Copy, Default)]
struct Measure {
    height: f32,
    drag: f32,
    scrolled: bool,
}

pub fn set_area(ctx: &Context, area: Option<Rect>) {
    ctx.data_mut(|d| match area {
        Some(area) => d.insert_temp(Id::new(AREA), area),
        None => d.remove::<Rect>(Id::new(AREA)),
    });
}

pub fn area(ctx: &Context) -> Option<Rect> {
    ctx.data(|d| d.get_temp::<Rect>(Id::new(AREA)))
}

pub fn above(ctx: &Context) -> Option<Rect> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<(u64, Rect, bool)>(Id::new(ABOVE)))
        .and_then(|(shown, rect, _)| (pass.saturating_sub(shown) <= 1).then_some(rect))
}

pub const KEEP_GAP: f32 = kit::SIDE;

#[derive(Clone, Copy)]
struct Lift {
    pass: u64,
    sheet_top: f32,
    delta: f32,
}

pub fn keep_above(ui: &Ui, rect: Rect) {
    let pass = ui.ctx().cumulative_pass_nr();
    let Some(above) = ui
        .ctx()
        .data(|d| d.get_temp::<(u64, Rect, bool)>(Id::new(ABOVE)))
        .filter(|(shown, _, sized)| *sized && pass.saturating_sub(*shown) <= 1)
        .map(|(_, above, _)| above)
    else {
        return;
    };
    let top = ui.clip_rect().top().max(above.top()) + KEEP_GAP;
    let bottom = above.bottom() - KEEP_GAP;
    let delta = if rect.bottom() > bottom {
        bottom - rect.bottom()
    } else if rect.top() < top {
        top - rect.top()
    } else {
        0.0
    };
    let lift = Lift {
        pass,
        sheet_top: above.bottom(),
        delta,
    };
    ui.ctx().data_mut(|d| d.insert_temp(Id::new(LIFT), lift));
}

pub fn lift(ui: &mut Ui) {
    let pass = ui.ctx().cumulative_pass_nr();
    let Some(lift) = ui
        .ctx()
        .data(|d| d.get_temp::<Lift>(Id::new(LIFT)))
        .filter(|lift| pass.saturating_sub(lift.pass) <= 1)
    else {
        return;
    };
    let room = ui.clip_rect().bottom() - lift.sheet_top + KEEP_GAP;
    if room > 0.0 {
        ui.add_space(room);
    }
    if lift.pass == pass && lift.delta != 0.0 {
        ui.scroll_with_delta(vec2(0.0, lift.delta));
    }
}

pub fn show<R>(
    ctx: &Context,
    id: Id,
    title: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> SheetResponse<R> {
    show_with_info(ctx, id, title, None, body)
}

pub fn show_with_info<R>(
    ctx: &Context,
    id: Id,
    title: &str,
    info: Option<&str>,
    body: impl FnOnce(&mut Ui) -> R,
) -> SheetResponse<R> {
    show_headed(
        ctx,
        id,
        |ui, _| {
            if !title.is_empty() {
                let edge = ui.max_rect().top();
                inset(ui, |ui| {
                    let label =
                        egui::Label::new(RichText::new(title).size(18.0).strong()).truncate();
                    match info {
                        Some(info) => {
                            ui.horizontal(|ui| {
                                ui.add(label);
                                crate::app::widgets::info_mark::info_mark_above(ui, info, edge);
                            });
                        }
                        None => {
                            ui.add(label);
                        }
                    }
                });
                ui.add_space(TITLE_GAP);
            }
            None
        },
        body,
    )
}

pub fn dialog<R>(ctx: &Context, id: Id, body: impl FnOnce(&mut Ui) -> R) -> SheetResponse<R> {
    let screen = ctx.content_rect();
    show_scrimmed(ctx, id, |_, _| None, body, screen)
}

pub fn show_headed<R>(
    ctx: &Context,
    id: Id,
    header: impl FnOnce(&mut Ui, bool) -> Option<f32>,
    body: impl FnOnce(&mut Ui) -> R,
) -> SheetResponse<R> {
    let column = area(ctx).unwrap_or(ctx.content_rect());
    show_scrimmed(ctx, id, header, body, column)
}

fn show_scrimmed<R>(
    ctx: &Context,
    id: Id,
    header: impl FnOnce(&mut Ui, bool) -> Option<f32>,
    body: impl FnOnce(&mut Ui) -> R,
    column: Rect,
) -> SheetResponse<R> {
    let mut measure = ctx.data(|d| d.get_temp::<Measure>(id)).unwrap_or_default();
    let sized = measure.height > 0.0;
    let most = column.height() * TALLEST;
    let height = if measure.height > 0.0 {
        measure.height.min(most)
    } else {
        most
    };
    let width = column.width().min(MAX_WIDTH);
    let rect = Rect::from_min_size(
        pos2(
            column.center().x - width / 2.0,
            column.bottom() - height + measure.drag,
        ),
        vec2(width, most),
    );
    let mut dragged_off = false;
    let modal = egui::Modal::new(id)
        .area(
            egui::Area::new(id)
                .kind(egui::UiKind::Modal)
                .sense(Sense::hover())
                .order(egui::Order::Foreground)
                .interactable(true)
                .constrain(false)
                .fixed_pos(rect.min),
        )
        .frame(Frame::NONE)
        .backdrop_color(egui::Color32::TRANSPARENT)
        .show(ctx, |ui| {
            let (behind, dim) = if crate::app::big_screen::hint_room(ui.ctx()) > 0.0 {
                (
                    Rect::from_min_max(
                        pos2(column.left(), ui.ctx().content_rect().top()),
                        column.max,
                    ),
                    crate::app::big_screen::dim(ui.visuals()),
                )
            } else {
                (column, kit::scrim(ui.visuals()))
            };
            crate::app::widgets::confirm::dim(ui, behind, dim);
            let fill = kit::card_fill(ui.visuals());
            let shown = ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                Frame::new()
                    .fill(fill)
                    .corner_radius(CornerRadius {
                        nw: 20,
                        ne: 20,
                        sw: 0,
                        se: 0,
                    })
                    .inner_margin(Margin {
                        left: kit::SIDE as i8,
                        right: kit::SIDE as i8,
                        top: 0,
                        bottom: kit::SIDE as i8,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let handle = grab_handle(ui);
                        if handle.dragged() {
                            measure.drag = (measure.drag + handle.drag_delta().y).max(0.0);
                        } else if handle.drag_stopped() {
                            dragged_off = measure.drag > CLOSE_DRAG;
                            measure.drag = 0.0;
                        }
                        let ink = header(ui, measure.scrolled);
                        let top = match (ink, measure.scrolled) {
                            (Some(ink), _) => Some(ink),
                            (None, true) => Some(ui.min_rect().bottom()),
                            (None, false) => None,
                        };
                        if let Some(top) = top {
                            ui.add_space(LINE_GAP - (ui.cursor().top() - top));
                        }
                        let lead = if ink.is_some() {
                            HEAD_GAP - LINE_GAP
                        } else {
                            0.0
                        };
                        let room = most - ui.min_rect().height() - kit::SIDE;
                        let margin = ui.visuals().clip_rect_margin;
                        ui.visuals_mut().clip_rect_margin = 0.0;
                        let scrolled = egui::ScrollArea::vertical()
                            .id_salt(id.with("scroll"))
                            .max_height(room.max(80.0))
                            .auto_shrink([false, true])
                            .show_following(ui, |ui| {
                                ui.visuals_mut().clip_rect_margin = margin;
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.add_space(lead);
                                body(ui)
                            });
                        ui.visuals_mut().clip_rect_margin = margin;
                        let now = scrolled.state.offset.y > 0.5;
                        if now != measure.scrolled {
                            measure.scrolled = now;
                            ui.ctx().request_repaint();
                        }
                        let top = scrolled.inner_rect;
                        let overflows = scrolled.content_size.y > top.height() + 0.5;
                        if overflows || now {
                            ui.painter().hline(
                                top.x_range(),
                                top.top(),
                                kit::hairline(ui.visuals()),
                            );
                        }
                        scrolled.inner
                    })
            });
            let frame = shown.inner;
            (frame.inner, frame.response.rect.height())
        });
    let outside = modal.backdrop_response.clicked() || dragged_off;
    let closed = modal.should_close() || dragged_off;
    let (inner, measured) = modal.inner;
    if (measured - measure.height).abs() > 0.5 {
        measure.height = measured;
        ctx.request_repaint();
    }
    if closed {
        ctx.data_mut(|d| d.remove::<Measure>(id));
    } else {
        ctx.data_mut(|d| d.insert_temp(id, measure));
        let above = Rect::from_min_max(column.min, pos2(column.max.x, rect.top()));
        let pass = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(Id::new(ABOVE), (pass, above, sized)));
    }
    SheetResponse {
        inner,
        closed,
        outside,
    }
}

pub fn side_rect(ctx: &Context) -> Rect {
    let column = area(ctx).unwrap_or(ctx.content_rect());
    let width = (column.width() * SIDE_SHARE)
        .clamp(SIDE_MIN, SIDE_MAX)
        .min(column.width());
    Rect::from_min_max(pos2(column.right() - width, column.top()), column.max)
}

pub fn side<R>(ctx: &Context, id: Id, body: impl FnOnce(&mut Ui) -> R) -> SheetResponse<R> {
    let column = area(ctx).unwrap_or(ctx.content_rect());
    let rect = side_rect(ctx);
    let modal = egui::Modal::new(id)
        .area(
            egui::Area::new(id)
                .kind(egui::UiKind::Modal)
                .sense(Sense::hover())
                .order(egui::Order::Foreground)
                .interactable(true)
                .constrain(false)
                .fixed_pos(rect.min),
        )
        .frame(Frame::NONE)
        .backdrop_color(egui::Color32::TRANSPARENT)
        .show(ctx, |ui| {
            let scrim = Rect::from_min_max(column.min, pos2(rect.left(), column.bottom()));
            ui.ctx().layer_painter(ui.layer_id()).rect_filled(
                scrim,
                CornerRadius::ZERO,
                kit::scrim(ui.visuals()),
            );
            let fill = kit::card_fill(ui.visuals());
            ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                Frame::new()
                    .fill(fill)
                    .inner_margin(bar_margin(kit::SIDE, kit::SIDE))
                    .show(ui, |ui| {
                        ui.set_min_size(rect.size() - vec2(kit::SIDE + BAR_EDGE, 2.0 * kit::SIDE));
                        ui.visuals_mut().clip_rect_margin = 0.0;
                        egui::ScrollArea::vertical()
                            .id_salt(id.with("scroll"))
                            .auto_shrink([false, false])
                            .show_following(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                gutter(ui, kit::SIDE, body)
                            })
                            .inner
                    })
                    .inner
            })
            .inner
        });
    SheetResponse {
        outside: modal.backdrop_response.clicked(),
        closed: modal.should_close(),
        inner: modal.inner,
    }
}

fn grab_handle(ui: &mut Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::DRAG);
    let bar = Rect::from_center_size(rect.center(), vec2(36.0, 4.0));
    ui.painter().rect_filled(
        bar,
        CornerRadius::same(2),
        ui.visuals().weak_text_color().gamma_multiply(0.6),
    );
    response
}

pub fn choices<T: Copy + PartialEq>(
    ui: &mut Ui,
    options: impl IntoIterator<Item = T>,
    current: T,
    label: impl Fn(T) -> String,
) -> Option<T> {
    let options: Vec<T> = options.into_iter().collect();
    let names: Vec<String> = options.iter().map(|option| label(*option)).collect();
    let rows = options
        .iter()
        .zip(&names)
        .map(|(option, name)| kit::Row::new(name).checked(*option == current))
        .collect();
    kit::rows(ui, rows).map(|index| options[index])
}

pub struct Amount<'a, T: egui::emath::Numeric> {
    pub value: &'a mut T,
    pub range: std::ops::RangeInclusive<T>,
    pub step: f64,
    pub default: T,
    pub text: fn(f64) -> String,
}

pub fn amount<T: egui::emath::Numeric>(ui: &mut Ui, amount: Amount<'_, T>) -> egui::Response {
    let Amount {
        value,
        range,
        step,
        default,
        text,
    } = amount;
    ui.add_space(4.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(text(value.to_f64())).size(28.0).strong());
    });
    ui.add_space(10.0);
    let mut response = ui
        .horizontal(|ui| {
            ui.add_space(INSET);
            let width = ui.available_width() - INSET;
            slider(ui, value, range, step, width)
        })
        .inner;
    ui.add_space(14.0);
    let is_default = (value.to_f64() - default.to_f64()).abs() < step / 2.0;
    ui.add_enabled_ui(!is_default, |ui| {
        ui.vertical_centered(|ui| {
            if kit::pill(ui, "Reset", false).clicked() {
                *value = default;
                response.mark_changed();
            }
        });
    });
    response
}

pub fn slider<T: egui::emath::Numeric>(
    ui: &mut Ui,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    step: f64,
    width: f32,
) -> egui::Response {
    slider_with_knob(ui, value, range, step, width, SLIDER_KNOB)
}

pub(super) fn slider_with_knob<T: egui::emath::Numeric>(
    ui: &mut Ui,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    step: f64,
    width: f32,
    knob: f32,
) -> egui::Response {
    let height = ui.spacing().interact_size.y.max(2.0 * knob + 8.0);
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(width.max(4.0 * knob), height), Sense::click_and_drag());
    let low = range.start().to_f64();
    let high = range.end().to_f64();
    let rail = rect.shrink2(vec2(knob, 0.0));
    crate::app::big_screen::offer_steps(ui.ctx(), &response);
    if let Some(dir) = crate::app::big_screen::take_step(&response)
        && high > low
    {
        let unit = if step > 0.0 {
            step
        } else {
            (high - low) / 20.0
        };
        let mut picked = value.to_f64() + f64::from(dir) * unit;
        if step > 0.0 {
            picked = low + ((picked - low) / step).round() * step;
        }
        let picked = picked.clamp(low, high);
        if (picked - value.to_f64()).abs() > f64::EPSILON {
            *value = T::from_f64(picked);
            response.mark_changed();
        }
    }
    if let Some(pointer) = response.interact_pointer_pos()
        && high > low
    {
        let t = ((pointer.x - rail.left()) / rail.width()).clamp(0.0, 1.0) as f64;
        let mut picked = low + t * (high - low);
        if step > 0.0 {
            picked = low + ((picked - low) / step).round() * step;
        }
        let picked = picked.clamp(low, high);
        if (picked - value.to_f64()).abs() > f64::EPSILON {
            *value = T::from_f64(picked);
            response.mark_changed();
        }
    }
    response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), value.to_f64(), ""));
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let t = if high > low {
            ((value.to_f64() - low) / (high - low)).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let x = egui::lerp(rail.left()..=rail.right(), t);
        let y = rect.center().y;
        let track = Rect::from_min_max(pos2(rect.left(), y - 3.0), pos2(rect.right(), y + 3.0));
        let painter = ui.painter();
        painter.rect_filled(track, CornerRadius::same(3), track_fill(visuals));
        painter.rect_filled(
            Rect::from_min_max(track.min, pos2(x, track.max.y)),
            CornerRadius::same(3),
            crate::app::ui_state::ui_theme::brand(visuals),
        );
        for (grow, alpha) in [(3.0, 10), (2.0, 18), (1.0, 30)] {
            painter.circle_filled(
                pos2(x, y),
                knob + grow,
                egui::Color32::from_black_alpha(alpha),
            );
        }
        painter.circle(
            pos2(x, y),
            knob,
            egui::Color32::WHITE,
            egui::Stroke::new(2.0_f32, crate::app::ui_state::ui_theme::brand(visuals)),
        );
    }
    response
}

pub fn bar_margin(side: f32, vertical: f32) -> Margin {
    Margin {
        left: side as i8,
        right: BAR_EDGE as i8,
        top: vertical as i8,
        bottom: vertical as i8,
    }
}

pub fn gutter<R>(ui: &mut Ui, side: f32, body: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::NONE
        .inner_margin(Margin {
            right: (side - BAR_EDGE) as i8,
            ..Margin::ZERO
        })
        .show(ui, body)
        .inner
}

pub fn track_fill(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        egui::Color32::from_white_alpha(72)
    } else {
        egui::Color32::from_black_alpha(80)
    }
}

pub fn inset<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::NONE
        .inner_margin(Margin::symmetric(INSET as i8, 0))
        .show(ui, body)
        .inner
}

pub fn note(ui: &mut Ui, text: &str) {
    inset(ui, |ui| {
        ui.add(egui::Label::new(RichText::new(text).size(14.0).weak()).wrap());
    });
    ui.add_space(16.0);
}

pub fn label(ui: &mut Ui, text: &str) {
    inset(ui, |ui| {
        ui.label(
            RichText::new(text)
                .size(13.0)
                .color(super::kit::section_color(ui.visuals())),
        );
    });
    ui.add_space(6.0);
}

pub fn percent(value: f64) -> String {
    format!("{:.0}%", value * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_slider_track_shows_on_every_ground() {
        use crate::app::ui_state::ui_theme::{UiTheme, contrast};
        use strum::IntoEnumIterator;
        for theme in UiTheme::iter().filter(|theme| *theme != UiTheme::System) {
            let ctx = Context::default();
            theme.apply(&ctx);
            let visuals = ctx.style().visuals.clone();
            for ground in [
                kit::card_fill(&visuals),
                visuals.window_fill,
                visuals.panel_fill,
            ] {
                let track = ground.blend(track_fill(&visuals));
                assert!(
                    contrast(track, ground) >= 2.0,
                    "{theme:?}: track {track:?} on {ground:?}"
                );
            }
        }
    }

    #[test]
    fn a_sheet_ends_on_the_top_of_its_area() {
        let ctx = Context::default();
        let area = Rect::from_min_max(pos2(0.0, 0.0), pos2(412.0, 800.0));
        let mut bottom = 0.0_f32;
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(412.0, 860.0))),
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                set_area(ctx, Some(area));
                show(ctx, Id::new("test_sheet"), "Room chat", |ui| {
                    ui.set_height(1000.0);
                });
            });
            bottom = output
                .shapes
                .iter()
                .map(|clipped| {
                    clipped
                        .shape
                        .visual_bounding_rect()
                        .intersect(clipped.clip_rect)
                })
                .filter(|rect| rect.is_positive())
                .map(|rect| rect.bottom())
                .fold(0.0, f32::max);
        }
        assert!(bottom <= area.bottom() + 0.5, "the sheet reaches {bottom}");
    }

    #[test]
    fn the_line_under_a_header_keeps_its_place_and_room_below() {
        let ctx = Context::default();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(412.0, 860.0));
        let mut seen = Vec::new();
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let (mut ink, mut first) = (0.0, 0.0);
            let output = ctx.run(input, |ctx| {
                show_headed(
                    ctx,
                    Id::new("test_sheet"),
                    |ui, _| {
                        ink = ui.label("In the Game Archive").rect.bottom();
                        ui.add_space(30.0);
                        Some(ink)
                    },
                    |ui| {
                        first = ui.cursor().top();
                        ui.set_height(2000.0);
                    },
                );
            });
            let line = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::LineSegment { points, .. }
                        if points[0].y == points[1].y
                            && points[0].y > ink
                            && points[0].y < first =>
                    {
                        Some(points[0].y)
                    }
                    _ => None,
                });
            seen.push((line.map(|y| y - ink), first - ink));
        }
        let (line, first) = *seen.last().unwrap();
        assert!(
            line.is_some_and(|line| (line - LINE_GAP).abs() < 0.5),
            "{seen:?}"
        );
        assert!((first - HEAD_GAP).abs() < 0.5, "{seen:?}");
    }

    fn frame(ctx: &Context, events: Vec<egui::Event>) -> (bool, bool) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 800.0))),
            events,
            ..Default::default()
        };
        let mut closed = false;
        let mut under = false;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                under = ui
                    .allocate_response(ui.available_size(), Sense::click())
                    .clicked();
            });
            closed = show(ctx, Id::new("test_sheet"), "Sort by", |ui| {
                ui.label("one");
                ui.label("two");
            })
            .closed;
        });
        (closed, under)
    }

    fn tap(ctx: &Context, at: egui::Pos2) -> (bool, bool) {
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        [
            frame(ctx, vec![egui::Event::PointerMoved(at), press(true)]),
            frame(ctx, vec![press(false)]),
            frame(ctx, vec![egui::Event::PointerGone]),
        ]
        .into_iter()
        .fold((false, false), |seen, now| {
            (seen.0 || now.0, seen.1 || now.1)
        })
    }

    fn settled() -> Context {
        let ctx = Context::default();
        for _ in 0..4 {
            assert_eq!(frame(&ctx, Vec::new()), (false, false));
        }
        ctx
    }

    #[test]
    fn a_tap_above_the_sheet_closes_it() {
        let ctx = settled();
        assert_eq!(tap(&ctx, pos2(200.0, 100.0)), (true, false));
    }

    #[test]
    fn a_tap_inside_the_sheet_keeps_it_open() {
        let ctx = settled();
        assert_eq!(tap(&ctx, pos2(200.0, 790.0)), (false, false));
    }
}
