use crate::app::big_screen::{Backdrop, ButtonIcons, ConfirmButton};
use crate::app::ui_state::hotkeys::{Hotkey, HotkeyAction, Hotkeys};
use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, BindingTarget, Captured, GbButton, KeyBindings, key_label,
};
use crate::app::ui_state::settings::{ScalingMode, Settings};
use crate::app::ui_state::touch::{HiddenButtons, LandscapeControls, TouchSettings};
use crate::app::widgets;
use crate::app::widgets::chip::{CHIP_HEIGHT, chip_text};
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::info_mark::info_mark;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::{Heading, Page};
use crate::app::widgets::text::faint;
use crate::icons;
use crate::local_link::Half;
use egui::{Response, RichText, Ui, Widget};
use strum::IntoEnumIterator;

#[cfg(target_arch = "wasm32")]
pub const RINGER_HINT: &str = "Chrome on Android vibrates only while the ringer is on.";

pub fn show(ui: &mut Ui, settings: &mut Settings, capture: &mut BindingCapture) -> bool {
    Page::show(ui, |page| {
        let debug = bindings(page, settings, capture);
        page.boxed(
            Some(Heading::new(icons::TELEVISION_SIMPLE, "Big screen")),
            |ui| {
                Form::show(ui, "controls_big_screen", |form| {
                    form.row_with_hint("Confirm button", ConfirmButton::NOTE, |ui| {
                        settings.dirty |= GenericSelect::from_enum(
                            &mut settings.big_screen.confirm,
                            "enum_select_confirm_button",
                        )
                        .default_value(ConfirmButton::default())
                        .ui(ui)
                        .changed();
                    });
                    form.row("Button icons", |ui| {
                        settings.dirty |= GenericSelect::from_enum(
                            &mut settings.big_screen.icons,
                            "enum_select_button_icons",
                        )
                        .default_value(ButtonIcons::default())
                        .ui(ui)
                        .changed();
                    });
                    form.row(crate::app::big_screen::BACKDROP, |ui| {
                        settings.dirty |= GenericSelect::from_enum(
                            &mut settings.big_screen.backdrop,
                            "enum_select_backdrop",
                        )
                        .default_value(Backdrop::default())
                        .ui(ui)
                        .changed();
                    });
                });
            },
        );
        page.section(
            Heading::new(icons::DEVICE_MOBILE, "On-screen buttons")
                .hint("The touch layout shows these on phones and small windows."),
            |ui| {
                touch_grid(ui, settings);
            },
        );
        debug
    })
}

