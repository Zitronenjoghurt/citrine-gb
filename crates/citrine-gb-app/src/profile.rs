macro_rules! scope {
    ($name:literal) => {
        #[cfg(feature = "profile")]
        puffin::profile_scope!($name);
    };
}

pub(crate) use scope;

#[cfg(feature = "profile")]
pub(crate) fn frame(ctx: &egui::Context) {
    puffin::GlobalProfiler::lock().new_frame();
    let id = open_id();
    let toggled = ctx.input_mut(|i| {
        i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::P,
        ))
    });
    let mut open = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
    if toggled {
        open = !open;
    }
    puffin::set_scopes_on(open);
    if open {
        open = puffin_egui::profiler_window(ctx);
    }
    ctx.data_mut(|d| d.insert_temp(id, open));
}

#[cfg(feature = "profile")]
fn open_id() -> egui::Id {
    egui::Id::new("profiler_open")
}

#[cfg(all(test, feature = "profile"))]
pub(crate) fn set_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

#[cfg(not(feature = "profile"))]
pub(crate) fn frame(_ctx: &egui::Context) {}
