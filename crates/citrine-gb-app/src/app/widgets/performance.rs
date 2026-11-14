use crate::app::tabs::ds_files::explorer::thousands;
use crate::app::ui_state::settings::{MATCH_SCREEN_LABEL, match_screen_line};
use crate::app::widgets::action::wide_button;
use crate::app::widgets::frame_graph::FrameGraph;
use crate::app::widgets::info_mark::{info_mark, info_mark_width};
use crate::app::widgets::section::{section, section_note, section_with_hint};
use crate::app::widgets::text::faint;
use crate::app::widgets::threads_debug::{REFRESH, millis};
use crate::emulator::load::{GpuClockKind, LoadReading, ScreenMatch};
use crate::frame_log::readout::{WINDOW, rate_gap};
use crate::frame_log::{Budget, FrameSample, Readout};
use crate::icons;
use egui::{
    Align, Button, CollapsingHeader, CornerRadius, Grid, Label, Layout, Response, RichText, Sense,
    Stroke, Ui, Vec2, Widget, pos2, vec2,
};
use std::collections::VecDeque;
use std::time::Duration;
use web_time::Instant;

const COLUMN_GAP: f32 = 12.0;
const LABEL_WIDTH: f32 = 66.0;

pub const GAME_HINT: &str = "Time the console thread spends running one game frame. The mark is one game frame, the time until the next one is due.";
pub const APP_HINT: &str = "Time the main thread spends on each screen refresh: reading input and drawing the game and the app's panels. The mark is one screen refresh.";
pub const GPU_STAMPS_HINT: &str = "Time the GPU spent each frame drawing the 3D scene at the chosen resolution and fitting it into the screen, read from the GPU's own clock. The GPU draws 3D only above Native resolution, once per game frame, so the mark is one game frame.";
pub const GPU_SUBMIT_HINT: &str = "Time from handing each frame's 3D scene to the GPU until the app saw it finished, at the chosen resolution. This GPU has no clock the app can read, so the time includes waiting behind other drawing and can run up to one screen refresh long. The GPU draws 3D only above Native resolution. The GPU draws the 3D once per game frame, so the mark is one game frame.";
pub const JIT_HINT: &str = "The JIT turns the game's ARM code into code for this device's CPU, in blocks of up to 128 instructions. Compiled is the share of instructions in the last half second that ran from those blocks; the rest ran one at a time. Compiling is the time spent making blocks for each second of play.";
pub const RECORD_HINT: &str = "Records the times of every frame, with the game and its 3D and JIT settings, to a CSV file you can share.";
#[cfg(target_os = "android")]
pub const STOP_RECORDING: &str = "Stop and share";
#[cfg(not(target_os = "android"))]
pub const STOP_RECORDING: &str = "Stop and save";

pub const PAUSED: &str = "Game paused";
pub const BEFORE_PAUSE: &str = "From before the game paused";

#[derive(Default)]
pub struct PerformanceState {
    seen: bool,
    measured: bool,
    last: Option<Sample>,
    ran: Option<LoadView>,
    view: LoadView,
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    at: Instant,
    reading: LoadReading,
}

