use crate::achievements::{Achievements, SIGN_UP_URL, Status};
use crate::app::tabs::TabViewer;
use crate::app::widgets::action::{main_button, more_button};
use crate::app::widgets::avatar;
use crate::app::widgets::settings::page::{Heading, Page};
use crate::icons;
use citrine_gb_achievements::{Achievement, GameState, GroupKind, User};
use egui::{Align, Key, Label, Layout, RichText, Sense, TextEdit, Ui, vec2};

pub const FEW: usize = 5;
pub const ABOUT: &str = "RetroAchievements.org adds achievements to classic games. Sign in with your RetroAchievements account to unlock them as you play.";
const BADGE: f32 = 36.0;
const SIGN_IN_WIDTH: f32 = 280.0;
pub const SIGN_IN_ID: &str = "achievements_sign_in";
const MARK: f32 = 30.0;

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let touch = viewer.ui.settings.layout.is_touch(ui.ctx());
    let shown = Shown {
        has_game: viewer.emulator.has_game(),
        now: crate::storage::now_unix(),
    };
    let pane = &mut *viewer.achievements;
    egui::ScrollArea::vertical()
        .id_salt("achievements_scroll")
        .auto_shrink(false)
        .show(ui, |ui| {
            if touch {
                crate::app::mobile::achievements::body(ui, pane, &shown);
            } else {
                body(ui, pane, &shown);
            }
        });
}

pub struct Shown {
    pub has_game: bool,
    pub now: u64,
}

pub struct Lists<'a> {
    pub almost: Vec<&'a Achievement>,
    pub locked: Vec<&'a Achievement>,
    pub unlocked: Vec<&'a Achievement>,
}

pub fn lists(game: &GameState) -> Lists<'_> {
    let mut lists = Lists {
        almost: Vec::new(),
        locked: Vec::new(),
        unlocked: Vec::new(),
    };
    for group in &game.groups {
        let list = match group.kind {
            GroupKind::AlmostThere => &mut lists.almost,
            GroupKind::ActiveChallenge | GroupKind::Locked => &mut lists.locked,
            GroupKind::RecentlyUnlocked | GroupKind::Unlocked => &mut lists.unlocked,
            _ => continue,
        };
        list.extend(&group.achievements);
    }
    lists
        .unlocked
        .sort_by_key(|achievement| std::cmp::Reverse(achievement.unlock_time));
    lists
}

pub enum Empty {
    NoGame,
    Loading,
    NoSet,
}

pub fn empty(pane: &Achievements, shown: &Shown) -> Option<Empty> {
    if !shown.has_game {
        Some(Empty::NoGame)
    } else if pane.no_set {
        Some(Empty::NoSet)
    } else if pane.game.is_none() {
        Some(Empty::Loading)
    } else {
        None
    }
}

impl Empty {
    pub fn text(&self) -> &'static str {
        match self {
            Empty::NoGame => "Open a game to see its achievements",
            Empty::Loading => "Loading achievements",
            Empty::NoSet => "This game has no achievements yet",
        }
    }
}

pub fn points(points: u32) -> String {
    match points {
        1 => "1 point".to_owned(),
        n => format!("{n} points"),
    }
}

