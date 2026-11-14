mod archives;

use archives::{Source, cache_dir, fetch, read_entry, read_manifest, sha256_hex};
use citrine_gb::console::{Keys, Rollback};
use citrine_gb::gba::machine::Gba;
use citrine_gb::nds::machine::{JitCache, Nds, NdsFirmware};
use citrine_gb_threads::{Mode, Threads};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

const PRESS_DELAY_FRAMES: u32 = 60;
const PRESS_HOLD_FRAMES: u32 = 5;
const PRESS_STEP_FRAMES: u32 = 10;
const STATE_EVERY: u32 = 60;
const FRAME_CYCLES: u32 = 560_190;
const GBA_FRAME_CYCLES: u32 = 280_896;
const CLOCK: i64 = 1_762_300_800;

#[derive(Deserialize)]
struct Manifest {
    sources: BTreeMap<String, LedgerSource>,
    suites: BTreeMap<String, Suite>,
}

#[derive(Deserialize)]
struct LedgerSource {
    #[serde(flatten)]
    source: Source,
    #[serde(default)]
    built: bool,
}

#[derive(Deserialize)]
struct Suite {
    source: String,
    root: String,
    cycles: u32,
    roms: Vec<RomEntry>,
}

#[derive(Deserialize)]
struct RomEntry {
    path: String,
    sha256: String,
    consoles: Vec<String>,
    cycles: Option<u32>,
    name: Option<String>,
    #[serde(default)]
    press: Vec<String>,
}

trait Machine: Sized {
    type Ends;
    const CONSOLE: &'static str;
    const CACHES: bool = false;
    const FRAME_CYCLES: u32;

    fn boot(rom: &[u8], jit: bool) -> Self;
    fn jit_on(&self) -> bool;
    fn prepare(&mut self);
    fn compile_on_threads(&mut self) {}
    fn record(&mut self);
    fn take(&mut self) -> (Self::Ends, usize, usize);
    fn expect(&mut self, ends: Self::Ends);
    fn verdict(&mut self) -> Result<(), String>;
    fn run_frame(&mut self);
    fn picture(&self) -> &[u8];
    fn set_keys(&mut self, keys: Keys);
    fn state(&self) -> Vec<u8>;
    fn load(&self, state: &[u8]) -> Self;
    fn stats(&self) -> String;
    fn set_cache(&mut self, _cache: Option<Arc<JitCache>>) {}
    fn mismatched(&self) -> u64 {
        0
    }
    fn differences(&self, _other: &Self) -> String {
        String::new()
    }
}

impl Machine for Nds {
    type Ends = citrine_gb::nds::machine::lockstep::Ends;
    const CONSOLE: &'static str = "nds";
    const CACHES: bool = true;
    const FRAME_CYCLES: u32 = FRAME_CYCLES;

    fn boot(rom: &[u8], jit: bool) -> Self {
        let mut nds = Nds::new(rom.into(), NdsFirmware::default()).expect("a DS ROM");
        nds.set_jit(jit);
        nds
    }

    fn jit_on(&self) -> bool {
        Nds::jit_on(self)
    }

    fn prepare(&mut self) {
        self.set_clock(CLOCK);
    }

    fn compile_on_threads(&mut self) {
        self.compile_jit_on(&Threads::new(Mode::Threads).spawner());
    }

    fn record(&mut self) {
        self.lockstep().record();
    }

    fn take(&mut self) -> (Self::Ends, usize, usize) {
        let ends = self.lockstep().take();
        let (blocks, arm7) = (ends.len(), ends.arm7());
        (ends, blocks, arm7)
    }

    fn expect(&mut self, ends: Self::Ends) {
        self.lockstep().expect(ends);
    }

    fn verdict(&mut self) -> Result<(), String> {
        self.lockstep().verdict()
    }

    fn run_frame(&mut self) {
        Nds::run_frame(self);
    }

    fn picture(&self) -> &[u8] {
        self.frame()
    }

    fn set_keys(&mut self, keys: Keys) {
        Nds::set_keys(self, keys);
    }

    fn state(&self) -> Vec<u8> {
        self.encode_state().expect("a state encodes")
    }

