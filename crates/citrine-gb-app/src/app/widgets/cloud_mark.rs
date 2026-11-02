use crate::icons;
use crate::online::cloud::SyncState;
use egui::{Align2, Color32, FontId, Response, Sense, TextStyle, Ui, WidgetInfo, WidgetType, vec2};

pub fn icon(state: SyncState) -> &'static str {
    match state {
        SyncState::Synced => icons::CLOUD_CHECK,
        SyncState::Waiting => icons::CLOUD_ARROW_UP,
        SyncState::Fetching => icons::CLOUD_ARROW_DOWN,
        SyncState::Conflict => icons::CLOUD_WARNING,
        SyncState::OnlyHere => icons::CLOUD_SLASH,
        SyncState::Full
        | SyncState::TooLarge
        | SyncState::NoStorage
        | SyncState::NoAccountStorage
        | SyncState::Unreadable => icons::CLOUD_X,
    }
}

pub fn color(ui: &Ui, state: SyncState) -> Color32 {
    match state {
        SyncState::Conflict
        | SyncState::Full
        | SyncState::TooLarge
        | SyncState::NoStorage
        | SyncState::NoAccountStorage
        | SyncState::Unreadable => ui.visuals().warn_fg_color,
        _ => ui.visuals().weak_text_color(),
    }
}

pub fn short_text(state: SyncState) -> Option<&'static str> {
    match state {
        SyncState::Full => Some("Cloud full"),
        SyncState::TooLarge => Some("Too large for the cloud"),
        SyncState::NoStorage | SyncState::NoAccountStorage => Some("No cloud storage"),
        SyncState::Unreadable => Some("Can't open the cloud copy"),
        _ => None,
    }
}

fn side(ui: &Ui) -> f32 {
    let touch = crate::app::mobile::touch_square(ui);
    if touch > 0.0 {
        touch
    } else {
        ui.text_style_height(&TextStyle::Body) + 4.0
    }
}

pub fn height(ui: &Ui) -> f32 {
    side(ui).max(ui.spacing().interact_size.y)
}

pub fn cloud_mark(ui: &mut Ui, state: SyncState) -> Response {
    let body = ui.text_style_height(&TextStyle::Body);
    let (rect, response) = ui.allocate_exact_size(vec2(side(ui), height(ui)), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, state.text()));

    if ui.is_rect_visible(rect) {
        if crate::app::widgets::info_mark::is_lit(&response) {
            ui.painter().circle_filled(
                rect.center(),
                body * 0.7,
                ui.visuals().widgets.hovered.weak_bg_fill,
            );
        }
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon(state),
            FontId::proportional(body),
            color(ui, state),
        );
    }

    crate::app::widgets::info_mark::hint_beside(&response, state.text());
    response
}

pub fn cloud_status(ui: &mut Ui, state: SyncState) {
    cloud_mark(ui, state);
    if let Some(text) = short_text(state) {
        ui.label(egui::RichText::new(text).small().color(color(ui, state)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Layout, Pos2, RawInput, Rect};

    #[derive(Clone, Copy)]
    enum Place {
        RowEnd,
        AfterText,
    }

    struct Row {
        ctx: Context,
        width: f32,
        place: Place,
        controls: Vec<Rect>,
        mark: Rect,
    }

    impl Row {
        fn new(width: f32, touch: bool, place: Place) -> Self {
            let ctx = Context::default();
            crate::app::mobile::touch_spacing(&ctx, touch);
            let mut row = Self {
                ctx,
                width,
                place,
                controls: Vec::new(),
                mark: Rect::NOTHING,
            };
            row.frame(Vec::new());
            row.frame(Vec::new());
            row
        }

        fn frame(&mut self, events: Vec<Event>) -> Option<Rect> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(self.width, 600.0))),
                events,
                ..Default::default()
            };
            let place = self.place;
            let mut controls = Vec::new();
            let mut mark = Rect::NOTHING;
            let output = self.ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Sky island");
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            controls.push(ui.button("Rename").rect);
                        });
                    });
                    ui.allocate_ui_with_layout(
                        vec2(ui.available_width(), height(ui)),
                        Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.small("1m ago");
                            match place {
                                Place::RowEnd => {
                                    ui.with_layout(
                                        Layout::right_to_left(egui::Align::Center),
                                        |ui| mark = cloud_status_rect(ui),
                                    );
                                }
                                Place::AfterText => mark = cloud_status_rect(ui),
                            }
                        },
                    );
                    ui.horizontal(|ui| {
                        controls.push(ui.button("Load").rect);
                        controls.push(ui.button("Overwrite").rect);
                        controls.push(ui.button("More").rect);
                    });
                });
            });
            self.controls = controls;
            self.mark = mark;
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == SyncState::Full.text() => {
                        Some(
                            Rect::from_min_size(text.pos, text.galley.size())
                                .expand2(vec2(8.0, 2.0)),
                        )
                    }
                    _ => None,
                })
        }

        fn tap_mark(&mut self) -> Rect {
            let at = self.mark.center();
            let press = |pressed| Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at), press(true)]);
            self.frame(vec![press(false), Event::PointerGone]);
            self.frame(Vec::new());
            self.frame(Vec::new()).expect("the hint opens")
        }
    }

    fn cloud_status_rect(ui: &mut Ui) -> Rect {
        let rect = cloud_mark(ui, SyncState::Full).rect;
        if let Some(text) = short_text(SyncState::Full) {
            ui.label(egui::RichText::new(text).small());
        }
        rect
    }

    fn assert_covers_no_control(width: f32, touch: bool, place: Place) {
        let mut row = Row::new(width, touch, place);
        let popup = row.tap_mark();
        assert!(!popup.intersects(row.mark), "{popup:?} covers the mark");
        assert!(
            Rect::from_min_size(Pos2::ZERO, vec2(width, 600.0)).contains_rect(popup),
            "{popup:?} leaves the screen"
        );
        for control in &row.controls {
            assert!(!popup.intersects(*control), "{popup:?} covers {control:?}");
        }
    }

    #[test]
    fn the_hint_at_a_row_end_covers_no_control() {
        for (width, touch) in [(412.0, true), (412.0, false), (1280.0, false)] {
            assert_covers_no_control(width, touch, Place::RowEnd);
        }
    }

    #[test]
    fn the_hint_after_a_status_text_covers_no_control() {
        for (width, touch) in [(412.0, true), (412.0, false), (1280.0, false)] {
            assert_covers_no_control(width, touch, Place::AfterText);
        }
    }
}
