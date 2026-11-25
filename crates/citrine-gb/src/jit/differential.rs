use super::native::{HOST_TIMED, Native};
use super::{CodeRam, CodeSpan, Entry, Fast, FastBus, Jit, JitBus, Ran, Spec, WINDOWS, key};
use crate::gba::cpu::{Core, CpuBus, CpuState};
use crate::gba::wait::{Access, Width};
use crate::nds::bus::{Arm7Bus, Buses};

const SIZE: usize = 0x4000;
const CODE_AT: usize = 0x1000;
const CODE_END: usize = 0x2000;
const BASE: u32 = 0x0200_0000;
const CODE: u32 = BASE + CODE_AT as u32;
const EXIT_AT: u32 = 0x400;
const BUDGET: u32 = 4096;
const STATES: usize = 6;
const PROGRAMS: usize = 600;
const SHOWN: usize = 12;
const MODES: [u32; 7] = [0x10, 0x11, 0x12, 0x13, 0x17, 0x1B, 0x1F];
const EDGES: [u32; 5] = [0, 1, u32::MAX, 0x8000_0000, 0x7FFF_FFFF];
const PSR_BITS: [(u32, &str); 8] = [
    (31, "N"),
    (30, "Z"),
    (29, "C"),
    (28, "V"),
    (27, "Q"),
    (7, "I"),
    (6, "F"),
    (5, "T"),
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }

    fn below(&mut self, count: u32) -> u32 {
        self.next() % count
    }

    fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }

    fn reg(&mut self) -> u32 {
        self.below(16)
    }

    fn value(&mut self) -> u32 {
        match self.below(10) {
            0..=2 => self.next(),
            3 | 4 => self.below(64),
            5..=8 => 0x0200_0000 | self.below(SIZE as u32),
            _ => EDGES[self.below(EDGES.len() as u32) as usize],
        }
    }
}

#[derive(Clone)]
struct Flat<const TIMED: bool> {
    memory: Vec<u8>,
    open: u32,
    debt: u32,
    fast: Fast,
}

fn index(addr: u32) -> usize {
    addr as usize & (SIZE - 1)
}

impl<const TIMED: bool> Flat<TIMED> {
    fn new(rng: &mut Rng, program: &Program) -> Self {
        let mut memory: Vec<u8> = (0..SIZE).map(|_| rng.next() as u8).collect();
        memory[CODE_AT..CODE_END].fill(0);
        let size = program.size() as usize;
        for (at, &word) in program.words.iter().enumerate() {
            let bytes = word.to_le_bytes();
            memory[CODE_AT + at * size..][..size].copy_from_slice(&bytes[..size]);
        }
        Self {
            memory,
            open: 0,
            debt: 0,
            fast: Fast::CLOSED,
        }
    }

    fn get(&self, addr: u32, width: Width) -> u32 {
        let addr = addr & !(width.bytes() as u32 - 1);
        (0..width.bytes() as u32).fold(0, |value, byte| {
            value | u32::from(self.memory[index(addr + byte)]) << (8 * byte)
        })
    }

    fn put(&mut self, addr: u32, width: Width, value: u32) {
        let addr = addr & !(width.bytes() as u32 - 1);
        for byte in 0..width.bytes() as u32 {
            let at = index(addr + byte);
            if !(CODE_AT..CODE_END).contains(&at) {
                self.memory[at] = (value >> (8 * byte)) as u8;
            }
        }
    }
}

impl<const TIMED: bool> CpuBus for Flat<TIMED> {
    fn fetch(&mut self, addr: u32, width: Width, _: Access) -> u32 {
        self.get(addr, width)
    }

    fn read(&mut self, addr: u32, width: Width, _: Access) -> u32 {
        self.get(addr, width)
    }

    fn write(&mut self, addr: u32, width: Width, value: u32, _: Access) {
        self.put(addr, width, value);
    }

    fn idle(&mut self) {}

    fn cycles(&self, addr: u32, width: Width, access: Access) -> u32 {
        let first = match access {
            Access::NonSequential => 2,
            Access::Sequential => 1,
        };
        first + u32::from(width == Width::Word) + u32::from(addr >> 24 != 0x02)
    }

    fn bus_value(&self) -> u32 {
        self.open
    }

    fn drive(&mut self, value: u32) {
        self.open = value;
    }
}

