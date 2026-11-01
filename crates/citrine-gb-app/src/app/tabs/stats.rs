use crate::app::events::AppEventQueue;
use crate::app::tabs::{Tab, TabViewer};
use crate::icons;
use crate::stats::{GameTotals, Overview, Progress, day_key, parse_day};
use egui::{Color32, CornerRadius, Rect, RichText, Sense, Ui, pos2, vec2};
use jiff::civil::Date;
use std::collections::BTreeMap;

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

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn last_played(unix: u64, today: Date) -> String {
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
    let mobile = crate::app::mobile_layout(viewer.ui.settings.layout, ui.ctx());
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
        .show(ui, |ui| {
            ui.add_space(4.0);
            if overview.games.is_empty() {
                empty(ui, events);
                return;
            }
            match state.page.clone() {
                Page::Game { rom, .. } => {
                    if let Some(game) = overview.game(&rom) {
                        game_page(ui, state, &mut range, game, today, mobile);
                    }
                }
                Page::Overview => overview_page(ui, state, &mut range, &overview, today),
            }
            ui.add_space(if mobile { 12.0 } else { 4.0 });
        });
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
            "Time with a linked game",
            overview.link_ms,
            play_time(overview.link_ms),
        ),
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

const ACCENT: Color32 = Color32::from_rgb(30, 136, 200);

fn toward_panel(ui: &Ui, color: Color32, t: f32) -> Color32 {
    color.lerp_to_gamma(ui.visuals().panel_fill, t)
}

fn empty_fill(ui: &Ui) -> Color32 {
    toward_panel(ui, ui.visuals().text_color(), 0.88)
}

struct Bar {
    label: String,
    tip: String,
    ms: u64,
    future: bool,
}

fn day_bar(days: &BTreeMap<String, u64>, day: Date, label: String, today: Date) -> Bar {
    let ms = days.get(&day_key(day)).copied().unwrap_or(0);
    Bar {
        label,
        tip: format!("{}: {}", day.strftime("%a %-d %b"), play_time(ms)),
        ms,
        future: day > today,
    }
}

fn day_bars(days: &BTreeMap<String, u64>, today: Date, range: Range) -> Vec<Bar> {
    match range {
        Range::Week => {
            let monday = crate::stats::week_start(today);
            (0..7)
                .filter_map(|n| monday.checked_add(jiff::Span::new().days(n)).ok())
                .map(|day| day_bar(days, day, day.strftime("%a").to_string(), today))
                .collect()
        }
        Range::Month => (1..=today.days_in_month())
            .filter_map(|n| today.with().day(n).build().ok())
            .map(|day| day_bar(days, day, day.day().to_string(), today))
            .collect(),
        Range::Year | Range::Calendar => (1..=12)
            .filter_map(|n| Date::new(today.year(), n, 1).ok())
            .map(|month| {
                let ms = days
                    .iter()
                    .filter_map(|(key, ms)| parse_day(key).map(|d| (d, *ms)))
                    .filter(|(d, _)| d.year() == month.year() && d.month() == month.month())
                    .map(|(_, ms)| ms)
                    .sum();
                Bar {
                    label: month.strftime("%b").to_string().chars().take(1).collect(),
                    tip: format!("{}: {}", month.strftime("%B"), play_time(ms)),
                    ms,
                    future: month.month() > today.month(),
                }
            })
            .collect(),
    }
}

fn readout(ui: &mut Ui, text: String) {
    let height = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add(egui::Label::new(text).truncate());
        },
    );
}

