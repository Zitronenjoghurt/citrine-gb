use super::JitBus;
use crate::gba::cpu::memory::rotate_word;
use crate::gba::cpu::psr::FLAG_T;
use crate::gba::cpu::shifter::Shift;
use crate::gba::cpu::{CoprocessorRegister, Core, CpuBus, Jump};
use crate::gba::wait::{Access, Width};

pub const EXIT: u64 = 1 << 31;
pub const SEQUENTIAL: u32 = 1;
pub const LAST: u32 = 2;
pub const THUMB: u32 = 2;
pub const IDLES_SHIFT: u32 = 8;
pub const RUN_DONE_SHIFT: u32 = 32;
pub const RUN_LAST_SHIFT: u32 = 48;

#[derive(Clone, Copy)]
pub struct Helpers {
    pub load: usize,
    pub store: usize,
    pub branch: usize,
    pub step: usize,
    pub shift: usize,
    pub exchange: usize,
    pub load_word: usize,
    pub store_word: usize,
    pub load_pc: usize,
    pub fetch: usize,
    pub swap: usize,
    pub fetch_run: usize,
    pub fetch_flat: usize,
}

impl Helpers {
    pub fn of<const V5: bool, B: JitBus>() -> Self {
        Self {
            load: load::<V5, B> as *const () as usize,
            store: store::<V5, B> as *const () as usize,
            branch: branch::<V5, B> as *const () as usize,
            step: step::<V5, B> as *const () as usize,
            shift: shift as *const () as usize,
            exchange: exchange::<V5, B> as *const () as usize,
            load_word: load_word::<V5, B> as *const () as usize,
            store_word: store_word::<V5, B> as *const () as usize,
            load_pc: load_pc::<V5, B> as *const () as usize,
            fetch: fetch::<B> as *const () as usize,
            swap: swap::<V5, B> as *const () as usize,
            fetch_run: fetch_run::<B> as *const () as usize,
            fetch_flat: fetch_flat::<B> as *const () as usize,
        }
    }

    pub fn table(&self) -> Vec<usize> {
        let Self {
            load,
            store,
            branch,
            step,
            shift,
            exchange,
            load_word,
            store_word,
            load_pc,
            fetch,
            swap: _,
            fetch_run,
            fetch_flat: _,
        } = *self;
        vec![
            load, store, branch, step, shift, exchange, load_word, store_word, load_pc, fetch,
            fetch_run,
        ]
    }
}

fn width(kind: u32) -> Width {
    match kind & 3 {
        0 => Width::Byte,
        1 => Width::Half,
        _ => Width::Word,
    }
}

fn access(sequential: u32) -> Access {
    if sequential & SEQUENTIAL != 0 {
        Access::Sequential
    } else {
        Access::NonSequential
    }
}

fn fetched<const V5: bool, B: JitBus>(core: &mut Core<V5>, bus: &mut B) {
    if !V5 && !B::TIMED_FETCH {
        let width = if core.regs.cpsr() & FLAG_T != 0 {
            Width::Half
        } else {
            Width::Word
        };
        core.cycles += bus.code_cycles(core.regs.r[15], width, Access::Sequential);
    }
}

fn settle<const V5: bool, B: JitBus>(core: &mut Core<V5>, bus: &mut B) -> u32 {
    if !V5 {
        core.cycles = bus.settle(core.cycles);
    }
    core.take_data()
}

pub(super) fn leaves_memory(addr: u32) -> bool {
    matches!(addr >> 24, 0x04 | 0x08..)
}

fn must_exit<const V5: bool, B: JitBus>(core: &Core<V5>, bus: &B, addr: u32, store: bool) -> bool {
    if !B::leaves(addr, store) {
        return false;
    }
    if !V5 {
        return true;
    }
    let flag = bus.stall_flag();
    let stalled = !flag.is_null() && unsafe { *flag } != 0;
    stalled || !bus.runs_on(core.irq_stays_masked())
}

unsafe extern "C" fn load<const V5: bool, B: JitBus>(
    core: *mut Core<V5>,
    bus: *mut B,
    addr: u32,
    kind: u32,
) -> u64 {
    let (core, bus) = unsafe { (&mut *core, &mut *bus) };
    core.cycles = 0;
    fetched(core, bus);
    let value = core.load_value(bus, addr, width(kind), kind & 4 != 0);
    core.load_idle(bus);
    let exit = if must_exit(core, bus, addr, false) {
        EXIT
    } else {
        0
    };
    u64::from(value) << 32 | exit | u64::from(settle(core, bus))
}

unsafe extern "C" fn store<const V5: bool, B: JitBus>(
    core: *mut Core<V5>,
    bus: *mut B,
    addr: u32,
    value: u32,
    kind: u32,
) -> u64 {
    let (core, bus) = unsafe { (&mut *core, &mut *bus) };
    core.cycles = 0;
    fetched(core, bus);
    core.store(bus, addr, width(kind), value, Access::NonSequential);
    let exit = if bus.take_code_hit() || must_exit(core, bus, addr, true) {
        EXIT
    } else {
        0
    };
    exit | u64::from(settle(core, bus))
}

