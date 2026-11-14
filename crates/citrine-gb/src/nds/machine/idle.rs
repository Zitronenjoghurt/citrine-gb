use super::{Burst, Fault, Nds, side};
use crate::gba::cpu::{CoprocessorRegister, Core, CpuBus, Jump};
use crate::gba::wait::{Access, Width};
#[cfg(feature = "jit-lockstep")]
use crate::jit::lockstep::Track;
use crate::nds::bus::{Arm7Bus, Arm9Bus};
use crate::nds::cp15::{Cp15, Fetch};
use crate::nds::{Cpu, geometry, render, three_d, wifi};

const SEARCH: u32 = 16;
const LOOP_BYTES: u32 = 0x400;
const PERIOD: usize = 128;
const MIN_SPAN: u64 = 48;
const MAX_TOUCHES: usize = 512;
const UNIT: u64 = 64;
const MAX_LEVEL: u32 = 12;
const IO: u32 = 0x04;
const IPC_RECEIVE: u32 = 0x10_0000;
const CART_DATA: u32 = 0x10_0010;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IdleStats {
    pub skipped: [u64; 2],
    pub skips: [u64; 2],
    pub probes: [u64; 2],
    pub proofs: [u64; 2],
}

#[derive(Debug, Clone, Default)]
pub(super) struct IdleLoops {
    off: bool,
    arm9: Watcher<true>,
    arm7: Watcher<false>,
}

#[derive(Debug, Clone)]
struct Watcher<const V5: bool> {
    next: u64,
    level: u32,
    lo: u32,
    span: u32,
    cost: u64,
    layout: Cp15,
    phases: Vec<Phase<V5>>,
    touches: Vec<Touch>,
    trial: Vec<Phase<V5>>,
    seen: Vec<Touch>,
    skipped: u64,
    skips: u64,
    probes: u64,
    proofs: u64,
}

