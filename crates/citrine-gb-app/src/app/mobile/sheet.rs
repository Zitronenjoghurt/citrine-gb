use super::kit;
use egui::{
    Context, CornerRadius, Frame, Id, Margin, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2,
};

const CLOSE_DRAG: f32 = 80.0;
const MAX_WIDTH: f32 = 560.0;
const TALLEST: f32 = 0.85;
pub const INSET: f32 = 12.0;
const AREA: &str = "sheet_area";

pub struct SheetResponse<R> {
    pub inner: R,
    pub closed: bool,
}

#[derive(Clone, Copy, Default)]
struct Measure {
    height: f32,
    drag: f32,
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

pub fn show<R>(
    ctx: &Context,
    id: Id,
    title: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> SheetResponse<R> {
    let column = area(ctx).unwrap_or(ctx.content_rect());
    let mut measure = ctx.data(|d| d.get_temp::<Measure>(id)).unwrap_or_default();
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
            ui.ctx().layer_painter(ui.layer_id()).rect_filled(
                column,
                CornerRadius::ZERO,
                kit::scrim(ui.visuals()),
            );
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
                        if !title.is_empty() {
                            inset(ui, |ui| {
                                ui.add(
                                    egui::Label::new(RichText::new(title).size(18.0).strong())
                                        .truncate(),
                                );
                            });
                            ui.add_space(2.0);
                        }
                        let room = most - ui.min_rect().height() - kit::SIDE;
                        egui::ScrollArea::vertical()
                            .id_salt(id.with("scroll"))
                            .max_height(room.max(80.0))
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                body(ui)
                            })
                            .inner
                    })
            });
            let frame = shown.inner;
            (frame.inner, frame.response.rect.height())
        });
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
    }
    SheetResponse { inner, closed }
}

fn grab_handle(ui: &mut Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::drag());
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

fn slider<T: egui::emath::Numeric>(
    ui: &mut Ui,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    step: f64,
    width: f32,
) -> egui::Response {
    let knob = 13.0;
    let height = ui.spacing().interact_size.y.max(2.0 * knob + 8.0);
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(width.max(4.0 * knob), height), Sense::click_and_drag());
    let low = range.start().to_f64();
    let high = range.end().to_f64();
    let rail = rect.shrink2(vec2(knob, 0.0));
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
        let track = Rect::from_min_max(pos2(rail.left(), y - 3.0), pos2(rail.right(), y + 3.0));
        let painter = ui.painter();
        painter.rect_filled(track, CornerRadius::same(3), kit::icon_tile_fill(visuals));
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
        painter.circle_filled(pos2(x, y), knob, egui::Color32::WHITE);
    }
    response
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
