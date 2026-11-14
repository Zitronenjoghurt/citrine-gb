use crate::app::big_screen::scroll::Following;
use crate::app::events::AppEventQueue;
use crate::app::tabs::{Tab, TabViewer};
use crate::icons;
use crate::stats::Overview;
use chart::{bars, calendar};
use egui::{CornerRadius, RichText, Ui, vec2};
use games::{game_page, games_list};
use jiff::civil::Date;
use std::collections::BTreeMap;

mod cable;
mod chart;
mod games;
mod phone;

pub const PAGE_SIZE: usize = 20;
const CHART_HEIGHT: f32 = 120.0;
const WIDE: f32 = 440.0;
const NARROW: f32 = 300.0;
const TOUCH_CELL: f32 = 24.0;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Overview,
    Game {
        rom: String,
        name: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Range {
    #[default]
    Week,
    Month,
    Year,
    Calendar,
}

impl Range {
    fn label(self) -> &'static str {
        match self {
            Self::Week => "Week",
            Self::Month => "Month",
            Self::Year => "Year",
            Self::Calendar => "Calendar",
        }
    }

    fn total_label(self) -> &'static str {
        match self {
            Self::Week => "This week",
            Self::Month => "This month",
            Self::Year | Self::Calendar => "This year",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    MostPlayed,
    LastPlayed,
    Name,
}

impl Sort {
    fn label(self) -> &'static str {
        match self {
            Self::MostPlayed => "Most played",
            Self::LastPlayed => "Last played",
            Self::Name => "Name",
        }
    }
}

#[derive(Default)]
pub struct StatsUi {
    pub page: Page,
    pub sort: Sort,
    pub search: String,
    pub list_page: usize,
    pub sort_sheet: bool,
}

pub fn play_time(ms: u64) -> String {
    let minutes = ms / 60_000;
    match minutes / 60 {
        0 => format!("{minutes} min"),
        hours => format!("{hours} h {:02} min", minutes % 60),
    }
}

fn scale_top(ms: u64) -> u64 {
    let step = match ms {
        0..3_600_000 => 600_000,
        3_600_000..36_000_000 => 3_600_000,
        _ => 36_000_000,
    };
    ms.div_ceil(step).max(1) * step
}

pub fn short_time(ms: u64) -> String {
    let minutes = ms / 60_000;
    match minutes {
        0..=59 => format!("{minutes} min"),
        _ => format!("{} h", minutes / 60),
    }
}

pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn day_text(day: Date, today: Date) -> String {
    if day.year() == today.year() {
        day.strftime("%-d %b").to_string()
    } else {
        day.strftime("%-d %b %Y").to_string()
    }
}

fn date_text(unix: u64, today: Date) -> String {
    day_text(crate::stats::local_date(unix), today)
}

fn sessions(n: u64) -> String {
    match n {
        1 => "1 session".into(),
        n => format!("{} sessions", count(n)),
    }
}

pub fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

pub fn last_played(unix: u64, today: Date) -> String {
    let day = crate::stats::local_date(unix);
    if day == today {
        "today".into()
    } else if today.yesterday().is_ok_and(|y| y == day) {
        "yesterday".into()
    } else {
        day_text(day, today)
    }
}

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let today = crate::stats::today();
    let mobile = viewer.ui.settings.layout.is_touch(ui.ctx());
    let events = &mut *viewer.events;
    let overview = viewer.stats.book().overview(today);
    let state = &mut viewer.ui.stats;
    let settings = &mut viewer.ui.settings;
    let mut range = settings.stats_range;
    if let Page::Game { rom, .. } = &state.page
        && overview.game(rom).is_none()
    {
        state.page = Page::Overview;
    }
    if state.page != Page::Overview
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        state.page = Page::Overview;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_following(ui, |ui| {
            ui.add_space(4.0);
            if overview.games.is_empty() {
                empty(ui, events);
                return;
            }
            match (state.page.clone(), mobile) {
                (Page::Game { rom, .. }, _) => {
                    if let Some(game) = overview.game(&rom) {
                        if mobile {
                            phone::game_page(ui, &mut range, game, today);
                        } else {
                            game_page(ui, state, &mut range, game, today);
                        }
                    }
                }
                (Page::Overview, true) => {
                    phone::overview_page(ui, state, &mut range, &overview, today)
                }
                (Page::Overview, false) => overview_page(ui, state, &mut range, &overview, today),
            }
            ui.add_space(if mobile { 12.0 } else { 4.0 });
        });
    if mobile && state.sort_sheet {
        phone::sort_sheet(ui.ctx(), state);
    }
    if range != settings.stats_range {
        settings.stats_range = range;
        settings.dirty = true;
    }
}

fn empty(ui: &mut Ui, events: &mut AppEventQueue) {
    ui.add_space(ui.available_height() * 0.2);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(icons::CHART_BAR).size(32.0).weak());
        ui.add_space(6.0);
        ui.label("Play a game and your play time shows here.");
        ui.add_space(10.0);
        if ui
            .button(format!("{}  {}", icons::JOYSTICK, Tab::Homebrew.label()))
            .clicked()
        {
            events.open_tab(Tab::Homebrew);
        }
    });
}