fn bars(ui: &mut Ui, id: &str, days: &BTreeMap<String, u64>, today: Date, range: Range) {
    let bars = day_bars(days, today, range);
    let max = scale_top(bars.iter().map(|b| b.ms).max().unwrap_or(0));
    let picked_id = ui.id().with((id, "picked", range.label()));
    let mut picked: Option<usize> = ui.data(|d| d.get_temp(picked_id)).flatten();
    let label_height = 16.0;
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), CHART_HEIGHT + label_height),
        Sense::hover(),
    );
    let plot = Rect::from_min_max(
        rect.min + vec2(0.0, 22.0),
        pos2(rect.max.x, rect.max.y - label_height),
    );
    let painter = ui.painter_at(rect);
    let grid = ui.visuals().widgets.noninteractive.bg_stroke;
    painter.line_segment([plot.left_top(), plot.right_top()], grid);
    painter.line_segment([plot.left_bottom(), plot.right_bottom()], grid);
    painter.text(
        plot.left_top() - vec2(0.0, 2.0),
        egui::Align2::LEFT_BOTTOM,
        short_time(max),
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
    let slot = plot.width() / bars.len() as f32;
    let gap = (slot * 0.25).clamp(2.0, 8.0);
    let color = ACCENT;
    let mut hovered = None;
    for (i, bar) in bars.iter().enumerate() {
        let x = plot.left() + slot * i as f32;
        let height = (bar.ms as f32 / max as f32) * plot.height();
        let lit = if bar.future {
            false
        } else {
            let column = Rect::from_min_max(pos2(x, plot.top()), pos2(x + slot, rect.bottom()));
            let response = ui.interact(column, ui.id().with((id, i)), Sense::click());
            if response.clicked() {
                picked = if picked == Some(i) { None } else { Some(i) };
            }
            if response.hovered() {
                hovered = Some(i);
            }
            response.hovered() || picked == Some(i)
        };
        if bar.ms > 0 {
            let shape = Rect::from_min_max(
                pos2(x + gap / 2.0, plot.bottom() - height.max(2.0)),
                pos2(x + slot - gap / 2.0, plot.bottom()),
            );
            let fill = if lit {
                color.lerp_to_gamma(ui.visuals().strong_text_color(), 0.25)
            } else {
                color
            };
            painter.rect_filled(
                shape,
                CornerRadius {
                    nw: 3,
                    ne: 3,
                    sw: 0,
                    se: 0,
                },
                fill,
            );
        }
        let labelled = range != Range::Month || i == 0 || (i + 1) % 5 == 0;
        if labelled || lit {
            painter.text(
                pos2(x + slot / 2.0, rect.bottom()),
                egui::Align2::CENTER_BOTTOM,
                &bar.label,
                egui::FontId::proportional(11.0),
                if lit {
                    ui.visuals().strong_text_color()
                } else if bar.future {
                    toward_panel(ui, ui.visuals().weak_text_color(), 0.6)
                } else {
                    ui.visuals().weak_text_color()
                },
            );
        }
    }
    ui.data_mut(|d| d.insert_temp(picked_id, picked));
    let text = match hovered.or(picked) {
        Some(i) => bars[i].tip.clone(),
        None => format!(
            "{}: {}",
            range.total_label(),
            play_time(bars.iter().map(|b| b.ms).sum())
        ),
    };
    readout(ui, text);
}

const LEVELS: [u64; 3] = [30 * 60_000, 60 * 60_000, 120 * 60_000];

fn level_fill(ui: &Ui, ms: u64) -> Color32 {
    if ms == 0 {
        return empty_fill(ui);
    }
    let level = LEVELS.iter().filter(|&&edge| ms >= edge).count();
    let steps = if ui.visuals().dark_mode {
        [0.45, 0.3, 0.15, 0.0]
    } else {
        [0.6, 0.4, 0.2, 0.0]
    };
    toward_panel(ui, ACCENT, steps[level])
}

fn level_cell(ui: &Ui, painter: &egui::Painter, square: Rect, ms: u64, lit: bool) {
    painter.rect_filled(square, CornerRadius::same(2), level_fill(ui, ms));
    if !lit {
        return;
    }
    let stroke = egui::Stroke::new(1.5_f32, ui.visuals().strong_text_color());
    painter.rect_stroke(
        square,
        CornerRadius::same(2),
        stroke,
        egui::StrokeKind::Inside,
    );
}