impl<const TIMED: bool> JitBus for Flat<TIMED> {
    const TIMED_FETCH: bool = TIMED;
    const WRITABLE: &'static [CodeRam] = &[];

    fn code_layout(&self) -> u64 {
        0
    }

    fn code_span(&self, addr: u32) -> Option<CodeSpan> {
        let at = index(addr);
        (CODE_AT..CODE_END).contains(&at).then(|| CodeSpan {
            ram: CodeRam::Rom,
            offset: (at - CODE_AT) as u32,
        })
    }

    fn code_word(&self, span: CodeSpan) -> u32 {
        self.get(CODE + span.offset, Width::Word)
    }

    fn mark_code(&mut self, _: CodeRam, _: usize) -> u32 {
        0
    }

    fn code_generation(&self, _: CodeRam, _: usize) -> Option<u32> {
        Some(0)
    }

    fn take_code_hit(&mut self) -> bool {
        false
    }

    fn take_stall(&mut self) -> u32 {
        0
    }

    fn code_stamp(&self, _: CodeRam) -> u32 {
        0
    }

    fn written_page(&self, _: CodeRam, _: u32) -> Option<usize> {
        None
    }
}

impl<const TIMED: bool> FastBus for Flat<TIMED> {
    const SPECS: [Spec; WINDOWS] = [Spec {
        len: SIZE as u32,
        code: false,
        cache: None,
        half: [0; 2],
        word: [0; 2],
    }; WINDOWS];

    fn fast(&mut self) -> *mut Fast {
        self.fast = Fast {
            debt: &raw mut self.debt,
            ..Fast::CLOSED
        };
        &raw mut self.fast
    }
}

struct Program {
    words: Vec<u32>,
    thumb: bool,
}

impl Program {
    fn size(&self) -> u32 {
        if self.thumb { 2 } else { 4 }
    }

    fn width(&self) -> Width {
        if self.thumb { Width::Half } else { Width::Word }
    }

    fn random(rng: &mut Rng, thumb: bool, v5: bool) -> Self {
        let count = if rng.chance(40) { 1 } else { 2 + rng.below(5) };
        let words = (0..count)
            .map(|_| {
                if thumb {
                    u32::from(thumb_op(rng))
                } else {
                    arm_op(rng, v5)
                }
            })
            .collect();
        Self::ending(words, thumb)
    }

    fn ending(mut words: Vec<u32>, thumb: bool) -> Self {
        let at = words.len() as u32 * if thumb { 2 } else { 4 };
        words.push(if thumb {
            0xE000 | ((EXIT_AT - at - 4) / 2)
        } else {
            0xEA00_0000 | ((EXIT_AT - at - 8) / 4)
        });
        Self { words, thumb }
    }
}

fn arm_op(rng: &mut Rng, v5: bool) -> u32 {
    let condition = if rng.chance(75) { 0xE } else { rng.below(15) };
    let body = match rng.below(15) {
        0 => 0x0200_0000 | rng.next() & 0x01FF_FFFF,
        1 => rng.next() & 0x01FF_FFEF,
        2 => rng.next() & 0x01FF_FF6F | 0x10,
        3 => rng.next() & 0x003F_FF0F | 0x90,
        4 => rng.next() & 0x007F_FF0F | 0x0080_0090,
        5 | 6 => {
            let word = rng.next() & 0x03FF_FFFF | 0x0400_0000;
            if word & 1 << 25 != 0 {
                word & !0x10
            } else {
                word
            }
        }
        7 => rng.next() & 0x01FF_FF0F | 0x90 | (1 + rng.below(3)) << 5,
        8 => {
            let list = if rng.chance(5) {
                0
            } else {
                rng.next() & rng.next() & 0xFFFF
            };
            rng.next() & 0x01FF_0000 | 0x0800_0000 | list
        }
        9 => 0x0A00_0000 | rng.below(2) << 24 | (rng.below(0x100).wrapping_sub(64) & 0xFF_FFFF),
        10 => 0x012F_FF10 | u32::from(v5 && rng.chance(30)) << 5 | rng.reg(),
        11 => match rng.below(3) {
            0 => 0x010F_0000 | rng.below(2) << 22 | rng.reg() << 12,
            1 => 0x0120_F000 | rng.below(2) << 22 | rng.below(16) << 16 | rng.reg(),
            _ => 0x0320_F000 | rng.below(2) << 22 | rng.below(16) << 16 | rng.below(0x1000),
        },
        12 => match rng.below(3) {
            0 => 0x016F_0F10 | rng.reg() << 12 | rng.reg(),
            1 => 0x0100_0050 | rng.below(4) << 21 | rng.next() & 0x000F_F00F,
            _ => 0x0100_0080 | rng.below(4) << 21 | rng.next() & 0x000F_FF6F,
        },
        13 => 0x0100_0090 | rng.below(2) << 22 | rng.reg() << 16 | rng.reg() << 12 | rng.reg(),
        _ => rng.next() & 0x0FFF_FFFF,
    };
    condition << 28 | body
}