impl<const V5: bool> Default for Watcher<V5> {
    fn default() -> Self {
        Self {
            next: 0,
            level: 0,
            lo: 0,
            span: 0,
            cost: 0,
            layout: Cp15::default(),
            phases: Vec::with_capacity(PERIOD),
            touches: Vec::with_capacity(MAX_TOUCHES),
            trial: Vec::with_capacity(PERIOD),
            seen: Vec::with_capacity(MAX_TOUCHES + 16),
            skipped: 0,
            skips: 0,
            probes: 0,
            proofs: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct Phase<const V5: bool> {
    pc: u32,
    at: u64,
    ahead: u32,
    core: Core<V5>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Code,
    Data,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Touch {
    kind: Kind,
    addr: u32,
    width: Width,
    value: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Proven,
    Missed,
}

enum Step {
    Pure,
    Impure,
    Fault,
}

impl Nds {
    pub fn skip_idle_loops(&mut self, on: bool) {
        self.idle.off = !on;
        self.idle.restart();
    }

    pub fn idle_stats(&self) -> IdleStats {
        let (arm9, arm7) = (&self.idle.arm9, &self.idle.arm7);
        IdleStats {
            skipped: [arm9.skipped, arm7.skipped],
            skips: [arm9.skips, arm7.skips],
            probes: [arm9.probes, arm7.probes],
            proofs: [arm9.proofs, arm7.proofs],
        }
    }

    #[inline(always)]
    pub(super) fn idle_slice<const ARM9: bool>(
        &mut self,
        stop: u64,
        target: u64,
        armed: bool,
    ) -> Option<Option<u64>> {
        let due = if ARM9 {
            self.arm9_cycles >= self.idle.arm9.next
        } else {
            self.arm7_cycles >= self.idle.arm7.next
        };
        if !due {
            return None;
        }
        self.idle_slow::<ARM9>(stop, target, armed)
    }

    #[inline(never)]
    fn idle_slow<const ARM9: bool>(
        &mut self,
        stop: u64,
        target: u64,
        armed: bool,
    ) -> Option<Option<u64>> {
        let cpu = if ARM9 { Cpu::Arm9 } else { Cpu::Arm7 };
        let clock = *self.clock_mut(cpu);
        if self.idle.off
            || !self.unwatched(cpu)
            || self.buses.halted(cpu)
            || self.buses.irq_line(cpu)
            || stop <= clock
        {
            if ARM9 {
                self.idle.arm9.wait(clock);
            } else {
                self.idle.arm7.wait(clock);
            }
            return None;
        }
        let skips = if ARM9 {
            self.idle.arm9.skips
        } else {
            self.idle.arm7.skips
        };
        let burst = if ARM9 {
            let mut bus = self.buses.arm9();
            self.idle.arm9.slice(
                &mut self.arm9,
                &mut bus,
                &mut self.arm9_cycles,
                stop,
                #[cfg(feature = "jit-lockstep")]
                &mut self.lockstep.arm9,
            )
        } else {
            let mut bus = self.buses.arm7();
            self.idle.arm7.slice(
                &mut self.arm7,
                &mut bus,
                &mut self.arm7_cycles,
                stop,
                #[cfg(feature = "jit-lockstep")]
                &mut self.lockstep.arm7,
            )
        };
        let now = if ARM9 {
            self.idle.arm9.skips
        } else {
            self.idle.arm7.skips
        };
        self.idle_skips += now - skips;
        match burst {
            Burst::Done => None,
            Burst::Attention(start) if !armed && self.buses.slice_cut() => Some(Some(start)),
            Burst::Attention(_) => None,
            Burst::Fault => {
                self.faults[side(cpu)] = Some(Fault {
                    cpu,
                    pc: self.pc_of(cpu),
                });
                *self.clock_mut(cpu) = target;
                Some(None)
            }
        }
    }

    #[cfg(feature = "debug")]
    fn unwatched(&self, cpu: Cpu) -> bool {
        !self.watches_instructions(cpu)
    }

    #[cfg(not(feature = "debug"))]
    fn unwatched(&self, _cpu: Cpu) -> bool {
        true
    }
}

impl IdleLoops {
    pub(super) fn restart(&mut self) {
        let next = if self.off { u64::MAX } else { 0 };
        (self.arm9.next, self.arm9.level) = (next, 0);
        (self.arm7.next, self.arm7.level) = (next, 0);
    }

    #[cfg(feature = "persistence")]
    pub(super) fn restarted(&self) -> Self {
        let mut idle = self.clone();
        idle.restart();
        idle
    }
}

impl<const V5: bool> Watcher<V5> {
    fn wait(&mut self, clock: u64) {
        self.next = clock + (UNIT << self.level);
    }

    fn back_off(&mut self, clock: u64) {
        self.level = (self.level + 1).min(MAX_LEVEL);
        self.wait(clock);
    }

    fn paid(&mut self) {
        (self.next, self.level) = (0, 0);
    }

    fn slice<B: Side>(
        &mut self,
        core: &mut Core<V5>,
        bus: &mut B,
        clock: &mut u64,
        stop: u64,
        #[cfg(feature = "jit-lockstep")] track: &mut Track<V5>,
    ) -> Burst {
        if core.pc().wrapping_sub(self.lo) < self.span
            && let Some(from) = self.phase_of(core, bus)
        {
            self.jump(
                core,
                bus,
                clock,
                stop,
                from,
                #[cfg(feature = "jit-lockstep")]
                track,
            );
            self.paid();
            return Burst::Done;
        }
        if stop < *clock + MIN_SPAN {
            self.wait(*clock);
            return Burst::Done;
        }
        self.probes += 1;
        let (burst, verdict) = self.probe(
            core,
            bus,
            clock,
            stop,
            #[cfg(feature = "jit-lockstep")]
            track,
        );
        match verdict {
            Verdict::Proven => self.paid(),
            Verdict::Missed => self.back_off(*clock),
        }
        burst
    }

    fn phase_of<B: Side>(&self, core: &Core<V5>, bus: &B) -> Option<usize> {
        let pc = core.pc();
        let ahead = bus.ahead();
        let from = self.phases.iter().position(|phase| {
            phase.pc == pc && phase.ahead == ahead && phase.core.runs_like(core)
        })?;
        let holds = bus.same_layout(&self.layout)
            && self
                .touches
                .iter()
                .all(|touch| bus.peek(touch) == Some(touch.value));
        holds.then_some(from)
    }

    fn jump<B: Side>(
        &mut self,
        core: &mut Core<V5>,
        bus: &mut B,
        clock: &mut u64,
        stop: u64,
        from: usize,
        #[cfg(feature = "jit-lockstep")] track: &mut Track<V5>,
    ) {
        let (phases, cost) = (&self.phases, self.cost);
        let count = phases.len() as u64;
        let base = phases[from].at;
        let point = base + (stop - *clock);
        let (passes, rest) = (point / cost, point % cost);
        let ends = |phase: &Phase<V5>| phase.core.ends();
        let (to, at, index) = match phases
            .iter()
            .position(|phase| phase.at >= rest && ends(phase))
        {
            Some(to) => (to, phases[to].at, passes * count + to as u64),
            None => {
                let to = phases.iter().position(ends).unwrap_or(0);
                (to, cost + phases[to].at, (passes + 1) * count + to as u64)
            }
        };
        let moved = passes * cost + at - base;
        #[cfg(feature = "jit-lockstep")]
        {
            let start = *clock;
            let first = from as u64;
            track.leaped(index - first, |offset| {
                let unrolled = first + offset;
                let phase = &phases[(unrolled % count) as usize];
                (
                    start + (unrolled / count) * cost + phase.at - base,
                    &phase.core,
                )
            });
        }
        #[cfg(not(feature = "jit-lockstep"))]
        let _ = index;
        *clock += moved;
        core.clone_from(&phases[to].core);
        bus.set_ahead(phases[to].ahead);
        self.skipped += moved;
        self.skips += 1;
    }

    fn probe<B: Side>(
        &mut self,
        core: &mut Core<V5>,
        bus: &mut B,
        clock: &mut u64,
        stop: u64,
        #[cfg(feature = "jit-lockstep")] track: &mut Track<V5>,
    ) -> (Burst, Verdict) {
        let mut head = None;
        for _ in 0..SEARCH {
            if *clock >= stop && core.ends() {
                return (Burst::Done, Verdict::Missed);
            }
            let (start, before) = (*clock, core.pc());
            let step = step(core, bus, clock, &mut self.seen, false);
            #[cfg(feature = "jit-lockstep")]
            if !matches!(step, Step::Fault) {
                track.stepped(core, *clock);
            }
            match step {
                Step::Fault => return (Burst::Fault, Verdict::Missed),
                Step::Impure => return (Burst::Attention(start), Verdict::Missed),
                Step::Pure => {}
            }
            let after = core.pc();
            if after <= before && before - after <= LOOP_BYTES {
                head = Some(after);
                break;
            }
        }
        let Some(head) = head else {
            return (Burst::Done, Verdict::Missed);
        };
        self.trial.clear();
        self.seen.clear();
        self.trial.push(Phase {
            pc: head,
            at: 0,
            ahead: bus.ahead(),
            core: core.clone(),
        });
        let begin = *clock;
        let mut closed = false;
        for _ in 0..PERIOD {
            if *clock >= stop && core.ends() {
                return (Burst::Done, Verdict::Missed);
            }
            let start = *clock;
            let step = step(core, bus, clock, &mut self.seen, true);
            #[cfg(feature = "jit-lockstep")]
            if !matches!(step, Step::Fault) {
                track.stepped(core, *clock);
            }
            match step {
                Step::Fault => return (Burst::Fault, Verdict::Missed),
                Step::Impure => return (Burst::Attention(start), Verdict::Missed),
                Step::Pure => {}
            }
            if self.seen.len() > MAX_TOUCHES {
                return (Burst::Done, Verdict::Missed);
            }
            let (pc, ahead) = (core.pc(), bus.ahead());
            let first = &self.trial[0];
            if pc == head && ahead == first.ahead && core.runs_like(&first.core) {
                closed = true;
                break;
            }
            self.trial.push(Phase {
                pc,
                at: *clock - begin,
                ahead,
                core: core.clone(),
            });
        }
        if !closed || !self.settle_touches(bus) {
            return (Burst::Done, Verdict::Missed);
        }
        self.trial[0].core.clone_from(core);
        std::mem::swap(&mut self.phases, &mut self.trial);
        std::mem::swap(&mut self.touches, &mut self.seen);
        self.cost = *clock - begin;
        bus.keep_layout(&mut self.layout);
        let (lo, hi) = self.phases.iter().fold((u32::MAX, 0), |(lo, hi), phase| {
            (lo.min(phase.pc), hi.max(phase.pc))
        });
        self.lo = lo;
        self.span = hi - lo + 1;
        self.proofs += 1;
        if *clock < stop {
            self.jump(
                core,
                bus,
                clock,
                stop,
                0,
                #[cfg(feature = "jit-lockstep")]
                track,
            );
        }
        (Burst::Done, Verdict::Proven)
    }

    fn settle_touches<B: Side>(&mut self, bus: &B) -> bool {
        self.seen.sort_unstable_by_key(|touch| {
            (
                touch.addr,
                touch.kind as u8,
                touch.width.bytes(),
                touch.value,
            )
        });
        self.seen.dedup();
        self.seen
            .iter()
            .all(|touch| bus.peek(touch) == Some(touch.value))
    }
}

#[inline(always)]
fn step<const V5: bool, B: Side>(
    core: &mut Core<V5>,
    bus: &mut B,
    clock: &mut u64,
    log: &mut Vec<Touch>,
    record: bool,
) -> Step {
    let mut watch = Watch {
        bus,
        pure: true,
        record,
        log,
    };
    let cycles = core.step(&mut watch);
    let pure = watch.pure;
    let (_, stall) = bus.settle_step();
    if cycles == 0 {
        return Step::Fault;
    }
    *clock += u64::from(cycles + stall);
    if pure && stall == 0 {
        Step::Pure
    } else {
        Step::Impure
    }
}

trait Side: CpuBus {
    fn settle_step(&mut self) -> (bool, u32);
    fn peek(&self, touch: &Touch) -> Option<u32>;
    fn ahead(&self) -> u32;
    fn set_ahead(&mut self, ahead: u32);
    fn keep_layout(&self, layout: &mut Cp15);
    fn same_layout(&self, layout: &Cp15) -> bool;
    fn changes_on_read(addr: u32) -> bool;
}

impl Side for Arm9Bus<'_> {
    #[inline(always)]
    fn settle_step(&mut self) -> (bool, u32) {
        self.take_events()
    }

    fn peek(&self, touch: &Touch) -> Option<u32> {
        let fetch = match touch.kind {
            Kind::Code => Fetch::Code,
            Kind::Data | Kind::Write => Fetch::Data,
        };
        self.peek_plain(touch.addr, touch.width, fetch, touch.kind == Kind::Data)
    }

    fn ahead(&self) -> u32 {
        Arm9Bus::ahead(self)
    }

    fn set_ahead(&mut self, ahead: u32) {
        Arm9Bus::set_ahead(self, ahead);
    }

    fn keep_layout(&self, layout: &mut Cp15) {
        layout.clone_from(self.cp15());
    }

    fn same_layout(&self, layout: &Cp15) -> bool {
        self.cp15() == layout
    }

    fn changes_on_read(addr: u32) -> bool {
        let word = addr & 0x00FF_FFFC;
        let offset = word as usize;
        word == IPC_RECEIVE
            || word == CART_DATA
            || offset == render::DISP3DCNT
            || (three_d::REGISTERS_FIRST..=three_d::REGISTERS_LAST).contains(&offset)
            || (geometry::FIRST..=geometry::LAST).contains(&offset)
    }
}

impl Side for Arm7Bus<'_> {
    #[inline(always)]
    fn settle_step(&mut self) -> (bool, u32) {
        self.take_events()
    }

    fn peek(&self, touch: &Touch) -> Option<u32> {
        self.peek_plain(touch.addr, touch.width, touch.kind == Kind::Data)
    }

    fn ahead(&self) -> u32 {
        0
    }

    fn set_ahead(&mut self, _ahead: u32) {}

    fn keep_layout(&self, _layout: &mut Cp15) {}

    fn same_layout(&self, _layout: &Cp15) -> bool {
        true
    }

    fn changes_on_read(addr: u32) -> bool {
        let word = addr & 0x00FF_FFFC;
        word == IPC_RECEIVE
            || word == CART_DATA
            || (wifi::BASE..wifi::BASE + wifi::SIZE).contains(&addr)
    }
}

struct Watch<'a, B> {
    bus: &'a mut B,
    pure: bool,
    record: bool,
    log: &'a mut Vec<Touch>,
}

impl<B: Side> Watch<'_, B> {
    #[inline(always)]
    fn note(&mut self, kind: Kind, addr: u32, width: Width, value: u32) {
        if self.record {
            self.log.push(Touch {
                kind,
                addr,
                width,
                value,
            });
        }
    }

