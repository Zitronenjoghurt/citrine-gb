//! Significance analysis over every committed run in `experiments/runs/`, written to
//! `experiments/analysis/`. The statistical model is documented in `experiments/README.md`.
//!
//! Usage: `make significance`, or `cargo run --release -p citrine-gb-lab --bin analyze -- [options]`.

use anyhow::{Context, Result};
use clap::Parser;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------------------------- input model

#[derive(serde::Deserialize)]
struct Results {
    meta: Meta,
    #[serde(default)]
    mooneye: Option<MooneyeResults>,
    #[serde(default)]
    diff: Vec<DiffRun>,
}

#[derive(serde::Deserialize)]
struct Meta {
    #[serde(default)]
    collected_at: String,
    #[serde(default)]
    git_commit: String,
    #[serde(default)]
    tolerances: Vec<usize>,
}

#[derive(serde::Deserialize)]
struct MooneyeResults {
    total: usize,
    passed: usize,
    #[serde(default)]
    tests: Vec<MooneyeTest>,
}

#[derive(serde::Deserialize)]
struct MooneyeTest {
    name: String,
    result: String,
}

#[derive(serde::Deserialize)]
struct DiffRun {
    rom: String,
    #[serde(default)]
    recording: Option<String>,
    tolerance: usize,
    status: String,
    #[serde(default)]
    match_rate: Option<f64>,
    #[serde(default)]
    metrics: Vec<MetricAgg>,
}

impl DiffRun {
    fn ssim(&self) -> Option<f64> {
        self.metrics
            .iter()
            .find(|m| m.name == "ssim")
            .map(|m| m.mean)
    }
    fn config_key(&self) -> String {
        match &self.recording {
            Some(_) => format!("{} +recording", self.rom),
            None => self.rom.clone(),
        }
    }
}

#[derive(serde::Deserialize)]
struct MetricAgg {
    name: String,
    mean: f64,
}

struct Run {
    label: String,
    results: Results,
}

// ------------------------------------------------------------------------------------- statistics

/// Via the Abramowitz & Stegun 7.1.26 error-function approximation.
fn norm_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let y = 1.0
        - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
            + 0.254829592)
            * t
            * (-x * x).exp();
    if x < 0.0 { -y } else { y }
}

fn two_sided_p(z: f64) -> f64 {
    (2.0 * (1.0 - norm_cdf(z.abs()))).clamp(0.0, 1.0)
}

fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        f64::NAN
    } else if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        f64::NAN
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

/// SplitMix64, so bootstrap intervals are reproducible across runs of the tool.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// Percentile bootstrap, returning `(median, lo, hi)`.
fn bootstrap_median_ci(values: &[f64], iters: usize, alpha: f64, rng: &mut Rng) -> (f64, f64, f64) {
    let point = median(values);
    if values.len() < 2 {
        return (point, f64::NAN, f64::NAN);
    }
    let n = values.len();
    let mut stats = Vec::with_capacity(iters);
    let mut sample = vec![0.0; n];
    for _ in 0..iters {
        for slot in sample.iter_mut() {
            *slot = values[rng.below(n)];
        }
        stats.push(median(&sample));
    }
    stats.sort_by(f64::total_cmp);
    let lo = stats[((alpha / 2.0) * (iters - 1) as f64).round() as usize];
    let hi = stats[((1.0 - alpha / 2.0) * (iters - 1) as f64).round() as usize];
    (point, lo, hi)
}

fn wilson_interval(passed: usize, total: usize) -> (f64, f64) {
    if total == 0 {
        return (f64::NAN, f64::NAN);
    }
    let z = 1.959_964;
    let n = total as f64;
    let p = passed as f64 / n;
    let denom = 1.0 + z * z / n;
    let center = (p + z * z / (2.0 * n)) / denom;
    let half = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / denom;
    ((center - half).max(0.0), (center + half).min(1.0))
}

struct Wilcoxon {
    n: usize,
    w_plus: f64,
    z: f64,
    p: f64,
    median_diff: f64,
}

