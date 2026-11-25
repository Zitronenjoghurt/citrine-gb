use crate::app::report::{Hand, Outgoing};
use crate::app::ui_state::settings::Settings;
use crate::emulator::Emulator;
use crate::frame_log::{FrameLog, PowerSample, Recorded, RowFacts, RunContext};
use crate::game_report::{FEATURES, console_name, game_code, shareable_settings};
use crate::media::{MediaId, MediaStore};
use crate::storage::now_unix;
use crate::utils::file_channels::FileChannels;
use citrine_gb::console::Console;
use citrine_gb_replay::Perf;
use serde::Serialize;
use std::io::Write;
use std::time::Duration;
use web_time::Instant;

pub const FRAMES: &str = "frames.csv";
pub const INFO: &str = "info.json";
const CHECK_EVERY: Duration = Duration::from_secs(1);

const NOTES: [&str; 9] = [
    "frames.csv has one row per app update; present_wait_ms is the time between updates minus the app's CPU time of the update before, so it holds the wait for the screen",
    "draw_ms is the CPU time after the update before (ending the UI pass, tessellating, painting); swap_wait_ms is the rest of the time from that update's end to this update's start: the wait for a free swap chain image, the submit and the present, or a sleep until the next repaint",
    "console_wait_ms is the time this update waited for the console thread's frame",
    "audio_queued_ms is the sound left in the queue after the device's last read; audio_underrun_ms is silence the device played since the last row because the queue ran dry; audio_overrun_ms is sound dropped since the last row because the queue was full or held too much",
    "ui_shapes counts the shapes the UI painted in that update",
    "in a play session, delay_frames is the input delay, ping_ms the median round trip, ahead_frames how far this side runs ahead of the friend, watching whether the friend's screen is shown; ran_frames, waited_frames, skipped_frames and repaid_frames count since the last row the frames the session ran, waited on the friend's input, skipped to let the friend catch up, and ran extra to make up a wait",
    "gpu_calibration_ms is the GPU time of one fixed compute dispatch (32 x 64 threads, 1024 xorshift steps each), sent once a second after a present while a game runs; it lands in the row of the update that read it back",
    "refresh_hz is the refresh rate the OS reports for the monitor the window is on, and refresh_from names the call it came from (windows_display_config, windows_display_settings, mac_display_mode, mac_screen_maximum), or estimate when the OS gave none and refresh_hz is the app's own estimate; estimated_hz is always that estimate, from the app's update times; paced_to_monitor is true when presents came faster than the monitor refreshes (vsync following another monitor) and the app timed its updates to the monitor's rate instead",
    "tape_frame is the recording's frame after that update, counted from its start; it steps back after a rewind and stays blank in a video-only recording",
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

pub struct Begun {
    unix: u64,
    gpu: Option<Gpu>,
    game: Option<Game>,
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
                gpu: gpu_of(frame),
                game: Game::of(emulator),
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
            tape_frame: emulator.tape_frame(),
        });
    }

    pub fn take_begun(&mut self) -> Option<Begun> {
        self.begun.take()
    }

    pub fn finish(
        &mut self,
        recorded: Recorded,
        begun: Option<Begun>,
        ctx: &egui::Context,
        emulator: &mut Emulator,
        settings: &Settings,
    ) -> Perf {
        self.begun = None;
        self.seen = None;
        self.settings = None;
        self.checked = None;
        let info = RunInfo::gather(&recorded, begun.as_ref(), ctx, emulator, settings);
        let gpu = begun.and_then(|begun| begun.gpu);
        let info = RunInfo { gpu, ..info };
        Perf {
            frames: recorded.csv.into_bytes(),
            info: serde_json::to_vec_pretty(&info).unwrap_or_default(),
            gpu: None,
        }
    }
}

pub fn cropped_perf(perf: &Perf, from: u64, to: u64) -> Option<Perf> {
    let csv = std::str::from_utf8(&perf.frames).ok()?;
    let cropped = crate::frame_log::tape_times::crop(csv, from, to)?;
    let info = serde_json::from_slice::<serde_json::Value>(&perf.info)
        .ok()
        .map(|mut info| {
            if let Some(span) = info
                .get_mut("recording")
                .and_then(serde_json::Value::as_object_mut)
            {
                if let Some(started) = span.get("started_unix").and_then(serde_json::Value::as_u64)
                {
                    let shifted = started + cropped.began_s.max(0.0).round() as u64;
                    span.insert("started_unix".into(), shifted.into());
                }
                span.insert("length_s".into(), cropped.length_s.into());
                span.insert("rows".into(), cropped.rows.into());
            }
            if let Some(samples) = info
                .get_mut("power")
                .and_then(|power| power.get_mut("samples"))
                .and_then(serde_json::Value::as_array_mut)
            {
                let span = cropped.began_s..=cropped.began_s + cropped.length_s;
                samples.retain_mut(|sample| {
                    let Some(time) = sample.get("time_s").and_then(serde_json::Value::as_f64)
                    else {
                        return false;
                    };
                    let inside = span.contains(&time);
                    if inside && let Some(sample) = sample.as_object_mut() {
                        let shifted = ((time - cropped.began_s) * 1000.0).round() / 1000.0;
                        sample.insert("time_s".into(), shifted.into());
                    }
                    inside
                });
            }
            serde_json::to_vec_pretty(&info).unwrap_or_default()
        })
        .unwrap_or_else(|| perf.info.clone());
    Some(Perf {
        frames: cropped.csv.into_bytes(),
        info,
        gpu: None,
    })
}

