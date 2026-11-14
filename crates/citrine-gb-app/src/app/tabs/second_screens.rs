use crate::app::mobile::{kit, sheet, touch_square};
use crate::app::second_screens::{Facts, PaneAction, PaneState, choosable, preset_icon, row_line};
use crate::app::tabs::TabViewer;
use crate::app::widgets::field::{code_field, row_field};
use crate::app::widgets::problem::problem_line;
use crate::app::widgets::qr::Qr;
use crate::app::widgets::section::{phone_label, section};
use crate::icons;
use crate::second_screens::{JoinState, Suggestion, SurfaceRow};
use citrine_gb_surface::mode::{Mode, Preset};
use citrine_gb_surface::pairing::Code;
use citrine_gb_surface::proto::SurfaceKind;
use egui::{RichText, Ui};

const QR_SIDE: f32 = 132.0;
const CODE_WIDTH: f32 = 120.0;
const MODE_WIDTH: f32 = 150.0;
const LAN_BLOCKED: &str = "Allow Citrine under System Settings > Privacy & Security > Local Network so devices on this network find this Mac.";

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let pane = &mut viewer.ui.second;
    let facts = pane.facts.clone();
    if facts.has_game {
        host_part(ui, pane, &facts);
        join_part(ui, pane, &facts);
    } else {
        join_part(ui, pane, &facts);
        host_part(ui, pane, &facts);
    }
}

fn host_part(ui: &mut Ui, pane: &mut PaneState, facts: &Facts) {
    if touch_square(ui) > 0.0 {
        phone_host_part(ui, pane, facts);
        return;
    }
    let listed = !facts.rows.is_empty() || facts.windows || facts.display.is_some();
    section(ui, icons::DEVICES, "Devices", |ui| {
        for row in &facts.rows {
            device_row(ui, pane, facts, row);
        }
        if facts.windows {
            open_row(
                ui,
                pane,
                icons::APP_WINDOW,
                "New window",
                "On this device",
                PaneAction::OpenWindow,
            );
        }
        if let Some(name) = &facts.display {
            open_row(
                ui,
                pane,
                icons::MONITOR,
                "Second display",
                name,
                PaneAction::OpenDisplay,
            );
        }
        if listed {
            ui.add_space(6.0);
        }
        match &facts.code {
            Some((code, left)) => {
                code_card(ui, pane, *code, *left, facts.link.as_deref());
                if facts.lan_blocked {
                    ui.label(RichText::new(LAN_BLOCKED).color(ui.visuals().warn_fg_color));
                }
            }
            None => {
                if ui.button(format!("{} Add a device", icons::PLUS)).clicked() {
                    pane.actions.push(PaneAction::AddDevice);
                }
            }
        }
    });
}

fn phone_host_part(ui: &mut Ui, pane: &mut PaneState, facts: &Facts) {
    phone_label(ui, "Devices", None);
    let mut picked = None;
    let mut action = None;
    let listed = !facts.rows.is_empty() || facts.display.is_some() || facts.code.is_none();
    if listed {
        kit::list(ui, |list| {
            for row in &facts.rows {
                let line = row_line(row);
                let tapped = list
                    .row(
                        kit::Row::new(&row.name)
                            .icon(kind_icon(row.kind))
                            .detail(&line)
                            .value(row.mode.label()),
                    )
                    .clicked();
                if tapped {
                    picked = Some(row.id);
                }
            }
            if let Some(name) = &facts.display {
                let display = kit::Row::new("Second display")
                    .icon(icons::MONITOR)
                    .detail(name)
                    .no_chevron();
                if list.row(display).clicked() {
                    action = Some(PaneAction::OpenDisplay);
                }
            }
            if facts.code.is_none() {
                let add = kit::Row::new("Add a device").icon(icons::PLUS).no_chevron();
                if list.row(add).clicked() {
                    action = Some(PaneAction::AddDevice);
                }
            }
        });
    }
    if let Some((code, left)) = facts.code {
        if listed {
            ui.add_space(8.0);
        }
        kit::card(ui, |ui| {
            kit::padded(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                code_card(ui, pane, code, left, facts.link.as_deref());
            });
        });
    }
    ui.add_space(6.0);
    if let Some(action) = action {
        pane.actions.push(action);
    }
    if picked.is_some() {
        pane.device_sheet = picked;
    }
    device_sheet(ui.ctx(), pane, facts);
}

