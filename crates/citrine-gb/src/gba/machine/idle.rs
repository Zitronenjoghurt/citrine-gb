use super::Gba;
use crate::gba::bus::Bus;
use crate::gba::cpu::{CoprocessorRegister, Cpu, CpuBus, Jump};
use crate::gba::open_bus::OpenBus;
use crate::gba::prefetch::Prefetch;
use crate::gba::region::Region;
use crate::gba::wait::{Access, WaitControl, Width};

const SEARCH: u32 = 16;
const LOOP_BYTES: u32 = 0x400;
const PERIOD: u32 = 128;
const MIN_SPAN: u32 = 48;
const MAX_TOUCHES: usize = 512;
const UNIT: u64 = 64;
const MAX_LEVEL: u32 = 12;
const RECHECK: u64 = 256;
const BLOCKED: u64 = 4096;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IdleStats {
    pub skipped: u64,
    pub skips: u64,
    pub probes: u64,
    pub proofs: u64,
}

#[derive(Default)]
pub(super) struct IdleLoops {
    off: bool,
    next: u64,
    probe_at: u64,
    reprobe_at: u64,
    level: u32,
    mode: Mode,
    reprobe: bool,
    lo: u32,
    span: u32,
    cost: u32,
    wait: WaitControl,
    phases: Vec<Phase>,
    touches: Vec<Touch>,
    begin: u64,
    last: u64,
    steps: u32,
    trial: Vec<Phase>,
    seen: Vec<Touch>,
    stats: IdleStats,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Watching,
    Searching(u32),
    Recording,
}

#[derive(Clone)]
struct Phase {
    pc: u32,
    at: u32,
    core: Cpu,
    fetch: (Prefetch, OpenBus),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Touch {
    addr: u32,
    width: Width,
    value: u32,
}

impl Gba {
    pub fn skip_idle_loops(&mut self, on: bool) {
        self.idle.off = !on;
        self.idle.restart();
    }

    pub fn idle_stats(&self) -> IdleStats {
        self.idle.stats
    }

    #[inline(always)]
    pub(super) fn idle_due(&self) -> bool {
        self.cycles >= self.idle.next
    }

    #[inline(always)]
    pub(super) fn idle_at(&self) -> u64 {
        self.idle.next
    }

    #[cold]
    #[inline(never)]
    pub(super) fn idle_turn(&mut self, until: u64) -> Option<u32> {
        if self.idle.off
            || until <= self.cycles
            || !self.no_debugger()
            || self.bus.serial().plugged()
        {
            self.idle.hold(self.cycles, BLOCKED);
            return None;
        }
        if self.bus.interrupts().pending() {
            self.idle.hold(self.cycles, UNIT);
            return None;
        }
        if self.idle.mode != Mode::Watching {
            if self.cycles != self.idle.last {
                self.idle.hold(self.cycles, UNIT);
                return None;
            }
            return Some(self.probe_step());
        }
        let left = (until - self.cycles).min(u64::from(u32::MAX)) as u32;
        let limit = self.bus.idle_span().min(left);
        let in_loop = self.idle.in_loop(self.cpu.pc());
        if in_loop && let Some(from) = self.idle.phase_of(&self.cpu, &self.bus) {
            return self.leap(from, limit);
        }
        if limit < MIN_SPAN {
            self.idle.hold(self.cycles, UNIT);
            return None;
        }
        if self.idle.probe_due(in_loop, self.cycles) {
            self.idle.start(in_loop, self.cycles);
            return Some(self.probe_step());
        }
        self.idle.schedule(self.cycles);
        None
    }

    fn leap(&mut self, from: usize, limit: u32) -> Option<u32> {
        let start = self.cycles;
        self.idle.next = start + u64::from(limit.max(1));
        let (to, retired, moved) = self.idle.landing(from, limit)?;
        self.bus.start_cpu_step();
        #[cfg(feature = "jit-lockstep")]
        {
            let idle = &self.idle;
            self.lockstep
                .leaped(retired, |offset| idle.unrolled(from, start, offset));
        }
        #[cfg(not(feature = "jit-lockstep"))]
        let _ = retired;
        let phase = &self.idle.phases[to];
        self.cpu.clone_from(&phase.core);
        self.bus.restore_fetch_state(phase.fetch);
        #[cfg(jit)]
        {
            self.straight = false;
        }
        self.bus.finish_cpu_step(moved);
        self.idle.paid(moved);
        Some(moved)
    }

