use crate::app::mobile::touch_square;
use crate::app::widgets::info_mark::info_mark;
use crate::app::widgets::progress::bar;
use crate::app::widgets::section::{heading, phone_label};
use crate::app::widgets::text::faint;
use citrine_gb_threads::{Kind, Reading, State};
use egui::{Align, Layout, Response, RichText, Ui, Widget, vec2};
use std::collections::HashMap;
use std::time::Duration;
use web_time::Instant;

pub const REFRESH: Duration = Duration::from_millis(500);

#[derive(Default)]
pub struct ThreadsDebugState {
    seen: bool,
    frames: u64,
    main: Duration,
    last: Option<Sample>,
    rows: Vec<Row>,
    main_row: Option<MainRow>,
    measured: bool,
}

struct Sample {
    at: Instant,
    frames: u64,
    main: Duration,
    readings: HashMap<u64, Reading>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MainRow {
    pub busy: f32,
    pub per_frame: Duration,
    pub frames_per_sec: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub reading: Reading,
    pub busy: f32,
    pub per_frame: Duration,
    pub wait: Option<Duration>,
    pub spin: f32,
    pub missed: Option<f32>,
    pub tasks: Vec<&'static str>,
}

impl ThreadsDebugState {
    pub fn mark_seen(&mut self) {
        self.seen = true;
    }

    pub fn take_seen(&mut self) -> bool {
        let seen = self.seen;
        self.seen = false;
        if !seen {
            self.last = None;
            self.rows.clear();
            self.main_row = None;
            self.measured = false;
        }
        seen
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn measured(&self) -> bool {
        self.measured
    }

    pub fn main_row(&self) -> Option<MainRow> {
        self.main_row
    }

    pub fn note_main(&mut self, time: Duration) {
        self.main += time;
    }

    pub fn update(&mut self, now: Instant, readings: impl FnOnce() -> Vec<Reading>) {
        self.frames += 1;
        if let Some(last) = &self.last
            && now.saturating_duration_since(last.at) < REFRESH
        {
            return;
        }
        let readings = readings();
        if let Some(last) = &self.last {
            let window = now.saturating_duration_since(last.at);
            let frames = self.frames - last.frames;
            self.main_row = Some(main_rate(self.main - last.main, window, frames));
            self.rows = readings
                .iter()
                .map(|reading| rate(reading, last.readings.get(&reading.id), window, frames))
                .collect();
            self.measured = true;
        } else {
            self.rows = readings
                .iter()
                .map(|reading| rate(reading, Some(reading), Duration::ZERO, 0))
                .collect();
        }
        self.last = Some(Sample {
            at: now,
            frames: self.frames,
            main: self.main,
            readings: readings
                .into_iter()
                .map(|reading| (reading.id, reading))
                .collect(),
        });
    }
}

pub fn main_rate(busy: Duration, window: Duration, frames: u64) -> MainRow {
    let secs = window.as_secs_f32();
    MainRow {
        busy: if secs > 0.0 {
            (busy.as_secs_f32() / secs).clamp(0.0, 1.0)
        } else {
            0.0
        },
        per_frame: if frames == 0 {
            Duration::ZERO
        } else {
            busy / u32::try_from(frames).unwrap_or(u32::MAX)
        },
        frames_per_sec: if secs > 0.0 {
            frames as f32 / secs
        } else {
            0.0
        },
    }
}

pub fn rate(now: &Reading, before: Option<&Reading>, window: Duration, frames: u64) -> Row {
    let zero = Reading {
        sent: 0,
        handled: 0,
        parks: 0,
        busy: Duration::ZERO,
        waited: Duration::ZERO,
        waits: 0,
        spun: Duration::ZERO,
        rounds: 0,
        missed: 0,
        tasks: Vec::new(),
        ..now.clone()
    };
    let before = before.unwrap_or(&zero);
    let busy = now.busy.saturating_sub(before.busy);
    let spun = now.spun.saturating_sub(before.spun);
    let share = |part: Duration| {
        if window.is_zero() {
            0.0
        } else {
            (part.as_secs_f32() / window.as_secs_f32()).clamp(0.0, 1.0)
        }
    };
    let waits = now.waits.saturating_sub(before.waits);
    let rounds = now.rounds.saturating_sub(before.rounds);
    Row {
        busy: share(busy),
        per_frame: if frames == 0 {
            Duration::ZERO
        } else {
            busy / u32::try_from(frames).unwrap_or(u32::MAX)
        },
        wait: (waits > 0).then(|| {
            now.waited.saturating_sub(before.waited) / u32::try_from(waits).unwrap_or(u32::MAX)
        }),
        spin: share(spun),
        missed: (rounds > 0)
            .then(|| now.missed.saturating_sub(before.missed) as f32 / rounds as f32),
        tasks: busiest(now, before),
        reading: now.clone(),
    }
}

fn busiest(now: &Reading, before: &Reading) -> Vec<&'static str> {
    let mut spent: Vec<(&'static str, Duration)> = now
        .tasks
        .iter()
        .map(|&(name, busy)| {
            let earlier = before
                .tasks
                .iter()
                .find(|(other, _)| *other == name)
                .map_or(Duration::ZERO, |(_, busy)| *busy);
            (name, busy.saturating_sub(earlier))
        })
        .filter(|(_, busy)| !busy.is_zero())
        .collect();
    spent.sort_by_key(|&(_, busy)| std::cmp::Reverse(busy));
    spent.into_iter().map(|(name, _)| name).collect()
}

pub struct Line {
    pub title: String,
    pub runs: String,
    pub busy: Option<f32>,
    pub facts: String,
}

pub struct Group {
    pub icon: &'static str,
    pub title: &'static str,
    pub hint: &'static str,
    pub lines: Vec<Line>,
}

pub struct ThreadsView {
    pub summary: String,
    pub groups: Vec<Group>,
}

pub const SUMMARY_HINT: &str = "Each row is one thread. Busy is the share of one core it used over the last half second; the time is how long it worked for each frame the app drew. Waiting counts messages it has not started yet. Parked threads sleep until a message wakes them; spinning threads keep checking so a linked console can go on at once.";

const NAME_SHARE: f32 = 0.32;
const PERCENT_WIDTH: f32 = 36.0;
const ROW_GAP: f32 = 6.0;
const PHONE_INDENT: i8 = 6;

impl Widget for ThreadsView {
    fn ui(self, ui: &mut Ui) -> Response {
        let phone = touch_square(ui) > 0.0;
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::Label::new(RichText::new(&self.summary).strong()).wrap());
                info_mark(ui, SUMMARY_HINT);
            });
            for group in self.groups {
                if group.lines.is_empty() {
                    continue;
                }
                group_heading(ui, phone, &group);
                let indent = if phone { PHONE_INDENT } else { 0 };
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(indent, 0))
                    .show(ui, |ui| {
                        let name_width = (ui.available_width() * NAME_SHARE).clamp(64.0, 160.0);
                        for (i, line) in group.lines.into_iter().enumerate() {
                            if i > 0 {
                                ui.add(egui::Separator::default().spacing(ROW_GAP));
                            }
                            line_ui(ui, line, name_width);
                        }
                    });
            }
        })
        .response
    }
}