fn device_sheet(ctx: &egui::Context, pane: &mut PaneState, facts: &Facts) {
    let Some(id) = pane.device_sheet else {
        return;
    };
    let Some(row) = facts.rows.iter().find(|r| r.id == id) else {
        pane.device_sheet = None;
        return;
    };
    let mut actions = Vec::new();
    let mut done = false;
    let shown = sheet::show(ctx, egui::Id::new(("surface_sheet", id)), &row.name, |ui| {
        ui.add_space(10.0);
        let custom = matches!(row.mode, Mode::Custom(_));
        kit::list(ui, |list| {
            for preset in Preset::ALL
                .into_iter()
                .filter(|p| p.offered(&facts.screens))
            {
                let on = row.mode == Mode::Preset(preset);
                let choice = kit::Row::new(preset.label())
                    .icon(preset_icon(preset))
                    .checked(on);
                if list.row(choice).clicked() && !on {
                    actions.push(PaneAction::SetMode(id, Mode::Preset(preset)));
                }
            }
            let choice = kit::Row::new("Custom")
                .icon(icons::SLIDERS_HORIZONTAL)
                .checked(custom);
            if list.row(choice).clicked() && !custom {
                let views = row.mode.views(&facts.screens);
                actions.push(PaneAction::SetMode(id, Mode::Custom(views)));
            }
        });
        if custom {
            ui.add_space(14.0);
            sheet::label(ui, "Shows");
            kit::list(ui, |list| {
                for (view, label) in choosable(&facts.screens, false) {
                    let mut on = row.mode.views(&facts.screens).contains(&view);
                    if list.switch(kit::SwitchRow::new(label, &mut on)) {
                        let mode = row.mode.toggled(&facts.screens, view);
                        actions.push(PaneAction::SetMode(id, mode));
                    }
                }
            });
        }
        ui.add_space(14.0);
        kit::list(ui, |list| {
            let remove = kit::Row::new("Remove").icon(icons::X).danger().no_chevron();
            if list.row(remove).clicked() {
                actions.push(PaneAction::Remove(id));
                done = true;
            }
        });
    });
    pane.actions.extend(actions);
    if shown.closed || done {
        pane.device_sheet = None;
    }
}

fn kind_icon(kind: SurfaceKind) -> &'static str {
    match kind {
        SurfaceKind::Window => icons::APP_WINDOW,
        SurfaceKind::Display => icons::MONITOR,
        SurfaceKind::Device => icons::DEVICE_MOBILE,
    }
}

fn open_row(
    ui: &mut Ui,
    pane: &mut PaneState,
    icon: &str,
    title: &str,
    detail: &str,
    action: PaneAction,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(18.0));
        ui.vertical(|ui| {
            ui.label(title);
            ui.label(RichText::new(detail).weak().small());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(format!("{} Open", icons::PLUS)).clicked() {
                pane.actions.push(action);
            }
        });
    });
}

fn code_card(ui: &mut Ui, pane: &mut PaneState, code: Code, left: f64, link: Option<&str>) {
    ui.horizontal(|ui| {
        if let Some(link) = link {
            ui.add(Qr::new(link, QR_SIDE));
        }
        ui.vertical(|ui| {
            ui.label(RichText::new(code.to_string()).size(36.0).strong());
            ui.label("Scan it, or type it on the other device.");
            let secs = left.max(0.0) as u64;
            ui.label(
                RichText::new(format!(
                    "{} {}:{:02} left",
                    icons::CLOCK,
                    secs / 60,
                    secs % 60
                ))
                .weak(),
            );
            let stop = if touch_square(ui) > 0.0 {
                kit::wide_button(ui, "Stop", false)
            } else {
                ui.button("Stop")
            };
            if stop.clicked() {
                pane.actions.push(PaneAction::StopAdding);
            }
        });
    });
}

fn device_row(ui: &mut Ui, pane: &mut PaneState, facts: &Facts, row: &SurfaceRow) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(kind_icon(row.kind)).size(18.0));
        ui.vertical(|ui| {
            ui.label(&row.name);
            let line = row_line(row);
            ui.label(RichText::new(line).small().weak());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(icons::X)
                .on_hover_text(format!("Remove {}", row.name))
                .clicked()
            {
                pane.actions.push(PaneAction::Remove(row.id));
            }
            mode_picker(ui, pane, facts, row);
        });
    });
    if pane.views_open == Some(row.id) && matches!(row.mode, Mode::Custom(_)) {
        ui.indent(("surface_views", row.id), |ui| {
            let panes = row.kind == SurfaceKind::Window;
            for (view, label) in choosable(&facts.screens, panes) {
                let mut on = row.mode.views(&facts.screens).contains(&view);
                if ui.checkbox(&mut on, label).changed() {
                    let mode = row.mode.toggled(&facts.screens, view);
                    pane.actions.push(PaneAction::SetMode(row.id, mode));
                }
            }
        });
    }
    ui.separator();
}

fn mode_picker(ui: &mut Ui, pane: &mut PaneState, facts: &Facts, row: &SurfaceRow) {
    egui::ComboBox::from_id_salt(("surface_mode", row.id))
        .width(MODE_WIDTH)
        .selected_text(row.mode.label())
        .show_ui(ui, |ui| {
            for preset in Preset::ALL
                .into_iter()
                .filter(|p| p.offered(&facts.screens))
            {
                let on = row.mode == Mode::Preset(preset);
                let text = format!("{} {}", preset_icon(preset), preset.label());
                if ui.selectable_label(on, text).clicked() {
                    pane.actions
                        .push(PaneAction::SetMode(row.id, Mode::Preset(preset)));
                    pane.views_open = None;
                }
            }
            ui.separator();
            let custom = matches!(row.mode, Mode::Custom(_));
            let text = format!("{} Custom", icons::SLIDERS_HORIZONTAL);
            if ui.selectable_label(custom, text).clicked() {
                if !custom {
                    let views = row.mode.views(&facts.screens);
                    pane.actions
                        .push(PaneAction::SetMode(row.id, Mode::Custom(views)));
                }
                pane.views_open = Some(row.id);
            }
        });
}

