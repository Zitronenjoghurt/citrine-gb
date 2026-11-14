use crate::callgrind::Profile;
use crate::manifest::{Manifest, Workload};
use crate::readings::{Reading, Readings, peak_kb};
use crate::{compare, flame, roms, run};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: citrine-gb-bench <command>
  list [--extra DIR]                     workload ids, one per line
  run [--extra DIR] [--only TEXT] [--repeat N] [--out FILE]
                                         run and check every workload, wall time per run
  one ID [--extra DIR]                   run and check one workload once (for callgrind)
  bless ID... | --all [--extra DIR]      store the current output as expected
  record --out FILE [--ir PROFILE=DIR] [--readings FILE] [--size NAME=BYTES] [--meta KEY=VALUE]
                                         merge readings, callgrind counts and sizes
  compare BASE HEAD [--profile NAME]     deltas with a verdict per workload
  flame OUT.svg CALLGRIND [--base CALLGRIND] [--title TEXT]";

struct Args {
    positional: Vec<String>,
    options: Vec<(String, String)>,
    flags: Vec<String>,
}

impl Args {
    fn parse(raw: Vec<String>) -> Self {
        let mut args = Args {
            positional: Vec::new(),
            options: Vec::new(),
            flags: Vec::new(),
        };
        let mut iter = raw.into_iter();
        while let Some(arg) = iter.next() {
            match arg.strip_prefix("--") {
                Some("all") => args.flags.push("all".into()),
                Some(name) => {
                    let value = iter.next().unwrap_or_default();
                    args.options.push((name.to_string(), value));
                }
                None => args.positional.push(arg),
            }
        }
        args
    }

