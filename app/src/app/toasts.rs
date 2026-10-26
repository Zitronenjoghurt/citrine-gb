use crate::app::events::AppEvent;
use crate::icons;
use egui::{Align2, Color32, Context, Id, Label, Order, Rect, RichText, Ui, pos2, vec2};
use std::time::Duration;

const LIFETIME: f32 = 3.5;
const BUTTON_LIFETIME: f32 = 6.0;
const FADE: f32 = 0.15;
const MARGIN: f32 = 8.0;
const SPACING: f32 = 6.0;
const MAX_WIDTH: f32 = 420.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Info,
    Success,
    Error,
}

pub struct Toast {
    id: u64,
    kind: Kind,
    text: String,
    remaining: Option<f32>,
    age: f32,
    closing: Option<f32>,
    button: Option<(String, AppEvent)>,
}

impl Toast {
    fn new(kind: Kind, text: impl Into<String>) -> Self {
        Self {
            id: 0,
            kind,
            text: text.into(),
            remaining: Some(LIFETIME),
            age: 0.0,
            closing: None,
            button: None,
        }
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self::new(Kind::Info, text)
    }

    pub fn duration(&mut self, duration: Option<Duration>) -> &mut Self {
        self.remaining = duration.map(|d| d.as_secs_f32());
        self
    }

    pub fn button(&mut self, label: impl Into<String>, event: AppEvent) -> &mut Self {
        if self.remaining.is_some() {
            self.remaining = Some(BUTTON_LIFETIME);
        }
        self.button = Some((label.into(), event));
        self
    }

    fn opacity(&self) -> f32 {
        let shown = (self.age / FADE).min(1.0);
        let hidden = self.closing.map_or(0.0, |c| (c / FADE).min(1.0));
        shown * (1.0 - hidden)
    }

    fn advance(&mut self, dt: f32, hovered: bool) {
        self.age += dt;
        if let Some(closing) = &mut self.closing {
            *closing += dt;
            return;
        }
        if hovered {
            return;
        }
        if let Some(remaining) = &mut self.remaining {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.closing = Some(0.0);
            }
        }
    }

    fn gone(&self) -> bool {
        self.closing.is_some_and(|c| c >= FADE)
    }
}

#[derive(Default)]
pub struct Toasts {
    toasts: Vec<Toast>,
    next_id: u64,
    area: Option<Rect>,
    strip: Option<Rect>,
    needs_strip: bool,
}

impl Toasts {
    pub fn add(&mut self, mut toast: Toast) -> &mut Toast {
        toast.id = self.next_id;
        self.next_id += 1;
        self.toasts.push(toast);
        self.toasts.last_mut().expect("just pushed")
    }

    pub fn info(&mut self, text: impl Into<String>) -> &mut Toast {
        self.add(Toast::new(Kind::Info, text))
    }

    pub fn success(&mut self, text: impl Into<String>) -> &mut Toast {
        self.add(Toast::new(Kind::Success, text))
    }

    pub fn error(&mut self, text: impl Into<String>) -> &mut Toast {
        self.add(Toast::new(Kind::Error, text))
    }

    pub fn dismiss_all_toasts(&mut self) {
        for toast in &mut self.toasts {
            toast.closing.get_or_insert(0.0);
        }
    }

    pub fn place(&mut self, area: Rect) {
        self.area = Some(area);
    }

    pub fn place_strip(&mut self, strip: Rect) {
        self.strip = Some(strip);
    }

    pub fn wants_strip(&self) -> bool {
        self.needs_strip && !self.toasts.is_empty()
    }

    pub fn strip_height(ctx: &Context) -> f32 {
        let style = ctx.style();
        style.spacing.interact_size.y + style.spacing.menu_margin.sum().y + MARGIN * 2.0
    }

    pub fn show(&mut self, ctx: &Context) -> Vec<AppEvent> {
        let mut events = Vec::new();
        let game = self.area.take();
        let strip = self.strip.take();
        self.needs_strip = game.is_none();
        let Some(area) = game.or(strip) else {
            return events;
        };
        if self.toasts.is_empty() || crate::app::widgets::confirm::is_open(ctx) {
            return events;
        }

        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let width = (area.width() - MARGIN * 2.0).clamp(0.0, MAX_WIDTH);
        let mut bottom = area.bottom() - MARGIN;

        for toast in self.toasts.iter_mut().rev() {
            let response = egui::Area::new(Id::new(("toast", toast.id)))
                .order(Order::Middle)
                .fixed_pos(pos2(area.center().x, bottom))
                .pivot(Align2::CENTER_BOTTOM)
                .constrain(false)
                .show(ctx, |ui| {
                    ui.set_opacity(toast.opacity());
                    egui::Frame::popup(ui.style())
                        .show(ui, |ui| body(ui, toast, width))
                        .inner
                });
            match response.inner {
                Clicked::Button => {
                    toast.closing.get_or_insert(0.0);
                    events.extend(toast.button.take().map(|(_, event)| event));
                }
                Clicked::Close => {
                    toast.closing.get_or_insert(0.0);
                }
                Clicked::Nothing => {}
            }
            let hovered = pointer.is_some_and(|p| response.response.rect.contains(p));
            toast.advance(dt, hovered);
            bottom -= response.response.rect.height() + SPACING;
        }

        self.toasts.retain(|toast| !toast.gone());
        events
    }
}

