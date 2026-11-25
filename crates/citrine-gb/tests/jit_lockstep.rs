mod archives;

use archives::{Source, cache_dir, fetch, read_entry, read_manifest, sha256_hex};
use citrine_gb::console::{Keys, Rollback};
use citrine_gb::gba::machine::Gba;
use citrine_gb::jit_cache::KEY_BYTES;
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
        let cache = Arc::new(JitCache::for_rom(rom, [0; KEY_BYTES]));
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

fn warm_case<M: Machine>(case: &Case) -> Result<usize, String> {
    let cold = Arc::new(JitCache::for_rom(&case.rom, [0; KEY_BYTES]));
    let warm = Arc::new(JitCache::for_rom(&case.rom, [0; KEY_BYTES]));
    run_with::<M>(case, Some(Arc::clone(&cold))).and_then(|_| {
        let loaded = warm.merge(&cold.encode()).map_err(|e| e.to_string())?;
        println!("{}: {loaded} blocks saved", case.name);
        if loaded == 0 {
            return Err("no blocks saved".to_string());
        }
        run_with::<M>(case, Some(Arc::clone(&warm)))
    })
}

fn warm_cache<M: Machine>(suite_name: &str) {
    let cases = cases::<M>(suite_name);
    let mut failures = Vec::new();
    for case in &cases {
        if let Err(error) = warm_case::<M>(case) {
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

const GBA_RAM_PROGRAM: [u32; 48] = [
    0xE3A0_0301,
    0xE280_0C02,
    0xE59F_10A0,
    0xE1C0_10B4,
    0xE3A0_2402,
    0xE3A0_3403,
    0xE283_3B02,
    0xE3A0_C403,
    0xE28C_CC01,
    0xE59F_1088,
    0xE58C_1000,
    0xE59F_1084,
    0xE58C_1004,
    0xE3A0_4000,
    0xE284_4001,
    0xE582_4000,
    0xE583_4000,
    0xE1C2_40B6,
    0xE1C3_40B7,
    0xE5C2_4009,
    0xE5C3_400B,
    0xE592_5000,
    0xE593_6001,
    0xE1D2_70B6,
    0xE1D3_80F6,
    0xE1D3_90D9,
    0xE5D2_A00B,
    0xE1D3_B0F7,
    0xE085_5006,
    0xE085_5007,
    0xE085_5008,
    0xE085_5009,
    0xE085_500A,
    0xE085_500B,
    0xE582_5010,
    0xE1D2_50D9,
    0xE583_5010,
    0xE204_10FF,
    0xE59F_601C,
    0xE181_1006,
    0xE58C_1000,
    0xE1A0_E00F,
    0xE12F_FF1C,
    0xEAFF_FFE1,
    0x0000_4317,
    0xE280_0001,
    0xE12F_FF1E,
    0xE280_0000,
];

#[test]
fn gba_ram_accesses_run_the_same_with_the_jit() {
    let case = Case {
        name: "gba-ram".into(),
        rom: GBA_RAM_PROGRAM
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect(),
        frames: 2 * STATE_EVERY + 1,
        press: Vec::new(),
    };
    check::<Gba>(std::slice::from_ref(&case));
    older_state::<Gba>(case);
}

const GBA_ROM: u32 = 0x0800_0000;
const BRANCH_ROM_SIZE: usize = 0x2_0100;
const BRANCH_SETUP: u32 = 0x0800_0100;
const BRANCH_IRQ: u32 = 0x0800_0300;
const BRANCH_IWRAM_SOURCE: u32 = 0x0800_0340;
const BRANCH_MAIN: u32 = 0x0800_0400;
const BRANCH_WAITS: u32 = 0x0800_0480;
const BRANCH_NEAR: u32 = 0x0800_0600;
const BRANCH_TO_IWRAM: u32 = BRANCH_NEAR + 0xC;
const BRANCH_TO_WS1: u32 = BRANCH_NEAR + 0xE;
const BRANCH_TO_WS2: u32 = BRANCH_NEAR + 0x10;
const BRANCH_PAGE_END: u32 = 0x0801_FFFC;
const BRANCH_FAR: u32 = 0x0802_0000;
const BRANCH_LAST: u32 = 0x0802_00FE;
const BRANCH_IWRAM: u32 = 0x0300_0100;
const BRANCH_WAITCNT: [u16; 4] = [0x4317, 0x40AC, 0x0317, 0x4014];

fn arm_b(cond: u32, from: u32, to: u32) -> u32 {
    cond << 28 | 0x0A00_0000 | to.wrapping_sub(from + 8) >> 2 & 0x00FF_FFFF
}

fn thumb_b(from: u32, to: u32) -> u16 {
    0xE000 | (to.wrapping_sub(from + 4) >> 1 & 0x7FF) as u16
}

fn thumb_bne(from: u32, to: u32) -> u16 {
    0xD100 | (to.wrapping_sub(from + 4) >> 1 & 0xFF) as u16
}

fn thumb_bl(from: u32, to: u32) -> [u16; 2] {
    let offset = to.wrapping_sub(from + 4);
    [
        0xF000 | (offset >> 12 & 0x7FF) as u16,
        0xF800 | (offset >> 1 & 0x7FF) as u16,
    ]
}

fn thumb_adr(from: u32, rd: u16, to: u32) -> u16 {
    let base = (from + 4) & !3;
    assert!(to.is_multiple_of(4) && to >= base && to - base < 0x400);
    0xA000 | rd << 8 | ((to - base) / 4) as u16
}

fn thumb_ldr_pc(from: u32, rd: u16, to: u32) -> u16 {
    let base = (from + 4) & !3;
    assert!(to.is_multiple_of(4) && to >= base && to - base < 0x400);
    0x4800 | rd << 8 | ((to - base) / 4) as u16
}

struct Code {
    at: u32,
    halves: Vec<u16>,
}

impl Code {
    fn new(at: u32) -> Self {
        Self {
            at,
            halves: Vec::new(),
        }
    }

    fn here(&self) -> u32 {
        self.at + 2 * self.halves.len() as u32
    }

    fn thumb(&mut self, halves: &[u16]) {
        self.halves.extend_from_slice(halves);
    }

    fn arm(&mut self, words: &[u32]) {
        for word in words {
            self.halves.extend([*word as u16, (word >> 16) as u16]);
        }
    }

    fn arm_b(&mut self, cond: u32, to: u32) {
        let word = arm_b(cond, self.here(), to);
        self.arm(&[word]);
    }

    fn b(&mut self, to: u32) {
        let half = thumb_b(self.here(), to);
        self.thumb(&[half]);
    }

    fn bne(&mut self, to: u32) {
        let half = thumb_bne(self.here(), to);
        self.thumb(&[half]);
    }

    fn bl(&mut self, to: u32) {
        let halves = thumb_bl(self.here(), to);
        self.thumb(&halves);
    }

    fn adr(&mut self, rd: u16, to: u32) {
        let half = thumb_adr(self.here(), rd, to);
        self.thumb(&[half]);
    }

    fn ldr_pc(&mut self, rd: u16, to: u32) {
        let half = thumb_ldr_pc(self.here(), rd, to);
        self.thumb(&[half]);
    }

    fn place(&self, rom: &mut [u8], at: u32) {
        let start = (at - GBA_ROM) as usize;
        for (index, half) in self.halves.iter().enumerate() {
            rom[start + 2 * index..start + 2 * index + 2].copy_from_slice(&half.to_le_bytes());
        }
    }
}

fn gba_branch_rom() -> Vec<u8> {
    let mut rom = vec![0; BRANCH_ROM_SIZE];
    let mut entry = Code::new(GBA_ROM);
    entry.arm_b(0xE, BRANCH_SETUP);
    entry.place(&mut rom, GBA_ROM);

    let mut setup = Code::new(BRANCH_SETUP);
    setup.arm(&[
        0xE3A0_0301,
        0xE3A0_1018,
        0xE1C0_10B4,
        0xE3A0_1402,
        0xE580_10B0,
        0xE3A0_1403,
        0xE281_1901,
        0xE580_10B4,
        0xE3A0_1004,
        0xE1C0_1BB8,
        0xE3A0_1CA7,
        0xE281_1060,
        0xE1C0_1BBA,
        0xE3A0_1403,
        0xE281_1C7F,
        0xE281_10FC,
        0xE3A0_2302,
        0xE282_2C03,
        0xE581_2000,
        0xE3A0_B301,
        0xE28B_BC02,
        0xE3A0_1C43,
        0xE281_1017,
        0xE1CB_10B4,
        0xE3A0_8403,
        0xE288_8C01,
        0xE3A0_2302,
        0xE282_2FD0,
        0xE1A0_3008,
        0xE3A0_100E,
    ]);
    let copy = setup.here();
    setup.arm(&[0xE492_C004, 0xE483_C004, 0xE251_1001]);
    setup.arm_b(0x1, copy);
    setup.arm(&[
        0xE3A0_4402,
        0xE584_4000,
        0xE584_8004,
        0xE584_B008,
        0xE3A0_940A,
        0xE289_9C06,
        0xE289_9001,
        0xE3A0_A40C,
        0xE28A_AC06,
        0xE28A_A001,
        0xE3A0_5000,
        0xE3A0_6000,
        0xE3A0_7000,
        0xE3A0_1003,
        0xE1CB_10B0,
        0xE3A0_1001,
        0xE1CB_10B8,
        0xE3A0_C302,
        0xE28C_CB01,
        0xE28C_C001,
        0xE12F_FF1C,
    ]);
    assert!(setup.here() <= BRANCH_IRQ);
    setup.place(&mut rom, BRANCH_SETUP);

    let mut irq = Code::new(BRANCH_IRQ);
    irq.arm(&[
        0xE3A0_3301,
        0xE283_3C02,
        0xE593_2000,
        0xE002_1822,
        0xE1C3_10B2,
        0xE12F_FF1E,
    ]);
    assert!(irq.here() <= BRANCH_IWRAM_SOURCE);
    irq.place(&mut rom, BRANCH_IRQ);

    let mut iwram = Code::new(BRANCH_IWRAM);
    iwram.arm(&[0xE3A0_5006]);
    let multiply = iwram.here();
    iwram.arm(&[0xE594_1000, 0xE002_0195, 0xE086_6002, 0xE255_5001]);
    iwram.arm_b(0x1, multiply);
    iwram.arm(&[0xE321_F09F, 0xE3A0_5003]);
    let masked = iwram.here();
    iwram.arm(&[0xE255_5001]);
    iwram.arm_b(0x1, masked);
    iwram.arm(&[0xE321_F01F]);
    let skip = iwram.here() + 8;
    iwram.arm_b(0xE, skip);
    iwram.arm(&[0xE286_6001]);
    assert_eq!(iwram.here(), skip);
    iwram.arm(&[0xE12F_FF1E]);
    assert_eq!(iwram.halves.len(), 2 * 14);
    iwram.place(&mut rom, BRANCH_IWRAM_SOURCE);

    let mut main = Code::new(BRANCH_MAIN);
    main.thumb(&[0x2003, 0x4038, 0x0040]);
    main.adr(1, BRANCH_WAITS);
    main.thumb(&[
        0x5A09, 0x4658, 0x8081, 0x8901, 0x2201, 0x4051, 0x8101, 0x3701, 0x250C,
    ]);
    let filled = main.here();
    main.thumb(&[0x6821, 0x4369, 0x1876, 0x3D01]);
    main.bne(filled);
    main.thumb(&[0x2505]);
    let loaded = main.here();
    main.thumb(&[0x1C20, 0xC80E, 0x18B6, 0x3D01]);
    main.bne(loaded);
    let next = main.here() + 2;
    main.b(next);
    let ahead = main.here() + 6;
    main.b(ahead);
    main.thumb(&[0x3601, 0x3602]);
    assert_eq!(main.here(), ahead);
    let skipped = main.here() + 4;
    main.b(skipped);
    main.thumb(&[0x3603]);
    assert_eq!(main.here(), skipped);
    for call in [
        BRANCH_NEAR,
        BRANCH_FAR,
        BRANCH_TO_IWRAM,
        BRANCH_TO_WS1,
        BRANCH_TO_WS2,
        BRANCH_PAGE_END,
        BRANCH_PAGE_END + 2,
        BRANCH_LAST,
    ] {
        main.bl(call);
    }
    main.b(BRANCH_MAIN);
    assert!(main.here() <= BRANCH_WAITS);
    main.place(&mut rom, BRANCH_MAIN);

    let mut waits = Code::new(BRANCH_WAITS);
    waits.thumb(&BRANCH_WAITCNT);
    waits.place(&mut rom, BRANCH_WAITS);

    let mut near = Code::new(BRANCH_NEAR);
    near.thumb(&[0x2304]);
    let counted = near.here();
    near.thumb(&[0x18F6, 0x3B01]);
    near.bne(counted);
    near.thumb(&[0x4770, 0x46C0]);
    assert_eq!(near.here(), BRANCH_TO_IWRAM);
    near.thumb(&[0x4740, 0x4748, 0x4750]);
    near.place(&mut rom, BRANCH_NEAR);

    let mut page_end = Code::new(BRANCH_PAGE_END);
    page_end.thumb(&[0x46C0, 0x4770]);
    page_end.place(&mut rom, BRANCH_PAGE_END);

    let mut far = Code::new(BRANCH_FAR);
    far.thumb(&[0xB500, 0x2303]);
    let summed = far.here();
    far.thumb(&[0x6861, 0x1876, 0x3B01]);
    far.bne(summed);
    far.bl(BRANCH_NEAR);
    far.thumb(&[0xBD00]);
    assert!(far.here() <= BRANCH_LAST);
    far.place(&mut rom, BRANCH_FAR);

    let mut last = Code::new(BRANCH_LAST);
    last.thumb(&[0x4770]);
    last.place(&mut rom, BRANCH_LAST);
    assert_eq!(BRANCH_LAST + 2 - GBA_ROM, BRANCH_ROM_SIZE as u32);
    rom
}

#[test]
fn gba_branch_encodings_match_hand_encoded_words() {
    assert_eq!(arm_b(0xE, GBA_ROM, BRANCH_SETUP), 0xEA00_003E);
    assert_eq!(arm_b(0x1, 0x0300_0114, 0x0300_0104), 0x1AFF_FFFA);
    assert_eq!(arm_b(0xE, 0x0300_012C, 0x0300_0134), 0xEA00_0000);
    assert_eq!(thumb_b(0x0800_0430, 0x0800_0432), 0xE7FF);
    assert_eq!(thumb_b(0x0800_0432, 0x0800_0438), 0xE001);
    assert_eq!(thumb_b(0x0800_0438, 0x0800_043C), 0xE000);
    assert_eq!(thumb_bne(0x0800_0422, 0x0800_041A), 0xD1FA);
    assert_eq!(thumb_bl(0x0800_0440, BRANCH_FAR), [0xF01F, 0xFDDE]);
    assert_eq!(thumb_bl(0x0802_000C, BRANCH_NEAR), [0xF7E0, 0xFAF8]);
    assert_eq!(thumb_adr(0x0800_0406, 1, BRANCH_WAITS), 0xA11E);
}

#[test]
fn gba_branches_run_the_same_with_the_jit() {
    let case = Case {
        name: "gba-branches".into(),
        rom: gba_branch_rom(),
        frames: 2 * STATE_EVERY + 1,
        press: Vec::new(),
    };
    check::<Gba>(std::slice::from_ref(&case));
    older_state::<Gba>(case);
}

const FETCH_ROM_SIZE: usize = 0x2_0100;
const FETCH_SETUP: u32 = 0x0800_0100;
const FETCH_IRQ: u32 = 0x0800_0300;
const FETCH_MAIN: u32 = 0x0800_0400;
const FETCH_POOL: u32 = 0x0800_0500;
const FETCH_WAITS: u32 = 0x0800_0540;
const FETCH_TABLE: u32 = 0x0800_0560;
const FETCH_PAGE: u32 = 0x0801_FFE0;
const FETCH_PAGE_END: u32 = 0x0802_0000;
const FETCH_EWRAM_COPY: u32 = 0x0200_0100;
const FETCH_MIRRORS: [u32; 2] = [0x0A01_FFE1, 0x0C01_FFE1];
const FETCH_WAITCNT: [u16; 8] = [
    0x4317, 0x4014, 0x4000, 0x0317, 0x40AC, 0x4317, 0x4004, 0x45B4,
];

fn gba_fetch_setup(rom: &mut [u8]) {
    let mut entry = Code::new(GBA_ROM);
    entry.arm_b(0xE, FETCH_SETUP);
    entry.place(rom, GBA_ROM);

    let mut setup = Code::new(FETCH_SETUP);
    setup.arm(&[
        0xE3A0_0301,
        0xE3A0_1018,
        0xE1C0_10B4,
        0xE3A0_1402,
        0xE580_10B0,
        0xE3A0_1403,
        0xE281_1901,
        0xE580_10B4,
        0xE3A0_1004,
        0xE1C0_1BB8,
        0xE3A0_1CA7,
        0xE281_1060,
        0xE1C0_1BBA,
        0xE3A0_1403,
        0xE281_1C7F,
        0xE281_10FC,
        0xE3A0_2302,
        0xE282_2C03,
        0xE581_2000,
        0xE3A0_B301,
        0xE28B_BC02,
        0xE3A0_1C43,
        0xE281_1017,
        0xE1CB_10B4,
        0xE3A0_9402,
        0xE589_9000,
        0xE589_B004,
        0xE3A0_7000,
        0xE3A0_1003,
        0xE1CB_10B0,
        0xE3A0_1001,
        0xE1CB_10B8,
        0xE3A0_C302,
        0xE28C_CB01,
        0xE28C_C001,
        0xE12F_FF1C,
    ]);
    assert!(setup.here() <= FETCH_IRQ);
    setup.place(rom, FETCH_SETUP);

    let mut irq = Code::new(FETCH_IRQ);
    irq.arm(&[
        0xE3A0_3301,
        0xE283_3C02,
        0xE593_2000,
        0xE002_1822,
        0xE1C3_10B2,
        0xE3A0_0403,
        0xE280_0C7F,
        0xE1D0_2FB8,
        0xE182_2001,
        0xE1C0_2FB8,
        0xE12F_FF1E,
    ]);
    assert!(irq.here() <= FETCH_MAIN);
    irq.place(rom, FETCH_IRQ);
}

fn gba_fetch_rom() -> Vec<u8> {
    let mut rom = vec![0; FETCH_ROM_SIZE];
    gba_fetch_setup(&mut rom);

    let pool = [
        FETCH_WAITS,
        FETCH_TABLE,
        FETCH_EWRAM_COPY,
        FETCH_MIRRORS[0],
        FETCH_MIRRORS[1],
    ];
    let literal = |value: u32| {
        let index = pool.iter().position(|&word| word == value).expect("pooled");
        FETCH_POOL + 4 * index as u32
    };
    let mut main = Code::new(FETCH_MAIN);
    let top = main.here();
    main.thumb(&[0x2007, 0x4038, 0x0040]);
    main.ldr_pc(1, literal(FETCH_WAITS));
    main.thumb(&[0x5A09, 0x4658, 0x8081, 0x3701]);
    main.thumb(&[
        0x464C, 0x6821, 0x6121, 0x88A2, 0x1889, 0xB406, 0x4351, 0xBC06, 0x6862, 0x1889, 0x6121,
    ]);
    main.thumb(&[0x1C20, 0xC87E]);
    main.thumb(&[
        0x3101, 0x3201, 0x3301, 0x3401, 0x3501, 0x3601, 0x3101, 0x3201, 0x3301, 0x3401, 0x3501,
        0x3601,
    ]);
    main.ldr_pc(1, literal(FETCH_TABLE));
    main.ldr_pc(2, literal(FETCH_TABLE));
    main.thumb(&[0x780B, 0x784B]);
    let skipped = main.here() + 4;
    main.b(skipped);
    main.thumb(&[0x3301]);
    assert_eq!(main.here(), skipped);
    main.bl(FETCH_PAGE);
    let mut veneer_calls = Vec::new();
    for mirror in FETCH_MIRRORS {
        main.ldr_pc(3, literal(mirror));
        veneer_calls.push(main.halves.len());
        main.thumb(&[0, 0]);
    }
    main.thumb(&[0x1C38, 0x3064, 0x2107, 0xDF06]);
    main.ldr_pc(0, literal(FETCH_TABLE));
    main.ldr_pc(1, literal(FETCH_EWRAM_COPY));
    main.thumb(&[0x2208, 0xDF0B, 0x20FF, 0x4038]);
    let waited = main.here() + 4;
    main.bne(waited);
    main.thumb(&[0xDF05]);
    assert_eq!(main.here(), waited);
    main.b(top);
    let veneer = main.here();
    main.thumb(&[0x4718]);
    for call in veneer_calls {
        let from = FETCH_MAIN + 2 * call as u32;
        main.halves[call..call + 2].copy_from_slice(&thumb_bl(from, veneer));
    }
    assert!(main.here() <= FETCH_POOL);
    main.place(&mut rom, FETCH_MAIN);

    let mut data = Code::new(FETCH_POOL);
    data.arm(&pool);
    assert!(data.here() <= FETCH_WAITS);
    data.place(&mut rom, FETCH_POOL);
    let mut waits = Code::new(FETCH_WAITS);
    waits.thumb(&FETCH_WAITCNT);
    waits.place(&mut rom, FETCH_WAITS);
    let mut table = Code::new(FETCH_TABLE);
    table.arm(&[0x0403_0201, 0x0807_0605, 0x8070_6050, 0xF0E0_D0C0]);
    table.place(&mut rom, FETCH_TABLE);

    let mut page = Code::new(FETCH_PAGE);
    page.thumb(&[
        0xB500, 0x4648, 0x6801, 0x6842, 0x1889, 0x6801, 0x3101, 0x6842, 0x3201, 0x6801, 0x6842,
        0x3101,
    ]);
    page.ldr_pc(3, FETCH_PAGE_END);
    page.ldr_pc(3, FETCH_PAGE_END);
    assert_eq!(page.here(), FETCH_PAGE_END - 4);
    page.thumb(&[0x6801, 0x6842, 0x3101, 0x6801, 0x6842, 0x1889, 0xBD00]);
    assert!(page.here() <= GBA_ROM + FETCH_ROM_SIZE as u32);
    page.place(&mut rom, FETCH_PAGE);
    rom
}

#[test]
fn gba_rom_fetch_encodings_match_hand_encoded_words() {
    assert_eq!(thumb_ldr_pc(0x0801_FFF8, 3, FETCH_PAGE_END), 0x4B01);
    assert_eq!(thumb_ldr_pc(0x0801_FFFA, 3, FETCH_PAGE_END), 0x4B01);
    assert_eq!(thumb_ldr_pc(0x0800_0406, 1, FETCH_POOL), 0x493E);
    assert_eq!(thumb_ldr_pc(0x0800_0400, 0, 0x0800_0404), 0x4800);
}

#[test]
fn gba_rom_fetches_run_the_same_with_the_jit() {
    let case = Case {
        name: "gba-rom-fetches".into(),
        rom: gba_fetch_rom(),
        frames: 2 * STATE_EVERY + 1,
        press: Vec::new(),
    };
    check::<Gba>(std::slice::from_ref(&case));
    let mut gba = <Gba as Machine>::boot(&case.rom, false);
    for _ in 0..case.frames {
        Machine::run_frame(&mut gba);
    }
    let passes = gba.cpu().reg(7);
    assert!(
        passes > 8 * 256,
        "the fetch loop ran {passes} passes, its vblank waits stall"
    );
    older_state::<Gba>(case);
}

const BIOS_ROM_SIZE: usize = 0x600;
const BIOS_MAIN: u32 = FETCH_MAIN;
const BIOS_POOL: u32 = 0x0800_0500;
const BIOS_WAITS: u32 = 0x0800_0540;
const BIOS_TABLE: u32 = 0x0800_0560;
const BIOS_EWRAM_COPY: u32 = 0x0200_0100;
const BIOS_IWRAM_FILL: u32 = 0x0300_0200;
const BIOS_FILL_WORDS: u32 = 0x0500_0010;
const BIOS_IWRAM_FAST: u32 = 0x0300_0300;

fn thumb_swi(n: u16) -> u16 {
    0xDF00 | n
}

fn thumb_add_reg(rd: u16, rn: u16, rm: u16) -> u16 {
    0x1800 | rm << 6 | rn << 3 | rd
}

fn gba_bios_rom() -> Vec<u8> {
    let mut rom = vec![0; BIOS_ROM_SIZE];
    gba_fetch_setup(&mut rom);

    let pool = [
        BIOS_WAITS,
        BIOS_TABLE,
        BIOS_EWRAM_COPY,
        BIOS_IWRAM_FILL,
        BIOS_FILL_WORDS,
        BIOS_IWRAM_FAST,
    ];
    let literal = |value: u32| {
        let index = pool.iter().position(|&word| word == value).expect("pooled");
        BIOS_POOL + 4 * index as u32
    };
    let mut main = Code::new(BIOS_MAIN);
    let top = main.here();
    main.thumb(&[0x2007, 0x4038, 0x0040]);
    main.ldr_pc(1, literal(BIOS_WAITS));
    main.thumb(&[0x5A09, 0x4658, 0x8081]);
    main.thumb(&[
        0x1C38,
        0x3064,
        0x2107,
        thumb_swi(6),
        thumb_add_reg(6, 6, 0),
        thumb_add_reg(6, 6, 1),
    ]);
    main.thumb(&[0x1C38, 0x0200, thumb_swi(8), thumb_add_reg(6, 6, 0)]);
    main.ldr_pc(0, literal(BIOS_TABLE));
    main.ldr_pc(1, literal(BIOS_EWRAM_COPY));
    main.thumb(&[0x2208, thumb_swi(0x0B)]);
    main.ldr_pc(0, literal(BIOS_TABLE));
    main.ldr_pc(1, literal(BIOS_IWRAM_FILL));
    main.ldr_pc(2, literal(BIOS_FILL_WORDS));
    main.thumb(&[thumb_swi(0x0B)]);
    main.ldr_pc(0, literal(BIOS_TABLE));
    main.ldr_pc(1, literal(BIOS_IWRAM_FAST));
    main.thumb(&[0x2210, thumb_swi(0x0C)]);
    main.ldr_pc(3, literal(BIOS_IWRAM_FAST));
    main.thumb(&[0x685B, thumb_add_reg(6, 6, 3), 0x203F, 0x4038]);
    let skipped = main.here() + 4;
    main.bne(skipped);
    main.thumb(&[thumb_swi(5)]);
    assert_eq!(main.here(), skipped);
    main.thumb(&[0x3701]);
    main.b(top);
    assert!(main.here() <= BIOS_POOL);
    main.place(&mut rom, BIOS_MAIN);

    let mut data = Code::new(BIOS_POOL);
    data.arm(&pool);
    assert!(data.here() <= BIOS_WAITS);
    data.place(&mut rom, BIOS_POOL);
    let mut waits = Code::new(BIOS_WAITS);
    waits.thumb(&FETCH_WAITCNT);
    assert!(waits.here() <= BIOS_TABLE);
    waits.place(&mut rom, BIOS_WAITS);
    let words: Vec<u32> = (1..=16u32)
        .map(|index| (index * 0x0101_0101) ^ 0x8040_2010)
        .collect();
    let mut table = Code::new(BIOS_TABLE);
    table.arm(&words);
    assert!(table.here() <= GBA_ROM + BIOS_ROM_SIZE as u32);
    table.place(&mut rom, BIOS_TABLE);
    rom
}

#[test]
fn gba_bios_branch_encodings_match_hand_encoded_words() {
    assert_eq!(thumb_swi(0x0B), 0xDF0B);
    assert_eq!(thumb_swi(0x0C), 0xDF0C);
    assert_eq!(thumb_swi(8), 0xDF08);
    assert_eq!(thumb_add_reg(6, 6, 0), 0x1836);
    assert_eq!(thumb_add_reg(6, 6, 1), 0x1876);
    assert_eq!(thumb_add_reg(6, 6, 3), 0x18F6);
    assert_eq!(thumb_bne(0x0800_0444, 0x0800_0448), 0xD100);
    assert_eq!(thumb_b(0x0800_044A, BIOS_MAIN), 0xE7D9);
    let rom = gba_bios_rom();
    let half = |at: u32| {
        let at = (at - GBA_ROM) as usize;
        u16::from_le_bytes([rom[at], rom[at + 1]])
    };
    assert_eq!(half(0x0800_043C), 0x685B);
    assert_eq!(half(0x0800_041C), 0x0200);
    assert_eq!(half(0x0800_0436), 0x2210);
    assert_eq!(half(0x0800_0444), 0xD100);
    assert_eq!(half(0x0800_044A), 0xE7D9);
    for (from, rd, to, word) in [
        (0x0800_0406, 1, BIOS_POOL, 0x493E),
        (0x0800_0422, 0, BIOS_POOL + 4, 0x4838),
        (0x0800_0424, 1, BIOS_POOL + 8, 0x4938),
        (0x0800_042A, 0, BIOS_POOL + 4, 0x4836),
        (0x0800_042C, 1, BIOS_POOL + 0xC, 0x4937),
        (0x0800_042E, 2, BIOS_POOL + 0x10, 0x4A38),
        (0x0800_0432, 0, BIOS_POOL + 4, 0x4834),
        (0x0800_0434, 1, BIOS_POOL + 0x14, 0x4937),
        (0x0800_043A, 3, BIOS_POOL + 0x14, 0x4B36),
    ] {
        assert_eq!(thumb_ldr_pc(from, rd, to), word, "{from:#010x}");
        assert_eq!(half(from), word, "{from:#010x}");
    }
}

#[test]
fn gba_bios_branches_run_the_same_with_the_jit() {
    let case = Case {
        name: "gba-bios-branches".into(),
        rom: gba_bios_rom(),
        frames: 2 * STATE_EVERY + 1,
        press: Vec::new(),
    };
    check::<Gba>(std::slice::from_ref(&case));
    let mut gba = <Gba as Machine>::boot(&case.rom, false);
    for _ in 0..case.frames {
        Machine::run_frame(&mut gba);
    }
    let passes = gba.cpu().reg(7);
    assert!(
        passes > 8 * 64,
        "the bios loop ran {passes} passes, its vblank waits stall"
    );
    older_state::<Gba>(case);
}

const LOAD_ROM_SIZE: usize = 0x2_0102;
const LOAD_SETUP: u32 = 0x0800_0100;
const LOAD_IRQ: u32 = 0x0800_0300;
const LOAD_MIXER_SOURCE: u32 = 0x0800_0340;
const LOAD_MAIN: u32 = 0x0800_0400;
const LOAD_POOL: u32 = 0x0800_0600;
const LOAD_WAITS: u32 = 0x0800_0640;
const LOAD_TABLE: u32 = 0x0800_0660;
const LOAD_PAGE: u32 = 0x0801_FFE0;
const LOAD_PAGE_END: u32 = 0x0802_0000;
const LOAD_TAIL: u32 = 0x0802_0100;
const LOAD_MIXER: u32 = 0x0300_0100;
const LOAD_EWRAM_COPY: u32 = 0x0200_0100;
const LOAD_WORDS: u32 = 0x0400_0004;
const LOAD_DATA: u32 = 0x80FF_7F81;

fn thumb_ldrb_imm(rd: u16, rn: u16, offset: u16) -> u16 {
    assert!(offset < 32);
    0x7800 | offset << 6 | rn << 3 | rd
}

fn thumb_ldrh_imm(rd: u16, rn: u16, offset: u16) -> u16 {
    assert!(offset.is_multiple_of(2) && offset < 64);
    0x8800 | (offset / 2) << 6 | rn << 3 | rd
}

fn gba_load_rom() -> Vec<u8> {
    let mut rom = vec![0; LOAD_ROM_SIZE];
    let mut entry = Code::new(GBA_ROM);
    entry.arm_b(0xE, LOAD_SETUP);
    entry.place(&mut rom, GBA_ROM);

    let mut mixer = Code::new(LOAD_MIXER);
    mixer.arm(&[0xE3A0_100A]);
    let delay = mixer.here();
    mixer.arm(&[0xE251_1001]);
    mixer.arm_b(0x1, delay);
    mixer.arm(&[0xE3A0_C006, 0xE3A0_A000]);
    let mixed = mixer.here();
    mixer.arm(&[
        0xE0D0_10D1,
        0xE1D0_20D1,
        0xE08A_A001,
        0xE08A_A002,
        0xE5D0_1002,
        0xE002_0C91,
        0xE08A_A002,
        0xE1D0_10B0,
        0xE1D0_20F2,
        0xE08A_A001,
        0xE08A_A002,
        0xE590_1000,
        0xE02A_A001,
        0xE25C_C001,
    ]);
    mixer.arm_b(0x1, mixed);
    mixer.arm(&[
        0xE3A0_1302,
        0xE591_20C0,
        0xE08A_A002,
        0xE281_1802,
        0xE281_1C01,
        0xE1D1_20B0,
        0xE08A_A002,
        0xE589_A020,
        0xE12F_FF1E,
    ]);
    let mixer_words = mixer.halves.len() as u32 / 2;
    assert!(LOAD_MIXER_SOURCE + 4 * mixer_words <= LOAD_MAIN);
    mixer.place(&mut rom, LOAD_MIXER_SOURCE);

    let mut setup = Code::new(LOAD_SETUP);
    setup.arm(&[
        0xE3A0_0301,
        0xE3A0_1018,
        0xE1C0_10B4,
        0xE3A0_1402,
        0xE580_10B0,
        0xE3A0_1403,
        0xE281_1901,
        0xE580_10B4,
        0xE3A0_1004,
        0xE1C0_1BB8,
        0xE3A0_1CA7,
        0xE281_1060,
        0xE1C0_1BBA,
        0xE3A0_1403,
        0xE281_1C7F,
        0xE281_10FC,
        0xE3A0_2302,
        0xE282_2C03,
        0xE581_2000,
        0xE3A0_B301,
        0xE28B_BC02,
        0xE3A0_1C43,
        0xE281_1017,
        0xE1CB_10B4,
        0xE3A0_9402,
        0xE589_9000,
        0xE589_B004,
        0xE3A0_7000,
        0xE3A0_8403,
        0xE288_8C01,
        0xE3A0_2302,
        0xE282_2FD0,
        0xE1A0_3008,
        0xE3A0_1000 | mixer_words,
    ]);
    let copy = setup.here();
    setup.arm(&[0xE492_C004, 0xE483_C004, 0xE251_1001]);
    setup.arm_b(0x1, copy);
    setup.arm(&[
        0xE3A0_1003,
        0xE1CB_10B0,
        0xE3A0_1001,
        0xE1CB_10B8,
        0xE3A0_C302,
        0xE28C_CB01,
        0xE28C_C001,
        0xE12F_FF1C,
    ]);
    assert!(setup.here() <= LOAD_IRQ);
    setup.place(&mut rom, LOAD_SETUP);

    let mut irq = Code::new(LOAD_IRQ);
    irq.arm(&[
        0xE3A0_3301,
        0xE283_3C02,
        0xE593_2000,
        0xE002_1822,
        0xE1C3_10B2,
        0xE3A0_0403,
        0xE280_0C7F,
        0xE1D0_2FB8,
        0xE182_2001,
        0xE1C0_2FB8,
        0xE12F_FF1E,
    ]);
    assert!(irq.here() <= LOAD_MIXER_SOURCE);
    irq.place(&mut rom, LOAD_IRQ);

    let pool = [
        LOAD_WAITS,
        LOAD_TABLE,
        LOAD_TABLE + 0x0200_0000,
        LOAD_TABLE + 0x0400_0000,
        LOAD_MIXER,
        LOAD_EWRAM_COPY,
        LOAD_WORDS,
        LOAD_DATA,
    ];
    let literal = |value: u32| {
        let index = pool.iter().position(|&word| word == value).expect("pooled");
        LOAD_POOL + 4 * index as u32
    };
    let mut main = Code::new(LOAD_MAIN);
    let top = main.here();
    main.thumb(&[0x2007, 0x4038, 0x0040]);
    main.ldr_pc(1, literal(LOAD_WAITS));
    main.thumb(&[0x5A09, 0x4658, 0x8081, 0x3701, 0x2603, 0x403E]);
    main.ldr_pc(4, literal(LOAD_TABLE));
    for k in 0..8 {
        for _ in 0..k {
            main.thumb(&[0x3101]);
        }
        main.ldr_pc(2, literal(LOAD_DATA));
        main.thumb(&[
            0x18AD,
            thumb_ldrb_imm(3, 4, k),
            0x57A2,
            0x18ED,
            0x18AD,
            0x5FA3,
            0x18ED,
        ]);
        main.thumb(&[thumb_ldrh_imm(3, 4, 2 * k), 0x18ED]);
    }
    main.thumb(&[0x4351, 0x4351, 0x4351]);
    main.ldr_pc(2, literal(LOAD_DATA));
    main.thumb(&[
        0x18AD,
        0x4648,
        0xC80E,
        0x1889,
        0x18C9,
        thumb_ldrb_imm(3, 4, 3),
        0x18ED,
    ]);
    main.thumb(&[
        thumb_ldrb_imm(3, 4, 5),
        thumb_ldrh_imm(2, 4, 6),
        0x18ED,
        0x18AD,
    ]);
    main.bl(LOAD_PAGE);
    main.ldr_pc(3, literal(LOAD_MIXER));
    let mut veneer_calls = Vec::new();
    for table in [
        LOAD_TABLE,
        LOAD_TABLE + 0x0200_0000,
        LOAD_TABLE + 0x0400_0000,
    ] {
        main.ldr_pc(0, literal(table));
        main.thumb(&[0x1980]);
        veneer_calls.push(main.halves.len());
        main.thumb(&[0, 0]);
    }
    main.ldr_pc(0, literal(LOAD_TABLE));
    main.ldr_pc(1, literal(LOAD_EWRAM_COPY));
    main.thumb(&[0x2208, 0xDF0B]);
    main.ldr_pc(0, literal(LOAD_TABLE));
    main.ldr_pc(1, literal(LOAD_EWRAM_COPY));
    main.ldr_pc(2, literal(LOAD_WORDS));
    main.thumb(&[0xDF0B, 0x4648, 0x6245, 0x203F, 0x4038]);
    let waited = main.here() + 4;
    main.bne(waited);
    main.thumb(&[0xDF05]);
    assert_eq!(main.here(), waited);
    main.b(top);
    let veneer = main.here();
    main.thumb(&[0x4718]);
    for call in veneer_calls {
        let from = LOAD_MAIN + 2 * call as u32;
        main.halves[call..call + 2].copy_from_slice(&thumb_bl(from, veneer));
    }
    assert!(main.here() <= LOAD_POOL);
    main.place(&mut rom, LOAD_MAIN);

    let mut data = Code::new(LOAD_POOL);
    data.arm(&pool);
    assert!(data.here() <= LOAD_WAITS);
    data.place(&mut rom, LOAD_POOL);
    let mut waits = Code::new(LOAD_WAITS);
    waits.thumb(&FETCH_WAITCNT);
    assert!(waits.here() <= LOAD_TABLE);
    waits.place(&mut rom, LOAD_WAITS);
    let mut table = Code::new(LOAD_TABLE);
    table.arm(&[
        0x81FF_807F,
        0x90FE_017E,
        0x3CC3_8F70,
        0x0FF0_5AA5,
        0x8001_7FFF,
        0xC0DE_8BAD,
        0xFFFF_8000,
        0x7FFF_0080,
    ]);
    table.place(&mut rom, LOAD_TABLE);

    let mut page = Code::new(LOAD_PAGE);
    page.thumb(&[
        0xB500,
        thumb_ldrb_imm(3, 4, 0),
        0x18ED,
        thumb_ldrh_imm(3, 4, 2),
        0x18ED,
        0x57A3,
        0x18ED,
        0x5FA3,
        0x18ED,
        0x6823,
        0x18ED,
    ]);
    assert_eq!(page.here(), LOAD_PAGE + 0x16);
    page.ldr_pc(3, LOAD_PAGE_END);
    page.thumb(&[
        0x18ED,
        thumb_ldrb_imm(3, 4, 1),
        0x18ED,
        thumb_ldrh_imm(3, 4, 0),
    ]);
    assert_eq!(page.here(), LOAD_PAGE_END);
    page.thumb(&[0x18ED, 0x57A3, 0x18ED, 0x6823, 0x18ED, 0xBD00]);
    assert!(page.here() <= LOAD_TAIL);
    page.place(&mut rom, LOAD_PAGE);
    let tail = (LOAD_TAIL - GBA_ROM) as usize;
    rom[tail..].copy_from_slice(&[0x81, 0x80]);
    rom
}

#[test]
fn gba_rom_load_encodings_match_hand_encoded_words() {
    assert_eq!(thumb_ldrb_imm(3, 4, 0), 0x7823);
    assert_eq!(thumb_ldrb_imm(3, 4, 1), 0x7863);
    assert_eq!(thumb_ldrb_imm(3, 4, 7), 0x79E3);
    assert_eq!(thumb_ldrh_imm(3, 4, 2), 0x8863);
    assert_eq!(thumb_ldrh_imm(3, 4, 14), 0x89E3);
    assert_eq!(thumb_ldr_pc(0x0801_FFF6, 3, LOAD_PAGE_END), 0x4B02);
    assert_eq!(arm_b(0x1, 0x0300_0108, 0x0300_0104), 0x1AFF_FFFD);
    assert_eq!(arm_b(0x1, 0x0300_014C, 0x0300_0114), 0x1AFF_FFF0);
    assert_eq!(arm_b(0x1, 0x0800_0194, 0x0800_0188), 0x1AFF_FFFB);
}

#[test]
fn gba_rom_loads_run_the_same_with_the_jit() {
    let case = Case {
        name: "gba-rom-loads".into(),
        rom: gba_load_rom(),
        frames: 2 * STATE_EVERY + 1,
        press: Vec::new(),
    };
    check::<Gba>(std::slice::from_ref(&case));
    let mut gba = <Gba as Machine>::boot(&case.rom, false);
    for _ in 0..case.frames {
        Machine::run_frame(&mut gba);
    }
    let passes = gba.cpu().reg(7);
    assert!(
        passes > 8 * 64,
        "the load loop ran {passes} passes, its vblank waits stall"
    );
    older_state::<Gba>(case);
}

const LITERAL_ROM_SIZE: usize = 0x2_0102;
const LITERAL_HEADER: u32 = 0x0800_00B0;
const LITERAL_HEADER_WORDS: u32 = 0x0800_00C4;
const LITERAL_MAIN: u32 = FETCH_MAIN;
const LITERAL_ROUTINE: u32 = 0x0800_0600;
const LITERAL_ROUTINE_WORDS: u32 = 0x0800_0620;
const LITERAL_POOL: u32 = 0x0800_0700;
const LITERAL_WAITS: u32 = 0x0800_0740;
const LITERAL_PAGE: u32 = 0x0801_FFE0;
const LITERAL_PAGE_WORDS: u32 = 0x0801_FFF8;
const LITERAL_PAGE_END: u32 = 0x0802_0000;
const LITERAL_END: u32 = 0x0802_00E0;
const LITERAL_LAST: u32 = 0x0802_00FC;
const LITERAL_TAIL: u32 = 0x0802_0100;
const LITERAL_PAST: u32 = 0x0802_0104;
const LITERAL_DATA: [u32; 4] = [0x80FF_7F81, 0x0123_4567, 0xFEDC_BA98, 0x7FFF_FFFF];
const LITERAL_MIRRORS: [u32; 3] = [
    LITERAL_ROUTINE + 1,
    LITERAL_ROUTINE + 0x0200_0001,
    LITERAL_ROUTINE + 0x0400_0001,
];

fn gba_literal_rom() -> Vec<u8> {
    let mut rom = vec![0; LITERAL_ROM_SIZE];
    gba_fetch_setup(&mut rom);

    let mut header = Code::new(LITERAL_HEADER);
    for at in [0x0800_00C4, 0x0800_00C8, 0x0800_00CC] {
        header.ldr_pc(2, at);
        header.thumb(&[0x18AD]);
    }
    header.thumb(&[0x4770]);
    assert!(header.here() <= LITERAL_HEADER_WORDS);
    header.place(&mut rom, LITERAL_HEADER);
    let mut words = Code::new(LITERAL_HEADER_WORDS);
    words.arm(&[0x1357_9BDF, 0x2468_ACE0, 0x0F1E_2D3C]);
    words.place(&mut rom, LITERAL_HEADER_WORDS);

    let mut routine = Code::new(LITERAL_ROUTINE);
    routine.ldr_pc(2, LITERAL_ROUTINE_WORDS);
    routine.ldr_pc(3, LITERAL_ROUTINE_WORDS + 4);
    routine.thumb(&[0x18AD, 0x18ED]);
    routine.ldr_pc(2, LITERAL_ROUTINE_WORDS);
    routine.thumb(&[0x18AD, 0x4770]);
    assert!(routine.here() <= LITERAL_ROUTINE_WORDS);
    routine.place(&mut rom, LITERAL_ROUTINE);
    let mut words = Code::new(LITERAL_ROUTINE_WORDS);
    words.arm(&[0x0BAD_F00D, 0xC0FF_EE00]);
    words.place(&mut rom, LITERAL_ROUTINE_WORDS);

    let mut page = Code::new(LITERAL_PAGE);
    page.ldr_pc(2, LITERAL_PAGE_WORDS);
    page.ldr_pc(3, LITERAL_PAGE_WORDS + 4);
    page.thumb(&[0x18AD, 0x18ED]);
    page.ldr_pc(2, LITERAL_PAGE_END);
    page.ldr_pc(3, LITERAL_PAGE_END + 4);
    page.thumb(&[0x18AD, 0x18ED, 0x4770]);
    assert!(page.here() <= LITERAL_PAGE_WORDS);
    page.place(&mut rom, LITERAL_PAGE);
    let mut words = Code::new(LITERAL_PAGE_WORDS);
    words.arm(&[0x8000_0001, 0x4000_0002, 0x2000_0004, 0x1000_0008]);
    words.place(&mut rom, LITERAL_PAGE_WORDS);

    let mut end = Code::new(LITERAL_END);
    end.ldr_pc(2, LITERAL_LAST);
    end.ldr_pc(3, LITERAL_TAIL);
    end.thumb(&[0x18AD, 0x18ED]);
    end.ldr_pc(2, LITERAL_PAST);
    end.thumb(&[0x18AD, 0x4770]);
    assert!(end.here() <= LITERAL_LAST);
    end.place(&mut rom, LITERAL_END);
    let mut words = Code::new(LITERAL_LAST);
    words.arm(&[0x55AA_33CC]);
    words.place(&mut rom, LITERAL_LAST);
    let tail = (LITERAL_TAIL - GBA_ROM) as usize;
    rom[tail..].copy_from_slice(&[0x81, 0x80]);

    let mut pool = vec![LITERAL_WAITS];
    pool.extend(LITERAL_DATA);
    pool.extend(LITERAL_MIRRORS);
    let literal = |value: u32| {
        let index = pool.iter().position(|&word| word == value).expect("pooled");
        LITERAL_POOL + 4 * index as u32
    };
    let mut main = Code::new(LITERAL_MAIN);
    let top = main.here();
    main.thumb(&[0x2007, 0x4038, 0x0040]);
    main.ldr_pc(1, literal(LITERAL_WAITS));
    main.thumb(&[0x5A09, 0x4658, 0x8081, 0x3701]);
    for k in 0..8 {
        for _ in 0..k {
            main.thumb(&[0x3101]);
        }
        main.ldr_pc(2, literal(LITERAL_DATA[k % 4]));
        main.thumb(&[0x18AD]);
    }
    main.ldr_pc(2, literal(LITERAL_DATA[0]));
    main.ldr_pc(3, literal(LITERAL_DATA[1]));
    main.thumb(&[0x18AD, 0x18ED]);
    if !main.here().is_multiple_of(4) {
        main.thumb(&[0x46C0]);
    }
    let aligned = main.here();
    main.ldr_pc(2, aligned + 4);
    main.thumb(&[0x18AD, 0x3101, 0x3101, 0x46C0]);
    let unaligned = main.here();
    assert_eq!(unaligned % 4, 2);
    main.ldr_pc(2, unaligned + 2);
    main.thumb(&[0x18AD, 0x3101]);
    main.thumb(&[0x4351, 0x4351, 0x4351]);
    main.ldr_pc(2, literal(LITERAL_DATA[2]));
    main.thumb(&[0x18AD, 0x4648, 0xC80E]);
    main.ldr_pc(2, literal(LITERAL_DATA[3]));
    main.thumb(&[0x18AD]);
    main.bl(LITERAL_HEADER);
    main.bl(LITERAL_PAGE);
    main.bl(LITERAL_END);
    let mut veneer_calls = Vec::new();
    for mirror in LITERAL_MIRRORS {
        main.ldr_pc(3, literal(mirror));
        veneer_calls.push(main.halves.len());
        main.thumb(&[0, 0]);
    }
    main.thumb(&[0x4648, 0x6245, 0x203F, 0x4038]);
    let waited = main.here() + 4;
    main.bne(waited);
    main.thumb(&[0xDF05]);
    assert_eq!(main.here(), waited);
    main.b(top);
    let veneer = main.here();
    main.thumb(&[0x4718]);
    for call in veneer_calls {
        let from = LITERAL_MAIN + 2 * call as u32;
        main.halves[call..call + 2].copy_from_slice(&thumb_bl(from, veneer));
    }
    assert!(main.here() <= LITERAL_ROUTINE);
    main.place(&mut rom, LITERAL_MAIN);

    let mut data = Code::new(LITERAL_POOL);
    data.arm(&pool);
    assert!(data.here() <= LITERAL_WAITS);
    data.place(&mut rom, LITERAL_POOL);
    let mut waits = Code::new(LITERAL_WAITS);
    waits.thumb(&FETCH_WAITCNT);
    waits.place(&mut rom, LITERAL_WAITS);
    rom
}

#[test]
fn gba_rom_literal_encodings_match_hand_encoded_words() {
    assert_eq!(thumb_ldr_pc(0x0800_00B0, 2, 0x0800_00C4), 0x4A04);
    assert_eq!(thumb_ldr_pc(0x0800_00B4, 2, 0x0800_00C8), 0x4A04);
    assert_eq!(thumb_ldr_pc(0x0800_00B8, 2, 0x0800_00CC), 0x4A04);
    assert_eq!(thumb_ldr_pc(0x0800_0600, 2, 0x0800_0620), 0x4A07);
    assert_eq!(thumb_ldr_pc(0x0800_0602, 3, 0x0800_0624), 0x4B08);
    assert_eq!(thumb_ldr_pc(0x0800_0608, 2, 0x0800_0620), 0x4A05);
    assert_eq!(thumb_ldr_pc(0x0801_FFE0, 2, 0x0801_FFF8), 0x4A05);
    assert_eq!(thumb_ldr_pc(0x0801_FFE2, 3, 0x0801_FFFC), 0x4B06);
    assert_eq!(thumb_ldr_pc(0x0801_FFE8, 2, LITERAL_PAGE_END), 0x4A05);
    assert_eq!(thumb_ldr_pc(0x0801_FFEA, 3, 0x0802_0004), 0x4B06);
    assert_eq!(thumb_ldr_pc(0x0802_00E0, 2, LITERAL_LAST), 0x4A06);
    assert_eq!(thumb_ldr_pc(0x0802_00E2, 3, LITERAL_TAIL), 0x4B07);
    assert_eq!(thumb_ldr_pc(0x0802_00E8, 2, LITERAL_PAST), 0x4A06);
    assert_eq!(thumb_ldr_pc(0x0800_0408, 2, 0x0800_040C), 0x4A00);
    assert_eq!(thumb_ldr_pc(0x0800_040A, 2, 0x0800_040C), 0x4A00);
    assert_eq!(thumb_bl(0x0800_0500, LITERAL_HEADER), [0xF7FF, 0xFDD6]);
    assert_eq!(thumb_bl(0x0800_0500, LITERAL_END), [0xF01F, 0xFDEE]);
    assert_eq!(thumb_bne(0x0800_0500, 0x0800_0504), 0xD100);
    let rom = gba_literal_rom();
    let half = |addr: u32| {
        let at = (addr - GBA_ROM) as usize;
        u16::from_le_bytes([rom[at], rom[at + 1]])
    };
    let word = |addr: u32| u32::from(half(addr)) | u32::from(half(addr + 2)) << 16;
    assert_eq!(half(LITERAL_HEADER), 0x4A04);
    assert_eq!(half(LITERAL_HEADER + 0xC), 0x4770);
    assert_eq!(word(0x0800_00CC), 0x0F1E_2D3C);
    assert_eq!(half(LITERAL_ROUTINE), 0x4A07);
    assert_eq!(half(LITERAL_ROUTINE + 0xC), 0x4770);
    assert_eq!(half(LITERAL_PAGE + 0x10), 0x4770);
    assert_eq!(word(LITERAL_PAGE_END), 0x2000_0004);
    assert_eq!(half(LITERAL_END + 0xC), 0x4770);
    assert_eq!(word(LITERAL_LAST), 0x55AA_33CC);
    assert_eq!(half(LITERAL_TAIL), 0x8081);
    assert_eq!(rom.len(), LITERAL_ROM_SIZE);
}

#[test]
fn gba_rom_literals_run_the_same_with_the_jit() {
    let case = Case {
        name: "gba-rom-literals".into(),
        rom: gba_literal_rom(),
        frames: 2 * STATE_EVERY + 1,
        press: Vec::new(),
    };
    check::<Gba>(std::slice::from_ref(&case));
    let blocks = warm_case::<Gba>(&case).unwrap_or_else(|error| panic!("warm: {error}"));
    assert!(blocks > 0, "the JIT ran no block with a warm cache");
    let mut gba = <Gba as Machine>::boot(&case.rom, false);
    for _ in 0..case.frames {
        Machine::run_frame(&mut gba);
    }
    let passes = gba.cpu().reg(7);
    assert!(
        passes > 8 * 64,
        "the literal loop ran {passes} passes, its vblank waits stall"
    );
    older_state::<Gba>(case);
}

const BLOCK_ROM_SIZE: usize = 0x700;
const BLOCK_SETUP: u32 = 0x0800_0100;
const BLOCK_IRQ: u32 = 0x0800_0300;
const BLOCK_ROUTINE_SOURCE: u32 = 0x0800_0340;
const BLOCK_MAIN: u32 = 0x0800_0400;
const BLOCK_POOL: u32 = 0x0800_0600;
const BLOCK_WAITS: u32 = 0x0800_0680;
const BLOCK_TABLE: u32 = 0x0800_06A0;
const BLOCK_ROUTINE: u32 = 0x0300_0700;
const BLOCK_IWRAM_DATA: u32 = 0x0300_0300;
const BLOCK_FAST_COPY: u32 = 0x0300_0380;
const BLOCK_EWRAM_DATA: u32 = 0x0200_0100;
const BLOCK_UNMAPPED: u32 = 0x1000_0000;
const BLOCK_WAITCNT: [u16; 4] = [0x4317, 0x4014, 0x0317, 0x45B4];

fn thumb_push(list: u16) -> u16 {
    assert!(list < 0x200);
    0xB400 | list
}

fn thumb_pop(list: u16) -> u16 {
    assert!(list < 0x100);
    0xBC00 | list
}

fn thumb_stmia(rb: u16, list: u16) -> u16 {
    assert!(rb < 8 && list < 0x100);
    0xC000 | rb << 8 | list
}

fn thumb_ldmia(rb: u16, list: u16) -> u16 {
    assert!(rb < 8 && list < 0x100);
    0xC800 | rb << 8 | list
}

fn arm_block(op: u32, rn: u32, list: u32) -> u32 {
    assert!(op < 0x20 && rn < 16 && list <= 0xFFFF);
    0xE800_0000 | op << 20 | rn << 16 | list
}

fn gba_block_setup(rom: &mut [u8], routine: &Code) {
    let mut entry = Code::new(GBA_ROM);
    entry.arm_b(0xE, BLOCK_SETUP);
    entry.place(rom, GBA_ROM);
    let routine_words = routine.halves.len() as u32 / 2;
    assert!(BLOCK_ROUTINE_SOURCE + 4 * routine_words <= BLOCK_MAIN);
    routine.place(rom, BLOCK_ROUTINE_SOURCE);

    let mut setup = Code::new(BLOCK_SETUP);
    setup.arm(&[
        0xE3A0_0301,
        0xE3A0_1018,
        0xE1C0_10B4,
        0xE3A0_1402,
        0xE580_10B0,
        0xE3A0_1403,
        0xE281_1901,
        0xE580_10B4,
        0xE3A0_1004,
        0xE1C0_1BB8,
        0xE3A0_1CA7,
        0xE281_1060,
        0xE1C0_1BBA,
        0xE3A0_1403,
        0xE281_1C7F,
        0xE281_10FC,
        0xE3A0_2302,
        0xE282_2C03,
        0xE581_2000,
        0xE3A0_B301,
        0xE28B_BC02,
        0xE3A0_1C43,
        0xE281_1017,
        0xE1CB_10B4,
        0xE3A0_6000,
        0xE3A0_7000,
        0xE3A0_8403,
        0xE288_8C07,
        0xE3A0_2302,
        0xE282_2FD0,
        0xE1A0_3008,
        0xE3A0_1000 | routine_words,
    ]);
    let copy = setup.here();
    setup.arm(&[0xE492_C004, 0xE483_C004, 0xE251_1001]);
    setup.arm_b(0x1, copy);
    setup.arm(&[
        0xE3A0_1003,
        0xE1CB_10B0,
        0xE3A0_1001,
        0xE1CB_10B8,
        0xE3A0_C302,
        0xE28C_CB01,
        0xE28C_C001,
        0xE12F_FF1C,
    ]);
    assert!(setup.here() <= BLOCK_IRQ);
    setup.place(rom, BLOCK_SETUP);

    let mut irq = Code::new(BLOCK_IRQ);
    irq.arm(&[
        0xE3A0_3301,
        0xE283_3C02,
        0xE593_2000,
        0xE002_1822,
        0xE1C3_10B2,
        0xE3A0_0403,
        0xE280_0C7F,
        0xE1D0_2FB8,
        0xE182_2001,
        0xE1C0_2FB8,
        0xE12F_FF1E,
    ]);
    assert!(irq.here() <= BLOCK_ROUTINE_SOURCE);
    irq.place(rom, BLOCK_IRQ);
}

fn gba_block_rom() -> Vec<u8> {
    let mut rom = vec![0; BLOCK_ROM_SIZE];
    let mut routine = Code::new(BLOCK_ROUTINE);
    routine.arm(&[
        arm_block(0x12, 13, 0x4FF0),
        0xE1A0_C000,
        0xE3A0_5000,
        arm_block(0x09, 12, 0x001E),
        0xE085_5001,
        0xE085_5004,
        0xE081_1005,
        arm_block(0x18, 12, 0x001E),
        arm_block(0x01, 0, 0x001E),
        0xE085_5002,
        0xE085_5003,
        0xE315_0001,
        0x189C_01C0,
        0x1085_5006,
        arm_block(0x12, 13, 0x0020),
        arm_block(0x0B, 0, 0x5FFE),
        0xE081_100E,
        arm_block(0x0B, 13, 0x0020),
        0xE085_5001,
        arm_block(0x09, 0, 0x0007),
        0xE085_5000,
        0xE085_5002,
        0xE1A0_0005,
        arm_block(0x0B, 13, 0x4FF0),
        0xE12F_FF1E,
    ]);
    gba_block_setup(&mut rom, &routine);

    let pool = [
        BLOCK_WAITS,
        BLOCK_UNMAPPED,
        BLOCK_IWRAM_DATA,
        BLOCK_EWRAM_DATA + 2,
        0x0300_0FF8,
        0x0200_0FFC,
        0x0300_8300,
        0x0204_0100,
        BLOCK_TABLE,
        0x0300_03FC,
        0x0300_0500,
        BLOCK_ROUTINE,
        BLOCK_EWRAM_DATA,
        BLOCK_FAST_COPY,
    ];
    let literal = |value: u32| {
        let index = pool.iter().position(|&word| word == value).expect("pooled");
        BLOCK_POOL + 4 * index as u32
    };
    let mut main = Code::new(BLOCK_MAIN);
    let top = main.here();
    main.thumb(&[0x2003, 0x4038, 0x0040]);
    main.ldr_pc(1, literal(BLOCK_WAITS));
    main.thumb(&[0x5A09, 0x4658, 0x8081]);
    main.thumb(&[thumb_push(0x1F0), thumb_pop(0xF0), thumb_pop(0x08), 0x18F6]);
    main.thumb(&[thumb_push(0x100), thumb_pop(0x08), 0x18F6]);
    main.ldr_pc(4, literal(BLOCK_UNMAPPED));
    main.ldr_pc(0, literal(BLOCK_IWRAM_DATA));
    main.thumb(&[
        0x1C39,
        0x1C32,
        0x1D7B,
        thumb_stmia(0, 0x0E),
        0x380C,
        thumb_ldmia(0, 0x0E),
        0x6825,
        0x1976,
        0x1876,
        0x18B6,
        0x18F6,
        0x1F01,
        thumb_ldmia(1, 0x06),
        0x1876,
        0x18B6,
        0x1C01,
        thumb_stmia(1, 0x03),
        thumb_stmia(0, 0x03),
        0x3808,
        thumb_ldmia(0, 0x0C),
        0x18B6,
        0x18F6,
    ]);
    main.ldr_pc(0, literal(BLOCK_EWRAM_DATA + 2));
    main.thumb(&[
        0x1C79,
        0x1C32,
        thumb_stmia(0, 0x06),
        0x3808,
        thumb_ldmia(0, 0x06),
        0x1876,
        0x18B6,
    ]);
    for crossing in [0x0300_0FF8, 0x0200_0FFC] {
        main.ldr_pc(0, literal(crossing));
        main.thumb(&[thumb_stmia(0, 0x2E), 0x3810, thumb_ldmia(0, 0x2E), 0x1976]);
    }
    for elsewhere in [0x0300_8300, 0x0204_0100, BLOCK_TABLE] {
        main.ldr_pc(0, literal(elsewhere));
        main.thumb(&[thumb_ldmia(0, 0x06), 0x1876, 0x18B6]);
    }
    main.ldr_pc(0, literal(0x0300_03FC));
    main.thumb(&[thumb_stmia(0, 0x06)]);
    let mut veneer_calls = Vec::new();
    for data in [BLOCK_IWRAM_DATA, BLOCK_EWRAM_DATA] {
        main.ldr_pc(3, literal(BLOCK_ROUTINE));
        main.ldr_pc(0, literal(data));
        veneer_calls.push(main.halves.len());
        main.thumb(&[0, 0, 0x1836]);
        if data == BLOCK_IWRAM_DATA {
            main.ldr_pc(0, literal(0x0300_0500));
            main.thumb(&[thumb_stmia(0, 0x02)]);
        }
    }
    main.ldr_pc(0, literal(BLOCK_EWRAM_DATA));
    main.ldr_pc(1, literal(BLOCK_FAST_COPY));
    main.thumb(&[0x2208, 0xDF0C]);
    main.ldr_pc(0, literal(BLOCK_FAST_COPY));
    main.thumb(&[thumb_ldmia(0, 0x06), 0x1876, 0x18B6]);
    main.thumb(&[0x3701, 0x203F, 0x4038]);
    let waited = main.here() + 4;
    main.bne(waited);
    main.thumb(&[0xDF05]);
    assert_eq!(main.here(), waited);
    main.b(top);
    let veneer = main.here();
    main.thumb(&[0x4718]);
    for call in veneer_calls {
        let from = BLOCK_MAIN + 2 * call as u32;
        main.halves[call..call + 2].copy_from_slice(&thumb_bl(from, veneer));
    }
    assert!(main.here() <= BLOCK_POOL);
    main.place(&mut rom, BLOCK_MAIN);

    let mut data = Code::new(BLOCK_POOL);
    data.arm(&pool);
    assert!(data.here() <= BLOCK_WAITS);
    data.place(&mut rom, BLOCK_POOL);
    let mut waits = Code::new(BLOCK_WAITS);
    waits.thumb(&BLOCK_WAITCNT);
    assert!(waits.here() <= BLOCK_TABLE);
    waits.place(&mut rom, BLOCK_WAITS);
    let mut table = Code::new(BLOCK_TABLE);
    table.arm(&[0x81FF_807F, 0x90FE_017E, 0x3CC3_8F70, 0x0FF0_5AA5]);
    assert!(table.here() <= GBA_ROM + BLOCK_ROM_SIZE as u32);
    table.place(&mut rom, BLOCK_TABLE);
    rom
}

#[test]
fn gba_block_transfer_encodings_match_hand_encoded_words() {
    assert_eq!(thumb_push(0x1F0), 0xB5F0);
    assert_eq!(thumb_push(0x100), 0xB500);
    assert_eq!(thumb_pop(0xF0), 0xBCF0);
    assert_eq!(thumb_pop(0x08), 0xBC08);
    assert_eq!(thumb_stmia(0, 0x0E), 0xC00E);
    assert_eq!(thumb_stmia(1, 0x03), 0xC103);
    assert_eq!(thumb_ldmia(0, 0x0E), 0xC80E);
    assert_eq!(thumb_ldmia(1, 0x06), 0xC906);
    assert_eq!(arm_block(0x12, 13, 0x4FF0), 0xE92D_4FF0);
    assert_eq!(arm_block(0x0B, 13, 0x4FF0), 0xE8BD_4FF0);
    assert_eq!(arm_block(0x09, 12, 0x001E), 0xE89C_001E);
    assert_eq!(arm_block(0x18, 12, 0x001E), 0xE98C_001E);
    assert_eq!(arm_block(0x01, 0, 0x001E), 0xE810_001E);
    assert_eq!(arm_block(0x0B, 0, 0x5FFE), 0xE8B0_5FFE);
    assert_eq!(arm_block(0x09, 0, 0x0007), 0xE890_0007);
    assert_eq!(arm_block(0x12, 13, 0x0020), 0xE92D_0020);
    assert_eq!(arm_block(0x0B, 13, 0x0020), 0xE8BD_0020);
    assert_eq!(
        (arm_block(0x09, 12, 0x01C0) & 0x0FFF_FFFF) | 0x1000_0000,
        0x189C_01C0
    );
    assert_eq!(arm_b(0xE, GBA_ROM, BLOCK_SETUP), 0xEA00_003E);
    assert_eq!(arm_b(0x1, 0x0800_018C, 0x0800_0180), 0x1AFF_FFFB);
    assert_eq!(thumb_bl(0x0800_0490, 0x0800_04C0), [0xF000, 0xF816]);
    assert_eq!(thumb_bl(0x0800_049E, 0x0800_04C0), [0xF000, 0xF80F]);
    assert_eq!(thumb_bne(0x0800_04BA, 0x0800_04BE), 0xD100);
    assert_eq!(thumb_b(0x0800_04BE, BLOCK_MAIN), 0xE79F);
}

#[test]
fn gba_block_transfers_run_the_same_with_the_jit() {
    let case = Case {
        name: "gba-block-transfers".into(),
        rom: gba_block_rom(),
        frames: 2 * STATE_EVERY + 1,
        press: Vec::new(),
    };
    check::<Gba>(std::slice::from_ref(&case));
    let mut gba = <Gba as Machine>::boot(&case.rom, false);
    for _ in 0..case.frames {
        Machine::run_frame(&mut gba);
    }
    let passes = gba.cpu().reg(7);
    assert!(
        passes > 8 * 64,
        "the block loop ran {passes} passes, its vblank waits stall"
    );
    older_state::<Gba>(case);
}

const RETURN_ROM_SIZE: usize = 0x800;
const RETURN_POOL: u32 = 0x0800_0600;
const RETURN_WAITS: u32 = 0x0800_0680;
const RETURN_ROM_ARM: u32 = 0x0800_0700;
const RETURN_ROM_LEAF: u32 = RETURN_ROM_ARM + 0x10;
const RETURN_BIOS: u32 = 0x0000_029C;
const RETURN_CONT1: u32 = BLOCK_ROUTINE + 0x1C;
const RETURN_CONT2: u32 = BLOCK_ROUTINE + 0x28;
const RETURN_CONT3: u32 = BLOCK_ROUTINE + 0x44;
const RETURN_ODD: u32 = BLOCK_ROUTINE + 0x5C;
const RETURN_AFTER_ODD: u32 = BLOCK_ROUTINE + 0x64;
const RETURN_AFTER_BIOS: u32 = BLOCK_ROUTINE + 0x78;
const RETURN_AFTER_S: u32 = BLOCK_ROUTINE + 0x88;
const RETURN_INNER: u32 = BLOCK_ROUTINE + 0x90;
const RETURN_TABLE: u32 = BLOCK_ROUTINE + 0x9C;
const RETURN_TABLE2: u32 = BLOCK_ROUTINE + 0xA8;
const RETURN_LITERAL: u32 = BLOCK_ROUTINE + 0xB0;
const RETURN_PASS_SUM: u32 = 1 + 4 + 2 + 0x1001 + 0x0303 + 3 + 5;

fn arm_bl(from: u32, to: u32) -> u32 {
    arm_b(0xE, from, to) | 1 << 24
}

fn arm_adr(from: u32, rd: u32, to: u32) -> u32 {
    let offset = to.wrapping_sub(from + 8);
    assert!(rd < 16 && offset < 0x100);
    0xE28F_0000 | rd << 12 | offset
}

fn arm_ldr_pc(from: u32, rd: u32, to: u32) -> u32 {
    let offset = to.wrapping_sub(from + 8);
    assert!(rd < 16 && offset < 0x1000);
    0xE59F_0000 | rd << 12 | offset
}

fn thumb_pop_pc(list: u16) -> u16 {
    assert!(list < 0x100);
    0xBD00 | list
}

fn push_return(code: &mut Code, to: u32) {
    let from = code.here();
    code.arm(&[
        arm_adr(from, 1, to),
        arm_block(0x12, 13, 0x0002),
        arm_block(0x12, 13, 0x0010),
    ]);
}

fn gba_block_return_rom() -> Vec<u8> {
    let mut rom = vec![0; RETURN_ROM_SIZE];
    let mut routine = Code::new(BLOCK_ROUTINE);
    routine.arm(&[arm_block(0x12, 13, 0x4070)]);
    let from = routine.here();
    routine.arm(&[arm_bl(from, RETURN_INNER)]);
    let from = routine.here();
    routine.arm(&[
        arm_ldr_pc(from, 0, RETURN_LITERAL),
        0xE1A0_E00F,
        0xE12F_FF10,
    ]);
    let from = routine.here();
    routine.arm(&[arm_adr(from, 0, RETURN_TABLE), arm_block(0x0B, 0, 0x8003)]);
    assert_eq!(routine.here(), RETURN_CONT1);
    routine.arm(&[0xE08A_A000]);
    let from = routine.here();
    routine.arm(&[arm_adr(from, 0, RETURN_TABLE2), arm_block(0x09, 0, 0x8001)]);
    assert_eq!(routine.here(), RETURN_CONT2);
    routine.arm(&[0xE08A_A000, 0xE156_0006, 0x18BD_8070]);
    push_return(&mut routine, RETURN_CONT3);
    routine.arm(&[0x08BD_8010]);
    assert_eq!(routine.here(), RETURN_CONT3);
    routine.arm(&[0xE28A_A003]);
    let from = routine.here();
    routine.arm(&[arm_adr(from, 9, RETURN_AFTER_ODD)]);
    push_return(&mut routine, RETURN_ODD | 1);
    routine.arm(&[arm_block(0x0B, 13, 0x8010)]);
    assert_eq!(routine.here(), RETURN_ODD);
    routine.arm(&[0xE28A_A005, 0xE12F_FF19]);
    assert_eq!(routine.here(), RETURN_AFTER_ODD);
    let from = routine.here();
    routine.arm(&[
        arm_adr(from, 14, RETURN_AFTER_BIOS),
        0xE3A0_1FA7,
        arm_block(0x12, 13, 0x0002),
        arm_block(0x12, 13, 0x0010),
        arm_block(0x0B, 13, 0x8010),
    ]);
    assert_eq!(routine.here(), RETURN_AFTER_BIOS);
    push_return(&mut routine, RETURN_AFTER_S);
    routine.arm(&[arm_block(0x0F, 13, 0x8010)]);
    assert_eq!(routine.here(), RETURN_AFTER_S);
    routine.arm(&[arm_block(0x0B, 13, 0x4070), 0xE12F_FF1E]);
    assert_eq!(routine.here(), RETURN_INNER);
    routine.arm(&[
        arm_block(0x12, 13, 0x4070),
        0xE28A_A001,
        arm_block(0x0B, 13, 0x8070),
    ]);
    assert_eq!(routine.here(), RETURN_TABLE);
    routine.arm(&[0x1001, 0x2002, RETURN_CONT1]);
    assert_eq!(routine.here(), RETURN_TABLE2);
    routine.arm(&[0x0303, RETURN_CONT2]);
    assert_eq!(routine.here(), RETURN_LITERAL);
    routine.arm(&[RETURN_ROM_ARM]);
    gba_block_setup(&mut rom, &routine);

    let mut rom_arm = Code::new(RETURN_ROM_ARM);
    rom_arm.arm(&[arm_block(0x12, 13, 0x4070)]);
    let from = rom_arm.here();
    rom_arm.arm(&[
        arm_bl(from, RETURN_ROM_LEAF),
        0xE28A_A002,
        arm_block(0x0B, 13, 0x8070),
    ]);
    assert_eq!(rom_arm.here(), RETURN_ROM_LEAF);
    rom_arm.arm(&[
        arm_block(0x12, 13, 0x4070),
        0xE28A_A004,
        arm_block(0x0B, 13, 0x8070),
    ]);
    assert!(rom_arm.here() <= GBA_ROM + RETURN_ROM_SIZE as u32);
    rom_arm.place(&mut rom, RETURN_ROM_ARM);

    let pool = [RETURN_WAITS, BLOCK_ROUTINE];
    let literal = |value: u32| {
        let index = pool.iter().position(|&word| word == value).expect("pooled");
        RETURN_POOL + 4 * index as u32
    };
    let mut main = Code::new(BLOCK_MAIN);
    let top = main.here();
    main.thumb(&[0x2003, 0x4038, 0x0040]);
    main.ldr_pc(1, literal(RETURN_WAITS));
    main.thumb(&[0x5A09, 0x4658, 0x8081]);
    main.ldr_pc(3, literal(BLOCK_ROUTINE));
    let veneer_call = main.halves.len();
    main.thumb(&[0, 0, 0x4456]);
    let function_call = main.halves.len();
    main.thumb(&[0, 0]);
    let even = (main.here() + 6).next_multiple_of(4);
    main.adr(0, even);
    main.thumb(&[thumb_push(0x01), thumb_pop_pc(0x00)]);
    if main.here() < even {
        main.thumb(&[0x46C0]);
    }
    assert_eq!(main.here(), even);
    main.thumb(&[0x3701, 0x203F, 0x4038]);
    let waited = main.here() + 4;
    main.bne(waited);
    main.thumb(&[thumb_swi(5)]);
    assert_eq!(main.here(), waited);
    main.b(top);
    let veneer = main.here();
    main.thumb(&[0x4718]);
    let function = main.here();
    main.thumb(&[thumb_push(0x110), 0x1C74, 0x1C26, thumb_pop_pc(0x10)]);
    for (call, to) in [(veneer_call, veneer), (function_call, function)] {
        let from = BLOCK_MAIN + 2 * call as u32;
        main.halves[call..call + 2].copy_from_slice(&thumb_bl(from, to));
    }
    assert!(main.here() <= RETURN_POOL);
    main.place(&mut rom, BLOCK_MAIN);

    let mut data = Code::new(RETURN_POOL);
    data.arm(&pool);
    assert!(data.here() <= RETURN_WAITS);
    data.place(&mut rom, RETURN_POOL);
    let mut waits = Code::new(RETURN_WAITS);
    waits.thumb(&BLOCK_WAITCNT);
    assert!(waits.here() <= RETURN_ROM_ARM);
    waits.place(&mut rom, RETURN_WAITS);
    rom
}

#[test]
fn gba_block_return_encodings_match_hand_encoded_words() {
    assert_eq!(arm_block(0x12, 13, 0x4070), 0xE92D_4070);
    assert_eq!(arm_block(0x0B, 13, 0x4070), 0xE8BD_4070);
    assert_eq!(arm_block(0x0B, 13, 0x8070), 0xE8BD_8070);
    assert_eq!(arm_block(0x0B, 13, 0x8010), 0xE8BD_8010);
    assert_eq!(arm_block(0x0F, 13, 0x8010), 0xE8FD_8010);
    assert_eq!(arm_block(0x0B, 0, 0x8003), 0xE8B0_8003);
    assert_eq!(arm_block(0x09, 0, 0x8001), 0xE890_8001);
    assert_eq!(arm_block(0x12, 13, 0x0002), 0xE92D_0002);
    assert_eq!(arm_block(0x12, 13, 0x0010), 0xE92D_0010);
    assert_eq!(
        (arm_block(0x0B, 13, 0x8070) & 0x0FFF_FFFF) | 0x1000_0000,
        0x18BD_8070
    );
    assert_eq!(arm_block(0x0B, 13, 0x8010) & 0x0FFF_FFFF, 0x08BD_8010);
    assert_eq!(thumb_push(0x110), 0xB510);
    assert_eq!(thumb_push(0x01), 0xB401);
    assert_eq!(thumb_pop_pc(0x10), 0xBD10);
    assert_eq!(thumb_pop_pc(0x00), 0xBD00);
    assert_eq!(thumb_swi(5), 0xDF05);
    assert_eq!(arm_bl(BLOCK_ROUTINE + 0x04, RETURN_INNER), 0xEB00_0021);
    assert_eq!(arm_bl(RETURN_ROM_ARM + 0x04, RETURN_ROM_LEAF), 0xEB00_0001);
    assert_eq!(
        arm_ldr_pc(BLOCK_ROUTINE + 0x08, 0, RETURN_LITERAL),
        0xE59F_00A0
    );
    let rom = gba_block_return_rom();
    let word = |at: u32| {
        let at = (at - GBA_ROM) as usize;
        u32::from_le_bytes(rom[at..at + 4].try_into().expect("four bytes"))
    };
    let routine = |at: u32| word(BLOCK_ROUTINE_SOURCE + at);
    let half = |at: u32| {
        let at = (at - GBA_ROM) as usize;
        u16::from_le_bytes([rom[at], rom[at + 1]])
    };
    for (from, rd, to, encoded) in [
        (0x14, 0, RETURN_TABLE, 0xE28F_0080),
        (0x20, 0, RETURN_TABLE2, 0xE28F_0080),
        (0x34, 1, RETURN_CONT3, 0xE28F_1008),
        (0x48, 9, RETURN_AFTER_ODD, 0xE28F_9014),
        (0x4C, 1, RETURN_ODD | 1, 0xE28F_1009),
        (0x64, 14, RETURN_AFTER_BIOS, 0xE28F_E00C),
        (0x78, 1, RETURN_AFTER_S, 0xE28F_1008),
    ] {
        assert_eq!(arm_adr(BLOCK_ROUTINE + from, rd, to), encoded, "{from:#x}");
        assert_eq!(routine(from), encoded, "{from:#x}");
    }
    for (at, encoded) in [
        (0x00, 0xE92D_4070),
        (0x04, 0xEB00_0021),
        (0x08, 0xE59F_00A0),
        (0x0C, 0xE1A0_E00F),
        (0x10, 0xE12F_FF10),
        (0x18, 0xE8B0_8003),
        (0x1C, 0xE08A_A000),
        (0x24, 0xE890_8001),
        (0x2C, 0xE156_0006),
        (0x30, 0x18BD_8070),
        (0x40, 0x08BD_8010),
        (0x44, 0xE28A_A003),
        (0x58, 0xE8BD_8010),
        (0x5C, 0xE28A_A005),
        (0x60, 0xE12F_FF19),
        (0x68, 0xE3A0_1FA7),
        (0x84, 0xE8FD_8010),
        (0x88, 0xE8BD_4070),
        (0x8C, 0xE12F_FF1E),
        (0x98, 0xE8BD_8070),
        (0xA4, RETURN_CONT1),
        (0xAC, RETURN_CONT2),
        (0xB0, RETURN_ROM_ARM),
    ] {
        assert_eq!(routine(at), encoded, "{at:#x}");
    }
    let mov = routine(0x68);
    assert_eq!(
        (mov & 0xFF).rotate_right(2 * ((mov >> 8) & 0xF)),
        RETURN_BIOS
    );
    assert_eq!(word(RETURN_ROM_ARM + 0x04), 0xEB00_0001);
    assert_eq!(word(RETURN_ROM_LEAF + 0x08), 0xE8BD_8070);
    assert_eq!(word(BLOCK_SETUP + 4 * 31), 0xE3A0_1000 | 45);
    assert_eq!(thumb_ldr_pc(0x0800_0406, 1, RETURN_POOL), 0x497E);
    assert_eq!(thumb_ldr_pc(0x0800_040E, 3, RETURN_POOL + 4), 0x4B7D);
    assert_eq!(thumb_bl(0x0800_0410, 0x0800_042C), [0xF000, 0xF80C]);
    assert_eq!(thumb_bl(0x0800_0416, 0x0800_042E), [0xF000, 0xF80A]);
    assert_eq!(thumb_adr(0x0800_041A, 0, 0x0800_0420), 0xA001);
    assert_eq!(thumb_bne(0x0800_0426, 0x0800_042A), 0xD100);
    assert_eq!(thumb_b(0x0800_042A, BLOCK_MAIN), 0xE7E9);
    for (at, encoded) in [
        (0x0800_0406, 0x497E),
        (0x0800_040E, 0x4B7D),
        (0x0800_0410, 0xF000),
        (0x0800_0412, 0xF80C),
        (0x0800_0414, 0x4456),
        (0x0800_0416, 0xF000),
        (0x0800_0418, 0xF80A),
        (0x0800_041A, 0xA001),
        (0x0800_041C, 0xB401),
        (0x0800_041E, 0xBD00),
        (0x0800_0420, 0x3701),
        (0x0800_0426, 0xD100),
        (0x0800_0428, 0xDF05),
        (0x0800_042A, 0xE7E9),
        (0x0800_042C, 0x4718),
        (0x0800_042E, 0xB510),
        (0x0800_0434, 0xBD10),
    ] {
        assert_eq!(half(at), encoded, "{at:#010x}");
    }
    assert_eq!(word(RETURN_POOL), RETURN_WAITS);
    assert_eq!(word(RETURN_POOL + 4), BLOCK_ROUTINE);
    let bios = &citrine_gb::gba::bios::CULT_OF_GBA[RETURN_BIOS as usize..][..4];
    assert_eq!(
        u32::from_le_bytes(bios.try_into().expect("four bytes")),
        0xE12F_FF1E
    );
}

#[test]
fn gba_block_returns_run_the_same_with_the_jit() {
    let case = Case {
        name: "gba-block-returns".into(),
        rom: gba_block_return_rom(),
        frames: 2 * STATE_EVERY + 1,
        press: Vec::new(),
    };
    check::<Gba>(std::slice::from_ref(&case));
    let mut gba = <Gba as Machine>::boot(&case.rom, false);
    for _ in 0..case.frames {
        Machine::run_frame(&mut gba);
    }
    let passes = gba.cpu().reg(7);
    assert!(
        passes > 8 * 64,
        "the return loop ran {passes} passes, its vblank waits stall"
    );
    let routines = gba.cpu().reg(10) / RETURN_PASS_SUM;
    assert!(
        (passes..=passes + 1).contains(&routines),
        "{routines} routine runs for {passes} passes"
    );
    older_state::<Gba>(case);
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
