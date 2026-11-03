use super::chart::{bars, calendar};
use super::games::{progress_chart, sorted};
use super::{
    PAGE_SIZE, Page, Range, Sort, StatsUi, capitalized, count, date_text, last_played, play_time,
    sessions,
};
use crate::app::mobile::kit::{self, Row};
use crate::app::mobile::sheet;
use crate::app::tabs::linked::LINK_ICON;
use crate::icons;
use crate::stats::{CableTotals, GameTotals, Overview, cable_sum};
use egui::{Align, Context, Id, Layout, Ui, vec2};
use jiff::civil::Date;
use std::collections::BTreeMap;

const PILL_GAP: f32 = 4.0;
pub(super) const RANGES: [Range; 4] = [Range::Week, Range::Month, Range::Year, Range::Calendar];
const SORTS: [Sort; 3] = [Sort::MostPlayed, Sort::LastPlayed, Sort::Name];

pub(super) fn overview_page(
    ui: &mut Ui,
    state: &mut StatsUi,
    range: &mut Range,
    overview: &Overview,
    today: Date,
) {
    facts(
        ui,
        vec![
            ("Today", play_time(overview.today_ms)),
            ("All time", play_time(overview.play_ms)),
            ("Games played", count(overview.games_played as u64)),
            ("Sessions", count(overview.sessions)),
        ],
    );
    play_time_section(ui, range, "overview", &overview.days, today);
    let more = nonzero(&[
        (
            "Snapshots made",
            overview.snapshots,
            count(overview.snapshots),
        ),
        ("Snapshots loaded", overview.loads, count(overview.loads)),
        (
            "Time online with friends",
            overview.together_ms,
            play_time(overview.together_ms),
        ),
        (
            "Chat messages sent",
            overview.chat_messages,
            count(overview.chat_messages),
        ),
    ]);
    if !more.is_empty() {
        kit::section_label(ui, "Across all games");
        facts(ui, more);
    }
    cable(ui, &overview.cable);
    kit::section_label(ui, "Games");
    games(ui, state, overview, today);
}

pub(super) fn game_page(ui: &mut Ui, range: &mut Range, game: &GameTotals, today: Date) {
    facts(
        ui,
        vec![
            ("Today", play_time(game.today_ms)),
            ("All time", play_time(game.play_ms)),
            ("Sessions", count(game.sessions)),
            ("Longest session", play_time(game.longest_ms)),
        ],
    );
    play_time_section(ui, range, &game.rom, &game.days, today);
    kit::section_label(ui, "Details");
    let average = game.play_ms.checked_div(game.sessions).unwrap_or(0);
    let mut details = vec![
        ("First played", date_text(game.first_played, today)),
        (
            "Last played",
            capitalized(&last_played(game.last_played, today)),
        ),
        ("Average session", play_time(average)),
    ];
    details.extend(nonzero(&[
        ("Snapshots made", game.snapshots, count(game.snapshots)),
        ("Snapshots loaded", game.loads, count(game.loads)),
        (
            "Time online with friends",
            game.together_ms,
            play_time(game.together_ms),
        ),
    ]));
    facts(ui, details);
    cable(ui, &game.cable);
    progress(ui, game);
}

pub(super) fn sort_sheet(ctx: &Context, state: &mut StatsUi) {
    let current = state.sort;
    let shown = sheet::show(ctx, Id::new("stats_sort"), "Sort by", |ui| {
        sheet::choices(ui, SORTS, current, |sort| sort.label().to_owned())
    });
    if let Some(sort) = shown.inner {
        state.sort = sort;
        state.list_page = 0;
    }
    if shown.inner.is_some() || shown.closed {
        state.sort_sheet = false;
    }
}

fn facts(ui: &mut Ui, items: Vec<(&str, String)>) {
    let rows = items
        .into_iter()
        .map(|(label, value)| Row::new(label).value(value).no_chevron())
        .collect();
    kit::rows(ui, rows);
}

fn nonzero<'a>(items: &[(&'a str, u64, String)]) -> Vec<(&'a str, String)> {
    items
        .iter()
        .filter(|(_, n, _)| *n > 0)
        .map(|(label, _, value)| (*label, value.clone()))
        .collect()
}

fn play_time_section(
    ui: &mut Ui,
    range: &mut Range,
    id: &str,
    days: &BTreeMap<String, u64>,
    today: Date,
) {
    kit::section_label(ui, "Play time");
    range_pills(ui, range);
    ui.add_space(8.0);
    chart_card(ui, |ui| match *range {
        Range::Calendar => calendar(ui, id, days, today, true),
        range => bars(ui, id, days, today, range),
    });
}