pub fn count(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

pub fn total_points(user: &User) -> String {
    format!(
        "{} points in total",
        count(user.points + user.points_hardcore)
    )
}

pub fn when(unlocked: i64, now: u64) -> String {
    let ago = (now as i64 - unlocked).max(0);
    match ago {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", ago / 60),
        3600..86400 => format!("{} h ago", ago / 3600),
        86400..172800 => "yesterday".to_owned(),
        _ => format!("{} days ago", ago / 86400),
    }
}

pub fn aside(achievement: &Achievement, now: u64) -> String {
    let points = points(achievement.points);
    if achievement.unlocked {
        format!("{} · {points}", when(achievement.unlock_time, now))
    } else if let Some((text, _)) = &achievement.progress {
        format!("{text} · {points}")
    } else {
        points
    }
}

pub fn seed(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub fn badge(ui: &mut Ui, pane: &mut Achievements, url: Option<&str>, side: f32) {
    let size = vec2(side, side);
    let texture = url.and_then(|url| pane.badges.get(ui.ctx(), url));
    match texture {
        Some(texture) => {
            ui.add(egui::Image::new((texture.id(), size)).corner_radius(4));
        }
        None => {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter()
                .rect_filled(rect, 4, ui.visuals().widgets.inactive.bg_fill);
        }
    }
}

pub fn achievement_row(
    ui: &mut Ui,
    pane: &mut Achievements,
    achievement: &Achievement,
    side: f32,
    now: u64,
) {
    ui.push_id(("achievement", achievement.id), |ui| {
        ui.horizontal(|ui| {
            badge(ui, pane, achievement.shown_badge_url(), side);
            let aside = aside(achievement, now);
            let aside_width = ui.fonts_mut(|f| {
                f.layout_no_wrap(
                    aside.clone(),
                    egui::TextStyle::Body.resolve(ui.style()),
                    egui::Color32::WHITE,
                )
                .size()
                .x
            });
            let spacing = ui.spacing().item_spacing.x;
            let room = (ui.available_width() - aside_width - spacing).max(0.0);
            ui.allocate_ui_with_layout(vec2(room, side), Layout::top_down(Align::Min), |ui| {
                ui.set_width(room);
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.add(Label::new(RichText::new(&achievement.title).strong()).truncate());
                ui.add(Label::new(RichText::new(&achievement.description).weak()).truncate());
                if let Some((_, fraction)) = &achievement.progress {
                    crate::app::widgets::progress::bar(ui, Some(*fraction));
                }
            });
            ui.label(RichText::new(aside).weak());
        });
    });
}

fn body(ui: &mut Ui, pane: &mut Achievements, shown: &Shown) {
    match pane.user().cloned() {
        Some(user) => {
            account(ui, pane, &user);
            ui.add_space(12.0);
            game(ui, pane, shown);
        }
        None => sign_in(ui, pane),
    }
}

fn sign_in(ui: &mut Ui, pane: &mut Achievements) {
    let width = ui.available_width().min(SIGN_IN_WIDTH);
    let id = egui::Id::new(SIGN_IN_ID);
    let last = ui.data(|d| d.get_temp::<egui::Rect>(id));
    let last_height = last.map_or(0.0, |r| r.height());
    ui.add_space(((ui.available_height() - last_height) / 2.0).max(0.0));
    let card = ui
        .vertical_centered(|ui| {
            ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(width);
                sign_in_card(ui, pane)
            })
            .inner
        })
        .inner;
    if last.is_none_or(|r| (r.height() - card.height()).abs() > 0.5) {
        ui.ctx()
            .request_discard("achievements sign in height changed");
    }
    ui.data_mut(|d| d.insert_temp(id, card));
}

fn sign_in_card(ui: &mut Ui, pane: &mut Achievements) -> egui::Rect {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            sign_in_form(ui, pane);
        })
        .response
        .rect
}

fn sign_in_form(ui: &mut Ui, pane: &mut Achievements) {
    let busy = pane.status == Status::SigningIn;
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(MARK, MARK), Sense::hover());
        let fill = ui.visuals().selection.bg_fill;
        ui.painter().rect_filled(rect, 7, fill);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::TROPHY,
            egui::FontId::proportional(18.0),
            ui.visuals().strong_text_color(),
        );
    });
    ui.add_space(8.0);
    sign_in_heading(ui);
    ui.add_space(12.0);
    let name = ui.add_enabled(
        !busy,
        TextEdit::singleline(&mut pane.form.username)
            .hint_text("Username")
            .desired_width(f32::INFINITY),
    );
    let password = ui.add_enabled(
        !busy,
        TextEdit::singleline(&mut pane.form.password)
            .password(true)
            .hint_text("Password")
            .desired_width(f32::INFINITY),
    );
    if let Some(problem) = &pane.problem {
        ui.add(Label::new(RichText::new(problem).color(ui.visuals().error_fg_color)).wrap());
    }
    ui.add_space(4.0);
    let ready = !busy && !pane.form.username.trim().is_empty() && !pane.form.password.is_empty();
    let enter = (name.lost_focus() || password.lost_focus()) && ui_enter(ui.ctx());
    let label = if busy {
        "Signing in".to_owned()
    } else {
        format!("{} Sign in", icons::SIGN_IN)
    };
    let height = ui.spacing().interact_size.y + 6.0;
    let button = main_button(ui, label).min_size(vec2(ui.available_width(), height));
    let clicked = ui
        .vertical_centered(|ui| ui.add_enabled(ready, button).clicked())
        .inner;
    if clicked || (ready && enter) {
        pane.sign_in();
    }
    ui.add_space(8.0);
    ui.vertical_centered(|ui| {
        ui.hyperlink_to(
            format!("Create an account {}", icons::ARROW_SQUARE_OUT),
            SIGN_UP_URL,
        );
    });
}

