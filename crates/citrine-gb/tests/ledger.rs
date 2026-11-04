mod archives;

use archives::{Source, fetch, ledger_dir, read_entry, read_manifest, sha256_hex};
use citrine_gb::ReadMemory;
#[cfg(feature = "gba")]
use citrine_gb::console::Keys;
use citrine_gb::console::{ConsoleKind, detect};
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::ppu::types::theme::DmgTheme;
use citrine_gb::gb::{GameBoy, GbModel};
#[cfg(feature = "gba")]
use citrine_gb::gba::bios::{Bios, Boot};
#[cfg(feature = "gba")]
use citrine_gb::gba::machine::{Fault, Gba};
use citrine_gb::rom::Rom;
use citrine_gb::rom::id::RomId;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const LD_B_B: u8 = 0x40;
#[cfg(feature = "gba")]
const ARM_SPIN: u32 = 0xEAFF_FFFE;
#[cfg(feature = "gba")]
const THUMB_SPIN: u16 = 0xE7FE;
#[cfg(feature = "gba")]
const BG0CNT: u32 = 0x0400_0008;
#[cfg(feature = "gba")]
const VRAM: u32 = 0x0600_0000;
#[cfg(feature = "gba")]
const MAP_BLOCK: u32 = 0x800;
#[cfg(feature = "gba")]
const CONSOLE_ROWS: u32 = 20;
#[cfg(feature = "gba")]
const CONSOLE_COLUMNS: u32 = 30;
#[cfg(feature = "gba")]
const CONSOLE_PASS: &str = "congratulations!";
#[cfg(feature = "gba")]
const CONSOLE_DONE: &str = "total:";
#[cfg(feature = "gba")]
const CONSOLE_FAIL: &str = "FAIL";
const PASS_REGS: [u8; 6] = [3, 5, 8, 13, 21, 34];
const FAIL_REGS: [u8; 6] = [0x42; 6];
const BLARGG_SIGNATURE: [u8; 3] = [0xDE, 0xB0, 0x61];
const BLARGG_RUNNING: u8 = 0x80;
const MAX_JOBS: usize = 4;
const PRESS_DELAY_FRAMES: u32 = 60;
const PRESS_HOLD_FRAMES: u32 = 5;
const PRESS_STEP_FRAMES: u32 = 10;
const BIOS_END: u32 = 0x4000;
#[cfg(feature = "gba")]
const SPIN_START: u32 = 0x0200_0000;
const BIOS_BOOT_CYCLES: u64 = 100_000_000;

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
    register: Option<u8>,
    roms: Vec<RomEntry>,
}

#[derive(Deserialize)]
struct RomEntry {
    path: String,
    sha256: String,
    consoles: Vec<Console>,
    #[serde(default)]
    images: BTreeMap<Console, String>,
    cycles: Option<u32>,
    register: Option<u8>,
    name: Option<String>,
    #[serde(default)]
    press: Vec<Button>,
    finish: Option<String>,
    unpassable: Option<String>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Rule {
    Fibonacci,
    Blargg,
    Screenshot,
    Timed,
    Register,
    Console,
}

impl Rule {
    fn runs_on(self, kind: ConsoleKind) -> bool {
        match self {
            Rule::Fibonacci | Rule::Blargg => matches!(kind, ConsoleKind::Gb | ConsoleKind::Gbc),
            Rule::Screenshot | Rule::Timed => true,
            Rule::Register | Rule::Console => kind == ConsoleKind::Gba,
        }
    }
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
enum Console {
    Dmg,
    Cgb,
    Gba,
}

impl Console {
    const ALL: [Console; 3] = [Console::Dmg, Console::Cgb, Console::Gba];

    fn name(self) -> &'static str {
        match self {
            Console::Dmg => "dmg",
            Console::Cgb => "cgb",
            Console::Gba => "gba",
        }
    }

    fn kind(self) -> ConsoleKind {
        match self {
            Console::Dmg => ConsoleKind::Gb,
            Console::Cgb => ConsoleKind::Gbc,
            Console::Gba => ConsoleKind::Gba,
        }
    }
}

struct Job {
    suite: String,
    line: String,
    console: Console,
    rule: Rule,
    register: Option<u8>,
    cycles: u32,
    rom: Vec<u8>,
    image: Option<Vec<u8>>,
    press: Vec<Button>,
    finish: Option<String>,
    unpassable: Option<String>,
}

enum Mode {
    Skip,
    Intro,
    Bios(PathBuf),
}

impl Mode {
    fn ledger_file(&self) -> &'static str {
        match self {
            Mode::Skip => "passing.txt",
            Mode::Intro => "passing-intro.txt",
            Mode::Bios(_) => "passing-bios.txt",
        }
    }
}

struct Args {
    write: bool,
    failures: bool,
    jobs: usize,
    mode: Mode,
    only: Vec<String>,
}

