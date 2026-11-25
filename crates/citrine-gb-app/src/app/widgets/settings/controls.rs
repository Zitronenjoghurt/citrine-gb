use crate::app::big_screen::{Backdrop, ButtonIcons, ConfirmButton};
use crate::app::ui_state::hotkeys::{Hotkey, HotkeyAction, Hotkeys};
use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, BindingTarget, Captured, GameButton, GbButton, KeyBindings,
    PadButton, Tilt, key_label,
};
use crate::app::ui_state::settings::{ScalingMode, Settings};
use crate::app::ui_state::settings_tree::Surface;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::touch::{HiddenButtons, LandscapeControls, TouchSettings};
use crate::app::widgets;
use crate::app::widgets::chip::{CHIP_HEIGHT, chip_text};
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::info_mark::info_mark;
use crate::app::widgets::reset_slider::{RESET_GAP, ResetSlider, percent_slider, reset_width};
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::Page;
use crate::app::widgets::text::faint;
use crate::combo::{Combo, Mods};
use crate::icons;
use crate::local_link::Half;
use egui::{RichText, Ui, Widget};
use strum::IntoEnumIterator;

#[cfg(target_arch = "wasm32")]
pub const RINGER_HINT: &str = "Chrome on Android vibrates only while the ringer is on.";
const CONTROLLER_TEST_HINT: &str =
    "Live controller state and an input log to send with a bug report";

fn label(row: RowId) -> &'static str {
    row.label(Surface::Desktop)
}

pub fn prepare(page: &mut Page, settings: &mut Settings, capture: &mut BindingCapture) {
    follow_capture(settings, capture);
    if capture.is_waiting() {
        page.plain(|ui| capture_banner(ui, capture, settings));
    }
}

pub fn follow_capture(settings: &mut Settings, capture: &mut BindingCapture) {
    capture.mark_shown();
    apply_capture(settings, capture);
    if settings.big_screen.on || crate::platform::is_tv() {
        capture.give_up_if_quiet(web_time::Instant::now());
    }
}

fn title_row(form: &mut Form, title: Option<&str>) {
    if let Some(title) = title {
        form.custom(|ui| ui.label(RichText::new(title).strong()));
    }
}

pub fn game_buttons(
    form: &mut Form,
    settings: &mut Settings,
    capture: &mut BindingCapture,
    title: Option<&str>,
) {
    title_row(form, title);
    if let Some(note) = unbound_note(settings) {
        let warn = form.ctx().style().visuals.warn_fg_color;
        form.custom(|ui| ui.colored_label(warn, note));
    }
    let lines = button_lines(settings);
    let hang = form.hang();
    form.custom(|ui| table(ui, settings, capture, lines, BUTTON_NAME, hang));
    form.buttons(|ui| {
        if reset_button(ui, !settings.bindings.buttons_are_default()) {
            settings.bindings.reset_buttons();
            capture.cancel();
            settings.dirty = true;
        }
    });
}

pub fn shortcuts(
    form: &mut Form,
    settings: &mut Settings,
    capture: &mut BindingCapture,
    title: Option<&str>,
) {
    title_row(form, title);
    let lines = shortcut_lines(settings);
    let hang = form.hang();
    form.custom(|ui| table(ui, settings, capture, lines, ACTION_NAME, hang));
    form.buttons(|ui| {
        if reset_button(ui, !settings.hotkeys.is_default()) {
            settings.hotkeys = Hotkeys::default();
            capture.cancel();
            settings.dirty = true;
        }
    });
}

pub fn split_keys(
    form: &mut Form,
    settings: &mut Settings,
    capture: &mut BindingCapture,
    title: Option<&str>,
) {
    title_row(form, title);
    let hang = form.hang();
    for half in Half::BOTH {
        form.custom(|ui| {
            ui.label(RichText::new(half.label()).strong());
            let lines = split_lines(settings, half);
            table(ui, settings, capture, lines, BUTTON_NAME, hang);
        });
    }
    form.buttons(|ui| {
        if reset_button(ui, !settings.local_link.keys_are_default()) {
            settings.local_link.reset_keys();
            capture.cancel();
            settings.dirty = true;
        }
    });
}