fn thumb_op(rng: &mut Rng) -> u16 {
    let field = rng.next() as u16;
    match rng.below(10) {
        0..=2 => 0x4000 | field & 0x3FF,
        3 => 0x4400 | field & 0x3FF,
        4 => 0xB400 | field & 0x9FF,
        5 => 0xC000 | field & 0xFFF,
        6 => field & 0x1FFF,
        _ => field,
    }
}

fn random_state<const V5: bool>(rng: &mut Rng, thumb: bool, pipeline: [u32; 2]) -> CpuState {
    let flags = if V5 { 0xF800_0000 } else { 0xF000_0000 };
    let mode = |rng: &mut Rng| MODES[rng.below(MODES.len() as u32) as usize];
    let cpsr = rng.next() & (flags | 0xC0) | mode(rng) | u32::from(thumb) << 5;
    let mut state = CpuState {
        cpsr,
        ..CpuState::default()
    };
    for reg in state
        .r
        .iter_mut()
        .chain(&mut state.fiq)
        .chain(&mut state.svc)
        .chain(&mut state.abt)
        .chain(&mut state.irq)
        .chain(&mut state.und)
    {
        *reg = rng.value();
    }
    for spsr in &mut state.spsr {
        *spsr = rng.next() & (flags | 0xE0) | mode(rng);
    }
    state.r[15] = CODE + if thumb { 4 } else { 8 };
    state.pipeline = pipeline;
    state
}

#[derive(Default)]
struct Tally {
    runs: usize,
    native: usize,
    refused: usize,
    failures: Vec<String>,
}

fn differences<const V5: bool>(jit: &Core<V5>, stepped: &Core<V5>) -> String {
    let (a, b) = (jit.state(), stepped.state());
    let mut out = Vec::new();
    for reg in 0..16 {
        if a.r[reg] != b.r[reg] {
            out.push(format!("r{reg} {:08X}/{:08X}", a.r[reg], b.r[reg]));
        }
    }
    if a.cpsr != b.cpsr {
        out.push(format!(
            "cpsr {:08X}/{:08X} ({})",
            a.cpsr,
            b.cpsr,
            psr_bits(a.cpsr ^ b.cpsr)
        ));
    }
    let banks: [(&str, &[u32], &[u32]); 5] = [
        ("fiq", &a.fiq, &b.fiq),
        ("svc", &a.svc, &b.svc),
        ("abt", &a.abt, &b.abt),
        ("irq", &a.irq, &b.irq),
        ("und", &a.und, &b.und),
    ];
    for (name, ours, theirs) in banks {
        for (at, (x, y)) in ours.iter().zip(theirs).enumerate() {
            if x != y {
                out.push(format!("{name} r{} {x:08X}/{y:08X}", 15 - ours.len() + at));
            }
        }
    }
    for (at, (x, y)) in a.spsr.iter().zip(&b.spsr).enumerate() {
        if x != y {
            out.push(format!("spsr{at} {x:08X}/{y:08X} ({})", psr_bits(x ^ y)));
        }
    }
    if a != b && out.is_empty() {
        out.push(format!(
            "pipeline {:08X?}/{:08X?}, next fetch {:?}/{:?}",
            a.pipeline, b.pipeline, a.next_fetch, b.next_fetch
        ));
    }
    if jit.cycles != stepped.cycles {
        out.push(format!("core cycles {}/{}", jit.cycles, stepped.cycles));
    }
    if out.is_empty() && jit != stepped {
        out.push(format!("core {jit:?}/{stepped:?}"));
    }
    out.join(", ")
}

fn psr_bits(changed: u32) -> String {
    let named = PSR_BITS
        .iter()
        .filter(|&&(bit, _)| changed & 1 << bit != 0)
        .map(|&(_, name)| name);
    let mode = (changed & 0x1F != 0).then_some("mode");
    named.chain(mode).collect::<Vec<_>>().join(" ")
}