fn overview_page(
    ui: &mut Ui,
    state: &mut StatsUi,
    range: &mut Range,
    overview: &Overview,
    today: Date,
) {
    tiles(
        ui,
        &[
            ("Today", play_time(overview.today_ms)),
            ("All time", play_time(overview.play_ms)),
            ("Games played", count(overview.games_played as u64)),
            ("Sessions", count(overview.sessions)),
        ],
    );
    ui.add_space(12.0);
    play_chart(ui, range, "overview", &overview.days, today);
    let more = [
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
    ];
    if more.iter().any(|(_, n, _)| *n > 0) {
        ui.add_space(12.0);
        heading(ui, icons::LIST_NUMBERS, "Across all games");
        nonzero_facts(ui, &more);
    }
    cable::section(ui, &overview.cable);
    ui.add_space(12.0);
    heading(ui, icons::GAME_CONTROLLER, "Games");
    games_list(ui, state, overview, today);
}

fn play_chart(ui: &mut Ui, range: &mut Range, id: &str, days: &BTreeMap<String, u64>, today: Date) {
    heading(ui, icons::CHART_BAR, "Play time");
    let touch = crate::app::mobile::touch_square(ui) > 0.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if touch {
            ui.spacing_mut().button_padding.x = 14.0;
        }
        for pick in [Range::Week, Range::Month, Range::Year, Range::Calendar] {
            ui.selectable_value(range, pick, pick.label());
        }
    });
    match *range {
        Range::Calendar => calendar(ui, id, days, today, touch),
        range => bars(ui, id, days, today, range),
    }
}

fn heading(ui: &mut Ui, icon: &str, text: &str) {
    ui.label(RichText::new(format!("{icon}  {text}")).heading());
}

fn tiles(ui: &mut Ui, items: &[(&str, String)]) {
    let edge = 20.0 + 2.0 * ui.visuals().widgets.noninteractive.bg_stroke.width;
    let card = |ui: &mut Ui, add: &mut dyn FnMut(&mut Ui)| {
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| add(ui));
    };
    if ui.available_width() < NARROW {
        let width = ui.available_width() - edge;
        card(ui, &mut |ui| {
            ui.set_width(width);
            for (label, value) in items {
                fact_row(ui, label, RichText::new(value).size(16.0).strong());
            }
        });
        return;
    }
    let columns = if ui.available_width() >= WIDE { 4 } else { 2 };
    let gap = ui.spacing().item_spacing.x;
    let width = (ui.available_width() - gap * (columns as f32 - 1.0)) / columns as f32;
    for row in items.chunks(columns) {
        ui.horizontal(|ui| {
            for (label, value) in row {
                card(ui, &mut |ui| {
                    ui.set_width(width - edge);
                    ui.vertical(|ui| {
                        ui.weak(*label);
                        ui.add(
                            egui::Label::new(RichText::new(value).size(18.0).strong()).truncate(),
                        );
                    });
                });
            }
        });
    }
}

fn fact_row(ui: &mut Ui, label: &str, value: impl Into<egui::WidgetText>) {
    let height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.weak(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(value);
            });
        },
    );
}

fn facts(ui: &mut Ui, items: &[(&str, String)]) {
    for (label, value) in items {
        fact_row(ui, label, value.as_str());
    }
}