    fn probe_step(&mut self) -> u32 {
        self.bus.start_cpu_step();
        let from = self.cpu.pc();
        let record = self.idle.mode == Mode::Recording;
        let mut watch = Watch {
            bus: &mut self.bus,
            pure: true,
            record,
            log: &mut self.idle.seen,
        };
        let cycles = self.cpu.step(&mut watch);
        let pure = watch.pure && cycles > 0 && self.bus.clear_of_dma();
        let cycles = self.finish_step(from, cycles);
        let end = self.cycles + u64::from(cycles);
        self.idle.last = end;
        self.idle.observe(from, &self.cpu, &self.bus, pure, end);
        cycles
    }
}

impl IdleLoops {
    pub(super) fn restart(&mut self) {
        self.mode = Mode::Watching;
        self.level = 0;
        self.probe_at = 0;
        self.reprobe_at = 0;
        self.next = if self.off { u64::MAX } else { 0 };
    }

    pub(super) fn forget(&mut self) {
        self.phases.clear();
        self.touches.clear();
        self.restart();
    }

    pub(super) fn restarted(&self) -> Self {
        let mut idle = Self {
            off: self.off,
            ..Self::default()
        };
        idle.restart();
        idle
    }

    fn hold(&mut self, now: u64, wait: u64) {
        self.mode = Mode::Watching;
        self.next = now + wait;
    }

    fn schedule(&mut self, now: u64) {
        let recheck = if self.span > 0 {
            now + RECHECK
        } else {
            u64::MAX
        };
        self.next = recheck.min(self.probe_at).max(now + UNIT);
    }

    fn probe_due(&self, in_loop: bool, now: u64) -> bool {
        if in_loop {
            now >= self.reprobe_at
        } else {
            now >= self.probe_at
        }
    }

    fn start(&mut self, reprobe: bool, now: u64) {
        self.mode = Mode::Searching(SEARCH);
        self.last = now;
        self.reprobe = reprobe;
        self.steps = 0;
        self.stats.probes += 1;
    }

    fn paid(&mut self, moved: u32) {
        self.level = 0;
        self.probe_at = 0;
        self.stats.skipped += u64::from(moved);
        self.stats.skips += 1;
    }

    fn miss(&mut self, end: u64) {
        if self.reprobe {
            self.reprobe_at = end + BLOCKED;
        } else {
            self.level = (self.level + 1).min(MAX_LEVEL);
            self.probe_at = end + (UNIT << self.level);
        }
        self.mode = Mode::Watching;
        self.schedule(end);
    }

    fn in_loop(&self, pc: u32) -> bool {
        pc.wrapping_sub(self.lo) < self.span
    }

    fn phase_of(&self, core: &Cpu, bus: &Bus) -> Option<usize> {
        let (pc, fetch) = (core.pc(), bus.fetch_state());
        let from = self.phases.iter().position(|phase| {
            phase.pc == pc && phase.fetch == fetch && phase.core.runs_like(core)
        })?;
        let holds = bus.wait() == self.wait && self.touches.iter().all(|touch| touch.holds(bus));
        holds.then_some(from)
    }

    fn landing(&self, from: usize, limit: u32) -> Option<(usize, u64, u32)> {
        if limit == 0 {
            return None;
        }
        let count = self.phases.len() as u64;
        let cost = u64::from(self.cost);
        let base = u64::from(self.phases[from].at);
        let point = base + u64::from(limit) - 1;
        let (passes, rest) = (point / cost, point % cost);
        let to = self
            .phases
            .iter()
            .rposition(|phase| u64::from(phase.at) <= rest)?;
        let moved = passes * cost + u64::from(self.phases[to].at) - base;
        let retired = passes * count + to as u64 - from as u64;
        (moved > 0).then_some((to, retired, moved as u32))
    }

    #[cfg(feature = "jit-lockstep")]
    fn unrolled(&self, from: usize, start: u64, offset: u64) -> (u64, &Cpu) {
        let count = self.phases.len() as u64;
        let unrolled = from as u64 + offset;
        let phase = &self.phases[(unrolled % count) as usize];
        let base = u64::from(self.phases[from].at);
        let clock = start + (unrolled / count) * u64::from(self.cost) + u64::from(phase.at) - base;
        (clock, &phase.core)
    }