fn suggestion_icon(suggestion: &Suggestion) -> &'static str {
    if suggestion.own {
        icons::USER_CIRCLE
    } else {
        icons::WIFI_HIGH
    }
}

fn suggestion_detail(suggestion: &Suggestion) -> &'static str {
    if suggestion.own {
        "Your device"
    } else {
        "On this network"
    }
}

fn suggestions(ui: &mut Ui, pane: &mut PaneState, facts: &Facts) {
    if facts.suggestions.is_empty() || facts.join.is_some() {
        return;
    }
    for suggestion in &facts.suggestions {
        ui.horizontal(|ui| {
            ui.label(RichText::new(suggestion_icon(suggestion)).size(18.0));
            ui.vertical(|ui| {
                ui.label(&suggestion.name);
                ui.label(RichText::new(suggestion_detail(suggestion)).weak().small());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Join").clicked() {
                    pane.actions
                        .push(PaneAction::JoinSuggestion(suggestion.clone()));
                }
            });
        });
    }
    ui.add_space(6.0);
}

fn phone_suggestions(ui: &mut Ui, pane: &mut PaneState, facts: &Facts) {
    if facts.suggestions.is_empty() || facts.join.is_some() {
        return;
    }
    let mut picked = None;
    kit::list(ui, |list| {
        for suggestion in &facts.suggestions {
            let row = kit::Row::new(&suggestion.name)
                .icon(suggestion_icon(suggestion))
                .detail(suggestion_detail(suggestion));
            if list.row(row).clicked() {
                picked = Some(suggestion.clone());
            }
        }
    });
    ui.add_space(10.0);
    if let Some(suggestion) = picked {
        pane.actions.push(PaneAction::JoinSuggestion(suggestion));
    }
}

fn asking_line(facts: &Facts) -> String {
    match &facts.join_host {
        Some(host) => format!("Waiting for {host} to allow this device"),
        None => "Waiting for the other device to allow this one".into(),
    }
}

fn join_part(ui: &mut Ui, pane: &mut PaneState, facts: &Facts) {
    let touch = touch_square(ui) > 0.0;
    pane.wants_suggestions = true;
    section(
        ui,
        icons::DEVICE_MOBILE,
        "Use this device as a second screen",
        |ui| {
            if touch {
                phone_suggestions(ui, pane, facts);
                phone_code_form(ui, pane, facts);
            } else {
                suggestions(ui, pane, facts);
                code_form(ui, pane, facts);
            }
            match facts.join {
                Some(JoinState::Looking) if !touch => {
                    ui.label(RichText::new("Connecting…").weak());
                }
                Some(JoinState::Asking) => {
                    if touch {
                        ui.add_space(8.0);
                    }
                    ui.label(RichText::new(asking_line(facts)).weak());
                }
                Some(JoinState::Failed(why)) => {
                    if touch {
                        ui.add_space(8.0);
                    }
                    problem_line(ui, why);
                }
                Some(JoinState::Looking | JoinState::Joined | JoinState::Reconnecting) | None => {}
            }
        },
    );
}

fn code_form(ui: &mut Ui, pane: &mut PaneState, facts: &Facts) {
    let busy = matches!(facts.join, Some(JoinState::Looking | JoinState::Asking));
    ui.horizontal(|ui| {
        let entered = code_field(ui, &mut pane.code_text, "Code", CODE_WIDTH, false);
        let code = Code::parse(&pane.code_text);
        let join = ui
            .add_enabled(code.is_some() && !busy, egui::Button::new("Join"))
            .clicked();
        if let Some(code) = code.filter(|_| (join || entered) && !busy) {
            pane.actions.push(PaneAction::Join(code));
        }
        if busy && ui.button("Cancel").clicked() {
            pane.actions.push(PaneAction::CancelJoin);
        }
    });
}

fn phone_code_form(ui: &mut Ui, pane: &mut PaneState, facts: &Facts) {
    let busy = matches!(facts.join, Some(JoinState::Looking | JoinState::Asking));
    let response = kit::field(
        ui,
        row_field(ui, &mut pane.code_text)
            .hint_text("Code")
            .char_limit(7),
    );
    let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let code = Code::parse(&pane.code_text);
    ui.add_space(10.0);
    let label = if busy { "Connecting" } else { "Join" };
    let join = ui
        .add_enabled_ui(code.is_some() && !busy, |ui| {
            kit::wide_button(ui, label, true)
        })
        .inner
        .clicked();
    if let Some(code) = code.filter(|_| (join || entered) && !busy) {
        pane.actions.push(PaneAction::Join(code));
    }
    if busy {
        ui.add_space(8.0);
        if kit::wide_button(ui, "Cancel", false).clicked() {
            pane.actions.push(PaneAction::CancelJoin);
        }
    }
}
