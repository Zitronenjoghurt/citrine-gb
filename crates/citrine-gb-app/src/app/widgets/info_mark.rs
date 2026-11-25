use crate::icons;
use egui::{
    Align2, Color32, Direction, FontId, Frame, Margin, Popup, PopupAnchor, PopupCloseBehavior,
    PopupKind, Rect, RectAlign, Response, RichText, Sense, SetOpenCommand, TextStyle, Ui,
    WidgetInfo, WidgetType, vec2,
};

const POPUP_WIDTH: f32 = 280.0;
const POINTED_GAP: f32 = 8.0;
const POINTER: f32 = 6.0;
const POINTED_EDGE: f32 = 16.0;
const POINTED_MARGIN_X: f32 = 10.0;
const RING_CLEAR: f32 = 2.0;

pub fn width(ui: &Ui) -> f32 {
    side(ui) + ui.spacing().item_spacing.x / 2.0
}

const COMPACT_GLYPH: f32 = 0.85;
const LIT_RADIUS: f32 = 0.55;

fn glyph_size(ui: &Ui) -> f32 {
    let body = ui.text_style_height(&TextStyle::Body);
    if crate::app::mobile::touch_square(ui) > 0.0 {
        body
    } else {
        body * COMPACT_GLYPH
    }
}

fn glyph_width(ui: &Ui) -> f32 {
    ui.painter()
        .layout_no_wrap(
            icons::INFO.to_owned(),
            FontId::proportional(glyph_size(ui)),
            Color32::PLACEHOLDER,
        )
        .size()
        .x
}

fn disc_radius(ui: &Ui) -> f32 {
    glyph_size(ui) * LIT_RADIUS
}

fn side(ui: &Ui) -> f32 {
    let touch = crate::app::mobile::touch_square(ui);
    if touch > 0.0 {
        touch
    } else {
        let glyph = glyph_width(ui);
        glyph + (disc_radius(ui) - glyph / 2.0).max(ring_shift(ui)).max(0.0)
    }
}

fn ring_shift(ui: &Ui) -> f32 {
    crate::app::big_screen::ring::MARGIN + RING_CLEAR - ui.spacing().item_spacing.x / 2.0
}

pub fn info_mark_width(ui: &Ui) -> f32 {
    side(ui) + ui.spacing().item_spacing.x / 2.0
}

pub fn info_mark(ui: &mut Ui, text: &str) -> Response {
    info_mark_shown(ui, text, false)
}

pub fn info_mark_shown(ui: &mut Ui, text: &str, shown: bool) -> Response {
    let (rect, response) = place(ui);
    mark(ui, rect, response, shown, |response| {
        show_hint(response, text, Placement::Plain, shown)
    })
}

pub fn info_mark_above(ui: &mut Ui, text: &str, edge: f32) -> Response {
    let (rect, response) = place(ui);
    let anchor = Rect::from_x_y_ranges(rect.x_range(), edge..=edge);
    mark(ui, rect, response, false, |response| {
        show_hint(response, text, Placement::Above(anchor), false);
    })
}

pub fn info_mark_over(ui: &mut Ui, text: &str, cover: Rect) -> (Response, f32) {
    let (rect, response) = place(ui);
    let ring_top =
        rect.center().y - glyph_width(ui) / 2.0 - crate::app::big_screen::ring::MARGIN - RING_CLEAR;
    let mut cover = cover;
    cover.max.y = cover.max.y.min(ring_top);
    let short = over_galley(ui, text, cover.width()).size().y + over_margin(ui).y - cover.height();
    let response = mark(ui, rect, response, false, |response| {
        show_hint(response, text, Placement::Over(cover), false)
    });
    (response, short)
}

fn over_margin(ui: &Ui) -> egui::Vec2 {
    Frame::popup(ui.style()).total_margin().sum()
}