/// Two-sided, tie-corrected, with a continuity correction. Positive diffs mean the later run won.
fn wilcoxon_signed_rank(diffs: &[f64]) -> Option<Wilcoxon> {
    let nonzero: Vec<f64> = diffs.iter().copied().filter(|d| *d != 0.0).collect();
    let n = nonzero.len();
    if n == 0 {
        return None;
    }

    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| nonzero[i].abs().total_cmp(&nonzero[j].abs()));

    let mut ranks = vec![0.0; n];
    let mut tie_correction = 0.0;
    let mut i = 0;
    while i < n {
        let mut j = i + 1;
        while j < n && (nonzero[order[j]].abs() - nonzero[order[i]].abs()).abs() < 1e-12 {
            j += 1;
        }
        let group = j - i;
        let avg_rank = (i + 1 + j) as f64 / 2.0;
        for &idx in &order[i..j] {
            ranks[idx] = avg_rank;
        }
        let t = group as f64;
        tie_correction += t * t * t - t;
        i = j;
    }

    let w_plus: f64 = (0..n).filter(|&k| nonzero[k] > 0.0).map(|k| ranks[k]).sum();
    let nf = n as f64;
    let mean_w = nf * (nf + 1.0) / 4.0;
    let var_w = nf * (nf + 1.0) * (2.0 * nf + 1.0) / 24.0 - tie_correction / 48.0;
    let sd = var_w.sqrt();
    let z = if sd > 0.0 {
        let cc = 0.5 * (w_plus - mean_w).signum();
        (w_plus - mean_w - cc) / sd
    } else {
        0.0
    };
    Some(Wilcoxon {
        n,
        w_plus,
        z,
        p: two_sided_p(z),
        median_diff: median(&nonzero),
    })
}

struct McNemar {
    /// Passed in the earlier run, failed in the later one.
    regressed: usize,
    /// Failed in the earlier run, passed in the later one.
    improved: usize,
    p: f64,
    exact: bool,
}

/// Exact binomial test when discordant pairs are few, else chi-square with continuity correction.
fn mcnemar(improved: usize, regressed: usize) -> McNemar {
    let discordant = improved + regressed;
    if discordant == 0 {
        return McNemar {
            regressed,
            improved,
            p: 1.0,
            exact: true,
        };
    }
    if discordant < 25 {
        let k = improved.min(regressed);
        let mut term = 0.5f64.powi(discordant as i32);
        let mut cum = term;
        for i in 0..k {
            term *= (discordant - i) as f64 / (i + 1) as f64;
            cum += term;
        }
        McNemar {
            regressed,
            improved,
            p: (2.0 * cum).min(1.0),
            exact: true,
        }
    } else {
        let b = regressed as f64;
        let c = improved as f64;
        let chi2 = ((b - c).abs() - 1.0).powi(2) / (b + c);
        McNemar {
            regressed,
            improved,
            p: two_sided_p(chi2.sqrt()),
            exact: false,
        }
    }
}

fn stars(p: f64, alpha: f64) -> &'static str {
    if p < 0.001 {
        "***"
    } else if p < 0.01 {
        "**"
    } else if p < alpha {
        "*"
    } else {
        "ns"
    }
}

// ------------------------------------------------------------------------------------- extraction

fn match_rates(run: &Results, tolerance: usize, include_recordings: bool) -> Vec<f64> {
    run.diff
        .iter()
        .filter(|d| {
            d.tolerance == tolerance
                && d.status == "ok"
                && (include_recordings || d.recording.is_none())
        })
        .filter_map(|d| d.match_rate)
        .collect()
}

fn ssims(run: &Results, tolerance: usize, include_recordings: bool) -> Vec<f64> {
    run.diff
        .iter()
        .filter(|d| {
            d.tolerance == tolerance
                && d.status == "ok"
                && (include_recordings || d.recording.is_none())
        })
        .filter_map(|d| d.ssim())
        .collect()
}

fn paired_match_rates(
    earlier: &Results,
    later: &Results,
    tolerance: usize,
    include_recordings: bool,
) -> (Vec<f64>, Vec<f64>) {
    let index = |run: &Results| -> BTreeMap<String, f64> {
        run.diff
            .iter()
            .filter(|d| {
                d.tolerance == tolerance
                    && d.status == "ok"
                    && (include_recordings || d.recording.is_none())
            })
            .filter_map(|d| d.match_rate.map(|r| (d.config_key(), r)))
            .collect()
    };
    let a = index(earlier);
    let b = index(later);
    let mut ea = Vec::new();
    let mut lb = Vec::new();
    for (key, va) in &a {
        if let Some(vb) = b.get(key) {
            ea.push(*va);
            lb.push(*vb);
        }
    }
    (ea, lb)
}

