use crate::achievements::boards::{Best, Entry, PRESENCE_HINT, RankingsState, View};
use crate::achievements::{Achievements, SIGN_UP_URL, Status};
use crate::app::mobile::kit;
use crate::app::tabs::achievements::boards;
use crate::app::tabs::achievements::{
    ABOUT, FEW, HARDCORE_HINT, HardcoreSwitch, Shown, achievement_row, count, empty, empty_state,
    fraction, lists, points_line, seed, total_points, unlocked_line,
};
use crate::app::widgets::avatar;
use crate::icons;
use citrine_gb_achievements::{Achievement, GameState, User};
use egui::{Align, Key, Label, Layout, RichText, TextEdit, Ui, vec2};

const BADGE: f32 = 40.0;
const AVATAR: f32 = 40.0;
const MARK: f32 = 30.0;
const ENTRY_AVATAR: f32 = 32.0;

pub fn body(ui: &mut Ui, pane: &mut Achievements, shown: &Shown) -> Option<bool> {
    let mut asked = None;
    match pane.user().cloned() {
        Some(user) => {
            account(ui, pane, &user);
            if let Some(switch) = &shown.hardcore {
                ui.add_space(14.0);
                asked = hardcore_row(ui, switch);
            }
            ui.add_space(14.0);
            game(ui, pane, shown);
        }
        None => sign_in(ui, pane),
    }
    ui.add_space(kit::SIDE);
    asked
}

fn hardcore_row(ui: &mut Ui, switch: &HardcoreSwitch) -> Option<bool> {
    let mut on = switch.on;
    let changed = kit::list(ui, |rows| {
        rows.switch(
            kit::SwitchRow::new("Hardcore", &mut on)
                .detail(switch.off_why)
                .info(HARDCORE_HINT)
                .enabled(switch.off_why.is_none()),
        )
    });
    changed.then_some(on)
}

fn sign_in(ui: &mut Ui, pane: &mut Achievements) {
    kit::card(ui, |ui| kit::padded(ui, |ui| sign_in_form(ui, pane)));
}

fn sign_in_form(ui: &mut Ui, pane: &mut Achievements) {
    let busy = pane.status == Status::SigningIn;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(MARK, MARK), egui::Sense::hover());
        let fill = ui.visuals().selection.bg_fill;
        ui.painter().rect_filled(rect, 7, fill);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::TROPHY,
            egui::FontId::proportional(18.0),
            ui.visuals().strong_text_color(),
        );
        ui.label(
            RichText::new("Sign in to RetroAchievements")
                .strong()
                .size(19.0),
        );
        crate::app::widgets::info_mark::info_mark(ui, ABOUT);
    });
    ui.add_space(16.0);
    ui.spacing_mut().item_spacing.y = 10.0;
    let name = ui
        .add_enabled_ui(!busy, |ui| {
            kit::field(
                ui,
                TextEdit::singleline(&mut pane.form.username).hint_text("Username"),
            )
        })
        .inner;
    let password = ui
        .add_enabled_ui(!busy, |ui| {
            kit::field(
                ui,
                TextEdit::singleline(&mut pane.form.password)
                    .password(true)
                    .hint_text("Password"),
            )
        })
        .inner;
    if let Some(problem) = &pane.problem {
        ui.label(RichText::new(problem).color(ui.visuals().error_fg_color));
    }
    let ready = !busy && !pane.form.username.trim().is_empty() && !pane.form.password.is_empty();
    let enter =
        (name.lost_focus() || password.lost_focus()) && ui.input(|i| i.key_pressed(Key::Enter));
    let label = if busy {
        "Signing in".to_owned()
    } else {
        format!("{} Sign in", icons::SIGN_IN)
    };
    let clicked = ui
        .add_enabled_ui(ready, |ui| kit::primary_button(ui, &label))
        .inner;
    if ready && (clicked || enter) {
        pane.sign_in();
    }
    ui.add_space(6.0);
    ui.vertical_centered(|ui| {
        ui.hyperlink_to(
            RichText::new(format!("Create an account {}", icons::ARROW_SQUARE_OUT)).size(15.0),
            SIGN_UP_URL,
        );
    });
    ui.add_space(6.0);
}