    fn load(&self, state: &[u8]) -> Self {
        self.decode_state(state).expect("the state loads")
    }

    fn stats(&self) -> String {
        format!("{:?}, ARM7: {:?}", self.jit_stats(), self.arm7_jit_stats())
    }

    fn set_cache(&mut self, cache: Option<Arc<JitCache>>) {
        self.set_jit_cache(cache);
    }

    fn mismatched(&self) -> u64 {
        let arm9 = self.jit_stats().map_or(0, |stats| stats.mismatched);
        let arm7 = self.arm7_jit_stats().map_or(0, |stats| stats.mismatched);
        arm9 + arm7
    }
}

impl Machine for Gba {
    type Ends = Vec<citrine_gb::jit::lockstep::BlockEnd<false>>;
    const CONSOLE: &'static str = "gba";
    const FRAME_CYCLES: u32 = GBA_FRAME_CYCLES;

    fn boot(rom: &[u8], jit: bool) -> Self {
        let mut gba = Gba::new(rom);
        gba.set_jit(jit);
        gba
    }

    fn jit_on(&self) -> bool {
        Gba::jit_on(self)
    }

    fn prepare(&mut self) {}

    fn compile_on_threads(&mut self) {
        self.compile_jit_on(&Threads::new(Mode::Threads).spawner());
    }

    fn record(&mut self) {
        self.lockstep().record();
    }

    fn take(&mut self) -> (Self::Ends, usize, usize) {
        let ends = self.lockstep().take();
        let blocks = ends.len();
        (ends, blocks, 0)
    }

    fn expect(&mut self, ends: Self::Ends) {
        self.lockstep().expect(ends);
    }

    fn verdict(&mut self) -> Result<(), String> {
        self.lockstep().verdict("GBA")
    }

    fn run_frame(&mut self) {
        Gba::run_frame(self);
    }

    fn picture(&self) -> &[u8] {
        self.frame()
    }

    fn set_keys(&mut self, keys: Keys) {
        Gba::set_keys(self, keys);
    }

    fn state(&self) -> Vec<u8> {
        self.encode_state().expect("a state encodes")
    }

    fn load(&self, state: &[u8]) -> Self {
        self.decode_state(state).expect("the state loads")
    }

    fn stats(&self) -> String {
        format!("{:?}", self.jit_stats())
    }

    fn set_cache(&mut self, cache: Option<Arc<JitCache>>) {
        self.set_jit_cache(cache);
    }

    fn mismatched(&self) -> u64 {
        self.jit_stats().map_or(0, |stats| stats.mismatched)
    }

    fn differences(&self, other: &Self) -> String {
        let mut found = Vec::new();
        let value = |gba: &Gba| {
            serde_json::json!({
                "cpu": serde_json::to_value(gba.cpu()).expect("the CPU serializes"),
                "bus": serde_json::to_value(gba.bus()).expect("the bus serializes"),
            })
        };
        differ("", &value(self), &value(other), &mut found);
        found.join("; ")
    }
}

struct Peers<M> {
    jit: M,
    interpreter: M,
    blocks: usize,
    arm7_blocks: usize,
}

impl<M: Machine> Peers<M> {
    fn boot(rom: &[u8]) -> Self {
        Self::boot_with(rom, None)
    }

    fn boot_with(rom: &[u8], cache: Option<Arc<JitCache>>) -> Self {
        let mut jit = M::boot(rom, true);
        jit.set_cache(cache);
        Self::pair(jit, M::boot(rom, false))
    }

    fn pair(mut jit: M, mut interpreter: M) -> Self {
        assert!(jit.jit_on(), "the JIT is on for this platform");
        assert!(!interpreter.jit_on());
        jit.compile_on_threads();
        jit.prepare();
        interpreter.prepare();
        jit.record();
        Self {
            jit,
            interpreter,
            blocks: 0,
            arm7_blocks: 0,
        }
    }

    fn set_keys(&mut self, keys: Keys) {
        self.jit.set_keys(keys);
        self.interpreter.set_keys(keys);
    }

