use crate::app::ui_state::settings::Settings;
use crate::emulator::Emulator;
use crate::frame_log::{FrameLog, PowerSample, Recorded, RowFacts, RunContext};
use crate::game_report::{FEATURES, console_name, game_code, shareable_settings};
use crate::storage::now_unix;
use citrine_gb::console::Console;
use citrine_gb::persistence::state::StateBody;
use serde::Serialize;
use std::io::Write;
use std::time::Duration;
use web_time::Instant;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub const FRAMES: &str = "frames.csv";
pub const INFO: &str = "info.json";
pub const STATE_START: &str = "state-start.snap";
pub const STATE_END: &str = "state-end.snap";
const CHECK_EVERY: Duration = Duration::from_secs(1);

const NOTES: [&str; 9] = [
    "frames.csv has one row per app update; present_wait_ms is the time between updates minus the app's CPU time of the update before, so it holds the wait for the screen",
    "draw_ms is the CPU time after the update before (ending the UI pass, tessellating, painting); swap_wait_ms is the rest of the time from that update's end to this update's start: the wait for a free swap chain image, the submit and the present, or a sleep until the next repaint",
    "console_wait_ms is the time this update waited for the console thread's frame",
    "audio_queued_ms is the sound left in the queue after the device's last read; audio_underrun_ms is silence the device played since the last row because the queue ran dry; audio_overrun_ms is sound dropped since the last row because the queue was full or held too much",
    "ui_shapes counts the shapes the UI painted in that update",
    "in a play session, delay_frames is the input delay, ping_ms the median round trip, ahead_frames how far this side runs ahead of the friend, watching whether the friend's screen is shown; ran_frames, waited_frames, skipped_frames and repaid_frames count since the last row the frames the session ran, waited on the friend's input, skipped to let the friend catch up, and ran extra to make up a wait",
    "gpu_calibration_ms is the GPU time of one fixed compute dispatch (32 x 64 threads, 1024 xorshift steps each), sent once a second after a present while a game runs; it lands in the row of the update that read it back",
    "refresh_hz is estimated from the app's own update times; the OS refresh rate is not read",
    "the .snap files are save states in the app's slot format, taken when the recording started and stopped",
];

#[derive(Debug, Clone, PartialEq)]
struct Seen {
    running: bool,
    game: String,
    frames: u128,
    size: Option<[u32; 2]>,
    pixels_per_point: Option<f32>,
    monitor: Option<[u32; 2]>,
    focused: Option<bool>,
}

impl Seen {
    fn of(ctx: &egui::Context, emulator: &Emulator) -> Self {
        let window = Window::of(ctx);
        Self {
            running: emulator.is_running(),
            game: emulator.game_name().to_string(),
            frames: emulator.total_frames,
            size: window.pixels,
            pixels_per_point: window.pixels_per_point,
            monitor: window.monitor_points,
            focused: window.focused,
        }
    }