fn group_heading(ui: &mut Ui, phone: bool, group: &Group) {
    if phone {
        ui.add_space(6.0);
        phone_label(ui, group.title, Some(group.hint));
        return;
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        heading(ui, group.icon, group.title);
        info_mark(ui, group.hint);
    });
    ui.add(egui::Separator::default().spacing(4.0));
}

fn line_ui(ui: &mut Ui, line: Line, name_width: f32) {
    let height = ui.text_style_height(&egui::TextStyle::Body);
    let row = vec2(ui.available_width(), height);
    ui.allocate_ui_with_layout(row, Layout::left_to_right(Align::Center), |ui| {
        ui.allocate_ui_with_layout(
            vec2(name_width, height),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_width(name_width);
                ui.add(egui::Label::new(RichText::new(&line.title).strong()).truncate());
            },
        );
        if let Some(busy) = line.busy {
            let spacing = ui.spacing().item_spacing.x;
            let bar_width = (ui.available_width() - PERCENT_WIDTH - spacing).max(0.0);
            ui.allocate_ui_with_layout(
                vec2(bar_width, height),
                Layout::left_to_right(Align::Center),
                |ui| {
                    bar(ui, Some(busy));
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(percent(busy));
            });
        }
    });
    let details = [line.runs.as_str(), line.facts.as_str()]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    ui.add(egui::Label::new(faint(details)).wrap());
}