pub fn tilt_keys(
    form: &mut Form,
    settings: &mut Settings,
    capture: &mut BindingCapture,
    title: Option<&str>,
) {
    title_row(form, title);
    let lines = tilt_lines(settings);
    let hang = form.hang();
    form.custom(|ui| table(ui, settings, capture, lines, TILT_NAME, hang));
    form.buttons(|ui| {
        if reset_button(ui, !settings.bindings.tilt_keys_are_default()) {
            settings.bindings.reset_tilt_keys();
            capture.cancel();
            settings.dirty = true;
        }
    });
}

pub fn controller_row(form: &mut Form, settings: &mut Settings, row: RowId) {
    match row {
        RowId::StickSteers => {
            settings.dirty |= form.toggle_with_hint(
                label(row),
                "Also reads the analog stick as a d-pad.",
                &mut settings.bindings.stick_as_dpad,
            );
        }
        RowId::StickDeadZone => {
            form.row_with_hint(label(row), DEAD_ZONE_HINT, |ui| {
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
        }
        RowId::Rumble => {
            settings.dirty |= form.toggle(label(row), &mut settings.gimmicks.rumble);
        }
        _ => {}
    }
}

pub fn controller_test(form: &mut Form) -> bool {
    form.row_with_hint(label(RowId::ControllerTest), CONTROLLER_TEST_HINT, |ui| {
        ui.button("Open").clicked()
    })
}

fn apply_capture(settings: &mut Settings, capture: &mut BindingCapture) {
    let Some((target, captured)) = capture.take() else {
        return;
    };
    match (target, captured) {
        (BindingTarget::LinkKey(half, button), Captured::Keys(keys, _)) => {
            let Some(key) = keys.only() else {
                return;
            };
            settings.local_link.bind_key(half, key, button);
        }
        (BindingTarget::LinkKey(..), Captured::Pads(_)) => return,
        (BindingTarget::Shortcut(action), Captured::Keys(keys, mods)) => {
            if mods == Mods::NONE {
                settings.bindings.unbind_keys(&keys);
            }
            settings.hotkeys.bind(Hotkey::new(keys, mods), action);
        }
        (BindingTarget::Shortcut(action), Captured::Pads(pads)) => {
            if settings.holds(action) {
                return;
            }
            settings.bindings.unbind_pads(&pads);
            settings.hotkeys.bind_pads(pads, action);
        }
        (BindingTarget::Tilt(tilt), Captured::Keys(keys, _)) => {
            let Some(key) = keys.only() else {
                return;
            };
            settings.bindings.bind_tilt_key(tilt, key);
        }
        (BindingTarget::Tilt(_), Captured::Pads(_)) => return,
        (target, captured) => {
            let Some(button) = GameButton::of(target) else {
                return;
            };
            match captured {
                Captured::Keys(keys, _) => {
                    settings.hotkeys.unbind_keys(&keys);
                    settings.bindings.bind_keys(keys, button);
                }
                Captured::Pads(pads) => {
                    settings.hotkeys.unbind_pads(&pads);
                    settings.bindings.bind_pads(pads, button);
                }
            }
        }
    }
    settings.dirty = true;
}

fn keys_text(keys: &Combo<egui::Key>) -> String {
    let names: Vec<String> = keys.inputs().iter().map(|key| key_label(*key)).collect();
    names.join("+")
}

fn pads_text(pads: &Combo<PadButton>) -> String {
    let names: Vec<&str> = pads.inputs().iter().map(|pad| pad.short_name()).collect();
    names.join(" + ")
}

fn held_text(target: BindingTarget, held: &Captured) -> String {
    match (target, held) {
        (BindingTarget::Shortcut(_), Captured::Keys(keys, mods)) => {
            Hotkey::new(keys.clone(), *mods).to_string()
        }
        (_, Captured::Keys(keys, _)) => keys_text(keys),
        (_, Captured::Pads(pads)) => pads_text(pads),
    }
}

fn taken_by(settings: &Settings, target: BindingTarget, held: &Captured) -> Option<String> {
    let bindings = &settings.bindings;
    let hotkeys = &settings.hotkeys;
    let shortcut = |owner: Option<HotkeyAction>| {
        owner
            .filter(|owner| BindingTarget::Shortcut(*owner) != target)
            .map(|owner| owner.label().to_owned())
    };
    let named = |owner: Option<GameButton>| owner.map(|owner| owner.to_string());
    if let BindingTarget::Shortcut(_) = target {
        return match held {
            Captured::Keys(keys, mods) => {
                shortcut(hotkeys.owner_of(&Hotkey::new(keys.clone(), *mods))).or_else(|| {
                    (*mods == Mods::NONE)
                        .then(|| named(bindings.owner_of_keys(keys, true)))
                        .flatten()
                })
            }
            Captured::Pads(pads) => shortcut(hotkeys.owner_of_pads(pads))
                .or_else(|| named(bindings.owner_of_pads(pads, true))),
        };
    }
    let button = GameButton::of(target)?;
    match held {
        Captured::Keys(keys, _) => named(bindings.moves_keys_from(keys, button))
            .or_else(|| shortcut(hotkeys.owner_of(&Hotkey::new(keys.clone(), Mods::NONE)))),
        Captured::Pads(pads) => named(bindings.moves_pads_from(pads, button))
            .or_else(|| shortcut(hotkeys.owner_of_pads(pads))),
    }
}

fn idle_text(capture: &BindingCapture) -> &'static str {
    match capture.pending {
        Some(pending)
            if matches!(
                pending.target,
                BindingTarget::LinkKey(..) | BindingTarget::Tilt(_)
            ) =>
        {
            "Press a key"
        }
        Some(pending) if pending.kind == BindingKind::Keyboard => "Hold the keys, then let go",
        Some(pending) if pending.kind == BindingKind::Gamepad => "Hold the buttons, then let go",
        _ => "Hold the keys or buttons, then let go",
    }
}

pub fn capture_banner(ui: &mut Ui, capture: &mut BindingCapture, settings: &Settings) {
    let held = capture.holding();
    let target = capture.pending.map(|pending| pending.target);
    egui::Frame::new()
        .fill(ui.visuals().selection.bg_fill.gamma_multiply(0.35))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .corner_radius(6)
        .show(ui, |ui| {
            let owner = match (target, &held) {
                (Some(target), Some(held)) => taken_by(settings, target, held),
                _ => None,
            };
            ui.horizontal(|ui| {
                ui.spinner();
                match (target, &held) {
                    (Some(target), Some(held)) => ui.strong(held_text(target, held)),
                    _ => ui.strong(idle_text(capture)),
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::app::mobile::compact_button(ui, "Cancel").clicked() {
                        capture.cancel();
                    }
                });
            });
            if let Some(owner) = owner {
                ui.label(format!("Taken by {owner}. Let go to move it here."));
            }
        });

    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        capture.cancel();
    }
    if settings.big_screen.on || crate::platform::is_tv() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
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