pub fn frame_times_zip(replay: &[u8]) -> Result<Vec<u8>, String> {
    let replay = citrine_gb_replay::Replay::read(replay).map_err(|err| err.to_string())?;
    let perf = replay
        .perf
        .ok_or_else(|| "the recording has no performance data".to_string())?;
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in [(FRAMES, &perf.frames), (INFO, &perf.info)] {
        zip.start_file(name, options)
            .map_err(|err| err.to_string())?;
        zip.write_all(bytes).map_err(|err| err.to_string())?;
    }
    zip.finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|err| err.to_string())
}

pub fn frame_times_name(id: &MediaId) -> String {
    let stem = std::path::Path::new(id.file_name())
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    format!("{stem} performance.zip")
}

pub fn export_frame_times(store: &MediaStore, id: &MediaId, files: &FileChannels) {
    let hand = Hand::of(files, "performance data");
    let spawner = files.spawner.clone();
    let file_name = frame_times_name(id);
    store.read_bytes(id, move |bytes| {
        spawner.run("frame times", move || {
            let zipped = bytes
                .ok_or_else(|| "the file is missing".to_string())
                .and_then(|bytes| frame_times_zip(&bytes));
            match zipped {
                Ok(zipped) => hand.over(Outgoing {
                    file_name,
                    bytes: zipped,
                    filter: "Performance data",
                    extension: "zip",
                    mime: "application/zip",
                }),
                Err(err) => hand.failed(err),
            }
        });
    });
}

pub fn video_name(id: &MediaId) -> String {
    let stem = std::path::Path::new(id.file_name())
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    format!("{stem}.mp4")
}

pub fn export_video(store: &MediaStore, id: &MediaId, files: &FileChannels) {
    let hand = Hand::of(files, "video");
    let spawner = files.spawner.clone();
    let file_name = video_name(id);
    store.read_bytes(id, move |bytes| {
        spawner.run("video", move || {
            let video = bytes
                .ok_or_else(|| "the file is missing".to_string())
                .and_then(|bytes| {
                    citrine_gb_replay::Replay::video(&bytes).map_err(|err| err.to_string())
                })
                .and_then(|video| video.ok_or_else(|| "the recording has no video".to_string()));
            match video {
                Ok(video) => hand.over(Outgoing {
                    file_name,
                    bytes: video,
                    filter: "MP4 video",
                    extension: "mp4",
                    mime: "video/mp4",
                }),
                Err(err) => hand.failed(err),
            }
        });
    });
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
    refresh_from: &'static str,
    estimated_hz: Option<f64>,
    paced_to_monitor: bool,
    window: Window,
    present_mode: Option<String>,
    max_frame_latency: Option<u32>,
}

impl Game {
    fn of(emulator: &mut Emulator) -> Option<Self> {
        emulator.has_game().then(|| {
            let machine = &emulator.core_mut().machine;
            let kind = machine.kind();
            Self {
                console: console_name(kind),
                title: machine.title(),
                code: game_code(kind, &machine.rom_bytes()),
                crc32: format!("{:08x}", machine.rom_id().crc32),
            }
        })
    }
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
        let estimated_hz = emulator
            .frame_budget()
            .refresh
            .filter(|period| !period.is_zero())
            .map(|period| (10.0 / period.as_secs_f64()).round() / 10.0);
        let shown = emulator.display();
        let (refresh_hz, refresh_from) = refresh(shown, estimated_hz);
        let game = match begun {
            Some(begun) => begun.game.clone(),
            None => Game::of(emulator),
        };
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
                refresh_from,
                estimated_hz,
                paced_to_monitor: shown.paced,
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
            notes: NOTES,
        }
    }
}

fn refresh(
    shown: crate::emulator::Display,
    estimated_hz: Option<f64>,
) -> (Option<f64>, &'static str) {
    match shown.monitor {
        Some(monitor) => (
            Some((monitor.hz * 1000.0).round() / 1000.0),
            monitor.source.name(),
        ),
        None => (estimated_hz, "estimate"),
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod calibration;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::monitor::{MonitorRate, RateSource};
    use crate::app::tests::{load_game, phone_app};
    use crate::frame_log::COLUMNS;

    #[test]
    fn the_refresh_rate_comes_from_the_monitor_or_else_the_estimate() {
        let mac = crate::emulator::Display {
            monitor: Some(MonitorRate {
                hz: 120.0,
                source: RateSource::MacDisplayMode,
            }),
            paced: false,
        };
        assert_eq!(refresh(mac, Some(119.9)), (Some(120.0), "mac_display_mode"));
        assert_eq!(
            refresh(crate::emulator::Display::default(), Some(59.9)),
            (Some(59.9), "estimate")
        );
        assert_eq!(
            refresh(crate::emulator::Display::default(), None),
            (None, "estimate")
        );
    }

    #[test]
    fn the_frame_times_track_holds_the_frames_and_the_run_info() {
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
        app.emulator.set_display(crate::emulator::Display {
            monitor: Some(MonitorRate {
                hz: 59.951_02,
                source: RateSource::WindowsDisplayConfig,
            }),
            paced: true,
        });
        let begun = watch.take_begun();
        let perf = watch.finish(recorded, begun, &ctx, &mut app.emulator, &app.ui.settings);
        assert_eq!(perf.gpu, None);

        let csv = String::from_utf8(perf.frames).unwrap();
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

        assert_eq!(column(lines[2], "tape_frame"), "", "nothing on the tape");

        let info: serde_json::Value = serde_json::from_slice(&perf.info).unwrap();
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
        assert_eq!(info["display"]["refresh_hz"], 59.951);
        assert_eq!(info["display"]["refresh_from"], "windows_display_config");
        assert_eq!(info["display"]["paced_to_monitor"], true);
        assert!(info.pointer("/display/estimated_hz").is_some());
        assert_eq!(info["recording"]["rows"], 2);
        assert!(info.pointer("/states").is_none());
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
