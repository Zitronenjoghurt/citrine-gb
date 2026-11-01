use crate::app::tabs::TabViewer;
use crate::app::ui_state::keybinds::GbButton;
use crate::app::widgets::settings::controls::section;
use crate::app::widgets::settings::form::Form;
use crate::controller_debug::{PadInfo, REPORT_FILE, Report, SystemInfo, capture_text, steer_text};
use crate::icons;
use crate::utils::file_saver::FileSaver;
use egui::{Label, RichText, Ui, Widget};
use strum::IntoEnumIterator;

const SHOWN_EVENTS: usize = 200;
const CHIP_HEIGHT: f32 = 20.0;
const AXIS_WIDTH: f32 = 88.0;

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let system = SystemInfo::current(viewer.pads.api());
    let pads = pads(viewer);
    let held: Vec<GbButton> = GbButton::iter()
        .filter(|b| viewer.emulator.is_held(b.joypad()))
        .collect();
    let focused = ui.input(|i| i.viewport().focused);
    let bindings = &viewer.ui.settings.bindings;
    let deadzone = bindings.stick_as_dpad.then(|| bindings.deadzone());

    ui.horizontal(|ui| {
        let copy = ui.button(format!("{} Copy report", icons::COPY));
        let save = ui.button(format!("{} Save report", icons::DOWNLOAD_SIMPLE));
        if copy.clicked() || save.clicked() {
            let report = Report {
                system: &system,
                focused,
                pads: &pads,
                bindings: &viewer.ui.settings.bindings,
                capture: viewer.ui.capture.pending,
                held: &held,
                log: viewer.ui.controller_debug.log(),
            }
            .to_string();
            if copy.clicked() {
                ui.ctx().copy_text(report);
                viewer.events.notify("Report copied");
            } else {
                FileSaver::new(REPORT_FILE)
                    .add_filter("Text", &["txt"])
                    .dispatch(report.into_bytes(), viewer.files.save_tx.clone());
            }
        }
    });
    ui.add_space(10.0);

    section(ui, icons::GAME_CONTROLLER, "Controllers", |ui| {
        if pads.is_empty() {
            ui.label(
                RichText::new("No controller found. Connect one and press a button on it.").weak(),
            );
        }
        for (i, pad) in pads.iter().enumerate() {
            if i > 0 {
                ui.add_space(12.0);
            }
            pad_rows(ui, pad, deadzone);
        }
    });

    section(ui, icons::JOYSTICK, "Game Boy buttons held", |ui| {
        ui.horizontal_wrapped(|ui| {
            for button in GbButton::iter() {
                chip(ui, &button.to_string(), held.contains(&button));
            }
        });
    });

    section(ui, icons::DESKTOP, "System", |ui| {
        Form::show(ui, "controller_debug_system", |form| {
            form.striped();
            form.row("Citrine", |ui| value(ui, system.version));
            form.row("Build", |ui| value(ui, &system.build));
            form.row("OS", |ui| value(ui, &system.os));
            form.row("Controller API", |ui| value(ui, &system.backend));
            let focus = match focused {
                Some(true) => "yes",
                Some(false) => "no",
                None => "unknown",
            };
            form.row("Window focused", |ui| value(ui, focus));
            form.row("Left stick steers", |ui| {
                value(ui, &steer_text(&viewer.ui.settings.bindings))
            });
            form.row("Binding capture", |ui| {
                value(ui, &capture_text(viewer.ui.capture.pending))
            });
        });
    });

    let count = viewer.ui.controller_debug.log().map_or(0, |log| log.len());
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(icons::LIST_BULLETS).size(15.0));
        ui.label(RichText::new("Event log").strong().size(15.0));
        ui.label(RichText::new(count.to_string()).weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clear = ui.add_enabled(
                count > 0,
                egui::Button::new(format!("{} Clear", icons::TRASH)),
            );
            if clear.clicked()
                && let Some(log) = viewer.ui.controller_debug.log_mut()
            {
                log.clear(web_time::Instant::now());
            }
        });
    });
    ui.separator();
    ui.add_space(2.0);
    ui.label(
        RichText::new("Clears when you close this tab.")
            .small()
            .weak(),
    );
    ui.add_space(4.0);

    let Some(log) = viewer.ui.controller_debug.log() else {
        return;
    };
    if log.is_empty() {
        ui.label(RichText::new("Press a button on a controller.").weak());
        return;
    }
    for entry in log.entries().rev().take(SHOWN_EVENTS) {
        ui.horizontal_top(|ui| {
            ui.label(RichText::new(entry.time()).monospace().size(11.0).weak());
            ui.add(Label::new(RichText::new(entry.event()).monospace().size(11.0)).wrap());
        });
    }
    let hidden = log.len().saturating_sub(SHOWN_EVENTS) + log.dropped();
    if hidden > 0 {
        ui.label(
            RichText::new(format!("{hidden} older events are in the report only."))
                .small()
                .weak(),
        );
    }
}