pub fn percent(share: f32) -> String {
    format!("{:.0}%", share * 100.0)
}

pub fn millis(time: Duration) -> String {
    let micros = (time.as_secs_f64() * 1_000_000.0).round();
    let tenths = (micros / 100.0).round();
    if micros < 100.0 {
        format!("{micros:.0} µs")
    } else if tenths >= 100.0 {
        format!("{:.0} ms", (micros / 1000.0).round())
    } else {
        format!("{:.1} ms", tenths / 10.0)
    }
}

pub fn state_word(row: &Row) -> &'static str {
    if row.reading.inline {
        return "runs on the main thread";
    }
    match row.reading.state {
        State::Starting => "starting",
        State::Working => "working",
        State::Parked => "parked",
        State::Spinning => "spinning",
        State::Ended => "stopped",
    }
}

pub fn facts(row: &Row, measured: bool) -> String {
    if !measured {
        return format!("Measuring, {}", state_word(row));
    }
    let mut parts = vec![format!("{} a frame", millis(row.per_frame))];
    match row.reading.kind {
        Kind::Worker => {
            parts.push(format!("{} waiting", row.reading.queued()));
            if let Some(wait) = row.wait {
                parts.push(format!("starts after {}", millis(wait)));
            }
        }
        Kind::Pool | Kind::Background | Kind::Side => {}
    }
    if row.spin > 0.0 {
        parts.push(format!("spins {}", percent(row.spin)));
    }
    if let Some(missed) = row.missed {
        parts.push(format!("{} of helper turns missed", percent(missed)));
    }
    parts.push(state_word(row).to_string());
    let mut text = parts.join(", ");
    if let Some(first) = text.get(..1) {
        text = first.to_uppercase() + &text[1..];
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE_MAX: f32 = 370.0;
    const DESKTOP_MAX: f32 = 260.0;

    fn reading(id: u64, busy_ms: u64, handled: u64) -> Reading {
        Reading {
            id,
            name: "console".into(),
            kind: Kind::Worker,
            state: State::Parked,
            inline: false,
            sent: handled + 2,
            handled,
            parks: 0,
            busy: Duration::from_millis(busy_ms),
            waited: Duration::from_micros(40 * handled),
            waits: handled,
            spun: Duration::ZERO,
            rounds: 0,
            missed: 0,
            tasks: Vec::new(),
        }
    }

    #[test]
    fn a_helper_names_the_tasks_it_ran_in_the_window_busiest_first() {
        let pool = |tasks: Vec<(&'static str, u64)>| Reading {
            kind: Kind::Pool,
            tasks: tasks
                .into_iter()
                .map(|(name, ms)| (name, Duration::from_millis(ms)))
                .collect(),
            ..reading(2, 0, 0)
        };
        let row = rate(
            &pool(vec![("3d", 40), ("2d lines", 90), ("snapshot", 5)]),
            Some(&pool(vec![("3d", 10), ("2d lines", 80), ("snapshot", 5)])),
            Duration::from_millis(500),
            30,
        );
        assert_eq!(row.tasks, ["3d", "2d lines"]);
    }

    #[test]
    fn rates_come_from_the_change_over_the_window() {
        let row = rate(
            &reading(1, 300, 60),
            Some(&reading(1, 100, 30)),
            Duration::from_millis(500),
            30,
        );
        assert!((row.busy - 0.4).abs() < 1e-4);
        assert_eq!(row.per_frame, Duration::from_millis(200) / 30);
        assert_eq!(row.wait, Some(Duration::from_micros(40)));
        assert_eq!(row.missed, None);
        assert_eq!(
            facts(&row, true),
            "6.7 ms a frame, 2 waiting, starts after 40 µs, parked"
        );
    }

    #[test]
    fn a_new_thread_counts_from_zero() {
        let row = rate(&reading(7, 50, 5), None, Duration::from_millis(500), 30);
        assert!((row.busy - 0.1).abs() < 1e-4);
    }

    #[test]
    fn the_first_half_second_says_it_is_measuring() {
        let mut state = ThreadsDebugState::default();
        let start = Instant::now();
        state.update(start, || vec![reading(1, 0, 0)]);
        assert!(!state.measured());
        assert_eq!(facts(&state.rows()[0], false), "Measuring, parked");
        state.update(start + Duration::from_millis(100), || {
            panic!("no reading before the refresh")
        });
        state.update(start + REFRESH, || vec![reading(1, 250, 30)]);
        assert!(state.measured());
        assert!((state.rows()[0].busy - 0.5).abs() < 1e-4);
        assert_eq!(state.rows()[0].per_frame, Duration::from_millis(125));
    }

    #[test]
    fn main_reads_its_own_frames_over_the_window() {
        let mut state = ThreadsDebugState::default();
        let start = Instant::now();
        state.note_main(Duration::from_secs(2));
        state.update(start, Vec::new);
        assert_eq!(state.main_row(), None);
        for _ in 0..30 {
            state.note_main(Duration::from_millis(5));
            state.update(start + Duration::from_millis(10), || panic!("too soon"));
        }
        state.update(start + REFRESH, Vec::new);
        let main = state.main_row().unwrap();
        assert!((main.busy - 0.3).abs() < 1e-4);
        assert_eq!(main.per_frame, Duration::from_millis(150) / 31);
        assert!((main.frames_per_sec - 62.0).abs() < 1e-3);
    }

    #[test]
    fn closing_the_pane_forgets_the_last_sample() {
        let mut state = ThreadsDebugState::default();
        state.mark_seen();
        assert!(state.take_seen());
        state.update(Instant::now(), || vec![reading(1, 0, 0)]);
        assert!(!state.take_seen());
        assert!(state.rows().is_empty());
    }

    fn standard_view() -> ThreadsView {
        let line = |title: &str, runs: &str, busy: f32, facts: &str| Line {
            title: title.into(),
            runs: runs.into(),
            busy: Some(busy),
            facts: facts.into(),
        };
        let group = |title: &'static str, lines: Vec<Line>| Group {
            icon: "",
            title,
            hint: "hint",
            lines,
        };
        ThreadsView {
            summary: "4 threads on 2 cores".into(),
            groups: vec![
                group(
                    "App",
                    vec![line(
                        "Main",
                        "Screen, input and the app",
                        0.53,
                        "21 ms a frame, 25 frames a second",
                    )],
                ),
                group(
                    "Games",
                    vec![
                        line(
                            "Console",
                            "Pentris",
                            0.25,
                            "10 ms a frame, 0 waiting, starts after 69 µs, parked",
                        ),
                        line(
                            "Helper 1",
                            "Linked consoles, side by side",
                            0.0,
                            "0 µs a frame, parked",
                        ),
                    ],
                ),
                group(
                    "Background",
                    vec![line(
                        "Saves",
                        "Writes saves and snapshots",
                        0.0,
                        "0 µs a frame, 0 waiting, parked",
                    )],
                ),
            ],
        }
    }

    fn pane_height(touch: bool, width: f32) -> f32 {
        let ctx = egui::Context::default();
        crate::app::Citrine::setup_fonts(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 2000.0),
            )),
            ..Default::default()
        };
        let mut height = 0.0;
        for _ in 0..3 {
            let _ = ctx.run(input.clone(), |ctx| {
                crate::app::mobile::touch_spacing(ctx, touch);
                egui::CentralPanel::default().show(ctx, |ui| {
                    height = ui.add(standard_view()).rect.height();
                });
            });
        }
        height
    }

    #[test]
    fn the_phone_pane_fits_the_standard_rows_in_little_height() {
        let phone = pane_height(true, 412.0);
        let desktop = pane_height(false, 380.0);
        println!("threads pane height: phone {phone:.0} pt, desktop {desktop:.0} pt");
        assert!(phone < PHONE_MAX, "phone pane is {phone} pt");
        assert!(desktop < DESKTOP_MAX, "desktop pane is {desktop} pt");
    }

    #[test]
    fn times_read_in_words_with_their_unit() {
        assert_eq!(millis(Duration::from_micros(42)), "42 µs");
        assert_eq!(millis(Duration::from_micros(2140)), "2.1 ms");
        assert_eq!(millis(Duration::from_micros(9960)), "10 ms");
        assert_eq!(millis(Duration::from_micros(9940)), "9.9 ms");
        assert_eq!(millis(Duration::from_nanos(99_700)), "0.1 ms");
        assert_eq!(millis(Duration::from_micros(10_500)), "11 ms");
        assert_eq!(millis(Duration::from_millis(17)), "17 ms");
        assert_eq!(percent(0.42), "42%");
    }
}