fn sign_in_heading(ui: &mut Ui) {
    let text = RichText::new("Sign in to RetroAchievements")
        .strong()
        .size(15.0);
    let width = ui
        .painter()
        .layout_no_wrap(
            "Sign in to RetroAchievements".to_owned(),
            egui::FontId::proportional(15.0),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x;
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        ui.label(text);
        crate::app::widgets::info_mark::info_mark(ui, ABOUT);
    });
}

fn ui_enter(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(Key::Enter))
}

fn account(ui: &mut Ui, pane: &mut Achievements, user: &User) {
    ui.horizontal(|ui| {
        let name = if user.display_name.is_empty() {
            &user.username
        } else {
            &user.display_name
        };
        avatar::avatar(ui, name, seed(&user.username), 36.0, None);
        let side = ui.spacing().interact_size.y;
        let more_width = side + ui.spacing().item_spacing.x;
        let room = (ui.available_width() - more_width).max(0.0);
        ui.allocate_ui_with_layout(vec2(room, 36.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(room);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add(Label::new(RichText::new(name).strong()).truncate());
            ui.add(Label::new(RichText::new(total_points(user)).weak().small()).truncate());
        });
        egui::containers::menu::MenuButton::from_button(more_button(side)).ui(ui, |ui| {
            if ui
                .button(format!("{} Sign out of RetroAchievements", icons::SIGN_OUT))
                .clicked()
            {
                pane.sign_out();
                ui.close();
            }
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
    Page::show(ui, |page| {
        if !lists.almost.is_empty() {
            page.section(Heading::new(icons::CHART_BAR, "Almost there"), |ui| {
                for achievement in &lists.almost {
                    achievement_row(ui, pane, achievement, BADGE, shown.now);
                }
            });
        }
        if !lists.locked.is_empty() {
            let title = format!("Locked · {}", lists.locked.len());
            page.section(Heading::new(icons::LOCK, &title), |ui| {
                let few = if show_all { usize::MAX } else { FEW };
                for achievement in lists.locked.iter().take(few) {
                    achievement_row(ui, pane, achievement, BADGE, shown.now);
                }
                if lists.locked.len() > FEW {
                    let label = if show_all { "Show fewer" } else { "Show all" };
                    if ui.link(label).clicked() {
                        pane.show_all = !show_all;
                    }
                }
            });
        }
        if !lists.unlocked.is_empty() {
            let title = format!("Unlocked · {}", lists.unlocked.len());
            page.section(Heading::new(icons::TROPHY, &title), |ui| {
                for achievement in &lists.unlocked {
                    achievement_row(ui, pane, achievement, BADGE, shown.now);
                }
            });
        }
    });
    pane.game = Some(game);
}

pub fn unlocked_line(game: &GameState) -> String {
    format!(
        "{} of {} unlocked",
        game.summary.unlocked, game.summary.total
    )
}

pub fn points_line(game: &GameState) -> String {
    format!(
        "{} of {} points",
        count(game.summary.points_unlocked),
        count(game.summary.points_total)
    )
}

pub fn fraction(game: &GameState) -> f32 {
    if game.summary.total == 0 {
        0.0
    } else {
        game.summary.unlocked as f32 / game.summary.total as f32
    }
}

fn summary(ui: &mut Ui, game: &GameState) {
    ui.add(Label::new(RichText::new(&game.title).weak()).truncate());
    ui.label(RichText::new(unlocked_line(game)).strong().size(16.0));
    crate::app::widgets::progress::bar(ui, Some(fraction(game)));
    ui.label(RichText::new(points_line(game)).weak().small());
    ui.add_space(8.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_counts_get_commas() {
        assert_eq!(count(5), "5");
        assert_eq!(count(1240), "1,240");
        assert_eq!(count(1_234_567), "1,234,567");
    }

    #[test]
    fn one_point_is_singular() {
        assert_eq!(points(1), "1 point");
        assert_eq!(points(25), "25 points");
    }

    #[test]
    fn unlock_times_read_like_a_person_says_them() {
        let now = 1_000_000;
        assert_eq!(when(now as i64 - 5, now), "just now");
        assert_eq!(when(now as i64 - 600, now), "10 min ago");
        assert_eq!(when(now as i64 - 7200, now), "2 h ago");
        assert_eq!(when(now as i64 - 90_000, now), "yesterday");
        assert_eq!(when(now as i64 - 3 * 86_400, now), "3 days ago");
    }
}
