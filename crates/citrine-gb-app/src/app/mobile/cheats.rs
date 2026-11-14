use crate::app::mobile::kit;
use crate::app::tabs::cheats::{Action, CheatsPane, Editing, Game};
use crate::app::widgets::action::icon_button;
use crate::app::widgets::text::one_line;
use crate::cheats::{self, SavedCheat};
use crate::icons;
use citrine_gb::console::ConsoleKind;
use egui::{Align, CornerRadius, FontId, Layout, Margin, RichText, TextEdit, Ui, vec2};

const BUTTON_SIDE: f32 = 36.0;
const NAME_SIZE: f32 = 16.0;

pub(crate) fn body(
    ui: &mut Ui,
    pane: &mut CheatsPane,
    list: &[SavedCheat],
    game: &Game<'_>,
    actions: &mut Vec<Action>,
) {
    let editing_row = pane.editing_row();
    let undo_at = pane.deleted.as_ref().map(|deleted| deleted.index);
    let adding = pane.adding();
    kit::list(ui, |rows| {
        for (index, cheat) in list.iter().enumerate() {
            if undo_at == Some(index) {
                rows.custom(|ui| undo_row(ui, pane, actions));
            }
            match (&mut pane.editing, editing_row == Some(index)) {
                (Some(editing), true) => {
                    rows.custom(|ui| editor(ui, editing, list, game.console, actions));
                }
                _ => rows.custom(|ui| cheat_row(ui, index, cheat, game.console, actions)),
            }
        }
        if undo_at.is_some_and(|at| at >= list.len()) {
            rows.custom(|ui| undo_row(ui, pane, actions));
        }
        match &mut pane.editing {
            Some(editing) if adding => {
                rows.custom(|ui| editor(ui, editing, list, game.console, actions));
            }
            _ => {
                let add = kit::Row::new("Add a cheat").icon(icons::PLUS).no_chevron();
                if rows.row(add).clicked() {
                    actions.push(Action::Add);
                }
            }
        }
    });
}

fn line(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), kit::ROW_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_height(kit::ROW_HEIGHT);
            ui.add_space(6.0);
            body(ui);
        },
    );
}

fn cheat_row(
    ui: &mut Ui,
    index: usize,
    cheat: &SavedCheat,
    console: ConsoleKind,
    actions: &mut Vec<Action>,
) {
    ui.push_id(("cheat", index), |ui| {
        line(ui, |ui| {
            let mut on = cheat.on;
            let hint = if on { "Turn off" } else { "Turn on" };
            if kit::switch(ui, &mut on).on_hover_text(hint).changed() {
                actions.push(Action::Toggle(index, on));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(6.0);
                if ui
                    .add(icon_button(icons::TRASH, BUTTON_SIDE))
                    .on_hover_text("Delete")
                    .clicked()
                {
                    actions.push(Action::Delete(index));
                }
                if ui
                    .add(icon_button(icons::PENCIL_SIMPLE, BUTTON_SIDE))
                    .on_hover_text("Edit")
                    .clicked()
                {
                    actions.push(Action::Edit(index));
                }
                let tag =
                    cheats::format_of(console, &cheat.code).map_or("Not a code", |f| f.label());
                ui.label(RichText::new(tag).small().weak());
                ui.add_space(4.0);
                let color = if cheat.on {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().weak_text_color()
                };
                let width = ui.available_width();
                let galley = one_line(ui, &cheat.name, NAME_SIZE, color, width);
                let elided = galley.elided;
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let response = ui.label(galley);
                    if elided {
                        response.on_hover_text(&cheat.name);
                    }
                });
            });
        });
    });
}

fn undo_row(ui: &mut Ui, pane: &CheatsPane, actions: &mut Vec<Action>) {
    let Some(deleted) = &pane.deleted else {
        return;
    };
    line(ui, |ui| {
        ui.add_space(6.0);
        let text = format!("Deleted {}", deleted.cheat.name);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(6.0);
            if kit::pill(ui, "Undo", false).clicked() {
                actions.push(Action::Undo);
            }
            let width = ui.available_width();
            let galley = one_line(ui, &text, NAME_SIZE, ui.visuals().weak_text_color(), width);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| ui.label(galley));
        });
    });
}

fn editor(
    ui: &mut Ui,
    editing: &mut Editing,
    list: &[SavedCheat],
    console: ConsoleKind,
    actions: &mut Vec<Action>,
) {
    kit::padded(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let others: Vec<SavedCheat> = list
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != editing.index)
            .map(|(_, cheat)| cheat.clone())
            .collect();
        ui.label(RichText::new("Name").weak());
        kit::field(
            ui,
            TextEdit::singleline(&mut editing.name).hint_text(cheats::default_name(&others)),
        );
        ui.add_space(4.0);
        ui.label(RichText::new("Code").weak());
        let code = code_box(ui, &mut editing.code, &cheats::formats_text(console));
        if std::mem::take(&mut editing.focus) {
            code.request_focus();
        }
        if code.changed() {
            editing.checked = false;
        }
        if code.lost_focus() {
            editing.checked = true;
        }
        let problem = if editing.checked && !editing.code.trim().is_empty() {
            cheats::problem(console, &editing.code)
        } else {
            None
        };
        if let Some(problem) = &problem {
            ui.label(RichText::new(problem).color(ui.visuals().error_fg_color));
        }
        ui.add_space(4.0);
        let width = ui.available_width();
        ui.allocate_ui_with_layout(
            vec2(width, kit::ROW_HEIGHT),
            Layout::right_to_left(Align::Center),
            |ui| {
                let done = if editing.index.is_some() {
                    "Save"
                } else {
                    "Create"
                };
                let ready = !editing.code.trim().is_empty();
                if ui
                    .add_enabled_ui(ready, |ui| kit::pill(ui, done, true))
                    .inner
                    .clicked()
                {
                    actions.push(Action::Submit);
                }
                if kit::pill(ui, "Cancel", false).clicked() {
                    actions.push(Action::Cancel);
                }
            },
        );
    });
}

fn code_box(ui: &mut Ui, code: &mut String, hint: &str) -> egui::Response {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        for state in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
        ] {
            state.corner_radius = CornerRadius::same(10);
        }
        let width = ui.available_width();
        ui.add(
            TextEdit::multiline(code)
                .hint_text(hint)
                .font(FontId::monospace(15.0))
                .margin(Margin::symmetric(10, 8))
                .desired_rows(3)
                .desired_width(width),
        )
    })
    .inner
}