fn nonzero_facts(ui: &mut Ui, items: &[(&str, u64, String)]) {
    for (label, n, value) in items {
        if *n > 0 {
            fact_row(ui, label, value.as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::GameTotals;
    use egui::{Rect, pos2};

    #[test]
    fn play_time_reads_as_hours_and_minutes() {
        assert_eq!(play_time(42_000), "0 min");
        assert_eq!(play_time(5 * 60_000), "5 min");
        assert_eq!(play_time(185 * 60_000), "3 h 05 min");
        assert_eq!(play_time(130 * 3_600_000 + 60_000), "130 h 01 min");
    }

    #[test]
    fn chart_scale_rounds_up() {
        assert_eq!(short_time(scale_top(0)), "10 min");
        assert_eq!(short_time(scale_top(47 * 60_000)), "50 min");
        assert_eq!(short_time(scale_top(111 * 60_000)), "2 h");
        assert_eq!(short_time(scale_top(120 * 60_000)), "2 h");
        assert_eq!(short_time(scale_top(47 * 3_600_000)), "50 h");
    }

    #[test]
    fn sessions_say_one_session() {
        assert_eq!(sessions(1), "1 session");
        assert_eq!(sessions(1253), "1,253 sessions");
    }

    #[test]
    fn counts_group_thousands() {
        assert_eq!(count(7), "7");
        assert_eq!(count(1253), "1,253");
        assert_eq!(count(1_000_000), "1,000,000");
    }

    fn tap_calendar(edge: bool) -> Option<Date> {
        let saturday = Date::new(2026, 10, 31).unwrap();
        let days = BTreeMap::new();
        let ctx = egui::Context::default();
        let mut picked_id = None;
        let mut target = egui::Pos2::ZERO;
        for step in 0..6 {
            let mut input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(412.0, 900.0))),
                ..Default::default()
            };
            match step {
                2 => input.events.push(egui::Event::PointerMoved(target)),
                3 | 4 => input.events.push(egui::Event::PointerButton {
                    pos: target,
                    button: egui::PointerButton::Primary,
                    pressed: step == 3,
                    modifiers: Default::default(),
                }),
                _ => {}
            }
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let start = ui.cursor().min;
                    let room = ui.available_width() - 28.0;
                    let weeks = ((room / (TOUCH_CELL + 3.0)).floor() as usize).max(1);
                    let cell = room / weeks as f32 - 3.0;
                    let x = start.x + 28.0 + (weeks - 1) as f32 * (cell + 3.0);
                    let y = start.y + 16.0 + 2.0 * (cell + 3.0) + cell / 2.0;
                    target = pos2(if edge { x + 0.5 } else { x + cell / 2.0 }, y);
                    picked_id = Some(ui.id().with(("t", "picked_day")));
                    calendar(ui, "t", &days, saturday, true);
                });
            });
        }
        ctx.data(|d| d.get_temp::<Option<Date>>(picked_id.unwrap()))
            .flatten()
    }

    #[test]
    fn tapping_a_calendar_day_picks_it() {
        let wednesday = Date::new(2026, 10, 28).unwrap();
        assert_eq!(tap_calendar(false), Some(wednesday));
        assert_eq!(tap_calendar(true), Some(wednesday));
    }

    fn phone_overview() -> Overview {
        let game = |rom: &str, name: &str, play_ms: u64| GameTotals {
            rom: rom.into(),
            name: name.into(),
            play_ms,
            sessions: 2,
            ..Default::default()
        };
        Overview {
            play_ms: 5_400_000,
            games_played: 2,
            games: vec![
                game("tetris", "Tetris", 3_600_000),
                game("zelda", "Link's Awakening", 1_800_000),
            ],
            ..Default::default()
        }
    }

    fn phone_frame(
        ctx: &egui::Context,
        state: &mut StatsUi,
        range: &mut Range,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let overview = phone_overview();
        let today = Date::new(2026, 10, 31).unwrap();
        ctx.style_mut(|style| style.spacing.interact_size.y = 44.0);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(360.0, 2400.0))),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.inner_margin(14))
                .show(ctx, |ui| {
                    phone::overview_page(ui, state, range, &overview, today);
                });
        })
    }

    fn shown_text(output: &egui::FullOutput, wanted: &str) -> Rect {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{wanted} is not on screen"))
    }

    fn tap_text(ctx: &egui::Context, state: &mut StatsUi, range: &mut Range, wanted: &str) {
        let at = shown_text(&phone_frame(ctx, state, range, Vec::new()), wanted).center();
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        phone_frame(ctx, state, range, vec![egui::Event::PointerMoved(at)]);
        phone_frame(ctx, state, range, vec![press(true)]);
        phone_frame(ctx, state, range, vec![press(false)]);
    }

    #[test]
    fn phone_range_pills_fit_one_line_at_360_px() {
        let ctx = egui::Context::default();
        let mut state = StatsUi::default();
        let mut range = Range::Week;
        phone_frame(&ctx, &mut state, &mut range, Vec::new());
        let output = phone_frame(&ctx, &mut state, &mut range, Vec::new());
        let pills: Vec<Rect> = phone::RANGES
            .iter()
            .map(|pick| shown_text(&output, pick.label()))
            .collect();
        for pill in &pills {
            assert!((pill.center().y - pills[0].center().y).abs() < 1.0);
            assert!(pill.right() <= 346.0, "{pill:?}");
        }
        tap_text(&ctx, &mut state, &mut range, "Month");
        assert_eq!(range, Range::Month);
    }

    #[test]
    fn a_phone_game_row_opens_the_game_page() {
        let ctx = egui::Context::default();
        let mut state = StatsUi::default();
        let mut range = Range::Week;
        phone_frame(&ctx, &mut state, &mut range, Vec::new());
        tap_text(&ctx, &mut state, &mut range, "Link's Awakening");
        assert_eq!(
            state.page,
            Page::Game {
                rom: "zelda".into(),
                name: "Link's Awakening".into(),
            }
        );
    }

    #[test]
    fn a_phone_games_list_has_a_search_and_a_sort_on_one_line() {
        let ctx = egui::Context::default();
        let mut state = StatsUi::default();
        let mut range = Range::Week;
        phone_frame(&ctx, &mut state, &mut range, Vec::new());
        let output = phone_frame(&ctx, &mut state, &mut range, Vec::new());
        let sort_label = format!("{}  Most played", crate::icons::ARROWS_DOWN_UP);
        let find_label = format!("{}  Find a game", crate::icons::MAGNIFYING_GLASS);
        let sort = shown_text(&output, &sort_label);
        let find = shown_text(&output, &find_label);
        assert!((sort.center().y - find.center().y).abs() < 1.0);
        assert!(sort.right() <= 346.0, "{sort:?}");
        tap_text(&ctx, &mut state, &mut range, &sort_label);
        assert!(state.sort_sheet);
    }
}
