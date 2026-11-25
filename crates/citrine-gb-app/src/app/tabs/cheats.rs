use crate::app::tabs::TabViewer;
use crate::app::widgets::action::{icon_button, main_button, quiet_button};
use crate::app::widgets::check::Check;
use crate::app::widgets::settings::form::Form;
use crate::cheats::{self, CheatBook, SavedCheat};
use crate::icons;
use citrine_gb::console::ConsoleKind;
use egui::{Align, Key, Label, Layout, Modifiers, RichText, TextEdit, TextStyle, Ui, vec2};

const UNDO_SECS: f64 = 10.0;

#[derive(Debug, Default)]
pub struct CheatsPane {
    pub(crate) editing: Option<Editing>,
    pub(crate) deleted: Option<Deleted>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Editing {
    rom_key: String,
    pub(crate) index: Option<usize>,
    pub(crate) name: String,
    pub(crate) code: String,
    pub(crate) focus: bool,
    pub(crate) checked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Deleted {
    rom_key: String,
    pub(crate) index: usize,
    pub(crate) cheat: SavedCheat,
    until: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Action {
    Toggle(usize, bool),
    Add,
    Edit(usize),
    Delete(usize),
    Undo,
    Cancel,
    Submit,
}

pub(crate) struct Game<'a> {
    rom_key: &'a str,
    pub(crate) console: ConsoleKind,
    now: f64,
}

impl CheatsPane {
    fn forget(&mut self, game: &Game<'_>) {
        if self
            .editing
            .as_ref()
            .is_some_and(|editing| editing.rom_key != game.rom_key)
        {
            self.editing = None;
        }
        if self
            .deleted
            .as_ref()
            .is_some_and(|deleted| deleted.rom_key != game.rom_key || deleted.until <= game.now)
        {
            self.deleted = None;
        }
    }

    pub(crate) fn editing_row(&self) -> Option<usize> {
        self.editing.as_ref().and_then(|editing| editing.index)
    }

    pub(crate) fn adding(&self) -> bool {
        self.editing
            .as_ref()
            .is_some_and(|editing| editing.index.is_none())
    }

    fn act(&mut self, action: Action, book: &mut CheatBook, game: &Game<'_>) -> bool {
        let rom_key = game.rom_key;
        match action {
            Action::Toggle(index, on) => book.set_on(rom_key, index, on),
            Action::Add => {
                self.editing = Some(Editing {
                    rom_key: rom_key.to_owned(),
                    index: None,
                    name: String::new(),
                    code: String::new(),
                    focus: true,
                    checked: false,
                });
                false
            }
            Action::Edit(index) => {
                let Some(cheat) = book.list(rom_key).get(index) else {
                    return false;
                };
                self.editing = Some(Editing {
                    rom_key: rom_key.to_owned(),
                    index: Some(index),
                    name: cheat.name.clone(),
                    code: cheat.code.clone(),
                    focus: true,
                    checked: false,
                });
                false
            }
            Action::Delete(index) => {
                let Some(cheat) = book.remove(rom_key, index) else {
                    return false;
                };
                match self.editing_row() {
                    Some(row) if row == index => self.editing = None,
                    Some(row) if row > index => {
                        if let Some(editing) = &mut self.editing {
                            editing.index = Some(row - 1);
                        }
                    }
                    _ => {}
                }
                self.deleted = Some(Deleted {
                    rom_key: rom_key.to_owned(),
                    index,
                    cheat,
                    until: game.now + UNDO_SECS,
                });
                true
            }
            Action::Undo => {
                let Some(deleted) = self.deleted.take() else {
                    return false;
                };
                if let Some(row) = self.editing_row().filter(|&row| row >= deleted.index)
                    && let Some(editing) = &mut self.editing
                {
                    editing.index = Some(row + 1);
                }
                book.insert(&deleted.rom_key, deleted.index, deleted.cheat);
                true
            }
            Action::Cancel => {
                self.editing = None;
                false
            }
            Action::Submit => {
                let Some(mut editing) = self.editing.take() else {
                    return false;
                };
                if cheats::problem(game.console, &editing.code).is_some() {
                    editing.checked = true;
                    self.editing = Some(editing);
                    return false;
                }
                let list = book.list(rom_key);
                let name = match editing.name.trim() {
                    "" => cheats::default_name(list),
                    name => name.to_owned(),
                };
                let code = editing.code.trim().to_owned();
                match editing.index {
                    None => {
                        book.add(
                            rom_key,
                            SavedCheat {
                                name,
                                code,
                                on: true,
                            },
                        );
                        true
                    }
                    Some(index) => {
                        let on = list.get(index).is_none_or(|cheat| cheat.on);
                        book.replace(rom_key, index, SavedCheat { name, code, on })
                    }
                }
            }
        }
    }
}

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let rom_key = viewer
        .emulator
        .rom_key()
        .filter(|_| viewer.emulator.has_game())
        .map(str::to_owned);
    let Some(rom_key) = rom_key else {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| ui.label(RichText::new("Open a game to add cheats").weak()));
        return;
    };
    let touch = viewer.ui.settings.layout.is_touch(ui.ctx());
    if viewer.emulator.hardcore() {
        let text = "Cheats are off in hardcore";
        let turn_off = if touch {
            crate::app::mobile::cheats::hardcore_note(ui, text)
        } else {
            crate::app::hardcore::off_note(ui, text)
        };
        if turn_off {
            crate::app::hardcore::turn_off(&mut viewer.ui.settings);
        }
        return;
    }
    let game = Game {
        rom_key: &rom_key,
        console: viewer.emulator.kind(),
        now: ui.input(|i| i.time),
    };
    let paused = viewer.play.session().is_some_and(|s| s.runs_engine());
    let draw = if touch {
        crate::app::mobile::cheats::body
    } else {
        body
    };
    let state = &mut *viewer.ui;
    let pane = &mut state.cheats;
    pane.forget(&game);
    if let Some(deleted) = &pane.deleted {
        ui.ctx()
            .request_repaint_after_secs((deleted.until - game.now).max(0.0) as f32);
    }
    let mut actions = Vec::new();
    egui::ScrollArea::vertical()
        .id_salt("cheats_scroll")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.add_space(4.0);
            if paused {
                ui.label(RichText::new("Cheats are off while you play together").weak());
                ui.add_space(8.0);
            }
            draw(
                ui,
                pane,
                state.settings.cheats.list(&rom_key),
                &game,
                &mut actions,
            );
            ui.add_space(8.0);
        });
    for action in actions {
        if pane.act(action, &mut state.settings.cheats, &game) {
            state.settings.dirty = true;
        }
    }
}

