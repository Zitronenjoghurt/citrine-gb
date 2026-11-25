use egui::{Align, Context, Id, Rect, Ui};

const GLOW_SECONDS: f64 = 1.5;
const MISS_SECONDS: f64 = 1.0;
const SETTLE_SECONDS: f64 = 0.25;

#[derive(Clone)]
struct Spot {
    label: &'static str,
    heading: bool,
    aimed: f64,
    since: Option<f64>,
}

fn key() -> Id {
    Id::new("settings_spotlight")
}

pub fn aim(ctx: &Context, label: &'static str) {
    put(ctx, label, false);
}

pub fn aim_heading(ctx: &Context, title: &'static str) {
    put(ctx, title, true);
}

fn put(ctx: &Context, label: &'static str, heading: bool) {
    let aimed = ctx.input(|i| i.time);
    let spot = Spot {
        label,
        heading,
        aimed,
        since: None,
    };
    ctx.data_mut(|d| d.insert_temp(key(), spot));
}

pub fn mark(ui: &Ui, label: &str, rect: Rect) {
    glow(ui, label, false, rect);
}

pub fn mark_heading(ui: &Ui, title: &str, rect: Rect) {
    glow(ui, title, true, rect);
}

fn glow(ui: &Ui, label: &str, heading: bool, rect: Rect) {
    let Some(mut spot) = ui.data(|d| d.get_temp::<Spot>(key())) else {
        return;
    };
    if spot.label != label || spot.heading != heading {
        return;
    }
    let now = ui.input(|i| i.time);
    let since = match spot.since {
        Some(since) => since,
        None if now - spot.aimed > MISS_SECONDS => {
            ui.data_mut(|d| d.remove::<Spot>(key()));
            return;
        }
        None => {
            spot.since = Some(now);
            ui.data_mut(|d| d.insert_temp(key(), spot));
            now
        }
    };
    if now - since < SETTLE_SECONDS {
        let align = if heading { Align::TOP } else { Align::Center };
        ui.scroll_to_rect(rect, Some(align));
    }
    let left = 1.0 - (now - since) / GLOW_SECONDS;
    if left <= 0.0 {
        ui.data_mut(|d| d.remove::<Spot>(key()));
        return;
    }
    let fill = ui
        .visuals()
        .selection
        .bg_fill
        .gamma_multiply(0.35 * left as f32);
    ui.painter().rect_filled(
        rect.expand(2.0),
        ui.visuals().widgets.noninteractive.corner_radius,
        fill,
    );
    ui.ctx().request_repaint();
}
