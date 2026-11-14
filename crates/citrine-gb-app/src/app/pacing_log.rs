use crate::emulator::load::{GPU_PASSES, GpuClockKind, GpuTotals, LoadReading};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Duration;
use web_time::Instant;

pub const VAR: &str = "CITRINE_FRAME_LOG";
const FILE_NAME: &str = "citrine-frame-log.csv";
const HEADER: &str = "second,app_frames,interval_ms,interval_max_ms,cpu_ms,cpu_max_ms,wait_ms,wait_max_ms,update_ms,update_max_ms,game_frames,shown_twice_per_s,dropped_per_s,pacer_hz,upscaling,gpu_clock,gpu_frames,gpu_ms,gpu_done_ms,gpu_work_ms,surface_errors,drawable_w,drawable_h,pixels_per_point,focused,fullscreen,present_mode,frame_latency,gpu_reference_ms,gpu_upscaled_ms,gpu_centers_ms,gpu_edges_ms,gpu_finish_ms,gpu_present_ms,gpu_composite_ms";

#[derive(Debug, Clone, Copy)]
pub struct PacingSample {
    pub start: Instant,
    pub cpu: Option<f32>,
    pub update: Duration,
    pub game_frames: u128,
    pub shown_twice: f64,
    pub dropped: f64,
    pub refresh_period: Option<f64>,
    pub load: LoadReading,
    pub surface_errors: u64,
    pub window: WindowFacts,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowFacts {
    pub drawable: Option<[u32; 2]>,
    pub pixels_per_point: Option<f32>,
    pub focused: Option<bool>,
    pub fullscreen: Option<bool>,
}

impl WindowFacts {
    pub fn of(ctx: &egui::Context) -> Self {
        ctx.input(|input| {
            let viewport = input.viewport();
            let pixels_per_point = viewport.native_pixels_per_point;
            Self {
                drawable: viewport
                    .inner_rect
                    .zip(pixels_per_point)
                    .map(|(rect, ppp)| {
                        let size = rect.size() * ppp;
                        [size.x.round() as u32, size.y.round() as u32]
                    }),
                pixels_per_point,
                focused: viewport.focused,
                fullscreen: viewport.fullscreen,
            }
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Spread {
    sum: Duration,
    max: Duration,
    count: u32,
}

impl Spread {
    fn add(&mut self, time: Duration) {
        self.sum += time;
        self.max = self.max.max(time);
        self.count += 1;
    }

    fn mean_ms(&self) -> String {
        if self.count == 0 {
            return String::new();
        }
        ms(self.sum / self.count)
    }

    fn max_ms(&self) -> String {
        if self.count == 0 {
            return String::new();
        }
        ms(self.max)
    }
}

#[derive(Debug, Default)]
struct Second {
    frames: u32,
    interval: Spread,
    cpu: Spread,
    wait: Spread,
    update: Spread,
}

pub struct PacingLog<W: Write> {
    out: W,
    surface: String,
    started: Option<Instant>,
    second: u64,
    last: Option<PacingSample>,
    first_of_second: Option<PacingSample>,
    now: Second,
}

impl PacingLog<BufWriter<File>> {
    pub fn from_env() -> Option<Self> {
        let value = std::env::var_os(VAR)?;
        let path = match value.to_str() {
            Some("" | "1") => default_path()?,
            _ => PathBuf::from(value),
        };
        match File::create(&path) {
            Ok(file) => {
                log::info!("pacing log: writing {}", path.display());
                let options = crate::platform::wgpu_options();
                let surface = format!(
                    "{:?},{}",
                    options.present_mode,
                    options
                        .desired_maximum_frame_latency
                        .map_or_else(|| "default".to_owned(), |latency| latency.to_string())
                );
                PacingLog::new(BufWriter::new(file), surface).ok()
            }
            Err(err) => {
                log::error!("pacing log: can't create {}: {err}", path.display());
                None
            }
        }
    }
}

fn default_path() -> Option<PathBuf> {
    let dirs = directories::UserDirs::new()?;
    let dir = dirs.desktop_dir().unwrap_or(dirs.home_dir());
    Some(dir.join(FILE_NAME))
}

impl<W: Write> PacingLog<W> {
    pub fn new(mut out: W, surface: String) -> std::io::Result<Self> {
        writeln!(out, "{HEADER}")?;
        out.flush()?;
        Ok(Self {
            out,
            surface,
            started: None,
            second: 0,
            last: None,
            first_of_second: None,
            now: Second::default(),
        })
    }

    pub fn frame(&mut self, sample: PacingSample) {
        let started = *self.started.get_or_insert(sample.start);
        let second = sample.start.saturating_duration_since(started).as_secs();
        if second != self.second {
            if let Some(last) = self.last {
                self.write_row(last);
            }
            self.second = second;
            self.now = Second::default();
            self.first_of_second = self.last;
        }
        if let Some(last) = self.last {
            let interval = sample.start.saturating_duration_since(last.start);
            self.now.interval.add(interval);
            if let Some(cpu) = sample.cpu {
                let cpu = Duration::from_secs_f32(cpu.max(0.0));
                self.now.cpu.add(cpu);
                self.now.wait.add(interval.saturating_sub(cpu));
            }
        }
        self.now.update.add(sample.update);
        self.now.frames += 1;
        self.last = Some(sample);
    }

    fn write_row(&mut self, last: PacingSample) {
        let before = self.first_of_second;
        let gpu_before = before.map_or_else(GpuTotals::default, |before| before.load.gpu);
        let gpu = last.load.gpu;
        let gpu_frames = gpu.frames.saturating_sub(gpu_before.frames);
        let gpu_time = gpu.time.saturating_sub(gpu_before.time);
        let gpu_done = gpu.done.saturating_sub(gpu_before.done);
        let mut passes = vec![String::new(); GPU_PASSES];
        let (gpu_ms, gpu_done_ms) = match u32::try_from(gpu_frames) {
            Ok(0) | Err(_) => (String::new(), String::new()),
            Ok(frames) => {
                for ((out, now), before) in passes.iter_mut().zip(gpu.passes).zip(gpu_before.passes)
                {
                    *out = ms(now.saturating_sub(before) / frames);
                }
                (ms(gpu_time / frames), ms(gpu_done / frames))
            }
        };
        let clock = match gpu.kind {
            Some(GpuClockKind::Stamps) => "stamps",
            Some(GpuClockKind::SubmitToDone) => "submit-to-done",
            None => "",
        };
        let game_frames = before.map_or(0, |before| {
            last.game_frames.saturating_sub(before.game_frames)
        });
        let now = &self.now;
        let window = last.window;
        let row = format!(
            "{},{},{},{},{},{},{},{},{},{},{},{:.1},{:.1},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            self.second,
            now.frames,
            now.interval.mean_ms(),
            now.interval.max_ms(),
            now.cpu.mean_ms(),
            now.cpu.max_ms(),
            now.wait.mean_ms(),
            now.wait.max_ms(),
            now.update.mean_ms(),
            now.update.max_ms(),
            game_frames,
            last.shown_twice,
            last.dropped,
            last.refresh_period
                .filter(|period| *period > 0.0)
                .map(|period| format!("{:.1}", 1.0 / period))
                .unwrap_or_default(),
            u8::from(last.load.upscaling),
            clock,
            gpu_frames,
            gpu_ms,
            gpu_done_ms,
            gpu.work.map(ms).unwrap_or_default(),
            last.surface_errors,
            window
                .drawable
                .map(|[w, _]| w.to_string())
                .unwrap_or_default(),
            window
                .drawable
                .map(|[_, h]| h.to_string())
                .unwrap_or_default(),
            window
                .pixels_per_point
                .map(|ppp| format!("{ppp:.2}"))
                .unwrap_or_default(),
            flag(window.focused),
            flag(window.fullscreen),
            self.surface,
            passes.join(","),
        );
        if writeln!(self.out, "{row}")
            .and_then(|()| self.out.flush())
            .is_err()
        {
            log::error!("pacing log: write failed");
        }
    }
}

fn flag(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "1",
        Some(false) => "0",
        None => "",
    }
}

fn ms(time: Duration) -> String {
    format!("{:.3}", time.as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(start: Instant, ms_in: u64, gpu_frames: u64, gpu_ms: u64) -> PacingSample {
        PacingSample {
            start: start + Duration::from_millis(ms_in),
            cpu: Some(0.002),
            update: Duration::from_millis(1),
            game_frames: u128::from(ms_in / 16),
            shown_twice: 0.0,
            dropped: 0.0,
            refresh_period: Some(1.0 / 120.0),
            surface_errors: 0,
            window: WindowFacts {
                drawable: Some([2560, 1600]),
                pixels_per_point: Some(2.0),
                focused: Some(true),
                fullscreen: Some(false),
            },
            load: LoadReading {
                upscaling: true,
                gpu: GpuTotals {
                    kind: Some(GpuClockKind::Stamps),
                    time: Duration::from_millis(gpu_ms),
                    done: Duration::from_millis(gpu_ms * 3),
                    frames: gpu_frames,
                    work: Some(Duration::from_millis(4)),
                    passes: [0, 0, 1, 2, 0, 0, 1]
                        .map(|share| Duration::from_millis(gpu_ms * share / 5)),
                },
                ..LoadReading::default()
            },
        }
    }

    #[test]
    fn a_second_of_frames_writes_one_row_with_intervals_waits_and_gpu_time() {
        let start = Instant::now();
        let mut log = PacingLog::new(Vec::new(), "AutoVsync,default".to_owned()).unwrap();
        for frame in 0..=100u64 {
            let at = frame * 10;
            log.frame(sample(start, at, frame / 2, frame / 2 * 5));
        }
        let text = String::from_utf8(log.out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert_eq!(lines[0], HEADER);
        let row: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(row.len(), HEADER.split(',').count());
        assert_eq!(row[0], "0");
        assert_eq!(row[1], "100");
        assert_eq!(row[2], "10.000");
        assert_eq!(row[4], "2.000");
        assert_eq!(row[6], "8.000");
        assert_eq!(row[13], "120.0");
        assert_eq!(row[15], "stamps");
        assert_eq!(row[16], "49");
        assert_eq!(row[17], "5.000");
        assert_eq!(row[18], "15.000");
        assert_eq!(row[19], "4.000");
        assert_eq!(
            row[21..28],
            ["2560", "1600", "2.00", "1", "0", "AutoVsync", "default"]
        );
        assert_eq!(
            row[28..],
            [
                "0.000", "0.000", "1.000", "2.000", "0.000", "0.000", "1.000"
            ]
        );
    }

    #[test]
    fn a_long_frame_shows_as_the_max_interval_and_wait() {
        let start = Instant::now();
        let mut log = PacingLog::new(Vec::new(), "AutoVsync,default".to_owned()).unwrap();
        for at in [0, 10, 20, 270, 280, 1000, 1010] {
            log.frame(sample(start, at, 0, 0));
        }
        let text = String::from_utf8(log.out).unwrap();
        let row: Vec<&str> = text.lines().nth(1).unwrap().split(',').collect();
        assert_eq!(row[3], "250.000");
        assert_eq!(row[7], "248.000");
        assert_eq!(row[17], "");
    }
}