fn body(
    ui: &mut Ui,
    pane: &mut CheatsPane,
    list: &[SavedCheat],
    game: &Game<'_>,
    actions: &mut Vec<Action>,
) {
    let editing_row = pane.editing_row();
    let undo_at = pane.deleted.as_ref().map(|deleted| deleted.index);
    let adding = pane.adding();
    let open = pane.editing.is_some();
    for (index, cheat) in list.iter().enumerate() {
        if undo_at == Some(index) {
            undo_row(ui, pane, actions);
        }
        match (&mut pane.editing, editing_row == Some(index)) {
            (Some(editing), true) => editor(ui, editing, list, game.console, actions),
            _ => cheat_row(ui, index, cheat, game.console, open, actions),
        }
    }
    if undo_at.is_some_and(|at| at >= list.len()) {
        undo_row(ui, pane, actions);
    }
    match &mut pane.editing {
        Some(editing) if adding => editor(ui, editing, list, game.console, actions),
        Some(_) => {}
        None => {
            if ui.button(format!("{} Add a cheat", icons::PLUS)).clicked() {
                actions.push(Action::Add);
            }
        }
    }
}

fn line(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_height(height);
            body(ui);
        },
    );
}

fn cheat_row(
    ui: &mut Ui,
    index: usize,
    cheat: &SavedCheat,
    console: ConsoleKind,
    open: bool,
    actions: &mut Vec<Action>,
) {
    ui.push_id(("cheat", index), |ui| {
        line(ui, |ui| {
            let mut on = cheat.on;
            let hint = if on { "Turn off" } else { "Turn on" };
            if ui
                .add(Check::new(&mut on, ""))
                .on_hover_text(hint)
                .changed()
            {
                actions.push(Action::Toggle(index, on));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let side = ui.spacing().interact_size.y;
                if ui
                    .add(icon_button(icons::TRASH, side).frame(false))
                    .on_hover_text("Delete")
                    .clicked()
                {
                    actions.push(Action::Delete(index));
                }
                if ui
                    .add_enabled(!open, icon_button(icons::PENCIL_SIMPLE, side).frame(false))
                    .on_hover_text("Edit")
                    .clicked()
                {
                    actions.push(Action::Edit(index));
                }
                let tag =
                    cheats::format_of(console, &cheat.code).map_or("Not a code", |f| f.label());
                ui.label(RichText::new(tag).small().weak());
                let name = if cheat.on {
                    RichText::new(&cheat.name).strong()
                } else {
                    RichText::new(&cheat.name).weak()
                };
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(Label::new(name).truncate());
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
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(quiet_button(ui, "Undo")).clicked() {
                actions.push(Action::Undo);
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let text = format!("Deleted {}", deleted.cheat.name);
                ui.add(Label::new(RichText::new(text).weak()).truncate());
            });
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
    let others: Vec<SavedCheat> = list
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != editing.index)
        .map(|(_, cheat)| cheat.clone())
        .collect();
    ui.add_space(4.0);
    Form::show(ui, "cheat_editor", |form| {
        let name = form.row("Name", |ui| {
            ui.add(
                TextEdit::singleline(&mut editing.name)
                    .hint_text(cheats::default_name(&others))
                    .desired_width(ui.available_width()),
            )
        });
        let code = form.row("Code", |ui| {
            ui.add(
                TextEdit::multiline(&mut editing.code)
                    .hint_text(cheats::formats_text(console))
                    .font(TextStyle::Monospace)
                    .desired_rows(3)
                    .desired_width(ui.available_width()),
            )
        });
        if std::mem::take(&mut editing.focus) {
            code.request_focus();
        }
        if code.changed() {
            editing.checked = false;
        }
        if code.lost_focus() {
            editing.checked = true;
        }
        if editing.checked
            && !editing.code.trim().is_empty()
            && let Some(problem) = cheats::problem(console, &editing.code)
        {
            form.hint(RichText::new(problem).color(form.ctx().style().visuals.error_fg_color));
        }
        form.buttons(|ui| {
            let done = if editing.index.is_some() {
                "Save"
            } else {
                "Create"
            };
            let ready = !editing.code.trim().is_empty();
            let enter = name.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if ui.add_enabled(ready, main_button(ui, done)).clicked() || (ready && enter) {
                actions.push(Action::Submit);
            }
            let escape = ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
            if ui.add(quiet_button(ui, "Cancel")).clicked() || escape {
                actions.push(Action::Cancel);
            }
        });
    });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "game";

    fn game() -> Game<'static> {
        Game {
            rom_key: KEY,
            console: ConsoleKind::Gbc,
            now: 0.0,
        }
    }

    fn create(pane: &mut CheatsPane, book: &mut CheatBook, name: &str, code: &str) {
        pane.act(Action::Add, book, &game());
        let editing = pane.editing.as_mut().unwrap();
        editing.name = name.into();
        editing.code = code.into();
        assert!(pane.act(Action::Submit, book, &game()));
    }

    fn draw(pane: &mut CheatsPane, book: &CheatBook, events: Vec<egui::Event>) -> Vec<Action> {
        let ctx = egui::Context::default();
        let mut actions = Vec::new();
        for events in [Vec::new(), events] {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    body(ui, pane, book.list(KEY), &game(), &mut actions);
                });
            });
        }
        actions
    }

    #[test]
    fn escape_closes_an_open_editor() {
        let mut pane = CheatsPane::default();
        let mut book = CheatBook::default();
        create(&mut pane, &mut book, "Lives", "01FF47D3");
        pane.act(Action::Edit(0), &mut book, &game());
        assert!(draw(&mut pane, &book, Vec::new()).is_empty());
        let escape = egui::Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        assert_eq!(draw(&mut pane, &book, vec![escape]), [Action::Cancel]);
    }

    #[test]
    fn create_adds_a_switched_on_cheat_and_cancel_leaves_nothing() {
        let mut pane = CheatsPane::default();
        let mut book = CheatBook::default();
        pane.act(Action::Add, &mut book, &game());
        pane.editing.as_mut().unwrap().code = "01FF47D3".into();
        assert!(!pane.act(Action::Cancel, &mut book, &game()));
        assert!(book.list(KEY).is_empty());
        create(&mut pane, &mut book, "", "01FF47D3");
        assert_eq!(
            book.list(KEY),
            [SavedCheat {
                name: "Cheat 1".into(),
                code: "01FF47D3".into(),
                on: true,
            }]
        );
        assert!(pane.editing.is_none());
    }

    #[test]
    fn a_code_that_does_not_read_is_never_saved() {
        let mut pane = CheatsPane::default();
        let mut book = CheatBook::default();
        pane.act(Action::Add, &mut book, &game());
        pane.editing.as_mut().unwrap().code = "nope".into();
        assert!(!pane.editing.as_ref().unwrap().checked);
        assert!(!pane.act(Action::Submit, &mut book, &game()));
        assert!(book.list(KEY).is_empty());
        assert!(pane.editing.as_ref().unwrap().checked);
    }

    #[test]
    fn edit_keeps_the_switch_and_save_replaces_in_place() {
        let mut pane = CheatsPane::default();
        let mut book = CheatBook::default();
        create(&mut pane, &mut book, "Money", "01FF47D3");
        create(&mut pane, &mut book, "Items", "01FF48D3");
        pane.act(Action::Toggle(0, false), &mut book, &game());
        pane.act(Action::Edit(0), &mut book, &game());
        pane.editing.as_mut().unwrap().name = "Max money".into();
        assert!(pane.act(Action::Submit, &mut book, &game()));
        assert_eq!(book.list(KEY)[0].name, "Max money");
        assert!(!book.list(KEY)[0].on);
        assert_eq!(book.list(KEY)[1].name, "Items");
    }

    #[test]
    fn undo_brings_a_deleted_cheat_back_to_its_row_until_it_runs_out() {
        let mut pane = CheatsPane::default();
        let mut book = CheatBook::default();
        create(&mut pane, &mut book, "Money", "01FF47D3");
        create(&mut pane, &mut book, "Items", "01FF48D3");
        assert!(pane.act(Action::Delete(0), &mut book, &game()));
        assert_eq!(book.list(KEY).len(), 1);
        assert!(pane.act(Action::Undo, &mut book, &game()));
        assert_eq!(book.list(KEY)[0].name, "Money");
        pane.act(Action::Delete(1), &mut book, &game());
        pane.forget(&Game {
            now: UNDO_SECS + 1.0,
            ..game()
        });
        assert!(!pane.act(Action::Undo, &mut book, &game()));
        assert_eq!(book.list(KEY).len(), 1);
    }

    #[test]
    fn another_game_drops_an_open_editor() {
        let mut pane = CheatsPane::default();
        let mut book = CheatBook::default();
        pane.act(Action::Add, &mut book, &game());
        pane.forget(&Game {
            rom_key: "other",
            ..game()
        });
        assert!(pane.editing.is_none());
    }
}