const TILT_NAME: NameCell = NameCell {
    width: 80.0,
    height: Some(CHIP_HEIGHT),
};

const ACTION_NAME: NameCell = NameCell {
    width: 175.0,
    height: None,
};

const ADD_WIDTH: f32 = 26.0;

const STACK_BELOW: f32 = 130.0;

pub struct Line {
    pub target: BindingTarget,
    pub name: String,
    pub info: Option<&'static str>,
    pub inputs: Vec<Input>,
    kind: BindingKind,
}

pub struct Input {
    pub icon: &'static str,
    pub text: String,
    pub full: String,
    unbind: Unbind,
}

enum Unbind {
    Keys(BindingTarget, Combo<egui::Key>),
    Pads(BindingTarget, Combo<PadButton>),
    Hotkey(Hotkey),
    HotkeyPads(Combo<PadButton>),
    LinkKey(Half, egui::Key),
    Tilt(Tilt),
}

impl Line {
    pub fn start(&self, capture: &mut BindingCapture) {
        capture.start(self.target, self.kind);
    }
}

pub fn unbind(settings: &mut Settings, input: &Input) {
    match &input.unbind {
        Unbind::Keys(target, combo) => settings.bindings.remove_keys(*target, combo),
        Unbind::Pads(target, combo) => settings.bindings.remove_pads(*target, combo),
        Unbind::Hotkey(hotkey) => settings.hotkeys.unbind(hotkey),
        Unbind::HotkeyPads(combo) => settings.hotkeys.unbind_pads(combo),
        Unbind::LinkKey(half, key) => settings.local_link.unbind_key(*half, *key),
        Unbind::Tilt(tilt) => settings.bindings.clear_tilt_key(*tilt),
    }
    settings.dirty = true;
}

