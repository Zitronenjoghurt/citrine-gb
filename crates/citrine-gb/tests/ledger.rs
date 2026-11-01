mod archives;

use archives::{Source, fetch, ledger_dir, read_entry, read_manifest, sha256_hex};
use citrine_gb::ReadMemory;
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::ppu::types::theme::DmgTheme;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::rom::Rom;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const LD_B_B: u8 = 0x40;
const PASS_REGS: [u8; 6] = [3, 5, 8, 13, 21, 34];
const FAIL_REGS: [u8; 6] = [0x42; 6];
const BLARGG_SIGNATURE: [u8; 3] = [0xDE, 0xB0, 0x61];
const BLARGG_RUNNING: u8 = 0x80;
const MAX_JOBS: usize = 4;
const PRESS_DELAY_FRAMES: u32 = 60;
const PRESS_HOLD_FRAMES: u32 = 5;
const PRESS_STEP_FRAMES: u32 = 10;

#[derive(Deserialize)]
struct Manifest {
    sources: BTreeMap<String, Source>,
    suites: BTreeMap<String, Suite>,
}

#[derive(Deserialize)]
struct Suite {
    source: String,
    root: String,
    rule: Rule,
    cycles: u32,
    roms: Vec<RomEntry>,
}

#[derive(Deserialize)]
struct RomEntry {
    path: String,
    sha256: String,
    models: Vec<Model>,
    #[serde(default)]
    images: BTreeMap<Model, String>,
    cycles: Option<u32>,
    name: Option<String>,
    #[serde(default)]
    press: Vec<Button>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Rule {
    Fibonacci,
    Blargg,
    Screenshot,
    Timed,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Button {
    A,
    B,
    Select,
    Start,
    Right,
    Left,
    Up,
    Down,
}

impl Button {
    fn state(self) -> JoypadState {
        match self {
            Button::A => JoypadState::A,
            Button::B => JoypadState::B,
            Button::Select => JoypadState::SELECT,
            Button::Start => JoypadState::START,
            Button::Right => JoypadState::RIGHT,
            Button::Left => JoypadState::LEFT,
            Button::Up => JoypadState::UP,
            Button::Down => JoypadState::DOWN,
        }
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
enum Model {
    Dmg,
    Cgb,
}

impl Model {
    const ALL: [Model; 2] = [Model::Dmg, Model::Cgb];

    fn name(self) -> &'static str {
        match self {
            Model::Dmg => "dmg",
            Model::Cgb => "cgb",
        }
    }

    fn gb_model(self) -> GbModel {
        match self {
            Model::Dmg => GbModel::Dmg,
            Model::Cgb => GbModel::Cgb,
        }
    }
}

struct Job {
    suite: String,
    line: String,
    model: Model,
    rule: Rule,
    cycles: u32,
    rom: Vec<u8>,
    image: Option<Vec<u8>>,
    press: Vec<Button>,
}

struct Args {
    write: bool,
    failures: bool,
    jobs: usize,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        write: false,
        failures: false,
        jobs: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(MAX_JOBS),
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--write" => args.write = true,
            "--failures" => args.failures = true,
            "--jobs" => {
                args.jobs = iter
                    .next()
                    .and_then(|n| n.parse().ok())
                    .filter(|&n| n > 0)
                    .ok_or("--jobs needs a number above 0")?;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

fn decode_png(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let pixels = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 0xFF]).collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Rgb => pixels
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 0xFF])
            .collect(),
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Indexed => return Err("indexed image was not expanded".into()),
    };
    Ok(rgba)
}

fn load_jobs(manifest: &Manifest) -> Result<Vec<Job>, String> {
    let mut archives = BTreeMap::new();
    for (name, source) in &manifest.sources {
        archives.insert(name.as_str(), fetch(name, source)?);
    }

    let mut jobs = Vec::new();
    for (suite_name, suite) in &manifest.suites {
        let archive = archives
            .get_mut(suite.source.as_str())
            .ok_or_else(|| format!("suite {suite_name} uses unknown source {}", suite.source))?;
        for entry in &suite.roms {
            let rom = read_entry(archive, &format!("{}/{}", suite.root, entry.path))?;
            let actual = sha256_hex(&rom);
            if actual != entry.sha256 {
                return Err(format!(
                    "{suite_name}/{} has SHA-256 {actual}, the manifest pins {}",
                    entry.path, entry.sha256
                ));
            }
            for &model in &entry.models {
                let image = match suite.rule {
                    Rule::Screenshot | Rule::Timed => {
                        let image = entry.images.get(&model).ok_or_else(|| {
                            format!("{suite_name}/{} has no {} image", entry.path, model.name())
                        })?;
                        let data = read_entry(archive, &format!("{}/{image}", suite.root))?;
                        Some(decode_png(&data).map_err(|e| format!("{image}: {e}"))?)
                    }
                    _ => None,
                };
                jobs.push(Job {
                    suite: suite_name.clone(),
                    line: format!(
                        "{} {suite_name}/{}",
                        model.name(),
                        entry.name.as_deref().unwrap_or(&entry.path)
                    ),
                    model,
                    rule: suite.rule,
                    cycles: entry.cycles.unwrap_or(suite.cycles),
                    rom: rom.clone(),
                    image,
                    press: entry.press.clone(),
                });
            }
        }
    }
    Ok(jobs)
}

