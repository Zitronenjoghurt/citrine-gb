mod archives;

use archives::{Source, cache_dir, fetch, ledger_dir, read_entry, read_manifest, sha256_hex};
use citrine_gb::gba::cpu::{Cpu, CpuBus, CpuState};
use citrine_gb::gba::wait::{Access, Width};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::process::ExitCode;

const SUITE: &str = "arm7tdmi";
const FILE_MAGIC: u32 = 0xD33D_BAE0;
const STATE_WORDS: usize = 40;
const KIND_CODE: u32 = 0;
const KIND_READ: u32 = 1;
const KIND_WRITE: u32 = 2;
const ACCESS_SEQUENTIAL: u32 = 1;
const WRITE_LOGGED_EARLY: u32 = 1;

#[derive(Deserialize)]
struct Manifest {
    cpu: BTreeMap<String, CpuSuite>,
}

#[derive(Deserialize)]
struct CpuSuite {
    url: String,
    sha256: String,
    root: String,
    files: Vec<FileEntry>,
}

#[derive(Deserialize)]
struct FileEntry {
    path: String,
    sha256: String,
}

#[derive(Default)]
struct Args {
    fetch: bool,
    write: bool,
    write_disasm: bool,
    show: usize,
    cycles: usize,
    only: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--fetch" => args.fetch = true,
            "--write" => args.write = true,
            "--write-disasm" => args.write_disasm = true,
            "--show" => {
                let count = iter.next().ok_or("--show needs a count")?;
                args.show = count.parse().map_err(|_| format!("bad count {count}"))?;
            }
            "--cycles" => {
                let count = iter.next().ok_or("--cycles needs a count")?;
                args.cycles = count.parse().map_err(|_| format!("bad count {count}"))?;
            }
            "--only" => args.only = Some(iter.next().ok_or("--only needs a group")?),
            "--nocapture" | "--quiet" | "-q" => {}
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Transaction {
    kind: u32,
    size: u32,
    addr: u32,
    data: u32,
    cycle: u32,
    access: u32,
}

impl std::fmt::Debug for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "kind {} size {} addr {:08x} data {:08x} cycle {} access {:x}",
            self.kind, self.size, self.addr, self.data, self.cycle, self.access
        )
    }
}

struct Case {
    initial: [u32; STATE_WORDS],
    expected: [u32; STATE_WORDS],
    transactions: Vec<Transaction>,
    opcode: u32,
    base_addr: u32,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn word(&self, at: usize) -> Result<u32, String> {
        let bytes = self
            .data
            .get(at..at + 4)
            .ok_or_else(|| format!("file ends at byte {at}"))?;
        Ok(u32::from_le_bytes(bytes.try_into().expect("four bytes")))
    }

    fn state(&self, at: usize) -> Result<[u32; STATE_WORDS], String> {
        let mut words = [0; STATE_WORDS];
        for (i, word) in words.iter_mut().enumerate() {
            *word = self.word(at + 8 + i * 4)?;
        }
        Ok(words)
    }

    fn case(&mut self) -> Result<Case, String> {
        let start = self.at;
        let total = self.word(start)? as usize;
        let mut at = start + 4;
        let initial = self.state(at)?;
        at += self.word(at)? as usize;
        let expected = self.state(at)?;
        at += self.word(at)? as usize;
        let count = self.word(at + 8)? as usize;
        let mut transactions = Vec::with_capacity(count);
        for i in 0..count {
            let base = at + 12 + i * 24;
            transactions.push(Transaction {
                kind: self.word(base)?,
                size: self.word(base + 4)?,
                addr: self.word(base + 8)?,
                data: self.word(base + 12)?,
                cycle: self.word(base + 16)?,
                access: self.word(base + 20)?,
            });
        }
        at += self.word(at)? as usize;
        let opcode = self.word(at + 8)?;
        let base_addr = self.word(at + 12)?;
        self.at = start + total;
        Ok(Case {
            initial,
            expected,
            transactions,
            opcode,
            base_addr,
        })
    }
}

fn parse(data: &[u8]) -> Result<Vec<Case>, String> {
    let mut reader = Reader { data, at: 0 };
    if reader.word(0)? != FILE_MAGIC {
        return Err("not a SingleStepTests file".into());
    }
    let count = reader.word(4)? as usize;
    reader.at = 8;
    (0..count).map(|_| reader.case()).collect()
}

