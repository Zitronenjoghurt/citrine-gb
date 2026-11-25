use crate::app::events::AppEvent;
use crate::app::widgets::action::dismiss_button;
use crate::app::widgets::progress::ProgressBar;
use crate::icons;
use crate::progress::Progress;
use egui::{
    Align2, Color32, Context, Id, Label, LayerId, Order, Rangef, Rect, RichText, Sense, Ui, Vec2,
    pos2, vec2,
};
use std::time::Duration;
use web_time::Instant;

const LIFETIME: f32 = 3.5;
const BUTTON_LIFETIME: f32 = 6.0;
const FADE: f32 = 0.15;
const HINT_LIFETIME: f32 = 3.0;
const READ_START: f32 = 2.0;
const CHARS_PER_SECOND: f32 = 15.0;

pub fn reading_time(text: &str, floor: f32) -> f32 {
    (READ_START + text.chars().count() as f32 / CHARS_PER_SECOND).max(floor)
}
const HINT_FADE: f32 = 0.3;
pub const MARGIN: f32 = 8.0;
const SPACING: f32 = 6.0;
const MAX_WIDTH: f32 = 420.0;
const MIN_WIDTH: f32 = 280.0;
const BANNER_WIDTH: f32 = 520.0;
const LOAD_SHOWS_AFTER: Duration = Duration::from_millis(400);
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const CARD_LIFETIME: f32 = 5.0;
const ROOM_SPEED: f32 = 180.0;

fn page_room_id() -> Id {
    Id::new("page_toast_room")
}

fn ease_page_room(ctx: &Context, room: f32, snap: bool) -> f32 {
    let last = ctx
        .data(|d| d.get_temp::<f32>(page_room_id()))
        .unwrap_or(0.0);
    let step = if ctx.current_pass_index() > 0 {
        0.0
    } else {
        ROOM_SPEED * ctx.input(|i| i.stable_dt).min(0.1)
    };
    let eased = if snap || crate::utils::reduced_motion() {
        room
    } else if room > last {
        (last + step).min(room)
    } else {
        (last - step).max(room)
    };
    if eased != room {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(page_room_id(), eased));
    eased
}