    fn frame(&mut self, frame: u32) -> Result<(), String> {
        self.jit.run_frame();
        let (ends, blocks, arm7) = self.jit.take();
        self.blocks += blocks;
        self.arm7_blocks += arm7;
        self.interpreter.expect(ends);
        self.interpreter.run_frame();
        self.interpreter
            .verdict()
            .map_err(|e| format!("frame {frame}: {e}"))?;
        if self.jit.picture() != self.interpreter.picture() {
            return Err(format!("frame {frame}: the pictures differ"));
        }
        if frame.is_multiple_of(STATE_EVERY) && self.jit.state() != self.interpreter.state() {
            return Err(format!(
                "frame {frame}: the states differ: {}",
                self.jit.differences(&self.interpreter)
            ));
        }
        Ok(())
    }

    fn load(&self, state: &[u8]) -> Self {
        Self::pair(self.jit.load(state), self.interpreter.load(state))
    }
}

fn differ(
    path: &str,
    jit: &serde_json::Value,
    interpreter: &serde_json::Value,
    found: &mut Vec<String>,
) {
    use serde_json::Value;
    if found.len() >= 12 || jit == interpreter {
        return;
    }
    match (jit, interpreter) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                differ(
                    &format!("{path}.{key}"),
                    value,
                    b.get(key).unwrap_or(&Value::Null),
                    found,
                );
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                differ(&format!("{path}[{index}]"), x, y, found);
            }
        }
        _ => {
            let short = |value: &Value| value.to_string().chars().take(160).collect::<String>();
            found.push(format!(
                "{path}: JIT {} interpreter {}",
                short(jit),
                short(interpreter)
            ));
        }
    }
}

fn press_step(press: &[Keys], frame: u32) -> Option<Keys> {
    let since = frame.checked_sub(PRESS_DELAY_FRAMES)?;
    let keys = *press.get((since / PRESS_STEP_FRAMES) as usize)?;
    match since % PRESS_STEP_FRAMES {
        0 => Some(keys),
        PRESS_HOLD_FRAMES => Some(Keys::empty()),
        _ => None,
    }
}

fn key(name: &str) -> Keys {
    Keys::from_name(&name.to_uppercase()).unwrap_or_else(|| panic!("no key {name}"))
}

struct Case {
    name: String,
    rom: Vec<u8>,
    frames: u32,
    press: Vec<Keys>,
}

fn open_built(source: &Source) -> Option<zip::ZipArchive<File>> {
    let path = cache_dir().join(format!("built-{}.zip", source.sha256));
    zip::ZipArchive::new(File::open(path).ok()?).ok()
}

fn cases<M: Machine>(suite_name: &str) -> Vec<Case> {
    let manifest: Manifest = read_manifest().expect("the ledger manifest reads");
    let suite = &manifest.suites[suite_name];
    let source = &manifest.sources[&suite.source];
    let archive = if source.built {
        open_built(&source.source)
    } else {
        Some(fetch(&suite.source, &source.source).expect("the ledger source downloads"))
    };
    let Some(mut archive) = citrine_gb_test_roms::found(
        suite_name,
        format_args!("the built {suite_name} archive, which `make ledger-nds` builds with docker"),
        archive,
    ) else {
        return Vec::new();
    };
    suite
        .roms
        .iter()
        .filter(|entry| entry.consoles.iter().any(|console| console == M::CONSOLE))
        .map(|entry| {
            let rom = read_entry(&mut archive, &format!("{}/{}", suite.root, entry.path))
                .expect("the ROM reads");
            assert_eq!(sha256_hex(&rom), entry.sha256, "{}", entry.path);
            let cycles = entry.cycles.unwrap_or(suite.cycles);
            Case {
                name: format!(
                    "{suite_name}/{}",
                    entry.name.as_deref().unwrap_or(&entry.path)
                ),
                rom,
                frames: cycles.div_ceil(M::FRAME_CYCLES),
                press: entry.press.iter().map(|name| key(name)).collect(),
            }
        })
        .collect()
}

fn run_case<M: Machine>(case: &Case) -> Result<usize, String> {
    let shelf = shelf::<M>(&case.name, &case.rom);
    let blocks = run_with::<M>(case, shelf.as_ref().map(|shelf| Arc::clone(&shelf.cache)))?;
    if let Some(shelf) = shelf {
        shelf.save();
    }
    Ok(blocks)
}