fn to_state(words: &[u32; STATE_WORDS]) -> CpuState {
    let mut state = CpuState::default();
    state.r.copy_from_slice(&words[0..16]);
    state.fiq.copy_from_slice(&words[16..23]);
    state.svc.copy_from_slice(&words[23..25]);
    state.abt.copy_from_slice(&words[25..27]);
    state.irq.copy_from_slice(&words[27..29]);
    state.und.copy_from_slice(&words[29..31]);
    state.cpsr = words[31];
    state.spsr.copy_from_slice(&words[32..37]);
    state.pipeline.copy_from_slice(&words[37..39]);
    state.next_fetch = access_of(words[39]);
    state
}

fn access_of(bits: u32) -> Access {
    if bits & ACCESS_SEQUENTIAL != 0 {
        Access::Sequential
    } else {
        Access::NonSequential
    }
}

fn access_bits(access: Access) -> u32 {
    match access {
        Access::Sequential => ACCESS_SEQUENTIAL,
        Access::NonSequential => 0,
    }
}

fn mask_of(width: Width) -> u32 {
    match width {
        Width::Byte => 0xFF,
        Width::Half => 0xFFFF,
        Width::Word => 0xFFFF_FFFF,
    }
}

fn size_of(width: Width) -> u32 {
    width.bytes() as u32
}

struct TestBus<'a> {
    case: &'a Case,
    log: Vec<Transaction>,
    cycle: u32,
    open_bus: u32,
}

impl TestBus<'_> {
    fn record(&mut self, kind: u32, addr: u32, width: Width, data: u32, access: Access) {
        self.log.push(Transaction {
            kind,
            size: size_of(width),
            addr,
            data,
            cycle: self.cycle,
            access: access_bits(access),
        });
        self.cycle += 1;
    }
}

impl CpuBus for TestBus<'_> {
    fn fetch(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        let data = if addr == self.case.base_addr {
            self.case.opcode
        } else {
            addr & mask_of(width)
        };
        self.record(KIND_CODE, addr, width, data, access);
        data
    }

    fn read(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        let data = self
            .case
            .transactions
            .iter()
            .find(|t| t.kind == KIND_READ && t.addr == addr && t.size == size_of(width))
            .map_or(0, |t| t.data);
        self.record(KIND_READ, addr, width, data, access);
        data
    }

    fn write(&mut self, addr: u32, width: Width, value: u32, access: Access) {
        self.record(KIND_WRITE, addr, width, value, access);
    }

    fn idle(&mut self) {
        self.cycle += 1;
    }

    fn cycles(&self, _addr: u32, _width: Width, _access: Access) -> u32 {
        1
    }

    fn bus_value(&self) -> u32 {
        self.open_bus
    }

    fn drive(&mut self, value: u32) {
        self.open_bus = value;
    }
}

#[derive(Default)]
struct Tally {
    total: usize,
    passed: usize,
    cycles_match: usize,
    cycles_raw: usize,
    failures: BTreeMap<&'static str, usize>,
}

struct Outcome {
    wrong: Vec<&'static str>,
    cycles_match: bool,
    cycles_raw: bool,
    detail: String,
}