unsafe extern "C" fn swap<const V5: bool, B: JitBus>(
    core: *mut Core<V5>,
    bus: *mut B,
    addr: u32,
    value: u32,
    kind: u32,
) -> u64 {
    let (core, bus) = unsafe { (&mut *core, &mut *bus) };
    core.cycles = 0;
    fetched(core, bus);
    let width = width(kind);
    let loaded = core.load(bus, addr, width, Access::NonSequential);
    core.store(bus, addr, width, value, Access::NonSequential);
    core.load_idle(bus);
    let loaded = match width {
        Width::Word => rotate_word(loaded, addr),
        _ => loaded,
    };
    let exit = if bus.take_code_hit() || must_exit(core, bus, addr, true) {
        EXIT
    } else {
        0
    };
    u64::from(loaded) << 32 | exit | u64::from(settle(core, bus))
}

unsafe extern "C" fn branch<const V5: bool, B: JitBus>(
    core: *mut Core<V5>,
    bus: *mut B,
    target: u32,
) -> u32 {
    let (core, bus) = unsafe { (&mut *core, &mut *bus) };
    core.cycles = 0;
    core.regs.r[15] = target;
    if V5 {
        core.reload(bus);
    } else {
        core.jump(bus);
    }
    settle(core, bus)
}

unsafe extern "C" fn exchange<const V5: bool, B: JitBus>(
    core: *mut Core<V5>,
    bus: *mut B,
    target: u32,
) -> u32 {
    let (core, bus) = unsafe { (&mut *core, &mut *bus) };
    core.cycles = 0;
    let cpsr = core.regs.cpsr();
    if target & 1 != 0 {
        core.regs.set_cpsr(cpsr | FLAG_T);
    } else {
        core.regs.set_cpsr(cpsr & !FLAG_T);
    }
    core.regs.r[15] = target;
    core.jump(bus);
    settle(core, bus)
}

unsafe extern "C" fn load_word<const V5: bool, B: JitBus>(
    core: *mut Core<V5>,
    bus: *mut B,
    addr: u32,
    sequential: u32,
) -> u64 {
    let (core, bus) = unsafe { (&mut *core, &mut *bus) };
    core.cycles = 0;
    let value = core.load(bus, addr, Width::Word, access(sequential));
    let exit = if must_exit(core, bus, addr, false) {
        EXIT
    } else {
        0
    };
    if !V5 && sequential & LAST != 0 {
        fetched(core, bus);
        core.load_idle(bus);
    }
    u64::from(value) << 32 | exit | u64::from(word_cycles(core, bus, sequential))
}

unsafe extern "C" fn store_word<const V5: bool, B: JitBus>(
    core: *mut Core<V5>,
    bus: *mut B,
    addr: u32,
    value: u32,
    sequential: u32,
) -> u64 {
    let (core, bus) = unsafe { (&mut *core, &mut *bus) };
    core.cycles = 0;
    core.store(bus, addr, Width::Word, value, access(sequential));
    let exit = if bus.take_code_hit() || must_exit(core, bus, addr, true) {
        EXIT
    } else {
        0
    };
    if !V5 && sequential & LAST != 0 {
        fetched(core, bus);
    }
    exit | u64::from(word_cycles(core, bus, sequential))
}

fn word_cycles<const V5: bool, B: JitBus>(
    core: &mut Core<V5>,
    bus: &mut B,
    sequential: u32,
) -> u32 {
    if V5 || B::TIMED_FETCH || sequential & LAST != 0 {
        return settle(core, bus);
    }
    core.take_data();
    0
}

unsafe extern "C" fn load_pc<const V5: bool, B: JitBus>(
    core: *mut Core<V5>,
    bus: *mut B,
    value: u32,
) -> u64 {
    let (core, bus) = unsafe { (&mut *core, &mut *bus) };
    core.cycles = 0;
    core.load_pc_exchange(bus, value);
    let refetched = core.refetched.0;
    u64::from(refetched) << 32 | u64::from(core.take_data())
}

pub(super) unsafe extern "C" fn step<const V5: bool, B: JitBus>(
    core: *mut Core<V5>,
    bus: *mut B,
) -> u64 {
    let (core, bus) = unsafe { (&mut *core, &mut *bus) };
    let mut watch = Watch { bus, left: false };
    let cpsr = core.regs.cpsr();
    let cycles = core.execute(&mut watch);
    let control = (core.regs.cpsr() ^ cpsr) & 0x0FFF_FFFF != 0;
    let exit = if watch.left || watch.bus.take_code_hit() || control || core.ldm_glitch {
        EXIT
    } else {
        0
    };
    exit | u64::from(cycles)
}