fn run_with<M: Machine>(case: &Case, cache: Option<Arc<JitCache>>) -> Result<usize, String> {
    let mut peers = Peers::<M>::boot_with(&case.rom, cache);
    for frame in 1..=case.frames {
        peers.frame(frame)?;
        if let Some(keys) = press_step(&case.press, frame) {
            peers.set_keys(keys);
        }
    }
    println!("{}: {}", case.name, peers.jit.stats());
    let mismatched = peers.jit.mismatched();
    if mismatched > 0 {
        return Err(format!(
            "{mismatched} cached blocks differ from a fresh compile"
        ));
    }
    Ok(peers.blocks)
}

fn shelf<M: Machine>(name: &str, rom: &[u8]) -> Option<Shelf> {
    if M::CACHES {
        Shelf::open(name, rom)
    } else {
        None
    }
}

struct Shelf {
    cache: Arc<JitCache>,
    path: PathBuf,
}

impl Shelf {
    fn open(name: &str, rom: &[u8]) -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("CITRINE_JIT_CACHE_DIR")?);
        let file: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let path = dir.join(format!("{file}.jit"));
        let cache = Arc::new(JitCache::for_rom(rom));
        match std::fs::read(&path) {
            Ok(bytes) => {
                let loaded = cache.merge(&bytes).expect("the cache file loads");
                println!("{name}: warm, {loaded} blocks from {}", path.display());
            }
            Err(_) => println!("{name}: cold"),
        }
        Some(Self { cache, path })
    }

    fn save(&self) {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).expect("the cache folder makes");
        }
        std::fs::write(&self.path, self.cache.encode()).expect("the cache file writes");
    }
}

fn suite(name: &str) {
    check::<Nds>(&cases::<Nds>(name));
}

fn gba_suite(name: &str) {
    check::<Gba>(&cases::<Gba>(name));
}

