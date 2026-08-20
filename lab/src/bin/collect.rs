//! Captures the emulator's accuracy in one run — the mooneye suite plus a SameBoy frame diff over
//! every local ROM — into `experiments/runs/<date>_<git-short-hash>/`. See `experiments/README.md`.
//!
//! Usage: `make results`, or `cargo run --release -p citrine-gb-lab --bin collect -- [options]`.

use anyhow::Context;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::rom::Rom;
use citrine_gb::rom::header::RomHeader;
use citrine_lab::metric::FrameMetric;
use citrine_lab::metrics;
use citrine_lab::recording::Recording;
use citrine_lab::runner::{Alignment, ComparisonReport, run_streaming};
use clap::Parser;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// In report column order.
const METRICS: [&str; 6] = ["exact", "px_match", "mse", "nmse", "psnr", "ssim"];

#[derive(Parser, Debug)]
#[command(
    name = "collect",
    about = "Collect mooneye + SameBoy-diff results for the thesis"
)]
struct Args {
    /// Frames to compare per diff run.
    #[arg(long, default_value_t = 3600)]
    frames: usize,

    /// Comma-separated cycle-alignment tolerances to sweep.
    #[arg(long, default_value = "0,1,2,5,10", value_delimiter = ',')]
    tolerances: Vec<usize>,

    /// Parallel diff runs (each uses ~3 threads of its own).
    #[arg(long, default_value_t = 3)]
    jobs: usize,

    /// Only diff ROMs whose file name contains this substring.
    #[arg(long)]
    only: Option<String>,

    #[arg(long, default_value_t = false)]
    skip_mooneye: bool,

    #[arg(long, default_value_t = false)]
    skip_diff: bool,

    /// Also write the full per-frame reports to `raw/` (large; gitignored).
    #[arg(long, default_value_t = false)]
    per_frame: bool,