#[derive(Default)]
struct Setup {
    #[cfg(feature = "gba")]
    bios: Option<Bios>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        write: false,
        failures: false,
        mode: Mode::Skip,
        only: Vec::new(),
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
            "--bios" => {
                args.mode = Mode::Bios(iter.next().ok_or("--bios needs a file")?.into());
            }
            "--intro" => args.mode = Mode::Intro,
            "--only" => {
                let names = iter.next().ok_or("--only needs ROM names")?;
                args.only = names.split(',').map(str::to_string).collect();
            }
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
            if RomId::new(&rom).sha256_hex_string().to_ascii_lowercase() != entry.sha256 {
                return Err(format!("{suite_name}/{} has a different RomId", entry.path));
            }
            let reads_as_gba = detect(&rom, Some(entry.path.as_str())) == Some(ConsoleKind::Gba)
                || detect(&rom, None) == Some(ConsoleKind::Gba);
            let register = entry.register.or(suite.register);
            match (suite.rule, register) {
                (Rule::Register, Some(0..=15))
                | (
                    Rule::Fibonacci | Rule::Blargg | Rule::Screenshot | Rule::Timed | Rule::Console,
                    None,
                ) => {}
                (Rule::Register, _) => {
                    return Err(format!(
                        "{suite_name}/{} needs a register from 0 to 15",
                        entry.path
                    ));
                }
                (_, Some(_)) => {
                    return Err(format!(
                        "{suite_name}/{} names a register its rule has no use for",
                        entry.path
                    ));
                }
            }
            for &console in &entry.consoles {
                if reads_as_gba != (console.kind() == ConsoleKind::Gba) {
                    return Err(format!(
                        "{suite_name}/{} does not read as a {} game",
                        entry.path,
                        console.name()
                    ));
                }
                if !suite.rule.runs_on(console.kind()) {
                    return Err(format!(
                        "{suite_name}/{} uses a rule {} has no use for",
                        entry.path,
                        console.name()
                    ));
                }
                let image = match suite.rule {
                    Rule::Screenshot | Rule::Timed => {
                        let image = entry.images.get(&console).ok_or_else(|| {
                            format!(
                                "{suite_name}/{} has no {} image",
                                entry.path,
                                console.name()
                            )
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
                        console.name(),
                        entry.name.as_deref().unwrap_or(&entry.path)
                    ),
                    console,
                    rule: suite.rule,
                    register,
                    cycles: entry.cycles.unwrap_or(suite.cycles),
                    rom: rom.clone(),
                    image,
                    press: entry.press.clone(),
                    finish: entry.finish.clone(),
                    unpassable: entry.unpassable.clone(),
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

fn shade(kind: ConsoleKind, px: &[u8]) -> u16 {
    match kind {
        ConsoleKind::Gb => (px[0] as u16 + px[1] as u16 + px[2] as u16 + 127) / 255,
        ConsoleKind::Gbc | ConsoleKind::Gba => {
            (px[0] >> 3) as u16 | ((px[1] >> 3) as u16) << 5 | ((px[2] >> 3) as u16) << 10
        }
    }
}

fn screenshot(frame: &[u8], kind: ConsoleKind, image: &[u8]) -> Result<(), String> {
    if frame.len() != image.len() {
        return Err("reference image has the wrong size".into());
    }
    let differing = frame
        .chunks(4)
        .zip(image.chunks(4))
        .filter(|(a, b)| shade(kind, a) != shade(kind, b))
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

fn run(job: &Job, setup: &Setup) -> Result<(), String> {
    match job.console.kind() {
        ConsoleKind::Gb => run_game_boy(job, GbModel::Dmg),
        ConsoleKind::Gbc => run_game_boy(job, GbModel::Cgb),
        #[cfg(feature = "gba")]
        ConsoleKind::Gba => run_gba(job, setup),
        #[cfg(not(feature = "gba"))]
        ConsoleKind::Gba => Err("no GBA core to run it on".into()),
    }
}

#[cfg(feature = "gba")]
fn console_text(gba: &Gba) -> String {
    let map = VRAM + u32::from(gba.bus().read16(BG0CNT) >> 8 & 0x1F) * MAP_BLOCK;
    (0..CONSOLE_ROWS)
        .map(|row| {
            let line: String = (0..CONSOLE_COLUMNS)
                .map(|column| {
                    let tile = gba.bus().read16(map + (row * 32 + column) * 2) & 0x3FF;
                    char::from_u32(u32::from(tile))
                        .filter(char::is_ascii_graphic)
                        .unwrap_or(' ')
                })
                .collect();
            line.trim().to_string()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(feature = "gba")]
fn boot_gba(job: &Job, setup: &Setup) -> Result<(Gba, u64), String> {
    let Some(bios) = &setup.bios else {
        let gba = Gba::new(&job.rom);
        let booted = gba.cycles();
        return Ok((gba, booted));
    };
    let mut gba = Gba::with_bios(&job.rom, bios.clone(), Boot::Intro);
    while gba.cpu().pc() < BIOS_END {
        if gba.cycles() > BIOS_BOOT_CYCLES {
            return Err(format!(
                "the BIOS did not start the game within {BIOS_BOOT_CYCLES} cycles"
            ));
        }
        gba.run_frame();
        if let Some(fault) = gba.fault() {
            return Err(describe_fault(fault));
        }
    }
    let booted = gba.cycles();
    Ok((gba, booted))
}

#[cfg(feature = "gba")]
fn run_gba(job: &Job, setup: &Setup) -> Result<(), String> {
    let (mut gba, booted) = boot_gba(job, setup)?;
    let image = job.image.as_deref().unwrap_or_default();
    let mut frames = 0;
    let mut shown = String::new();
    while gba.cycles() < booted + u64::from(job.cycles) {
        gba.run_frame();
        if let Some(fault) = gba.fault() {
            return Err(describe_fault(fault));
        }
        frames += 1;
        if let Some((button, down)) = press_step(&job.press, frames) {
            gba.set_keys(if down {
                Keys::from(button.state())
            } else {
                Keys::empty()
            });
        }
        if let Rule::Console = job.rule {
            let text = console_text(&gba);
            if text.contains(CONSOLE_PASS) {
                return Ok(());
            }
            if text.contains(CONSOLE_DONE) && text == shown {
                return Err(text);
            }
            if let Some(finish) = job.finish.as_deref()
                && text.contains(finish)
                && text == shown
            {
                return if text.contains(CONSOLE_FAIL) {
                    Err(text)
                } else {
                    Ok(())
                };
            }
            shown = text;
            continue;
        }
        if !spins(&gba) {
            continue;
        }
        match (job.rule, job.register) {
            (Rule::Register, Some(register)) => {
                return match gba.cpu().reg(usize::from(register)) {
                    0 => Ok(()),
                    failed => Err(format!("test {failed} failed")),
                };
            }
            (Rule::Screenshot, _) => return screenshot(gba.frame(), ConsoleKind::Gba, image),
            _ => {}
        }
    }
    match job.rule {
        Rule::Timed => screenshot(gba.frame(), ConsoleKind::Gba, image),
        Rule::Console => Err(format!("no result within {} cycles: {shown}", job.cycles)),
        _ => Err(format!("no result within {} cycles", job.cycles)),
    }
}

#[cfg(feature = "gba")]
fn spins(gba: &Gba) -> bool {
    let pc = gba.cpu().reg(15);
    let thumb = gba.cpu().thumb();
    let at = pc.wrapping_sub(if thumb { 4 } else { 8 });
    if at < SPIN_START {
        return false;
    }
    if thumb {
        gba.bus().read16(at) == THUMB_SPIN
    } else {
        gba.bus().read32(at) == ARM_SPIN
    }
}

#[cfg(feature = "gba")]
fn describe_fault(fault: Fault) -> String {
    match fault {
        Fault::Unsupported(addr) => {
            format!("stopped at {addr:08X}, an instruction the CPU can't run yet")
        }
    }
}

fn run_game_boy(job: &Job, model: GbModel) -> Result<(), String> {
    let kind = job.console.kind();
    let mut gb = GameBoy::new_empty(model);
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
                return screenshot(
                    gb.frame().as_slice(),
                    kind,
                    job.image.as_deref().unwrap_or_default(),
                );
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
        Rule::Timed => screenshot(
            gb.frame().as_slice(),
            kind,
            job.image.as_deref().unwrap_or_default(),
        ),
        _ => Err(format!("no result within {} cycles", job.cycles)),
    }
}

fn press_step(buttons: &[Button], frame: u32) -> Option<(Button, bool)> {
    let since = frame.checked_sub(PRESS_DELAY_FRAMES)?;
    let button = *buttons.get((since / PRESS_STEP_FRAMES) as usize)?;
    match since % PRESS_STEP_FRAMES {
        0 => Some((button, true)),
        PRESS_HOLD_FRAMES => Some((button, false)),
        _ => None,
    }
}

fn press(gb: &mut GameBoy, buttons: &[Button], frame: u32) {
    match press_step(buttons, frame) {
        Some((button, true)) => gb.press_button(button.state()),
        Some((button, false)) => gb.release_button(button.state()),
        None => {}
    }
}

fn run_all(jobs: &[Job], setup: &Setup, threads: usize) -> Vec<Result<(), String>> {
    let next = AtomicUsize::new(0);
    let results = Mutex::new((0..jobs.len()).map(|_| None).collect::<Vec<_>>());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let result = std::panic::catch_unwind(|| run(job, setup))
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
    let mut counts: BTreeMap<&str, BTreeMap<Console, (usize, usize)>> = BTreeMap::new();
    for (job, result) in jobs.iter().zip(results) {
        if job.unpassable.is_some() {
            continue;
        }
        for suite in [job.suite.as_str(), "total"] {
            let count = counts
                .entry(suite)
                .or_default()
                .entry(job.console)
                .or_default();
            count.0 += result.is_ok() as usize;
            count.1 += 1;
        }
    }
    let total = counts.remove("total").unwrap_or_default();
    let header: String = Console::ALL.map(|c| format!("{:>10}", c.name())).concat();
    println!("\n{:<12}{header}", "suite");
    for (suite, consoles) in counts.into_iter().chain([("total", total)]) {
        let cells: String = Console::ALL
            .map(|console| match consoles.get(&console) {
                Some((passed, all)) => format!("{:>10}", format!("{passed}/{all}")),
                None => format!("{:>10}", "-"),
            })
            .concat();
        println!("{suite:<12}{cells}");
    }
}

#[cfg(feature = "gba")]
fn setup(args: &Args) -> Result<Option<Setup>, String> {
    let path = match &args.mode {
        Mode::Skip => return Ok(Some(Setup::default())),
        Mode::Intro => {
            return Ok(Some(Setup {
                bios: Some(Bios::built_in()),
            }));
        }
        Mode::Bios(path) => path,
    };
    let image = match std::fs::read(path) {
        Ok(image) => image,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let bios = Bios::user(&image).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(Setup { bios: Some(bios) }))
}

#[cfg(not(feature = "gba"))]
fn setup(args: &Args) -> Result<Option<Setup>, String> {
    match args.mode {
        Mode::Skip => Ok(Some(Setup::default())),
        Mode::Intro => Err("--intro needs the gba feature".into()),
        Mode::Bios(path) => Err(format!("--bios {} needs the gba feature", path.display())),
    }
}

fn ledger() -> Result<ExitCode, String> {
    let args = parse_args()?;
    let Some(setup) = setup(&args)? else {
        println!("no BIOS file, skipping the BIOS ledger");
        return Ok(ExitCode::SUCCESS);
    };
    let ledger_path = ledger_dir().join(args.mode.ledger_file());
    let manifest: Manifest = read_manifest()?;
    let listed: BTreeSet<String> = std::fs::read_to_string(&ledger_path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect();

    let mut jobs = load_jobs(&manifest)?;
    if !matches!(args.mode, Mode::Skip) {
        jobs.retain(|job| job.console == Console::Gba);
    }
    if !args.only.is_empty() {
        jobs.retain(|job| {
            args.only
                .iter()
                .any(|name| job.line.contains(name.as_str()))
        });
    }
    println!(
        "running {} ROM and console pairs on {} threads",
        jobs.len(),
        args.jobs
    );
    let start = Instant::now();
    let results = run_all(&jobs, &setup, args.jobs);
    println!("done in {:.1}s", start.elapsed().as_secs_f64());

    if args.failures {
        println!();
        for (job, result) in jobs.iter().zip(&results) {
            if let Err(reason) = result
                && job.unpassable.is_none()
            {
                println!("FAIL {}: {reason}", job.line);
            }
        }
    }
    print_summary(&jobs, &results);

    let mut unpassable_passes = Vec::new();
    for (job, result) in jobs.iter().zip(&results) {
        let Some(why) = &job.unpassable else { continue };
        if result.is_ok() {
            unpassable_passes.push(job.line.as_str());
        }
        let outcome = match result {
            Ok(()) => "PASSES".to_string(),
            Err(reason) => reason.clone(),
        };
        println!(
            "\nknown unpassable, not counted: {}\n  why: {why}\n  result: {outcome}",
            job.line
        );
    }

    let mut outcomes: BTreeMap<&str, &Result<(), String>> = BTreeMap::new();
    for (job, result) in jobs.iter().zip(&results) {
        if job.unpassable.is_none() {
            outcomes.insert(job.line.as_str(), result);
        }
    }
    let passing: BTreeSet<String> = outcomes
        .iter()
        .filter(|(_, result)| result.is_ok())
        .map(|(line, _)| line.to_string())
        .collect();
    let dropped: Vec<&String> = listed
        .difference(&passing)
        .filter(|line| args.only.is_empty() || outcomes.contains_key(line.as_str()))
        .collect();
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

    if !unpassable_passes.is_empty() {
        println!("\n{} ROMs marked unpassable pass:", unpassable_passes.len());
        for line in &unpassable_passes {
            println!("  ! {line}");
        }
        println!("check the pass is real, then drop its unpassable mark in the manifest");
    }

    if dropped.is_empty() {
        return Ok(if unpassable_passes.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
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