fn calendar(ui: &mut Ui, id: &str, days: &BTreeMap<String, u64>, today: Date, touch: bool) {
    let gap = 3.0;
    let left = 28.0;
    let room = ui.available_width() - left;
    let weeks = if touch {
        ((room / (TOUCH_CELL + gap)).floor() as usize).max(1)
    } else if ui.available_width() >= WIDE {
        26
    } else {
        17
    };
    let cell = (room / weeks as f32 - gap).clamp(8.0, if touch { f32::MAX } else { 18.0 });
    let size = vec2(
        left + weeks as f32 * (cell + gap),
        7.0 * (cell + gap) + 16.0 + 6.0 + cell,
    );
    let picked_id = ui.id().with((id, "picked_day"));
    let mut picked: Option<Date> = ui.data(|d| d.get_temp(picked_id)).flatten();
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    let start = crate::stats::week_start(today)
        .checked_sub(jiff::Span::new().weeks(weeks as i64 - 1))
        .unwrap_or(today);
    for (row, name) in ["Mon", "", "Wed", "", "Fri", "", "Sun"].iter().enumerate() {
        painter.text(
            pos2(
                rect.left(),
                rect.top() + 16.0 + row as f32 * (cell + gap) + cell / 2.0,
            ),
            egui::Align2::LEFT_CENTER,
            *name,
            egui::FontId::proportional(10.0),
            ui.visuals().weak_text_color(),
        );
    }
    let mut hovered = None;
    let mut total = 0;
    for week in 0..weeks {
        for weekday in 0..7 {
            let Ok(day) = start.checked_add(jiff::Span::new().days(week as i64 * 7 + weekday))
            else {
                continue;
            };
            if day > today {
                continue;
            }
            let x = rect.left() + left + week as f32 * (cell + gap);
            let y = rect.top() + 16.0 + weekday as f32 * (cell + gap);
            if day.day() <= 7 && weekday == 0 {
                painter.text(
                    pos2(x, rect.top()),
                    egui::Align2::LEFT_TOP,
                    day.strftime("%b").to_string(),
                    egui::FontId::proportional(10.0),
                    ui.visuals().weak_text_color(),
                );
            }
            let ms = days.get(&day_key(day)).copied().unwrap_or(0);
            total += ms;
            let square = Rect::from_min_size(pos2(x, y), vec2(cell, cell));
            let response = ui.interact(
                square.expand(gap / 2.0),
                ui.id().with((id, week, weekday)),
                Sense::click(),
            );
            if response.clicked() {
                picked = if picked == Some(day) { None } else { Some(day) };
            }
            if response.hovered() {
                hovered = Some((day, ms));
            }
            level_cell(
                ui,
                &painter,
                square,
                ms,
                response.hovered() || picked == Some(day),
            );
        }
    }
    ui.data_mut(|d| d.insert_temp(picked_id, picked));
    legend(
        ui,
        &painter,
        pos2(rect.left(), rect.top() + 16.0 + 7.0 * (cell + gap) + 6.0),
        cell,
    );
    let text = match hovered.or(picked.map(|d| (d, days.get(&day_key(d)).copied().unwrap_or(0)))) {
        Some((day, ms)) => format!("{}: {}", day.strftime("%a %-d %b"), play_time(ms)),
        None => format!("Last {weeks} weeks: {}", play_time(total)),
    };
    readout(ui, text);
}

fn legend(ui: &Ui, painter: &egui::Painter, top_left: egui::Pos2, side: f32) {
    let font = egui::FontId::proportional(11.0);
    let color = ui.visuals().weak_text_color();
    let middle = top_left.y + side / 2.0;
    let text = painter.text(
        pos2(top_left.x, middle),
        egui::Align2::LEFT_CENTER,
        "0 h",
        font.clone(),
        color,
    );
    let mut x = text.right() + 4.0;
    for ms in [0, 1, LEVELS[0], LEVELS[1], LEVELS[2]] {
        let square = Rect::from_min_size(pos2(x, top_left.y), vec2(side, side));
        level_cell(ui, painter, square, ms, false);
        x += side + 3.0;
    }
    painter.text(
        pos2(x + 1.0, middle),
        egui::Align2::LEFT_CENTER,
        "2 h or more",
        font,
        color,
    );
}

fn sorted<'a>(state: &StatsUi, overview: &'a Overview) -> Vec<&'a GameTotals> {
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

fn games_list(ui: &mut Ui, state: &mut StatsUi, overview: &Overview, today: Date) {
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
            egui::TextEdit::singleline(&mut state.search)
                .hint_text(format!("{}  Find a game", icons::MAGNIFYING_GLASS))
                .min_size(vec2(0.0, ui.spacing().interact_size.y))
                .vertical_align(egui::Align::Center)
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

fn game_row(ui: &mut Ui, game: &GameTotals, top: u64, today: Date) -> bool {
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

fn game_tiles(ui: &mut Ui, game: &GameTotals) {
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

fn game_facts(ui: &mut Ui, game: &GameTotals, today: Date) {
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
                "Time with a linked game",
                game.link_ms,
                play_time(game.link_ms),
            ),
            (
                "Time online with friends",
                game.together_ms,
                play_time(game.together_ms),
            ),
        ],
    );
}

fn game_page(
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
    pokemon(ui, game);
}

fn pokemon(ui: &mut Ui, game: &GameTotals) {
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

fn progress_chart(ui: &mut Ui, progress: &BTreeMap<String, Progress>) {
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

fn span_days(first: Date, last: Date) -> f32 {
    (last - first).get_days().max(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn week_bars_run_monday_to_sunday() {
        let saturday = Date::new(2026, 10, 31).unwrap();
        let labels: Vec<String> = day_bars(&BTreeMap::new(), saturday, Range::Week)
            .into_iter()
            .map(|bar| bar.label)
            .collect();
        assert_eq!(labels, ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
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
}