    /// Output directory (default: experiments/runs/<date>_<git-short-hash>).
    #[arg(long)]
    out: Option<PathBuf>,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn git(args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .current_dir(repo_root())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

// ------------------------------------------------------------------------------------- results

#[derive(serde::Serialize)]
struct Results {
    meta: Meta,
    #[serde(skip_serializing_if = "Option::is_none")]
    mooneye: Option<MooneyeResults>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diff: Vec<DiffRun>,
}

#[derive(serde::Serialize)]
struct Meta {
    collected_at: String,
    git_commit: String,
    git_branch: String,
    git_dirty: bool,
    frames: usize,
    tolerances: Vec<usize>,
    metrics: Vec<String>,
    align: String,
    normalization: String,
    host: String,
    total_wall_time_s: f64,
}

#[derive(serde::Serialize)]
struct MooneyeResults {
    total: usize,
    passed: usize,
    failed: usize,
    categories: BTreeMap<String, CategoryCount>,
    tests: Vec<MooneyeTest>,
}

#[derive(Default, serde::Serialize)]
struct CategoryCount {
    passed: usize,
    failed: usize,
}

#[derive(serde::Serialize)]
struct MooneyeTest {
    name: String,
    category: String,
    /// `pass` or `fail`.
    result: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    note: String,
}

#[derive(serde::Serialize)]
struct DiffRun {
    rom: String,
    category: String,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    recording: Option<String>,
    tolerance: usize,
    /// `ok`, `error` or `panic`.
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compared_frames: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    divergent_frames: Option<usize>,
    /// 1 - divergent/compared.
    #[serde(skip_serializing_if = "Option::is_none")]
    match_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_divergent_frame: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_divergent_frame: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    metrics: Vec<MetricAgg>,
    wall_time_s: f64,
}

#[derive(serde::Serialize)]
struct MetricAgg {
    name: String,
    mean: f64,
    best: f64,
    worst: f64,
}

fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

// ------------------------------------------------------------------------------------- mooneye

const LD_B_B: u8 = 0x40;
const MAX_CYCLES: u32 = 30_000_000;
const PASS_REGS: [u8; 6] = [3, 5, 8, 13, 21, 34];
const FAIL_REGS: [u8; 6] = [0x42; 6];

/// Mirrors `DMG_SUITE` in `lib/tests/mooneye.rs`.
fn dmg_suite_match(rel: &str) -> bool {
    if !(rel.starts_with("acceptance/") || rel.starts_with("emulator-only/")) {
        return false;
    }
    let Some(stem) = rel.strip_suffix(".gb") else {
        return false;
    };
    let name = stem.rsplit('/').next().unwrap_or(stem);
    match name.rfind('-') {
        None => true,
        Some(i) => {
            let variant = &name[i + 1..];
            variant.contains('G') || variant.contains("dmg")
        }
    }
}

fn collect_mooneye_roms(build_root: &Path) -> anyhow::Result<Vec<(String, PathBuf)>> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(&path, out)?;
            } else {
                out.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    walk(build_root, &mut files).with_context(|| {
        format!(
            "failed to read mooneye build dir {} (run `make build-tests` first)",
            build_root.display()
        )
    })?;

    let mut roms: Vec<(String, PathBuf)> = files
        .into_iter()
        .filter_map(|path| {
            let rel = path
                .strip_prefix(build_root)
                .ok()?
                .to_string_lossy()
                .into_owned();
            dmg_suite_match(&rel).then(|| (rel.trim_end_matches(".gb").to_string(), path))
        })
        .collect();
    roms.sort();
    Ok(roms)
}

/// Same convention as `lib/tests/mooneye.rs`: run to `LD B, B`, then check B..L for the pass
/// signature.
fn run_mooneye_rom(path: &Path) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| format!("load error: {e}"))?;
    let rom = Rom::new(&data);
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&rom)
        .map_err(|e| format!("load error: {e:?}"))?;

    while gb.cpu.ir != LD_B_B {
        gb.step();
        if gb.cycle_counter >= MAX_CYCLES {
            return Err(format!("no result marker within {MAX_CYCLES} cycles"));
        }
    }

    let regs = [gb.cpu.b, gb.cpu.c, gb.cpu.d, gb.cpu.e, gb.cpu.h, gb.cpu.l];
    match regs {
        PASS_REGS => Ok(()),
        FAIL_REGS => Err("reported failure (B/C/D/E/H/L = 0x42)".to_string()),
        _ => Err(format!("unexpected registers {regs:02X?}")),
    }
}

fn mooneye_category(name: &str) -> String {
    let parts: Vec<&str> = name.split('/').collect();
    if parts.len() > 2 {
        format!("{}/{}", parts[0], parts[1])
    } else {
        parts[0].to_string()
    }
}

fn run_mooneye_suite(jobs: usize) -> anyhow::Result<MooneyeResults> {
    let build_root = repo_root().join("tests/mooneye/build");
    let roms = collect_mooneye_roms(&build_root)?;
    println!("\n=== mooneye suite: {} tests ===", roms.len());

    let next = AtomicUsize::new(0);
    let tests: Mutex<Vec<Option<MooneyeTest>>> =
        Mutex::new((0..roms.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((name, path)) = roms.get(i) else {
                        break;
                    };
                    let outcome = catch_unwind(AssertUnwindSafe(|| run_mooneye_rom(path)))
                        .unwrap_or_else(|_| Err("panicked".to_string()));
                    let (result, note) = match outcome {
                        Ok(()) => ("pass", String::new()),
                        Err(note) => ("fail", note),
                    };
                    tests.lock().unwrap()[i] = Some(MooneyeTest {
                        name: name.clone(),
                        category: mooneye_category(name),
                        result: result.to_string(),
                        note,
                    });
                }
            });
        }
    });

    let tests: Vec<MooneyeTest> = tests.into_inner().unwrap().into_iter().flatten().collect();
    let mut categories: BTreeMap<String, CategoryCount> = BTreeMap::new();
    for test in &tests {
        let entry = categories.entry(test.category.clone()).or_default();
        if test.result == "pass" {
            entry.passed += 1;
        } else {
            entry.failed += 1;
        }
    }
    let passed = tests.iter().filter(|t| t.result == "pass").count();
    println!("mooneye: {passed}/{} passed", tests.len());
    Ok(MooneyeResults {
        total: tests.len(),
        passed,
        failed: tests.len() - passed,
        categories,
        tests,
    })
}