fn account(ui: &mut Ui, pane: &mut Achievements, user: &User) {
    kit::card(ui, |ui| {
        kit::padded(ui, |ui| {
            ui.horizontal(|ui| {
                let name = if user.display_name.is_empty() {
                    &user.username
                } else {
                    &user.display_name
                };
                avatar::avatar(ui, name, seed(&user.username), AVATAR, None);
                let side = crate::app::mobile::touch_square(ui).max(ui.spacing().interact_size.y);
                let room = (ui.available_width() - side - ui.spacing().item_spacing.x).max(0.0);
                ui.allocate_ui_with_layout(
                    vec2(room, AVATAR),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(room);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add(Label::new(RichText::new(name).size(16.0)).truncate());
                        ui.add(
                            Label::new(RichText::new(total_points(user)).weak().size(14.0))
                                .truncate(),
                        );
                    },
                );
                let more = egui::Button::new(RichText::new(icons::DOTS_THREE_VERTICAL).size(20.0))
                    .frame(false)
                    .min_size(vec2(side, side));
                egui::containers::menu::MenuButton::from_button(more).ui(ui, |ui| {
                    if ui
                        .button(format!("{} Sign out of RetroAchievements", icons::SIGN_OUT))
                        .clicked()
                    {
                        pane.sign_out();
                        ui.close();
                    }
                });
            });
        });
    });
}

fn game(ui: &mut Ui, pane: &mut Achievements, shown: &Shown) {
    if let Some(empty) = empty(pane, shown) {
        empty_state(ui, pane, empty);
        return;
    }
    let Some(game) = pane.game.take() else {
        return;
    };
    let me = pane
        .user()
        .map(|user| crate::achievements::boards::me(user).to_owned())
        .unwrap_or_default();
    if let Some(id) = pane.boards.open {
        rankings(ui, pane, id, &me);
        pane.game = Some(game);
        return;
    }
    if let Some(line) = pane.boards.presence.clone() {
        presence(ui, &game.title, &line);
        ui.add_space(14.0);
    }
    let tabs = pane.boards.tabs() && shown.hardcore.is_some();
    if tabs {
        if let Some(view) = kit::segmented(
            ui,
            &[
                (View::Achievements, "Achievements"),
                (View::Leaderboards, "Leaderboards"),
            ],
            pane.boards.view,
        ) {
            pane.boards.view = view;
        }
        ui.add_space(14.0);
        if pane.boards.view == View::Leaderboards {
            board_list(ui, pane);
            pane.game = Some(game);
            return;
        }
    }
    summary(ui, &game);
    let lists = lists(&game);
    let show_all = pane.show_all;
    if !lists.almost.is_empty() {
        group(ui, pane, "Almost there", &lists.almost, None, shown.now);
    }
    if !lists.locked.is_empty() {
        let title = format!("Locked · {}", lists.locked.len());
        let few = if show_all { usize::MAX } else { FEW };
        let more = (lists.locked.len() > FEW).then_some(show_all);
        let shown_rows: Vec<_> = lists.locked.iter().take(few).copied().collect();
        if group(ui, pane, &title, &shown_rows, more, shown.now) {
            pane.show_all = !show_all;
        }
    }
    if !lists.unlocked.is_empty() {
        let title = format!("Unlocked · {}", lists.unlocked.len());
        group(ui, pane, &title, &lists.unlocked, None, shown.now);
    }
    pane.game = Some(game);
}

fn presence(ui: &mut Ui, title: &str, line: &str) {
    kit::card(ui, |ui| {
        kit::padded(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add(Label::new(RichText::new(title).size(16.0)).truncate());
            let row = ui.fonts_mut(|f| f.row_height(&egui::FontId::proportional(14.0)));
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), row), layout, |ui| {
                ui.add(Label::new(RichText::new(line).weak().size(14.0)).truncate());
                crate::app::widgets::info_mark::info_mark_in_line(ui, PRESENCE_HINT);
            });
        });
    });
}

