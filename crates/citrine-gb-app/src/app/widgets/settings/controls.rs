use crate::app::ui_state::hotkeys::{Hotkey, HotkeyAction, Hotkeys};
use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, BindingTarget, Captured, GbButton, KeyBindings, key_label,
};
use crate::app::ui_state::settings::{ScalingMode, Settings};
use crate::app::ui_state::touch::{LandscapeControls, TouchSettings};
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::info_mark::info_mark;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::settings::form::Form;
use crate::icons;
use crate::local_link::Half;
use egui::{Response, RichText, Ui, Widget};
use strum::IntoEnumIterator;

pub fn show(ui: &mut Ui, settings: &mut Settings, capture: &mut BindingCapture) -> bool {
    capture.mark_shown();
    apply_capture(settings, capture);
    capture_banner(ui, capture);
    let mut debug = false;

    section(ui, icons::GAME_CONTROLLER, "Game Boy buttons", |ui| {
        let unbound = settings.bindings.unbound();
        if !unbound.is_empty() {
            let names: Vec<String> = unbound.iter().map(|b| b.to_string()).collect();
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("{} has nothing bound to it.", names.join(", ")),
            );
            ui.add_space(6.0);
        }

        buttons_table(ui, settings, capture);

        ui.add_space(10.0);
        Form::show(ui, "stick_steering", |form| {
            form.row_with_hint(
                "Left stick steers",
                "Also reads the analog stick as a d-pad.",
                |ui| {
                    settings.dirty |= ui
                        .checkbox(&mut settings.bindings.stick_as_dpad, "")
                        .changed();
                },
            );
            form.row_with_hint("Stick dead zone", DEAD_ZONE_HINT, |ui| {
                ui.add_enabled_ui(settings.bindings.stick_as_dpad, |ui| {
                    let response = ResetSlider::new(
                        &mut settings.bindings.stick_deadzone,
                        KeyBindings::DEADZONES,
                    )
                    .default_value(KeyBindings::DEFAULT_DEADZONE)
                    .suffix("%")
                    .ui(ui);
                    if response.drag_stopped() || (response.changed() && !response.dragged()) {
                        settings.dirty = true;
                    }
                });
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if reset_button(ui, !settings.bindings.is_default()) {
                settings.bindings = KeyBindings::default();
                capture.cancel();
                settings.dirty = true;
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            debug = ui
                .button(format!("{} Controller debug", icons::BUG))
                .on_hover_text("Live controller state and an input log to send with a bug report")
                .clicked();
        });
    });

    section(ui, icons::KEYBOARD, "Shortcuts", |ui| {
        shortcuts_table(ui, settings, capture);

        ui.add_space(10.0);
        if reset_button(ui, !settings.hotkeys.is_default()) {
            settings.hotkeys = Hotkeys::default();
            capture.cancel();
            settings.dirty = true;
        }
    });

    section(
        ui,
        crate::app::tabs::linked::LINK_ICON,
        "Keyboard split in two",
        |ui| {
            for half in Half::BOTH {
                ui.horizontal(|ui| ui.label(RichText::new(half.label()).strong()));
                ui.add_space(2.0);
                split_table(ui, settings, capture, half);
                ui.add_space(8.0);
            }
            if reset_button(ui, !settings.local_link.keys_are_default()) {
                settings.local_link.reset_keys();
                capture.cancel();
                settings.dirty = true;
            }
        },
    );

    section_with_hint(
        ui,
        icons::DEVICE_MOBILE,
        "On-screen buttons",
        "The touch layout shows these on phones and small windows.",
        |ui| {
            touch_grid(ui, settings);
        },
    );
    debug
}

fn apply_capture(settings: &mut Settings, capture: &mut BindingCapture) {
    let Some((target, captured)) = capture.take() else {
        return;
    };
    match (target, captured) {
        (BindingTarget::Button(button), Captured::Key(key, _)) => {
            settings.bindings.bind_key(key, button);
        }
        (BindingTarget::Button(button), Captured::Pad(pad)) => {
            settings.hotkeys.unbind_pad(pad);
            settings.bindings.bind_pad(pad, button);
        }
        (BindingTarget::Shortcut(action), Captured::Pad(pad)) => {
            if action.is_hold() {
                return;
            }
            settings.bindings.unbind_pad(pad);
            settings.hotkeys.bind_pad(pad, action);
        }
        (BindingTarget::Shortcut(action), Captured::Key(key, modifiers)) => {
            settings
                .hotkeys
                .bind(Hotkey::from_event(key, modifiers), action);
        }
        (BindingTarget::LinkKey(half, button), Captured::Key(key, _)) => {
            settings.local_link.bind_key(half, key, button);
        }
        (BindingTarget::LinkKey(..), Captured::Pad(_)) => return,
    }
    settings.dirty = true;
}

fn capture_banner(ui: &mut Ui, capture: &mut BindingCapture) {
    if !capture.is_waiting() {
        return;
    }

    egui::Frame::new()
        .fill(ui.visuals().selection.bg_fill.gamma_multiply(0.35))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .corner_radius(6)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.strong("Press a key or button…");
                ui.label(RichText::new("Esc to cancel").weak().small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::app::mobile::compact_button(ui, "Cancel").clicked() {
                        capture.cancel();
                    }
                });
            });
        });

    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        capture.cancel();
    }
    ui.add_space(8.0);
}