fn over_galley(ui: &Ui, text: &str, width: f32) -> std::sync::Arc<egui::Galley> {
    let inner = (width - over_margin(ui).x).max(1.0);
    let layout = |max: f32| {
        let mut job =
            std::sync::Arc::unwrap_or_clone(egui::WidgetText::from(text).into_layout_job(
                ui.style(),
                egui::FontSelection::Style(TextStyle::Body),
                ui.text_valign(),
            ));
        job.wrap.max_width = max;
        job.halign = egui::Align::Center;
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    layout(super::text::balanced_width(inner, |width| {
        layout(width).rows.len()
    }))
}

fn place(ui: &mut Ui) -> (Rect, Response) {
    if ui.layout().main_dir() == Direction::LeftToRight {
        ui.add_space(-ui.spacing().item_spacing.x / 2.0);
    }
    let side = side(ui);
    let height = side.max(ui.spacing().interact_size.y);
    let (rect, placed) = ui.allocate_exact_size(vec2(side, height), Sense::hover());
    let response = ui.interact(hit_rect(ui, rect), placed.id.with("tap"), Sense::click());
    (rect, response)
}

pub fn info_mark_below(ui: &mut Ui, text: &str) -> Response {
    if ui.layout().main_dir() == Direction::LeftToRight {
        ui.add_space(-ui.spacing().item_spacing.x / 2.0);
    }
    let side = side(ui);
    let height = side.max(ui.spacing().interact_size.y);
    let (rect, placed) = ui.allocate_exact_size(vec2(side, height), Sense::hover());
    let response = ui.interact(hit_rect(ui, rect), placed.id.with("tap"), Sense::click());
    paint(ui, rect, &response, false);
    let glyph = Rect::from_center_size(
        glyph_center(ui, rect),
        vec2(glyph_width(ui), glyph_size(ui)),
    );
    show_hint(&response, text, pointed(&response, text, glyph), false);
    response
}

pub fn tap_hint(response: &Response, text: &str, target: Rect) {
    show_hint(response, text, pointed(response, text, target), false);
}

pub fn hover_hint_at(response: &Response, text: &str, target: Rect) {
    let placement = pointed(response, text, target);
    let Placement::Below {
        anchor,
        target,
        width,
    } = placement
    else {
        return;
    };
    let shown = Popup::from_response(response)
        .kind(PopupKind::Tooltip)
        .sense(Sense::hover())
        .width(width)
        .anchor(anchor)
        .gap(POINTED_GAP)
        .align(RectAlign::BOTTOM)
        .align_alternatives(&[])
        .frame(pointed_frame(&response.ctx))
        .open(response.hovered())
        .show(|ui| {
            ui.set_max_width(width);
            ui.label(text);
        });
    if let Some(shown) = shown {
        pointer(
            &response.ctx,
            shown.response.layer_id,
            shown.response.rect,
            target,
        );
    }
}

fn pointed(response: &Response, text: &str, target: Rect) -> Placement {
    let ctx = &response.ctx;
    let screen = ctx.content_rect();
    let style = ctx.style();
    let text_width = ctx
        .fonts_mut(|fonts| {
            fonts.layout_no_wrap(
                text.to_owned(),
                TextStyle::Body.resolve(&style),
                Color32::PLACEHOLDER,
            )
        })
        .size()
        .x;
    let width = (text_width + 1.0).min(screen.width() - 2.0 * (POINTED_EDGE + POINTED_MARGIN_X));
    let half = width / 2.0 + POINTED_MARGIN_X;
    let x = target
        .center()
        .x
        .max(screen.left() + POINTED_EDGE + half)
        .min(screen.right() - POINTED_EDGE - half);
    let anchor = Rect::from_min_max(egui::pos2(x, target.top()), egui::pos2(x, target.bottom()));
    Placement::Below {
        anchor,
        target,
        width,
    }
}

fn pointed_frame(ctx: &egui::Context) -> Frame {
    Frame::popup(&ctx.style()).inner_margin(Margin::symmetric(POINTED_MARGIN_X as i8, 4))
}

pub fn info_mark_in_line(ui: &mut Ui, text: &str) -> Response {
    if ui.layout().main_dir() == Direction::LeftToRight {
        ui.add_space(-ui.spacing().item_spacing.x / 2.0);
    }
    let side = side(ui);
    let height = side.max(ui.spacing().interact_size.y);
    let line = ui.available_height().min(height);
    let (slot, placed) = ui.allocate_exact_size(vec2(side, line), Sense::hover());
    let rect = Rect::from_center_size(slot.center(), vec2(side, height));
    let response = ui.interact(hit_rect(ui, rect), placed.id.with("tap"), Sense::click());
    mark(ui, rect, response, false, |response| hint(response, text))
}

fn hit_rect(ui: &Ui, rect: Rect) -> Rect {
    let radius = disc_radius(ui);
    rect.union(Rect::from_center_size(
        glyph_center(ui, rect),
        vec2(radius * 2.0, radius * 2.0),
    ))
}

fn mark(
    ui: &Ui,
    rect: Rect,
    response: Response,
    shown: bool,
    hint: impl FnOnce(&Response),
) -> Response {
    paint(ui, rect, &response, shown);
    hint(&response);
    response
}

fn glyph_shift(ui: &Ui, width: f32) -> f32 {
    let room = (width - glyph_width(ui)).max(0.0);
    let most = if crate::app::mobile::touch_square(ui) > 0.0 {
        room / 2.0
    } else {
        room
    };
    ring_shift(ui).clamp(0.0, most)
}

pub fn info_mark_ink_width(ui: &Ui) -> f32 {
    ui.spacing().item_spacing.x / 2.0 + glyph_shift(ui, side(ui)) + glyph_width(ui)
}

fn glyph_center(ui: &Ui, rect: Rect) -> egui::Pos2 {
    let shift = glyph_shift(ui, rect.width());
    egui::pos2(rect.left() + glyph_width(ui) / 2.0 + shift, rect.center().y)
}

fn paint(ui: &Ui, rect: Rect, response: &Response, shown: bool) {
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "More about this"));

    let center = glyph_center(ui, rect);
    let ring = glyph_width(ui);
    crate::app::big_screen::ring::mark(
        response,
        crate::app::big_screen::ring::Mark::Ring {
            rect: Rect::from_center_size(center, vec2(ring, ring)),
            radius: ring / 2.0,
        },
    );
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let lit = shown || is_lit(response);
        if lit {
            let radius = disc_radius(ui);
            ui.painter()
                .circle_filled(center, radius, visuals.widgets.hovered.weak_bg_fill);
        }
        let color = mark_color(visuals, lit);
        ui.painter().text(
            center,
            Align2::CENTER_CENTER,
            icons::INFO,
            FontId::proportional(glyph_size(ui)),
            color,
        );
    }
}

