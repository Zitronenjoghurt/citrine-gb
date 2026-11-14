mod arena;
#[cfg(test)]
mod differential;
mod disk;
mod emit;
mod fast;
mod helpers;
#[cfg(feature = "jit-lockstep")]
pub mod lockstep;
mod lookup;
mod marks;
mod native;
mod scan;
mod worker;

#[cfg(test)]
pub use fast::{CODE_CYCLES, HALF_CYCLES, MAIN, SEQUENTIAL_CYCLES, WORD_CYCLES};
pub use fast::{Cache, Fast, FastBus, Kept, PAGE_SIZE, Page, Pages, Spec, Stamp, WINDOWS, Window};
pub use marks::{CodeMarks, PAGE_BITS, UNMARKED_PAGES};

use crate::gba::cpu::psr::{FLAG_I, FLAG_T};
use crate::gba::cpu::{CachedWord, Core, CpuBus, Refetch};
use crate::gba::wait::{Access, Width};
use crate::jit_cache::JitCache;
use arena::{Arena, Code};
use citrine_gb_threads::Spawner;
use emit::Emitter;
use helpers::EXIT;
use lookup::Lookup;
use native::Native;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;
use worker::{Done, Heat, Lane, Outcome, Scan};

const HOT: u8 = if cfg!(feature = "jit-lockstep") { 0 } else { 2 };
const MAX_INSTRUCTIONS: usize = 128;
const BLOCK_BYTES: u32 = 1 << marks::PAGE_BITS;
const ARENA_LIMIT: usize = 64 << 20;
const CODE_RAMS: usize = 6;