fn key_input(key: egui::Key, unbind: Unbind) -> Input {
    Input {
        icon: icons::KEYBOARD,
        text: key_label(key),
        full: key.name().to_owned(),
        unbind,
    }
}

pub fn button_lines(settings: &Settings) -> Vec<Line> {
    BindingTarget::buttons()
        .filter_map(|target| {
            let name = target.button_name()?;
            let keys = settings
                .bindings
                .key_combos(target)
                .into_iter()
                .map(|combo| {
                    let full: Vec<&str> = combo.inputs().iter().map(|key| key.name()).collect();
                    Input {
                        icon: icons::KEYBOARD,
                        text: keys_text(&combo),
                        full: full.join("+"),
                        unbind: Unbind::Keys(target, combo),
                    }
                });
            let pads = settings
                .bindings
                .pad_combos(target)
                .into_iter()
                .map(|combo| Input {
                    icon: icons::GAME_CONTROLLER,
                    text: pads_text(&combo),
                    full: combo.to_string(),
                    unbind: Unbind::Pads(target, combo),
                });
            Some(Line {
                target,
                name,
                info: None,
                inputs: keys.chain(pads).collect(),
                kind: BindingKind::Any,
            })
        })
        .collect()
}

pub fn split_lines(settings: &Settings, half: Half) -> Vec<Line> {
    GbButton::iter()
        .map(|button| Line {
            target: BindingTarget::LinkKey(half, button),
            name: button.to_string(),
            info: None,
            inputs: settings
                .local_link
                .keys_for(half, button)
                .into_iter()
                .map(|key| key_input(key, Unbind::LinkKey(half, key)))
                .collect(),
            kind: BindingKind::Keyboard,
        })
        .collect()
}

pub fn tilt_lines(settings: &Settings) -> Vec<Line> {
    Tilt::iter()
        .map(|tilt| Line {
            target: BindingTarget::Tilt(tilt),
            name: tilt.to_string(),
            info: None,
            inputs: settings
                .bindings
                .tilt_key(tilt)
                .map(|key| key_input(key, Unbind::Tilt(tilt)))
                .into_iter()
                .collect(),
            kind: BindingKind::Keyboard,
        })
        .collect()
}

pub fn shortcut_lines(settings: &Settings) -> Vec<Line> {
    let dev_mode = settings.dev_mode;
    HotkeyAction::iter()
        .filter(|action| dev_mode || !action.is_debugger())
        .filter(|action| cfg!(feature = "nds") || !(action.is_ds() || action.is_talk()))
        .map(|action| {
            let keys = settings.hotkeys.keys_for(action).into_iter().map(|hotkey| {
                let label = hotkey.to_string();
                Input {
                    icon: icons::KEYBOARD,
                    text: label.clone(),
                    full: label,
                    unbind: Unbind::Hotkey(hotkey),
                }
            });
            let pads = settings
                .hotkeys
                .pads_for(action)
                .into_iter()
                .map(|combo| Input {
                    icon: icons::GAME_CONTROLLER,
                    text: pads_text(&combo),
                    full: combo.to_string(),
                    unbind: Unbind::HotkeyPads(combo),
                });
            let kind = if settings.holds(action) {
                BindingKind::Keyboard
            } else {
                BindingKind::Any
            };
            Line {
                target: BindingTarget::Shortcut(action),
                name: action.label().to_owned(),
                info: Some(action.description()),
                inputs: keys.chain(pads).collect(),
                kind,
            }
        })
        .collect()
}

