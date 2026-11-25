use crate::app::big_screen::scroll::Following;
use crate::app::events::AppEventQueue;
use crate::app::tabs::{Tab, TabViewer};
use crate::app::widgets::segmented::segmented;
use crate::icons;
use crate::stats::{Book, Overview, Period};
use chart::{Shown, bars, calendar};
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
const SECTION_GAP: f32 = 18.0;
pub(super) const ACROSS_GAMES: &str = "Across all games";
const CARD_GAMES: usize = 3;
pub(super) const RANGES: [(Range, &str); 4] = [
    (Range::Week, "Week"),
    (Range::Month, "Month"),
    (Range::Year, "Year"),
    (Range::Calendar, "Calendar"),
];

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
    fn span(self, today: Date) -> (Date, Date) {
        match self {
            Self::Week => {
                let monday = crate::stats::week_start(today);
                let sunday = monday
                    .checked_add(jiff::Span::new().days(6))
                    .unwrap_or(today);
                (monday, sunday)
            }
            Self::Month => (today.first_of_month(), today.last_of_month()),
            Self::Year | Self::Calendar => (today.first_of_year(), today.last_of_year()),
        }
    }

    fn title(self, weeks: usize) -> String {
        match self {
            Self::Week => "This week".into(),
            Self::Month => "This month".into(),
            Self::Year => "This year".into(),
            Self::Calendar => format!("Last {weeks} weeks"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Day(Date),
    Month(Date),
}

impl Pick {
    fn span(self) -> (Date, Date) {
        match self {
            Self::Day(day) => (day, day),
            Self::Month(month) => (month.first_of_month(), month.last_of_month()),
        }
    }

    fn title(self, today: Date) -> String {
        match self {
            Self::Day(day) if day == today => format!("Today, {}", day.strftime("%-d %b")),
            Self::Day(day) if day.year() == today.year() => day.strftime("%A %-d %b").to_string(),
            Self::Day(day) => day.strftime("%A %-d %b %Y").to_string(),
            Self::Month(month) if month.year() == today.year() => month.strftime("%B").to_string(),
            Self::Month(month) => month.strftime("%B %Y").to_string(),
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
    pub picked: Option<Pick>,
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
    let book = viewer.stats.book();
    let overview = book.overview(today);
    let state = &mut viewer.ui.stats;
    let settings = &mut viewer.ui.settings;
    let mut range = settings.stats_range;
    if let Page::Game { rom, .. } = &state.page
        && overview.game(rom).is_none()
    {
        state.page = Page::Overview;
    }
    escape(ui, state);
    let page = state.page.clone();
    scrolled(ui, mobile, |ui| {
        if overview.games.is_empty() {
            empty(ui, events);
            return;
        }
        let view = View { book, today };
        match (page, mobile) {
            (Page::Game { rom, .. }, _) => {
                if let Some(game) = overview.game(&rom) {
                    if mobile {
                        phone::game_page(ui, state, &mut range, &view, game);
                    } else {
                        game_page(ui, state, &mut range, &view, game);
                    }
                }
            }
            (Page::Overview, true) => phone::overview_page(ui, state, &mut range, &view, &overview),
            (Page::Overview, false) => overview_page(ui, state, &mut range, &view, &overview),
        }
    });
    if mobile && state.sort_sheet {
        phone::sort_sheet(ui.ctx(), state);
    }
    if range != settings.stats_range {
        settings.stats_range = range;
        settings.dirty = true;
    }
}

fn escape(ui: &Ui, state: &mut StatsUi) {
    if (state.picked.is_some() || state.page != Page::Overview)
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        if state.picked.is_some() {
            state.picked = None;
        } else {
            state.page = Page::Overview;
        }
    }
}

fn scrolled(ui: &mut Ui, mobile: bool, body: impl FnOnce(&mut Ui)) {
    let gutter = if mobile {
        0.0
    } else {
        crate::app::widgets::scroll::fixed_gutter(ui)
    };
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_following(ui, |ui| {
            ui.set_max_width(ui.available_width() - gutter);
            ui.add_space(4.0);
            body(ui);
            ui.add_space(if mobile { 12.0 } else { 8.0 });
        });
}

pub(super) struct View<'a> {
    pub(super) book: &'a Book,
    pub(super) today: Date,
}

pub(super) struct Picked {
    pub(super) title: String,
    pub(super) period: Period,
    pub(super) pinned: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn chart(
    ui: &mut Ui,
    state: &mut StatsUi,
    range: Range,
    view: &View,
    id: &str,
    days: &BTreeMap<String, u64>,
    rom: Option<&str>,
    touch: bool,
) -> Picked {
    let shown: Shown = match range {
        Range::Calendar => calendar(ui, id, days, view.today, touch, &mut state.picked),
        range => bars(ui, id, days, view.today, range, &mut state.picked),
    };
    let look = shown.hovered.or(state.picked);
    let (first, last) = look.map_or((shown.first, shown.last), Pick::span);
    Picked {
        title: look.map_or_else(|| range.title(shown.weeks), |pick| pick.title(view.today)),
        period: view.book.period(first, last, rom),
        pinned: state.picked.is_some() && shown.hovered.is_none_or(|h| Some(h) == state.picked),
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
            .button(format!("{}  {}", icons::HOMEBREW, Tab::Homebrew.label()))
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
    view: &View,
    overview: &Overview,
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
    time_played(ui, state, range, view, "overview", &overview.days, None);
    section(ui, icons::CARTRIDGE, "Games", |_| {});
    games_list(ui, state, overview, view.today);
    together(
        ui,
        overview.together_ms,
        overview.chat_messages,
        &overview.cable,
    );
    snapshots(ui, overview.snapshots, overview.loads);
}

fn time_played(
    ui: &mut Ui,
    state: &mut StatsUi,
    range: &mut Range,
    view: &View,
    id: &str,
    days: &BTreeMap<String, u64>,
    rom: Option<&str>,
) {
    section(ui, icons::CHART_BAR, "Time played", |ui| {
        range_control(ui, state, range)
    });
    let picked = chart(ui, state, *range, view, id, days, rom, false);
    ui.add_space(8.0);
    if period_card(ui, &picked, rom.is_none()) {
        state.picked = None;
    }
}

fn range_control(ui: &mut Ui, state: &mut StatsUi, range: &mut Range) {
    if let Some(pick) = segmented(ui, &RANGES, *range) {
        *range = pick;
        state.picked = None;
    }
}

fn period_card(ui: &mut Ui, picked: &Picked, every_game: bool) -> bool {
    let period = &picked.period;
    let mut cleared = false;
    card(ui, |ui| {
        title_row(ui, &picked.title, |ui| {
            if picked.pinned {
                cleared = ui
                    .small_button(icons::X)
                    .on_hover_text("Esc clears the pick")
                    .clicked();
            }
            ui.label(RichText::new(play_time(period.play_ms)).size(16.0).strong());
        });
        if period.play_ms == 0 {
            ui.weak("Nothing played.");
            return;
        }
        if !every_game {
            period_facts(ui, period);
            return;
        }
        for game in period.games.iter().take(CARD_GAMES) {
            fact_row(ui, &game.name, play_time(game.play_ms));
        }
        let rest = &period.games[period.games.len().min(CARD_GAMES)..];
        if !rest.is_empty() {
            let ms = rest.iter().map(|game| game.play_ms).sum();
            let label = match rest.len() {
                1 => "1 more game".to_owned(),
                n => format!("{n} more games"),
            };
            fact_row(ui, &label, RichText::new(play_time(ms)).weak());
        }
    });
    if every_game && period.play_ms > 0 {
        ui.add_space(8.0);
        card(ui, |ui| {
            title_row(ui, ACROSS_GAMES, |_| {});
            period_facts(ui, period);
        });
    }
    cleared
}

fn title_row(ui: &mut Ui, title: &str, right: impl FnOnce(&mut Ui)) {
    let height = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add(egui::Label::new(RichText::new(title).strong()).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
        },
    );
}

pub(super) fn period_facts(ui: &mut Ui, period: &Period) {
    facts(ui, &[("Sessions", count(period.sessions))]);
    let link = cable::cable_ms(&period.cable);
    nonzero_facts(
        ui,
        &[
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
        ],
    );
}

fn together(
    ui: &mut Ui,
    together_ms: u64,
    chat_messages: u64,
    cable: &[crate::stats::CableTotals],
) {
    if together_ms == 0 && chat_messages == 0 && !cable::has_cable(cable) {
        return;
    }
    section(ui, icons::USERS, "Playing together", |_| {});
    cable::rows(ui, cable);
    nonzero_facts(
        ui,
        &[
            (
                "Time online with friends",
                together_ms,
                play_time(together_ms),
            ),
            ("Chat messages sent", chat_messages, count(chat_messages)),
        ],
    );
}

fn snapshots(ui: &mut Ui, made: u64, loaded: u64) {
    if made == 0 && loaded == 0 {
        return;
    }
    section(ui, icons::SAVE_STATE, "Snapshots", |_| {});
    facts(ui, &[("Made", count(made)), ("Loaded", count(loaded))]);
}

fn section(ui: &mut Ui, icon: &str, text: &str, right: impl FnOnce(&mut Ui)) {
    ui.add_space(SECTION_GAP);
    let height = ui.spacing().interact_size.y + 6.0;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add(egui::Label::new(RichText::new(format!("{icon}  {text}")).heading()).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
        },
    );
    ui.add_space(6.0);
}

fn heading(ui: &mut Ui, icon: &str, text: &str) {
    section(ui, icon, text, |_| {});
}

fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let edge = 20.0 + 2.0 * ui.visuals().widgets.noninteractive.bg_stroke.width;
    let width = ui.available_width() - edge;
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(width);
            add(ui);
        });
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
    fn a_game_opened_but_never_played_has_no_last_played_date() {
        let noon = 1_763_208_000;
        let today = crate::stats::local_date(noon);
        let opened = GameTotals {
            launches: 1,
            ..Default::default()
        };
        assert_eq!(games::played_line(&opened, today), "Not played yet");
        let played = GameTotals {
            last_played: noon,
            sessions: 2,
            ..Default::default()
        };
        assert_eq!(
            games::played_line(&played, today),
            "Last played today · 2 sessions"
        );
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

    fn tap_calendar(edge: bool, touch: bool) -> Option<Pick> {
        let saturday = Date::new(2026, 10, 31).unwrap();
        let days = BTreeMap::new();
        let ctx = egui::Context::default();
        let mut picked = None;
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
                    let weeks = if touch {
                        ((room / (TOUCH_CELL + 3.0)).floor() as usize).max(1)
                    } else if ui.available_width() >= WIDE {
                        26
                    } else {
                        17
                    };
                    let most = if touch { f32::MAX } else { 18.0 };
                    let cell = (room / weeks as f32 - 3.0).clamp(8.0, most);
                    let x = start.x + 28.0 + (weeks - 1) as f32 * (cell + 3.0);
                    let y = start.y + 16.0 + 2.0 * (cell + 3.0) + cell / 2.0;
                    target = pos2(if edge { x + 0.5 } else { x + cell / 2.0 }, y);
                    calendar(ui, "t", &days, saturday, touch, &mut picked);
                });
            });
        }
        picked
    }

    #[test]
    fn tapping_a_calendar_day_picks_it() {
        let wednesday = Pick::Day(Date::new(2026, 10, 28).unwrap());
        assert_eq!(tap_calendar(false, true), Some(wednesday));
        assert_eq!(tap_calendar(true, true), Some(wednesday));
    }

    #[test]
    fn clicking_a_calendar_day_on_desktop_picks_it() {
        let wednesday = Pick::Day(Date::new(2026, 10, 28).unwrap());
        assert_eq!(tap_calendar(false, false), Some(wednesday));
    }

    fn desk_frame(
        ctx: &egui::Context,
        state: &mut StatsUi,
        range: &mut Range,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let overview = phone_overview();
        let book = Book::default();
        let view = View {
            book: &book,
            today: Date::new(2026, 10, 31).unwrap(),
        };
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(640.0, 2400.0))),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                overview_page(ui, state, range, &view, &overview);
            });
        })
    }

    fn has_text(output: &egui::FullOutput, wanted: &str) -> bool {
        output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text().trim() == wanted)
        })
    }

    pub(super) fn played_day() -> Picked {
        let game = |name: &str, minutes: u64| crate::stats::PeriodGame {
            rom: name.to_lowercase(),
            name: name.into(),
            play_ms: minutes * 60_000,
            sessions: 1,
        };
        Picked {
            title: "Wednesday 4 Nov".into(),
            period: Period {
                play_ms: 224 * 60_000,
                sessions: 4,
                snapshots: 1,
                games: vec![game("Crystal", 93), game("Tetris", 47)],
                ..Default::default()
            },
            pinned: true,
        }
    }

    pub(super) fn text_y(output: &egui::FullOutput, wanted: &str) -> Option<f32> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
                    Some(text.visual_bounding_rect().center().y)
                }
                _ => None,
            })
    }

    #[test]
    fn a_played_day_lists_its_games_apart_from_the_other_counts() {
        for every_game in [true, false] {
            let ctx = egui::Context::default();
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    period_card(ui, &played_day(), every_game);
                });
            });
            let sessions = text_y(&output, "Sessions").expect("Sessions");
            let across = text_y(&output, ACROSS_GAMES);
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
    fn a_picked_day_fills_the_card_and_its_x_clears_it() {
        let ctx = egui::Context::default();
        let mut state = StatsUi {
            picked: Some(Pick::Day(Date::new(2026, 10, 28).unwrap())),
            ..Default::default()
        };
        let mut range = Range::Calendar;
        desk_frame(&ctx, &mut state, &mut range, Vec::new());
        let output = desk_frame(&ctx, &mut state, &mut range, Vec::new());
        assert!(has_text(&output, "Wednesday 28 Oct"));
        assert!(has_text(&output, "Nothing played."));
        let at = shown_text(&output, icons::X).center();
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        desk_frame(
            &ctx,
            &mut state,
            &mut range,
            vec![egui::Event::PointerMoved(at)],
        );
        desk_frame(&ctx, &mut state, &mut range, vec![press(true)]);
        desk_frame(&ctx, &mut state, &mut range, vec![press(false)]);
        assert_eq!(state.picked, None);
        let output = desk_frame(&ctx, &mut state, &mut range, Vec::new());
        assert!(!has_text(&output, "Wednesday 28 Oct"));
        assert!(!has_text(&output, icons::X));
    }

    fn press_escape(state: &mut StatsUi) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| escape(ui, state));
        });
    }

    #[test]
    fn esc_clears_the_picked_day_before_it_leaves_a_game() {
        let game = Page::Game {
            rom: "tetris".into(),
            name: "Tetris".into(),
        };
        let mut state = StatsUi {
            page: game.clone(),
            picked: Some(Pick::Day(Date::new(2026, 10, 28).unwrap())),
            ..Default::default()
        };
        press_escape(&mut state);
        assert_eq!((state.page.clone(), state.picked), (game, None));
        press_escape(&mut state);
        assert_eq!(state.page, Page::Overview);
    }

    fn content_span(rows: usize) -> (f32, f32) {
        let ctx = egui::Context::default();
        let mut width = 0.0;
        let mut room = 0.0;
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(640.0, 400.0))),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let edge = ui.max_rect().right();
                    scrolled(ui, false, |ui| {
                        for row in 0..rows {
                            ui.label(format!("row {row}"));
                        }
                        width = ui.max_rect().width();
                        room = edge - ui.max_rect().right();
                    });
                });
            });
        }
        (width, room)
    }

    #[test]
    fn the_desktop_page_keeps_one_width_clear_of_the_scroll_bar() {
        let scroll = egui::Style::default().spacing.scroll;
        let bar = scroll.bar_inner_margin + scroll.bar_width;
        let (short, short_room) = content_span(2);
        let (long, long_room) = content_span(200);
        assert!(short > 500.0, "{short}");
        assert!(
            (short - long).abs() < 0.5,
            "{short} without a bar, {long} with"
        );
        for room in [short_room, long_room] {
            assert!(
                room >= bar - 0.5,
                "the content ends {room} pt from the edge, the bar takes {bar}"
            );
        }
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
        let book = Book::default();
        let view = View {
            book: &book,
            today: Date::new(2026, 10, 31).unwrap(),
        };
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
                    phone::overview_page(ui, state, range, &view, &overview);
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
    fn phone_range_choice_fits_one_line_at_360_px() {
        let ctx = egui::Context::default();
        let mut state = StatsUi::default();
        let mut range = Range::Week;
        phone_frame(&ctx, &mut state, &mut range, Vec::new());
        let output = phone_frame(&ctx, &mut state, &mut range, Vec::new());
        let pills: Vec<Rect> = RANGES
            .iter()
            .map(|(_, label)| shown_text(&output, label))
            .collect();
        for pill in &pills {
            assert!((pill.center().y - pills[0].center().y).abs() < 1.0);
            assert!(pill.right() <= 346.0, "{pill:?}");
        }
        tap_text(&ctx, &mut state, &mut range, "Month");
        assert_eq!(range, Range::Month);
    }

    #[test]
    fn the_phone_says_time_played_once_with_the_total_beside_the_period() {
        let ctx = egui::Context::default();
        let mut state = StatsUi::default();
        let mut range = Range::Week;
        phone_frame(&ctx, &mut state, &mut range, Vec::new());
        let output = phone_frame(&ctx, &mut state, &mut range, Vec::new());
        let said = output
            .shapes
            .iter()
            .filter(|clipped| {
                matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text().trim() == "Time played")
            })
            .count();
        assert_eq!(said, 1);
        let period = shown_text(&output, "This week");
        let beside = output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text().trim() == play_time(0) => {
                let total = text.visual_bounding_rect();
                (total.center().y - period.center().y).abs() < 1.0 && total.left() > period.right()
            }
            _ => false,
        });
        assert!(beside, "no total on the line of {period:?}");
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