pub fn strip_room(ctx: &Context) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(page_room_id()))
        .unwrap_or(0.0)
}

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
    other: Option<(String, AppEvent)>,
    closed: Option<AppEvent>,
    needs_action: bool,
    load: Option<(&'static str, Progress)>,
    note: Option<String>,
    card: Option<Card>,
    drawn: Option<f32>,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct Card {
    pub badge: Option<egui::TextureHandle>,
    pub side: f32,
    pub title: String,
    pub detail: String,
    pub aside: String,
}

impl Toast {
    fn new(kind: Kind, text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            id: 0,
            kind,
            remaining: Some(reading_time(&text, LIFETIME)),
            text,
            age: 0.0,
            closing: None,
            fade: FADE,
            button: None,
            other: None,
            closed: None,
            needs_action: false,
            drawn: None,
            load: None,
            note: None,
            card: None,
        }
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn card(card: Card) -> Self {
        let mut toast = Self::new(Kind::Success, card.title.clone());
        let text = format!("{} {}", card.title, card.detail);
        toast.remaining = Some(reading_time(&text, CARD_LIFETIME));
        toast.card = Some(card);
        toast
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
            self.remaining = Some(reading_time(&self.text, BUTTON_LIFETIME));
        }
        self.button = Some((label.into(), event));
        self
    }

    pub fn or(&mut self, label: impl Into<String>, event: AppEvent) -> &mut Self {
        self.other = Some((label.into(), event));
        self
    }

    pub fn needs_action(&mut self) -> &mut Self {
        self.needs_action = true;
        self
    }

    pub fn on_close(&mut self, event: AppEvent) -> &mut Self {
        self.closed = Some(event);
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
        if hovered || self.needs_action {
            return;
        }
        if let Some(remaining) = &mut self.remaining {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.closing = Some(0.0);
            }
        }
    }

    fn repeats(&self, other: &Toast) -> bool {
        self.closing.is_none()
            && self.kind == other.kind
            && self.text == other.text
            && self.button.is_none()
            && self.other.is_none()
            && self.load.is_none()
            && self.card.is_none()
            && other.card.is_none()
    }

    fn comes_back_as(&self, other: &Toast) -> bool {
        self.closing.is_some()
            && self.needs_action
            && self.kind == other.kind
            && self.text == other.text
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
    page: Option<Rect>,
    strip: Option<Rect>,
    side: Option<f32>,
    clip: Option<Rect>,
    banner: Option<Rect>,
    spare: Option<Rect>,
    held: Option<(f32, usize)>,
    on_page: bool,
    dismissed: Vec<&'static str>,
    waiting: Vec<(&'static str, Instant)>,
    #[cfg(test)]
    shown_in: Option<Rect>,
}

impl Toasts {
    pub fn add(&mut self, mut toast: Toast) -> &mut Toast {
        if let Some(at) = self.toasts.iter().position(|shown| shown.repeats(&toast)) {
            let shown = &mut self.toasts[at];
            if shown.remaining.is_some() {
                shown.remaining = toast.remaining;
            }
            return shown;
        }
        if let Some(at) = self
            .toasts
            .iter()
            .position(|shown| shown.comes_back_as(&toast))
        {
            let shown = &mut self.toasts[at];
            toast.id = shown.id;
            toast.age = shown.age;
            toast.drawn = shown.drawn;
            *shown = toast;
            return shown;
        }
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

    pub fn load_note(&mut self, key: &'static str, note: &str) {
        for toast in &mut self.toasts {
            if toast.load.is_some_and(|(k, _)| k == key) && toast.note.as_deref() != Some(note) {
                toast.note = Some(note.to_owned());
            }
        }
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

    pub fn dismissed(&self, key: &'static str) -> bool {
        self.dismissed.contains(&key)
    }

    pub fn loads(&self) -> Vec<(&str, Progress)> {
        self.toasts
            .iter()
            .filter(|t| t.closing.is_none())
            .filter_map(|t| t.load.map(|(_, p)| (t.text.as_str(), p)))
            .collect()
    }

    pub fn dismiss_all_toasts(&mut self) {
        for toast in self.toasts.iter_mut().filter(|t| !t.needs_action) {
            toast.closing.get_or_insert(0.0);
        }
    }

    pub fn close(&mut self, text: &str) {
        for toast in self.toasts.iter_mut().filter(|t| t.text == text) {
            toast.closing.get_or_insert(0.0);
        }
    }

    pub fn place(&mut self, area: Rect) {
        self.area = Some(area);
    }

    pub fn place_picture(&mut self, picture: Rect) {
        self.picture = Some(picture);
    }

    pub fn place_page(&mut self, page: Rect) {
        self.page = Some(page);
    }

    pub fn place_strip(&mut self, strip: Rect, need: f32, side: f32) {
        self.side = Some(side);
        let height = need.max(strip.height());
        self.page = Some(Rect::from_min_size(strip.min, vec2(strip.width(), height)));
        self.strip = (height > strip.height()).then_some(strip);
    }

    pub fn place_banner(&mut self, banner: Rect) {
        self.banner = Some(banner);
    }

    pub fn place_spare(&mut self, spare: Rect) {
        self.spare = Some(spare);
    }

    fn page_room(&mut self, ctx: &Context) -> Option<f32> {
        let asking = (crate::app::mobile::sheet::above(ctx).is_some()
            || crate::app::widgets::confirm::open_lately(ctx))
            && !crate::app::widgets::confirm::answered_lately(ctx);
        if asking {
            return Some(strip_room(ctx)).filter(|held| *held > 0.0);
        }
        (self.on_page && !self.toasts.is_empty()).then(|| self.held_need(ctx))
    }

    pub fn page_strip(&mut self, ctx: &Context) -> (Option<f32>, f32) {
        let need = self.page_room(ctx);
        let room = ease_page_room(ctx, need.unwrap_or(0.0), !self.on_page);
        (need, room)
    }

    pub fn reserve(&mut self, ctx: &Context, free: Rect, picture: Rect) -> Option<f32> {
        self.room_needed(ctx).filter(|&height| {
            let need = vec2(MIN_WIDTH + MARGIN * 2.0, height);
            fitting(free, picture, need, Edge::Bottom).is_none()
        })
    }

    pub fn clear_column(
        &mut self,
        ctx: &Context,
        area: Rect,
        taken: &[Rect],
        narrowest: f32,
    ) -> Option<Rect> {
        let height = self.room_needed(ctx)?;
        let width = narrowest.min(MIN_WIDTH + MARGIN * 2.0);
        clear_column(area, taken, vec2(width, height))
    }

    pub fn room_needed(&mut self, ctx: &Context) -> Option<f32> {
        let height = self.held_need(ctx);
        let shown = !self.toasts.is_empty() && !crate::app::widgets::confirm::is_open(ctx);
        shown.then_some(height)
    }

    #[cfg(test)]
    pub fn press(&mut self, label: &str) -> Option<AppEvent> {
        let shows = |button: &Option<(String, AppEvent)>| {
            button.as_ref().is_some_and(|(shown, _)| shown == label)
        };
        let toast = self
            .toasts
            .iter_mut()
            .find(|toast| shows(&toast.button) || shows(&toast.other))?;
        toast.closing.get_or_insert(0.0);
        let pressed = if shows(&toast.button) {
            toast.button.take()
        } else {
            toast.other.take()
        };
        toast.button = None;
        toast.other = None;
        pressed.map(|(_, event)| event)
    }

    #[cfg(test)]
    pub fn press_close(&mut self, text: &str) -> Option<AppEvent> {
        let toast = self.toasts.iter_mut().find(|toast| toast.text == text)?;
        toast.closing.get_or_insert(0.0);
        toast.button = None;
        toast.other = None;
        toast.closed.take()
    }

    #[cfg(test)]
    pub fn age(&mut self, seconds: f32) {
        for toast in &mut self.toasts {
            toast.advance(seconds, false);
            toast.advance(toast.fade, false);
        }
        self.toasts.retain(|toast| !toast.gone());
    }

    #[cfg(test)]
    pub fn needing_action(&self) -> Vec<&str> {
        self.toasts
            .iter()
            .filter(|toast| toast.needs_action && toast.closing.is_none())
            .map(|toast| toast.text.as_str())
            .collect()
    }

    #[cfg(test)]
    pub fn button_labels(&self) -> Vec<&str> {
        self.toasts
            .iter()
            .flat_map(|toast| [&toast.button, &toast.other])
            .filter_map(|button| button.as_ref().map(|(label, _)| label.as_str()))
            .collect()
    }

    #[cfg(test)]
    pub fn shown_in(&self) -> Option<Rect> {
        self.shown_in
    }

    #[cfg(test)]
    pub fn rects(&self, ctx: &Context) -> Vec<Rect> {
        self.toasts
            .iter()
            .filter_map(|toast| ctx.memory(|mem| mem.area_rect(toast_id(toast.id))))
            .map(|rect| self.clip.map_or(rect, |clip| rect.intersect(clip)))
            .collect()
    }

    pub fn texts(&self) -> Vec<&str> {
        self.toasts
            .iter()
            .map(|toast| toast.text.as_str())
            .collect()
    }

    pub fn is_open(&self, text: &str) -> bool {
        self.toasts
            .iter()
            .any(|toast| toast.closing.is_none() && toast.text == text)
    }

    #[cfg(test)]
    pub fn open_texts(&self) -> Vec<&str> {
        self.toasts
            .iter()
            .filter(|toast| toast.closing.is_none())
            .map(|toast| toast.text.as_str())
            .collect()
    }

    fn held_need(&mut self, ctx: &Context) -> f32 {
        if self.toasts.is_empty() {
            self.held = None;
            return 0.0;
        }
        let count = self
            .toasts
            .iter()
            .filter(|toast| toast.closing.is_none())
            .count();
        if count == 0 {
            return self
                .held
                .map_or_else(|| self.stack_height(ctx) + MARGIN * 2.0, |(held, _)| held);
        }
        let need = self.stack_height(ctx) + MARGIN * 2.0;
        let held = match self.held {
            Some((held, most)) if count < most => (held.max(need), most),
            _ => (need, count),
        };
        self.held = Some(held);
        held.0
    }

    fn stack_height(&self, ctx: &Context) -> f32 {
        let heights: Vec<f32> = self
            .toasts
            .iter()
            .filter(|toast| toast.closing.is_none())
            .map(|toast| toast.drawn.unwrap_or_else(|| toast_height(ctx, toast)))
            .collect();
        heights.iter().sum::<f32>() + SPACING * heights.len().saturating_sub(1) as f32
    }

    pub fn show(&mut self, ctx: &Context) -> Vec<AppEvent> {
        let mut events = Vec::new();
        let layers: Vec<LayerId> = self
            .toasts
            .iter()
            .map(|toast| LayerId::new(Order::Middle, toast_id(toast.id)))
            .collect();
        ctx.data_mut(|d| d.insert_temp(layers_id(), layers));
        let game = self.area.take();
        let picture = self.picture.take();
        let page = self.page.take();
        let strip = self.strip.take();
        let side = self.side.take().unwrap_or(MARGIN);
        self.clip = None;
        let banner = self.banner.take();
        let spare = self.spare.take();
        let need = vec2(MIN_WIDTH + MARGIN * 2.0, self.held_need(ctx));
        self.on_page = game.is_none();
        let sheet = crate::app::mobile::sheet::above(ctx);
        let spot = match game {
            _ if sheet.is_some() => sheet.map(|rect| Spot {
                rect,
                edge: Edge::Bottom,
            }),
            Some(game) => {
                let game = banner.map_or(game, |banner| without(game, banner));
                Some(spare_spot(game, picture, spare, need))
            }
            None => page.map(|page| Spot {
                rect: page,
                edge: Edge::Bottom,
            }),
        };
        let Some(Spot { rect: area, edge }) = spot else {
            return events;
        };
        #[cfg(test)]
        {
            self.shown_in = game.map(|game| banner.map_or(game, |banner| without(game, banner)));
        }
        if self.toasts.is_empty() || crate::app::widgets::confirm::is_open(ctx) {
            return events;
        }

        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let on_page = game.is_none() && sheet.is_none();
        let room = if on_page {
            let frame = egui::Frame::popup(&ctx.style());
            let stroke = frame.total_margin().sum().x - frame.inner_margin.sum().x;
            (area.width() - side * 2.0 - stroke).max(0.0)
        } else {
            (area.width() - MARGIN * 2.0).max(0.0)
        };
        let width = room.min(MAX_WIDTH);
        self.clip = strip.filter(|_| on_page);
        let clip = self.clip;
        let (mut edge_y, pivot, step) = match edge {
            Edge::Bottom => {
                let stack = self.stack_height(ctx);
                let gap = picture
                    .filter(|picture| {
                        picture.left() < area.right() && picture.right() > area.left()
                    })
                    .map(|picture| area.bottom() - picture.bottom())
                    .filter(|strip| *strip >= stack && *strip < stack + MARGIN * 2.0)
                    .map_or(MARGIN, |strip| (strip - stack) / 2.0);
                (area.bottom() - gap, Align2::CENTER_BOTTOM, -1.0)
            }
            Edge::Top => (area.top() + MARGIN, Align2::CENTER_TOP, 1.0),
        };

        for toast in self.toasts.iter_mut().rev() {
            let response = egui::Area::new(toast_id(toast.id))
                .order(if sheet.is_some() {
                    Order::Tooltip
                } else {
                    Order::Middle
                })
                .sense(Sense::hover())
                .fixed_pos(pos2(area.center().x, edge_y))
                .pivot(pivot)
                .constrain(false)
                .show(ctx, |ui| {
                    ui.set_opacity(toast.opacity());
                    if let Some(clip) = clip {
                        ui.set_clip_rect(clip);
                    }
                    let line = line_width(ui, toast);
                    let width = if on_page {
                        width.max(line).min(room)
                    } else if line > width && line <= room {
                        line
                    } else {
                        width
                    };
                    egui::Frame::popup(ui.style())
                        .show(ui, |ui| body(ui, toast, width))
                        .inner
                });
            match response.inner {
                Clicked::Button => {
                    toast.closing.get_or_insert(0.0);
                    events.extend(toast.button.take().map(|(_, event)| event));
                    toast.other = None;
                }
                Clicked::Other => {
                    toast.closing.get_or_insert(0.0);
                    events.extend(toast.other.take().map(|(_, event)| event));
                    toast.button = None;
                }
                Clicked::Close => {
                    toast.closing.get_or_insert(0.0);
                    events.extend(toast.closed.take());
                    toast.button = None;
                    toast.other = None;
                    if let Some((key, _)) = toast.load {
                        self.dismissed.push(key);
                    }
                }
                Clicked::Nothing => {}
            }
            toast.drawn = Some(response.response.rect.height());
            let hovered = pointer.is_some_and(|p| response.response.rect.contains(p));
            toast.advance(dt, hovered);
            edge_y += step * (response.response.rect.height() + SPACING);
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
    let content = if let Some(card) = &toast.card {
        let font = egui::TextStyle::Body.resolve(&style);
        let line = ctx.fonts_mut(|fonts| fonts.row_height(&font));
        card.side.max(line * 2.0)
    } else if toast.load.is_some() {
        let font = egui::TextStyle::Body.resolve(&style);
        let line = ctx.fonts_mut(|fonts| fonts.row_height(&font));
        let note = if toast.note.is_some() {
            line + style.spacing.item_spacing.y
        } else {
            0.0
        };
        crate::app::widgets::progress::ProgressBar::tallest(line).max(row) + note
    } else if toast.other.is_some() {
        row * 3.0 + style.spacing.item_spacing.y
    } else {
        row
    };
    content + frame_height(ctx)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub rect: Rect,
    pub edge: Edge,
}

pub fn free_spot(panel: Rect, picture: Rect, need: Vec2) -> Option<Rect> {
    fitting(panel, picture, need, Edge::Bottom).map(|spot| spot.rect)
}

fn fitting(panel: Rect, picture: Rect, need: Vec2, prefer: Edge) -> Option<Spot> {
    let below = Spot {
        rect: Rect::from_min_max(pos2(panel.left(), picture.bottom()), panel.max),
        edge: Edge::Bottom,
    };
    let right = Spot {
        rect: Rect::from_min_max(pos2(picture.right(), panel.top()), panel.max),
        edge: Edge::Bottom,
    };
    let left = Spot {
        rect: Rect::from_min_max(panel.min, pos2(picture.left(), panel.bottom())),
        edge: Edge::Bottom,
    };
    let above = Spot {
        rect: Rect::from_min_max(panel.min, pos2(panel.right(), picture.top())),
        edge: Edge::Top,
    };
    let order = match prefer {
        Edge::Bottom => [below, right, left, above],
        Edge::Top => [above, below, right, left],
    };
    order
        .into_iter()
        .find(|spot| spot.rect.width() >= need.x && spot.rect.height() >= need.y)
}

fn clear_column(area: Rect, taken: &[Rect], need: Vec2) -> Option<Rect> {
    let band = area.with_min_y(area.bottom() - need.y);
    let mut blocked: Vec<Rangef> = taken
        .iter()
        .filter(|rect| rect.intersects(band))
        .map(|rect| rect.x_range())
        .collect();
    blocked.sort_by(|a, b| a.min.total_cmp(&b.min));
    let mut gaps = Vec::new();
    let mut left = area.left();
    for range in blocked {
        if range.min > left {
            gaps.push(Rangef::new(left, range.min));
        }
        left = left.max(range.max);
    }
    if area.right() > left {
        gaps.push(Rangef::new(left, area.right()));
    }
    gaps.into_iter()
        .filter(|gap| gap.span() >= need.x)
        .map(|gap| {
            let top = taken
                .iter()
                .filter(|rect| rect.max.x > gap.min && rect.min.x < gap.max)
                .map(|rect| rect.bottom())
                .fold(area.top(), f32::max);
            Rect::from_x_y_ranges(gap, top..=area.bottom())
        })
        .filter(|column| column.height() >= need.y)
        .max_by(|a, b| a.width().total_cmp(&b.width()))
}

fn spare_spot(panel: Rect, picture: Option<Rect>, spare: Option<Rect>, need: Vec2) -> Spot {
    match spare.filter(|spare| spare.height() >= need.y - 0.5) {
        Some(spare) => Spot {
            rect: spare,
            edge: Edge::Bottom,
        },
        None => float_spot(panel, picture, need, Edge::Bottom),
    }
}

pub fn float_spot(panel: Rect, picture: Option<Rect>, need: Vec2, prefer: Edge) -> Spot {
    let band = |edge| {
        let height = need.y.min(panel.height());
        let rect = match edge {
            Edge::Top => panel.with_max_y(panel.top() + height),
            Edge::Bottom => panel.with_min_y(panel.bottom() - height),
        };
        Spot { rect, edge }
    };
    let Some(picture) = picture else {
        return band(prefer);
    };
    if let Some(spot) = fitting(panel, picture, need, prefer) {
        return spot;
    }
    let gap_above = picture.top() - panel.top();
    let gap_below = panel.bottom() - picture.bottom();
    let edge = if gap_above > gap_below {
        Edge::Top
    } else if gap_below > gap_above {
        Edge::Bottom
    } else {
        prefer
    };
    band(edge)
}

fn toast_id(toast: u64) -> Id {
    Id::new(("toast", toast))
}

fn layers_id() -> Id {
    Id::new("toast layers")
}

pub fn layers(ctx: &Context) -> Vec<LayerId> {
    ctx.data(|d| d.get_temp::<Vec<LayerId>>(layers_id()))
        .unwrap_or_default()
}

pub fn without(area: Rect, banner: Rect) -> Rect {
    if banner.center().y < area.center().y {
        area.with_min_y(banner.bottom().min(area.bottom()))
    } else {
        area.with_max_y(banner.top().max(area.top()))
    }
}

pub fn banner<R>(
    ctx: &Context,
    id: Id,
    panel: Rect,
    picture: Option<Rect>,
    add: impl FnOnce(&mut Ui) -> R,
) -> (R, Rect) {
    let width = (panel.width() - MARGIN * 2.0).clamp(0.0, BANNER_WIDTH);
    let height = ctx.memory(|mem| mem.area_rect(id)).map_or_else(
        || ctx.style().spacing.interact_size.y * 2.0,
        |rect| rect.height(),
    );
    let spot = float_spot(
        panel,
        picture,
        vec2(width, height + MARGIN * 2.0),
        Edge::Top,
    );
    let (y, pivot) = match spot.edge {
        Edge::Top => (spot.rect.top() + MARGIN, Align2::CENTER_TOP),
        Edge::Bottom => (spot.rect.bottom() - MARGIN, Align2::CENTER_BOTTOM),
    };
    let shown = egui::Area::new(id)
        .order(Order::Middle)
        .fixed_pos(pos2(panel.center().x, y))
        .pivot(pivot)
        .constrain(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .show(ui, |ui| {
                    ui.set_width(width - ui.spacing().menu_margin.sum().x);
                    add(ui)
                })
                .inner
        });
    let rect = shown.response.rect.expand(MARGIN);
    (shown.inner, rect)
}

enum Clicked {
    Nothing,
    Button,
    Other,
    Close,
}

fn card_body(ui: &mut Ui, card: &Card, width: f32) -> Clicked {
    ui.horizontal(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let inner = width - ui.spacing().menu_margin.sum().x;
        let side = vec2(card.side, card.side);
        match &card.badge {
            Some(badge) => {
                ui.add(egui::Image::new((badge.id(), side)).corner_radius(4));
            }
            None if card.side > 0.0 => {
                let (rect, _) = ui.allocate_exact_size(side, egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 4, ui.visuals().widgets.inactive.bg_fill);
            }
            None => {}
        }
        let aside_width = text_width(ui, &card.aside, egui::TextStyle::Body);
        let close_width = text_width(ui, icons::X, egui::TextStyle::Body);
        let room = (inner - card.side - aside_width - close_width - spacing * 3.0).max(0.0);
        let font = egui::TextStyle::Body.resolve(ui.style());
        let line = ui.fonts_mut(|f| f.row_height(&font));
        ui.allocate_ui_with_layout(
            vec2(room, card.side.max(line * 2.0)),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(room);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add_space(((card.side - line * 2.0) / 2.0).max(0.0));
                ui.add(Label::new(RichText::new(&card.title).strong()).truncate());
                ui.add(Label::new(RichText::new(&card.detail).weak()).truncate());
            },
        );
        ui.label(RichText::new(&card.aside).weak());
        if close_button(ui, DISMISS).clicked() {
            Clicked::Close
        } else {
            Clicked::Nothing
        }
    })
    .inner
}

const DISMISS: &str = "Dismiss";
const HIDE: &str = "Hide";

fn close_button(ui: &mut Ui, name: &str) -> egui::Response {
    let response = ui.add(dismiss_button());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    response.on_hover_text(name)
}

fn kind_icon(ui: &Ui, kind: Kind) -> (&'static str, Color32) {
    match kind {
        Kind::Info => (
            icons::INFO,
            crate::app::widgets::info_mark::mark_color(ui.visuals(), false),
        ),
        Kind::Success => (
            icons::CHECK_CIRCLE,
            if ui.visuals().dark_mode {
                Color32::from_rgb(130, 210, 130)
            } else {
                Color32::from_rgb(30, 130, 60)
            },
        ),
        Kind::Error => (icons::WARNING_CIRCLE, ui.visuals().error_fg_color),
    }
}

struct Chrome {
    button: f32,
    total: f32,
}

fn chrome(ui: &Ui, toast: &Toast) -> Chrome {
    let spacing = ui.spacing().item_spacing.x;
    let (icon, _) = kind_icon(ui, toast.kind);
    let icon_width = text_width(ui, icon, egui::TextStyle::Body);
    let close_width = text_width(ui, icons::X, egui::TextStyle::Body);
    let button = toast.button.as_ref().map_or(0.0, |(label, _)| {
        crate::app::widgets::homebrew::action_width(ui, &[label.as_str()]) + spacing
    });
    Chrome {
        button,
        total: ui.spacing().menu_margin.sum().x + icon_width + close_width + button + spacing * 2.0,
    }
}

fn text_width(ui: &Ui, text: &str, style: egui::TextStyle) -> f32 {
    ui.fonts_mut(|f| {
        f.layout_no_wrap(text.to_owned(), style.resolve(ui.style()), Color32::WHITE)
            .size()
            .x
    })
}

fn line_width(ui: &Ui, toast: &Toast) -> f32 {
    if toast.card.is_some() || toast.load.is_some() {
        return 0.0;
    }
    let Chrome { button, total } = chrome(ui, toast);
    let buttons = if toast.other.is_some() {
        let spacing = ui.spacing().item_spacing.x;
        let padding = ui.spacing().button_padding.x;
        [&toast.button, &toast.other]
            .into_iter()
            .flatten()
            .map(|(label, _)| {
                text_width(ui, label, egui::TextStyle::Button) + padding * 2.0 + spacing
            })
            .sum()
    } else {
        button
    };
    text_width(ui, &toast.text, egui::TextStyle::Body) + total - button + buttons + 1.0
}

fn body(ui: &mut Ui, toast: &Toast, width: f32) -> Clicked {
    if let Some(card) = &toast.card {
        return card_body(ui, card, width);
    }
    let (icon, color) = kind_icon(ui, toast.kind);
    if toast.other.is_some() {
        return choice_body(ui, toast, width, icon, color);
    }
    let Chrome {
        button: button_width,
        total,
    } = chrome(ui, toast);

    ui.horizontal(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let room = (width - total).max(0.0);

        ui.label(RichText::new(icon).color(color));
        if let Some((_, progress)) = toast.load {
            let bar = ProgressBar::new(&toast.text, progress);
            let height = bar.height(ui, room);
            let layout = egui::Layout::top_down(egui::Align::Min);
            ui.allocate_ui_with_layout(vec2(room, height), layout, |ui| {
                ui.set_width(room);
                ui.add(bar);
                if let Some(note) = &toast.note {
                    ui.add(Label::new(RichText::new(note).weak()).wrap());
                }
            });
            return if close_button(ui, HIDE).clicked() {
                Clicked::Close
            } else {
                Clicked::Nothing
            };
        }
        ui.allocate_ui(vec2(room, ui.spacing().interact_size.y), |ui| {
            ui.set_width(room);
            ui.set_min_height(ui.spacing().interact_size.y);
            ui.add(Label::new(toast.text.as_str()).wrap());
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
        if close_button(ui, DISMISS).clicked() {
            clicked = Clicked::Close;
        }
        clicked
    })
    .inner
}

fn choice_body(ui: &mut Ui, toast: &Toast, width: f32, icon: &str, color: Color32) -> Clicked {
    let inner = width - ui.spacing().menu_margin.sum().x;
    ui.set_width(inner);
    if line_width(ui, toast) <= width {
        return ui
            .horizontal(|ui| {
                ui.label(RichText::new(icon).color(color));
                ui.label(toast.text.as_str());
                let row = vec2(ui.available_width(), ui.spacing().interact_size.y);
                ui.allocate_ui_with_layout(
                    row,
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let mut clicked = Clicked::Nothing;
                        if ui.add(dismiss_button()).on_hover_text("Dismiss").clicked() {
                            clicked = Clicked::Close;
                        }
                        if let Some((label, _)) = &toast.button
                            && ui.button(label.as_str()).clicked()
                        {
                            clicked = Clicked::Button;
                        }
                        if let Some((label, _)) = &toast.other
                            && ui.button(label.as_str()).clicked()
                        {
                            clicked = Clicked::Other;
                        }
                        clicked
                    },
                )
                .inner
            })
            .inner;
    }
    let closed = ui
        .horizontal(|ui| {
            let spacing = ui.spacing().item_spacing.x;
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
            let room = (inner - icon_width - close_width - spacing * 2.0).max(0.0);
            ui.label(RichText::new(icon).color(color));
            ui.allocate_ui(vec2(room, ui.spacing().interact_size.y), |ui| {
                ui.set_width(room);
                ui.add(Label::new(toast.text.as_str()).wrap());
            });
            ui.add(dismiss_button()).on_hover_text("Dismiss").clicked()
        })
        .inner;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let mut clicked = if closed {
            Clicked::Close
        } else {
            Clicked::Nothing
        };
        if let Some((label, _)) = &toast.button
            && ui.button(label.as_str()).clicked()
        {
            clicked = Clicked::Button;
        }
        if let Some((label, _)) = &toast.other
            && ui.button(label.as_str()).clicked()
        {
            clicked = Clicked::Other;
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
    fn a_clear_column_runs_from_the_bottom_up_to_the_lowest_thing_above_it() {
        let area = Rect::from_min_size(egui::Pos2::ZERO, vec2(640.0, 400.0));
        let taken = [
            Rect::from_min_max(pos2(0.0, 55.0), pos2(378.0, 338.0)),
            Rect::from_min_max(pos2(14.0, 270.0), pos2(130.0, 384.0)),
            Rect::from_min_max(pos2(401.0, 361.0), pos2(488.0, 392.0)),
            Rect::from_min_max(pos2(557.0, 348.0), pos2(595.0, 386.0)),
        ];
        assert_eq!(
            clear_column(area, &taken, vec2(242.0, 44.0)),
            Some(Rect::from_min_max(pos2(130.0, 338.0), pos2(401.0, 400.0)))
        );
        assert_eq!(clear_column(area, &taken, vec2(300.0, 44.0)), None);
        assert_eq!(clear_column(area, &taken, vec2(242.0, 70.0)), None);
    }

    #[test]
    fn an_info_toast_draws_its_mark_in_the_info_marks_grey() {
        let ctx = egui::Context::default();
        for visuals in [egui::Visuals::light(), egui::Visuals::dark()] {
            ctx.set_visuals(visuals);
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let (icon, color) = kind_icon(ui, Kind::Info);
                    assert_eq!(icon, icons::INFO);
                    assert_eq!(
                        color,
                        crate::app::widgets::info_mark::mark_color(ui.visuals(), false)
                    );
                });
            });
        }
    }

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
    fn a_load_toast_says_why_it_waits_and_its_close_button_is_named_hide() {
        let ctx = egui::Context::default();
        let mut toasts = Toasts::default();
        let text = "Getting your save from Steam Deck";
        let note = "The game starts when it arrives, or after 4 seconds";
        toasts.load("hold", text, Progress::unknown(), start());
        toasts.load("hold", text, Progress::unknown(), later());
        toasts.load_note("hold", note);
        let area = Rect::from_min_max(pos2(0.0, 48.0), pos2(412.0, 860.0));
        let frame = |toasts: &mut Toasts, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(412.0, 915.0))),
                events,
                ..Default::default()
            };
            ctx.run(input, |ctx| {
                crate::app::mobile::touch_spacing(ctx, true);
                toasts.place(area);
                toasts.show(ctx);
            })
        };
        let mut output = frame(&mut toasts, Vec::new());
        for _ in 0..2 {
            output = frame(&mut toasts, Vec::new());
        }
        let shown = |wanted: &str| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shape) if shape.galley.text() == wanted => {
                        Some(clipped.shape.visual_bounding_rect())
                    }
                    _ => None,
                })
        };
        let rect = toasts.rects(&ctx)[0];
        let said = shown(note).expect("the toast says why the game waits");
        assert!(rect.contains_rect(said), "{rect:?} {said:?}");
        let close = shown(icons::X).expect("a close button");
        let output = frame(&mut toasts, crate::app::tests::tap(close.center()));
        let named = output.platform_output.events.iter().any(|event| {
            matches!(event, egui::output::OutputEvent::Clicked(info)
                if info.label.as_deref() == Some(HIDE))
        });
        assert!(named, "{:?}", output.platform_output.events);
        assert!(toasts.dismissed("hold"));
    }

    #[test]
    fn a_toast_under_a_tight_game_picture_keeps_equal_gaps() {
        let ctx = egui::Context::default();
        let mut toasts = Toasts::default();
        let text = "Getting your save from Steam Deck";
        toasts.load("hold", text, Progress::unknown(), start());
        toasts.load("hold", text, Progress::unknown(), later());
        toasts.load_note(
            "hold",
            "The game starts when it arrives, or after 4 seconds",
        );
        let panel = Rect::from_min_max(pos2(0.0, 40.0), pos2(640.0, 790.0));
        let frame = |toasts: &mut Toasts, picture: Rect| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 800.0))),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                toasts.place(panel);
                toasts.place_picture(picture);
                toasts.show(ctx);
            });
        };
        let tall = Rect::from_min_max(pos2(20.0, 50.0), pos2(620.0, 600.0));
        for _ in 0..3 {
            frame(&mut toasts, tall);
        }
        let roomy = toasts.rects(&ctx)[0];
        assert!(
            (panel.bottom() - roomy.bottom() - MARGIN).abs() < 0.5,
            "{roomy:?}"
        );
        let height = roomy.height();
        let tight = tall.with_max_y(panel.bottom() - height - MARGIN);
        for _ in 0..3 {
            frame(&mut toasts, tight);
        }
        let toast = toasts.rects(&ctx)[0];
        let above = toast.top() - tight.bottom();
        let below = panel.bottom() - toast.bottom();
        assert!(above > 2.0, "{above} above the toast");
        assert!((above - below).abs() <= 1.0, "{above} above, {below} below");
    }

    #[test]
    fn a_notice_repeated_every_frame_shows_once_per_cause() {
        let mut toasts = Toasts::default();
        for _ in 0..120 {
            toasts.error("Emulation error: no snapshot");
            toasts.error("Couldn't answer the invite");
        }
        assert_eq!(
            toasts.open_texts(),
            ["Emulation error: no snapshot", "Couldn't answer the invite"]
        );
        toasts.toasts[0].closing = Some(0.0);
        toasts.error("Emulation error: no snapshot");
        assert_eq!(
            toasts.open_texts(),
            ["Couldn't answer the invite", "Emulation error: no snapshot"]
        );
        let mut undos = Toasts::default();
        for _ in 0..2 {
            undos
                .info("HUD removed")
                .button("Undo", AppEvent::OpenTab { tab: Tab::Media });
        }
        assert_eq!(undos.open_texts(), ["HUD removed", "HUD removed"]);
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
    fn a_toast_that_needs_action_stays_until_pressed_or_closed() {
        let mut toasts = Toasts::default();
        toasts
            .error("Add a file to start the game")
            .button("Add", AppEvent::OpenTab { tab: Tab::Media })
            .needs_action()
            .duration(Some(Duration::from_secs(1)));
        toasts.toasts[0].advance(600.0, false);
        toasts.dismiss_all_toasts();
        assert!(toasts.toasts[0].closing.is_none());
        toasts.close("Add a file to start the game");
        assert!(toasts.toasts[0].closing.is_some());
    }

    #[test]
    fn a_long_notice_stays_long_enough_to_read() {
        let stop = "Recording stopped because this device couldn't keep up";
        let mut long = Toast::new(Kind::Error, stop);
        long.advance(5.5, false);
        assert!(long.closing.is_none());
        long.advance(0.2, false);
        assert!(long.closing.is_some());
        let mut short = Toast::new(Kind::Success, "Saved");
        short.advance(LIFETIME + 0.01, false);
        assert!(short.closing.is_some());
        let mut with_button = Toast::info(format!("{stop}. Saved the part recorded so far"));
        with_button.button("Open", AppEvent::OpenTab { tab: Tab::Media });
        with_button.advance(BUTTON_LIFETIME + 0.01, false);
        assert!(with_button.closing.is_none());
        with_button.advance(1.8, false);
        assert!(with_button.closing.is_some());
    }

    #[test]
    fn a_notice_with_two_choices_stays_until_one_is_pressed() {
        let mut toasts = Toasts::default();
        toasts
            .info("Pick one")
            .button("Use", AppEvent::OpenTab { tab: Tab::Media })
            .or("Delete", AppEvent::OpenTab { tab: Tab::Games })
            .needs_action();
        toasts.age(600.0);
        toasts.dismiss_all_toasts();
        assert_eq!(toasts.needing_action(), vec!["Pick one"]);
        assert!(matches!(
            toasts.press("Delete"),
            Some(AppEvent::OpenTab { tab: Tab::Games })
        ));
        assert!(toasts.press("Use").is_none());
        toasts.age(1.0);
        assert!(toasts.texts().is_empty());
    }

    #[test]
    fn a_notice_brought_back_while_it_fades_takes_its_old_place() {
        let mut toasts = Toasts::default();
        let notice = |toasts: &mut Toasts| {
            toasts
                .info("Pick one")
                .button("Use", AppEvent::OpenTab { tab: Tab::Media })
                .or("Delete", AppEvent::OpenTab { tab: Tab::Games })
                .needs_action();
        };
        notice(&mut toasts);
        let first = toasts.toasts[0].id;
        assert!(toasts.press("Delete").is_some());
        assert!(toasts.needing_action().is_empty());
        notice(&mut toasts);
        assert_eq!(toasts.texts(), vec!["Pick one"]);
        assert_eq!(toasts.needing_action(), vec!["Pick one"]);
        assert_eq!(toasts.toasts[0].id, first);
        assert_eq!(toasts.button_labels(), vec!["Use", "Delete"]);
    }

    #[test]
    fn closing_a_notice_with_two_choices_sends_its_close_event() {
        let mut toasts = Toasts::default();
        toasts
            .info("Pick one")
            .button("Use", AppEvent::OpenTab { tab: Tab::Media })
            .or("Delete", AppEvent::OpenTab { tab: Tab::Games })
            .on_close(AppEvent::OpenTab { tab: Tab::Saves })
            .needs_action();
        assert!(matches!(
            toasts.press_close("Pick one"),
            Some(AppEvent::OpenTab { tab: Tab::Saves })
        ));
        assert!(toasts.needing_action().is_empty());
        assert!(toasts.button_labels().is_empty());
    }

    #[test]
    fn a_toast_with_an_optional_button_still_fades() {
        let mut toast = Toast::info("Saved");
        toast.button("Open", AppEvent::OpenTab { tab: Tab::Media });
        toast.advance(BUTTON_LIFETIME - 0.01, false);
        assert!(toast.closing.is_none());
        toast.advance(0.02, false);
        assert!(toast.closing.is_some());
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
    fn a_long_notice_wraps_inside_its_toast_instead_of_cutting_off() {
        let ctx = egui::Context::default();
        let mut toasts = Toasts::default();
        let text = "Couldn't load Slot 1. It was saved by a different version of Citrine.";
        toasts.error(text);
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
        let frame = ctx
            .memory(|mem| mem.area_rect(Id::new(("toast", toasts.toasts[0].id))))
            .unwrap();
        let (galley, bounds) = output
            .unwrap()
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text() == text => {
                    Some((shape.galley.clone(), clipped.shape.visual_bounding_rect()))
                }
                _ => None,
            })
            .unwrap();
        assert!(!galley.elided, "{text}");
        assert!(galley.rows.len() > 1, "{}", galley.rows.len());
        assert!(frame.contains_rect(bounds), "{frame:?} {bounds:?}");
        assert!(
            (toasts.toasts[0].drawn.unwrap() - frame.height()).abs() < 0.5,
            "{frame:?}"
        );
    }

    #[test]
    fn a_page_notice_on_desktop_takes_one_line_when_the_page_has_room() {
        let ctx = egui::Context::default();
        let mut toasts = Toasts::default();
        let text = "'odd.bin' is 384 bytes. A GB boot ROM is 256 bytes.";
        toasts
            .error(text)
            .button("Pick another", AppEvent::OpenTab { tab: Tab::Media });
        let page = Rect::from_min_max(pos2(0.0, 48.0), pos2(1280.0, 800.0));
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 800.0))),
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(input(), |ctx| {
                toasts.place_page(page);
                toasts.show(ctx);
            }));
        }
        let galley = output
            .unwrap()
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text() == text => {
                    Some(shape.galley.clone())
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(galley.rows.len(), 1, "{text}");
        assert!(!galley.elided, "{text}");
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
    fn a_popup_takes_the_free_band_under_the_picture_then_beside_it() {
        let panel = Rect::from_min_max(pos2(0.0, 30.0), pos2(800.0, 800.0));
        let need = vec2(MIN_WIDTH + MARGIN * 2.0, 60.0);
        let picture = Rect::from_min_max(pos2(80.0, 60.0), pos2(720.0, 700.0));
        assert_eq!(
            float_spot(panel, Some(picture), need, Edge::Bottom),
            Spot {
                rect: Rect::from_min_max(pos2(0.0, 700.0), panel.max),
                edge: Edge::Bottom
            }
        );
        let tall = Rect::from_min_max(pos2(160.0, 40.0), pos2(480.0, 790.0));
        assert_eq!(
            float_spot(panel, Some(tall), need, Edge::Bottom).rect,
            Rect::from_min_max(pos2(480.0, 30.0), panel.max)
        );
        assert_eq!(
            float_spot(panel, Some(picture), need, Edge::Top).rect,
            Rect::from_min_max(pos2(0.0, 700.0), panel.max)
        );
    }

    #[test]
    fn without_room_a_popup_floats_over_the_picture_edge_that_leaves_more_of_it() {
        let panel = Rect::from_min_max(pos2(0.0, 30.0), pos2(800.0, 800.0));
        let need = vec2(MIN_WIDTH + MARGIN * 2.0, 60.0);
        let low = Rect::from_min_max(pos2(10.0, 70.0), pos2(790.0, 790.0));
        assert_eq!(
            float_spot(panel, Some(low), need, Edge::Bottom),
            Spot {
                rect: Rect::from_min_max(pos2(0.0, 30.0), pos2(800.0, 90.0)),
                edge: Edge::Top
            }
        );
        let high = Rect::from_min_max(pos2(10.0, 40.0), pos2(790.0, 760.0));
        assert_eq!(
            float_spot(panel, Some(high), need, Edge::Top),
            Spot {
                rect: Rect::from_min_max(pos2(0.0, 740.0), panel.max),
                edge: Edge::Bottom
            }
        );
        let full = Rect::from_min_max(pos2(10.0, 30.0), pos2(790.0, 800.0));
        assert_eq!(
            float_spot(panel, Some(full), need, Edge::Top).edge,
            Edge::Top
        );
        assert_eq!(
            float_spot(panel, None, need, Edge::Bottom).rect,
            Rect::from_min_max(pos2(0.0, 740.0), panel.max)
        );
    }

    #[test]
    fn the_spot_holds_its_height_until_every_toast_is_gone() {
        let ctx = Context::default();
        let mut toasts = Toasts::default();
        assert_eq!(toasts.held_need(&ctx), 0.0);
        toasts.info("one");
        let one = toasts.held_need(&ctx);
        toasts.info("two");
        let two = toasts.held_need(&ctx);
        assert!(two > one);
        toasts.toasts.remove(0);
        assert_eq!(toasts.held_need(&ctx), two);
        toasts.toasts.clear();
        assert_eq!(toasts.held_need(&ctx), 0.0);
    }

    #[test]
    fn the_spot_shrinks_to_a_toast_drawn_lower_than_its_guess() {
        let ctx = Context::default();
        let mut toasts = Toasts::default();
        toasts
            .info("Mika's game would have saved over yours.")
            .button("Use", AppEvent::OpenTab { tab: Tab::Media })
            .or("Delete", AppEvent::OpenTab { tab: Tab::Media });
        let guess = toasts.held_need(&ctx);
        let drawn = guess - MARGIN * 2.0 - 12.0;
        toasts.toasts[0].drawn = Some(drawn);
        assert_eq!(toasts.held_need(&ctx), drawn + MARGIN * 2.0);
        toasts.info("Saved");
        let two = toasts.held_need(&ctx);
        toasts.toasts.remove(1);
        assert_eq!(toasts.held_need(&ctx), two);
    }

    #[test]
    fn a_toast_with_a_button_leaves_room_at_the_page_end() {
        let ctx = egui::Context::default();
        let page = Rect::from_min_max(pos2(0.0, 0.0), pos2(412.0, 800.0));
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(412.0, 915.0))),
            ..Default::default()
        };
        let mut toasts = Toasts::default();
        toasts
            .success("You're playing Mika's save.")
            .button("Undo", AppEvent::OpenTab { tab: Tab::Media });
        let mut need = 0.0;
        for _ in 0..3 {
            let _ = ctx.run(input(), |ctx| {
                need = toasts.page_room(ctx).unwrap_or(0.0);
                toasts.place_page(page);
                toasts.show(ctx);
            });
        }
        let top = toasts.rects(&ctx)[0].top();
        assert!(
            need >= page.bottom() - top,
            "room {need} for a toast from {top}"
        );
    }

    #[test]
    fn on_a_wide_pane_a_notice_and_a_toast_take_one_line_and_fit_a_phone() {
        let heights = |width: f32| {
            let ctx = egui::Context::default();
            let page = Rect::from_min_max(pos2(0.0, 0.0), pos2(width, 800.0));
            let input = || egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 800.0))),
                ..Default::default()
            };
            let mut toasts = Toasts::default();
            toasts
                .info("Mika's saves didn't replace yours.")
                .button(
                    "Use Mika's, keep mine in Saves",
                    AppEvent::OpenTab { tab: Tab::Media },
                )
                .or("Delete Mika's saves", AppEvent::OpenTab { tab: Tab::Media })
                .needs_action();
            toasts
                .success("You're playing Mika's save. Yours is kept in Saves.")
                .button("Undo", AppEvent::OpenTab { tab: Tab::Media });
            for _ in 0..3 {
                let _ = ctx.run(input(), |ctx| {
                    toasts.place_page(page);
                    toasts.show(ctx);
                });
            }
            let row = ctx.style().spacing.interact_size.y;
            let rects = toasts.rects(&ctx);
            (
                rects.iter().map(|rect| rect.height()).collect::<Vec<_>>(),
                row,
                rects,
            )
        };
        let (wide, row, rects) = heights(760.0);
        for (height, rect) in wide.iter().zip(&rects) {
            assert!(
                *height < row * 2.0,
                "{height} for one line of {row} at {rect:?}"
            );
            assert!(rect.width() <= 760.0 - MARGIN * 2.0 + 0.5, "{rect:?}");
        }
        let (narrow, row, rects) = heights(412.0);
        assert!(
            narrow.iter().any(|height| *height >= row * 2.0),
            "{narrow:?} at 412: the notice wraps or stacks"
        );
        for rect in rects {
            assert!(rect.width() <= 412.0 - MARGIN * 2.0 + 0.5, "{rect:?}");
        }
    }

    #[test]
    fn a_page_strip_shrinks_to_the_shorter_toast_that_replaces_a_notice() {
        let ctx = egui::Context::default();
        let page = Rect::from_min_max(pos2(0.0, 0.0), pos2(412.0, 800.0));
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(412.0, 915.0))),
            ..Default::default()
        };
        let mut toasts = Toasts::default();
        toasts
            .info("Mika's saves didn't replace yours.")
            .button(
                "Use Mika's, keep mine in Saves",
                AppEvent::OpenTab { tab: Tab::Media },
            )
            .or("Delete Mika's saves", AppEvent::OpenTab { tab: Tab::Media })
            .needs_action();
        let mut strip = 0.0;
        let mut frame = |toasts: &mut Toasts| {
            let _ = ctx.run(input(), |ctx| {
                strip = toasts.page_room(ctx).unwrap_or(0.0);
                toasts.place_page(page);
                toasts.show(ctx);
            });
            strip
        };
        for _ in 0..3 {
            frame(&mut toasts);
        }
        let notice = frame(&mut toasts);
        toasts
            .press("Use Mika's, keep mine in Saves")
            .expect("the Use button");
        toasts
            .success("You're playing Mika's save.")
            .button("Undo", AppEvent::OpenTab { tab: Tab::Media });
        let mut last = notice;
        for _ in 0..30 {
            last = frame(&mut toasts);
        }
        assert_eq!(toasts.open_texts(), ["You're playing Mika's save."]);
        let toast = toasts.rects(&ctx)[0];
        assert!(last < notice - 10.0, "{last} against {notice}");
        assert!(
            (last - (toast.height() + MARGIN * 2.0)).abs() < 0.5,
            "{last} against {toast:?}"
        );
        toasts.age(BUTTON_LIFETIME * 2.0);
        assert_eq!(frame(&mut toasts), 0.0);
    }

    #[test]
    fn a_notice_waiting_for_an_answer_leaves_room_at_the_page_end() {
        let ctx = egui::Context::default();
        let page = Rect::from_min_max(pos2(0.0, 0.0), pos2(412.0, 800.0));
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(412.0, 915.0))),
            ..Default::default()
        };
        let mut toasts = Toasts::default();
        let mut need = None;
        let _ = ctx.run(input(), |ctx| {
            need = toasts.page_room(ctx);
        });
        assert_eq!(need, None);

        toasts
            .info("Saves from Mika's game in Play together went to a separate file.")
            .button("Use", AppEvent::OpenTab { tab: Tab::Media })
            .or("Delete", AppEvent::OpenTab { tab: Tab::Media })
            .needs_action();
        let mut top = f32::INFINITY;
        for _ in 0..3 {
            let _ = ctx.run(input(), |ctx| {
                need = toasts.page_room(ctx);
                toasts.place_page(page);
                toasts.show(ctx);
            });
            top = toasts
                .rects(&ctx)
                .iter()
                .map(|rect| rect.top())
                .fold(f32::INFINITY, f32::min);
        }
        assert!(top < page.bottom());
        let need = need.unwrap_or(0.0);
        assert!(
            need >= page.bottom() - top,
            "room {need} for a stack from {top}"
        );
    }
}
