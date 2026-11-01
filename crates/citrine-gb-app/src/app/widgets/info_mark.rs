use crate::icons;
use egui::{
    Align2, Color32, Direction, FontId, Frame, Margin, Popup, PopupCloseBehavior, PopupKind,
    RectAlign, Response, RichText, Sense, SetOpenCommand, TextStyle, Ui, WidgetInfo, WidgetType,
    vec2,
};

const POPUP_WIDTH: f32 = 280.0;

pub fn width(ui: &Ui) -> f32 {
    side(ui) + ui.spacing().item_spacing.x / 2.0
}

const COMPACT_GLYPH: f32 = 0.85;

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

fn side(ui: &Ui) -> f32 {
    let touch = crate::app::mobile::touch_square(ui);
    if touch > 0.0 { touch } else { glyph_width(ui) }
}

pub fn info_mark(ui: &mut Ui, text: &str) -> Response {
    if ui.layout().main_dir() == Direction::LeftToRight {
        ui.add_space(-ui.spacing().item_spacing.x / 2.0);
    }
    let side = side(ui);
    let height = side.max(ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_exact_size(vec2(side, height), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "More about this"));

    if ui.is_rect_visible(rect) {
        let center = egui::pos2(rect.left() + glyph_width(ui) / 2.0, rect.center().y);
        let visuals = ui.visuals();
        let lit = is_lit(&response);
        if lit {
            let radius = glyph_size(ui) * 0.7;
            ui.painter()
                .circle_filled(center, radius, visuals.widgets.hovered.weak_bg_fill);
        }
        let color = if lit {
            visuals.strong_text_color()
        } else {
            visuals.weak_text_color()
        };
        ui.painter().text(
            center,
            Align2::CENTER_CENTER,
            icons::INFO,
            FontId::proportional(glyph_size(ui)),
            color,
        );
    }

    hint(&response, text);
    response
}

pub fn is_lit(response: &Response) -> bool {
    let pinned = Popup::is_id_open(&response.ctx, Popup::default_response_id(response));
    pinned || response.hovered() || response.has_focus()
}

pub fn hint(response: &Response, text: &str) {
    show_hint(response, text, None);
}

const BESIDE: [RectAlign; 1] = [RectAlign::RIGHT];

pub fn hint_beside(response: &Response, text: &str) {
    show_hint(response, text, Some(RectAlign::LEFT));
}

fn show_hint(response: &Response, text: &str, beside: Option<RectAlign>) {
    let pinned = Popup::is_id_open(&response.ctx, Popup::default_response_id(response));
    let pointed = response.hovered() || response.has_focus();
    let width = POPUP_WIDTH.min(response.ctx.content_rect().width() - 32.0);
    let popup = Popup::from_response(response)
        .kind(PopupKind::Popup)
        .close_behavior(if response.clicked() {
            PopupCloseBehavior::IgnoreClicks
        } else {
            PopupCloseBehavior::CloseOnClickOutside
        })
        .width(width);
    let popup = match beside {
        Some(align) => popup
            .align(align)
            .align_alternatives(&BESIDE)
            .frame(Frame::popup(&response.ctx.style()).inner_margin(Margin::symmetric(8, 2))),
        None => popup,
    };
    let popup = if pinned || response.clicked() {
        popup.open_memory(response.clicked().then_some(SetOpenCommand::Toggle))
    } else {
        popup.open(pointed)
    };
    popup.show(|ui| {
        ui.set_max_width(width);
        if beside.is_some() {
            ui.label(RichText::new(text).small());
        } else {
            ui.label(text);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Pos2, RawInput, Rect};

    const TEXT: &str = "What this setting does";

    struct Page {
        ctx: Context,
        mark: Rect,
    }

    impl Page {
        fn new() -> Self {
            let mut page = Self {
                ctx: Context::default(),
                mark: Rect::NOTHING,
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
            let output = self.ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Setting");
                        mark = info_mark(ui, TEXT).rect;
                    });
                    ui.add_space(300.0);
                    ui.label("Below");
                });
            });
            self.mark = mark;
            output.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text() == TEXT)
            })
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
    fn a_second_tap_on_the_mark_closes_the_hint() {
        let mut page = Page::new();
        assert!(page.tap(page.mark.center()));
        assert!(!page.tap(page.mark.center()));
    }
}
