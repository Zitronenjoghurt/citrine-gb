use super::KeysPart;
use crate::app::mobile::kit;
use crate::app::ui_state::hotkeys::Hotkeys;
use crate::app::ui_state::keybinds::BindingCapture;
use crate::app::ui_state::settings::Settings;
use crate::app::widgets::info_mark::info_mark;
use crate::app::widgets::settings::controls::{self, Input, Line};
use crate::icons;
use crate::local_link::Half;
use egui::text::{LayoutJob, TextFormat};
use egui::{Align, CornerRadius, FontId, Layout, RichText, Stroke, Ui, vec2};

const CHIP_HEIGHT: f32 = 32.0;

pub(super) fn sheet_body(
    ui: &mut Ui,
    settings: &mut Settings,
    capture: &mut BindingCapture,
    part: KeysPart,
) {
    controls::follow_capture(settings, capture);
    if capture.is_waiting() {
        controls::capture_banner(ui, capture, settings);
        ui.add_space(8.0);
    }
    if part == KeysPart::Buttons
        && let Some(note) = controls::unbound_note(settings)
    {
        let warn = ui.visuals().warn_fg_color;
        ui.colored_label(warn, note);
        ui.add_space(8.0);
    }
    let groups: Vec<(Option<&str>, Vec<Line>)> = match part {
        KeysPart::Buttons => vec![(None, controls::button_lines(settings))],
        KeysPart::Shortcuts => vec![(None, controls::shortcut_lines(settings))],
        KeysPart::Split => Half::BOTH
            .into_iter()
            .map(|half| (Some(half.label()), controls::split_lines(settings, half)))
            .collect(),
        KeysPart::Tilt => vec![(None, controls::tilt_lines(settings))],
    };
    for (title, lines) in groups {
        if let Some(title) = title {
            kit::section_label(ui, title);
        }
        kit::list(ui, |list| {
            for line in &lines {
                list.custom(|ui| line_row(ui, settings, capture, line));
            }
        });
        ui.add_space(12.0);
    }
    let changed = !is_default(settings, part);
    let reset = ui
        .add_enabled_ui(changed, |ui| {
            kit::wide_button(ui, "Reset to defaults", false)
        })
        .inner;
    if reset.clicked() {
        match part {
            KeysPart::Buttons => settings.bindings.reset_buttons(),
            KeysPart::Shortcuts => settings.hotkeys = Hotkeys::default(),
            KeysPart::Split => settings.local_link.reset_keys(),
            KeysPart::Tilt => settings.bindings.reset_tilt_keys(),
        }
        capture.cancel();
        settings.dirty = true;
    }
}

fn is_default(settings: &Settings, part: KeysPart) -> bool {
    match part {
        KeysPart::Buttons => settings.bindings.buttons_are_default(),
        KeysPart::Shortcuts => settings.hotkeys.is_default(),
        KeysPart::Split => settings.local_link.keys_are_default(),
        KeysPart::Tilt => settings.bindings.tilt_keys_are_default(),
    }
}

fn line_row(ui: &mut Ui, settings: &mut Settings, capture: &mut BindingCapture, line: &Line) {
    let waiting = controls::waiting_text(capture, line.target);
    let mut add = false;
    let mut gone = None;
    let size = vec2(ui.available_width(), kit::ROW_HEIGHT);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_height(kit::ROW_HEIGHT);
        ui.add_space(12.0);
        let ink = ui.visuals().strong_text_color();
        ui.add(egui::Label::new(RichText::new(&line.name).size(16.0).color(ink)).truncate());
        if let Some(info) = line.info {
            info_mark(ui, info);
        }
        let chips = Layout::right_to_left(Align::Center).with_main_wrap(true);
        ui.with_layout(chips, |ui| {
            ui.add_space(12.0);
            add = add_button(ui, waiting.as_deref());
            if line.inputs.is_empty() && waiting.is_none() {
                let weak = kit::value_color(ui.visuals());
                ui.label(RichText::new("Nothing bound").size(14.0).color(weak));
            }
            for (index, input) in line.inputs.iter().enumerate().rev() {
                if chip(ui, input).clicked() {
                    gone = Some(index);
                }
            }
        });
    });
    if let Some(index) = gone {
        controls::unbind(settings, &line.inputs[index]);
    }
    if add {
        line.start(capture);
    }
}

fn chip(ui: &mut Ui, input: &Input) -> egui::Response {
    let visuals = ui.visuals();
    let font = FontId::proportional(15.0);
    let mut job = LayoutJob::default();
    let text = format!("{} {}", input.icon, input.text);
    job.append(
        &text,
        0.0,
        TextFormat::simple(font.clone(), visuals.strong_text_color()),
    );
    job.append(
        icons::X,
        8.0,
        TextFormat::simple(font, kit::value_color(visuals)),
    );
    let fill = kit::icon_tile_fill(visuals);
    ui.add(
        egui::Button::new(job)
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(8))
            .min_size(vec2(CHIP_HEIGHT, CHIP_HEIGHT)),
    )
    .on_hover_text(format!("{}, tap to unbind", input.full))
}

fn add_button(ui: &mut Ui, waiting: Option<&str>) -> bool {
    let visuals = ui.visuals();
    let (text, fill) = match waiting {
        Some(held) => (
            RichText::new(held)
                .size(15.0)
                .color(visuals.selection.stroke.color),
            visuals.selection.bg_fill,
        ),
        None => (
            RichText::new(icons::PLUS)
                .size(18.0)
                .color(visuals.strong_text_color()),
            kit::icon_tile_fill(visuals),
        ),
    };
    let side = CHIP_HEIGHT.max(ui.spacing().interact_size.y);
    ui.add(
        egui::Button::new(text)
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same((side / 2.0) as u8))
            .min_size(vec2(side, side)),
    )
    .on_hover_text("Bind another input")
    .clicked()
}
