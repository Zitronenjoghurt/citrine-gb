use super::games::{progress_chart, sorted};
use super::{
    CARD_GAMES, PAGE_SIZE, Page, Picked, RANGES, Range, Sort, StatsUi, View, capitalized, chart,
    count, date_text, last_played, play_time, sessions,
};
use crate::app::mobile::kit::{self, Row};
use crate::app::mobile::sheet;
use crate::app::tabs::linked::LINK_ICON;
use crate::icons;
use crate::stats::{CableTotals, GameTotals, Overview, cable_sum};
use egui::{Align, Context, Id, Layout, Ui, vec2};
use jiff::civil::Date;
use std::collections::BTreeMap;

const SORTS: [Sort; 3] = [Sort::MostPlayed, Sort::LastPlayed, Sort::Name];

pub(super) fn overview_page(
    ui: &mut Ui,
    state: &mut StatsUi,
    range: &mut Range,
    view: &View,
    overview: &Overview,
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
    play_time_section(ui, state, range, view, "overview", &overview.days, None);
    kit::section_label(ui, "Games");
    games(ui, state, overview, view.today);
    together(
        ui,
        overview.together_ms,
        overview.chat_messages,
        &overview.cable,
    );
    snapshots(ui, overview.snapshots, overview.loads);
}

pub(super) fn game_page(
    ui: &mut Ui,
    state: &mut StatsUi,
    range: &mut Range,
    view: &View,
    game: &GameTotals,
) {
    facts(
        ui,
        vec![
            ("Today", play_time(game.today_ms)),
            ("All time", play_time(game.play_ms)),
            ("Sessions", count(game.sessions)),
            ("Longest session", play_time(game.longest_ms)),
        ],
    );
    play_time_section(
        ui,
        state,
        range,
        view,
        &game.rom,
        &game.days,
        Some(&game.rom),
    );
    kit::section_label(ui, "Details");
    let average = game.play_ms.checked_div(game.sessions).unwrap_or(0);
    let mut rows = Vec::new();
    if game.first_played > 0 {
        rows.push(("First played", date_text(game.first_played, view.today)));
    }
    if game.last_played > 0 {
        rows.push((
            "Last played",
            capitalized(&last_played(game.last_played, view.today)),
        ));
    }
    rows.push(("Average session", play_time(average)));
    facts(ui, rows);
    together(ui, game.together_ms, 0, &game.cable);
    snapshots(ui, game.snapshots, game.loads);
    progress(ui, game);
}

fn together(ui: &mut Ui, together_ms: u64, chat_messages: u64, cable: &[CableTotals]) {
    let more = nonzero(&[
        (
            "Time online with friends",
            together_ms,
            play_time(together_ms),
        ),
        ("Chat messages sent", chat_messages, count(chat_messages)),
    ]);
    if more.is_empty() && !super::cable::has_cable(cable) {
        return;
    }
    kit::section_label(ui, "Playing together");
    cable_rows(ui, cable);
    if !more.is_empty() {
        if super::cable::has_cable(cable) {
            ui.add_space(8.0);
        }
        facts(ui, more);
    }
}

fn snapshots(ui: &mut Ui, made: u64, loaded: u64) {
    if made == 0 && loaded == 0 {
        return;
    }
    kit::section_label(ui, "Snapshots");
    facts(ui, vec![("Made", count(made)), ("Loaded", count(loaded))]);
}

fn period_card(ui: &mut Ui, picked: &Picked, every_game: bool) -> bool {
    let period = &picked.period;
    let button = picked.pinned.then_some((icons::X, "Clear the pick"));
    let cleared = kit::section_label_total(ui, &picked.title, &play_time(period.play_ms), button);
    let mut games: Vec<(String, String)> = Vec::new();
    if every_game {
        for game in period.games.iter().take(CARD_GAMES) {
            games.push((game.name.clone(), play_time(game.play_ms)));
        }
        let rest = &period.games[period.games.len().min(CARD_GAMES)..];
        if !rest.is_empty() {
            let label = match rest.len() {
                1 => "1 more game".to_owned(),
                n => format!("{n} more games"),
            };
            games.push((label, play_time(rest.iter().map(|game| game.play_ms).sum())));
        }
    }
    let mut facts: Vec<(String, String)> = Vec::new();
    if period.play_ms > 0 {
        facts.push(("Sessions".into(), count(period.sessions)));
    }
    let link = super::cable::cable_ms(&period.cable);
    for (label, n, value) in [
        ("Snapshots made", period.snapshots, count(period.snapshots)),
        ("Snapshots loaded", period.loads, count(period.loads)),
        ("Link cable", link, play_time(link)),
        (
            "Time online with friends",
            period.together_ms,
            play_time(period.together_ms),
        ),
        (
            "Chat messages sent",
            period.chat_messages,
            count(period.chat_messages),
        ),
    ] {
        if n > 0 {
            facts.push((label.into(), value));
        }
    }
    value_rows(ui, &games);
    if !games.is_empty() && !facts.is_empty() {
        kit::section_label(ui, super::ACROSS_GAMES);
    }
    value_rows(ui, &facts);
    cleared
}