// ------------------------------------------------------------------------------------- diff

struct RomConfig {
    path: PathBuf,
    rom: Vec<u8>,
    category: &'static str,
    model: GbModel,
    recording: Option<(String, Recording)>,
}

impl RomConfig {
    fn label(&self) -> String {
        let rec = if self.recording.is_some() {
            " +recording"
        } else {
            ""
        };
        format!("{}{rec}", self.path.file_name().unwrap().to_string_lossy())
    }
}

fn model_name(model: GbModel) -> &'static str {
    match model {
        GbModel::Dmg => "dmg",
        GbModel::Cgb => "cgb",
    }
}

/// Keyed by uppercase ROM SHA-256.
fn discover_recordings(root: &Path) -> BTreeMap<String, (String, Recording)> {
    let mut recordings = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(root.join("roms")) else {
        return recordings;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json")
            && let Ok(json) = std::fs::read_to_string(&path)
            && let Ok(recording) = Recording::from_json(&json)
        {
            let mut recording = recording;
            recording.sort();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            recordings.insert(recording.rom_sha256.to_uppercase(), (name, recording));
        }
    }
    recordings
}

fn discover_roms(root: &Path, only: Option<&str>) -> anyhow::Result<Vec<RomConfig>> {
    let mut recordings = discover_recordings(root);

    let mut sources: Vec<(&'static str, PathBuf)> = Vec::new();
    for dir_entry in std::fs::read_dir(root.join("roms/games"))? {
        let path = dir_entry?.path();
        if path.extension().is_some_and(|e| e == "gb" || e == "gbc") {
            sources.push(("game", path));
        }
    }
    sources.push(("test", root.join("roms/test/dmg-acid2.gb")));
    for dir_entry in std::fs::read_dir(root.join("roms/test/blargg"))? {
        let path = dir_entry?.path();
        if path.extension().is_some_and(|e| e == "gb") {
            sources.push(("test", path));
        }
    }
    sources.sort_by(|a, b| a.1.cmp(&b.1));

    let mut configs = Vec::new();
    for (category, path) in sources {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if let Some(filter) = only
            && !name.contains(filter)
        {
            continue;
        }
        let rom = std::fs::read(&path).with_context(|| format!("failed to read {name}"))?;
        let sha: String = RomHeader::calculate_sha256(&rom)
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect();
        let model = if path.extension().is_some_and(|e| e == "gbc") {
            GbModel::Cgb
        } else {
            GbModel::Dmg
        };
        if let Some((rec_name, recording)) = recordings.remove(&sha) {
            let rec_model = recording.model;
            configs.push(RomConfig {
                path: path.clone(),
                rom: rom.clone(),
                category,
                model: rec_model,
                recording: Some((rec_name, recording)),
            });
        }
        configs.push(RomConfig {
            path,
            rom,
            category,
            model,
            recording: None,
        });
    }
    configs.sort_by(|a, b| a.label().cmp(&b.label()));

    for (sha, (name, _)) in recordings {
        println!("WARNING: recording {name} matches no discovered ROM (sha256 {sha})");
    }
    Ok(configs)
}

fn build_metrics() -> Vec<Box<dyn FrameMetric>> {
    METRICS
        .iter()
        .map(|name| metrics::by_name(name).expect("known metric"))
        .collect()
}

fn run_diff(
    config: &RomConfig,
    tolerance: usize,
    frames: usize,
    boot_roms: &BTreeMap<&'static str, Vec<u8>>,
    per_frame_dir: Option<&Path>,
) -> DiffRun {
    let mut run = DiffRun {
        rom: config
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        category: config.category.to_string(),
        model: model_name(config.model).to_string(),
        recording: config.recording.as_ref().map(|(name, _)| name.clone()),
        tolerance,
        status: "ok".to_string(),
        error: None,
        compared_frames: None,
        divergent_frames: None,
        match_rate: None,
        first_divergent_frame: None,
        last_divergent_frame: None,
        metrics: Vec::new(),
        wall_time_s: 0.0,
    };

    let recording = match &config.recording {
        Some((_, recording)) => recording.clone(),
        None => Recording::new("", config.model),
    };
    let boot_rom = &boot_roms[model_name(config.model)];
    let metrics = build_metrics();

    let start = Instant::now();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        run_streaming(
            citrine_lab::emulators::SameBoyEmulator::new(),
            citrine_lab::emulators::CitrineEmulator::new(),
            &config.rom,
            Some(boot_rom),
            &recording,
            &metrics,
            frames,
            true,
            Alignment::Cycle,
            tolerance,
            |_, _, _, _| Ok(()),
        )
    }));
    run.wall_time_s = (start.elapsed().as_secs_f64() * 10.0).round() / 10.0;

    let report: ComparisonReport = match outcome {
        Ok(Ok(report)) => report,
        Ok(Err(err)) => {
            run.status = "error".to_string();
            run.error = Some(format!("{err:#}"));
            return run;
        }
        Err(_) => {
            run.status = "panic".to_string();
            run.error = Some("comparison panicked".to_string());
            return run;
        }
    };

    if let Some(dir) = per_frame_dir {
        let id = format!(
            "{}{}_tol{tolerance}",
            run.rom.replace(['(', ')', ' '], "_"),
            if run.recording.is_some() { "+rec" } else { "" },
        );
        if let Ok(file) = std::fs::File::create(dir.join(format!("{id}.json"))) {
            let _ = serde_json::to_writer(std::io::BufWriter::new(file), &report);
        }
    }

    run.compared_frames = Some(report.compared_frame_count);
    run.divergent_frames = Some(report.divergent_frame_count);
    run.match_rate = (report.compared_frame_count > 0).then(|| {
        round6(1.0 - report.divergent_frame_count as f64 / report.compared_frame_count as f64)
    });
    run.first_divergent_frame = report.first_divergent_frame;
    run.last_divergent_frame = report.last_divergent_frame;
    run.metrics = report
        .summaries
        .iter()
        .map(|s| MetricAgg {
            name: s.name.clone(),
            mean: round6(s.mean),
            best: round6(s.best),
            worst: round6(s.worst),
        })
        .collect();
    run
}

