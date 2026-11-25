use crate::app::events::AppEventQueue;
use crate::app::ui_state::settings::Settings;
use crate::app::widgets::camera_view;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::reset_slider::reset_width;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::tilt::TiltIndicator;
use crate::emulator::gimmicks::camera::{CameraView, Feed};
use crate::emulator::gimmicks::clock::hours_label;
use crate::emulator::gimmicks::settings::{
    CLOCK_FORWARD_HINT, CLOCK_HINT, MAX_CLOCK_OFFSET, PRINTER_HINT, PRINTER_LINKED, RECENTER,
};
use crate::emulator::gimmicks::{Cart, Motion};
use egui::{Align, Layout, Ui, Widget, vec2};

const HOUR: i64 = 3600;
const TILT_HINT: &str = "Tilt with I, J, K and L or a controller's right stick.";

#[derive(Clone, Copy)]
pub struct CartridgeState<'a> {
    pub rom_key: &'a str,
    pub cart: Cart,
    pub motion: Motion,
    pub printer_free: bool,
    pub touch: bool,
}

pub(crate) fn motion_row(
    form: &mut Form,
    state: &CartridgeState,
    events: &mut Option<&mut AppEventQueue>,
) {
    let label = if state.cart.tilt { "Tilt" } else { "Rotate" };
    let row = |ui: &mut Ui| {
        ui.horizontal(|ui| {
            let side = ui.spacing().interact_size.y;
            let level = (state.touch
                && TiltIndicator::new(state.motion).side(side).ui(ui).clicked())
                | ui.button(RECENTER).clicked();
            if level && let Some(events) = events.as_deref_mut() {
                events.centre_tilt();
            }
        });
    };
    if state.touch {
        form.row(label, row);
    } else {
        form.row_with_hint(label, TILT_HINT, row);
    }
}

pub(crate) fn printer_row(form: &mut Form, s: &mut Settings, state: &CartridgeState, label: &str) {
    let mut on = s.gimmicks.printer(state.rom_key);
    if form.toggle_when(label, Some(PRINTER_HINT), state.printer_free, &mut on) {
        s.gimmicks.set_printer(state.rom_key, on);
        s.dirty = true;
    }
    if !state.printer_free {
        form.hint(PRINTER_LINKED);
    }
}

pub(crate) fn clock_row(form: &mut Form, s: &mut Settings, state: &CartridgeState, label: &str) {
    let saved = s.gimmicks.clock_offset(state.rom_key) / HOUR;
    let most = MAX_CLOCK_OFFSET / HOUR;
    let (least, default) = if state.cart.clock_back {
        (-most, 0)
    } else {
        (saved, saved)
    };
    let hint = if state.cart.clock_back {
        CLOCK_HINT
    } else {
        CLOCK_FORWARD_HINT
    };
    form.row_with_hint(label, hint, |ui| {
        let mut hours = saved;
        let response = ResetSlider::new(&mut hours, least..=most)
            .default_value(default)
            .text(hours_label)
            .ui(ui);
        if hours != saved {
            s.gimmicks.set_clock_offset(state.rom_key, hours * HOUR);
        }
        if response.drag_stopped() || (response.changed() && !response.dragged()) {
            s.dirty = true;
        }
    });
}

pub(crate) fn camera_frame(ui: &mut Ui, s: &mut Settings, feed: Option<(&Feed, u64)>) {
    s.dirty |= camera_view::show(ui, feed, &mut s.gimmicks.camera_view);
    if feed.is_none() {
        return;
    }
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        let moved = s.gimmicks.camera_view != CameraView::default();
        let button =
            crate::app::mobile::compact(ui, egui::Button::new(crate::icons::CROSSHAIR_SIMPLE))
                .min_size(vec2(reset_width(ui), 0.0));
        if ui
            .add_enabled(moved, button)
            .on_hover_text(camera_view::CENTRE)
            .clicked()
        {
            s.gimmicks.camera_view = CameraView::default();
            s.dirty = true;
        }
    });
}