fn value_rows(ui: &mut Ui, lines: &[(String, String)]) {
    if lines.is_empty() {
        return;
    }
    let rows = lines
        .iter()
        .map(|(label, value)| Row::new(label).value(value.clone()).no_chevron())
        .collect();
    kit::rows(ui, rows);
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
    state: &mut StatsUi,
    range: &mut Range,
    view: &View,
    id: &str,
    days: &BTreeMap<String, u64>,
    rom: Option<&str>,
) -> Picked {
    kit::section_label(ui, "Time played");
    if let Some(pick) = kit::segmented(ui, &RANGES, *range) {
        *range = pick;
        state.picked = None;
    }
    ui.add_space(8.0);
    let picked = chart_card(ui, |ui| chart(ui, state, *range, view, id, days, rom, true));
    if period_card(ui, &picked, rom.is_none()) {
        state.picked = None;
    }
    picked
}

fn chart_card<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    kit::card(ui, |ui| {
        ui.visuals_mut().panel_fill = kit::card_fill(ui.visuals());
        kit::padded(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            body(ui)
        })
    })
}

fn cable_rows(ui: &mut Ui, cable: &[CableTotals]) {
    if !super::cable::has_cable(cable) {
        return;
    }
    let mut lines = Vec::new();
    for (label, icon, online) in [
        ("Local cable", LINK_ICON, false),
        ("Online cable", icons::GLOBE_SIMPLE, true),
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
            if game.last_played == 0 {
                return "Not played yet".to_owned();
            }
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

#[cfg(test)]
mod tests {
    use crate::app::ui_state::ui_theme::UiTheme;
    use egui::Color32;

    fn grey(colour: Color32) -> f32 {
        let [r, g, b, _] = colour.to_array();
        0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
    }

    #[test]
    fn a_played_day_lists_its_games_apart_from_the_other_counts() {
        use super::super::tests::{played_day, text_y};
        for every_game in [true, false] {
            let ctx = egui::Context::default();
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    super::period_card(ui, &played_day(), every_game);
                });
            });
            let sessions = text_y(&output, "Sessions").expect("Sessions");
            let across = text_y(&output, super::super::ACROSS_GAMES);
            if every_game {
                let game = text_y(&output, "Tetris").expect("a game row");
                let across = across.expect("the games and the counts share one card");
                assert!(game < across && across < sessions);
            } else {
                assert_eq!(across, None);
            }
        }
    }

    #[test]
    fn an_empty_calendar_day_shows_on_the_chart_card_as_on_the_desktop_page() {
        for theme in [UiTheme::Light, UiTheme::Dark] {
            let ctx = egui::Context::default();
            theme.apply(&ctx);
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let page = ui.visuals().panel_fill;
                    let desktop = super::super::chart::empty_fill(ui);
                    let (card, phone) = super::chart_card(ui, |ui| {
                        (ui.visuals().panel_fill, super::super::chart::empty_fill(ui))
                    });
                    assert_eq!(card, super::kit::card_fill(ui.visuals()), "{theme:?}");
                    let desktop_lift = (grey(desktop) - grey(page)).abs();
                    let phone_lift = (grey(phone) - grey(card)).abs();
                    assert!(
                        phone_lift >= 12.0 && phone_lift >= desktop_lift - 4.0,
                        "{theme:?}: an empty day is {phone_lift} off the card, {desktop_lift} off the desktop page"
                    );
                });
            });
        }
    }
}