enum Clicked {
    Nothing,
    Button,
    Close,
}

fn body(ui: &mut Ui, toast: &Toast, width: f32) -> Clicked {
    let (icon, color) = match toast.kind {
        Kind::Info => (icons::INFO, ui.visuals().hyperlink_color),
        Kind::Success => (
            icons::CHECK_CIRCLE,
            if ui.visuals().dark_mode {
                Color32::from_rgb(130, 210, 130)
            } else {
                Color32::from_rgb(30, 130, 60)
            },
        ),
        Kind::Error => (icons::WARNING_CIRCLE, ui.visuals().error_fg_color),
    };

    ui.horizontal(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let inner = width - ui.spacing().menu_margin.sum().x;
        let icon_width = ui.fonts_mut(|f| {
            f.layout_no_wrap(
                icon.to_string(),
                egui::TextStyle::Body.resolve(ui.style()),
                color,
            )
            .size()
            .x
        });
        let close_width = icon_width + ui.spacing().button_padding.x * 2.0;
        let text_width = egui::WidgetText::from(toast.text.as_str())
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Body,
            )
            .size()
            .x;
        let button_width = toast.button.as_ref().map_or(0.0, |(label, _)| {
            egui::WidgetText::from(label.as_str())
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Button,
                )
                .size()
                .x
                .max(ui.spacing().interact_size.y)
                + ui.spacing().button_padding.x * 2.0
                + spacing
        });
        let room = (inner - icon_width - close_width - button_width - spacing * 2.0).max(0.0);

        ui.label(RichText::new(icon).color(color));
        ui.allocate_ui(
            vec2(text_width.min(room), ui.spacing().interact_size.y),
            |ui| {
                ui.add(Label::new(toast.text.as_str()).truncate());
            },
        );
        let mut clicked = Clicked::Nothing;
        if let Some((label, _)) = &toast.button {
            let min = vec2(ui.spacing().interact_size.y, 0.0);
            if ui
                .add(egui::Button::new(label.as_str()).min_size(min))
                .clicked()
            {
                clicked = Clicked::Button;
            }
        }
        if ui
            .add(egui::Button::new(icons::X).frame(false))
            .on_hover_text("Dismiss")
            .clicked()
        {
            clicked = Clicked::Close;
        }
        clicked
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tabs::Tab;

    #[test]
    fn a_toast_stays_while_hovered() {
        let mut toast = Toast::new(Kind::Success, "Saved");
        toast.advance(LIFETIME * 2.0, true);
        assert!(toast.closing.is_none());
        toast.advance(LIFETIME + 0.01, false);
        assert!(toast.closing.is_some());
        toast.advance(FADE, false);
        assert!(toast.gone());
    }

    #[test]
    fn a_toast_without_duration_waits_for_dismissal() {
        let mut toasts = Toasts::default();
        toasts.add(Toast::info("Focus mode")).duration(None);
        toasts.toasts[0].advance(60.0, false);
        assert!(toasts.toasts[0].closing.is_none());
        toasts.dismiss_all_toasts();
        assert!(toasts.toasts[0].closing.is_some());
    }

    fn rects_after(area: Rect, texts: &[&str]) -> Vec<Rect> {
        let ctx = egui::Context::default();
        let mut toasts = Toasts::default();
        for text in texts {
            toasts
                .error(*text)
                .button("Open", AppEvent::OpenTab { tab: Tab::Media });
        }
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(412.0, 915.0))),
            ..Default::default()
        };
        let mut rects = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(input(), |ctx| {
                toasts.place(area);
                toasts.show(ctx);
            });
            rects = output
                .shapes
                .iter()
                .filter(|clipped| matches!(clipped.shape, egui::Shape::Text(_)))
                .map(|clipped| clipped.shape.visual_bounding_rect())
                .filter(|rect| rect.is_positive())
                .collect();
        }
        rects
    }

    #[test]
    fn toasts_stay_inside_their_area_even_with_long_text() {
        let area = Rect::from_min_max(pos2(0.0, 48.0), pos2(412.0, 520.0));
        let long = "Restarted with 'a really long save file name for a game with a long title.sav'";
        let rects = rects_after(area, &[long, "Failed to save"]);
        assert!(rects.len() >= 6, "{rects:?}");
        for rect in rects {
            assert!(
                area.expand(1.0).contains_rect(rect),
                "{rect:?} outside {area:?}"
            );
        }
    }
}
