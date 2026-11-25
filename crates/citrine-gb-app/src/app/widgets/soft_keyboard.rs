#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Seen {
    shown: f32,
    full: [Option<f32>; 2],
}

fn id() -> egui::Id {
    egui::Id::new("soft_keyboard")
}

fn wide(ctx: &egui::Context) -> usize {
    let screen = ctx.viewport_rect();
    usize::from(screen.width() > screen.height())
}

fn seen(ctx: &egui::Context) -> Seen {
    ctx.data(|d| d.get_temp::<Seen>(id())).unwrap_or_default()
}

#[cfg_attr(
    not(any(target_os = "android", test)),
    expect(dead_code, reason = "only Android reports the keyboard")
)]
pub fn note(ctx: &egui::Context, shown: f32, full: f32, wide: bool) {
    let mut seen = seen(ctx);
    seen.shown = shown.max(0.0);
    if full > 0.0 {
        seen.full[usize::from(wide)] = Some(full);
    }
    ctx.data_mut(|d| d.insert_temp(id(), seen));
}

pub fn shown(ctx: &egui::Context) -> f32 {
    seen(ctx).shown
}

pub fn full(ctx: &egui::Context) -> Option<f32> {
    seen(ctx).full[wide(ctx)]
}