fn record<const V5: bool>(
    tally: &mut Tally,
    program: &Program,
    state: &CpuState,
    ran: Ran,
    cores: [&Core<V5>; 2],
    memory: [&[u8]; 2],
    cycles: u32,
) {
    let [core, stepped] = cores;
    let at = (0..SIZE).find(|&at| memory[0][at] != memory[1][at]);
    if stepped == core && at.is_none() && cycles == ran.cycles {
        return;
    }
    let mut found = differences(core, stepped);
    if let Some(at) = at {
        found.push_str(&format!(
            ", memory at {at:04X} {:02X}/{:02X}",
            memory[0][at], memory[1][at]
        ));
    }
    if cycles != ran.cycles {
        found.push_str(&format!(", clock {}/{cycles}", ran.cycles));
    }
    tally.failures.push(format!(
        "{:08X?} after {} instructions: {found} (jit/interpreter); start {state:08X?}",
        program.words, ran.retired
    ));
}

fn count(tally: &mut Tally, jit: &Jit, thumb: bool) {
    tally.runs += 1;
    if let Some(Entry::Ready { block, .. }) = jit.entries.get(&key(CODE, thumb)) {
        tally.native += usize::from(block.native);
    }
}

fn check<const V5: bool, const TIMED: bool>(rng: &mut Rng, program: &Program, tally: &mut Tally) {
    let bus = Flat::<TIMED>::new(rng, program);
    let mut jit = Jit {
        native: Native::start::<V5, Flat<TIMED>>(),
        asked: true,
        ..Default::default()
    };
    let width = program.width();
    let pipeline = [bus.get(CODE, width), bus.get(CODE + program.size(), width)];
    for _ in 0..STATES {
        let state = random_state::<V5>(rng, program.thumb, pipeline);
        check_state::<V5, TIMED>(&mut jit, &bus, program, &state, tally);
    }
}

fn check_state<const V5: bool, const TIMED: bool>(
    jit: &mut Jit,
    bus: &Flat<TIMED>,
    program: &Program,
    state: &CpuState,
    tally: &mut Tally,
) {
    let start = Core::<V5>::from_state(state);
    let (mut core, mut jitted) = (start.clone(), bus.clone());
    let mut ran = None;
    for _ in 0..4 {
        (core, jitted) = (start.clone(), bus.clone());
        ran = jit.run(&mut core, &mut jitted, BUDGET);
        if ran.is_some() {
            break;
        }
    }
    let Some(ran) = ran else {
        tally.refused += 1;
        return;
    };
    count(tally, jit, program.thumb);
    let (mut stepped, mut plain) = (start.clone(), bus.clone());
    let mut cycles = 0;
    for _ in 0..ran.retired {
        cycles += stepped.step(&mut plain);
    }
    record(
        tally,
        program,
        state,
        ran,
        [&core, &stepped],
        [&jitted.memory, &plain.memory],
        cycles,
    );
}

fn replay_arm<const V5: bool>(ops: &[u32], cpsr: u32, r: [u32; 16]) -> Tally {
    let program = Program::ending(ops.to_vec(), false);
    let bus = Flat::<false>::new(&mut Rng(1), &program);
    let mut jit = Jit {
        native: Native::start::<V5, Flat<false>>(),
        asked: true,
        ..Default::default()
    };
    let state = CpuState {
        cpsr,
        r,
        pipeline: [bus.get(CODE, Width::Word), bus.get(CODE + 4, Width::Word)],
        ..CpuState::default()
    };
    let mut tally = Tally::default();
    check_state::<V5, false>(&mut jit, &bus, &program, &state, &mut tally);
    tally
}

fn arm7_buses(rng: &mut Rng, program: &Program) -> Buses {
    let mut buses = Buses::default();
    let mut bus = buses.arm7();
    for at in (0..SIZE).step_by(4) {
        let value = if (CODE_AT..CODE_END).contains(&at) {
            0
        } else {
            rng.next()
        };
        bus.write(BASE + at as u32, Width::Word, value);
    }
    for (at, &word) in program.words.iter().enumerate() {
        bus.write(CODE + at as u32 * program.size(), program.width(), word);
    }
    buses
}