fn fibonacci(gb: &GameBoy) -> Result<(), String> {
    let regs = [gb.cpu.b, gb.cpu.c, gb.cpu.d, gb.cpu.e, gb.cpu.h, gb.cpu.l];
    match regs {
        PASS_REGS => Ok(()),
        FAIL_REGS => Err("reported failure".into()),
        _ => Err(format!("stopped with registers {regs:02X?}")),
    }
}

fn shade(model: Model, px: &[u8]) -> u16 {
    match model {
        Model::Dmg => (px[0] as u16 + px[1] as u16 + px[2] as u16 + 127) / 255,
        Model::Cgb => {
            (px[0] >> 3) as u16 | ((px[1] >> 3) as u16) << 5 | ((px[2] >> 3) as u16) << 10
        }
    }
}

fn screenshot(gb: &GameBoy, model: Model, image: &[u8]) -> Result<(), String> {
    let frame = gb.frame().as_slice();
    if frame.len() != image.len() {
        return Err("reference image has the wrong size".into());
    }
    let differing = frame
        .chunks(4)
        .zip(image.chunks(4))
        .filter(|(a, b)| shade(model, a) != shade(model, b))
        .count();
    match differing {
        0 => Ok(()),
        n => Err(format!("{n} pixels differ from the reference")),
    }
}

#[derive(Default)]
struct Blargg {
    serial: String,
    running: bool,
}

