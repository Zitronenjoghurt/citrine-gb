use crate::app::events::AppEvent;
use crate::app::widgets::action::dismiss_button;
use crate::app::widgets::progress::ProgressBar;
use crate::icons;
use crate::progress::Progress;
use egui::{Align2, Color32, Context, Id, Label, Order, Rect, RichText, Ui, pos2, vec2};
use std::time::Duration;
use web_time::Instant;

const LIFETIME: f32 = 3.5;
const BUTTON_LIFETIME: f32 = 6.0;
const FADE: f32 = 0.15;
const HINT_LIFETIME: f32 = 3.0;
const HINT_FADE: f32 = 0.3;
const MARGIN: f32 = 8.0;
const SPACING: f32 = 6.0;
const MAX_WIDTH: f32 = 420.0;
const MIN_WIDTH: f32 = 280.0;
const LOAD_SHOWS_AFTER: Duration = Duration::from_millis(400);

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
    fade: f32,
    button: Option<(String, AppEvent)>,
    load: Option<(&'static str, Progress)>,
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
            fade: FADE,
            button: None,
            load: None,
        }
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self::new(Kind::Info, text)
    }

    pub fn hint(text: impl Into<String>, reduced_motion: bool) -> Self {
        let mut toast = Self::new(Kind::Info, text);
        toast.remaining = Some(HINT_LIFETIME);
        toast.fade = if reduced_motion { 0.0 } else { HINT_FADE };
        toast
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
        let part = |time: f32| {
            if self.fade > 0.0 {
                (time / self.fade).min(1.0)
            } else {
                1.0
            }
        };
        let shown = part(self.age);
        let hidden = self.closing.map_or(0.0, part);
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
        self.closing.is_some_and(|c| c >= self.fade)
    }
}