fn note(ui: &mut Ui, pane: &Achievements) {
    if let Some(note) = pane.boards.note() {
        ui.label(RichText::new(note).weak().size(14.0));
        ui.add_space(8.0);
    }
}

fn board_list(ui: &mut Ui, pane: &mut Achievements) {
    note(ui, pane);
    let rows: Vec<(u32, String, String, String, bool)> = pane
        .boards
        .list
        .iter()
        .map(|board| {
            (
                board.id,
                board.title.clone(),
                board.description.clone(),
                boards::aside(board),
                board.best == Best::Unknown,
            )
        })
        .collect();
    let (opened, seen) = kit::list(ui, |list| {
        let mut opened = None;
        let mut seen = Vec::new();
        for (id, title, description, aside, unknown) in &rows {
            let row = kit::Row::new(title)
                .detail(description)
                .value(aside.clone());
            let response = list.row(row);
            if response.clicked() {
                opened = Some(*id);
            }
            if *unknown && list.ui().is_rect_visible(response.rect) {
                seen.push(*id);
            }
        }
        (opened, seen)
    });
    for id in seen {
        pane.boards.want_best(id);
    }
    if let Some(id) = opened {
        pane.boards.open = Some(id);
    }
}

fn rankings(ui: &mut Ui, pane: &mut Achievements, id: u32, me: &str) {
    let Some(board) = pane.boards.board(id).cloned() else {
        return;
    };
    kit::card(ui, |ui| {
        kit::padded(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(&board.description).size(15.0));
            if let Some(rankings) = &pane.boards.rankings {
                ui.label(
                    RichText::new(format!("{} entries", count(rankings.entries)))
                        .weak()
                        .size(14.0),
                );
            }
        });
    });
    ui.add_space(8.0);
    note(ui, pane);
    match pane.boards.rankings_state() {
        Some(RankingsState::Shown) => {}
        Some(RankingsState::Failed) => {
            ui.label(RichText::new(boards::UNREACHABLE).weak().size(14.0));
            if kit::wide_button(ui, "Try again", false).clicked() {
                pane.boards.retry_rankings();
            }
            return;
        }
        _ => {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| ui.spinner());
            return;
        }
    }
    let Some(rankings) = pane.boards.rankings.clone() else {
        return;
    };
    entries(
        ui,
        &format!("Top {}", rankings.top.len()),
        &rankings.top,
        me,
    );
    if !rankings.around.is_empty() {
        entries(ui, "Around you", &rankings.around, me);
    }
}

fn entries(ui: &mut Ui, title: &str, rows: &[Entry], me: &str) {
    kit::section_label(ui, title);
    kit::card(ui, |ui| {
        for (index, entry) in rows.iter().enumerate() {
            if index > 0 {
                kit::divider(ui);
            }
            kit::padded(ui, |ui| {
                boards::entry_row_sized(ui, entry, entry.user == me, ENTRY_AVATAR)
            });
        }
    });
}

fn summary(ui: &mut Ui, game: &GameState) {
    kit::card(ui, |ui| {
        kit::padded(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(RichText::new(unlocked_line(game)).strong().size(17.0));
            crate::app::widgets::progress::bar(ui, Some(fraction(game)));
            ui.label(RichText::new(points_line(game)).weak().size(14.0));
        });
    });
}

fn group(
    ui: &mut Ui,
    pane: &mut Achievements,
    title: &str,
    rows: &[&Achievement],
    more: Option<bool>,
    now: u64,
) -> bool {
    kit::section_label(ui, title);
    let mut toggled = false;
    kit::card(ui, |ui| {
        for (index, achievement) in rows.iter().enumerate() {
            if index > 0 {
                kit::divider(ui);
            }
            kit::padded(ui, |ui| achievement_row(ui, pane, achievement, BADGE, now));
        }
        if let Some(all) = more {
            kit::divider(ui);
            kit::padded(ui, |ui| {
                let label = if all { "Show fewer" } else { "Show all" };
                toggled = ui.link(RichText::new(label).size(15.0)).clicked();
            });
        }
    });
    toggled
}