fn paired_tolerances(
    run: &Results,
    tol_low: usize,
    tol_high: usize,
    include_recordings: bool,
) -> (Vec<f64>, Vec<f64>) {
    let index = |tol: usize| -> BTreeMap<String, f64> {
        run.diff
            .iter()
            .filter(|d| {
                d.tolerance == tol
                    && d.status == "ok"
                    && (include_recordings || d.recording.is_none())
            })
            .filter_map(|d| d.match_rate.map(|r| (d.config_key(), r)))
            .collect()
    };
    let lo = index(tol_low);
    let hi = index(tol_high);
    let mut low = Vec::new();
    let mut high = Vec::new();
    for (key, vlo) in &lo {
        if let Some(vhi) = hi.get(key) {
            low.push(*vlo);
            high.push(*vhi);
        }
    }
    (low, high)
}

/// Returns (improved fail→pass, regressed pass→fail), paired by test name.
fn paired_mooneye(earlier: &MooneyeResults, later: &MooneyeResults) -> (usize, usize) {
    let index = |m: &MooneyeResults| -> BTreeMap<String, bool> {
        m.tests
            .iter()
            .map(|t| (t.name.clone(), t.result == "pass"))
            .collect()
    };
    let a = index(earlier);
    let b = index(later);
    let mut improved = 0;
    let mut regressed = 0;
    for (name, &pass_a) in &a {
        if let Some(&pass_b) = b.get(name) {
            match (pass_a, pass_b) {
                (false, true) => improved += 1,
                (true, false) => regressed += 1,
                _ => {}
            }
        }
    }
    (improved, regressed)
}

// ------------------------------------------------------------------------------------- loading

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn load_runs(runs_dir: &Path) -> Result<Vec<Run>> {
    let mut runs = Vec::new();
    let entries = std::fs::read_dir(runs_dir)
        .with_context(|| format!("failed to read runs dir {}", runs_dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        let results_path = path.join("results.json");
        if !results_path.is_file() {
            continue;
        }
        let json = std::fs::read_to_string(&results_path)
            .with_context(|| format!("failed to read {}", results_path.display()))?;
        let results: Results = serde_json::from_str(&json)
            .with_context(|| format!("failed to parse {}", results_path.display()))?;
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        runs.push(Run { label, results });
    }
    runs.sort_by(|a, b| {
        a.results
            .meta
            .collected_at
            .cmp(&b.results.meta.collected_at)
            .then(a.label.cmp(&b.label))
    });
    Ok(runs)
}

// ------------------------------------------------------------------------------------- cli / main

#[derive(Parser, Debug)]
#[command(
    name = "analyze",
    about = "Significance analysis over committed experiment runs"
)]
struct Args {
    /// Directory holding the per-run result folders.
    #[arg(long)]
    runs_dir: Option<PathBuf>,

    /// Output directory for the analysis CSVs and markdown summary.
    #[arg(long)]
    out: Option<PathBuf>,

    /// Bootstrap resamples per confidence interval.
    #[arg(long, default_value_t = 10_000)]
    bootstrap: usize,

    /// PRNG seed for reproducible bootstrap intervals.
    #[arg(long, default_value_t = 0x0C17_A17E_u64)]
    seed: u64,

    /// Significance level for the star annotations.
    #[arg(long, default_value_t = 0.05)]
    alpha: f64,

    /// Include `<rom> +recording` replays as separate observations.
    #[arg(long, default_value_t = false)]
    include_recordings: bool,
}

fn fmt(x: f64) -> String {
    if x.is_nan() {
        "-".to_string()
    } else {
        format!("{x:.4}")
    }
}