pub(super) fn range_pills(ui: &mut Ui, range: &mut Range) {
    let height = ui.spacing().interact_size.y.max(36.0);
    let gaps = PILL_GAP * (RANGES.len() - 1) as f32;
    let width = (ui.available_width() - gaps) / RANGES.len() as f32;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = PILL_GAP;
        for pick in RANGES {
            let pill = ui.allocate_ui_with_layout(
                vec2(width, height),
                Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| kit::pill(ui, pick.label(), pick == *range),
            );
            if pill.inner.clicked() {
                *range = pick;
            }
        }
    });
}

fn chart_card(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    kit::card(ui, |ui| {
        kit::padded(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            body(ui);
        })
    });
}

fn cable(ui: &mut Ui, cable: &[CableTotals]) {
    if !super::cable::has_cable(cable) {
        return;
    }
    kit::section_label(ui, "Link cable");
    let mut lines = Vec::new();
    for (label, icon, online) in [
        ("Local cable", LINK_ICON, false),
        ("Online cable", icons::GLOBE, true),
    ] {
        let sum = cable_sum(cable, online);
        if sum.ms > 0 {
            lines.push((label, icon, sessions(sum.sessions), sum.ms));
        }
    }
    for totals in cable.iter().filter(|totals| totals.ms > 0) {
        let (icon, _) = super::cable::kind(&totals.partner);
        let way = if totals.partner.online() {
            "Online"
        } else {
            "Local"
        };
        let detail = format!("{way} · {}", sessions(totals.sessions));
        lines.push((super::cable::name(totals), icon, detail, totals.ms));
    }
    let rows = lines
        .iter()
        .map(|(label, icon, detail, ms)| {
            Row::new(label)
                .icon(icon)
                .detail(detail)
                .value(play_time(*ms))
                .no_chevron()
        })
        .collect();
    kit::rows(ui, rows);
}

fn games(ui: &mut Ui, state: &mut StatsUi, overview: &Overview, today: Date) {
    if overview.games.len() > 1 {
        find_row(ui, state);
        ui.add_space(8.0);
    }
    let games = sorted(state, overview);
    if games.is_empty() {
        kit::card(ui, |ui| kit::padded(ui, |ui| ui.weak("No game matches.")));
        return;
    }
    let shown = (state.list_page + 1).saturating_mul(PAGE_SIZE);
    let details: Vec<String> = games
        .iter()
        .take(shown)
        .map(|game| {
            format!(
                "Played {} · {}",
                last_played(game.last_played, today),
                sessions(game.sessions)
            )
        })
        .collect();
    let rows = games
        .iter()
        .zip(&details)
        .map(|(game, detail)| {
            Row::new(&game.name)
                .detail(detail)
                .value(play_time(game.play_ms))
        })
        .collect();
    if let Some(index) = kit::rows(ui, rows) {
        state.page = Page::Game {
            rom: games[index].rom.clone(),
            name: games[index].name.clone(),
        };
    }
    if games.len() > shown {
        ui.add_space(8.0);
        if kit::wide_button(ui, "Show more", false).clicked() {
            state.list_page += 1;
        }
    }
}

fn find_row(ui: &mut Ui, state: &mut StatsUi) {
    let height = ui.spacing().interact_size.y.max(40.0);
    let search = state.search.clone();
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        Layout::right_to_left(Align::Center),
        |ui| {
            let label = format!("{}  {}", icons::ARROWS_DOWN_UP, state.sort.label());
            if kit::pill(ui, &label, false).clicked() {
                state.sort_sheet = true;
            }
            kit::field(
                ui,
                egui::TextEdit::singleline(&mut state.search)
                    .hint_text(format!("{}  Find a game", icons::MAGNIFYING_GLASS)),
            );
        },
    );
    if search != state.search {
        state.list_page = 0;
    }
}

fn progress(ui: &mut Ui, game: &GameTotals) {
    let Some((_, latest)) = game.progress.iter().next_back() else {
        return;
    };
    kit::section_label(ui, "Game progress");
    facts(
        ui,
        vec![
            ("Badges", latest.badges.to_string()),
            ("Pokémon caught", latest.dex_owned.to_string()),
            ("Pokémon seen", latest.dex_seen.to_string()),
            ("Trainer card time", play_time(latest.in_game_secs * 1000)),
        ],
    );
    ui.add_space(8.0);
    chart_card(ui, |ui| progress_chart(ui, &game.progress));
}
