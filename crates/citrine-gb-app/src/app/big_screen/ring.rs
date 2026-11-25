use egui::{Context, CornerRadius, Id, Rect, Response, Stroke, StrokeKind};

pub const GAP: f32 = 2.0;
pub const WIDTH: f32 = 3.0;
pub const MARGIN: f32 = GAP + WIDTH;
const MAX_RADIUS: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mark {
    Ring { rect: Rect, radius: f32 },
    Row { rect: Rect, radius: f32, clip: Rect },
}

fn ring_id() -> Id {
    Id::new("big_screen_ring")
}

fn mark_id() -> Id {
    Id::new("big_screen_mark")
}

pub(super) fn set_shown(ctx: &Context, shown: bool) {
    ctx.data_mut(|d| d.insert_temp(ring_id(), shown));
}

pub fn shown(response: &Response) -> bool {
    shown_id(&response.ctx, response.id)
}

pub fn shown_id(ctx: &Context, id: Id) -> bool {
    ctx.memory(|m| m.has_focus(id)) && ctx.data(|d| d.get_temp::<bool>(ring_id())).unwrap_or(false)
}

pub fn mark(response: &Response, mark: Mark) {
    if response.has_focus() {
        let pass = response.ctx.cumulative_pass_nr();
        response
            .ctx
            .data_mut(|d| d.insert_temp(mark_id(), (pass, response.id, mark)));
    }
}

pub fn row(response: &Response, rect: Rect, radius: f32, clip: Rect) -> bool {
    let shown = shown(response);
    if shown {
        mark(response, Mark::Row { rect, radius, clip });
    }
    shown
}

fn ring_for(response: &Response, marked: Option<Mark>) -> Option<(Rect, CornerRadius)> {
    let (rect, radius) = match marked {
        Some(Mark::Row { rect, radius, .. }) => {
            (rect.shrink(WIDTH + GAP), (radius - WIDTH - GAP).max(0.0))
        }
        Some(Mark::Ring { rect, radius }) => (rect, radius),
        None => (response.rect, MAX_RADIUS.min(response.rect.height() / 2.0)),
    };
    let radius = (radius + GAP).round().clamp(0.0, u8::MAX as f32) as u8;
    Some((rect.expand(GAP), CornerRadius::same(radius)))
}

fn marked(ctx: &Context) -> Option<(Response, Option<Mark>)> {
    let response = ctx
        .memory(|m| m.focused())
        .and_then(|id| ctx.read_response(id))?;
    let pass = ctx.cumulative_pass_nr();
    let marked = ctx
        .data(|d| d.get_temp::<(u64, Id, Mark)>(mark_id()))
        .filter(|(at, id, _)| *id == response.id && at + 1 >= pass)
        .map(|(_, _, mark)| mark);
    Some((response, marked))
}

#[cfg(test)]
pub(crate) fn current(ctx: &Context) -> Option<(Response, Option<(Rect, CornerRadius)>)> {
    let (mut response, marked) = marked(ctx)?;
    if let Some(last) = ctx.viewport(|v| v.prev_pass.widgets.get(response.id).copied()) {
        response.rect = last.rect;
        response.interact_rect = last.interact_rect;
    }
    let ring = ring_for(&response, marked);
    Some((response, ring))
}

pub(super) fn paint(ctx: &Context) {
    let Some((response, marked)) = marked(ctx) else {
        return;
    };
    let Some((rect, radius)) = ring_for(&response, marked) else {
        return;
    };
    let visuals = ctx.style().visuals.clone();
    let color = crate::app::ui_state::ui_theme::brand_mark(&visuals);
    let shown = match marked {
        Some(Mark::Row { rect, clip, .. }) => clip.intersect(rect.expand(1.0)),
        _ => response.interact_rect.expand(MARGIN + 1.0),
    };
    ctx.layer_painter(response.layer_id)
        .with_clip_rect(shown)
        .rect_stroke(rect, radius, Stroke::new(WIDTH, color), StrokeKind::Outside);
}
