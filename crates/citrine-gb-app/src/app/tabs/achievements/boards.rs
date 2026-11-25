use super::count;
use crate::achievements::Achievements;
use crate::achievements::boards::{Best, Board, Entry, PRESENCE_HINT, Rankings, RankingsState};
use crate::app::widgets::avatar;
use crate::app::widgets::info_mark::info_mark;
use crate::app::widgets::settings::page::{Heading, Page};
use crate::icons;
use egui::{Align, Label, Layout, RichText, Sense, Ui, vec2};

const AVATAR: f32 = 24.0;
const RANK: f32 = 44.0;
pub const PROFILE_URL: &str = "https://retroachievements.org/user/";

pub fn aside(board: &Board) -> String {
    match &board.best {
        Best::Score(entry) => format!("{}  ·  #{}", entry.score, count(entry.rank)),
        Best::NoScore => "No score yet".to_owned(),
        Best::Unknown => String::new(),
    }
}

pub fn presence(ui: &mut Ui, line: &str) {
    ui.horizontal(|ui| {
        ui.add(Label::new(RichText::new(line).weak().small()).truncate());
        info_mark(ui, PRESENCE_HINT);
    });
}

pub fn note(ui: &mut Ui, pane: &Achievements) {
    if let Some(note) = pane.boards.note() {
        ui.label(RichText::new(note).weak().small());
        ui.add_space(6.0);
    }
}

pub fn board_row(ui: &mut Ui, board: &Board) -> (bool, bool) {
    let aside = aside(board);
    let height = 36.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let visible = ui.is_rect_visible(rect);
    if response.hovered() {
        ui.painter().rect_filled(
            rect.expand2(vec2(4.0, 1.0)),
            4,
            ui.visuals().widgets.hovered.weak_bg_fill,
        );
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let aside_width = child.fonts_mut(|f| {
        f.layout_no_wrap(
            aside.clone(),
            egui::TextStyle::Body.resolve(child.style()),
            egui::Color32::WHITE,
        )
        .size()
        .x
    });
    let room = (rect.width() - aside_width - child.spacing().item_spacing.x).max(0.0);
    child.allocate_ui_with_layout(vec2(room, height), Layout::top_down(Align::Min), |ui| {
        ui.set_width(room);
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.add(
            Label::new(RichText::new(&board.title).strong())
                .truncate()
                .selectable(false),
        );
        ui.add(
            Label::new(RichText::new(&board.description).weak())
                .truncate()
                .selectable(false),
        );
    });
    child.add(Label::new(RichText::new(aside).weak()).selectable(false));
    let clicked = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked();
    (clicked, visible)
}

pub fn list(ui: &mut Ui, pane: &mut Achievements) {
    note(ui, pane);
    let mut opened = None;
    let mut seen = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for board in &pane.boards.list {
                let (clicked, visible) = board_row(ui, board);
                if clicked {
                    opened = Some(board.id);
                }
                if visible && board.best == Best::Unknown {
                    seen.push(board.id);
                }
            }
        });
    for id in seen {
        pane.boards.want_best(id);
    }
    if let Some(id) = opened {
        pane.boards.open = Some(id);
    }
}

pub fn entry_row(ui: &mut Ui, entry: &Entry, mine: bool) {
    entry_row_sized(ui, entry, mine, AVATAR);
}

pub fn entry_row_sized(ui: &mut Ui, entry: &Entry, mine: bool, side: f32) {
    let height = side + 8.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let visuals = ui.visuals();
    if mine {
        ui.painter().rect_filled(
            rect.expand2(vec2(4.0, 0.0)),
            4,
            visuals.selection.bg_fill.gamma_multiply(0.35),
        );
    } else if response.hovered() {
        ui.painter().rect_filled(
            rect.expand2(vec2(4.0, 0.0)),
            4,
            visuals.widgets.hovered.weak_bg_fill,
        );
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.allocate_ui_with_layout(
        vec2(RANK, height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(RANK);
            ui.add(
                Label::new(RichText::new(format!("#{}", count(entry.rank))).weak())
                    .selectable(false),
            );
        },
    );
    let picked = avatar::avatar(
        &mut child,
        &entry.user,
        super::seed(&entry.user),
        side,
        None,
    )
    .clicked();
    let score = RichText::new(&entry.score);
    let score_width = child.fonts_mut(|f| {
        f.layout_no_wrap(
            entry.score.clone(),
            egui::TextStyle::Body.resolve(child.style()),
            egui::Color32::WHITE,
        )
        .size()
        .x
    });
    let room = (child.available_width() - score_width - child.spacing().item_spacing.x).max(0.0);
    child.allocate_ui_with_layout(
        vec2(room, height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(room);
            let name = RichText::new(&entry.user);
            ui.add(
                Label::new(if mine { name.strong() } else { name })
                    .truncate()
                    .selectable(false),
            );
        },
    );
    child.add(Label::new(score).selectable(false));
    let clicked = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked();
    if !mine && (clicked || picked) {
        ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
            "{PROFILE_URL}{}",
            entry.user
        )));
    }
}

pub fn ranking_sections(page: &mut Page, rankings: &Rankings, me: &str) {
    let title = format!("Top {}", rankings.top.len());
    page.section(Heading::new(icons::RANKING, &title), |ui| {
        for entry in &rankings.top {
            entry_row(ui, entry, entry.user == me);
        }
    });
    if !rankings.around.is_empty() {
        page.section(Heading::new(icons::USER, "Around you"), |ui| {
            for entry in &rankings.around {
                entry_row(ui, entry, entry.user == me);
            }
        });
    }
}

pub fn rankings(ui: &mut Ui, pane: &mut Achievements, id: u32, me: &str) {
    let back = ui
        .button(format!("{}  All leaderboards", icons::ARROW_LEFT))
        .clicked();
    let escape = !egui::Popup::is_any_open(ui.ctx())
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    if back || escape {
        pane.boards.open = None;
        return;
    }
    ui.add_space(8.0);
    let Some(board) = pane.boards.board(id).cloned() else {
        return;
    };
    ui.label(RichText::new(&board.title).strong().size(16.0));
    ui.label(RichText::new(&board.description).weak());
    if let Some(rankings) = &pane.boards.rankings {
        ui.label(
            RichText::new(format!("{} entries", count(rankings.entries)))
                .weak()
                .small(),
        );
    }
    ui.add_space(8.0);
    note(ui, pane);
    match pane.boards.rankings_state() {
        Some(RankingsState::Shown) => {}
        Some(RankingsState::Failed) => {
            ui.label(RichText::new(UNREACHABLE).weak());
            if ui.button("Try again").clicked() {
                pane.boards.retry_rankings();
            }
            return;
        }
        _ => {
            ui.vertical_centered(|ui| ui.spinner());
            return;
        }
    }
    let Some(rankings) = pane.boards.rankings.clone() else {
        return;
    };
    Page::show(ui, |page| ranking_sections(page, &rankings, me));
}

pub const UNREACHABLE: &str = "Can't reach RetroAchievements";