fn run_all_diffs(
    configs: &[RomConfig],
    tolerances: &[usize],
    frames: usize,
    jobs: usize,
    per_frame_dir: Option<&Path>,
) -> anyhow::Result<Vec<DiffRun>> {
    let root = repo_root();
    let mut boot_roms = BTreeMap::new();
    for (model, file) in [("dmg", "dmg_boot.bin"), ("cgb", "cgb_boot.bin")] {
        let path = root.join("roms/boot").join(file);
        boot_roms.insert(
            model,
            std::fs::read(&path)
                .with_context(|| format!("failed to read boot ROM {}", path.display()))?,
        );
    }

    let tasks: Vec<(&RomConfig, usize)> = configs
        .iter()
        .flat_map(|c| tolerances.iter().map(move |&t| (c, t)))
        .collect();
    println!(
        "\n=== diff runs: {} ROM configs x {} tolerances = {} runs, {frames} frames each, {jobs} parallel ===",
        configs.len(),
        tolerances.len(),
        tasks.len(),
    );

    let next = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<DiffRun>>> = Mutex::new((0..tasks.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(config, tolerance)) = tasks.get(i) else {
                        break;
                    };
                    let run = run_diff(config, tolerance, frames, &boot_roms, per_frame_dir);
                    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    let note = match run.match_rate {
                        Some(rate) if run.status == "ok" => {
                            format!("match {:6.2}%  ({}s)", rate * 100.0, run.wall_time_s)
                        }
                        _ => run.status.to_uppercase(),
                    };
                    println!(
                        "[{done}/{}] {:<44} tol={tolerance:<2} {note}",
                        tasks.len(),
                        config.label(),
                    );
                    results.lock().unwrap()[i] = Some(run);
                }
            });
        }
    });

    Ok(results
        .into_inner()
        .unwrap()
        .into_iter()
        .flatten()
        .collect())
}

