use crate::emulator::load::GPU_PASSES;
use std::collections::VecDeque;
use std::fmt::Write;
use std::time::Duration;
use web_time::Instant;

pub mod readout;
pub mod tape_times;

pub use readout::Readout;
pub use tape_times::TapeTimes;

pub const SHOWN: Duration = Duration::from_secs(10);
const MOST_SHOWN: usize = 4096;
const MOST_RECORDED: usize = 60 * 60 * 30;

pub const COLUMNS: [&str; 48] = [
    "time_s",
    "display_ms",
    "ui_ms",
    "game_ms",
    "game_frames",
    "repeated",
    "dropped",
    "gpu_ms",
    "gpu_frames",
    "gpu_full_ms",
    "jit_compile_ms",
    "jit_blocks",
    "budget_ms",
    "refresh_ms",
    "console",
    "game",
    "renderer",
    "resolution_3d",
    "smooth_edges",
    "jit",
    "gpu_clock",
    "gpu_reference_ms",
    "gpu_opaque_ms",
    "gpu_tail_ms",
    "gpu_edges_ms",
    "gpu_finish_ms",
    "gpu_present_ms",
    "gpu_mipmaps_ms",
    "gpu_composite_ms",
    "present_wait_ms",
    "ui_shapes",
    "draw_ms",
    "swap_wait_ms",
    "console_wait_ms",
    "audio_queued_ms",
    "audio_underrun_ms",
    "audio_overrun_ms",
    "gpu_calibration_ms",
    "delay_frames",
    "ping_ms",
    "ran_frames",
    "waited_frames",
    "skipped_frames",
    "repaid_frames",
    "ahead_frames",
    "watching",
    "events",
    "tape_frame",
];

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Totals {
    pub game: Duration,
    pub game_frames: u64,
    pub repeated: u64,
    pub dropped: u64,
    pub gpu: Duration,
    pub gpu_frames: u64,
    pub gpu_full: Option<Duration>,
    pub gpu_passes: [Duration; GPU_PASSES],
    pub jit_compile: Duration,
    pub jit_blocks: u64,
    pub console_wait: Duration,
    pub audio: AudioTotals,
    pub session: Option<SessionTotals>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SessionTotals {
    pub delay: u32,
    pub ping_ms: Option<u32>,
    pub ran: u64,
    pub waited: u64,
    pub skipped: u64,
    pub repaid: u64,
    pub ahead: f64,
    pub watching: bool,
}

impl SessionTotals {
    fn since(self, before: Option<Self>) -> Self {
        let before = before.unwrap_or_default();
        Self {
            ran: count_since(self.ran, before.ran),
            waited: count_since(self.waited, before.waited),
            skipped: count_since(self.skipped, before.skipped),
            repaid: count_since(self.repaid, before.repaid),
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioTotals {
    pub rate: u32,
    pub underrun_frames: u64,
    pub overrun_frames: u64,
    pub queued_frames: Option<u64>,
}

impl AudioTotals {
    fn time(&self, frames: u64) -> Duration {
        if self.rate == 0 {
            return Duration::ZERO;
        }
        Duration::from_secs_f64(frames as f64 / f64::from(self.rate))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioSample {
    pub queued: Option<Duration>,
    pub underrun: Duration,
    pub overrun: Duration,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Budget {
    pub game: Duration,
    pub refresh: Option<Duration>,
}

impl Budget {
    pub fn screen(&self) -> Duration {
        self.refresh.unwrap_or(self.game)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameSample {
    pub at: Duration,
    pub display: Duration,
    pub ui: Duration,
    pub game: Duration,
    pub game_frames: u64,
    pub repeated: u64,
    pub dropped: u64,
    pub gpu: Option<Duration>,
    pub gpu_frames: u64,
    pub gpu_full: Option<Duration>,
    pub gpu_passes: Option<[Duration; GPU_PASSES]>,
    pub jit_compile: Duration,
    pub jit_blocks: u64,
    pub console_wait: Duration,
    pub audio: AudioSample,
    pub budget: Budget,
}

impl FrameSample {
    pub fn game_per_frame(&self) -> Option<Duration> {
        u32::try_from(self.game_frames)
            .ok()
            .filter(|frames| *frames > 0)
            .map(|frames| self.game / frames)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunContext {
    pub console: &'static str,
    pub game: String,
    pub renderer: &'static str,
    pub resolution: String,
    pub smooth_edges: String,
    pub jit: bool,
    pub gpu_clock: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowFacts {
    pub started: Option<Instant>,
    pub cpu: Option<Duration>,
    pub ui_shapes: Option<usize>,
    pub gpu_calibration: Option<Duration>,
    pub events: Vec<String>,
    pub tape_frame: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct PowerSample {
    #[serde(rename = "time_s", serialize_with = "seconds")]
    pub at: Duration,
    pub thermal: &'static str,
    pub low_power: bool,
}

fn seconds<S: serde::Serializer>(time: &Duration, out: S) -> Result<S::Ok, S::Error> {
    out.serialize_f64((time.as_secs_f64() * 1000.0).round() / 1000.0)
}

struct Row {
    sample: FrameSample,
    context: usize,
    present_wait: Option<Duration>,
    ui_shapes: Option<usize>,
    draw: Option<Duration>,
    swap_wait: Option<Duration>,
    gpu_calibration: Option<Duration>,
    session: Option<SessionTotals>,
    events: Vec<String>,
    tape_frame: Option<u64>,
}

struct Recording {
    started: Instant,
    contexts: Vec<RunContext>,
    rows: Vec<Row>,
    pending: RowFacts,
    power: Vec<PowerSample>,
}

pub struct Recorded {
    pub csv: String,
    pub frames: usize,
    pub length: Duration,
    pub last_context: Option<RunContext>,
    pub power: Vec<PowerSample>,
}

#[derive(Default)]
pub struct FrameLog {
    began: Option<Instant>,
    last: Option<(Instant, Totals)>,
    last_ui: Option<Duration>,
    samples: VecDeque<FrameSample>,
    readout: Option<Readout>,
    recording: Option<Recording>,
}

fn since(now: Duration, before: Duration) -> Duration {
    now.checked_sub(before).unwrap_or(now)
}

fn count_since(now: u64, before: u64) -> u64 {
    now.checked_sub(before).unwrap_or(now)
}

impl FrameLog {
    pub fn note(
        &mut self,
        now: Instant,
        ui: Duration,
        totals: Totals,
        budget: Budget,
        context: impl FnOnce() -> RunContext,
    ) {
        let began = *self.began.get_or_insert(now);
        let last_ui = self.last_ui.replace(ui);
        let Some((last_at, before)) = self.last.replace((now, totals)) else {
            return;
        };
        let gpu = since(totals.gpu, before.gpu);
        let gpu_frames = count_since(totals.gpu_frames, before.gpu_frames);
        let frames = u32::try_from(gpu_frames).ok().filter(|frames| *frames > 0);
        let sample = FrameSample {
            at: now.saturating_duration_since(began),
            display: now.saturating_duration_since(last_at),
            ui,
            game: since(totals.game, before.game),
            game_frames: count_since(totals.game_frames, before.game_frames),
            repeated: count_since(totals.repeated, before.repeated),
            dropped: count_since(totals.dropped, before.dropped),
            gpu: frames.map(|frames| gpu / frames),
            gpu_frames,
            gpu_full: totals.gpu_full,
            gpu_passes: frames.map(|frames| {
                std::array::from_fn(|pass| {
                    since(totals.gpu_passes[pass], before.gpu_passes[pass]) / frames
                })
            }),
            jit_compile: since(totals.jit_compile, before.jit_compile),
            jit_blocks: count_since(totals.jit_blocks, before.jit_blocks),
            console_wait: since(totals.console_wait, before.console_wait),
            audio: AudioSample {
                queued: totals
                    .audio
                    .queued_frames
                    .map(|frames| totals.audio.time(frames)),
                underrun: totals.audio.time(count_since(
                    totals.audio.underrun_frames,
                    before.audio.underrun_frames,
                )),
                overrun: totals.audio.time(count_since(
                    totals.audio.overrun_frames,
                    before.audio.overrun_frames,
                )),
            },
            budget,
        };
        let oldest = sample.at.saturating_sub(SHOWN);
        while self
            .samples
            .front()
            .is_some_and(|first| first.at < oldest || self.samples.len() >= MOST_SHOWN)
        {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
        if let Some(recording) = &mut self.recording
            && recording.rows.len() < MOST_RECORDED
        {
            let context = context();
            if recording.contexts.last() != Some(&context) {
                recording.contexts.push(context);
            }
            let sample = FrameSample {
                at: now.saturating_duration_since(recording.started),
                ..sample
            };
            let facts = std::mem::take(&mut recording.pending);
            let draw = facts
                .cpu
                .zip(last_ui)
                .map(|(cpu, ui)| cpu.saturating_sub(ui));
            let outside = facts
                .started
                .map(|started| started.saturating_duration_since(last_at));
            recording.rows.push(Row {
                sample,
                context: recording.contexts.len() - 1,
                present_wait: facts.cpu.map(|cpu| sample.display.saturating_sub(cpu)),
                ui_shapes: facts.ui_shapes,
                draw,
                swap_wait: outside
                    .zip(draw)
                    .map(|(outside, draw)| outside.saturating_sub(draw)),
                gpu_calibration: facts.gpu_calibration,
                session: totals.session.map(|session| session.since(before.session)),
                events: facts.events,
                tape_frame: facts.tape_frame,
            });
        }
    }

    pub fn facts(&mut self, facts: RowFacts) {
        if let Some(recording) = &mut self.recording {
            let mut events = std::mem::take(&mut recording.pending.events);
            events.extend(facts.events);
            recording.pending = RowFacts { events, ..facts };
        }
    }

    pub fn note_power(&mut self, now: Instant, thermal: &'static str, low_power: bool) {
        if let Some(recording) = &mut self.recording {
            recording.power.push(PowerSample {
                at: now.saturating_duration_since(recording.started),
                thermal,
                low_power,
            });
        }
    }

    pub fn samples(&self) -> &VecDeque<FrameSample> {
        &self.samples
    }

    pub fn readout(&mut self) -> Option<Readout> {
        let newest = self.samples.back()?.at;
        let stale = self.readout.as_ref().is_none_or(|last| {
            newest < last.at || newest.saturating_sub(last.at) >= readout::EVERY
        });
        if stale {
            self.readout = readout::readout(&self.samples);
        }
        self.readout.clone()
    }

    pub fn start_recording(&mut self, now: Instant) {
        self.recording = Some(Recording {
            started: now,
            contexts: Vec::new(),
            rows: Vec::new(),
            pending: RowFacts::default(),
            power: Vec::new(),
        });
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    pub fn stop_recording(&mut self) -> Option<Recorded> {
        let recording = self.recording.take()?;
        let mut csv = COLUMNS.join(",");
        csv.push('\n');
        for row in &recording.rows {
            write_row(&mut csv, row, &recording.contexts[row.context]);
        }
        Some(Recorded {
            csv,
            frames: recording.rows.len(),
            length: recording
                .rows
                .last()
                .map_or(Duration::ZERO, |row| row.sample.at),
            last_context: recording.contexts.last().cloned(),
            power: recording.power,
        })
    }
}

fn ms(time: Duration) -> String {
    format!("{:.3}", time.as_secs_f64() * 1000.0)
}

fn quoted(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

fn write_row(csv: &mut String, row: &Row, context: &RunContext) {
    let sample = &row.sample;
    let _ = writeln!(
        csv,
        "{:.3},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        sample.at.as_secs_f64(),
        ms(sample.display),
        ms(sample.ui),
        ms(sample.game),
        sample.game_frames,
        sample.repeated,
        sample.dropped,
        sample.gpu.map(ms).unwrap_or_default(),
        sample.gpu_frames,
        sample.gpu_full.map(ms).unwrap_or_default(),
        ms(sample.jit_compile),
        sample.jit_blocks,
        ms(sample.budget.game),
        sample.budget.refresh.map(ms).unwrap_or_default(),
        context.console,
        quoted(&context.game),
        context.renderer,
        context.resolution,
        context.smooth_edges,
        if context.jit { "on" } else { "off" },
        context.gpu_clock,
        passes(sample.gpu_passes),
        row.present_wait.map(ms).unwrap_or_default(),
        row.ui_shapes
            .map(|shapes| shapes.to_string())
            .unwrap_or_default(),
        row.draw.map(ms).unwrap_or_default(),
        row.swap_wait.map(ms).unwrap_or_default(),
        ms(sample.console_wait),
        sample.audio.queued.map(ms).unwrap_or_default(),
        ms(sample.audio.underrun),
        ms(sample.audio.overrun),
        row.gpu_calibration.map(ms).unwrap_or_default(),
        session(row.session),
        quoted(&row.events.join(";")),
        row.tape_frame
            .map(|frame| frame.to_string())
            .unwrap_or_default(),
    );
}

fn session(session: Option<SessionTotals>) -> String {
    let Some(s) = session else {
        return ",,,,,,,".to_string();
    };
    format!(
        "{},{},{},{},{},{},{:.2},{}",
        s.delay,
        s.ping_ms.map(|ms| ms.to_string()).unwrap_or_default(),
        s.ran,
        s.waited,
        s.skipped,
        s.repaid,
        s.ahead,
        if s.watching { "on" } else { "off" },
    )
}

fn passes(passes: Option<[Duration; GPU_PASSES]>) -> String {
    match passes {
        Some(passes) => passes.map(ms).join(","),
        None => [""; GPU_PASSES].join(","),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: Budget = Budget {
        game: Duration::from_micros(16_715),
        refresh: None,
    };

    fn context() -> RunContext {
        RunContext {
            console: "DS",
            game: "Mario Kart DS".to_string(),
            renderer: "gpu",
            resolution: "4x".to_string(),
            smooth_edges: "off".to_string(),
            jit: true,
            gpu_clock: "stamps",
        }
    }

    fn totals(frame: u64) -> Totals {
        Totals {
            game: Duration::from_millis(7 * frame),
            game_frames: frame,
            repeated: frame / 4,
            dropped: 0,
            gpu: Duration::from_millis(3 * frame),
            gpu_frames: frame,
            gpu_full: Some(Duration::from_micros(1_500)),
            gpu_passes: [
                Duration::from_micros(500 * frame),
                Duration::ZERO,
                Duration::from_millis(frame),
                Duration::from_millis(frame),
                Duration::ZERO,
                Duration::ZERO,
                Duration::ZERO,
                Duration::from_micros(500 * frame),
            ],
            jit_compile: Duration::from_micros(500 * frame),
            jit_blocks: 2 * frame,
            console_wait: Duration::from_micros(250 * frame),
            audio: AudioTotals {
                rate: 48_000,
                underrun_frames: 48 * frame,
                overrun_frames: 0,
                queued_frames: Some(3_840),
            },
            session: None,
        }
    }

    #[test]
    fn each_display_frame_gets_what_changed_since_the_one_before() {
        let start = Instant::now();
        let mut log = FrameLog::default();
        log.note(start, Duration::ZERO, totals(0), BUDGET, context);
        assert!(log.samples().is_empty());
        log.note(
            start + Duration::from_millis(17),
            Duration::from_millis(4),
            Totals {
                gpu_frames: 0,
                gpu: Duration::ZERO,
                ..totals(1)
            },
            BUDGET,
            context,
        );
        log.note(
            start + Duration::from_millis(50),
            Duration::from_millis(5),
            totals(3),
            BUDGET,
            context,
        );
        let samples: Vec<_> = log.samples().iter().copied().collect();
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].display, Duration::from_millis(17));
        assert_eq!(samples[0].game, Duration::from_millis(7));
        assert_eq!(samples[0].gpu, None);
        assert_eq!(samples[1].display, Duration::from_millis(33));
        assert_eq!(samples[1].game_frames, 2);
        assert_eq!(samples[1].game_per_frame(), Some(Duration::from_millis(7)));
        assert_eq!(samples[1].gpu, Some(Duration::from_millis(3)));
        assert_eq!(samples[1].gpu_frames, 3);
        assert_eq!(samples[1].repeated, 0);
    }

    #[test]
    fn a_new_game_restarts_the_totals_without_a_huge_frame() {
        let start = Instant::now();
        let mut log = FrameLog::default();
        log.note(start, Duration::ZERO, totals(600), BUDGET, context);
        log.note(
            start + Duration::from_millis(17),
            Duration::ZERO,
            totals(1),
            BUDGET,
            context,
        );
        assert_eq!(log.samples()[0].game, Duration::from_millis(7));
        assert_eq!(log.samples()[0].game_frames, 1);
    }

    #[test]
    fn the_graph_keeps_the_last_ten_seconds() {
        let start = Instant::now();
        let mut log = FrameLog::default();
        for frame in 0..=720u64 {
            log.note(
                start + Duration::from_micros(16_667 * frame),
                Duration::ZERO,
                totals(frame),
                BUDGET,
                context,
            );
        }
        let samples = log.samples();
        let newest = samples.back().unwrap().at;
        assert!(newest - samples.front().unwrap().at <= SHOWN);
        assert!(
            (595..=601).contains(&samples.len()),
            "{} samples",
            samples.len()
        );
        assert!(!log.is_recording());
    }

    #[test]
    fn a_recording_saves_one_csv_row_per_display_frame_with_its_settings() {
        let start = Instant::now();
        let mut log = FrameLog::default();
        log.note(start, Duration::ZERO, totals(0), BUDGET, context);
        log.start_recording(start);
        log.note(
            start + Duration::from_micros(16_700),
            Duration::from_micros(3_250),
            Totals {
                repeated: 1,
                ..totals(1)
            },
            BUDGET,
            || RunContext {
                game: "Pokémon \"HeartGold\", JP".to_string(),
                ..context()
            },
        );
        log.note(
            start + Duration::from_micros(50_000),
            Duration::from_micros(2_000),
            Totals {
                gpu_frames: 1,
                gpu: Duration::from_millis(3),
                gpu_full: None,
                dropped: 1,
                ..totals(3)
            },
            Budget {
                refresh: Some(Duration::from_micros(5_556)),
                ..BUDGET
            },
            || RunContext {
                jit: false,
                renderer: "software",
                resolution: "native".to_string(),
                gpu_clock: "",
                ..context()
            },
        );
        assert!(log.is_recording());
        let recorded = log.stop_recording().unwrap();
        assert!(!log.is_recording());
        assert_eq!(recorded.frames, 2);
        let lines: Vec<&str> = recorded.csv.lines().collect();
        assert_eq!(
            lines,
            [
                "time_s,display_ms,ui_ms,game_ms,game_frames,repeated,dropped,gpu_ms,gpu_frames,gpu_full_ms,jit_compile_ms,jit_blocks,budget_ms,refresh_ms,console,game,renderer,resolution_3d,smooth_edges,jit,gpu_clock,gpu_reference_ms,gpu_opaque_ms,gpu_tail_ms,gpu_edges_ms,gpu_finish_ms,gpu_present_ms,gpu_mipmaps_ms,gpu_composite_ms,present_wait_ms,ui_shapes,draw_ms,swap_wait_ms,console_wait_ms,audio_queued_ms,audio_underrun_ms,audio_overrun_ms,gpu_calibration_ms,delay_frames,ping_ms,ran_frames,waited_frames,skipped_frames,repaid_frames,ahead_frames,watching,events,tape_frame",
                "0.017,16.700,3.250,7.000,1,1,0,3.000,1,1.500,0.500,2,16.715,,DS,\"Pokémon \"\"HeartGold\"\", JP\",gpu,4x,off,on,stamps,0.500,0.000,1.000,1.000,0.000,0.000,0.000,0.500,,,,,0.250,80.000,1.000,0.000,,,,,,,,,,,",
                "0.050,33.300,2.000,14.000,2,0,1,,0,,1.000,4,16.715,5.556,DS,Mario Kart DS,software,native,off,off,,,,,,,,,,,,,,0.500,80.000,2.000,0.000,,,,,,,,,,,",
            ]
        );
    }

    #[test]
    fn a_recorded_row_carries_the_gpu_time_of_each_pass_and_the_clock() {
        let start = Instant::now();
        let mut log = FrameLog::default();
        log.note(start, Duration::ZERO, totals(2), BUDGET, context);
        log.start_recording(start);
        log.note(
            start + Duration::from_millis(17),
            Duration::ZERO,
            totals(6),
            BUDGET,
            || RunContext {
                gpu_clock: "submit-to-done",
                ..context()
            },
        );
        let recorded = log.stop_recording().unwrap();
        let lines: Vec<&str> = recorded.csv.lines().collect();
        let column = |name: &str| {
            let at = COLUMNS.iter().position(|column| *column == name).unwrap();
            lines[1].split(',').nth(at).unwrap().to_string()
        };
        assert_eq!(column("gpu_clock"), "submit-to-done");
        assert_eq!(column("gpu_reference_ms"), "0.500");
        assert_eq!(column("gpu_tail_ms"), "1.000");
        assert_eq!(column("gpu_edges_ms"), "1.000");
        assert_eq!(column("gpu_composite_ms"), "0.500");
        assert_eq!(column("gpu_ms"), "3.000");
    }

    #[test]
    fn a_recorded_row_carries_the_session_counts_since_the_last_row() {
        let session = |frame: u64| SessionTotals {
            delay: 6,
            ping_ms: Some(80),
            ran: frame,
            waited: frame / 2,
            skipped: frame / 3,
            repaid: frame / 4,
            ahead: 1.25,
            watching: false,
        };
        let start = Instant::now();
        let mut log = FrameLog::default();
        log.note(start, Duration::ZERO, totals(0), BUDGET, context);
        log.start_recording(start);
        log.note(
            start + Duration::from_millis(17),
            Duration::ZERO,
            Totals {
                session: Some(session(4)),
                ..totals(1)
            },
            BUDGET,
            context,
        );
        log.note(
            start + Duration::from_millis(34),
            Duration::ZERO,
            Totals {
                session: Some(session(12)),
                ..totals(2)
            },
            BUDGET,
            context,
        );
        log.note(
            start + Duration::from_millis(51),
            Duration::ZERO,
            totals(3),
            BUDGET,
            context,
        );
        let recorded = log.stop_recording().unwrap();
        let lines: Vec<&str> = recorded.csv.lines().collect();
        let row = |line: usize| {
            let at = COLUMNS
                .iter()
                .position(|column| *column == "delay_frames")
                .unwrap();
            lines[line].split(',').skip(at).take(8).collect::<Vec<_>>()
        };
        assert_eq!(row(1), ["6", "80", "4", "2", "1", "1", "1.25", "off"]);
        assert_eq!(row(2), ["6", "80", "8", "4", "3", "2", "1.25", "off"]);
        assert_eq!(row(3), ["", "", "", "", "", "", "", ""]);
    }

    #[test]
    fn a_recorded_row_carries_the_present_wait_ui_shapes_and_events_since_the_last_row() {
        let start = Instant::now();
        let mut log = FrameLog::default();
        log.note(start, Duration::ZERO, totals(0), BUDGET, context);
        log.facts(RowFacts {
            events: vec!["lost".to_string()],
            ..RowFacts::default()
        });
        log.start_recording(start);
        log.facts(RowFacts {
            cpu: Some(Duration::from_millis(9)),
            ui_shapes: Some(40),
            events: vec!["pause".to_string()],
            ..RowFacts::default()
        });
        log.facts(RowFacts {
            started: Some(start + Duration::from_millis(3)),
            cpu: Some(Duration::from_millis(2)),
            ui_shapes: Some(412),
            gpu_calibration: Some(Duration::from_micros(1500)),
            events: vec!["settings: video, audio".to_string()],
            tape_frame: Some(7),
        });
        log.note_power(start + Duration::from_millis(5), "fair", true);
        log.note(
            start + Duration::from_millis(5),
            Duration::ZERO,
            totals(1),
            BUDGET,
            context,
        );
        log.note(
            start + Duration::from_millis(10),
            Duration::ZERO,
            totals(2),
            BUDGET,
            context,
        );
        let recorded = log.stop_recording().unwrap();
        let lines: Vec<&str> = recorded.csv.lines().collect();
        assert!(
            lines[1].ends_with(
                ",3.000,412,2.000,1.000,0.250,80.000,1.000,0.000,1.500,,,,,,,,,\"pause;settings: video, audio\",7"
            ),
            "{}",
            lines[1]
        );
        assert!(
            lines[2].ends_with(",,,,0.250,80.000,1.000,0.000,,,,,,,,,,,"),
            "{}",
            lines[2]
        );
        assert_eq!(recorded.length, Duration::from_millis(10));
        assert_eq!(recorded.last_context, Some(context()));
        assert_eq!(
            recorded.power,
            [PowerSample {
                at: Duration::from_millis(5),
                thermal: "fair",
                low_power: true,
            }]
        );
    }
}