fn arm7_memory(buses: &mut Buses) -> Vec<u8> {
    let mut bus = buses.arm7();
    (0..SIZE)
        .step_by(4)
        .flat_map(|at| bus.read(BASE + at as u32, Width::Word).to_le_bytes())
        .collect()
}

fn check_arm7(rng: &mut Rng, program: &Program, tally: &mut Tally) {
    let buses = arm7_buses(rng, program);
    let width = program.width();
    let pipeline = {
        let mut copy = buses.clone();
        let mut bus = copy.arm7();
        [
            bus.read(CODE, width),
            bus.read(CODE + program.size(), width),
        ]
    };
    for _ in 0..STATES {
        let state = random_state::<false>(rng, program.thumb, pipeline);
        let start = Core::<false>::from_state(&state);
        let mut jit = Jit {
            native: Native::start::<false, Arm7Bus<'static>>(),
            asked: true,
            ..Default::default()
        };
        let mut jitted = buses.clone();
        let mut core = start.clone();
        let mut ran = None;
        for _ in 0..4 {
            core = start.clone();
            ran = jit.run(&mut core, &mut jitted.arm7(), BUDGET);
            if ran.is_some() {
                break;
            }
        }
        let Some(ran) = ran else {
            tally.refused += 1;
            continue;
        };
        count(tally, &jit, program.thumb);
        let (mut stepped, mut plain) = (start.clone(), buses.clone());
        let mut cycles = 0;
        let mut bus = plain.arm7();
        for _ in 0..ran.retired {
            cycles += stepped.step(&mut bus);
        }
        record(
            tally,
            program,
            &state,
            ran,
            [&core, &stepped],
            [&arm7_memory(&mut jitted), &arm7_memory(&mut plain)],
            cycles,
        );
    }
}

fn differ(thumb: bool, v5: bool, seed: u64, check: fn(&mut Rng, &Program, &mut Tally)) {
    let programs = std::env::var("CITRINE_DIFF_PROGRAMS")
        .ok()
        .and_then(|count| count.parse().ok())
        .unwrap_or(PROGRAMS);
    let mut rng = Rng(seed);
    let mut tally = Tally::default();
    for _ in 0..programs {
        let program = Program::random(&mut rng, thumb, v5);
        check(&mut rng, &program, &mut tally);
    }
    let shown: Vec<&String> = tally.failures.iter().take(SHOWN).collect();
    assert!(
        tally.failures.is_empty(),
        "{} of {} runs differ ({} native, {} refused):\n{}",
        tally.failures.len(),
        tally.runs,
        tally.native,
        tally.refused,
        shown
            .iter()
            .map(|failure| failure.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(tally.runs > programs, "{} runs", tally.runs);
    assert!(
        tally.native * 4 >= tally.runs * 3,
        "{} of {} runs native",
        tally.native,
        tally.runs
    );
}

#[test]
fn arm7_arm_blocks_match_the_interpreter() {
    differ(false, false, 0x0A4A_0001, check_arm7);
}

#[test]
fn arm7_thumb_blocks_match_the_interpreter() {
    differ(true, false, 0x0A4B_0002, check_arm7);
}

#[test]
fn armv5_arm_blocks_match_the_interpreter() {
    differ(false, true, 0x0A5A_0003, check::<true, false>);
}

#[test]
fn armv5_thumb_blocks_match_the_interpreter() {
    differ(true, true, 0x0A5B_0004, check::<true, false>);
}

#[test]
fn timed_armv4_arm_blocks_match_the_interpreter() {
    if HOST_TIMED {
        differ(false, false, 0x0B4A_0005, check::<false, true>);
    }
}

#[test]
fn timed_armv4_thumb_blocks_match_the_interpreter() {
    if HOST_TIMED {
        differ(true, false, 0x0B4B_0006, check::<false, true>);
    }
}

#[test]
fn a_rotated_immediate_clears_the_carry_an_adcs_reads() {
    let mut r = [0x0200_0100; 16];
    r[0] = 5;
    r[15] = CODE + 8;
    for first in [0xE330_0304, 0xE3F0_2D6B, 0xE3D0_EDE2] {
        let tally = replay_arm::<true>(&[first, 0xE2B0_0000], 0x2000_001F, r);
        assert_eq!(tally.native, 1, "{first:08X} ran native");
        assert!(tally.failures.is_empty(), "{}", tally.failures.join("\n"));
    }
}