impl Blargg {
    fn poll(&mut self, gb: &mut GameBoy) -> Option<Result<(), String>> {
        if let Some(byte) = gb.serial.take_sent() {
            self.serial.push(byte as char);
            if self.serial.contains("Passed") {
                return Some(Ok(()));
            }
            if self.serial.contains("Failed") {
                return Some(Err(self
                    .serial
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")));
            }
        }

        let signature = [0xA001, 0xA002, 0xA003].map(|addr| gb.cartridge.read_naive(addr));
        if signature != BLARGG_SIGNATURE {
            return None;
        }
        match gb.cartridge.read_naive(0xA000) {
            BLARGG_RUNNING => self.running = true,
            0 if self.running => return Some(Ok(())),
            code if self.running => {
                let text: String = (0xA004..0xC000)
                    .map(|addr| gb.cartridge.read_naive(addr))
                    .take_while(|&byte| byte != 0)
                    .map(|byte| byte as char)
                    .collect();
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                return Some(Err(format!("result code {code:02X}: {text}")));
            }
            _ => {}
        }
        None
    }
}

fn run(job: &Job) -> Result<(), String> {
    let mut gb = GameBoy::new_empty(job.model.gb_model());
    gb.load_rom(&Rom::new(&job.rom))
        .map_err(|e| format!("failed to load: {e:?}"))?;
    gb.ppu.dmg_theme = DmgTheme::GreyScale;

    let mut blargg = Blargg::default();
    let mut frames = 0;
    while gb.cycle_counter < job.cycles {
        gb.step();
        if gb.ppu.frame_ready {
            gb.ppu.frame_ready = false;
            gb.apu.flush_audio();
            gb.apu.audio_buffer.clear();
            frames += 1;
            press(&mut gb, &job.press, frames);
        }
        match job.rule {
            Rule::Fibonacci if gb.cpu.ir == LD_B_B => return fibonacci(&gb),
            Rule::Screenshot if gb.cpu.ir == LD_B_B => {
                return screenshot(&gb, job.model, job.image.as_deref().unwrap_or_default());
            }
            Rule::Blargg => {
                if let Some(result) = blargg.poll(&mut gb) {
                    return result;
                }
            }
            _ => {}
        }
    }
    match job.rule {
        Rule::Timed => screenshot(&gb, job.model, job.image.as_deref().unwrap_or_default()),
        _ => Err(format!("no result within {} cycles", job.cycles)),
    }
}

fn press(gb: &mut GameBoy, buttons: &[Button], frame: u32) {
    let Some(since) = frame.checked_sub(PRESS_DELAY_FRAMES) else {
        return;
    };
    let Some(button) = buttons.get((since / PRESS_STEP_FRAMES) as usize) else {
        return;
    };
    match since % PRESS_STEP_FRAMES {
        0 => gb.press_button(button.state()),
        PRESS_HOLD_FRAMES => gb.release_button(button.state()),
        _ => {}
    }
}

fn run_all(jobs: &[Job], threads: usize) -> Vec<Result<(), String>> {
    let next = AtomicUsize::new(0);
    let results = Mutex::new((0..jobs.len()).map(|_| None).collect::<Vec<_>>());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let result = std::panic::catch_unwind(|| run(job))
                        .unwrap_or_else(|_| Err("panicked".into()));
                    results.lock().unwrap()[i] = Some(result);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .flatten()
        .collect()
}

fn print_summary(jobs: &[Job], results: &[Result<(), String>]) {
    let mut counts: BTreeMap<&str, BTreeMap<Model, (usize, usize)>> = BTreeMap::new();
    for (job, result) in jobs.iter().zip(results) {
        for suite in [job.suite.as_str(), "total"] {
            let count = counts
                .entry(suite)
                .or_default()
                .entry(job.model)
                .or_default();
            count.0 += result.is_ok() as usize;
            count.1 += 1;
        }
    }
    let total = counts.remove("total").unwrap_or_default();
    println!("\n{:<12}{:>10}{:>10}", "suite", "dmg", "cgb");
    for (suite, models) in counts.into_iter().chain([("total", total)]) {
        let cells = Model::ALL.map(|model| match models.get(&model) {
            Some((passed, all)) => format!("{passed}/{all}"),
            None => "-".to_string(),
        });
        println!("{suite:<12}{:>10}{:>10}", cells[0], cells[1]);
    }
}

fn ledger() -> Result<ExitCode, String> {
    let args = parse_args()?;
    let ledger_path = ledger_dir().join("passing.txt");
    let manifest: Manifest = read_manifest()?;
    let listed: BTreeSet<String> = std::fs::read_to_string(&ledger_path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect();

    let jobs = load_jobs(&manifest)?;
    println!(
        "running {} ROM and model pairs on {} threads",
        jobs.len(),
        args.jobs
    );
    let start = Instant::now();
    let results = run_all(&jobs, args.jobs);
    println!("done in {:.1}s", start.elapsed().as_secs_f64());

    if args.failures {
        println!();
        for (job, result) in jobs.iter().zip(&results) {
            if let Err(reason) = result {
                println!("FAIL {}: {reason}", job.line);
            }
        }
    }
    print_summary(&jobs, &results);

    let mut outcomes: BTreeMap<&str, &Result<(), String>> = BTreeMap::new();
    for (job, result) in jobs.iter().zip(&results) {
        outcomes.insert(job.line.as_str(), result);
    }
    let passing: BTreeSet<String> = outcomes
        .iter()
        .filter(|(_, result)| result.is_ok())
        .map(|(line, _)| line.to_string())
        .collect();
    let dropped: Vec<&String> = listed.difference(&passing).collect();
    let added: Vec<&String> = passing.difference(&listed).collect();

    if !added.is_empty() {
        println!("\n{} new passes:", added.len());
        for line in &added {
            println!("  + {line}");
        }
    }

    if args.write {
        let mut lines: Vec<&String> = listed.union(&passing).collect();
        lines.sort();
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(&ledger_path, text)
            .map_err(|e| format!("{}: {e}", ledger_path.display()))?;
        println!("\n{} lines in {}", lines.len(), ledger_path.display());
    } else if !added.is_empty() {
        println!("run `make ledger` to add them");
    }

    if dropped.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    println!("\n{} ledger lines no longer pass:", dropped.len());
    for line in &dropped {
        let reason = match outcomes.get(line.as_str()) {
            Some(Err(reason)) => reason.as_str(),
            _ => "not in the manifest",
        };
        println!("  - {line}: {reason}");
    }
    println!("fix them, or remove a line by hand and say why in the commit");
    Ok(ExitCode::FAILURE)
}

fn main() -> ExitCode {
    ledger().unwrap_or_else(|e| {
        eprintln!("ledger: {e}");
        ExitCode::FAILURE
    })
}