#[derive(Debug, Clone, Copy)]
pub struct Window {
    #[cfg(feature = "nds")]
    pub time: Duration,
    pub before: LoadReading,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LoadView {
    pub ds: bool,
    pub gpu: GpuView,
    pub jit: Option<JitView>,
    pub paused: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum GpuView {
    #[default]
    Native,
    Measuring(Option<GpuClockKind>),
    Idle(GpuClockKind),
    Timed(GpuClockKind, Duration),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JitView {
    pub blocks: u64,
    pub threads: usize,
    pub compiled: Option<f32>,
    pub compiling: Option<Duration>,
}

impl PerformanceState {
    pub fn mark_seen(&mut self) {
        self.seen = true;
    }

    pub fn take_seen(&mut self) -> bool {
        let seen = std::mem::take(&mut self.seen);
        if !seen {
            self.measured = false;
        }
        seen
    }

    pub fn measuring_before_hold(&self) -> bool {
        !self.measured && self.last.is_some_and(|last| last.reading.ds)
    }

    pub fn view(&self) -> LoadView {
        self.view
    }

    pub fn update(&mut self, now: Instant, running: bool, reading: LoadReading) {
        if let Some(last) = self.last
            && now.saturating_duration_since(last.at) < REFRESH
        {
            return;
        }
        let window = self.last.map(|last| Window {
            #[cfg(feature = "nds")]
            time: now.saturating_duration_since(last.at),
            before: last.reading,
        });
        self.last = Some(Sample { at: now, reading });
        if !reading.ds {
            self.ran = None;
        }
        if running {
            self.measured |= window.is_some();
            let view = load_view(reading, window);
            if window.is_some() {
                self.ran = Some(view);
            }
            self.view = view;
        } else {
            self.view = paused_view(reading, self.ran);
        }
    }
}

pub fn load_view(now: LoadReading, window: Option<Window>) -> LoadView {
    let gpu = match (now.upscaling, now.gpu.kind, window) {
        (false, _, _) => GpuView::Native,
        (true, kind, None) | (true, kind @ None, _) => GpuView::Measuring(kind),
        (true, Some(kind), Some(window)) => {
            let frames = now.gpu.frames.saturating_sub(window.before.gpu.frames);
            let time = now.gpu.time.saturating_sub(window.before.gpu.time);
            match u32::try_from(frames) {
                Ok(0) => GpuView::Idle(kind),
                Ok(frames) => GpuView::Timed(kind, time / frames),
                Err(_) => GpuView::Idle(kind),
            }
        }
    };
    LoadView {
        ds: now.ds,
        gpu,
        jit: jit_view(now, window),
        paused: false,
    }
}

pub fn paused_view(now: LoadReading, ran: Option<LoadView>) -> LoadView {
    let fresh = load_view(now, None);
    let gpu = match (fresh.gpu, ran.map(|ran| ran.gpu)) {
        (GpuView::Native, _) => GpuView::Native,
        (_, Some(gpu @ (GpuView::Idle(_) | GpuView::Timed(..)))) => gpu,
        (gpu, _) => gpu,
    };
    let jit = fresh.jit.map(|jit| JitView {
        blocks: jit.blocks,
        threads: jit.threads,
        ..ran.and_then(|ran| ran.jit).unwrap_or(jit)
    });
    LoadView {
        ds: now.ds,
        gpu,
        jit,
        paused: true,
    }
}

#[cfg(feature = "nds")]
fn jit_view(now: LoadReading, window: Option<Window>) -> Option<JitView> {
    let counts = now.jit?;
    let before = window.map(|window| (window.time, window.before.jit.unwrap_or_default()));
    Some(JitView {
        blocks: counts.blocks,
        threads: counts.threads,
        compiled: before.and_then(|(_, before)| {
            let compiled = counts.compiled.saturating_sub(before.compiled);
            let stepped = counts.stepped.saturating_sub(before.stepped);
            let all = compiled + stepped;
            (all > 0).then(|| compiled as f32 / all as f32)
        }),
        compiling: before
            .filter(|(time, _)| !time.is_zero())
            .map(|(time, before)| {
                counts
                    .compile_time
                    .saturating_sub(before.compile_time)
                    .div_f64(time.as_secs_f64())
            }),
    })
}

#[cfg(not(feature = "nds"))]
fn jit_view(_: LoadReading, _: Option<Window>) -> Option<JitView> {
    None
}

pub fn budget_text(budget: Duration) -> String {
    format!("{:.1} ms", budget.as_secs_f64() * 1000.0)
}

pub const GAME_FRAMES_LINE: &str = "game frames a second";
pub const SMOOTH: &str = "Smooth";
pub const SLOW: &str = "Below full speed";
pub const MEASURING: &str = "Measuring";
pub const REPEATED_HINT: &str = "A frame that stays on the screen one refresh longer because the next one wasn't ready in time. When the screen's rate isn't a whole multiple of the game's, some frames also stay one refresh longer. Those aren't late. The graph marks them as repeats, but this count leaves them out.";
const SMOOTH_SHARE: f64 = 0.99;
const BAR_SCALE: f64 = 1.6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Time(Duration),
    Text(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub label: &'static str,
    pub cause: &'static str,
    pub hint: &'static str,
    pub value: Value,
    pub allowed: Duration,
}

impl Part {
    fn over_by(&self) -> Option<f64> {
        match self.value {
            Value::Time(time) if time > self.allowed && !self.allowed.is_zero() => {
                Some(time.as_secs_f64() / self.allowed.as_secs_f64())
            }
            _ => None,
        }
    }

    pub fn over(&self) -> bool {
        self.over_by().is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Paused,
    Smooth,
    Late(u32),
    Slow,
    HeldBack(&'static str),
}

impl Verdict {
    pub fn text(self) -> String {
        match self {
            Self::Paused => PAUSED.to_string(),
            Self::Smooth => SMOOTH.to_string(),
            Self::Late(late) => format!(
                "{late} frame{} late in the last {} seconds",
                if late == 1 { "" } else { "s" },
                WINDOW.as_secs()
            ),
            Self::Slow => SLOW.to_string(),
            Self::HeldBack(cause) => format!("Held back by the {cause}"),
        }
    }
}

pub fn verdict(readout: Option<&Readout>, parts: &[Part], paused: bool) -> Option<Verdict> {
    if paused {
        return Some(Verdict::Paused);
    }
    let readout = readout?;
    if readout.late == 0 && readout.fps >= readout.rate * SMOOTH_SHARE {
        return Some(Verdict::Smooth);
    }
    let slowest = parts
        .iter()
        .filter_map(|part| part.over_by().map(|by| (part.cause, by)))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    Some(match slowest {
        Some((cause, _)) => Verdict::HeldBack(cause),
        None if readout.late > 0 => Verdict::Late(readout.late),
        None => Verdict::Slow,
    })
}

pub fn headline_text(readout: Option<&Readout>) -> String {
    match readout {
        Some(readout) => format!(
            "{:.1} of {:.1}",
            readout.fps.min(readout.rate),
            readout.rate
        ),
        None => MEASURING.to_string(),
    }
}

pub fn screen_match_text(status: ScreenMatch) -> String {
    match status {
        ScreenMatch::Off => "Off".to_string(),
        ScreenMatch::Shared => "Off while playing with others".to_string(),
        ScreenMatch::Measuring => MEASURING.to_string(),
        ScreenMatch::Matched => "On".to_string(),
        ScreenMatch::Unmatched(hz) => format!("Off at {hz:.0} Hz"),
    }
}

pub fn per_second_text(per_second: f64) -> String {
    format!("{per_second:.1} a second")
}

pub fn repeated_hint(budget: Budget) -> String {
    let gap = rate_gap(budget);
    if gap < 0.01 {
        return REPEATED_HINT.to_string();
    }
    let every = 1.0 / gap;
    let often = if every < 1.5 {
        "about once a second".to_string()
    } else {
        format!("about once every {every:.0} seconds")
    };
    format!("{REPEATED_HINT} On this screen that happens {often}.")
}

pub fn app_updates_text(shown: f32, budget: Budget) -> String {
    match budget.refresh {
        Some(refresh) => {
            let rate = (1.0 / refresh.as_secs_f32()).round();
            format!("{:.0} of {rate:.0} a second", shown.round().min(rate))
        }
        None => format!("{:.0} a second", shown.round()),
    }
}

impl LoadView {
    fn gpu_part(&self, allowed: Duration) -> Option<Part> {
        if cfg!(target_arch = "wasm32") || !self.ds {
            return None;
        }
        let kind = match self.gpu {
            GpuView::Native => None,
            GpuView::Measuring(kind) => kind,
            GpuView::Idle(kind) | GpuView::Timed(kind, _) => Some(kind),
        };
        let value = match self.gpu {
            GpuView::Native => Value::Text("Off at Native"),
            GpuView::Measuring(_) if self.paused => Value::Text(PAUSED),
            GpuView::Measuring(_) => Value::Text("Measuring"),
            GpuView::Idle(_) => Value::Text("No 3D frames"),
            GpuView::Timed(_, time) => Value::Time(time),
        };
        let hint = match kind {
            Some(GpuClockKind::SubmitToDone) => GPU_SUBMIT_HINT,
            Some(GpuClockKind::Stamps) | None => GPU_STAMPS_HINT,
        };
        Some(Part {
            label: "3D GPU",
            cause: "3D GPU",
            hint,
            value,
            allowed,
        })
    }

    fn budget_note(&self) -> Option<&'static str> {
        self.paused.then_some(BEFORE_PAUSE)
    }

    fn jit(&self) -> Option<Facts> {
        let jit = self.jit?;
        let measured = jit.compiled.is_some() || jit.compiling.is_some();
        let mut rows = vec![("Blocks", blocks(jit.blocks))];
        if measured || !self.paused {
            let measuring = || "Measuring".to_string();
            rows.push((
                "Compiled",
                jit.compiled
                    .map_or_else(measuring, |share| format!("{:.0}%", share * 100.0)),
            ));
            rows.push((
                "Compiling",
                jit.compiling
                    .map_or_else(measuring, |time| format!("{} a second", millis(time))),
            ));
        }
        rows.push(("Compiles on", compiles_on(jit.threads)));
        Some(Facts {
            hint: JIT_HINT,
            rows,
            note: match (self.paused, measured) {
                (false, _) => None,
                (true, true) => Some(BEFORE_PAUSE),
                (true, false) => Some(PAUSED),
            },
        })
    }
}

pub fn compiles_on(threads: usize) -> String {
    match threads {
        0 => "Console thread".to_string(),
        1 => "1 thread of its own".to_string(),
        n => format!("{n} threads of its own"),
    }
}

fn blocks(count: u64) -> String {
    thousands(usize::try_from(count).unwrap_or(usize::MAX))
}

struct Facts {
    hint: &'static str,
    rows: Vec<(&'static str, String)>,
    note: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceAction {
    StartRecording,
    StopRecording,
}

pub struct Timings {
    pub game: Duration,
    pub app: Duration,
    pub shown: f32,
}

pub struct PerformanceWidget<'a> {
    pub budget: Budget,
    pub timings: Timings,
    pub readout: Option<Readout>,
    pub load: LoadView,
    pub samples: &'a VecDeque<FrameSample>,
    pub recording: Option<(Duration, usize)>,
    pub screen_match: Option<ScreenMatch>,
}

impl PerformanceWidget<'_> {
    fn parts(&self) -> Vec<Part> {
        let game = self
            .readout
            .as_ref()
            .and_then(|readout| readout.game)
            .unwrap_or(self.timings.game);
        let app = self
            .readout
            .as_ref()
            .map_or(self.timings.app, |readout| readout.app);
        let mut parts = vec![
            Part {
                label: "Game",
                cause: "game",
                hint: GAME_HINT,
                value: Value::Time(game),
                allowed: self.budget.game,
            },
            Part {
                label: "App",
                cause: "app",
                hint: APP_HINT,
                value: Value::Time(app),
                allowed: self.budget.screen(),
            },
        ];
        parts.extend(self.load.gpu_part(self.budget.game));
        parts
    }

    fn verdict(&self, parts: &[Part]) -> Option<Verdict> {
        verdict(self.readout.as_ref(), parts, self.load.paused)
    }

    fn more(&self) -> Vec<(&'static str, String, Option<String>)> {
        let (repeated, dropped) = match &self.readout {
            Some(readout) => (
                per_second_text(readout.repeated_late),
                per_second_text(readout.dropped),
            ),
            None => (MEASURING.to_string(), MEASURING.to_string()),
        };
        let mut rows = vec![
            (
                "App updates",
                app_updates_text(self.timings.shown, self.budget),
                None,
            ),
            ("Late repeats", repeated, Some(repeated_hint(self.budget))),
            ("Frames dropped", dropped, None),
        ];
        if let Some(status) = self.screen_match {
            rows.push((
                MATCH_SCREEN_LABEL,
                screen_match_text(status),
                match_screen_line(Some(status)),
            ));
        }
        rows
    }

    pub fn show(self, ui: &mut Ui) -> (Response, Option<PerformanceAction>) {
        let mut action = None;
        let parts = self.parts();
        let verdict = self.verdict(&parts);
        let more = self.more();
        let jit = self.load.jit();
        let response = ui
            .vertical(|ui| {
                ui.add_space(4.0);
                headline(ui, headline_text(self.readout.as_ref()), verdict);
                ui.add_space(8.0);
                let value_width = value_width(ui, &parts);
                let label_width = label_width(ui, &parts);
                for part in &parts {
                    bar_row(ui, part, label_width, value_width);
                }
                if let Some(note) = self.load.budget_note() {
                    section_note(ui, note);
                }
                ui.add_space(8.0);
                section(ui, icons::CHART_LINE, "Last 10 seconds", |ui| {
                    FrameGraph {
                        samples: self.samples,
                        budget: self.budget,
                        gpu: !cfg!(target_arch = "wasm32") && self.load.ds,
                    }
                    .ui(ui);
                });
                ui.add_space(-4.0);
                CollapsingHeader::new(RichText::new("More").strong())
                    .id_salt("performance_more")
                    .default_open(false)
                    .show(ui, |ui| {
                        facts_grid(ui, "performance_pace", more);
                    });
                if crate::app::mobile::touch_square(ui) == 0.0 {
                    ui.add_space(12.0);
                }
                if let Some(facts) = jit {
                    section_with_hint(ui, icons::LIGHTNING, "JIT", facts.hint, |ui| {
                        facts_grid(
                            ui,
                            "JIT",
                            facts
                                .rows
                                .into_iter()
                                .map(|(label, value)| (label, value, None))
                                .collect(),
                        );
                        if let Some(note) = facts.note {
                            section_note(ui, note);
                        }
                    });
                }
                section_with_hint(ui, icons::RECORD, "Record", RECORD_HINT, |ui| {
                    action = record_ui(ui, self.recording);
                });
            })
            .response;
        (response, action)
    }
}

fn headline(ui: &mut Ui, text: String, verdict: Option<Verdict>) {
    ui.label(RichText::new(text).size(26.0).strong());
    ui.label(RichText::new(GAME_FRAMES_LINE).weak());
    let Some(verdict) = verdict else {
        return;
    };
    ui.add_space(4.0);
    let visuals = ui.visuals();
    let (icon, color) = match verdict {
        Verdict::Paused => (icons::PAUSE, visuals.text_color()),
        Verdict::Smooth => (icons::CHECK_CIRCLE, visuals.text_color()),
        Verdict::Late(_) | Verdict::Slow => (icons::INFO, visuals.warn_fg_color),
        Verdict::HeldBack(_) => (icons::WARNING, visuals.error_fg_color),
    };
    ui.horizontal_top(|ui| {
        ui.label(RichText::new(icon).color(color));
        ui.add(Label::new(RichText::new(verdict.text()).color(color)).wrap());
    });
}

fn bar_row(ui: &mut Ui, part: &Part, label_width: f32, value_width: f32) {
    let visuals = ui.visuals().clone();
    let over = part.over();
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let note_height = ui.text_style_height(&egui::TextStyle::Small);
    let height = match part.value {
        Value::Time(_) => 2.0 * (note_height + 4.0) + 12.0,
        Value::Text(_) => 22.0,
    }
    .max(ui.spacing().interact_size.y);
    let (value, color) = match part.value {
        Value::Time(time) if over => (budget_text(time), visuals.error_fg_color),
        Value::Time(time) => (budget_text(time), visuals.text_color()),
        Value::Text(text) => (text.to_string(), visuals.weak_text_color()),
    };
    let tick = ui
        .horizontal(|ui| {
            label_cell(ui, vec2(label_width, height), part.label, Some(part.hint));
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
            let painter = ui.painter();
            let galley = painter.layout_no_wrap(value, body, color);
            painter.galley(
                pos2(
                    rect.right() - galley.size().x,
                    rect.center().y - galley.size().y / 2.0,
                ),
                galley,
                color,
            );
            let Value::Time(time) = part.value else {
                return None;
            };
            let bar = egui::Rect::from_min_max(
                pos2(rect.left(), rect.center().y - 6.0),
                pos2(
                    rect.right() - value_width - COLUMN_GAP / 2.0,
                    rect.center().y + 6.0,
                ),
            );
            painter.rect_filled(bar, CornerRadius::same(3), visuals.extreme_bg_color);
            let scale = part.allowed.as_secs_f64() * BAR_SCALE;
            let share = if scale > 0.0 {
                (time.as_secs_f64() / scale).min(1.0) as f32
            } else {
                0.0
            };
            let mut filled = bar;
            filled.set_width(bar.width() * share);
            let fill = if over {
                visuals.error_fg_color
            } else {
                visuals.text_color().gamma_multiply(0.45)
            };
            painter.rect_filled(filled, CornerRadius::same(3), fill);
            let x = bar.left() + bar.width() / BAR_SCALE as f32;
            painter.line_segment(
                [pos2(x, bar.top() - 3.0), pos2(x, bar.bottom() + 3.0)],
                Stroke::new(1.5_f32, visuals.text_color()),
            );
            Some((x, bar.left(), rect.right(), bar.bottom()))
        })
        .inner;
    let Some((x, left, right, bar_bottom)) = tick else {
        return;
    };
    let color = if over {
        visuals.error_fg_color
    } else {
        visuals.weak_text_color()
    };
    let painter = ui.painter().clone();
    let galley = painter.layout_no_wrap(
        format!("{} allowed", budget_text(part.allowed)),
        small,
        color,
    );
    let size = galley.size();
    let start = (x - size.x / 2.0).clamp(left, (right - size.x).max(left));
    painter.galley(pos2(start, bar_bottom + 4.0), galley, color);
}

fn value_width(ui: &Ui, parts: &[Part]) -> f32 {
    let body = egui::TextStyle::Body.resolve(ui.style());
    parts
        .iter()
        .filter_map(|part| match part.value {
            Value::Time(time) => Some(budget_text(time)),
            Value::Text(_) => None,
        })
        .map(|text| {
            ui.painter()
                .layout_no_wrap(text, body.clone(), ui.visuals().text_color())
                .size()
                .x
        })
        .fold(0.0, f32::max)
}

fn label_width(ui: &Ui, parts: &[Part]) -> f32 {
    let body = egui::TextStyle::Body.resolve(ui.style());
    parts
        .iter()
        .map(|part| {
            ui.painter()
                .layout_no_wrap(
                    part.label.to_string(),
                    body.clone(),
                    ui.visuals().text_color(),
                )
                .size()
                .x
                + info_mark_width(ui)
        })
        .fold(LABEL_WIDTH, f32::max)
}

fn record_ui(ui: &mut Ui, recording: Option<(Duration, usize)>) -> Option<PerformanceAction> {
    match recording {
        None => wide_button(ui, Button::new(format!("{} Start", icons::RECORD)))
            .clicked()
            .then_some(PerformanceAction::StartRecording),
        Some((length, frames)) => {
            let stop =
                wide_button(ui, Button::new(format!("{} {STOP_RECORDING}", icons::STOP))).clicked();
            ui.label(faint(recorded(length, frames)));
            stop.then_some(PerformanceAction::StopRecording)
        }
    }
}

pub fn recorded(length: Duration, frames: usize) -> String {
    let secs = length.as_secs();
    format!(
        "{}:{:02}, {} frames",
        secs / 60,
        secs % 60,
        thousands(frames)
    )
}

fn label_cell(ui: &mut Ui, size: Vec2, label: &str, hint: Option<&str>) {
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(size);
        ui.label(label);
        if let Some(hint) = hint {
            info_mark(ui, hint);
        }
    });
}

fn facts_grid(ui: &mut Ui, id: &str, rows: Vec<(&str, String, Option<String>)>) {
    Grid::new(("performance_facts", id))
        .num_columns(2)
        .min_col_width(0.0)
        .spacing([COLUMN_GAP, ui.spacing().item_spacing.y])
        .show(ui, |ui| {
            for (label, value, hint) in rows {
                label_cell(
                    ui,
                    vec2(LABEL_WIDTH, ui.spacing().interact_size.y),
                    label,
                    hint.as_deref(),
                );
                ui.add(Label::new(value).wrap());
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "nds")]
    use crate::emulator::load::GpuTotals;

    const DEFAULT_DOCK_WIDTH: f32 = 200.0;
    const BUDGET: Budget = Budget {
        game: Duration::from_micros(16_715),
        refresh: None,
    };
    const AT_180_HZ: Budget = Budget {
        refresh: Some(Duration::from_micros(5_556)),
        ..BUDGET
    };

    #[cfg(feature = "nds")]
    fn gpu(view: &LoadView) -> Value {
        view.gpu_part(BUDGET.game).unwrap().value
    }

    fn part(cause: &'static str, micros: u64, allowed: Duration) -> Part {
        Part {
            label: cause,
            cause,
            hint: "",
            value: Value::Time(Duration::from_micros(micros)),
            allowed,
        }
    }

    fn race(game: u64, app: u64, gpu: u64) -> Vec<Part> {
        vec![
            part("game", game, AT_180_HZ.game),
            part("app", app, AT_180_HZ.screen()),
            part("3D GPU", gpu, AT_180_HZ.game),
        ]
    }

    fn readout(fps: f64, late: u32) -> Readout {
        Readout {
            at: Duration::ZERO,
            fps,
            rate: 59.83,
            late,
            game: Some(Duration::from_millis(4)),
            app: Duration::from_millis(1),
            repeated: 0.0,
            repeated_late: 0.0,
            dropped: 0.0,
            low: None,
            refresh: None,
            audio_queued: None,
            audio_underrun: Duration::ZERO,
        }
    }

    #[test]
    fn nothing_late_is_smooth_even_with_a_part_over_its_mark() {
        let over = race(4_500, 1_500, 20_000);
        assert!(over[2].over());
        assert_eq!(
            verdict(Some(&readout(59.8, 0)), &over, false),
            Some(Verdict::Smooth)
        );
        assert_eq!(Verdict::Smooth.text(), SMOOTH);
    }

    #[test]
    fn late_frames_are_held_back_by_the_part_furthest_over_its_mark() {
        assert_eq!(
            verdict(Some(&readout(59.8, 14)), &race(4_500, 1_500, 20_000), false),
            Some(Verdict::HeldBack("3D GPU"))
        );
        assert_eq!(
            verdict(Some(&readout(40.0, 3)), &race(20_000, 7_000, 3_000), false),
            Some(Verdict::HeldBack("app"))
        );
        assert_eq!(
            Verdict::HeldBack("3D GPU").text(),
            "Held back by the 3D GPU"
        );
        let off = Part {
            value: Value::Text("Off at Native"),
            ..part("3D GPU", 0, Duration::ZERO)
        };
        assert!(!off.over());
    }

    #[test]
    fn late_frames_with_every_part_in_time_are_counted() {
        let in_time = race(4_500, 1_500, 9_000);
        assert_eq!(
            verdict(Some(&readout(59.8, 14)), &in_time, false),
            Some(Verdict::Late(14))
        );
        assert_eq!(
            Verdict::Late(14).text(),
            "14 frames late in the last 5 seconds"
        );
        assert_eq!(
            Verdict::Late(1).text(),
            "1 frame late in the last 5 seconds"
        );
        assert_eq!(
            verdict(Some(&readout(50.0, 0)), &in_time, false),
            Some(Verdict::Slow)
        );
    }

    #[test]
    fn a_paused_game_says_so_before_anything_else() {
        let over = race(4_500, 1_500, 20_000);
        assert_eq!(
            verdict(Some(&readout(0.0, 30)), &over, true),
            Some(Verdict::Paused)
        );
        assert_eq!(verdict(None, &over, true), Some(Verdict::Paused));
        assert_eq!(verdict(None, &over, false), None);
    }

    #[test]
    fn the_headline_counts_game_frames_against_the_game_rate() {
        assert_eq!(headline_text(Some(&readout(59.64, 0))), "59.6 of 59.8");
        assert_eq!(headline_text(Some(&readout(61.0, 0))), "59.8 of 59.8");
        assert_eq!(headline_text(None), MEASURING);
    }

    #[test]
    fn late_repeats_and_drops_read_per_second() {
        assert_eq!(per_second_text(0.01), "0.0 a second");
        assert_eq!(per_second_text(1.64), "1.6 a second");
        assert!(
            repeated_hint(AT_180_HZ)
                .ends_with(" On this screen that happens about once every 2 seconds."),
            "{}",
            repeated_hint(AT_180_HZ)
        );
        assert_eq!(repeated_hint(BUDGET), REPEATED_HINT);
    }

    #[test]
    fn app_updates_count_against_the_screen_rate() {
        assert_eq!(app_updates_text(179.4, AT_180_HZ), "179 of 180 a second");
        assert_eq!(app_updates_text(185.0, AT_180_HZ), "180 of 180 a second");
        assert_eq!(app_updates_text(59.8, BUDGET), "60 a second");
    }

    #[test]
    fn the_3d_gpu_gets_one_game_frame() {
        let samples = VecDeque::new();
        let widget = PerformanceWidget {
            budget: AT_180_HZ,
            timings: Timings {
                game: Duration::from_millis(8),
                app: Duration::from_millis(4),
                shown: 180.0,
            },
            readout: Some(Readout {
                game: Some(Duration::from_millis(8)),
                app: Duration::from_millis(4),
                ..readout(59.8, 0)
            }),
            load: LoadView {
                ds: true,
                gpu: GpuView::Timed(GpuClockKind::Stamps, Duration::from_millis(15)),
                ..LoadView::default()
            },
            ..widest(&samples)
        };
        let parts = widget.parts();
        let allowed: Vec<(&str, String, bool)> = parts
            .iter()
            .map(|part| (part.label, budget_text(part.allowed), part.over()))
            .collect();
        let mut want = vec![
            ("Game", "16.7 ms".to_string(), false),
            ("App", "5.6 ms".to_string(), false),
        ];
        if !cfg!(target_arch = "wasm32") {
            want.push(("3D GPU", "16.7 ms".to_string(), false));
        }
        assert_eq!(allowed, want);
        assert_eq!(parts[0].value, Value::Time(Duration::from_millis(8)));
        assert_eq!(widget.verdict(&parts), Some(Verdict::Smooth));
    }

    #[test]
    fn more_says_whether_the_refresh_rate_is_matched() {
        let samples = VecDeque::new();
        let row = |status| {
            let widget = PerformanceWidget {
                screen_match: status,
                ..widest(&samples)
            };
            widget
                .more()
                .into_iter()
                .find(|(label, _, _)| *label == MATCH_SCREEN_LABEL)
                .map(|(_, value, hint)| (value, hint.is_some()))
        };
        assert_eq!(row(None), None);
        assert_eq!(row(Some(ScreenMatch::Matched)), Some(("On".into(), false)));
        assert_eq!(row(Some(ScreenMatch::Off)), Some(("Off".into(), false)));
        assert_eq!(
            row(Some(ScreenMatch::Unmatched(139.4))),
            Some(("Off at 139 Hz".into(), true))
        );
    }

    #[test]
    fn the_jit_says_where_it_compiles() {
        assert_eq!(compiles_on(0), "Console thread");
        assert_eq!(compiles_on(3), "3 threads of its own");
        let view = LoadView {
            jit: Some(JitView {
                blocks: 1200,
                threads: 0,
                compiled: Some(0.5),
                compiling: Some(Duration::from_millis(3)),
            }),
            ..LoadView::default()
        };
        let facts = view.jit().unwrap();
        assert_eq!(
            facts.rows,
            [
                ("Blocks", "1,200".to_string()),
                ("Compiled", "50%".to_string()),
                ("Compiling", "3.0 ms a second".to_string()),
                ("Compiles on", "Console thread".to_string()),
            ]
        );
    }

    fn widest(samples: &VecDeque<FrameSample>) -> PerformanceWidget<'_> {
        let slow = Duration::from_millis(99);
        PerformanceWidget {
            budget: BUDGET,
            timings: Timings {
                game: slow,
                app: slow,
                shown: 999.0,
            },
            readout: Some(Readout {
                game: Some(slow),
                app: slow,
                repeated: 144.0,
                repeated_late: 144.0,
                dropped: 144.0,
                ..readout(59.83, 999)
            }),
            load: LoadView {
                ds: true,
                gpu: GpuView::Timed(GpuClockKind::Stamps, slow),
                jit: Some(JitView {
                    blocks: 1_234_567,
                    threads: 3,
                    compiled: Some(1.0),
                    compiling: Some(slow),
                }),
                paused: false,
            },
            samples,
            recording: Some((Duration::from_secs(3599), 215_940)),
            screen_match: Some(ScreenMatch::Shared),
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn upscaling_with_no_3d_on_screen_counts_no_frames_and_is_not_native() {
        let reading = LoadReading {
            ds: true,
            upscaling: true,
            gpu: GpuTotals {
                kind: Some(GpuClockKind::Stamps),
                ..Default::default()
            },
            ..Default::default()
        };
        let window = Window {
            time: Duration::from_secs(1),
            before: reading,
        };
        let view = load_view(reading, Some(window));
        assert_eq!(view.gpu, GpuView::Idle(GpuClockKind::Stamps));
        assert_eq!(gpu(&view), Value::Text("No 3D frames"));
        assert_eq!(view.budget_note(), None);
    }

    #[cfg(feature = "nds")]
    fn reading(compiled: u64, frames: u64) -> LoadReading {
        LoadReading {
            ds: true,
            upscaling: true,
            gpu: GpuTotals {
                kind: Some(GpuClockKind::Stamps),
                time: Duration::from_millis(2 * frames),
                done: Duration::ZERO,
                frames,
                work: None,
                ..GpuTotals::default()
            },
            jit: Some(citrine_gb::nds::machine::JitCounts {
                blocks: 10,
                compiled,
                stepped: compiled / 49,
                compile_time: Duration::from_millis(compiled / 100),
                threads: 3,
            }),
        }
    }

    #[cfg(feature = "nds")]
    fn rows(facts: Option<Facts>) -> (Vec<(&'static str, String)>, Option<&'static str>) {
        let facts = facts.unwrap();
        (facts.rows, facts.note)
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_paused_game_shows_the_last_reading_from_while_it_ran() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut state = PerformanceState::default();
        state.update(at(0), true, reading(0, 0));
        state.update(at(600), true, reading(980, 30));
        state.update(at(1200), false, reading(980, 30));
        state.update(at(1800), false, reading(980, 30));
        let view = state.view();
        assert!(view.paused);
        let (jit, note) = rows(view.jit());
        assert_eq!(jit[1], ("Compiled", "98%".to_string()));
        assert_eq!(note, Some(BEFORE_PAUSE));
        assert_eq!(
            (gpu(&view), view.budget_note()),
            (Value::Time(Duration::from_millis(2)), Some(BEFORE_PAUSE))
        );
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_phone_hold_keeps_the_jit_reading_and_says_the_gpu_had_none() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut state = PerformanceState::default();
        let unwatched = |compiled| LoadReading {
            gpu: GpuTotals::default(),
            ..reading(compiled, 0)
        };
        state.update(at(0), true, unwatched(0));
        state.update(at(600), true, unwatched(500));
        state.update(at(1200), false, reading(500, 0));
        let view = state.view();
        let (jit, note) = rows(view.jit());
        assert_eq!(jit[1], ("Compiled", "98%".to_string()));
        assert_eq!(note, Some(BEFORE_PAUSE));
        assert_eq!(gpu(&view), Value::Text(PAUSED));
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_game_paused_before_any_reading_says_so_once_per_section() {
        let start = Instant::now();
        let mut state = PerformanceState::default();
        state.update(start, false, reading(0, 0));
        state.update(start + Duration::from_millis(600), false, reading(0, 0));
        let view = state.view();
        let (jit, note) = rows(view.jit());
        assert!(jit.iter().all(|(_, value)| value != PAUSED), "{jit:?}");
        assert_eq!(note, Some(PAUSED));
        assert_eq!(gpu(&view), Value::Text(PAUSED));
        assert_eq!(
            jit,
            [
                ("Blocks", "10".to_string()),
                ("Compiles on", "3 threads of its own".to_string())
            ]
        );
        state.update(start + Duration::from_millis(1200), true, reading(0, 0));
        state.update(start + Duration::from_millis(1800), true, reading(490, 10));
        let (jit, note) = rows(state.view().jit());
        assert_eq!(jit[1], ("Compiled", "98%".to_string()));
        assert_eq!(note, None);
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_phone_holds_the_game_until_a_reading_exists() {
        let start = Instant::now();
        let mut state = PerformanceState::default();
        assert!(!state.take_seen());
        state.update(start, true, reading(0, 0));
        assert!(state.measuring_before_hold());
        state.update(start + Duration::from_millis(600), true, reading(490, 10));
        assert!(!state.measuring_before_hold());
        assert!(!state.take_seen());
        assert!(state.measuring_before_hold());
    }

    #[test]
    fn every_column_fits_the_default_dock() {
        let ctx = egui::Context::default();
        crate::app::Citrine::setup_fonts(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 1600.0),
            )),
            ..Default::default()
        };
        let samples = VecDeque::new();
        let mut texts = Vec::new();
        let mut left = 0.0;
        for _ in 0..2 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    left = ui.min_rect().left();
                    ui.allocate_ui(egui::vec2(DEFAULT_DOCK_WIDTH, 1600.0), |ui| {
                        widest(&samples).show(ui);
                    });
                });
            });
            texts = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.text().to_owned(),
                        text.pos.x + text.galley.rect.left(),
                        text.pos.x + text.galley.rect.right(),
                    )),
                    _ => None,
                })
                .collect();
        }
        for name in [
            "59.8 of 59.8",
            GAME_FRAMES_LINE,
            "Held back by the 3D GPU",
            "Game",
            "App",
            "3D GPU",
            "99.0 ms",
            "16.7 ms allowed",
            "More",
            "Compiles on",
            "1,234,567",
            STOP_RECORDING,
        ] {
            assert!(
                texts.iter().any(|(text, _, _)| text.ends_with(name)),
                "{name}: {texts:?}"
            );
        }
        let right = left + DEFAULT_DOCK_WIDTH;
        let cut: Vec<_> = texts.iter().filter(|(_, _, end)| *end > right).collect();
        assert!(cut.is_empty(), "past {right}: {cut:?}");
        let ends = |value: &str| -> Vec<f32> {
            texts
                .iter()
                .filter(|(text, _, _)| text == value)
                .map(|(_, _, end)| *end)
                .collect()
        };
        let values = ends("99.0 ms");
        assert_eq!(values.len(), 3, "{texts:?}");
        assert!(values.iter().all(|end| (end - values[0]).abs() < 0.5));
        assert_eq!(ends("16.7 ms allowed").len(), 3, "{texts:?}");
    }
}
