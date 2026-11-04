use anyhow::Context;
use citrine_gb::console::ConsoleKind;
use citrine_gb::gb::GbModel;
use citrine_lab::emulator::FrameEmulator;
use citrine_lab::emulators::{CitrineEmulator, MgbaEmulator, SameBoyEmulator};
use citrine_lab::metric::FrameMetric;
use citrine_lab::metrics;
use citrine_lab::recording::Recording;
use citrine_lab::report::{ConsoleReporter, FrameDumper, JsonReporter, Reporter};
use citrine_lab::runner::{ComparisonReport, run_alone, run_streaming};
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

fn parse_console(s: &str) -> anyhow::Result<ConsoleKind> {
    match s.to_ascii_lowercase().as_str() {
        "dmg" => Ok(ConsoleKind::Gb),
        "cgb" => Ok(ConsoleKind::Gbc),
        "gba" => Ok(ConsoleKind::Gba),
        other => anyhow::bail!("unknown model '{other}' (expected dmg, cgb or gba)"),
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

    let (recording, console) = match &args.recording {
        Some(path) => {
            let recording = citrine_lab::recording::load(path)
                .with_context(|| format!("failed to load recording {}", path.display()))?;
            let console = recording.model.into();
            (recording, console)
        }
        None => {
            let console = parse_console(&args.model)?;
            let model = GbModel::try_from(console).unwrap_or(GbModel::Dmg);
            (Recording::new("", model), console)
        }
    };

    let metrics = build_metrics(&args.metrics)?;

    let mut reference: Box<dyn FrameEmulator + Send> = match console {
        ConsoleKind::Gba => Box::new(MgbaEmulator::new()),
        ConsoleKind::Gb | ConsoleKind::Gbc => Box::new(SameBoyEmulator::new()),
    };
    let refusal = CitrineEmulator::new()
        .load(&rom, boot_rom_ref, console)
        .err()
        .map(|e| e.to_string());

    match &refusal {
        Some(refusal) => println!(
            "{refusal}\nrunning {} frames on {} alone ({} input event(s))...",
            args.frames,
            reference.name(),
            recording.events.len()
        ),
        None => println!(
            "running {} frames on citrine and {} ({} input event(s))...",
            args.frames,
            reference.name(),
            recording.events.len()
        ),
    }

    let mut dumper = match args.dump_divergences {
        Some(dir) => Some(FrameDumper::new(
            dir,
            (args.max_dumps != 0).then_some(args.max_dumps),
            args.dump_all_frames || refusal.is_some(),
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

    let report = if let Some(refusal) = refusal {
        let frames = run_alone(
            &mut reference,
            &rom,
            boot_rom_ref,
            &recording,
            console,
            args.frames,
            |index, frame| {
                if let Some(dumper) = dumper.as_mut() {
                    dumper.handle_alone(index, frame)?;
                }
                progress.inc(1);
                Ok(())
            },
        )?;
        ComparisonReport::reference_alone(reference.name(), "citrine", frames, refusal)
    } else {
        let mut divergent = 0usize;
        run_streaming(
            reference,
            CitrineEmulator::new(),
            &rom,
            boot_rom_ref,
            &recording,
            console,
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
        )?
    };
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