fn pads(viewer: &TabViewer) -> Vec<PadInfo> {
    let seen = viewer
        .ui
        .controller_debug
        .log()
        .map_or(&[][..], |log| log.seen());
    viewer.pads.pads(seen)
}

fn pad_rows(ui: &mut Ui, pad: &PadInfo, deadzone: Option<f32>) {
    let title = format!("Pad {}  {}", pad.id, pad.name);
    ui.add(Label::new(RichText::new(&title).strong()).truncate())
        .on_hover_text(&title);
    ui.add_space(4.0);
    Form::show(ui, ("controller_debug_pad", pad.id), |form| {
        form.striped();
        for (label, text) in &pad.details {
            form.row(label, |ui| value(ui, text).on_hover_text(text));
        }
        for axis in &pad.axes {
            form.row(&axis.name, |ui| {
                let deadzone = deadzone.filter(|_| axis.axis.is_left_stick());
                axis_bar(ui, axis.value, deadzone);
                ui.label(RichText::new(format!("{:+.2}", axis.value)).monospace())
                    .on_hover_text(format!("Code {}", axis.code))
            });
        }
        for raw in &pad.raw {
            form.row(&format!("Unnamed {}", raw.code), |ui| {
                value(ui, &format!("{:.2}", raw.value))
            });
        }
    });
    ui.add_space(6.0);
    if pad.buttons.is_empty() {
        ui.label(RichText::new("No named buttons.").weak());
    }
    ui.horizontal_wrapped(|ui| {
        for button in &pad.buttons {
            chip(ui, &button.name, button.pressed).on_hover_text(format!("Code {}", button.code));
        }
    });
}

fn axis_bar(ui: &mut Ui, value: f32, deadzone: Option<f32>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(AXIS_WIDTH, 8.0), egui::Sense::hover());
    let visuals = ui.visuals();
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, visuals.extreme_bg_color);
    let centre = rect.center().x;
    let end = centre + value.clamp(-1.0, 1.0) * rect.width() / 2.0;
    let fill = egui::Rect::from_x_y_ranges(centre.min(end)..=centre.max(end), rect.y_range());
    let inside = deadzone.is_some_and(|zone| value.abs() <= zone);
    let colour = if inside {
        visuals.widgets.inactive.bg_fill
    } else {
        visuals.selection.bg_fill
    };
    painter.rect_filled(fill, 2.0, colour);
    painter.vline(
        centre,
        rect.y_range(),
        visuals.widgets.noninteractive.bg_stroke,
    );
    if let Some(zone) = deadzone {
        for side in [-1.0, 1.0] {
            painter.vline(
                centre + side * zone * rect.width() / 2.0,
                rect.y_range(),
                visuals.widgets.noninteractive.fg_stroke,
            );
        }
    }
}

fn chip(ui: &mut Ui, label: &str, lit: bool) -> egui::Response {
    let visuals = ui.visuals();
    let (fill, text) = if lit {
        (
            visuals.selection.bg_fill,
            RichText::new(label)
                .size(11.0)
                .color(visuals.selection.stroke.color),
        )
    } else {
        (
            visuals.widgets.inactive.weak_bg_fill,
            RichText::new(label).size(11.0).weak(),
        )
    };
    egui::Button::new(text)
        .fill(fill)
        .stroke(visuals.widgets.inactive.bg_stroke)
        .corner_radius(4)
        .min_size(egui::vec2(0.0, CHIP_HEIGHT))
        .sense(egui::Sense::hover())
        .ui(ui)
}

fn value(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(Label::new(text).truncate())
}