fn write_csv(path: &Path, rows: &[Vec<String>]) -> Result<()> {
    let text = rows
        .iter()
        .map(|r| r.join(","))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(path, text)?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let root = repo_root();
    let runs_dir = args
        .runs_dir
        .clone()
        .unwrap_or_else(|| root.join("experiments/runs"));
    let out_dir = args
        .out
        .clone()
        .unwrap_or_else(|| root.join("experiments/analysis"));
    std::fs::create_dir_all(&out_dir)?;

    let runs = load_runs(&runs_dir)?;
    if runs.is_empty() {
        anyhow::bail!("no runs with results.json found in {}", runs_dir.display());
    }
    let mut rng = Rng(args.seed);
    let mut md = String::new();
    md.push_str("# Significance analysis\n\n");
    md.push_str(&format!(
        "{} run(s), bootstrap iters {}, seed {}, alpha {}, recordings {}.\n\n",
        runs.len(),
        args.bootstrap,
        args.seed,
        args.alpha,
        if args.include_recordings {
            "included"
        } else {
            "excluded"
        },
    ));
    md.push_str(
        "> Observations are per-ROM; the measurement is deterministic, so the only sampling \
         variability is the ROM selection. Tolerances are analysed separately (never pooled: the \
         columns are re-scorings of the same run). ROMs are a hand-picked set, so intervals \
         describe the population of ROMs *like these*, not all Game Boy software.\n\n",
    );

    // --- per-run descriptive precision -------------------------------------------------------
    println!("\n=== per-run precision (bootstrap 95% CI over ROMs) ===");
    let mut per_run_rows = vec![vec![
        "run".into(),
        "tolerance".into(),
        "n_roms".into(),
        "match_median".into(),
        "match_mean".into(),
        "match_ci_lo".into(),
        "match_ci_hi".into(),
        "ssim_median".into(),
        "ssim_ci_lo".into(),
        "ssim_ci_hi".into(),
    ]];
    let mut mooneye_rows = vec![vec![
        "run".into(),
        "passed".into(),
        "total".into(),
        "pass_rate".into(),
        "wilson_lo".into(),
        "wilson_hi".into(),
    ]];
    for run in &runs {
        println!("\n{}  (commit {})", run.label, run.results.meta.git_commit);
        if let Some(m) = &run.results.mooneye {
            let (lo, hi) = wilson_interval(m.passed, m.total);
            println!(
                "  mooneye: {}/{} = {:.1}%  (Wilson 95% CI {:.1}%..{:.1}%)",
                m.passed,
                m.total,
                100.0 * m.passed as f64 / m.total as f64,
                100.0 * lo,
                100.0 * hi,
            );
            mooneye_rows.push(vec![
                run.label.clone(),
                m.passed.to_string(),
                m.total.to_string(),
                fmt(m.passed as f64 / m.total as f64),
                fmt(lo),
                fmt(hi),
            ]);
        }
        for &tol in &run.results.meta.tolerances {
            let rates = match_rates(&run.results, tol, args.include_recordings);
            if rates.is_empty() {
                continue;
            }
            let (med, lo, hi) = bootstrap_median_ci(&rates, args.bootstrap, args.alpha, &mut rng);
            let ss = ssims(&run.results, tol, args.include_recordings);
            let (smed, slo, shi) = bootstrap_median_ci(&ss, args.bootstrap, args.alpha, &mut rng);
            println!(
                "  tol={tol:<2} n={:<3} match median {:.4} [CI {:.4}..{:.4}]  ssim median {:.4} [CI {:.4}..{:.4}]",
                rates.len(),
                med,
                lo,
                hi,
                smed,
                slo,
                shi,
            );
            per_run_rows.push(vec![
                run.label.clone(),
                tol.to_string(),
                rates.len().to_string(),
                fmt(med),
                fmt(mean(&rates)),
                fmt(lo),
                fmt(hi),
                fmt(smed),
                fmt(slo),
                fmt(shi),
            ]);
        }
    }
    write_csv(&out_dir.join("per_run.csv"), &per_run_rows)?;
    write_csv(&out_dir.join("mooneye_per_run.csv"), &mooneye_rows)?;

    // --- within-run tolerance effect ----------------------------------------------------------
    // Paired within-ROM, so tolerances are never pooled as independent observations.
    println!("\n=== within-run tolerance effect (paired Wilcoxon on match rate) ===");
    let mut tol_rows = vec![vec![
        "run".into(),
        "from_tol".into(),
        "to_tol".into(),
        "n".into(),
        "improved".into(),
        "worsened".into(),
        "median_delta".into(),
        "z".into(),
        "p_value".into(),
        "signif".into(),
    ]];
    for run in &runs {
        let tols = &run.results.meta.tolerances;
        if tols.len() < 2 {
            continue;
        }
        println!("\n{}", run.label);
        let mut steps: Vec<(usize, usize)> = tols.windows(2).map(|w| (w[0], w[1])).collect();
        steps.push((tols[0], *tols.last().unwrap()));
        for (lo_tol, hi_tol) in steps {
            let (low, high) =
                paired_tolerances(&run.results, lo_tol, hi_tol, args.include_recordings);
            if low.is_empty() {
                continue;
            }
            let diffs: Vec<f64> = high.iter().zip(&low).map(|(h, l)| h - l).collect();
            let improved = diffs.iter().filter(|d| **d > 0.0).count();
            let worsened = diffs.iter().filter(|d| **d < 0.0).count();
            let overall = if lo_tol == tols[0] && hi_tol == *tols.last().unwrap() && tols.len() > 2
            {
                " (overall)"
            } else {
                ""
            };
            match wilcoxon_signed_rank(&diffs) {
                Some(w) => {
                    println!(
                        "  tol {lo_tol:>2} -> {hi_tol:<2}{overall:<10} n={:<3} +{improved}/-{worsened}  median Δ={:+.4}  z={:+.3}  p={:.4} {}",
                        w.n,
                        w.median_diff,
                        w.z,
                        w.p,
                        stars(w.p, args.alpha),
                    );
                    tol_rows.push(vec![
                        run.label.clone(),
                        lo_tol.to_string(),
                        hi_tol.to_string(),
                        w.n.to_string(),
                        improved.to_string(),
                        worsened.to_string(),
                        format!("{:+.4}", w.median_diff),
                        format!("{:+.3}", w.z),
                        fmt(w.p),
                        stars(w.p, args.alpha).into(),
                    ]);
                }
                None => {
                    println!(
                        "  tol {lo_tol:>2} -> {hi_tol:<2}{overall:<10} identical (no differences)"
                    );
                }
            }
        }
    }
    write_csv(&out_dir.join("tolerance_effect.csv"), &tol_rows)?;

    // --- pairwise progression -----------------------------------------------------------------
    let mut pairwise_rows = vec![vec![
        "earlier".into(),
        "later".into(),
        "test".into(),
        "tolerance".into(),
        "n".into(),
        "statistic".into(),
        "detail".into(),
        "p_value".into(),
        "signif".into(),
    ]];
    if runs.len() < 2 {
        println!("\n(only one run — pairwise significance tests need at least two runs)");
    } else {
        println!("\n=== pairwise progression (consecutive runs) ===");
        for pair in runs.windows(2) {
            let (earlier, later) = (&pair[0], &pair[1]);
            println!("\n{}  ->  {}", earlier.label, later.label);

            if let (Some(me), Some(ml)) = (&earlier.results.mooneye, &later.results.mooneye) {
                let (improved, regressed) = paired_mooneye(me, ml);
                let r = mcnemar(improved, regressed);
                println!(
                    "  mooneye McNemar: +{} improved / -{} regressed  p={:.4} {} [{}]",
                    r.improved,
                    r.regressed,
                    r.p,
                    stars(r.p, args.alpha),
                    if r.exact { "exact" } else { "chi2+cc" },
                );
                pairwise_rows.push(vec![
                    earlier.label.clone(),
                    later.label.clone(),
                    "mcnemar_mooneye".into(),
                    "-".into(),
                    (r.improved + r.regressed).to_string(),
                    format!("+{}/-{}", r.improved, r.regressed),
                    if r.exact {
                        "exact".into()
                    } else {
                        "chi2+cc".into()
                    },
                    fmt(r.p),
                    stars(r.p, args.alpha).into(),
                ]);
            }

            let tolerances: Vec<usize> = later.results.meta.tolerances.clone();
            for tol in tolerances {
                let (ea, lb) = paired_match_rates(
                    &earlier.results,
                    &later.results,
                    tol,
                    args.include_recordings,
                );
                if ea.is_empty() {
                    continue;
                }
                let diffs: Vec<f64> = lb.iter().zip(&ea).map(|(l, e)| l - e).collect();
                match wilcoxon_signed_rank(&diffs) {
                    Some(w) => {
                        println!(
                            "  tol={tol:<2} Wilcoxon match-rate: n={:<3} W+={:<7.1} median Δ={:+.4} z={:+.3} p={:.4} {}",
                            w.n,
                            w.w_plus,
                            w.median_diff,
                            w.z,
                            w.p,
                            stars(w.p, args.alpha),
                        );
                        pairwise_rows.push(vec![
                            earlier.label.clone(),
                            later.label.clone(),
                            "wilcoxon_match_rate".into(),
                            tol.to_string(),
                            w.n.to_string(),
                            format!("W+={:.1}", w.w_plus),
                            format!("medianΔ={:+.4};z={:+.3}", w.median_diff, w.z),
                            fmt(w.p),
                            stars(w.p, args.alpha).into(),
                        ]);
                    }
                    None => {
                        println!("  tol={tol:<2} Wilcoxon match-rate: no differences (identical)");
                    }
                }
            }
        }
    }
    write_csv(&out_dir.join("pairwise.csv"), &pairwise_rows)?;

    md.push_str("See `per_run.csv`, `mooneye_per_run.csv` and `pairwise.csv` in this directory.\n");
    std::fs::write(out_dir.join("significance.md"), md)?;

    println!("\nwritten to {}", out_dir.display());
    Ok(())
}