    fn events(&self, now: &Self) -> Vec<String> {
        let mut events = Vec::new();
        if self.game != now.game || now.frames < self.frames {
            events.push("game load".to_string());
        } else if self.running != now.running {
            events.push(if now.running { "resume" } else { "pause" }.to_string());
        }
        if self.monitor != now.monitor || self.pixels_per_point != now.pixels_per_point {
            events.push("display change".to_string());
        } else if self.size != now.size {
            events.push("window resize".to_string());
        }
        if self.focused != now.focused {
            match now.focused {
                Some(false) => events.push("focus lost".to_string()),
                Some(true) => events.push("focus back".to_string()),
                None => {}
            }
        }
        events
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
struct Window {
    points: Option<[f32; 2]>,
    pixels: Option<[u32; 2]>,
    pixels_per_point: Option<f32>,
    monitor_points: Option<[u32; 2]>,
    fullscreen: Option<bool>,
    focused: Option<bool>,
}

impl Window {
    fn of(ctx: &egui::Context) -> Self {
        ctx.input(|input| {
            let viewport = input.viewport();
            let pixels_per_point = viewport.native_pixels_per_point;
            Self {
                points: viewport.inner_rect.map(|rect| rect.size().into()),
                pixels: viewport
                    .inner_rect
                    .zip(pixels_per_point)
                    .map(|(rect, ppp)| {
                        let size = rect.size() * ppp;
                        [size.x.round() as u32, size.y.round() as u32]
                    }),
                pixels_per_point,
                monitor_points: viewport
                    .monitor_size
                    .map(|size| [size.x.round() as u32, size.y.round() as u32]),
                fullscreen: viewport.fullscreen,
                focused: viewport.focused,
            }
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
struct Gpu {
    adapter: String,
    backend: String,
    device_type: String,
    driver: String,
    driver_info: String,
}

struct Begun {
    unix: u64,
    state: Option<StateBody>,
    gpu: Option<Gpu>,
}

#[derive(Default)]
pub struct RecordWatch {
    seen: Option<Seen>,
    settings: Option<serde_json::Value>,
    checked: Option<Instant>,
    begun: Option<Begun>,
    #[cfg(not(target_arch = "wasm32"))]
    calibration: calibration::Calibration,
}

impl RecordWatch {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn calibrate(&mut self, frame: &eframe::Frame, emulator: &Emulator, log: &FrameLog) {
        if !log.is_recording() || !emulator.is_running() {
            return;
        }
        if let Some(state) = frame.wgpu_render_state() {
            self.calibration
                .send(&state.device, &state.queue, Instant::now());
        }
    }

    pub fn follow(
        &mut self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        started: Instant,
        emulator: &mut Emulator,
        settings: &Settings,
        log: &mut FrameLog,
    ) {
        if !log.is_recording() {
            self.seen = None;
            self.settings = None;
            self.checked = None;
            self.begun = None;
            return;
        }
        if self.begun.is_none() {
            self.begun = Some(Begun {
                unix: now_unix(),
                state: state_of(emulator),
                gpu: gpu_of(frame),
            });
        }
        let now = Seen::of(ctx, emulator);
        let mut events = self
            .seen
            .replace(now.clone())
            .map(|seen| seen.events(&now))
            .unwrap_or_default();
        let at = Instant::now();
        if self
            .checked
            .is_none_or(|checked| at.saturating_duration_since(checked) >= CHECK_EVERY)
        {
            self.checked = Some(at);
            let current = shareable_settings(settings);
            if let Some(changed) = changed_sections(self.settings.as_ref(), current.as_ref()) {
                events.push(format!("settings: {changed}"));
            }
            self.settings = current;
            if let Some((thermal, low_power)) = crate::platform::power() {
                log.note_power(at, thermal, low_power);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let gpu_calibration = frame
            .wgpu_render_state()
            .and_then(|state| self.calibration.take(&state.device));
        #[cfg(target_arch = "wasm32")]
        let gpu_calibration = None;
        log.facts(RowFacts {
            started: Some(started),
            cpu: frame
                .info()
                .cpu_usage
                .map(|cpu| Duration::from_secs_f32(cpu.max(0.0))),
            ui_shapes: Some(shapes(ctx)),
            gpu_calibration,
            events,
        });
    }

    pub fn finish(
        &mut self,
        recorded: Recorded,
        ctx: &egui::Context,
        emulator: &mut Emulator,
        settings: &Settings,
    ) -> Pack {
        let begun = self.begun.take();
        self.seen = None;
        self.settings = None;
        self.checked = None;
        let info = RunInfo::gather(&recorded, begun.as_ref(), ctx, emulator, settings);
        let (start, gpu) = begun.map_or((None, None), |begun| (begun.state, begun.gpu));
        Pack {
            csv: recorded.csv,
            info: RunInfo { gpu, ..info },
            start,
            end: state_of(emulator),
        }
    }
}

fn state_of(emulator: &mut Emulator) -> Option<StateBody> {
    if !emulator.has_game() {
        return None;
    }
    emulator.core_mut().machine.state_body(now_unix()).ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn gpu_of(frame: &eframe::Frame) -> Option<Gpu> {
    let info = frame.wgpu_render_state()?.adapter.get_info();
    Some(Gpu {
        adapter: info.name,
        backend: format!("{:?}", info.backend),
        device_type: format!("{:?}", info.device_type),
        driver: info.driver,
        driver_info: info.driver_info,
    })
}

#[cfg(target_arch = "wasm32")]
fn gpu_of(_frame: &eframe::Frame) -> Option<Gpu> {
    None
}

fn shapes(ctx: &egui::Context) -> usize {
    let layers: Vec<egui::LayerId> = ctx.memory(|memory| memory.layer_ids().collect());
    ctx.graphics(|graphics| {
        layers
            .iter()
            .filter_map(|layer| graphics.get(*layer))
            .map(|list| list.next_idx().0)
            .sum()
    })
}

fn changed_sections(
    before: Option<&serde_json::Value>,
    now: Option<&serde_json::Value>,
) -> Option<String> {
    let (before, now) = (before?.as_object()?, now?.as_object()?);
    let changed: Vec<&str> = now
        .iter()
        .filter(|(key, value)| before.get(*key) != Some(value))
        .map(|(key, _)| key.as_str())
        .collect();
    (!changed.is_empty()).then(|| changed.join(" "))
}

#[derive(Debug, Clone, Serialize)]
struct App {
    version: &'static str,
    commit: &'static str,
    profile: &'static str,
    target: String,
    features: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
struct System {
    os: String,
    arch: &'static str,
    cpu: Option<String>,
    cpu_threads: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
struct Display {
    refresh_hz: Option<f64>,
    window: Window,
    present_mode: Option<String>,
    max_frame_latency: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct Game {
    console: &'static str,
    title: String,
    code: Option<String>,
    crc32: String,
}

#[derive(Debug, Clone, Serialize)]
struct Speed {
    renderer: &'static str,
    resolution_3d: String,
    smooth_edges: String,
    jit: bool,
    gpu_clock: &'static str,
}

impl From<RunContext> for Speed {
    fn from(context: RunContext) -> Self {
        Self {
            renderer: context.renderer,
            resolution_3d: context.resolution,
            smooth_edges: context.smooth_edges,
            jit: context.jit,
            gpu_clock: context.gpu_clock,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct Span {
    started_unix: Option<u64>,
    length_s: f64,
    rows: usize,
}

#[derive(Debug, Clone, Serialize)]
struct Power {
    source: Option<&'static str>,
    missing: Option<&'static str>,
    samples: Vec<PowerSample>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunInfo {
    app: App,
    system: System,
    gpu: Option<Gpu>,
    display: Display,
    game: Option<Game>,
    speed: Speed,
    settings: Option<serde_json::Value>,
    recording: Span,
    power: Power,
    states: Vec<&'static str>,
    notes: [&'static str; 9],
}

impl RunInfo {
    fn gather(
        recorded: &Recorded,
        begun: Option<&Begun>,
        ctx: &egui::Context,
        emulator: &mut Emulator,
        settings: &Settings,
    ) -> Self {
        let system = crate::controller_debug::SystemInfo::current(String::new());
        #[cfg(not(target_arch = "wasm32"))]
        let (present_mode, max_frame_latency) = {
            let options = crate::platform::wgpu_options();
            (
                Some(format!("{:?}", options.present_mode)),
                options.desired_maximum_frame_latency,
            )
        };
        #[cfg(target_arch = "wasm32")]
        let (present_mode, max_frame_latency) = (None, None);
        let refresh_hz = emulator
            .frame_budget()
            .refresh
            .filter(|period| !period.is_zero())
            .map(|period| (10.0 / period.as_secs_f64()).round() / 10.0);
        let game = emulator.has_game().then(|| {
            let machine = &emulator.core_mut().machine;
            let kind = machine.kind();
            Game {
                console: console_name(kind),
                title: machine.title(),
                code: game_code(kind, &machine.rom_bytes()),
                crc32: format!("{:08x}", machine.rom_id().crc32),
            }
        });
        let on_mac = crate::platform::power().is_some();
        Self {
            app: App {
                version: system.version,
                commit: env!("CITRINE_COMMIT"),
                profile: env!("CITRINE_PROFILE"),
                target: system.build,
                features: FEATURES
                    .iter()
                    .filter(|(_, on)| *on)
                    .map(|(name, _)| *name)
                    .collect(),
            },
            system: System {
                os: system.os,
                arch: std::env::consts::ARCH,
                cpu: crate::platform::cpu_name(),
                cpu_threads: std::thread::available_parallelism()
                    .ok()
                    .map(|threads| threads.get()),
            },
            gpu: None,
            display: Display {
                refresh_hz,
                window: Window::of(ctx),
                present_mode,
                max_frame_latency,
            },
            game,
            speed: recorded
                .last_context
                .clone()
                .unwrap_or_else(|| emulator.run_context())
                .into(),
            settings: shareable_settings(settings),
            recording: Span {
                started_unix: begun.map(|begun| begun.unix),
                length_s: recorded.length.as_secs_f64(),
                rows: recorded.frames,
            },
            power: Power {
                source: on_mac.then_some(
                    "NSProcessInfo thermalState and isLowPowerModeEnabled, once a second",
                ),
                missing: (!on_mac)
                    .then_some("only macOS reports the thermal state and Low Power Mode"),
                samples: recorded.power.clone(),
            },
            states: Vec::new(),
            notes: NOTES,
        }
    }
}

pub struct Pack {
    csv: String,
    info: RunInfo,
    start: Option<StateBody>,
    end: Option<StateBody>,
}

impl Pack {
    pub fn zip(mut self) -> std::io::Result<Vec<u8>> {
        let states: Vec<(&'static str, Vec<u8>)> =
            [(STATE_START, &self.start), (STATE_END, &self.end)]
                .into_iter()
                .filter_map(|(name, state)| {
                    let state = state.as_ref()?;
                    let packed = state.pack(state.quality(true)).ok()?;
                    Some((name, packed))
                })
                .collect();
        self.info.states = states.iter().map(|(name, _)| *name).collect();
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(FRAMES, deflated)?;
        zip.write_all(self.csv.as_bytes())?;
        zip.start_file(INFO, deflated)?;
        zip.write_all(&serde_json::to_vec_pretty(&self.info).map_err(std::io::Error::other)?)?;
        for (name, bytes) in states {
            zip.start_file(name, stored)?;
            zip.write_all(&bytes)?;
        }
        Ok(zip.finish()?.into_inner())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod calibration;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{load_game, phone_app};
    use crate::frame_log::COLUMNS;
    use std::io::{Cursor, Read};
    use zip::ZipArchive;

    fn read(zip: &mut ZipArchive<Cursor<Vec<u8>>>, name: &str) -> String {
        let mut text = String::new();
        zip.by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    }

    #[test]
    fn a_recording_zip_holds_the_frames_the_run_info_and_both_states() {
        let mut app = phone_app("record-zip");
        load_game(
            &mut app,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            None,
        );
        let was_running = app.emulator.is_running();
        let ctx = egui::Context::default();
        let frame = eframe::Frame::_new_kittest();
        let mut watch = RecordWatch::default();
        let mut log = FrameLog::default();
        let start = Instant::now();
        log.start_recording(start);
        for step in 0..3u64 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| ui.label("frames"));
                if step == 2 {
                    app.emulator.toggle_running();
                }
                watch.follow(
                    ctx,
                    &frame,
                    Instant::now(),
                    &mut app.emulator,
                    &app.ui.settings,
                    &mut log,
                );
                log.note(
                    start + Duration::from_millis(17 * step),
                    Duration::ZERO,
                    app.emulator.frame_totals(),
                    app.emulator.frame_budget(),
                    || app.emulator.run_context(),
                );
            });
        }
        let recorded = log.stop_recording().unwrap();
        let bytes = watch
            .finish(recorded, &ctx, &mut app.emulator, &app.ui.settings)
            .zip()
            .unwrap();
        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut names: Vec<String> = zip
            .file_names()
            .map(|name| name.unwrap().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, [FRAMES, INFO, STATE_END, STATE_START]);

        let csv = read(&mut zip, FRAMES);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], COLUMNS.join(","));
        assert_eq!(lines.len(), 3);
        let column = |line: &str, name: &str| {
            let at = COLUMNS.iter().position(|column| *column == name).unwrap();
            line.split(',').nth(at).unwrap().to_string()
        };
        assert!(column(lines[1], "ui_shapes").parse::<usize>().unwrap() > 0);
        assert_eq!(
            column(lines[2], "events"),
            if was_running { "pause" } else { "resume" }
        );

        let info: serde_json::Value = serde_json::from_str(&read(&mut zip, INFO)).unwrap();
        for path in [
            "/app/version",
            "/app/commit",
            "/app/profile",
            "/system/os",
            "/system/arch",
            "/display/window",
            "/game/title",
            "/game/crc32",
            "/speed/jit",
            "/settings/ds_3d",
            "/recording/rows",
            "/power/samples",
        ] {
            assert!(info.pointer(path).is_some(), "{path} missing");
        }
        assert_eq!(info["game"]["console"], "gbc");
        assert_eq!(info["recording"]["rows"], 2);
        assert_eq!(
            info.pointer("/states").unwrap(),
            &serde_json::json!([STATE_START, STATE_END])
        );
        if cfg!(target_os = "macos") {
            assert!(
                !info
                    .pointer("/power/samples")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        } else {
            assert!(info.pointer("/power/missing").unwrap().is_string());
        }
        for private in ["/settings/synced", "/settings/discord", "/game/sha1"] {
            assert!(info.pointer(private).is_none(), "{private} shared");
        }
    }
}