static EPOCHS: AtomicU32 = AtomicU32::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeRam {
    Main,
    Itcm,
    Bios,
    Wram,
    SharedWram,
    Rom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeSpan {
    pub ram: CodeRam,
    pub offset: u32,
}

impl CodeSpan {
    fn page(self) -> usize {
        self.offset as usize >> marks::PAGE_BITS
    }
}

pub trait JitBus: CpuBus + FastBus {
    const TIMED_FETCH: bool = false;
    const CP15: bool = false;
    const WRITABLE: &'static [CodeRam];

    fn leaves(addr: u32, _store: bool) -> bool {
        helpers::leaves_memory(addr)
    }

    fn flat_code(_pc: u32) -> bool {
        false
    }

    fn code_layout(&self) -> u64;
    fn code_span(&self, addr: u32) -> Option<CodeSpan>;
    fn code_word(&self, span: CodeSpan) -> u32;
    fn mark_code(&mut self, ram: CodeRam, page: usize) -> u32;
    fn code_generation(&self, ram: CodeRam, page: usize) -> Option<u32>;
    fn take_code_hit(&mut self) -> bool;
    fn take_stall(&mut self) -> u32;
    fn stall_flag(&self) -> *const u32 {
        std::ptr::null()
    }
    fn cp15_cells(&mut self) -> Cp15Cells {
        Cp15Cells::NONE
    }
    fn code_stamp(&self, ram: CodeRam) -> u32;
    fn written_page(&self, ram: CodeRam, stamp: u32) -> Option<usize>;

    fn runs_on(&self, _masked: bool) -> bool {
        false
    }

    fn fetch_flat(&mut self, _addr: u32, _width: Width, _count: u32, _room: u32) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cp15Cells {
    pub attention: *mut bool,
    pub pending: *mut bool,
    pub dtcm: *const u32,
}

impl Cp15Cells {
    pub const NONE: Self = Self {
        attention: std::ptr::null_mut(),
        pending: std::ptr::null_mut(),
        dtcm: std::ptr::null(),
    };
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JitStats {
    pub runs: u64,
    pub retired: u64,
    pub compiles: u64,
    pub thumb_compiles: u64,
    pub refused: u64,
    pub stale: u64,
    pub queued: u64,
    pub dropped: u64,
    pub flushes: u64,
    pub misses: u64,
    pub stepped: u64,
    pub compile_nanos: u64,
    pub held_nanos: u64,
    pub loaded: u64,
    pub load_nanos: u64,
    pub mismatched: u64,
    pub lookups: u64,
    pub forgotten: u64,
    pub near: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ran {
    pub cycles: u32,
    pub retired: u32,
    pub last: u32,
}

struct Compiled {
    code: Code,
    native: bool,
    thumb: bool,
    words: Box<[u32]>,
    spans: Box<[CodeSpan]>,
    pages: Box<[(CodeRam, usize)]>,
}

impl Compiled {
    fn enters(&self, start: u32, pipeline: [u32; 2]) -> Option<(usize, u32, *const u32)> {
        let at = start as usize;
        let entry = if self.native {
            (self.code.entry() + 4) | 1
        } else {
            self.code.entry()
        };
        (self.words[at..at + 2] == pipeline).then_some((entry, start, self.words.as_ptr()))
    }
}

#[derive(Clone)]
enum Entry {
    Cold(u8),
    Never(u32),
    Queued(Heat),
    Ready {
        block: Arc<Compiled>,
        start: u32,
        epoch: u32,
        gens: Box<[u32]>,
    },
}

#[derive(Default)]
struct PcHasher(u64);

impl Hasher for PcHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(byte)).wrapping_mul(0x517C_C1B7_2722_0A95);
        }
    }

    fn write_u32(&mut self, value: u32) {
        self.0 = u64::from(value).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type Table = HashMap<u32, Entry, BuildHasherDefault<PcHasher>>;

pub struct Jit {
    entries: Table,
    arena: Arena,
    lane: Option<Lane>,
    emitter: Option<Emitter>,
    inflight: usize,
    scans: u64,
    tag: u32,
    layout: Option<u64>,
    epoch: u32,
    broken: bool,
    stats: JitStats,
    disk: Option<Arc<JitCache>>,
    lookup: Lookup,
    seen: [u32; CODE_RAMS],
    native: Option<Native>,
    asked: bool,
}

impl Default for Jit {
    fn default() -> Self {
        Self {
            entries: Table::default(),
            arena: Arena::default(),
            lane: None,
            emitter: None,
            inflight: 0,
            scans: 0,
            tag: 0,
            layout: None,
            epoch: next_epoch(),
            broken: false,
            stats: JitStats::default(),
            disk: None,
            lookup: Lookup::default(),
            seen: [0; CODE_RAMS],
            native: None,
            asked: false,
        }
    }
}

impl Jit {
    pub fn fork(&self) -> Self {
        self.arena.seal();
        let mut entries = self.entries.clone();
        for entry in entries.values_mut() {
            if matches!(entry, Entry::Queued(_)) {
                *entry = Entry::Cold(HOT);
            }
        }
        Self {
            entries,
            lane: self.lane.as_ref().map(Lane::fork),
            layout: self.layout,
            broken: self.broken,
            stats: self.stats,
            disk: self.disk.clone(),
            native: self.native.as_ref().map(Native::fork),
            asked: self.asked,
            ..Self::default()
        }
    }

    pub fn restart(&mut self) {
        for entry in self.entries.values_mut() {
            if matches!(entry, Entry::Queued(_)) {
                *entry = Entry::Cold(HOT);
            }
        }
        self.lane = self.lane.as_ref().map(Lane::fork);
        self.inflight = 0;
        self.scans = 0;
        self.seen = [0; CODE_RAMS];
        self.recheck();
    }

    pub fn set_cache(&mut self, cache: Option<Arc<JitCache>>) {
        self.disk = cache;
    }

    pub fn compile_on(&mut self, spawner: &Spawner, workers: usize) {
        if self.lane.is_none() {
            self.lane = Some(Lane::start(spawner, workers));
        }
    }

    pub fn run<const V5: bool, B: JitBus>(
        &mut self,
        core: &mut Core<V5>,
        bus: &mut B,
        budget: u32,
    ) -> Option<Ran> {
        if V5 {
            core.word = CachedWord::NONE;
        }
        if self.broken {
            return None;
        }
        if !self.asked {
            self.asked = true;
            self.native = Native::new::<V5, B>();
        }
        let layout = bus.code_layout();
        if self.layout != Some(layout) {
            self.flush();
            self.layout = Some(layout);
        }
        if self.inflight > 0 {
            let began = Instant::now();
            while self.inflight > 0
                && let Some(done) = self.lane.as_ref().and_then(Lane::try_take)
            {
                self.install(done, bus);
            }
            self.stats.held_nanos += began.elapsed().as_nanos() as u64;
        }
        let mut ran = Ran {
            cycles: 0,
            retired: 0,
            last: 0,
        };
        let mut scans = self.scans;
        let mut runs = 0;
        let mut end = 0;
        bus.take_code_hit();
        self.forget_written(bus);
        let mut fast = bus.fast();
        while ran.cycles < budget || !Self::ends::<V5, B>(core) {
            let ((code, start, words), slot) = match self.find(core) {
                Some(found) => found,
                None => {
                    let Some(found) = self.enter(core, bus) else {
                        break;
                    };
                    if self.scans != scans {
                        scans = self.scans;
                        fast = bus.fast();
                    }
                    (found, 0)
                }
            };
            let left = budget.saturating_sub(ran.cycles);
            let (packed, stall) = match &mut self.native {
                Some(native) if code & 1 != 0 => {
                    let packed = unsafe {
                        native.call(
                            code,
                            core,
                            bus,
                            left,
                            start,
                            fast,
                            words,
                            &mut self.lookup,
                            slot,
                        )
                    };
                    let (stall, chains) = native.settled();
                    runs += u64::from(chains);
                    (packed, stall)
                }
                _ => (
                    unsafe { arena::call(code, core, bus, left, start, fast, words) },
                    0,
                ),
            };
            runs += 1;
            if packed == EXIT {
                let packed = loop {
                    let packed = unsafe { helpers::step::<V5, B>(core, bus) };
                    end = ran.cycles + (packed as u32 & (EXIT as u32 - 1));
                    ran.cycles = end + bus.take_stall();
                    ran.retired += 1;
                    self.stats.near += 1;
                    if !V5
                        || packed & EXIT != 0
                        || ran.cycles >= budget && Self::ends::<V5, B>(core)
                    {
                        break packed;
                    }
                };
                if packed & EXIT != 0 {
                    break;
                }
                continue;
            }
            end = ran.cycles + (packed as u32 & (EXIT as u32 - 1));
            ran.cycles = end + stall + bus.take_stall();
            ran.retired += (packed >> 32) as u32;
            if packed & EXIT != 0 {
                if ran.cycles >= budget && Self::ends::<V5, B>(core) || !self.resumes(core, bus) {
                    break;
                }
                bus.take_code_hit();
                self.forget_written(bus);
                fast = bus.fast();
                if self.find(core).is_none() {
                    break;
                }
            }
        }
        ran.last = end.saturating_sub(core.cycles);
        self.stats.runs += runs;
        self.stats.retired += u64::from(ran.retired);
        (ran.retired > 0).then_some(ran)
    }

    #[inline(always)]
    fn ends<const V5: bool, B: JitBus>(core: &Core<V5>) -> bool {
        B::TIMED_FETCH || core.ends()
    }

    #[inline(always)]
    fn resumes<const V5: bool, B: JitBus>(&self, core: &Core<V5>, bus: &B) -> bool {
        bus.runs_on(core.irq_stays_masked())
            && self.inflight == 0
            && !self.broken
            && self.layout == Some(bus.code_layout())
    }

    #[inline(always)]
    fn find<const V5: bool>(&self, core: &Core<V5>) -> Option<(lookup::Enter, usize)> {
        if core.ldm_glitch {
            return None;
        }
        let thumb = (core.regs.cpsr() & FLAG_T) >> FLAG_T.trailing_zeros();
        let pc = core.regs.r[15].wrapping_sub(8 >> thumb);
        if pc & (3 >> thumb) != 0 {
            return None;
        }
        self.lookup.find_slot(pc | thumb, core.pipeline)
    }

    #[inline(always)]
    fn forget_written<B: JitBus>(&mut self, bus: &B) {
        for &ram in B::WRITABLE {
            let stamp = bus.code_stamp(ram);
            let seen = std::mem::replace(&mut self.seen[ram as usize], stamp);
            let mut at = seen;
            while at != stamp && !self.lookup.is_empty() {
                match bus.written_page(ram, at) {
                    Some(page) => self.stats.forgotten += self.lookup.forget_page(ram, page),
                    None => self.lookup.clear(),
                }
                at = at.wrapping_add(1);
            }
        }
    }

    #[cold]
    #[inline(never)]
    fn enter<const V5: bool, B: JitBus>(
        &mut self,
        core: &Core<V5>,
        bus: &mut B,
    ) -> Option<(usize, u32, *const u32)> {
        self.stats.lookups += 1;
        if core.ldm_glitch {
            return None;
        }
        let thumb = core.regs.cpsr() & FLAG_T != 0;
        let pc = core.regs.r[15].wrapping_sub(2 * size(thumb));
        if pc & (size(thumb) - 1) != 0 {
            return None;
        }
        self.ready::<V5, B>(pc, thumb, core.pipeline, bus)
    }

    pub fn recheck(&mut self) {
        self.epoch = next_epoch();
        self.lookup.clear();
    }

    pub fn stepped(&mut self) {
        self.stats.stepped += 1;
    }

    pub fn stats(&self) -> JitStats {
        self.stats
    }

    pub fn compile_threads(&self) -> usize {
        self.lane.as_ref().map_or(0, Lane::workers)
    }

    fn ready<const V5: bool, B: JitBus>(
        &mut self,
        pc: u32,
        thumb: bool,
        pipeline: [u32; 2],
        bus: &mut B,
    ) -> Option<(usize, u32, *const u32)> {
        let epoch = self.epoch;
        let entry = self
            .entries
            .entry(key(pc, thumb))
            .or_insert(Entry::Cold(HOT));
        match entry {
            Entry::Ready {
                block,
                start,
                epoch: seen,
                gens,
            } => {
                if current(block, seen, gens, epoch, bus) {
                    let code = block.enters(*start, pipeline);
                    if code.is_none() {
                        self.stats.misses += 1;
                    }
                    if let Some(code) = code {
                        self.lookup
                            .fill(key(pc, thumb), pipeline, code, &block.pages);
                    }
                    return code;
                }
                self.stats.stale += 1;
            }
            Entry::Never(word) if *word == pipeline[0] => return None,
            Entry::Never(_) => *entry = Entry::Cold(HOT),
            Entry::Queued(heat) => {
                heat.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            Entry::Cold(left) if *left > 0 => {
                *left -= 1;
                return None;
            }
            Entry::Cold(_) => {}
        }
        let began = Instant::now();
        let code = self.compile::<V5, B>(pc, thumb, pipeline, bus);
        self.stats.held_nanos += began.elapsed().as_nanos() as u64;
        code
    }

    fn compile<const V5: bool, B: JitBus>(
        &mut self,
        pc: u32,
        thumb: bool,
        pipeline: [u32; 2],
        bus: &mut B,
    ) -> Option<(usize, u32, *const u32)> {
        self.lookup.forget(key(pc, thumb));
        let Some(scan) = self.scan::<V5, B>(pc, thumb, bus) else {
            self.stats.refused += 1;
            self.entries
                .insert(key(pc, thumb), Entry::Never(pipeline[0]));
            return None;
        };
        self.scans += 1;
        let heat = Heat::default();
        let count = scan.ops.len() as u32;
        for start in 1..count {
            let inner = pc.wrapping_add(start * size(thumb));
            let inner = self.entries.entry(key(inner, thumb));
            let inner = inner.or_insert(Entry::Cold(HOT));
            if matches!(inner, Entry::Cold(_)) {
                *inner = Entry::Queued(Arc::clone(&heat));
            }
        }
        let job = self.job::<V5, B>(scan);
        self.inflight += 1;
        self.entries
            .insert(key(pc, thumb), Entry::Queued(Arc::clone(&heat)));
        let job = match job {
            Ok(job) => job,
            Err(done) => {
                self.install(*done, bus);
                return self.entered(pc, thumb, pipeline);
            }
        };
        match &self.lane {
            None => {
                let done = worker::compile(&mut self.emitter, job);
                self.install(done, bus);
            }
            Some(lane) => {
                lane.send(job, heat);
                self.stats.queued += 1;
                if !cfg!(feature = "jit-lockstep") {
                    return None;
                }
                while self.inflight > 0 {
                    let Some(done) = self.lane.as_ref().and_then(Lane::take) else {
                        self.broken = true;
                        return None;
                    };
                    self.install(done, bus);
                }
            }
        }
        self.entered(pc, thumb, pipeline)
    }

    fn entered(
        &self,
        pc: u32,
        thumb: bool,
        pipeline: [u32; 2],
    ) -> Option<(usize, u32, *const u32)> {
        match self.entries.get(&key(pc, thumb)) {
            Some(Entry::Ready { block, start, .. }) => block.enters(*start, pipeline),
            _ => None,
        }
    }

    fn install<B: JitBus>(&mut self, done: Done, bus: &mut B) {
        self.inflight -= 1;
        if done.loaded {
            self.stats.load_nanos += done.nanos;
        } else {
            self.stats.compile_nanos += done.nanos;
        }
        self.stats.mismatched += u64::from(done.mismatched);
        if done.tag != self.tag {
            return;
        }
        let Scan {
            pc,
            thumb,
            words,
            spans,
            pages,
            gens,
            ..
        } = done.scan;
        self.lookup.forget(key(pc, thumb));
        let bytes = match done.outcome {
            Outcome::Code(bytes) => bytes,
            Outcome::Refused => {
                self.stats.refused += 1;
                self.entries.insert(key(pc, thumb), Entry::Never(words[0]));
                self.release(pc, thumb, words.len() as u32 - 2);
                return;
            }
            Outcome::Broken => {
                self.broken = true;
                return;
            }
        };
        if self.arena.allocated() > ARENA_LIMIT {
            self.flush();
        }
        let native = native::is_native(&bytes.bytes);
        let prelude = self.native.as_ref().and_then(Native::prelude);
        let Some(code) = self.arena.place(bytes, prelude) else {
            self.broken = true;
            return;
        };
        let block = Arc::new(Compiled {
            code,
            native,
            thumb,
            words,
            spans,
            pages,
        });
        let epoch = self.epoch;
        let mut gens = gens;
        let mut seen = epoch;
        if !current(&block, &mut seen, &mut gens, epoch, bus) {
            self.stats.dropped += 1;
            self.entries.insert(key(pc, thumb), Entry::Cold(HOT));
            self.release(pc, thumb, block.words.len() as u32 - 2);
            return;
        }
        if done.loaded {
            self.stats.loaded += 1;
        } else {
            self.stats.compiles += 1;
            self.stats.thumb_compiles += u64::from(thumb);
        }
        let count = block.words.len() as u32 - 2;
        for start in 1..count {
            let inner = pc.wrapping_add(start * size(thumb));
            let inner = self.entries.entry(key(inner, thumb));
            let inner = inner.or_insert(Entry::Cold(HOT));
            if !matches!(inner, Entry::Ready { .. }) {
                *inner = Entry::Ready {
                    block: Arc::clone(&block),
                    start,
                    epoch,
                    gens: gens.clone(),
                };
            }
        }
        self.entries.insert(
            key(pc, thumb),
            Entry::Ready {
                block,
                start: 0,
                epoch,
                gens,
            },
        );
    }

    fn release(&mut self, pc: u32, thumb: bool, count: u32) {
        for start in 1..count {
            let inner = pc.wrapping_add(start * size(thumb));
            if let Some(inner @ Entry::Queued(_)) = self.entries.get_mut(&key(inner, thumb)) {
                *inner = Entry::Cold(HOT);
            }
        }
    }

    fn scan<const V5: bool, B: JitBus>(&self, pc: u32, thumb: bool, bus: &mut B) -> Option<Scan> {
        let first = bus.code_span(pc)?;
        let size = size(thumb);
        let mut words = Vec::new();
        let mut spans = Vec::new();
        for index in 0..MAX_INSTRUCTIONS as u32 + 2 {
            let addr = pc.wrapping_add(index * size);
            match bus.code_span(addr) {
                Some(span) if span.ram == first.ram => {
                    words.push(code_unit(bus, span, thumb));
                    spans.push(span);
                }
                _ => break,
            }
        }
        let mut ops = Vec::new();
        for (index, &word) in words.iter().enumerate().take(words.len().saturating_sub(2)) {
            let addr = pc.wrapping_add(index as u32 * size);
            if (addr ^ pc) & !(BLOCK_BYTES - 1) != 0 {
                break;
            }
            let op = if thumb {
                scan::classify_thumb::<V5>(word, addr)
            } else {
                scan::classify::<V5>(word)
            };
            ops.push(op);
            if op.stops() {
                break;
            }
        }
        let count = ops.len();
        if count == 0 {
            return None;
        }
        words.truncate(count + 2);
        spans.truncate(count + 2);
        let width = if thumb { Width::Half } else { Width::Word };
        let fetch = (0..count as u32)
            .map(|index| {
                let addr = pc.wrapping_add((index + 2) * size);
                [Access::NonSequential, Access::Sequential].map(|access| {
                    if V5 {
                        bus.code_cycles(addr, width, access)
                    } else {
                        bus.cycles(addr, width, access)
                    }
                })
            })
            .collect::<Vec<_>>();
        let mut pages: Vec<(CodeRam, usize)> = Vec::new();
        for span in &spans {
            if !pages.contains(&(span.ram, span.page())) {
                pages.push((span.ram, span.page()));
            }
        }
        let gens = pages
            .iter()
            .map(|&(ram, page)| bus.mark_code(ram, page))
            .collect();
        Some(Scan {
            pc,
            thumb,
            words: words.into_boxed_slice(),
            spans: spans.into_boxed_slice(),
            ops: ops.into_boxed_slice(),
            fetch: fetch.into_boxed_slice(),
            pages: pages.into_boxed_slice(),
            gens,
        })
    }

    fn flush(&mut self) {
        self.stats.flushes += 1;
        self.tag = self.tag.wrapping_add(1);
        self.entries.clear();
        self.lookup.clear();
        self.arena = Arena::default();
    }
}

impl<const V5: bool> Core<V5> {
    pub fn irq_stays_masked(&self) -> bool {
        self.irq_masked && self.regs.cpsr() & FLAG_I != 0
    }

    fn take_data(&mut self) -> u32 {
        let cycles = self.cycles + self.data;
        self.fetched = 0;
        self.data = 0;
        self.refetched = Refetch(0);
        cycles
    }
}

fn current<B: JitBus>(
    block: &Compiled,
    seen: &mut u32,
    gens: &mut [u32],
    epoch: u32,
    bus: &mut B,
) -> bool {
    let unchanged = *seen == epoch
        && block
            .pages
            .iter()
            .zip(gens.iter())
            .all(|(&(ram, page), &gen_seen)| bus.code_generation(ram, page) == Some(gen_seen));
    if unchanged {
        return true;
    }
    if block
        .spans
        .iter()
        .zip(block.words.iter())
        .any(|(&span, &word)| code_unit(bus, span, block.thumb) != word)
    {
        return false;
    }
    for (gen_seen, &(ram, page)) in gens.iter_mut().zip(block.pages.iter()) {
        *gen_seen = bus.mark_code(ram, page);
    }
    *seen = epoch;
    true
}

fn size(thumb: bool) -> u32 {
    if thumb { 2 } else { 4 }
}

fn key(pc: u32, thumb: bool) -> u32 {
    pc | u32::from(thumb)
}

fn code_unit<B: JitBus>(bus: &B, span: CodeSpan, thumb: bool) -> u32 {
    if !thumb {
        return bus.code_word(span);
    }
    let word = bus.code_word(CodeSpan {
        offset: span.offset & !3,
        ..span
    });
    (word >> ((span.offset & 2) * 8)) & 0xFFFF
}

pub fn target() -> String {
    Emitter::new()
        .map(|emitter| emitter.target())
        .unwrap_or_default()
}

fn next_epoch() -> u32 {
    EPOCHS.fetch_add(1, Ordering::Relaxed)
}