pub fn bindings(page: &mut Page, settings: &mut Settings, capture: &mut BindingCapture) -> bool {
    capture.mark_shown();
    apply_capture(settings, capture);
    if capture.is_waiting() {
        page.plain(|ui| capture_banner(ui, capture));
    }
    let mut debug = false;
    page.boxed(
        Some(Heading::new(icons::GAME_CONTROLLER, BUTTONS_HEADING)),
        |ui| {
            Form::show(ui, "controls_buttons", |form| {
                let unbound = settings.bindings.unbound();
                if !unbound.is_empty() {
                    let names: Vec<String> = unbound.iter().map(|b| b.to_string()).collect();
                    let warn = form.ctx().style().visuals.warn_fg_color;
                    form.custom(|ui| {
                        ui.colored_label(
                            warn,
                            format!("{} has nothing bound to it.", names.join(", ")),
                        )
                    });
                }

                form.custom(|ui| buttons_table(ui, settings, capture));

                settings.dirty |= form.toggle_with_hint(
                    "Left stick steers",
                    "Also reads the analog stick as a d-pad.",
                    &mut settings.bindings.stick_as_dpad,
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
                form.buttons(|ui| {
                    if reset_button(ui, !settings.bindings.is_default()) {
                        settings.bindings = KeyBindings::default();
                        capture.cancel();
                        settings.dirty = true;
                    }
                    debug = ui
                        .button(format!("{} Controller debug", icons::BUG))
                        .on_hover_text(
                            "Live controller state and an input log to send with a bug report",
                        )
                        .clicked();
                });
            });
        },
    );

    page.boxed(Some(Heading::new(icons::KEYBOARD, "Shortcuts")), |ui| {
        Form::show(ui, "controls_shortcuts", |form| {
            form.custom(|ui| shortcuts_table(ui, settings, capture));
            form.buttons(|ui| {
                if reset_button(ui, !settings.hotkeys.is_default()) {
                    settings.hotkeys = Hotkeys::default();
                    capture.cancel();
                    settings.dirty = true;
                }
            });
        });
    });

    page.boxed(
        Some(Heading::new(
            crate::app::tabs::linked::LINK_ICON,
            "Keyboard split in two",
        )),
        |ui| {
            Form::show(ui, "controls_split", |form| {
                for half in Half::BOTH {
                    form.custom(|ui| {
                        ui.label(RichText::new(half.label()).strong());
                        split_table(ui, settings, capture, half);
                    });
                }
                form.buttons(|ui| {
                    if reset_button(ui, !settings.local_link.keys_are_default()) {
                        settings.local_link.reset_keys();
                        capture.cancel();
                        settings.dirty = true;
                    }
                });
            });
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
        (BindingTarget::Shoulder(shoulder), Captured::Key(key, _)) => {
            settings.bindings.bind_shoulder_key(key, shoulder);
        }
        (BindingTarget::Shoulder(shoulder), Captured::Pad(pad)) => {
            settings.hotkeys.unbind_pad(pad);
            settings.bindings.bind_shoulder_pad(pad, shoulder);
        }
        (BindingTarget::Ds(button), Captured::Key(key, _)) => {
            settings.bindings.bind_ds_key(key, button);
        }
        (BindingTarget::Ds(button), Captured::Pad(pad)) => {
            settings.hotkeys.unbind_pad(pad);
            settings.bindings.bind_ds_pad(pad, button);
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
    egui::Frame::new()
        .fill(ui.visuals().selection.bg_fill.gamma_multiply(0.35))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .corner_radius(6)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.strong("Press a key or button…");
                ui.label(faint("Esc to cancel"));
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

#[cfg(feature = "gba")]
const BUTTONS_HEADING: &str = "Buttons";
#[cfg(not(feature = "gba"))]
const BUTTONS_HEADING: &str = "Game Boy buttons";

const STACK_BELOW: f32 = 130.0;

fn buttons_table(ui: &mut Ui, settings: &mut Settings, capture: &mut BindingCapture) {
    for (index, target) in BindingTarget::buttons().enumerate() {
        button_row(ui, settings, capture, index, target);
    }
}

fn button_row(
    ui: &mut Ui,
    settings: &mut Settings,
    capture: &mut BindingCapture,
    index: usize,
    target: BindingTarget,
) {
    let Some(label) = target.button_name() else {
        return;
    };
    striped_row(ui, index, |ui| {
        let name = |ui: &mut Ui| {
            ui.label(RichText::new(&label).strong());
        };
        let chips = |ui: &mut Ui, settings: &mut Settings| {
            let (keys, pads) = settings.bindings.bound_inputs(target);
            if keys.is_empty() && pads.is_empty() {
                ui.label(faint("Nothing bound"));
            }
            for key in keys {
                if chip(ui, icons::KEYBOARD, &key_label(key), key.name()) {
                    settings.bindings.remove_key(target, key);
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
                    settings.bindings.remove_pad(target, pad);
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
                    ui.label(faint("Nothing bound"));
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
    let dev_mode = settings.dev_mode;
    let actions = HotkeyAction::iter()
        .filter(|action| dev_mode || !action.is_debugger())
        .filter(|action| cfg!(feature = "nds") || !action.is_ds());
    for (index, action) in actions.enumerate() {
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
                    ui.label(faint("Nothing bound"));
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
    let stacked = ui.available_width() - ACTION_NAME.width < STACK_BELOW;
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
    let fill = ui.visuals().widgets.inactive.weak_bg_fill;
    let response = widgets::chip::chip(ui, chip_text(format!("{icon} {label}")), fill).ui(ui);

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

    let response = ui
        .scope(|ui| {
            Form::show(ui, "controls", |form| {
                dirty |= form.toggle("Show", &mut s.show_controls);

                form.row("Size", |ui| {
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

                form.row_with_hint("With the UI hidden", HiddenButtons::NOTE, |ui| {
                    dirty |= GenericSelect::from_enum(
                        &mut s.hidden_buttons,
                        "touch_hidden_buttons_select",
                    )
                    .default_value(HiddenButtons::default())
                    .ui(ui)
                    .changed();
                });

                dirty |= form.toggle_with_hint(
                    "Left-handed",
                    "Puts the d-pad on the right.",
                    &mut s.mirrored,
                );

                #[cfg(target_arch = "wasm32")]
                {
                    dirty |= form.toggle_with_hint("Vibrate on press", RINGER_HINT, &mut s.haptics);
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    dirty |= form.toggle("Vibrate on press", &mut s.haptics);
                }
            })
        })
        .response;

    settings.dirty |= dirty;
    response
}