    fn observe(&mut self, from: u32, core: &Cpu, bus: &Bus, pure: bool, end: u64) {
        if !pure {
            self.miss(end);
            return;
        }
        let pc = core.pc();
        match self.mode {
            Mode::Searching(left) => {
                if pc <= from && from - pc <= LOOP_BYTES {
                    self.begin_trial(pc, core, bus, end);
                } else if left > 1 {
                    self.mode = Mode::Searching(left - 1);
                } else {
                    self.miss(end);
                }
            }
            Mode::Recording => self.record(pc, core, bus, end),
            Mode::Watching => {}
        }
    }

    fn begin_trial(&mut self, pc: u32, core: &Cpu, bus: &Bus, end: u64) {
        self.trial.clear();
        self.seen.clear();
        self.trial.push(Phase {
            pc,
            at: 0,
            core: core.clone(),
            fetch: bus.fetch_state(),
        });
        self.begin = end;
        self.mode = Mode::Recording;
    }

    fn record(&mut self, pc: u32, core: &Cpu, bus: &Bus, end: u64) {
        self.steps += 1;
        if self.steps > PERIOD || self.seen.len() > MAX_TOUCHES {
            self.miss(end);
            return;
        }
        let fetch = bus.fetch_state();
        let first = &self.trial[0];
        if pc == first.pc {
            if fetch == first.fetch && core.runs_like(&first.core) {
                self.close(core, bus, end);
            } else {
                self.begin_trial(pc, core, bus, end);
            }
            return;
        }
        self.trial.push(Phase {
            pc,
            at: (end - self.begin) as u32,
            core: core.clone(),
            fetch,
        });
    }

    fn close(&mut self, core: &Cpu, bus: &Bus, end: u64) {
        self.seen
            .sort_unstable_by_key(|touch| (touch.addr, touch.width.bytes(), touch.value));
        self.seen.dedup();
        if !self.seen.iter().all(|touch| touch.holds(bus)) {
            self.miss(end);
            return;
        }
        self.seen
            .retain(|touch| !matches!(Region::of(touch.addr), Region::Rom(_)));
        self.trial[0].core.clone_from(core);
        std::mem::swap(&mut self.phases, &mut self.trial);
        std::mem::swap(&mut self.touches, &mut self.seen);
        self.cost = (end - self.begin) as u32;
        self.wait = bus.wait();
        let (lo, hi) = self.phases.iter().fold((u32::MAX, 0), |(lo, hi), phase| {
            (lo.min(phase.pc), hi.max(phase.pc))
        });
        self.lo = lo;
        self.span = hi - lo + 1;
        self.stats.proofs += 1;
        self.mode = Mode::Watching;
        self.reprobe_at = 0;
        self.level = 0;
        self.next = 0;
    }
}

impl Touch {
    fn holds(&self, bus: &Bus) -> bool {
        bus.plain_value(self.addr, self.width) == Some(self.value)
    }
}

struct Watch<'a> {
    bus: &'a mut Bus,
    pure: bool,
    record: bool,
    log: &'a mut Vec<Touch>,
}

impl Watch<'_> {
    #[inline(always)]
    fn check(&mut self, addr: u32, width: Width) {
        if !self.bus.plain(addr, width) {
            self.pure = false;
        }
    }

    #[inline(always)]
    fn note(&mut self, addr: u32, width: Width, value: u32) {
        self.check(addr, width);
        if self.record {
            let bytes = width.bytes() as u32;
            self.log.push(Touch {
                addr: addr & !(bytes - 1),
                width,
                value: value & (u32::MAX >> (32 - 8 * bytes)),
            });
        }
    }
}