unsafe extern "C" fn fetch<B: JitBus>(bus: *mut B, addr: u32, packed: u32) -> u32 {
    let bus = unsafe { &mut *bus };
    let width = if packed & THUMB != 0 {
        Width::Half
    } else {
        Width::Word
    };
    let (cycles, _) = bus.timed_fetch(addr, width, access(packed));
    let idles = packed >> IDLES_SHIFT;
    for _ in 0..idles {
        bus.idle();
    }
    cycles + idles
}

unsafe extern "C" fn fetch_run<B: JitBus>(
    bus: *mut B,
    addr: u32,
    packed: u32,
    count: u32,
    total: u32,
    budget: u32,
) -> u64 {
    let bus = unsafe { &mut *bus };
    let (width, step) = if packed & THUMB != 0 {
        (Width::Half, 2)
    } else {
        (Width::Word, 4)
    };
    let room = budget.saturating_sub(total);
    if bus.fetch_flat(addr, width, count, room) {
        return u64::from(count) | u64::from(count + 1) << RUN_DONE_SHIFT | 1 << RUN_LAST_SHIFT;
    }
    let mut access = access(packed);
    let (mut sum, mut done, mut last) = (0u32, 0u32, 0u32);
    while done < count {
        let (cycles, _) = bus.timed_fetch(addr.wrapping_add(done * step), width, access);
        access = Access::Sequential;
        sum += cycles;
        last = cycles;
        done += 1;
        if sum >= room {
            break;
        }
    }
    if sum < room {
        done = count + 1;
    }
    u64::from(sum) | u64::from(done) << RUN_DONE_SHIFT | u64::from(last) << RUN_LAST_SHIFT
}

unsafe extern "C" fn fetch_flat<B: JitBus>(bus: *mut B, addr: u32, packed: u32, room: i32) -> u64 {
    let bus = unsafe { &mut *bus };
    let width = if packed & THUMB != 0 {
        Width::Half
    } else {
        Width::Word
    };
    let count = packed >> IDLES_SHIFT;
    if bus.fetch_flat(addr, width, count, room.max(0) as u32) {
        return u64::from(count) | 1 << 32;
    }
    let (cycles, _) = bus.timed_fetch(addr, width, access(packed));
    u64::from(cycles) | u64::from(cycles) << 32
}

extern "C" fn shift(kind: u32, value: u32, amount: u32, carry: u32) -> u64 {
    let (result, carry) = Shift::from_bits(kind).by_register(value, amount, carry != 0);
    u64::from(result) | u64::from(carry) << 32
}

struct Watch<'a, B> {
    bus: &'a mut B,
    left: bool,
}

impl<B: JitBus> Watch<'_, B> {
    fn data(&mut self, addr: u32, store: bool) {
        self.left |= B::leaves(addr, store);
    }
}

impl<B: JitBus> CpuBus for Watch<'_, B> {
    fn fetch(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.bus.fetch(addr, width, access)
    }

    fn read(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.data(addr, false);
        self.bus.read(addr, width, access)
    }

    fn write(&mut self, addr: u32, width: Width, value: u32, access: Access) {
        self.data(addr, true);
        self.bus.write(addr, width, value, access);
    }

    fn idle(&mut self) {
        self.bus.idle();
    }

    fn cycles(&self, addr: u32, width: Width, access: Access) -> u32 {
        self.bus.cycles(addr, width, access)
    }

    fn code_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.bus.code_cycles(addr, width, access)
    }

    fn data_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.bus.data_cycles(addr, width, access)
    }

    fn timed_fetch(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        self.bus.timed_fetch(addr, width, access)
    }

    fn timed_read(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        self.data(addr, false);
        self.bus.timed_read(addr, width, access)
    }

    fn branch(&mut self) {
        self.bus.branch();
    }

    fn jump(&mut self, jump: Jump) {
        self.bus.jump(jump);
    }

    fn settle(&mut self, cycles: u32) -> u32 {
        self.bus.settle(cycles)
    }

    fn bus_value(&self) -> u32 {
        self.bus.bus_value()
    }

    fn drive(&mut self, value: u32) {
        self.bus.drive(value);
    }

    fn coprocessor_read(&mut self, register: CoprocessorRegister) -> Option<u32> {
        self.left = true;
        self.bus.coprocessor_read(register)
    }

    fn coprocessor_write(&mut self, register: CoprocessorRegister, value: u32) -> bool {
        self.left = true;
        self.bus.coprocessor_write(register, value)
    }

    fn vector_base(&self) -> u32 {
        self.bus.vector_base()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shift_helper_packs_the_carry_above_the_result() {
        assert_eq!(shift(0, 0x8000_0001, 1, 0), 0x1_0000_0002);
        assert_eq!(shift(1, 0x10, 0, 1), 0x1_0000_0010);
        assert_eq!(shift(3, 1, 33, 0), 0x1_8000_0000);
    }

    #[test]
    fn io_and_the_gba_slot_end_a_block_and_ram_does_not() {
        assert!(leaves_memory(0x0400_0208));
        assert!(leaves_memory(0x0800_0000));
        assert!(!leaves_memory(0x0200_0000));
        assert!(!leaves_memory(0x0600_0000));
    }
}