pub fn mark_color(visuals: &egui::Visuals, lit: bool) -> Color32 {
    if lit {
        visuals.strong_text_color()
    } else {
        visuals.weak_text_color()
    }
}

pub fn is_lit(response: &Response) -> bool {
    let pinned = Popup::is_id_open(&response.ctx, Popup::default_response_id(response));
    pinned || response.hovered() || response.has_focus()
}

pub fn hint(response: &Response, text: &str) {
    show_hint(response, text, Placement::Plain, false);
}

#[derive(Clone, Copy)]
enum Placement {
    Plain,
    Below {
        anchor: Rect,
        target: Rect,
        width: f32,
    },
    Beside(RectAlign),
    Above(Rect),
    Over(Rect),
}

const BESIDE: [RectAlign; 1] = [RectAlign::RIGHT];

const ABOVE: [RectAlign; 1] = [RectAlign::TOP_START];

const SCREEN_EDGE: f32 = 16.0;

const ABOVE_GAP: f32 = 6.0;

pub fn hint_above(response: &Response, text: &str) {
    show_hint(response, text, Placement::Above(response.rect), false);
}

pub fn hint_beside(response: &Response, text: &str) {
    show_hint(response, text, Placement::Beside(RectAlign::LEFT), false);
}

fn fits_below(response: &Response, height: f32) -> bool {
    let room = response.ctx.available_rect();
    let below = response.rect.bottom() + height;
    let above = response.rect.top() - height;
    below <= room.bottom() || above < room.top()
}