    #[inline(always)]
    fn check_read(&mut self, addr: u32) {
        if addr >> 24 == IO && B::changes_on_read(addr) {
            self.pure = false;
        }
    }

    #[inline(always)]
    fn check_write(&mut self, addr: u32) {
        if addr >> 24 == IO {
            self.pure = false;
        }
    }
}

impl<B: Side> CpuBus for Watch<'_, B> {
    #[inline(always)]
    fn fetch(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        let value = self.bus.fetch(addr, width, access);
        self.note(Kind::Code, addr, width, value);
        value
    }

    #[inline(always)]
    fn read(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.check_read(addr);
        let value = self.bus.read(addr, width, access);
        self.note(Kind::Data, addr, width, value);
        value
    }

    #[inline(always)]
    fn write(&mut self, addr: u32, width: Width, value: u32, access: Access) {
        self.check_write(addr);
        self.note(Kind::Write, addr, width, value);
        self.bus.write(addr, width, value, access);
    }

    #[inline(always)]
    fn idle(&mut self) {
        self.bus.idle();
    }

    #[inline(always)]
    fn cycles(&self, addr: u32, width: Width, access: Access) -> u32 {
        self.bus.cycles(addr, width, access)
    }

    #[inline(always)]
    fn code_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.bus.code_cycles(addr, width, access)
    }

    #[inline(always)]
    fn data_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.bus.data_cycles(addr, width, access)
    }

    #[inline(always)]
    fn timed_fetch(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        let (cycles, value) = self.bus.timed_fetch(addr, width, access);
        self.note(Kind::Code, addr & !(width.bytes() as u32 - 1), width, value);
        (cycles, value)
    }

    #[inline(always)]
    fn timed_read(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        self.check_read(addr);
        let (cycles, value) = self.bus.timed_read(addr, width, access);
        self.note(Kind::Data, addr, width, value);
        (cycles, value)
    }

    #[inline(always)]
    fn branch(&mut self) {
        self.bus.branch();
    }

    #[inline(always)]
    fn jump(&mut self, jump: Jump) {
        self.bus.jump(jump);
    }

    #[inline(always)]
    fn settle(&mut self, cycles: u32) -> u32 {
        self.bus.settle(cycles)
    }

    #[inline(always)]
    fn bus_value(&self) -> u32 {
        self.bus.bus_value()
    }

    #[inline(always)]
    fn drive(&mut self, value: u32) {
        self.bus.drive(value);
    }

    fn coprocessor_read(&mut self, register: CoprocessorRegister) -> Option<u32> {
        self.pure = false;
        self.bus.coprocessor_read(register)
    }

    fn coprocessor_write(&mut self, register: CoprocessorRegister, value: u32) -> bool {
        self.pure = false;
        self.bus.coprocessor_write(register, value)
    }

    #[inline(always)]
    fn vector_base(&self) -> u32 {
        self.bus.vector_base()
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::nds;
    use super::Nds;
    use crate::gba::cpu::{Arm9, Cpu as Arm7};
    use crate::gba::wait::Width;

    const SHARED: u32 = 0x0210_0000;
    const WAIT_FOR_LINE_100_IN_ITCM: [u32; 16] = [
        0xE3A0_1301,
        0xE3A0_3621,
        0xE28F_4018,
        0xE3A0_5401,
        0xE3A0_6006,
        0xE494_0004,
        0xE485_0004,
        0xE256_6001,
        0x1AFF_FFFB,
        0xE3A0_F401,
        0xE1D1_00B6,
        0xE350_0064,
        0x1AFF_FFFC,
        0xE282_2001,
        0xE583_2000,
        0xEAFF_FFF9,
    ];
    const WAIT_FOR_A_NEW_WORD: [u32; 7] = [
        0xE3A0_3621,
        0xE593_0000,
        0xE150_0004,
        0x0AFF_FFFC,
        0xE1A0_4000,
        0xE285_5001,
        0xEAFF_FFF9,
    ];
    const POLL_A_FLAG_THROUGH_A_CALL_IN_ITCM: [u32; 22] = [
        0xE3A0_3621,
        0xE3A0_D622,
        0xE3A0_2000,
        0xE28F_4018,
        0xE3A0_5401,
        0xE3A0_600B,
        0xE494_0004,
        0xE485_0004,
        0xE256_6001,
        0x1AFF_FFFB,
        0xE3A0_F401,
        0xEB00_0006,
        0xE350_0000,
        0x0AFF_FFFC,
        0xE282_2001,
        0xE583_2004,
        0xE3A0_0000,
        0xE583_0000,
        0xEAFF_FFF7,
        0xE92D_4010,
        0xE593_0000,
        0xE8BD_8010,
    ];
    const RAISE_THE_FLAG_ON_LINE_100: [u32; 11] = [
        0xE3A0_1301,
        0xE3A0_3621,
        0xE3A0_4001,
        0xE1D1_00B6,
        0xE350_0064,
        0x1AFF_FFFC,
        0xE583_4000,
        0xE1D1_00B6,
        0xE350_0064,
        0x0AFF_FFFC,
        0xEAFF_FFF7,
    ];
    const COUNT_FOREVER: [u32; 3] = [0xE3A0_3621, 0xE280_0001, 0xEAFF_FFFD];

    fn run(arm9: &[u32], arm7: &[u32], skip: bool) -> Nds {
        let mut nds = nds(arm9, arm7);
        #[cfg(feature = "jit")]
        nds.set_jit(false);
        nds.skip_idle_loops(skip);
        for _ in 0..4 {
            nds.run_frame();
        }
        nds
    }

    fn outcome(nds: &mut Nds) -> (u64, u64, u64, Arm9, Arm7, u32, u32) {
        (
            nds.arm9_cycles,
            nds.arm7_cycles,
            nds.now,
            nds.arm9.clone(),
            nds.arm7.clone(),
            nds.buses.arm9().read(SHARED, Width::Word),
            nds.buses.arm9().read(SHARED + 4, Width::Word),
        )
    }

    #[test]
    fn skipped_wait_loops_end_on_the_same_cycle_and_state() {
        let (arm9, arm7) = (&WAIT_FOR_LINE_100_IN_ITCM, &WAIT_FOR_A_NEW_WORD);
        let (mut skipped, mut ran) = (run(arm9, arm7, true), run(arm9, arm7, false));
        assert_eq!(outcome(&mut skipped), outcome(&mut ran));
        assert!(outcome(&mut ran).5 > 0);
    }

    #[test]
    fn both_cpus_skip_their_wait_loops() {
        let (arm9, arm7) = (&WAIT_FOR_LINE_100_IN_ITCM, &WAIT_FOR_A_NEW_WORD);
        let stats = run(arm9, arm7, true).idle_stats();
        assert_eq!(
            stats.skipped.map(|cycles| cycles > 0),
            [true; 2],
            "{stats:?}"
        );
        assert_eq!(run(arm9, arm7, false).idle_stats().skipped, [0, 0]);
    }

    #[test]
    fn a_wait_loop_that_calls_and_returns_is_skipped_exactly() {
        let (arm9, arm7) = (
            &POLL_A_FLAG_THROUGH_A_CALL_IN_ITCM,
            &RAISE_THE_FLAG_ON_LINE_100,
        );
        let (mut skipped, mut ran) = (run(arm9, arm7, true), run(arm9, arm7, false));
        assert_eq!(outcome(&mut skipped), outcome(&mut ran));
        assert!(outcome(&mut ran).6 >= 3);
        assert!(skipped.idle_stats().skipped[0] > 0);
    }

    #[test]
    fn a_loop_that_never_repeats_is_probed_less_and_less() {
        let (mut skipped, mut ran) = (
            run(&COUNT_FOREVER, &COUNT_FOREVER, true),
            run(&COUNT_FOREVER, &COUNT_FOREVER, false),
        );
        assert_eq!(outcome(&mut skipped), outcome(&mut ran));
        let stats = skipped.idle_stats();
        assert_eq!(stats.proofs, [0, 0]);
        assert!(stats.probes.iter().all(|&probes| probes < 40), "{stats:?}");
    }
}