fn run_case(case: &Case) -> Outcome {
    let mut cpu = Cpu::from_state(&to_state(&case.initial));
    let mut bus = TestBus {
        case,
        log: Vec::new(),
        cycle: 1,
        open_bus: 0,
    };
    cpu.step(&mut bus);
    let actual = cpu.state();
    let expected = to_state(&case.expected);

    let mut wrong = Vec::new();
    if actual.r != expected.r {
        wrong.push("registers");
    }
    if (actual.fiq, actual.svc, actual.abt, actual.irq, actual.und)
        != (
            expected.fiq,
            expected.svc,
            expected.abt,
            expected.irq,
            expected.und,
        )
    {
        wrong.push("banked registers");
    }
    if actual.cpsr != expected.cpsr {
        wrong.push("cpsr");
    }
    if actual.spsr != expected.spsr {
        wrong.push("spsr");
    }
    if actual.pipeline != expected.pipeline {
        wrong.push("pipeline");
    }
    let same_access = |a: &Transaction, b: &Transaction| {
        (a.kind, a.size, a.addr, a.data) == (b.kind, b.size, b.addr, b.data)
    };
    let bus_ok = bus.log.len() == case.transactions.len()
        && bus
            .log
            .iter()
            .zip(&case.transactions)
            .all(|(a, b)| same_access(a, b));
    if !bus_ok {
        wrong.push("bus accesses");
    } else {
        let sequence_ok = bus
            .log
            .iter()
            .zip(&case.transactions)
            .all(|(a, b)| a.access & ACCESS_SEQUENTIAL == b.access & ACCESS_SEQUENTIAL)
            && actual.next_fetch == expected.next_fetch;
        if !sequence_ok {
            wrong.push("sequential flags");
        }
    }
    let cycles_on = |shift: u32| {
        bus_ok
            && bus
                .log
                .iter()
                .zip(&case.transactions)
                .all(|(a, b)| a.cycle == b.cycle + shift * u32::from(b.kind == KIND_WRITE))
    };
    let cycles_raw = cycles_on(0);
    let cycles_match = cycles_on(WRITE_LOGGED_EARLY);

    let mut detail = String::new();
    if !wrong.is_empty() || !cycles_match {
        use std::fmt::Write as _;
        let _ = writeln!(
            detail,
            "  opcode {:08x} at {:08x}: {}",
            case.opcode,
            case.base_addr,
            if wrong.is_empty() {
                "cycles".to_string()
            } else {
                wrong.join(", ")
            }
        );
        let before = &case.initial;
        let _ = writeln!(
            detail,
            "    cpsr {:08x} -> expected {:08x} actual {:08x}",
            before[31], expected.cpsr, actual.cpsr
        );
        let mut actual_words = case.expected;
        actual_words[0..16].copy_from_slice(&actual.r);
        actual_words[16..23].copy_from_slice(&actual.fiq);
        actual_words[23..25].copy_from_slice(&actual.svc);
        actual_words[25..27].copy_from_slice(&actual.abt);
        actual_words[27..29].copy_from_slice(&actual.irq);
        actual_words[29..31].copy_from_slice(&actual.und);
        actual_words[31] = actual.cpsr;
        actual_words[32..37].copy_from_slice(&actual.spsr);
        actual_words[37..39].copy_from_slice(&actual.pipeline);
        for i in 0..39 {
            if actual_words[i] != case.expected[i] || (i < 16 && before[i] != case.expected[i]) {
                let _ = writeln!(
                    detail,
                    "    word {i}: before {:08x} expected {:08x} actual {:08x}",
                    before[i], case.expected[i], actual_words[i]
                );
            }
        }
        if !bus_ok || !cycles_match {
            for t in &case.transactions {
                let _ = writeln!(detail, "    expected {t:?}");
            }
            for t in &bus.log {
                let _ = writeln!(detail, "    actual   {t:?}");
            }
        }
    }
    Outcome {
        wrong,
        cycles_match,
        cycles_raw,
        detail,
    }
}

fn disasm_line(group: &str, case: &Case) -> (u32, String) {
    let cpu = Cpu::from_state(&to_state(&case.initial));
    let op = cpu.pipeline()[0];
    let text = cpu.instruction(false).text();
    if cpu.thumb() {
        let op = op & 0xFFFF;
        (op >> 6, format!("{group} {:08X} {op:04X} {text}", cpu.pc()))
    } else {
        let key = (op >> 16) & 0xFF0 | (op >> 4) & 0xF;
        (key, format!("{group} {:08X} {op:08X} {text}", cpu.pc()))
    }
}

fn check_disasm(lines: &[String], write: bool) -> Result<bool, String> {
    let path = ledger_dir().join("arm7tdmi-disasm.txt");
    let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    if write {
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("arm7tdmi: {} disassembly lines written", lines.len());
        return Ok(true);
    }
    let expected =
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let expected: Vec<&str> = expected.lines().collect();
    let wrong: Vec<String> = lines
        .iter()
        .zip(&expected)
        .filter(|(actual, expected)| actual != *expected)
        .take(10)
        .map(|(actual, expected)| format!("  expected {expected}\n  got      {actual}"))
        .collect();
    if wrong.is_empty() && lines.len() == expected.len() {
        println!("arm7tdmi: {} disassembly lines match", lines.len());
        return Ok(true);
    }
    println!(
        "arm7tdmi: disassembly differs ({} lines, {} expected)",
        lines.len(),
        expected.len()
    );
    for line in wrong {
        println!("{line}");
    }
    Ok(false)
}

fn group_name(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(".json.bin").unwrap_or(file)
}

