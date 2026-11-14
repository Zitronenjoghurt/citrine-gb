use crate::achievements::{Achievements, SIGN_UP_URL, Status};
use crate::app::mobile::kit;
use crate::app::tabs::achievements::{
    ABOUT, FEW, Shown, achievement_row, empty, fraction, lists, points_line, seed, total_points,
    unlocked_line,
};
use crate::app::widgets::avatar;
use crate::icons;
use citrine_gb_achievements::{Achievement, GameState, User};
use egui::{Align, Key, Label, Layout, RichText, TextEdit, Ui, vec2};

const BADGE: f32 = 40.0;
const AVATAR: f32 = 40.0;
const MARK: f32 = 30.0;

pub fn body(ui: &mut Ui, pane: &mut Achievements, shown: &Shown) {
    match pane.user().cloned() {
        Some(user) => {
            account(ui, pane, &user);
            ui.add_space(14.0);
            game(ui, pane, shown);
        }
        None => sign_in(ui, pane),
    }
    ui.add_space(kit::SIDE);
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
        ui.add_space(16.0);
        ui.vertical_centered(|ui| ui.label(RichText::new(empty.text()).weak()));
        return;
    }
    let Some(game) = pane.game.take() else {
        return;
    };
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