#[derive(Default)]
pub struct Toasts {
    toasts: Vec<Toast>,
    next_id: u64,
    area: Option<Rect>,
    picture: Option<Rect>,
    dock: Option<Rect>,
    strip: Option<Rect>,
    needs_strip: bool,
    held_strip: Option<f32>,
    dismissed: Vec<&'static str>,
    waiting: Vec<(&'static str, Instant)>,
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

    pub fn load(
        &mut self,
        key: &'static str,
        text: impl Into<String>,
        progress: Progress,
        now: Instant,
    ) {
        if self.dismissed.contains(&key) {
            return;
        }
        let text = text.into();
        if let Some(toast) = self
            .toasts
            .iter_mut()
            .find(|t| t.load.is_some_and(|(k, _)| k == key) && t.closing.is_none())
        {
            toast.text = text;
            toast.load = Some((key, progress));
            return;
        }
        let since = match self.waiting.iter().find(|(k, _)| *k == key) {
            Some((_, since)) => *since,
            None => {
                self.waiting.push((key, now));
                now
            }
        };
        if now.duration_since(since) < LOAD_SHOWS_AFTER {
            return;
        }
        let toast = self.add(Toast::new(Kind::Info, text));
        toast.remaining = None;
        toast.load = Some((key, progress));
    }

    pub fn load_done(&mut self, key: &'static str) {
        self.dismissed.retain(|k| *k != key);
        self.waiting.retain(|(k, _)| *k != key);
        for toast in &mut self.toasts {
            if toast.load.is_some_and(|(k, _)| k == key) {
                toast.closing.get_or_insert(0.0);
            }
        }
    }

    pub fn loads(&self) -> Vec<(&str, Progress)> {
        self.toasts
            .iter()
            .filter(|t| t.closing.is_none())
            .filter_map(|t| t.load.map(|(_, p)| (t.text.as_str(), p)))
            .collect()
    }

    pub fn dismiss_all_toasts(&mut self) {
        for toast in &mut self.toasts {
            toast.closing.get_or_insert(0.0);
        }
    }

    pub fn place(&mut self, area: Rect) {
        self.area = Some(area);
    }

    pub fn place_picture(&mut self, picture: Rect) {
        self.picture = Some(picture);
    }

    pub fn place_dock(&mut self, dock: Rect) {
        self.dock = Some(dock);
    }

    pub fn place_strip(&mut self, strip: Rect) {
        self.strip = Some(strip);
    }

    #[cfg(test)]
    pub fn button_labels(&self) -> Vec<&str> {
        self.toasts
            .iter()
            .filter_map(|toast| toast.button.as_ref().map(|(label, _)| label.as_str()))
            .collect()
    }

    #[cfg(test)]
    pub fn rects(&self, ctx: &Context) -> Vec<Rect> {
        self.toasts
            .iter()
            .filter_map(|toast| ctx.memory(|mem| mem.area_rect(Id::new(("toast", toast.id)))))
            .collect()
    }

    pub fn texts(&self) -> Vec<&str> {
        self.toasts
            .iter()
            .map(|toast| toast.text.as_str())
            .collect()
    }

    pub fn wants_strip(&self) -> bool {
        self.needs_strip && !self.toasts.is_empty()
    }

    pub fn strip_height(&self, ctx: &Context) -> f32 {
        let one = ctx.style().spacing.interact_size.y + frame_height(ctx) + MARGIN * 2.0;
        self.held_strip.map_or(one, |held| held.max(one))
    }

    pub fn held_height(&self, ctx: &Context) -> f32 {
        if self.held_strip.is_some() && !self.toasts.is_empty() {
            self.strip_height(ctx)
        } else {
            0.0
        }
    }

    fn stack_height(&self, ctx: &Context) -> f32 {
        let heights: Vec<f32> = self
            .toasts
            .iter()
            .map(|toast| toast_height(ctx, toast))
            .collect();
        heights.iter().sum::<f32>() + SPACING * heights.len().saturating_sub(1) as f32
    }

    pub fn show(&mut self, ctx: &Context) -> Vec<AppEvent> {
        let mut events = Vec::new();
        let game = self.area.take();
        let picture = self.picture.take();
        let dock = self.dock.take();
        let strip = self.strip.take();
        if self.toasts.is_empty() {
            self.held_strip = None;
        }
        let need = self.stack_height(ctx) + MARGIN * 2.0;
        let spot = match (game, picture) {
            (Some(area), Some(picture)) if self.held_strip.is_none() => {
                free_spot(dock.unwrap_or(area), area, picture, need)
            }
            _ => game,
        };
        if spot.is_none() && dock.is_some() && !self.toasts.is_empty() {
            self.held_strip = Some(self.held_strip.unwrap_or(0.0).max(need));
        }
        self.needs_strip = game.is_none() || self.held_strip.is_some();
        let area = match self.held_strip {
            Some(_) => strip,
            None => spot.or(strip),
        };
        let Some(area) = area else {
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
                    if let Some((key, _)) = toast.load {
                        self.dismissed.push(key);
                    }
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

fn frame_height(ctx: &Context) -> f32 {
    let style = ctx.style();
    style.spacing.menu_margin.sum().y + style.visuals.window_stroke.width * 2.0
}

fn toast_height(ctx: &Context, toast: &Toast) -> f32 {
    let style = ctx.style();
    let row = style.spacing.interact_size.y;
    let content = if toast.load.is_some() {
        let font = egui::TextStyle::Body.resolve(&style);
        let line = ctx.fonts_mut(|fonts| fonts.row_height(&font));
        crate::app::widgets::progress::ProgressBar::tallest(line).max(row)
    } else {
        row
    };
    content + frame_height(ctx)
}

fn free_spot(window: Rect, panel: Rect, picture: Rect, need: f32) -> Option<Rect> {
    let below = Rect::from_min_max(pos2(panel.left(), picture.bottom()), panel.max);
    let right = Rect::from_min_max(pos2(picture.right(), panel.top()), panel.max);
    let left = Rect::from_min_max(panel.min, pos2(picture.left(), panel.bottom()));
    let above = Rect::from_min_max(panel.min, pos2(panel.right(), picture.top()));
    let dock_right = Rect::from_min_max(pos2(panel.right(), window.top()), window.max);
    let dock_left = Rect::from_min_max(window.min, pos2(panel.left(), window.bottom()));
    let dock_below = Rect::from_min_max(pos2(window.left(), panel.bottom()), window.max);
    [below, right, left, above, dock_right, dock_left, dock_below]
        .into_iter()
        .find(|spot| spot.width() >= MIN_WIDTH + MARGIN * 2.0 && spot.height() >= need)
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
        let button_width = toast.button.as_ref().map_or(0.0, |(label, _)| {
            crate::app::widgets::homebrew::action_width(ui, &[label.as_str()])
        });
        let button_width = if button_width > 0.0 {
            button_width + spacing
        } else {
            0.0
        };
        let room = (inner - icon_width - close_width - button_width - spacing * 2.0).max(0.0);

        ui.label(RichText::new(icon).color(color));
        if let Some((_, progress)) = toast.load {
            let bar = ProgressBar::new(&toast.text, progress);
            let height = bar.height(ui, room);
            ui.allocate_ui(vec2(room, height), |ui| {
                ui.set_width(room);
                ui.add(bar);
            });
            return if ui.add(dismiss_button()).on_hover_text("Dismiss").clicked() {
                Clicked::Close
            } else {
                Clicked::Nothing
            };
        }
        ui.allocate_ui(vec2(room, ui.spacing().interact_size.y), |ui| {
            ui.set_width(room);
            ui.add(Label::new(toast.text.as_str()).truncate());
        });
        let mut clicked = Clicked::Nothing;
        if let Some((label, _)) = &toast.button {
            let min = vec2(button_width - spacing, 0.0);
            if ui
                .add(egui::Button::new(label.as_str()).min_size(min))
                .clicked()
            {
                clicked = Clicked::Button;
            }
        }
        if ui.add(dismiss_button()).on_hover_text("Dismiss").clicked() {
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

    fn start() -> Instant {
        Instant::now()
    }

    fn later() -> Instant {
        Instant::now() + LOAD_SHOWS_AFTER
    }

    #[test]
    fn a_short_load_shows_no_toast() {
        let mut toasts = Toasts::default();
        let now = start();
        toasts.load("save", "Saving the recording", Progress::unknown(), now);
        toasts.load("save", "Saving the recording", Progress::unknown(), now);
        assert!(toasts.loads().is_empty());
        toasts.load_done("save");
        toasts.load("save", "Saving the recording", Progress::unknown(), later());
        assert!(toasts.loads().is_empty());
    }

    #[test]
    fn a_load_keeps_one_toast_that_follows_its_progress_until_done() {
        let mut toasts = Toasts::default();
        toasts.load(
            "codec",
            "Downloading the video codec",
            Progress::bytes(0, 4),
            start(),
        );
        toasts.load(
            "codec",
            "Downloading the video codec",
            Progress::bytes(1, 4),
            later(),
        );
        toasts.load(
            "codec",
            "Downloading the video codec",
            Progress::bytes(3, 4),
            later(),
        );
        assert_eq!(
            toasts.loads(),
            [("Downloading the video codec", Progress::bytes(3, 4))]
        );
        toasts.load_done("codec");
        assert!(toasts.loads().is_empty());
        toasts.load(
            "codec",
            "Downloading the video codec",
            Progress::unknown(),
            start(),
        );
        toasts.load(
            "codec",
            "Downloading the video codec",
            Progress::unknown(),
            later(),
        );
        assert_eq!(toasts.loads().len(), 1);
    }

    #[test]
    fn a_dismissed_load_stays_away_until_it_ends() {
        let mut toasts = Toasts::default();
        toasts.load("save", "Saving the recording", Progress::unknown(), start());
        toasts.load("save", "Saving the recording", Progress::unknown(), later());
        toasts.toasts[0].closing = Some(0.0);
        toasts.dismissed.push("save");
        toasts.load("save", "Saving the recording", Progress::unknown(), later());
        assert!(toasts.loads().is_empty());
        toasts.load_done("save");
        toasts.load("save", "Saving the recording", Progress::unknown(), start());
        toasts.load("save", "Saving the recording", Progress::unknown(), later());
        assert_eq!(toasts.loads().len(), 1);
    }

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

    #[test]
    fn a_hint_fades_out_after_three_seconds() {
        let mut hint = Toast::hint("Press Esc to leave focus mode", false);
        hint.advance(2.9, false);
        assert!(hint.closing.is_none());
        hint.advance(0.2, false);
        assert!(hint.closing.is_some() && !hint.gone());
        hint.advance(0.15, false);
        assert!(hint.opacity() > 0.0 && hint.opacity() < 1.0);
        hint.advance(0.2, false);
        assert!(hint.gone());

        let mut still = Toast::hint("Press Esc to leave focus mode", true);
        still.advance(0.01, false);
        assert_eq!(still.opacity(), 1.0);
        still.advance(3.0, false);
        assert!(still.gone());
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

    #[test]
    fn on_a_phone_a_download_and_an_error_toast_share_one_width_and_the_bar_sits_centred() {
        let ctx = egui::Context::default();
        let mut toasts = Toasts::default();
        let progress = Progress::bytes(700 * 1024, 2500 * 1024);
        toasts.load("shelf", "Downloading Hyperspace Roll", progress, start());
        toasts.load("shelf", "Downloading Hyperspace Roll", progress, later());
        toasts
            .error("Couldn't download Pentris")
            .button("Try again", AppEvent::OpenTab { tab: Tab::Media });
        let area = Rect::from_min_max(pos2(0.0, 48.0), pos2(412.0, 860.0));
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(412.0, 915.0))),
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(input(), |ctx| {
                crate::app::mobile::touch_spacing(ctx, true);
                toasts.place(area);
                toasts.show(ctx);
            }));
        }
        let frames: Vec<Rect> = toasts
            .toasts
            .iter()
            .map(|toast| {
                ctx.memory(|mem| mem.area_rect(Id::new(("toast", toast.id))))
                    .unwrap()
            })
            .collect();
        assert!(
            (frames[0].width() - frames[1].width()).abs() < 0.5,
            "{frames:?}"
        );
        let shapes = output.unwrap().shapes;
        let title = shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == "Downloading Hyperspace Roll" => {
                    Some(clipped.shape.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap();
        let bar = shapes
            .iter()
            .map(|clipped| clipped.shape.visual_bounding_rect())
            .find(|rect| {
                (rect.height() - crate::app::widgets::progress::BAR_HEIGHT).abs() < 0.5
                    && frames[0].contains_rect(*rect)
            })
            .unwrap();
        let above = title.top() - frames[0].top();
        let below = frames[0].bottom() - bar.bottom();
        assert!((above - below).abs() < 4.0, "{above} above, {below} below");
    }

    #[test]
    fn the_free_spot_is_the_band_under_the_picture_then_beside_it_then_another_pane() {
        let window = Rect::from_min_max(pos2(0.0, 30.0), pos2(1280.0, 800.0));
        let panel = Rect::from_min_max(pos2(0.0, 30.0), pos2(800.0, 800.0));
        let picture = Rect::from_min_max(pos2(80.0, 60.0), pos2(720.0, 700.0));
        assert_eq!(
            free_spot(window, panel, picture, 60.0),
            Some(Rect::from_min_max(pos2(0.0, 700.0), panel.max))
        );
        let tall = Rect::from_min_max(pos2(160.0, 40.0), pos2(480.0, 790.0));
        let beside = free_spot(window, panel, tall, 60.0).unwrap();
        assert_eq!(beside, Rect::from_min_max(pos2(480.0, 30.0), panel.max));
        let wide = Rect::from_min_max(pos2(10.0, 40.0), pos2(790.0, 790.0));
        let pane = free_spot(window, panel, wide, 60.0).unwrap();
        assert_eq!(pane.left(), 800.0);
        assert!(!pane.intersects(wide));
        assert_eq!(free_spot(panel, panel, wide, 60.0), None);
    }
}