pub(crate) fn section(ui: &mut Ui, icon: &str, title: &str, body: impl FnOnce(&mut Ui)) {
    section_with(ui, icon, title, None, body);
}

pub(crate) fn section_with_hint(
    ui: &mut Ui,
    icon: &str,
    title: &str,
    hint: &str,
    body: impl FnOnce(&mut Ui),
) {
    section_with(ui, icon, title, Some(hint), body);
}

fn section_with(
    ui: &mut Ui,
    icon: &str,
    title: &str,
    hint: Option<&str>,
    body: impl FnOnce(&mut Ui),
) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(15.0));
        ui.label(RichText::new(title).strong().size(15.0));
        if let Some(hint) = hint {
            info_mark(ui, hint);
        }
    });
    ui.separator();
    ui.add_space(2.0);
    body(ui);
    ui.add_space(14.0);
}

const DEAD_ZONE_HINT: &str = "How far the stick moves before it presses a direction.";

#[derive(Clone, Copy)]
struct NameCell {
    width: f32,
    height: Option<f32>,
}

const BUTTON_NAME: NameCell = NameCell {
    width: 62.0,
    height: Some(CHIP_HEIGHT),
};

const ACTION_NAME: NameCell = NameCell {
    width: 175.0,
    height: None,
};

const ADD_WIDTH: f32 = 26.0;

const CHIP_HEIGHT: f32 = 20.0;

const STACK_BELOW: f32 = 130.0;

fn buttons_table(ui: &mut Ui, settings: &mut Settings, capture: &mut BindingCapture) {
    for (index, button) in GbButton::iter().enumerate() {
        let target = BindingTarget::Button(button);

        striped_row(ui, index, |ui| {
            let name = |ui: &mut Ui| {
                ui.label(RichText::new(button.to_string()).strong());
            };
            let chips = |ui: &mut Ui, settings: &mut Settings| {
                let keys = settings.bindings.keys_for(button);
                let pads = settings.bindings.pads_for(button);
                if keys.is_empty() && pads.is_empty() {
                    ui.label(RichText::new("Nothing bound").weak().small());
                }
                for key in keys {
                    if chip(ui, icons::KEYBOARD, &key_label(key), key.name()) {
                        settings.bindings.unbind_key(key);
                        settings.dirty = true;
                    }
                }
                for pad in pads {
                    if chip(
                        ui,
                        icons::GAME_CONTROLLER,
                        pad.short_name(),
                        &pad.to_string(),
                    ) {
                        settings.bindings.unbind_pad(pad);
                        settings.dirty = true;
                    }
                }
            };

            if binding_row(
                ui,
                settings,
                name,
                BUTTON_NAME,
                chips,
                capture.waiting_for(target),
            ) {
                capture.start(target, BindingKind::Any);
            }
        });
    }
}