pub fn unbound_note(settings: &Settings) -> Option<String> {
    let unbound = settings.bindings.unbound();
    if unbound.is_empty() {
        return None;
    }
    let names: Vec<String> = unbound.iter().map(|b| b.to_string()).collect();
    Some(format!("{} has nothing bound to it.", names.join(", ")))
}

fn table(
    ui: &mut Ui,
    settings: &mut Settings,
    capture: &mut BindingCapture,
    lines: Vec<Line>,
    cell: NameCell,
    hang: f32,
) {
    for (index, line) in lines.iter().enumerate() {
        striped_row(ui, index, hang, |ui| {
            let name = |ui: &mut Ui| {
                let Some(info) = line.info else {
                    ui.label(RichText::new(&line.name).strong());
                    return;
                };
                let in_cell = ui.available_width() <= cell.width;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&line.name).strong());
                    if in_cell {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            info_mark(ui, info);
                        });
                    } else {
                        info_mark(ui, info);
                    }
                });
            };
            let chips = |ui: &mut Ui, settings: &mut Settings| {
                if line.inputs.is_empty() {
                    ui.label(faint("Nothing bound"));
                }
                for input in &line.inputs {
                    if chip(ui, input.icon, &input.text, &input.full) {
                        unbind(settings, input);
                    }
                }
            };
            if binding_row(
                ui,
                settings,
                name,
                cell,
                chips,
                waiting_text(capture, line.target),
            ) {
                line.start(capture);
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
    waiting: Option<String>,
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
                add = add_button(ui, waiting.as_deref());
                chips(ui, settings);
            });
        });
    });

    add
}

const STRIPE_PAD: i8 = 6;

fn striped_row(ui: &mut Ui, index: usize, hang: f32, body: impl FnOnce(&mut Ui)) {
    let fill = if index % 2 == 1 {
        ui.visuals().text_color().gamma_multiply(0.05)
    } else {
        egui::Color32::TRANSPARENT
    };
    let hang = hang.clamp(0.0, STRIPE_PAD as f32);

    let back = ui.painter().add(egui::Shape::Noop);
    let rect = egui::Frame::new()
        .inner_margin(egui::Margin {
            left: STRIPE_PAD - hang as i8,
            right: STRIPE_PAD,
            top: 4,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            body(ui);
        })
        .response
        .rect;
    ui.painter().set(
        back,
        egui::epaint::RectShape::filled(rect.with_min_x(rect.left() - hang), 4, fill),
    );
}

fn chip(ui: &mut Ui, icon: &str, label: &str, full: &str) -> bool {
    let fill = ui.visuals().widgets.inactive.weak_bg_fill;
    let response = widgets::chip::chip(ui, chip_text(format!("{icon} {label}")), fill).ui(ui);

    response
        .on_hover_text(format!("{full}\n{} Click to unbind", icons::X))
        .clicked()
}

pub fn waiting_text(capture: &BindingCapture, target: BindingTarget) -> Option<String> {
    if !capture.waiting_for(target) {
        return None;
    }
    Some(
        capture
            .holding()
            .map_or_else(|| "…".to_owned(), |held| held_text(target, &held)),
    )
}