impl CpuBus for Watch<'_> {
    #[inline(always)]
    fn fetch(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        let value = <Bus as CpuBus>::fetch(self.bus, addr, width, access);
        self.note(addr, width, value);
        value
    }

    #[inline(always)]
    fn read(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        let value = <Bus as CpuBus>::read(self.bus, addr, width, access);
        self.note(addr, width, value);
        value
    }

    #[inline(always)]
    fn write(&mut self, addr: u32, width: Width, value: u32, access: Access) {
        if !matches!(Region::of(addr), Region::Ewram | Region::Iwram) {
            self.pure = false;
        }
        self.note(addr, width, value);
        <Bus as CpuBus>::write(self.bus, addr, width, value, access);
    }

    #[inline(always)]
    fn idle(&mut self) {
        <Bus as CpuBus>::idle(self.bus);
    }

    #[inline(always)]
    fn cycles(&self, addr: u32, width: Width, access: Access) -> u32 {
        <Bus as CpuBus>::cycles(&*self.bus, addr, width, access)
    }

    #[inline(always)]
    fn code_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.check(addr, width);
        <Bus as CpuBus>::code_cycles(self.bus, addr, width, access)
    }

    #[inline(always)]
    fn data_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.check(addr, width);
        <Bus as CpuBus>::data_cycles(self.bus, addr, width, access)
    }

    #[inline(always)]
    fn timed_fetch(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        let (cycles, value) = <Bus as CpuBus>::timed_fetch(self.bus, addr, width, access);
        self.note(addr, width, value);
        (cycles, value)
    }

    #[inline(always)]
    fn timed_read(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        let (cycles, value) = <Bus as CpuBus>::timed_read(self.bus, addr, width, access);
        self.note(addr, width, value);
        (cycles, value)
    }

    #[inline(always)]
    fn timed_refill(&mut self, addr: u32, width: Width) -> [(u32, u32); 2] {
        let words = <Bus as CpuBus>::timed_refill(self.bus, addr, width);
        let next = addr.wrapping_add(width.bytes() as u32);
        self.note(addr, width, words[0].1);
        self.note(next, width, words[1].1);
        words
    }

    #[inline(always)]
    fn timed_write(&mut self, addr: u32, width: Width, value: u32, access: Access) -> u32 {
        if !matches!(Region::of(addr), Region::Ewram | Region::Iwram) {
            self.pure = false;
        }
        self.note(addr, width, value);
        <Bus as CpuBus>::timed_write(self.bus, addr, width, value, access)
    }

    #[inline(always)]
    fn branch(&mut self) {
        <Bus as CpuBus>::branch(self.bus);
    }

    #[inline(always)]
    fn jump(&mut self, jump: Jump) {
        <Bus as CpuBus>::jump(self.bus, jump);
    }

    #[inline(always)]
    fn settle(&mut self, cycles: u32) -> u32 {
        <Bus as CpuBus>::settle(self.bus, cycles)
    }

    #[inline(always)]
    fn bus_value(&self) -> u32 {
        <Bus as CpuBus>::bus_value(&*self.bus)
    }

    #[inline(always)]
    fn drive(&mut self, value: u32) {
        <Bus as CpuBus>::drive(self.bus, value);
    }

    fn coprocessor_read(&mut self, register: CoprocessorRegister) -> Option<u32> {
        self.pure = false;
        <Bus as CpuBus>::coprocessor_read(self.bus, register)
    }

    fn coprocessor_write(&mut self, register: CoprocessorRegister, value: u32) -> bool {
        self.pure = false;
        <Bus as CpuBus>::coprocessor_write(self.bus, register, value)
    }

    #[inline(always)]
    fn vector_base(&self) -> u32 {
        <Bus as CpuBus>::vector_base(&*self.bus)
    }
}

#[cfg(test)]
mod tests {
    use super::Gba;
    use crate::gba::cpu::Cpu;

    const COUNTER: u32 = 0x0300_0004;
    const SETUP: [u32; 14] = [
        0xE59F_0058,
        0xE59F_1058,
        0xE580_1000,
        0xE3A0_0301,
        0xE3A0_1008,
        0xE1C0_10B4,
        0xE280_2C02,
        0xE59F_1044,
        0xE1C2_10B4,
        0xE3A0_1001,
        0xE582_1000,
        0xE582_1008,
        0xE3A0_3403,
        0xE3A0_4000,
    ];
    const ARM_WAIT_FOR_THE_VBLANK_FLAG: [u32; 8] = [
        0xE593_0000,
        0xE350_0000,
        0x0AFF_FFFC,
        0xE3A0_0000,
        0xE583_0000,
        0xE284_4001,
        0xE583_4004,
        0xEAFF_FFF7,
    ];
    const THUMB_WAIT_FOR_THE_VBLANK_FLAG: [u32; 6] = [
        0xE28F_0001,
        0xE12F_FF10,
        0x2800_6818,
        0x2000_D0FC,
        0x3401_6018,
        0xE7F7_605C,
    ];
    const LITERALS: [u32; 3] = [0x0300_7FFC, 0x0800_006C, 0x0000_4317];
    const RAISE_THE_FLAG: [u32; 7] = [
        0xE3A0_0301,
        0xE280_0C02,
        0xE3A0_1001,
        0xE1C0_10B2,
        0xE3A0_0403,
        0xE580_1000,
        0xE12F_FF1E,
    ];
    const COUNT_FOREVER: [u32; 2] = [0xE280_0001, 0xEAFF_FFFD];