fn split_table(ui: &mut Ui, settings: &mut Settings, capture: &mut BindingCapture, half: Half) {
    for (index, button) in GbButton::iter().enumerate() {
        let target = BindingTarget::LinkKey(half, button);

        striped_row(ui, index, |ui| {
            let name = |ui: &mut Ui| {
                ui.label(RichText::new(button.to_string()).strong());
            };
            let chips = |ui: &mut Ui, settings: &mut Settings| {
                let keys = settings.local_link.keys_for(half, button);
                if keys.is_empty() {
                    ui.label(RichText::new("Nothing bound").weak().small());
                }
                for key in keys {
                    if chip(ui, icons::KEYBOARD, &key_label(key), key.name()) {
                        settings.local_link.unbind_key(half, key);
                        settings.dirty = true;
                    }
                }
            };

            if binding_row(
                ui,
                settings,
                name,
                BUTTON_NAME,
                chips,
                capture.waiting_for(target),
            ) {
                capture.start(target, BindingKind::Keyboard);
            }
        });
    }
}

fn shortcuts_table(ui: &mut Ui, settings: &mut Settings, capture: &mut BindingCapture) {
    for (index, action) in HotkeyAction::iter().enumerate() {
        let target = BindingTarget::Shortcut(action);

        striped_row(ui, index, |ui| {
            let name = |ui: &mut Ui| {
                let in_cell = ui.available_width() <= ACTION_NAME.width;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(action.label()).strong());
                    if in_cell {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            info_mark(ui, action.description());
                        });
                    } else {
                        info_mark(ui, action.description());
                    }
                });
            };
            let chips = |ui: &mut Ui, settings: &mut Settings| {
                let bound = settings.hotkeys.keys_for(action);
                let pads = settings.hotkeys.pads_for(action);
                if bound.is_empty() && pads.is_empty() {
                    ui.label(RichText::new("Nothing bound").weak().small());
                }
                for hotkey in bound {
                    let label = hotkey.to_string();
                    if chip(ui, icons::KEYBOARD, &label, &label) {
                        settings.hotkeys.unbind(hotkey);
                        settings.dirty = true;
                    }
                }
                for pad in pads {
                    if chip(
                        ui,
                        icons::GAME_CONTROLLER,
                        pad.short_name(),
                        &pad.to_string(),
                    ) {
                        settings.hotkeys.unbind_pad(pad);
                        settings.dirty = true;
                    }
                }
            };

            if binding_row(
                ui,
                settings,
                name,
                ACTION_NAME,
                chips,
                capture.waiting_for(target),
            ) {
                let kind = if action.is_hold() {
                    BindingKind::Keyboard
                } else {
                    BindingKind::Any
                };
                capture.start(target, kind);
            }
        });
    }
}

fn binding_row(
    ui: &mut Ui,
    settings: &mut Settings,
    name: impl Fn(&mut Ui),
    cell: NameCell,
    chips: impl FnOnce(&mut Ui, &mut Settings),
    waiting: bool,
) -> bool {
    let stacked = ui.available_width() - cell.width < STACK_BELOW;
    let mut add = false;

    ui.vertical(|ui| {
        if stacked {
            name(ui);
        }
        ui.horizontal_top(|ui| {
            if !stacked {
                let layout = match cell.height {
                    Some(_) => egui::Layout::left_to_right(egui::Align::Center),
                    None => egui::Layout::top_down(egui::Align::LEFT),
                };
                ui.allocate_ui_with_layout(
                    egui::vec2(cell.width, cell.height.unwrap_or(0.0)),
                    layout,
                    |ui| {
                        ui.set_min_width(cell.width);
                        name(ui);
                    },
                );
            }
            ui.horizontal_wrapped(|ui| {
                add = add_button(ui, waiting);
                chips(ui, settings);
            });
        });
    });

    add
}

