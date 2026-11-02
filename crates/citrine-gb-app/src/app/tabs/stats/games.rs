use super::chart::ACCENT;
use super::{
    PAGE_SIZE, Page, Range, Sort, StatsUi, capitalized, count, date_text, facts, heading,
    last_played, nonzero_facts, play_chart, play_time, sessions, tiles,
};
use crate::app::widgets::field::row_field;
use crate::icons;
use crate::stats::{GameTotals, Overview, Progress, parse_day};
use egui::{CornerRadius, Rect, RichText, Sense, Ui, pos2, vec2};
use jiff::civil::Date;
use std::collections::BTreeMap;

pub(super) fn sorted<'a>(state: &StatsUi, overview: &'a Overview) -> Vec<&'a GameTotals> {
    let needle = state.search.to_lowercase();
    let mut games: Vec<&GameTotals> = overview
        .games
        .iter()
        .filter(|g| needle.is_empty() || g.name.to_lowercase().contains(&needle))
        .collect();
    match state.sort {
        Sort::MostPlayed => {}
        Sort::LastPlayed => games.sort_by_key(|g| std::cmp::Reverse(g.last_played)),
        Sort::Name => games.sort_by_key(|g| g.name.to_lowercase()),
    }
    games
}

pub(super) fn games_list(ui: &mut Ui, state: &mut StatsUi, overview: &Overview, today: Date) {
    ui.horizontal(|ui| {
        let before = (state.search.clone(), state.sort);
        egui::ComboBox::from_id_salt("stats_sort")
            .selected_text(state.sort.label())
            .show_ui(ui, |ui| {
                for sort in [Sort::MostPlayed, Sort::LastPlayed, Sort::Name] {
                    ui.selectable_value(&mut state.sort, sort, sort.label());
                }
            });
        ui.add(
            row_field(ui, &mut state.search)
                .hint_text(format!("{}  Find a game", icons::MAGNIFYING_GLASS))
                .desired_width(ui.available_width()),
        );
        if before != (state.search.clone(), state.sort) {
            state.list_page = 0;
        }
    });
    ui.add_space(4.0);
    let games = sorted(state, overview);
    if games.is_empty() {
        ui.weak("No game matches.");
        return;
    }
    let pages = games.len().div_ceil(PAGE_SIZE).max(1);
    state.list_page = state.list_page.min(pages - 1);
    let top = overview.games.first().map_or(1, |g| g.play_ms.max(1));
    for game in games
        .iter()
        .skip(state.list_page * PAGE_SIZE)
        .take(PAGE_SIZE)
    {
        if game_row(ui, game, top, today) {
            state.page = Page::Game {
                rom: game.rom.clone(),
                name: game.name.clone(),
            };
        }
    }
    if pages > 1 {
        crate::app::tabs::media::pager(ui, &mut state.list_page, pages);
    }
}

pub(super) fn game_row(ui: &mut Ui, game: &GameTotals, top: u64, today: Date) -> bool {
    let width = ui.available_width();
    let response = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(0, 5))
        .show(ui, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 3.0;
            let line = ui.text_style_height(&egui::TextStyle::Body);
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), line),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.label(icons::CARET_RIGHT);
                    ui.label(RichText::new(play_time(game.play_ms)).strong());
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(&game.name).truncate());
                    });
                },
            );
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "Last played {} · {}",
                        last_played(game.last_played, today),
                        sessions(game.sessions)
                    ))
                    .weak(),
                )
                .truncate(),
            );
            let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), Sense::hover());
            ui.painter()
                .rect_filled(bar, CornerRadius::same(2), ui.visuals().faint_bg_color);
            let share = game.play_ms as f32 / top as f32;
            let filled = Rect::from_min_size(bar.min, vec2(bar.width() * share, bar.height()));
            ui.painter()
                .rect_filled(filled, CornerRadius::same(2), ACCENT);
        })
        .response;
    let response = ui.interact(
        response.rect,
        ui.id().with(("game", &game.rom)),
        Sense::click(),
    );
    if response.hovered() {
        let painter = ui
            .painter()
            .with_clip_rect(ui.clip_rect().expand2(vec2(8.0, 0.0)));
        painter.rect_stroke(
            response.rect.expand2(vec2(6.0, 0.0)),
            CornerRadius::same(6),
            ui.visuals().widgets.hovered.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    response.clicked()
}

pub(super) fn game_tiles(ui: &mut Ui, game: &GameTotals) {
    tiles(
        ui,
        &[
            ("Today", play_time(game.today_ms)),
            ("All time", play_time(game.play_ms)),
            ("Sessions", count(game.sessions)),
            ("Longest session", play_time(game.longest_ms)),
        ],
    );
}

pub(super) fn game_facts(ui: &mut Ui, game: &GameTotals, today: Date) {
    let average = game.play_ms.checked_div(game.sessions).unwrap_or(0);
    facts(
        ui,
        &[
            ("First played", date_text(game.first_played, today)),
            (
                "Last played",
                capitalized(&last_played(game.last_played, today)),
            ),
            ("Average session", play_time(average)),
        ],
    );
    nonzero_facts(
        ui,
        &[
            ("Snapshots made", game.snapshots, count(game.snapshots)),
            ("Snapshots loaded", game.loads, count(game.loads)),
            (
                "Time online with friends",
                game.together_ms,
                play_time(game.together_ms),
            ),
        ],
    );
}

pub(super) fn game_page(
    ui: &mut Ui,
    state: &mut StatsUi,
    range: &mut Range,
    game: &GameTotals,
    today: Date,
    mobile: bool,
) {
    if !mobile {
        if ui
            .button(format!("{}  All games", icons::ARROW_LEFT))
            .clicked()
        {
            state.page = Page::Overview;
        }
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(&game.name).heading()).truncate());
        ui.add_space(6.0);
    }
    game_tiles(ui, game);
    ui.add_space(12.0);
    play_chart(ui, range, &game.rom, &game.days, today);
    ui.add_space(12.0);
    heading(ui, icons::LIST_NUMBERS, "Details");
    game_facts(ui, game, today);
    super::cable::section(ui, &game.cable);
    pokemon(ui, game);
}