// ------------------------------------------------------------------------------------- output

fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn write_csv(path: &Path, rows: &[Vec<String>]) -> anyhow::Result<()> {
    let text = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|f| csv_field(f))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(path, text)?;
    Ok(())
}

fn fmt_opt<T: std::fmt::Display>(value: &Option<T>) -> String {
    value.as_ref().map(|v| v.to_string()).unwrap_or_default()
}

fn fmt_f64(value: f64) -> String {
    format!("{value:.6}")
}

fn write_outputs(out_dir: &Path, results: &Results) -> anyhow::Result<()> {
    std::fs::write(
        out_dir.join("results.json"),
        serde_json::to_string_pretty(results)? + "\n",
    )?;

    if let Some(mooneye) = &results.mooneye {
        let mut rows = vec![vec![
            "test".into(),
            "category".into(),
            "result".into(),
            "note".into(),
        ]];
        rows.extend(mooneye.tests.iter().map(|t| {
            vec![
                t.name.clone(),
                t.category.clone(),
                t.result.clone(),
                t.note.clone(),
            ]
        }));
        write_csv(&out_dir.join("mooneye_tests.csv"), &rows)?;

        let mut rows = vec![vec![
            "category".into(),
            "passed".into(),
            "failed".into(),
            "total".into(),
            "pass_rate".into(),
        ]];
        for (category, count) in &mooneye.categories {
            let total = count.passed + count.failed;
            rows.push(vec![
                category.clone(),
                count.passed.to_string(),
                count.failed.to_string(),
                total.to_string(),
                fmt_f64(count.passed as f64 / total as f64),
            ]);
        }
        rows.push(vec![
            "TOTAL".into(),
            mooneye.passed.to_string(),
            mooneye.failed.to_string(),
            mooneye.total.to_string(),
            fmt_f64(mooneye.passed as f64 / mooneye.total.max(1) as f64),
        ]);
        write_csv(&out_dir.join("mooneye_summary.csv"), &rows)?;
    }

    if !results.diff.is_empty() {
        let mut header: Vec<String> = [
            "rom",
            "category",
            "model",
            "recording",
            "tolerance",
            "status",
            "compared_frames",
            "divergent_frames",
            "match_rate",
            "first_divergent_frame",
            "last_divergent_frame",
        ]
        .map(String::from)
        .to_vec();
        for metric in METRICS {
            for stat in ["mean", "best", "worst"] {
                header.push(format!("{metric}_{stat}"));
            }
        }
        let mut rows = vec![header];
        for run in &results.diff {
            let mut row = vec![
                run.rom.clone(),
                run.category.clone(),
                run.model.clone(),
                run.recording.clone().unwrap_or_default(),
                run.tolerance.to_string(),
                run.status.clone(),
                fmt_opt(&run.compared_frames),
                fmt_opt(&run.divergent_frames),
                run.match_rate.map(fmt_f64).unwrap_or_default(),
                fmt_opt(&run.first_divergent_frame),
                fmt_opt(&run.last_divergent_frame),
            ];
            for metric in METRICS {
                match run.metrics.iter().find(|m| m.name == metric) {
                    Some(m) => row.extend([fmt_f64(m.mean), fmt_f64(m.best), fmt_f64(m.worst)]),
                    None => row.extend([String::new(), String::new(), String::new()]),
                }
            }
            rows.push(row);
        }
        write_csv(&out_dir.join("diff_results.csv"), &rows)?;

        let tolerances = &results.meta.tolerances;
        for (file, value) in [
            (
                "diff_pivot_match_rate.csv",
                (|run: &DiffRun| run.match_rate) as fn(&DiffRun) -> Option<f64>,
            ),
            ("diff_pivot_ssim.csv", |run: &DiffRun| {
                run.metrics
                    .iter()
                    .find(|m| m.name == "ssim")
                    .map(|m| m.mean)
            }),
        ] {
            let mut by_rom: BTreeMap<String, BTreeMap<usize, String>> = BTreeMap::new();
            for run in &results.diff {
                let key = match &run.recording {
                    Some(_) => format!("{} +recording", run.rom),
                    None => run.rom.clone(),
                };
                let cell = match value(run) {
                    Some(v) if run.status == "ok" => fmt_f64(v),
                    _ => run.status.clone(),
                };
                by_rom.entry(key).or_default().insert(run.tolerance, cell);
            }
            let mut rows = vec![
                std::iter::once("rom".to_string())
                    .chain(tolerances.iter().map(|t| format!("tolerance_{t}")))
                    .collect::<Vec<_>>(),
            ];
            for (rom, cells) in by_rom {
                rows.push(
                    std::iter::once(rom)
                        .chain(
                            tolerances
                                .iter()
                                .map(|t| cells.get(t).cloned().unwrap_or_default()),
                        )
                        .collect(),
                );
            }
            write_csv(&out_dir.join(file), &rows)?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------------------------------- main

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let root = repo_root();

    let commit = git(&["rev-parse", "--short", "HEAD"]);
    let dirty = !git(&["status", "--porcelain"]).is_empty();
    let date = Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown-date".to_string());

    let out_dir = args.out.clone().unwrap_or_else(|| {
        let suffix = if dirty { "-dirty" } else { "" };
        root.join("experiments/runs")
            .join(format!("{date}_{commit}{suffix}"))
    });
    std::fs::create_dir_all(&out_dir)?;
    if dirty {
        println!("WARNING: working tree is dirty - results will not map cleanly to a commit");
    }

    let per_frame_dir = args.per_frame.then(|| out_dir.join("raw"));
    if let Some(dir) = &per_frame_dir {
        std::fs::create_dir_all(dir)?;
    }

    let start = Instant::now();
    let mooneye = if args.skip_mooneye {
        None
    } else {
        Some(run_mooneye_suite(args.jobs)?)
    };
    let diff = if args.skip_diff {
        Vec::new()
    } else {
        let configs = discover_roms(&root, args.only.as_deref())?;
        run_all_diffs(
            &configs,
            &args.tolerances,
            args.frames,
            args.jobs,
            per_frame_dir.as_deref(),
        )?
    };

    let results = Results {
        meta: Meta {
            collected_at: Command::new("date")
                .arg("-Iseconds")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default(),
            git_commit: git(&["rev-parse", "HEAD"]),
            git_branch: git(&["rev-parse", "--abbrev-ref", "HEAD"]),
            git_dirty: dirty,
            frames: args.frames,
            tolerances: args.tolerances.clone(),
            metrics: METRICS.iter().map(|m| m.to_string()).collect(),
            align: "cycle".to_string(),
            normalization: "greyscale".to_string(),
            host: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            total_wall_time_s: (start.elapsed().as_secs_f64() * 10.0).round() / 10.0,
        },
        mooneye,
        diff,
    };

    write_outputs(&out_dir, &results)?;

    println!("\n=== done in {}s ===", results.meta.total_wall_time_s);
    if let Some(mooneye) = &results.mooneye {
        println!("mooneye: {}/{} passed", mooneye.passed, mooneye.total);
    }
    if !results.diff.is_empty() {
        let failed = results.diff.iter().filter(|r| r.status != "ok").count();
        println!(
            "diff runs: {}/{} ok{}",
            results.diff.len() - failed,
            results.diff.len(),
            if failed > 0 {
                format!(" ({failed} failed - see results.json)")
            } else {
                String::new()
            }
        );
    }
    println!("results: {}", out_dir.display());
    Ok(())
}