fn striped_row(ui: &mut Ui, index: usize, body: impl FnOnce(&mut Ui)) {
    let fill = if index % 2 == 1 {
        ui.visuals().text_color().gamma_multiply(0.05)
    } else {
        egui::Color32::TRANSPARENT
    };

    egui::Frame::new()
        .fill(fill)
        .inner_margin(egui::Margin::symmetric(6, 4))
        .corner_radius(4)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            body(ui);
        });
}

fn chip(ui: &mut Ui, icon: &str, label: &str, full: &str) -> bool {
    let visuals = ui.visuals();
    let text = RichText::new(format!("{icon} {label}")).size(11.0);
    let response = egui::Button::new(text)
        .fill(visuals.widgets.inactive.weak_bg_fill)
        .stroke(visuals.widgets.inactive.bg_stroke)
        .corner_radius(4)
        .min_size(egui::vec2(0.0, CHIP_HEIGHT))
        .ui(ui);

    response
        .on_hover_text(format!("{full}\n{} Click to unbind", icons::X))
        .clicked()
}

fn add_button(ui: &mut Ui, waiting: bool) -> bool {
    let label = if waiting { "…" } else { icons::PLUS };
    let mut button = egui::Button::new(RichText::new(label).size(11.0))
        .corner_radius(4)
        .min_size(egui::vec2(ADD_WIDTH - 6.0, CHIP_HEIGHT));
    if waiting {
        button = button.fill(ui.visuals().selection.bg_fill);
    }
    button.ui(ui).on_hover_text("Bind another input").clicked()
}

fn reset_button(ui: &mut Ui, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(format!(
            "{} Reset to defaults",
            icons::ARROW_COUNTER_CLOCKWISE
        )),
    )
    .clicked()
}

fn touch_grid(ui: &mut Ui, settings: &mut Settings) -> Response {
    let s = &mut settings.touch;
    let mut dirty = false;
    let mut ui_scale_response = None;

    let response = ui
        .scope(|ui| {
            Form::show(ui, "controls", |form| {
                form.row("Show buttons", |ui| {
                    dirty |= ui.checkbox(&mut s.show_controls, "").changed();
                });

                form.row("Button size", |ui| {
                    dirty |= ResetSlider::new(
                        &mut s.scale,
                        TouchSettings::MIN_SCALE..=TouchSettings::MAX_SCALE,
                    )
                    .step_by(0.05)
                    .default_value(1.0)
                    .ui(ui)
                    .changed();
                });

                form.row("Opacity", |ui| {
                    dirty |= ResetSlider::new(&mut s.opacity, TouchSettings::MIN_OPACITY..=1.0)
                        .step_by(0.05)
                        .default_value(TouchSettings::DEFAULT_OPACITY)
                        .ui(ui)
                        .changed();
                });

                form.row("UI size", |ui| {
                    ui_scale_response = Some(
                        ResetSlider::new(
                            &mut s.ui_scale,
                            TouchSettings::MIN_UI_SCALE..=TouchSettings::MAX_UI_SCALE,
                        )
                        .step_by(0.05)
                        .default_value(1.0)
                        .ui(ui),
                    );
                });

                form.row_with_hint(
                    "Screen scaling",
                    "Separate from the desktop setting.",
                    |ui| {
                        dirty |= GenericSelect::from_enum(&mut s.scaling, "touch_scaling_select")
                            .default_value(ScalingMode::Fit)
                            .ui(ui)
                            .changed();
                    },
                );

                form.row("In landscape", |ui| {
                    dirty |= GenericSelect::from_enum(&mut s.landscape, "touch_landscape_select")
                        .default_value(LandscapeControls::default())
                        .ui(ui)
                        .changed();
                });

                form.row_with_hint("Left-handed", "Puts the d-pad on the right.", |ui| {
                    dirty |= ui.checkbox(&mut s.mirrored, "").changed();
                });

                form.row("Vibrate on press", |ui| {
                    dirty |= ui.checkbox(&mut s.haptics, "").changed();
                });
            })
        })
        .response;

    settings.dirty |= dirty;
    if let Some(scale) = ui_scale_response {
        settings.note_scale_drag(&scale);
    }
    response
}