pub(super) fn pokemon(ui: &mut Ui, game: &GameTotals) {
    let Some((_, latest)) = game.progress.iter().next_back() else {
        return;
    };
    ui.add_space(12.0);
    heading(ui, icons::TROPHY, "Game progress");
    tiles(
        ui,
        &[
            ("Badges", latest.badges.to_string()),
            ("Pokémon caught", latest.dex_owned.to_string()),
            ("Pokémon seen", latest.dex_seen.to_string()),
            ("Trainer card time", play_time(latest.in_game_secs * 1000)),
        ],
    );
    ui.add_space(8.0);
    progress_chart(ui, &game.progress);
}

pub(super) fn progress_chart(ui: &mut Ui, progress: &BTreeMap<String, Progress>) {
    let points: Vec<(Date, Progress)> = progress
        .iter()
        .filter_map(|(key, p)| parse_day(key).map(|d| (d, *p)))
        .collect();
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return;
    };
    let (outer, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 124.0), Sense::hover());
    let painter = ui.painter_at(outer);
    let rect = Rect::from_min_max(
        outer.min + vec2(14.0, 20.0),
        pos2(outer.max.x - 14.0, outer.max.y - 16.0),
    );
    let grid = ui.visuals().widgets.noninteractive.bg_stroke;
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], grid);
    let mut month = first.0.first_of_month();
    while month <= last.0 {
        if month >= first.0 {
            let x = rect.left()
                + (month - first.0).get_days() as f32 / span_days(first.0, last.0) * rect.width();
            painter.text(
                pos2(x, outer.bottom()),
                egui::Align2::CENTER_BOTTOM,
                month.strftime("%b").to_string(),
                egui::FontId::proportional(11.0),
                ui.visuals().weak_text_color(),
            );
        }
        let Ok(next) = month.checked_add(jiff::Span::new().months(1)) else {
            break;
        };
        month = next;
    }
    let span = span_days(first.0, last.0);
    let max = points
        .iter()
        .map(|(_, p)| p.dex_owned)
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let x_of = |d: Date| rect.left() + (d - first.0).get_days() as f32 / span * rect.width();
    let color = ACCENT;
    let line: Vec<egui::Pos2> = points
        .iter()
        .map(|(d, p)| {
            pos2(
                x_of(*d),
                rect.bottom() - p.dex_owned as f32 / max * (rect.height() - 14.0),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        line.clone(),
        egui::Stroke::new(2.0_f32, color),
    ));
    let mut badges = 0;
    let mut free_x = outer.left();
    for ((d, p), at) in points.iter().zip(&line) {
        if p.badges > badges {
            badges = p.badges;
            let x = x_of(*d);
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                ui.visuals().widgets.noninteractive.bg_stroke,
            );
            let full = format!("{} {}", icons::MEDAL, p.badges);
            for text in [full, icons::MEDAL.to_string()] {
                let galley = painter.layout_no_wrap(
                    text,
                    egui::FontId::proportional(11.0),
                    ui.visuals().text_color(),
                );
                let width = galley.size().x;
                let left = (x - width / 2.0).clamp(outer.left(), outer.right() - width);
                if left >= free_x {
                    painter.galley(pos2(left, outer.top()), galley, ui.visuals().text_color());
                    free_x = left + width + 4.0;
                    break;
                }
            }
        }
        painter.circle_filled(*at, 3.0, color);
    }
    if let Some(pointer) = response.hover_pos()
        && let Some((d, p)) = points
            .iter()
            .min_by_key(|(d, _)| (x_of(*d) - pointer.x).abs() as u32)
    {
        response.on_hover_text_at_pointer(format!(
            "{}: {} caught, {} badges",
            d.strftime("%-d %b"),
            p.dex_owned,
            p.badges
        ));
    }
}

pub(super) fn span_days(first: Date, last: Date) -> f32 {
    (last - first).get_days().max(1) as f32
}
