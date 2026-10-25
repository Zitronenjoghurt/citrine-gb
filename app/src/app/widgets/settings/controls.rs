use crate::app::ui_state::hotkeys::{Hotkey, HotkeyAction, Hotkeys};
use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, BindingTarget, Captured, GbButton, KeyBindings,
};
use crate::app::ui_state::settings::{ScalingMode, Settings};
use crate::app::ui_state::touch::{LandscapeControls, TouchSettings};
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::icons;
use egui::{Response, RichText, Ui, Widget};
use strum::IntoEnumIterator;

pub fn show(ui: &mut Ui, settings: &mut Settings, capture: &mut BindingCapture) {
    apply_capture(settings, capture);
    capture_banner(ui, capture);

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
        ui.horizontal(|ui| {
            if reset_button(ui, !settings.bindings.is_default()) {
                settings.bindings = KeyBindings::default();
                capture.cancel();
                settings.dirty = true;
            }
            settings.dirty |= ui
                .checkbox(&mut settings.bindings.stick_as_dpad, "Left stick steers")
                .on_hover_text("Also read the analog stick as a d-pad")
                .changed();
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

    section(ui, icons::DEVICE_MOBILE, "On-screen buttons", |ui| {
        ui.label(
            RichText::new("The touch layout shows these on phones and small windows.")
                .small()
                .weak(),
        );
        ui.add_space(6.0);
        touch_grid(ui, settings);
    });
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
            settings.bindings.bind_pad(pad, button);
        }
        (BindingTarget::Shortcut(action), Captured::Key(key, modifiers)) => {
            settings
                .hotkeys
                .bind(Hotkey::from_event(key, modifiers), action);
        }
        (BindingTarget::Shortcut(_), Captured::Pad(_)) => return,
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
                    if ui.small_button("Cancel").clicked() {
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

fn section(ui: &mut Ui, icon: &str, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(15.0));
        ui.label(RichText::new(title).strong().size(15.0));
    });
    ui.separator();
    ui.add_space(2.0);
    body(ui);
    ui.add_space(14.0);
}

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
    width: 150.0,
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
                    if chip(ui, icons::KEYBOARD, key.name(), key.name()) {
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

fn shortcuts_table(ui: &mut Ui, settings: &mut Settings, capture: &mut BindingCapture) {
    for (index, action) in HotkeyAction::iter().enumerate() {
        let target = BindingTarget::Shortcut(action);

        striped_row(ui, index, |ui| {
            let name = |ui: &mut Ui| {
                ui.label(RichText::new(action.label()).strong());
                ui.label(RichText::new(action.description()).weak().small());
            };
            let chips = |ui: &mut Ui, settings: &mut Settings| {
                let bound = settings.hotkeys.keys_for(action);
                if bound.is_empty() {
                    ui.label(RichText::new("Nothing bound").weak().small());
                }
                for hotkey in bound {
                    let label = hotkey.to_string();
                    if chip(ui, icons::KEYBOARD, &label, &label) {
                        settings.hotkeys.unbind(hotkey);
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
                capture.start(target, BindingKind::Keyboard);
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

    let response = egui::Grid::new("touch_settings_grid")
        .num_columns(2)
        .spacing([16.0, 8.0])
        .min_col_width(140.0)
        .show(ui, |ui| {
            ui.label("Show buttons");
            dirty |= ui.checkbox(&mut s.show_controls, "").changed();
            ui.end_row();

            ui.label("Button size");
            dirty |= ResetSlider::new(
                &mut s.scale,
                TouchSettings::MIN_SCALE..=TouchSettings::MAX_SCALE,
            )
            .step_by(0.05)
            .default_value(1.0)
            .ui(ui)
            .changed();
            ui.end_row();

            ui.label("Opacity");
            dirty |= ResetSlider::new(&mut s.opacity, TouchSettings::MIN_OPACITY..=1.0)
                .step_by(0.05)
                .default_value(TouchSettings::DEFAULT_OPACITY)
                .ui(ui)
                .changed();
            ui.end_row();

            ui.label("UI size");
            ui_scale_response = Some(
                ResetSlider::new(
                    &mut s.ui_scale,
                    TouchSettings::MIN_UI_SCALE..=TouchSettings::MAX_UI_SCALE,
                )
                .step_by(0.05)
                .default_value(1.0)
                .ui(ui),
            );
            ui.end_row();

            ui.label("Screen scaling");
            dirty |= GenericSelect::from_enum(&mut s.scaling, "touch_scaling_select")
                .default_value(ScalingMode::Fit)
                .ui(ui)
                .on_hover_text("Separate from the desktop setting")
                .changed();
            ui.end_row();

            ui.label("In landscape");
            dirty |= GenericSelect::from_enum(&mut s.landscape, "touch_landscape_select")
                .default_value(LandscapeControls::default())
                .ui(ui)
                .changed();
            ui.end_row();

            ui.label("Left-handed");
            dirty |= ui
                .checkbox(&mut s.mirrored, "")
                .on_hover_text("Put the d-pad on the right")
                .changed();
            ui.end_row();

            ui.label("Vibrate on press");
            dirty |= ui.checkbox(&mut s.haptics, "").changed();
            ui.end_row();
        })
        .response;

    settings.dirty |= dirty;
    if let Some(scale) = ui_scale_response {
        settings.note_scale_drag(&scale);
    }
    response
}
