use crate::app::events::AppEventQueue;
use crate::app::ui_state::settings::Settings;
use crate::app::widgets::camera_view;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::reset_slider::reset_width;
use crate::app::widgets::section::section;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::tilt::TiltIndicator;
use crate::emulator::gimmicks::camera::{CameraView, Feed};
use crate::emulator::gimmicks::clock::hours_label;
use crate::emulator::gimmicks::settings::{
    CLOCK_FORWARD_HINT, CLOCK_HINT, DEFAULT_LIGHT, MAX_CLOCK_OFFSET, PRINTER_HINT, PRINTER_LINKED,
    RECENTER,
};
use crate::emulator::gimmicks::{CameraSource, Cart, LightSource, Motion};
use egui::{Align, Layout, Ui, Widget, vec2};

const HOUR: i64 = 3600;
const TILT_HINT: &str = "Tilt with I, J, K and L or a controller's right stick.";

#[derive(Clone, Copy)]
pub struct CartridgeState<'a> {
    pub rom_key: &'a str,
    pub cart: Cart,
    pub motion: Motion,
    pub printer_free: bool,
    pub camera_error: Option<&'a str>,
    pub camera_feed: Option<(&'a Feed, u64)>,
    pub light_sensor: bool,
    pub live_camera: bool,
    pub touch: bool,
}

pub fn show(
    ui: &mut Ui,
    s: &mut Settings,
    state: CartridgeState,
    mut events: Option<&mut AppEventQueue>,
) {
    let cart = state.cart;
    section(ui, crate::icons::GAME_CONTROLLER, "Cartridge", |ui| {
        Form::show(ui, "general", |form| {
            if cart.clock {
                clock_row(form, s, &state);
            }
            if cart.rumble {
                form.row("Rumble", |ui| {
                    s.dirty |= crate::app::widgets::check::checkbox(ui, &mut s.gimmicks.rumble, "")
                        .changed();
                });
            }
            if cart.motion() {
                let label = if cart.tilt { "Tilt" } else { "Rotate" };
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
            if cart.light {
                light_rows(form, s, state.light_sensor);
            }
            if cart.camera {
                camera_rows(form, s, &state, events.take());
            }
            if cart.printer {
                form.row_with_hint("Printer", PRINTER_HINT, |ui| {
                    let mut on = s.gimmicks.printer(state.rom_key);
                    let changed = ui
                        .add_enabled(
                            state.printer_free,
                            crate::app::widgets::check::Check::new(&mut on, ""),
                        )
                        .changed();
                    if changed {
                        s.gimmicks.set_printer(state.rom_key, on);
                        s.dirty = true;
                    }
                });
                if !state.printer_free {
                    form.hint(PRINTER_LINKED);
                }
            }
        });
    });
}

fn clock_row(form: &mut Form, s: &mut Settings, state: &CartridgeState) {
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
    form.row_with_hint("Clock", hint, |ui| {
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

fn light_rows(form: &mut Form, s: &mut Settings, light_sensor: bool) {
    if light_sensor {
        form.row("Light sensor", |ui| {
            let mut sensor = s.gimmicks.light_source == LightSource::Sensor;
            if crate::app::widgets::check::checkbox(ui, &mut sensor, "").changed() {
                s.gimmicks.light_source = if sensor {
                    LightSource::Sensor
                } else {
                    LightSource::Slider
                };
                s.dirty = true;
            }
        });
    }
    if light_sensor && s.gimmicks.light_source == LightSource::Sensor {
        return;
    }
    form.row("Sunlight", |ui| {
        let mut percent = (s.gimmicks.light * 100.0).round() as u32;
        let response = ResetSlider::new(&mut percent, 0..=100)
            .default_value((DEFAULT_LIGHT * 100.0) as u32)
            .suffix(" %")
            .ui(ui);
        if response.changed() {
            s.gimmicks.light = percent as f32 / 100.0;
        }
        if response.drag_stopped() || (response.changed() && !response.dragged()) {
            s.dirty = true;
        }
    });
}

fn camera_rows(
    form: &mut Form,
    s: &mut Settings,
    state: &CartridgeState,
    mut events: Option<&mut AppEventQueue>,
) {
    let choose = |ui: &mut Ui| {
        if ui.button("Choose picture").clicked()
            && let Some(events) = events.as_deref_mut()
        {
            events.pick_camera_picture();
        }
    };
    if !state.live_camera {
        form.row("Camera", choose);
        return;
    }
    let live = s.gimmicks.camera == CameraSource::Camera && state.camera_error.is_none();
    let select = |ui: &mut Ui| {
        s.dirty |= GenericSelect::from_enum(&mut s.gimmicks.camera, "enum_select_camera_source")
            .default_value(CameraSource::default())
            .ui(ui)
            .changed();
    };
    if live {
        form.row_with_hint("Camera", camera_view::DESKTOP_HINT, select);
    } else {
        form.row("Camera", select);
    }
    if s.gimmicks.camera == CameraSource::Picture {
        form.under(choose);
    } else if let Some(error) = state.camera_error {
        form.hint(error);
    } else {
        form.full(|ui| camera_frame(ui, s, state.camera_feed));
    }
}

fn camera_frame(ui: &mut Ui, s: &mut Settings, feed: Option<(&Feed, u64)>) {
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
