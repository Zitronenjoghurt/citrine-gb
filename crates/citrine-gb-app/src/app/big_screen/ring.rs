use egui::{Context, CornerRadius, Id, Rect, Response, Stroke, StrokeKind};

pub const GAP: f32 = 2.0;
pub const WIDTH: f32 = 3.0;
pub const MARGIN: f32 = GAP + WIDTH;
const MAX_RADIUS: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mark {
    Ring { rect: Rect, radius: f32 },
    Fill,
}

fn ring_id() -> Id {
    Id::new("big_screen_ring")
}

fn mark_id() -> Id {
    Id::new("big_screen_mark")
}

pub(super) fn set_shown(ctx: &Context, shown: bool) {
    ctx.data_mut(|d| {
        d.insert_temp(ring_id(), shown);
        d.remove::<(Id, Mark)>(mark_id());
    });
}

pub fn shown(response: &Response) -> bool {
    response.has_focus()
        && response
            .ctx
            .data(|d| d.get_temp::<bool>(ring_id()))
            .unwrap_or(false)
}

pub fn mark(response: &Response, mark: Mark) {
    if response.has_focus() {
        response
            .ctx
            .data_mut(|d| d.insert_temp(mark_id(), (response.id, mark)));
    }
}

pub fn fill_color(visuals: &egui::Visuals, ground: egui::Color32) -> egui::Color32 {
    let share = if visuals.dark_mode { 0.3 } else { 0.25 };
    ground.lerp_to_gamma(visuals.selection.bg_fill, share)
}

pub fn fill(response: &Response) -> bool {
    let shown = shown(response);
    if shown {
        mark(response, Mark::Fill);
    }
    shown
}

fn ring_for(response: &Response, marked: Option<Mark>) -> Option<(Rect, CornerRadius)> {
    let (rect, radius) = match marked {
        Some(Mark::Fill) => return None,
        Some(Mark::Ring { rect, radius }) => (rect, radius),
        None => (response.rect, MAX_RADIUS.min(response.rect.height() / 2.0)),
    };
    let radius = (radius + GAP).round().clamp(0.0, u8::MAX as f32) as u8;
    Some((rect.expand(GAP), CornerRadius::same(radius)))
}

pub(super) fn current(ctx: &Context) -> Option<(Response, Option<(Rect, CornerRadius)>)> {
    let response = ctx
        .memory(|m| m.focused())
        .and_then(|id| ctx.read_response(id))?;
    let marked = ctx
        .data(|d| d.get_temp::<(Id, Mark)>(mark_id()))
        .filter(|(id, _)| *id == response.id)
        .map(|(_, mark)| mark);
    let ring = ring_for(&response, marked);
    Some((response, ring))
}

pub(super) fn paint(ctx: &Context) {
    let Some((response, Some((rect, radius)))) = current(ctx) else {
        return;
    };
    let visuals = ctx.style().visuals.clone();
    let color = crate::app::ui_state::ui_theme::brand_mark(&visuals);
    let shown = response.interact_rect.expand(MARGIN + 1.0);
    ctx.layer_painter(response.layer_id)
        .with_clip_rect(shown)
        .rect_stroke(rect, radius, Stroke::new(WIDTH, color), StrokeKind::Outside);
}
