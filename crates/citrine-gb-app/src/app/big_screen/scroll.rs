use egui::style::ScrollAnimation;
use egui::{Align, Color32, Context, Id, LayerId, Mesh, Order, Rect, Ui, WidgetRect, pos2, vec2};

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

fn fade_id() -> Id {
    Id::new("big_screen_page_fade")
}

pub fn set_fade(ctx: &Context, on: bool, strip: f32) {
    let frame = on.then(|| (ctx.cumulative_frame_nr(), strip));
    ctx.data_mut(|d| d.insert_temp(fade_id(), frame));
}

fn fades(ui: &Ui) -> Option<f32> {
    let (frame, strip) = ui
        .data(|d| d.get_temp::<Option<(u64, f32)>>(fade_id()))
        .flatten()?;
    (ui.layer_id().order == Order::Background && frame == ui.ctx().cumulative_frame_nr())
        .then_some(strip)
}

fn fade_rect_id() -> Id {
    Id::new("big_screen_page_fade_rect")
}

#[cfg(test)]
pub fn faded(ctx: &Context) -> Option<Rect> {
    ctx.data(|d| d.get_temp(fade_rect_id()))
}

fn fade_tops_id() -> Id {
    Id::new("big_screen_page_fade_tops")
}

fn scrolled_from_top(ui: &Ui, view: Rect, offset: f32) {
    let frame = ui.ctx().cumulative_frame_nr();
    ui.data_mut(|d| {
        let tops = d.get_temp_mut_or_default::<(u64, Vec<(Rect, f32)>)>(fade_tops_id());
        if tops.0 != frame {
            *tops = (frame, Vec::new());
        }
        tops.1.push((view, offset));
    });
}

fn fade_top_rect_id() -> Id {
    Id::new("big_screen_page_fade_top_rect")
}

#[cfg(test)]
pub fn faded_top(ctx: &Context) -> Option<Rect> {
    ctx.data(|d| d.get_temp(fade_top_rect_id())).flatten()
}

pub fn fade_top(ctx: &Context, page: Rect, fill: Color32) {
    let frame = ctx.cumulative_frame_nr();
    let tops = ctx
        .data(|d| d.get_temp::<(u64, Vec<(Rect, f32)>)>(fade_tops_id()))
        .filter(|tops| tops.0 == frame)
        .map(|tops| tops.1)
        .unwrap_or_default();
    let painter = ctx.layer_painter(LayerId::background());
    let mut faded = None;
    for (view, offset) in tops {
        if offset <= 0.0 {
            continue;
        }
        let left = view.left().max(page.left());
        let right = view.right().min(page.right());
        let bottom = (view.top() + PEEK).min(view.bottom());
        let shown = fill.gamma_multiply((offset / PEEK).min(1.0));
        let band = Rect::from_x_y_ranges(left..=right, view.top()..=bottom);
        faded = faded.or(Some(band));
        let mut mesh = Mesh::default();
        mesh.colored_vertex(pos2(left, view.top()), shown);
        mesh.colored_vertex(pos2(right, view.top()), shown);
        mesh.colored_vertex(pos2(left, bottom), Color32::TRANSPARENT);
        mesh.colored_vertex(pos2(right, bottom), Color32::TRANSPARENT);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 3, 2);
        painter.add(mesh);
    }
    ctx.data_mut(|d| d.insert_temp(fade_top_rect_id(), faded));
}

pub fn fade_bottom(ctx: &Context, page: Rect, fill: Color32, strip: f32) {
    let top = (page.bottom() - (PEEK - strip).max(0.0)).max(page.top());
    let faded = Rect::from_x_y_ranges(page.x_range(), top..=page.bottom());
    ctx.data_mut(|d| d.insert_temp(fade_rect_id(), faded));
    let mut mesh = Mesh::default();
    mesh.colored_vertex(pos2(page.left(), top), Color32::TRANSPARENT);
    mesh.colored_vertex(pos2(page.right(), top), Color32::TRANSPARENT);
    mesh.colored_vertex(page.left_bottom(), fill);
    mesh.colored_vertex(page.right_bottom(), fill);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    ctx.layer_painter(LayerId::background()).add(mesh);
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

fn this_pass_layer(ctx: &Context, id: Id) -> Option<egui::LayerId> {
    ctx.viewport(|viewport| viewport.this_pass.widgets.get(id).map(|w| w.layer_id))
}

pub fn follow(ui: &Ui) {
    let ctx = ui.ctx();
    let wanted = pending(ctx);
    if !wanted.focus && !wanted.keys && !wanted.moves() {
        return;
    }
    let focused = ctx
        .memory(|m| m.focused())
        .filter(|id| this_pass_layer(ctx, *id) == Some(ui.layer_id()))
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
    let layer = this_pass_layer(ctx, focused);
    let top = super::tab_bottom(ctx) - 1.0;
    let next = ctx.viewport(|viewport| {
        let focusable: Vec<_> = viewport
            .this_pass
            .widgets
            .layers()
            .filter(|(id, _)| Some(**id) == layer)
            .flat_map(|(_, widgets)| widgets.iter())
            .filter(|widget| super::is_stop(widget) && widget.rect.top() >= top)
            .collect();
        let holds_others = |widget: &WidgetRect| {
            !widget.sense.senses_click()
                && focusable
                    .iter()
                    .any(|other| other.id != widget.id && widget.rect.contains_rect(other.rect))
        };
        focusable
            .iter()
            .filter(|widget| widget.interact_rect == widget.rect && !holds_others(widget))
            .min_by(|a, b| {
                let apart = |r: &Rect| r.max.x <= rect.min.x || r.min.x >= rect.max.x;
                let gap = |r: &Rect| (r.center().y - rect.center().y).abs();
                apart(&a.rect)
                    .cmp(&apart(&b.rect))
                    .then(gap(&a.rect).total_cmp(&gap(&b.rect)))
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
        let mut clip = Rect::NOTHING;
        let output = self.show(ui, |ui| {
            clip = ui.clip_rect();
            let inner = body(ui);
            if let Some(strip) = fades(ui) {
                let gap = ui.spacing().item_spacing.y;
                ui.add_space((PEEK + gap - strip).max(0.0) - gap);
            }
            follow(ui);
            inner
        });
        if fades(ui).is_some() {
            let view = output.inner_rect;
            let top = clip
                .top()
                .clamp(view.top() - ui.visuals().clip_rect_margin, view.top());
            let view = Rect::from_x_y_ranges(view.x_range(), top..=view.bottom());
            scrolled_from_top(ui, view, output.state.offset.y);
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_top_fade_starts_where_the_scrolled_rows_are_cut() {
        let ctx = Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 300.0))),
            ..Default::default()
        };
        let mut clip = Rect::NOTHING;
        for _ in 0..3 {
            let _ = ctx.run(input(), |ctx| {
                set_fade(ctx, true, 0.0);
                let page = egui::CentralPanel::default()
                    .show(ctx, |ui| {
                        ui.add_space(40.0);
                        egui::ScrollArea::vertical()
                            .vertical_scroll_offset(120.0)
                            .show_following(ui, |ui| {
                                clip = ui.clip_rect();
                                for row in 0..40 {
                                    ui.label(format!("Row {row}"));
                                }
                            });
                    })
                    .response
                    .rect;
                fade_top(ctx, page, Color32::WHITE);
            });
        }
        let fade = faded_top(&ctx).expect("a scrolled list fades under its top edge");
        assert!(
            (fade.top() - clip.top()).abs() < 0.01,
            "the fade {fade:?} starts where the rows are cut at {clip:?}"
        );
    }
}