fn show_hint(response: &Response, text: &str, placement: Placement, shown: bool) {
    let pinned = Popup::is_id_open(&response.ctx, Popup::default_response_id(response));
    let pointed = shown || response.hovered() || response.has_focus();
    let screen = response.ctx.content_rect();
    let width = match placement {
        Placement::Below { width, .. } => width,
        _ => POPUP_WIDTH.min(screen.width() - 2.0 * SCREEN_EDGE),
    };
    let popup = Popup::from_response(response)
        .kind(PopupKind::Popup)
        .sense(Sense::hover())
        .close_behavior(if response.clicked() {
            PopupCloseBehavior::IgnoreClicks
        } else {
            PopupCloseBehavior::CloseOnClickOutside
        })
        .width(width);
    let popup = match placement {
        Placement::Beside(align) => popup
            .align(align)
            .align_alternatives(&BESIDE)
            .frame(Frame::popup(&response.ctx.style()).inner_margin(Margin::symmetric(8, 2))),
        Placement::Below { anchor, .. } => popup
            .anchor(anchor)
            .gap(POINTED_GAP)
            .align(RectAlign::BOTTOM)
            .align_alternatives(&[])
            .frame(pointed_frame(&response.ctx)),
        Placement::Above(anchor) => {
            let half = popup.get_expected_size().map_or(width, |size| size.x) / 2.0;
            let (low, high) = (
                screen.left() + SCREEN_EDGE + half,
                screen.right() - SCREEN_EDGE - half,
            );
            let x = if low <= high {
                anchor.center().x.clamp(low, high)
            } else {
                screen.center().x
            };
            popup
                .anchor(PopupAnchor::ParentRect(
                    anchor.translate(vec2(x - anchor.center().x, 0.0)),
                ))
                .gap(ABOVE_GAP)
                .align(RectAlign::TOP)
                .align_alternatives(&[])
        }
        Placement::Plain => {
            let expected = popup.get_expected_size();
            let size = expected.unwrap_or(vec2(width, 0.0));
            let left = response
                .rect
                .left()
                .min(screen.right() - SCREEN_EDGE - size.x)
                .max(screen.left() + SCREEN_EDGE);
            let popup = popup
                .anchor(Rect::from_x_y_ranges(left..=left, response.rect.y_range()))
                .align_alternatives(&ABOVE);
            match expected {
                Some(size) if !fits_below(response, size.y) => popup.align(RectAlign::TOP_START),
                _ => popup,
            }
        }
        Placement::Over(cover) => popup
            .anchor(cover)
            .align(RectAlign {
                parent: Align2::CENTER_BOTTOM,
                child: Align2::CENTER_BOTTOM,
            })
            .align_alternatives(&[])
            .gap(0.0)
            .width(cover.width()),
    };
    let popup = if pinned || response.clicked() {
        popup.open_memory(response.clicked().then_some(SetOpenCommand::Toggle))
    } else {
        popup.open(pointed)
    };
    let shown = popup.show(|ui| {
        if let Placement::Over(cover) = placement {
            let inner = cover.size() - over_margin(ui);
            ui.set_min_size(inner);
            ui.set_max_width(inner.x);
            let galley = over_galley(ui, text, cover.width());
            ui.vertical_centered(|ui| {
                ui.add_space(((inner.y - galley.size().y) / 2.0).max(0.0));
                ui.label(galley);
            });
            return;
        }
        ui.set_max_width(width);
        if matches!(placement, Placement::Beside(_)) {
            ui.label(RichText::new(text).small());
        } else {
            ui.label(text);
        }
    });
    if let (Placement::Below { target, .. }, Some(shown)) = (placement, shown) {
        pointer(
            &response.ctx,
            shown.response.layer_id,
            shown.response.rect,
            target,
        );
    }
}

