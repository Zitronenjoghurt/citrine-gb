//! CLI entry point for the Citrine lab.
//!
//! Replays an input recording (or runs input-free) on Citrine and SameBoy, then scores the two
//! frame sequences with the requested metrics.

use anyhow::Context;
use citrine_gb::gb::GbModel;
use citrine_lab::emulators::{CitrineEmulator, SameBoyEmulator};
use citrine_lab::metric::FrameMetric;
use citrine_lab::metrics;
use citrine_lab::recording::Recording;
use citrine_lab::report::{ConsoleReporter, FrameDumper, JsonReporter, Reporter};
use citrine_lab::runner::run_streaming;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "citrine-gb-lab",
    about = "Compare Citrine against SameBoy frame by frame"
)]
struct Args {
    /// Path to the ROM to run.
    #[arg(long)]
    rom: PathBuf,

    /// Path to an input recording JSON. If omitted, both emulators run with no input.
    #[arg(long)]
    recording: Option<PathBuf>,

    /// Path to a boot ROM fed to BOTH emulators (recommended; SameBoy needs one to run).
    #[arg(long)]
    boot_rom: Option<PathBuf>,

    /// Number of frames to compare.
    #[arg(long, default_value_t = 600)]
    frames: usize,

    /// Comma-separated metrics: exact, px_match, mse, nmse, psnr, ssim.
    #[arg(long, default_value = "exact,px_match,mse,ssim")]
    metrics: String,

    /// Model to use when no recording is provided.
    #[arg(long, default_value = "dmg")]
    model: String,

    /// Directory to dump PNGs of diverging frames (reference/candidate/diff).
    #[arg(long)]
    dump_divergences: Option<PathBuf>,

    /// Max number of frames to dump (0 = no limit). Only applies with --dump-divergences.
    #[arg(long, default_value_t = 0)]
    max_dumps: usize,

    /// Dump every frame (not just diverging ones) with --dump-divergences.
    #[arg(long, default_value_t = false)]
    dump_all_frames: bool,

    /// Write the full report as JSON to this path.
    #[arg(long)]
    json: Option<PathBuf>,

    /// Compare raw emulator output instead of normalizing both to a canonical greyscale.
    #[arg(long, default_value_t = false)]
    raw: bool,

    /// Frame pairing: `cycle` (default) pairs frames by nearest emulated cycle, cancelling the
    /// ~1-frame phase offset from boot LCD transitions; `emission` pairs the Nth frame of each.
    #[arg(long, value_enum, default_value_t = AlignArg::Cycle)]
    align: AlignArg,

    /// Cycle-alignment tolerance, in frames: a frame matches if any candidate within ±N frames of
    /// the nearest-cycle one matches. `1` absorbs the irreducible sub-frame sampling skew between
    /// two independent emulators; `0` is strict. Ignored when `--align emission`.
    #[arg(long, default_value_t = 0)]
    tolerance: usize,
}

#[derive(Copy, Clone, Debug, clap::ValueEnum)]
enum AlignArg {
    Cycle,
    Emission,
}

impl From<AlignArg> for citrine_lab::runner::Alignment {
    fn from(a: AlignArg) -> Self {
        match a {
            AlignArg::Cycle => Self::Cycle,
            AlignArg::Emission => Self::Emission,
        }
    }
}

fn parse_model(s: &str) -> anyhow::Result<GbModel> {
    match s.to_ascii_lowercase().as_str() {
        "dmg" => Ok(GbModel::Dmg),
        "cgb" => Ok(GbModel::Cgb),
        other => anyhow::bail!("unknown model '{other}' (expected dmg or cgb)"),
    }
}

fn build_metrics(spec: &str) -> anyhow::Result<Vec<Box<dyn FrameMetric>>> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|name| metrics::by_name(name).with_context(|| format!("unknown metric '{name}'")))
        .collect()
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let rom = std::fs::read(&args.rom)
        .with_context(|| format!("failed to read ROM {}", args.rom.display()))?;

    let boot_rom = match &args.boot_rom {
        Some(path) => Some(
            std::fs::read(path)
                .with_context(|| format!("failed to read boot ROM {}", path.display()))?,
        ),
        None => None,
    };
    let boot_rom_ref = boot_rom.as_deref();

    let recording = match &args.recording {
        Some(path) => citrine_lab::recording::load(path)
            .with_context(|| format!("failed to load recording {}", path.display()))?,
        None => Recording::new("", parse_model(&args.model)?),
    };

    let metrics = build_metrics(&args.metrics)?;

    println!(
        "running {} frames on Citrine and SameBoy ({} input event(s))...",
        args.frames,
        recording.events.len()
    );

    // Optional streaming PNG dumper — fed each frame pair as it is compared.
    let mut dumper = match args.dump_divergences {
        Some(dir) => Some(FrameDumper::new(
            dir,
            (args.max_dumps != 0).then_some(args.max_dumps),
            args.dump_all_frames,
        )?),
        None => None,
    };

    let progress = ProgressBar::new(args.frames as u64);
    progress.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] \
             {human_pos}/{human_len} frames ({per_sec}, ETA {eta})  {msg}",
        )
        .expect("valid template")
        .progress_chars("█▉▊▋▌▍▎▏ ")
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏✓"),
    );
    progress.enable_steady_tick(Duration::from_millis(80));

    // SameBoy is the reference; Citrine is the candidate under test. They run on their own threads;
    // this closure runs on the main thread for every compared pair.
    let mut divergent = 0usize;
    let report = run_streaming(
        SameBoyEmulator::new(),
        CitrineEmulator::new(),
        &rom,
        boot_rom_ref,
        &recording,
        &metrics,
        args.frames,
        !args.raw,
        args.align.into(),
        args.tolerance,
        |index, reference, candidate, diverged| {
            if diverged {
                divergent += 1;
                progress.set_message(format!("{divergent} divergent"));
            }
            if let Some(dumper) = dumper.as_mut() {
                dumper.handle(index, reference, candidate, diverged)?;
            }
            progress.inc(1);
            Ok(())
        },
    )?;
    progress.finish_and_clear();

    if let Some(dumper) = &dumper {
        dumper.finish();
    }

    let mut reporters: Vec<Box<dyn Reporter>> = vec![Box::new(ConsoleReporter::default())];
    if let Some(path) = args.json {
        reporters.push(Box::new(JsonReporter { path }));
    }
    for reporter in &reporters {
        reporter.emit(&report)?;
    }

    Ok(())
}