fn read_baseline() -> Result<BTreeMap<String, usize>, String> {
    let path = ledger_dir().join("arm7tdmi.txt");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (group, count) = line
                .split_once(' ')
                .ok_or_else(|| format!("bad baseline line {line}"))?;
            let count = count
                .trim()
                .parse()
                .map_err(|_| format!("bad baseline line {line}"))?;
            Ok((group.to_string(), count))
        })
        .collect()
}

fn write_baseline(results: &BTreeMap<String, Tally>) -> Result<(), String> {
    let path = ledger_dir().join("arm7tdmi.txt");
    let text: String = results
        .iter()
        .map(|(group, tally)| format!("{group} {}\n", tally.passed))
        .collect();
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn run() -> Result<bool, String> {
    let args = parse_args()?;
    let manifest: Manifest = read_manifest()?;
    let suite = manifest
        .cpu
        .get(SUITE)
        .ok_or("the manifest has no [cpu.arm7tdmi]")?;
    let cached = cache_dir().join(format!("{}.zip", suite.sha256));
    if !args.fetch && !cached.is_file() {
        println!(
            "arm7tdmi: skipped, {} is not in the ledger cache; run with --fetch to download it",
            suite.url
        );
        return Ok(true);
    }
    let source = Source {
        url: suite.url.clone(),
        sha256: suite.sha256.clone(),
    };
    let mut archive = fetch(SUITE, &source)?;

    let mut results = BTreeMap::new();
    let mut disasm = Vec::new();
    for file in &suite.files {
        let group = group_name(&file.path);
        if args.only.as_deref().is_some_and(|only| only != group) {
            continue;
        }
        let data = read_entry(&mut archive, &format!("{}/{}", suite.root, file.path))?;
        let actual = sha256_hex(&data);
        if actual != file.sha256 {
            return Err(format!(
                "{} has SHA-256 {actual}, the manifest pins {}",
                file.path, file.sha256
            ));
        }
        if !file.path.ends_with(".json.bin") {
            continue;
        }
        let mut tally = Tally::default();
        let mut shown = 0;
        let mut shown_cycles = 0;
        let mut patterns = BTreeMap::new();
        for case in parse(&data).map_err(|e| format!("{}: {e}", file.path))? {
            let (key, line) = disasm_line(group, &case);
            patterns.entry(key).or_insert(line);
            let Outcome {
                wrong,
                cycles_match,
                cycles_raw,
                detail,
            } = run_case(&case);
            tally.total += 1;
            tally.cycles_match += usize::from(cycles_match);
            tally.cycles_raw += usize::from(cycles_raw);
            if wrong.is_empty() {
                tally.passed += 1;
                if !cycles_match && shown_cycles < args.cycles {
                    shown_cycles += 1;
                    print!("{detail}");
                }
            } else {
                for reason in wrong {
                    *tally.failures.entry(reason).or_default() += 1;
                }
                if shown < args.show {
                    shown += 1;
                    print!("{detail}");
                }
            }
        }
        let reasons: Vec<String> = tally
            .failures
            .iter()
            .map(|(reason, count)| format!("{reason} {count}"))
            .collect();
        println!(
            "{group}: {}/{} passed, cycles match in {} ({} as logged){}",
            tally.passed,
            tally.total,
            tally.cycles_match,
            tally.cycles_raw,
            if reasons.is_empty() {
                String::new()
            } else {
                format!(" (wrong: {})", reasons.join(", "))
            }
        );
        results.insert(group.to_string(), tally);
        disasm.extend(patterns.into_values());
    }

    if args.only.is_some() {
        return Ok(true);
    }
    let disasm_ok = check_disasm(&disasm, args.write_disasm)?;
    if args.write {
        write_baseline(&results)?;
        println!("arm7tdmi: baseline written");
        return Ok(true);
    }
    let baseline = read_baseline()?;
    let mut ok = true;
    for (group, tally) in &results {
        let floor = baseline.get(group).copied().unwrap_or(0);
        if tally.passed < floor {
            println!(
                "arm7tdmi: {group} fell to {} from its baseline {floor}",
                tally.passed
            );
            ok = false;
        }
    }
    let passed: usize = results.values().map(|t| t.passed).sum();
    let total: usize = results.values().map(|t| t.total).sum();
    println!("arm7tdmi: {passed}/{total} passed");
    Ok(ok && disasm_ok)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("arm7tdmi: {e}");
            ExitCode::FAILURE
        }
    }
}