    fn program(wait: &[u32]) -> Vec<u8> {
        let mut words = SETUP.to_vec();
        words.extend_from_slice(wait);
        words.resize(24, 0);
        words.extend_from_slice(&LITERALS);
        words.extend_from_slice(&RAISE_THE_FLAG);
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    fn run(rom: &[u8], skip: bool, jit: bool) -> Gba {
        let mut gba = Gba::new(rom);
        #[cfg(feature = "jit")]
        gba.set_jit(jit);
        #[cfg(not(feature = "jit"))]
        let _ = jit;
        gba.skip_idle_loops(skip);
        for _ in 0..6 {
            gba.run_frame();
        }
        gba
    }

    fn outcome(gba: &Gba) -> (u64, Cpu, u32, Vec<u8>) {
        (
            gba.cycles(),
            gba.cpu().clone(),
            gba.bus().read32(COUNTER),
            gba.frame().to_vec(),
        )
    }

    #[cfg(feature = "persistence")]
    fn state(gba: &Gba) -> Vec<u8> {
        rmp_serde::to_vec(&(&gba.cpu, &gba.bus)).expect("state")
    }

    fn jits() -> Vec<bool> {
        if cfg!(feature = "jit") {
            vec![false, true]
        } else {
            vec![false]
        }
    }

    #[test]
    fn skipped_wait_loops_end_on_the_same_cycle_and_state() {
        for wait in [
            &ARM_WAIT_FOR_THE_VBLANK_FLAG[..],
            &THUMB_WAIT_FOR_THE_VBLANK_FLAG[..],
        ] {
            let rom = program(wait);
            for jit in jits() {
                let (skipped, ran) = (run(&rom, true, jit), run(&rom, false, jit));
                assert_eq!(outcome(&skipped), outcome(&ran), "jit {jit}");
                #[cfg(feature = "persistence")]
                assert!(state(&skipped) == state(&ran), "jit {jit}");
                assert!(outcome(&ran).2 >= 5, "{:?}", outcome(&ran).2);
                let stats = skipped.idle_stats();
                assert!(stats.skipped > 0, "jit {jit} {stats:?}");
                assert_eq!(ran.idle_stats().skipped, 0);
            }
        }
    }

    #[test]
    fn most_of_a_wait_loop_frame_is_skipped() {
        let rom = program(&THUMB_WAIT_FOR_THE_VBLANK_FLAG);
        let stats = run(&rom, true, false).idle_stats();
        assert!(
            stats.skipped > 6 * super::super::CYCLES_PER_FRAME / 2,
            "{stats:?}"
        );
    }

    #[test]
    fn a_rom_patch_in_a_skipped_loop_runs_the_patched_code() {
        use crate::cheats::CheatTarget;
        let compare_immediate = (SETUP.len() + 2) * 4 + 2;
        let rom = program(&THUMB_WAIT_FOR_THE_VBLANK_FLAG);
        for jit in jits() {
            let [skipped, ran] = [true, false].map(|skip| {
                let mut gba = run(&rom, skip, jit);
                let before = gba.bus().read32(COUNTER);
                gba.set_rom_patches(&[(compare_immediate, 1)]);
                for _ in 0..3 {
                    gba.run_frame();
                }
                assert!(gba.bus().read32(COUNTER) > before + 100, "jit {jit}");
                gba
            });
            assert!(skipped.idle_stats().proofs > 0, "jit {jit}");
            assert_eq!(outcome(&skipped), outcome(&ran), "jit {jit}");
        }
    }

    #[test]
    fn a_loop_that_never_repeats_is_probed_less_and_less() {
        let rom = program(&COUNT_FOREVER);
        let (skipped, ran) = (run(&rom, true, false), run(&rom, false, false));
        assert_eq!(outcome(&skipped), outcome(&ran));
        let stats = skipped.idle_stats();
        assert_eq!(stats.proofs, 0);
        assert!(stats.probes < 40, "{stats:?}");
    }
}