fn add_button(ui: &mut Ui, waiting: Option<&str>) -> bool {
    let mut label = RichText::new(waiting.unwrap_or(icons::PLUS)).size(11.0);
    if waiting.is_some() {
        label = label.color(ui.visuals().selection.stroke.color);
    }
    let mut button = egui::Button::new(label)
        .corner_radius(4)
        .min_size(egui::vec2(ADD_WIDTH - 6.0, CHIP_HEIGHT));
    if waiting.is_some() {
        button = button.fill(ui.visuals().selection.bg_fill);
    }
    button.ui(ui).on_hover_text("Bind another input").clicked()
}

fn reset_button(ui: &mut Ui, enabled: bool) -> bool {
    let width = ui.available_width() - RESET_GAP - reset_width(ui);
    ui.add_enabled(
        enabled,
        egui::Button::new(format!(
            "{} Reset to defaults",
            icons::ARROW_COUNTER_CLOCKWISE
        ))
        .min_size(egui::vec2(width.max(0.0), 0.0)),
    )
    .clicked()
}

pub fn touch_row(form: &mut Form, settings: &mut Settings, row: RowId) {
    let s = &mut settings.touch;
    let mut dirty = false;
    match row {
        RowId::TouchShow => dirty |= form.toggle(label(row), &mut s.show_controls),
        RowId::TouchSize => {
            form.row(label(row), |ui| {
                dirty |= percent_slider(
                    ui,
                    &mut s.scale,
                    TouchSettings::MIN_SCALE..=TouchSettings::MAX_SCALE,
                    0.05,
                    1.0,
                )
                .changed();
            });
        }
        RowId::TouchOpacity => {
            form.row(label(row), |ui| {
                dirty |= percent_slider(
                    ui,
                    &mut s.opacity,
                    TouchSettings::MIN_OPACITY..=1.0,
                    0.05,
                    TouchSettings::DEFAULT_OPACITY,
                )
                .changed();
            });
        }
        RowId::TouchScaling => {
            form.row(label(row), |ui| {
                dirty |= GenericSelect::from_enum(&mut s.scaling, "touch_scaling_select")
                    .default_value(ScalingMode::Fit)
                    .ui(ui)
                    .changed();
            });
        }
        RowId::TouchLandscape => {
            form.row(label(row), |ui| {
                dirty |= GenericSelect::from_enum(&mut s.landscape, "touch_landscape_select")
                    .default_value(LandscapeControls::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::TouchHidden => {
            form.row(label(row), |ui| {
                dirty |=
                    GenericSelect::from_enum(&mut s.hidden_buttons, "touch_hidden_buttons_select")
                        .default_value(HiddenButtons::default())
                        .ui(ui)
                        .changed();
            });
        }
        RowId::LeftHanded => {
            dirty |=
                form.toggle_with_hint(label(row), "Puts the d-pad on the right.", &mut s.mirrored);
        }
        RowId::Haptics => {
            #[cfg(target_arch = "wasm32")]
            {
                dirty |= form.toggle_with_hint(label(row), RINGER_HINT, &mut s.haptics);
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                dirty |= form.toggle(label(row), &mut s.haptics);
            }
        }
        _ => {}
    }
    settings.dirty |= dirty;
}

pub fn big_screen_row(form: &mut Form, settings: &mut Settings, row: RowId) {
    let b = &mut settings.big_screen;
    let mut dirty = false;
    match row {
        RowId::BigScreenMode => dirty |= form.toggle(label(row), &mut b.on),
        RowId::ConfirmButton => {
            form.row(label(row), |ui| {
                dirty |= GenericSelect::from_enum(&mut b.confirm, "enum_select_confirm_button")
                    .default_value(ConfirmButton::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::ButtonIcons => {
            form.row(label(row), |ui| {
                dirty |= GenericSelect::from_enum(&mut b.icons, "enum_select_button_icons")
                    .default_value(ButtonIcons::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::AroundTheGame => {
            form.row(label(row), |ui| {
                dirty |= GenericSelect::from_enum(&mut b.backdrop, "enum_select_backdrop")
                    .default_value(Backdrop::default())
                    .ui(ui)
                    .changed();
            });
        }
        _ => {}
    }
    settings.dirty |= dirty;
}