fn pointer(ctx: &egui::Context, layer: egui::LayerId, popup: Rect, target: Rect) {
    let style = ctx.style();
    let stroke = style.visuals.window_stroke();
    let reach = style.visuals.menu_corner_radius.nw as f32 + POINTER;
    let x = target
        .center()
        .x
        .max(popup.left() + reach)
        .min(popup.right() - reach);
    let base = popup.top() + stroke.width;
    let tip = egui::pos2(x, popup.top() - POINTER);
    let left = egui::pos2(x - POINTER, base);
    let right = egui::pos2(x + POINTER, base);
    let painter = ctx.layer_painter(layer);
    painter.add(egui::Shape::convex_polygon(
        vec![left, right, tip],
        style.visuals.window_fill(),
        egui::Stroke::NONE,
    ));
    painter.line_segment([left, tip], stroke);
    painter.line_segment([tip, right], stroke);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Pos2, RawInput};

    const TEXT: &str = "What this setting does";

    struct Page {
        ctx: Context,
        mark: Rect,
        label: String,
        above: f32,
        hint: Option<Rect>,
        label_rect: Rect,
    }

    impl Page {
        fn new() -> Self {
            Self::with_row("Setting", 0.0)
        }

        fn with_row(label: impl Into<String>, above: f32) -> Self {
            let mut page = Self {
                ctx: Context::default(),
                mark: Rect::NOTHING,
                label: label.into(),
                above,
                hint: None,
                label_rect: Rect::NOTHING,
            };
            page.frame(Vec::new());
            page.frame(Vec::new());
            page
        }

        fn frame(&mut self, events: Vec<Event>) -> bool {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(412.0, 600.0))),
                events,
                ..Default::default()
            };
            let mut mark = self.mark;
            let (label, above) = (self.label.as_str(), self.above);
            let mut label_rect = self.label_rect;
            let output = self.ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(above);
                    ui.horizontal(|ui| {
                        label_rect = ui.label(label).rect;
                        mark = info_mark(ui, TEXT).rect;
                    });
                    ui.add_space(300.0);
                    ui.label("Below");
                });
            });
            self.mark = mark;
            self.label_rect = label_rect;
            self.hint = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == TEXT => {
                        Some(Rect::from_min_size(text.pos, text.galley.size()))
                    }
                    _ => None,
                });
            self.hint.is_some()
        }

        fn settle(&mut self, events: Vec<Event>) -> bool {
            self.frame(events);
            self.frame(Vec::new())
        }

        fn tap(&mut self, at: Pos2) -> bool {
            let press = |pressed| Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at), press(true)]);
            self.frame(vec![press(false), Event::PointerGone]);
            self.frame(Vec::new())
        }

        fn key(&mut self, key: egui::Key) -> bool {
            self.settle(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }])
        }
    }

    #[test]
    fn an_in_line_mark_keeps_its_tap_height_without_growing_the_row() {
        let ctx = Context::default();
        let mut row = Rect::NOTHING;
        let mut mark = Rect::NOTHING;
        let _ = ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().interact_size.y = 44.0;
                let layout = egui::Layout::left_to_right(egui::Align::Center);
                row = ui
                    .allocate_ui_with_layout(vec2(300.0, 17.0), layout, |ui| {
                        ui.label("In Pewter City");
                        mark = info_mark_in_line(ui, TEXT).rect;
                    })
                    .response
                    .rect;
            });
        });
        assert_eq!(mark.height(), 44.0);
        assert!(row.height() < 30.0, "{row:?}");
        assert!(
            (mark.center().y - row.center().y).abs() < 1.0,
            "{row:?} {mark:?}"
        );
    }

    #[test]
    fn the_hint_is_closed_until_asked_for() {
        let mut page = Page::new();
        assert!(!page.settle(Vec::new()));
    }

    #[test]
    fn hovering_the_mark_opens_the_hint_and_leaving_closes_it() {
        let mut page = Page::new();
        assert!(page.settle(vec![Event::PointerMoved(page.mark.center())]));
        assert!(!page.settle(vec![Event::PointerMoved(Pos2::new(300.0, 500.0))]));
    }

    #[test]
    fn focusing_the_mark_opens_the_hint() {
        let mut page = Page::new();
        assert!(page.key(egui::Key::Tab));
    }

    #[test]
    fn a_tap_opens_the_hint_until_a_tap_elsewhere_or_escape() {
        let mut page = Page::new();
        assert!(page.tap(page.mark.center()));
        assert!(page.settle(Vec::new()));
        assert!(!page.tap(Pos2::new(300.0, 500.0)));

        assert!(page.tap(page.mark.center()));
        assert!(!page.key(egui::Key::Escape));
    }

    #[test]
    fn a_hint_too_wide_to_start_at_its_mark_still_opens_below_it() {
        let mut page = Page::with_row("Talk button on screen, a long label", 300.0);
        assert!(page.mark.left() + POPUP_WIDTH > 412.0, "{:?}", page.mark);
        assert!(page.tap(page.mark.center()));
        let hint = page.hint.expect("hint shown");
        assert!(hint.top() >= page.mark.bottom(), "{hint:?} {:?}", page.mark);
    }

    #[test]
    fn a_hint_keeps_clear_of_the_screen_edges_wherever_its_mark_sits() {
        for length in 1..40 {
            let mut page = Page::with_row("x".repeat(length), 300.0);
            assert!(page.tap(page.mark.center()));
            let hint = page.hint.expect("hint shown");
            let margin = Frame::popup(&page.ctx.style()).total_margin();
            assert!(
                hint.left() - margin.left >= SCREEN_EDGE - 0.5,
                "{length} {hint:?}"
            );
            assert!(
                hint.right() + margin.right <= 412.5 - SCREEN_EDGE,
                "{length} {hint:?}"
            );
            assert!(hint.top() >= page.mark.bottom(), "{length} {hint:?}");
        }
    }

    #[test]
    fn a_hint_with_no_room_above_a_bottom_bar_opens_above_its_mark() {
        let ctx = Context::default();
        let mut mark = Rect::NOTHING;
        let mut hint = None;
        for _ in 0..4 {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(412.0, 600.0))),
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                egui::TopBottomPanel::bottom("bar")
                    .exact_height(60.0)
                    .show(ctx, |_| {});
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(500.0);
                    mark = info_mark_shown(ui, TEXT, true).rect;
                });
            });
            hint = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == TEXT => {
                        Some(text.visual_bounding_rect())
                    }
                    _ => None,
                });
        }
        let hint = hint.expect("the hint shows");
        assert!(
            hint.bottom() <= mark.top(),
            "the hint {hint:?} opens above the mark {mark:?}"
        );
    }

    #[test]
    fn an_open_hint_holds_no_focus_stop() {
        let mut page = Page::new();
        assert!(page.key(egui::Key::Tab));
        let stops = page.ctx.viewport(|viewport| {
            viewport
                .prev_pass
                .widgets
                .layers()
                .filter(|(layer, _)| layer.order == egui::Order::Foreground)
                .flat_map(|(_, widgets)| widgets.iter())
                .filter(|widget| widget.sense.is_focusable())
                .count()
        });
        assert_eq!(stops, 0);
    }

    #[test]
    fn a_touch_mark_rings_inside_its_square() {
        use crate::app::big_screen::ring;
        let mut page = Page::new();
        page.ctx
            .style_mut(|style| style.spacing.interact_size.y = 44.0);
        assert!(page.key(egui::Key::Tab));
        let (response, marked) = ring::current(&page.ctx).expect("the mark has focus");
        let (ring, _) = marked.expect("the mark rings");
        let outer = ring.expand(ring::WIDTH);
        assert!(
            response
                .rect
                .expand(ring::MARGIN + 0.5)
                .contains_rect(outer),
            "the ring {outer:?} spills past {:?}",
            response.rect
        );
        assert!(
            outer.left() >= page.label_rect.right(),
            "the ring {outer:?} covers the label {:?}",
            page.label_rect
        );
    }

    #[test]
    fn a_second_tap_on_the_mark_closes_the_hint() {
        let mut page = Page::new();
        assert!(page.tap(page.mark.center()));
        assert!(!page.tap(page.mark.center()));
    }

    #[test]
    fn a_hint_over_a_block_never_leaves_a_word_alone_on_its_last_line() {
        let hint = crate::app::second_screens::ask_code_hint("Pixel 8");
        let ctx = Context::default();
        let _ = ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for width in (200..=420).step_by(5) {
                    let galley = over_galley(ui, &hint, width as f32);
                    let rows: Vec<String> = galley
                        .rows
                        .iter()
                        .map(|row| row.row.glyphs.iter().map(|g| g.chr).collect())
                        .collect();
                    if let Some(last) = rows.last().filter(|_| rows.len() > 1) {
                        assert!(
                            last.trim().contains(' '),
                            "at {width} the last line is one word: {rows:?}"
                        );
                    }
                }
            });
        });
    }
}