fn check<M: Machine>(cases: &[Case]) {
    let mut failures = Vec::new();
    let only = std::env::var("CITRINE_LOCKSTEP_ONLY").unwrap_or_default();
    for case in cases.iter().filter(|case| case.name.contains(&only)) {
        match run_case::<M>(case) {
            Ok(0) => failures.push(format!("{}: the JIT ran no block", case.name)),
            Ok(blocks) => println!("{}: {blocks} blocks in lockstep", case.name),
            Err(error) => failures.push(format!("{}: {error}", case.name)),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn armwrestler_runs_the_same_with_the_jit() {
    suite("armwrestler");
}

#[test]
fn arm7wrestler_runs_the_same_with_the_jit() {
    suite("arm7wrestler");
}

#[test]
fn rockwrestler_runs_the_same_with_the_jit() {
    suite("rockwrestler");
}

#[test]
fn blocksds_runs_the_same_with_the_jit() {
    suite("blocksds");
}

#[test]
fn nds_interp_runs_the_same_with_the_jit() {
    let cases = cases::<Nds>("nds-interp");
    check::<Nds>(&cases[..cases.len().min(1)]);
}

fn rewriting_rom(on_arm7: bool) -> Vec<u8> {
    let entry = if on_arm7 { 0x0380_0000 } else { 0x0200_0000u32 };
    let mut program = vec![0u32; 0x803];
    program[..12].copy_from_slice(&[
        0xE3A0_0000,
        0xE3A0_1000,
        0xE59F_2030,
        0xE3A0_4C01,
        0xEB00_07FA,
        0xE592_3000,
        0xE223_3003,
        0xE582_3000,
        0xE254_4001,
        0x1AFF_FFF9,
        0xEAFF_FFF4,
        0,
    ]);
    program[0x10] = entry + 0x2000;
    program[0x800..].copy_from_slice(&[0xE280_0001, 0xE081_1000, 0xE12F_FF1E]);
    let idle = [0xEAFF_FFFEu32];
    if on_arm7 {
        two_cpu_rom(&idle, &program)
    } else {
        two_cpu_rom(&program, &idle)
    }
}

fn two_cpu_rom(arm9: &[u32], arm7: &[u32]) -> Vec<u8> {
    let bytes = |words: &[u32]| -> Vec<u8> { words.iter().flat_map(|w| w.to_le_bytes()).collect() };
    let (arm9, arm7) = (bytes(arm9), bytes(arm7));
    let arm7_at = 0x200 + arm9.len();
    let mut rom = vec![0; arm7_at + arm7.len()];
    rom[..4].copy_from_slice(b"TEST");
    for (at, offset, ram, code) in [
        (0x20, 0x200, 0x0200_0000, &arm9),
        (0x30, arm7_at, 0x0380_0000, &arm7),
    ] {
        for (index, value) in [offset as u32, ram, ram, code.len() as u32]
            .into_iter()
            .enumerate()
        {
            rom[at + 4 * index..at + 4 * index + 4].copy_from_slice(&value.to_le_bytes());
        }
        rom[offset..offset + code.len()].copy_from_slice(code);
    }
    rom
}

#[test]
fn a_block_rewritten_under_a_linked_jump_runs_the_same() {
    check::<Nds>(&[Case {
        name: "rewritten".into(),
        rom: rewriting_rom(false),
        frames: 20,
        press: Vec::new(),
    }]);
}

#[test]
fn a_block_rewritten_under_a_linked_jump_on_the_arm7_runs_the_same() {
    let mut peers = Peers::<Nds>::boot(&rewriting_rom(true));
    for frame in 1..=20 {
        peers.frame(frame).unwrap_or_else(|e| panic!("{e}"));
    }
    assert!(peers.arm7_blocks > 0);
}

#[test]
fn an_arm7_ldm_glitch_before_a_linked_jump_runs_the_same() {
    let program = [
        0xE321_F0DFu32,
        0xE3A0_D0F0,
        0xE321_F0D3,
        0xE3A0_DC0F,
        0xE3A0_0638,
        0xE280_0A01,
        0xE284_4001,
        0xE8D0_0006,
        0xE083_300D,
        0xEAFF_FFFB,
    ];
    let mut peers = Peers::<Nds>::boot(&two_cpu_rom(&[0xEAFF_FFFE], &program));
    for frame in 1..=20 {
        peers.frame(frame).unwrap_or_else(|e| panic!("{e}"));
    }
    assert!(peers.arm7_blocks > 0);
}

fn warm_cache<M: Machine>(suite_name: &str) {
    let cases = cases::<M>(suite_name);
    let mut failures = Vec::new();
    for case in &cases {
        let cold = Arc::new(JitCache::for_rom(&case.rom));
        let warm = Arc::new(JitCache::for_rom(&case.rom));
        let outcome = run_with::<M>(case, Some(Arc::clone(&cold))).and_then(|_| {
            let loaded = warm.merge(&cold.encode()).map_err(|e| e.to_string())?;
            println!("{}: {loaded} blocks saved", case.name);
            if loaded == 0 {
                return Err("no blocks saved".to_string());
            }
            run_with::<M>(case, Some(Arc::clone(&warm)))
        });
        if let Err(error) = outcome {
            failures.push(format!("{} warm: {error}", case.name));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_warm_cache_runs_the_same_with_the_jit() {
    warm_cache::<Nds>("armwrestler");
}

#[test]
fn a_warm_gba_cache_runs_the_same_with_the_jit() {
    warm_cache::<Gba>("jsmolka");
}

#[test]
fn jsmolka_runs_the_same_with_the_jit() {
    gba_suite("jsmolka");
}

#[test]
fn alyosha_tas_runs_the_same_with_the_jit() {
    gba_suite("alyosha-tas");
}

#[test]
fn nba_emu_runs_the_same_with_the_jit() {
    gba_suite("nba-emu");
}

#[test]
fn destoer_runs_the_same_with_the_jit() {
    gba_suite("destoer");
}

fn older_state<M: Machine>(case: Case) {
    let mut peers = Peers::<M>::boot(&case.rom);
    let mut early = None;
    for frame in 1..=case.frames {
        peers.frame(frame).expect("the first run stays in lockstep");
        if let Some(keys) = press_step(&case.press, frame) {
            peers.set_keys(keys);
        }
        if frame == PRESS_DELAY_FRAMES + 3 {
            early = Some((frame, peers.jit.state()));
        }
    }
    let (from, state) = early.expect("the run passes the save frame");
    let mut loaded = peers.load(&state);
    for frame in from + 1..=case.frames {
        loaded
            .frame(frame)
            .unwrap_or_else(|e| panic!("after the load: {e}"));
        if let Some(keys) = press_step(&case.press, frame) {
            loaded.set_keys(keys);
        }
    }
    assert!(loaded.blocks > 0, "the JIT ran no block after the load");
}

#[test]
fn an_older_state_loaded_over_newer_code_runs_the_same() {
    if let Some(case) = cases::<Nds>("armwrestler").into_iter().nth(4) {
        older_state::<Nds>(case);
    }
}

#[test]
fn an_older_gba_state_loaded_over_newer_code_runs_the_same() {
    if let Some(case) = cases::<Gba>("jsmolka").into_iter().next() {
        older_state::<Gba>(case);
    }
}

#[cfg(feature = "persistence")]
fn script_keys(name: &str) -> Keys {
    match name {
        "0" => Keys::empty(),
        _ => name
            .split('+')
            .map(key)
            .fold(Keys::empty(), |all, k| all | k),
    }
}

#[cfg(feature = "persistence")]
#[test]
#[ignore = "needs a game in CITRINE_LOCKSTEP_GAME, run through heavy"]
fn a_game_runs_the_same_with_the_jit() {
    let path = std::env::var("CITRINE_LOCKSTEP_GAME").expect("a game path");
    let rom = std::fs::read(&path).expect("the game reads");
    if path.to_lowercase().ends_with(".gba") {
        let peers = play_game::<Gba>(&rom);
        assert!(peers.blocks > 0);
    } else {
        let peers = play_game::<Nds>(&rom);
        assert!(peers.blocks > 0);
        assert!(peers.arm7_blocks > 0);
    }
}

#[cfg(feature = "persistence")]
fn play_game<M: Machine + citrine_gb::console::Console>(rom: &[u8]) -> Peers<M> {
    let script =
        std::fs::read_to_string(std::env::var("CITRINE_LOCKSTEP_SCRIPT").expect("a script path"))
            .expect("the script reads");
    let shelf = shelf::<M>("game", rom);
    let mut peers =
        Peers::<M>::boot_with(rom, shelf.as_ref().map(|shelf| Arc::clone(&shelf.cache)));
    if let Ok(path) = std::env::var("CITRINE_LOCKSTEP_SAVE")
        && !path.is_empty()
    {
        let data = std::fs::read(path).expect("the save reads");
        for machine in [&mut peers.jit, &mut peers.interpreter] {
            let save = machine.battery_from_file(&data);
            machine.put_battery(save, 0);
        }
    }
    let mut frame = 0;
    let mut run = |peers: &mut Peers<M>, keys: Keys, count: u32| {
        peers.set_keys(keys);
        for _ in 0..count {
            frame += 1;
            peers.frame(frame).unwrap_or_else(|e| panic!("{e}"));
        }
    };
    for line in script.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let number = |at: usize, default: u32| {
            words
                .get(at)
                .map_or(default, |w| w.parse().expect("a number"))
        };
        match words.as_slice() {
            ["wait", ..] => run(&mut peers, Keys::empty(), number(1, 1)),
            ["hold", keys, ..] => run(&mut peers, script_keys(keys), number(2, 1)),
            ["tap", keys, ..] => {
                for _ in 0..number(2, 1) {
                    run(&mut peers, script_keys(keys), 6);
                    run(&mut peers, Keys::empty(), number(3, 24));
                }
            }
            ["time", keys, count, ..] => run(
                &mut peers,
                script_keys(keys),
                count.parse().expect("a count"),
            ),
            _ => {}
        }
    }
    println!(
        "{frame} frames, {} blocks in lockstep, {} on the ARM7",
        peers.blocks, peers.arm7_blocks
    );
    println!("{}", peers.jit.stats());
    assert_eq!(
        peers.jit.mismatched(),
        0,
        "cached blocks differ from a fresh compile"
    );
    if let Some(shelf) = shelf {
        shelf.save();
    }
    peers
}
