use egui::style::ScrollAnimation;
use egui::{Align, Context, Id, Rect, Ui, vec2};

pub const PEEK: f32 = 24.0;
pub const STICK_SPEED: f32 = 900.0;
pub const PAGE_SHARE: f32 = 0.85;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Pending {
    focus: bool,
    keys: bool,
    lines: f32,
    pages: i32,
    anywhere: bool,
    scrolled: bool,
}

impl Pending {
    fn moves(&self) -> bool {
        self.lines != 0.0 || self.pages != 0
    }
}

fn pending_id() -> Id {
    Id::new("big_screen_scroll")
}

fn update(ctx: &Context, change: impl FnOnce(&mut Pending)) {
    ctx.data_mut(|d| change(d.get_temp_mut_or_default::<Pending>(pending_id())));
}

fn pending(ctx: &Context) -> Pending {
    ctx.data(|d| d.get_temp::<Pending>(pending_id()))
        .unwrap_or_default()
}

pub(super) fn focus_moved(ctx: &Context) {
    update(ctx, |p| p.focus = true);
}

pub(super) fn keys_shown(ctx: &Context, shown: bool) {
    update(ctx, |p| p.keys = shown);
}

pub(super) fn by_lines(ctx: &Context, lines: f32) {
    update(ctx, |p| p.lines += lines);
}

pub(super) fn by_pages(ctx: &Context, pages: i32) {
    update(ctx, |p| p.pages += pages);
}

pub(super) fn end_pass(ctx: &Context, paging: bool) -> bool {
    let unused = ctx.input(|i| i.smooth_scroll_delta.y);
    let mut scrolled = false;
    update(ctx, |p| {
        if !paging {
            *p = Pending::default();
            return;
        }
        scrolled = std::mem::take(&mut p.scrolled);
        p.focus = false;
        p.anywhere = p.moves();
        if unused != 0.0 {
            p.lines -= unused;
            p.anywhere = true;
        }
    });
    scrolled
}

fn this_pass_rect(ctx: &Context, id: Id) -> Option<(Rect, Rect)> {
    ctx.viewport(|viewport| {
        viewport
            .this_pass
            .widgets
            .get(id)
            .map(|widget| (widget.rect, widget.interact_rect))
    })
}

pub fn follow(ui: &Ui) {
    let ctx = ui.ctx();
    let wanted = pending(ctx);
    if !wanted.focus && !wanted.keys && !wanted.moves() {
        return;
    }
    let focused = ctx
        .memory(|m| m.focused())
        .and_then(|id| this_pass_rect(ctx, id))
        .map(|(rect, _)| rect)
        .filter(|rect| ui.min_rect().contains_rect(*rect));
    if let Some(rect) = focused {
        if wanted.keys {
            ui.scroll_to_rect_animation(rect, Some(Align::Min), ScrollAnimation::none());
        } else if wanted.focus {
            ui.scroll_to_rect(rect.expand2(vec2(0.0, PEEK)), None);
        }
    }
    if wanted.moves() && (focused.is_some() || wanted.anywhere) {
        let page = ui.clip_rect().height() * PAGE_SHARE;
        let down = wanted.lines + wanted.pages as f32 * page;
        ui.scroll_with_delta_animation(vec2(0.0, -down), ScrollAnimation::none());
        update(ctx, |p| {
            p.lines = 0.0;
            p.pages = 0;
            p.anywhere = false;
            p.scrolled = true;
        });
    }
    if focused.is_some() {
        update(ctx, |p| p.focus = false);
    }
}

pub(super) fn refocus_in_view(ctx: &Context) {
    let Some(focused) = ctx.memory(|m| m.focused()) else {
        return;
    };
    let Some((rect, shown)) = this_pass_rect(ctx, focused) else {
        return;
    };
    let shown_height = shown.intersect(rect).height().max(0.0);
    if shown_height >= rect.height() - 0.5 {
        return;
    }
    let layer = ctx.viewport(|v| v.this_pass.widgets.get(focused).map(|w| w.layer_id));
    let next = ctx.viewport(|viewport| {
        viewport
            .this_pass
            .widgets
            .layers()
            .filter(|(id, _)| Some(**id) == layer)
            .flat_map(|(_, widgets)| widgets.iter())
            .filter(|widget| widget.sense.is_focusable() && widget.enabled)
            .filter(|widget| widget.rect.area() > 0.0 && widget.interact_rect == widget.rect)
            .min_by(|a, b| {
                let gap = |r: &Rect| (r.center().y - rect.center().y).abs();
                gap(&a.rect)
                    .total_cmp(&gap(&b.rect))
                    .then(a.rect.left().total_cmp(&b.rect.left()))
            })
            .map(|widget| widget.id)
    });
    if let Some(next) = next {
        ctx.memory_mut(|m| m.request_focus(next));
    }
}

pub trait Following {
    fn show_following<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> egui::scroll_area::ScrollAreaOutput<R>;
}

impl Following for egui::ScrollArea {
    fn show_following<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> egui::scroll_area::ScrollAreaOutput<R> {
        self.show(ui, |ui| {
            let inner = body(ui);
            follow(ui);
            inner
        })
    }
}