    fn one(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn all(&self, name: &str) -> impl Iterator<Item = &str> {
        self.options
            .iter()
            .filter(move |(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn manifest(&self) -> Result<Manifest, String> {
        match self.one("extra") {
            Some(dir) => Manifest::load(Path::new(dir)),
            None => Manifest::load(&Manifest::public()),
        }
    }
}

pub(crate) fn main(raw: Vec<String>) -> ExitCode {
    let mut raw = raw;
    if raw.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let command = raw.remove(0);
    let args = Args::parse(raw);
    let result = match command.as_str() {
        "list" => list(&args),
        "run" => run_all(&args),
        "one" => one(&args),
        "bless" => bless(&args),
        "record" => record(&args),
        "compare" => compare_files(&args),
        "flame" => flame_file(&args),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn list(args: &Args) -> Result<bool, String> {
    for workload in &args.manifest()?.workloads {
        println!("{}", workload.id);
    }
    Ok(true)
}

fn rom_for(workload: &Workload, manifest: &Manifest) -> Result<Option<Vec<u8>>, String> {
    workload
        .rom
        .as_ref()
        .map(|source| roms::load(source, &manifest.dir))
        .transpose()
}

fn check(manifest: &Manifest, workload: &Workload, outcome: &run::Outcome) -> Vec<String> {
    match manifest.expected.get(&workload.id) {
        Some(expected) => outcome.digest.differences(expected),
        None => vec!["no expected output, run bless first".to_string()],
    }
}

fn run_all(args: &Args) -> Result<bool, String> {
    let manifest = args.manifest()?;
    let repeat: usize = args
        .one("repeat")
        .map_or(Ok(1), str::parse)
        .map_err(|e| format!("--repeat: {e}"))?;
    let only = args.one("only");
    let chosen: Vec<&Workload> = manifest
        .workloads
        .iter()
        .filter(|workload| only.is_none_or(|text| workload.id.contains(text)))
        .collect();
    let mut loaded = Vec::new();
    for workload in &chosen {
        loaded.push(rom_for(workload, &manifest)?);
    }
    let mut readings = Readings::default();
    for _ in 0..repeat {
        for (workload, rom) in chosen.iter().zip(&loaded) {
            let reading = readings
                .workloads
                .entry(workload.id.clone())
                .or_insert_with(|| Reading {
                    console: workload.console.name().to_string(),
                    ..Reading::default()
                });
            match run::run(workload, rom.as_deref()) {
                Ok(outcome) => {
                    reading.wall_ms.push(outcome.wall.as_secs_f64() * 1000.0);
                    let failures = check(&manifest, workload, &outcome);
                    if reading.digest.is_none() || !failures.is_empty() {
                        reading.failures.extend(failures);
                    }
                    reading.digest.get_or_insert(outcome.digest);
                }
                Err(message) => reading.failures.push(message),
            }
        }
    }
    readings
        .meta
        .insert("peak_kb".into(), peak_kb().unwrap_or(0).to_string());
    println!("| workload | console | median ms | frames/s | check |\n|---|---|---|---|---|");
    for workload in &chosen {
        let reading = &readings.workloads[&workload.id];
        let mut times = reading.wall_ms.clone();
        times.sort_by(f64::total_cmp);
        let middle = times.get(times.len() / 2).copied().unwrap_or(0.0);
        println!(
            "| {} | {} | {middle:.2} | {:.0} | {} |",
            workload.id,
            reading.console,
            if middle > 0.0 {
                f64::from(workload.frames) / middle * 1000.0
            } else {
                0.0
            },
            if reading.failures.is_empty() {
                "ok"
            } else {
                "FAIL"
            }
        );
    }
    let failed = readings.failed();
    for (id, failures) in &failed {
        eprintln!("FAIL {id}: {}", failures.join("; "));
    }
    let ok = failed.is_empty();
    if let Some(out) = args.one("out") {
        readings.save(Path::new(out))?;
    }
    Ok(ok)
}

fn one(args: &Args) -> Result<bool, String> {
    let manifest = args.manifest()?;
    let id = args.positional.first().ok_or(USAGE)?;
    let workload = manifest
        .find(id)
        .ok_or_else(|| format!("no workload {id}"))?;
    let rom = rom_for(workload, &manifest)?;
    let outcome = run::run(workload, rom.as_deref())?;
    let failures = check(&manifest, workload, &outcome);
    println!(
        "{id} {:.2} ms peak {} kB {}",
        outcome.wall.as_secs_f64() * 1000.0,
        peak_kb().unwrap_or(0),
        if failures.is_empty() { "ok" } else { "FAIL" }
    );
    for failure in &failures {
        eprintln!("FAIL {id}: {failure}");
    }
    Ok(failures.is_empty())
}

fn bless(args: &Args) -> Result<bool, String> {
    let mut manifest = args.manifest()?;
    let ids: Vec<String> = if args.flags.iter().any(|flag| flag == "all") {
        manifest.workloads.iter().map(|w| w.id.clone()).collect()
    } else {
        args.positional.clone()
    };
    if ids.is_empty() {
        return Err(USAGE.to_string());
    }
    for id in ids {
        let workload = manifest
            .find(&id)
            .ok_or_else(|| format!("no workload {id}"))?
            .clone();
        let rom = rom_for(&workload, &manifest)?;
        let outcome = run::run(&workload, rom.as_deref())?;
        let old = manifest.expected.insert(id.clone(), outcome.digest.clone());
        let changes = old.map_or_else(
            || vec!["new".to_string()],
            |old| outcome.digest.differences(&old),
        );
        println!(
            "{id}: {}",
            if changes.is_empty() {
                "unchanged".to_string()
            } else {
                changes.join("; ")
            }
        );
    }
    manifest.save_expected()?;
    Ok(true)
}

fn record(args: &Args) -> Result<bool, String> {
    let out = PathBuf::from(args.one("out").ok_or(USAGE)?);
    let mut readings = Readings::default();
    for path in args.all("readings") {
        readings.absorb(Readings::load(Path::new(path))?);
    }
    for spec in args.all("ir") {
        let (profile, dir) = spec.split_once('=').ok_or("--ir wants PROFILE=DIR")?;
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| format!("{dir}: {e}"))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|e| e == "out"))
            .collect();
        entries.sort();
        for path in entries {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let id = stem.rsplit_once('.').map_or(stem, |(id, _)| id).to_string();
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            readings
                .workloads
                .entry(id)
                .or_default()
                .ir
                .entry(profile.to_string())
                .or_default()
                .push(Profile::parse(&text).total);
        }
    }
    for spec in args.all("size") {
        let (name, bytes) = spec.split_once('=').ok_or("--size wants NAME=BYTES")?;
        readings.sizes.insert(
            name.to_string(),
            bytes.parse().map_err(|e| format!("{spec}: {e}"))?,
        );
    }
    for spec in args.all("meta") {
        let (key, value) = spec.split_once('=').ok_or("--meta wants KEY=VALUE")?;
        readings.meta.insert(key.to_string(), value.to_string());
    }
    for spec in args.all("peak") {
        let (id, kb) = spec.split_once('=').ok_or("--peak wants ID=KB")?;
        readings
            .workloads
            .entry(id.to_string())
            .or_default()
            .peak_kb = kb.parse().ok();
    }
    if let Ok(manifest) = args.manifest() {
        for workload in &manifest.workloads {
            if let Some(reading) = readings.workloads.get_mut(&workload.id)
                && reading.console.is_empty()
            {
                reading.console = workload.console.name().to_string();
            }
        }
    }
    for (id, reading) in &mut readings.workloads {
        if reading.console.is_empty() && id.starts_with("app-") {
            reading.console = "App".to_string();
        }
    }
    readings.save(&out)?;
    Ok(readings.failed().is_empty())
}

fn compare_files(args: &Args) -> Result<bool, String> {
    let [base, head] = args.positional.as_slice() else {
        return Err(USAGE.to_string());
    };
    let profile = args.one("profile").unwrap_or("dist");
    let (text, ok) = compare::report(
        &Readings::load(Path::new(base))?,
        &Readings::load(Path::new(head))?,
        profile,
    );
    print!("{text}");
    Ok(ok)
}

fn flame_file(args: &Args) -> Result<bool, String> {
    let [out, head] = args.positional.as_slice() else {
        return Err(USAGE.to_string());
    };
    let title = args.one("title").unwrap_or("citrine-gb");
    flame::render(
        title,
        Path::new(head),
        args.one("base").map(Path::new),
        Path::new(out),
    )?;
    Ok(true)
}
