use citrine_gb::gba::cpu::{Arm9, CoprocessorRegister, Core, Cpu, CpuBus, CpuState, Mode};
use citrine_gb::gba::disasm;
use citrine_gb::gba::wait::{Access, Width};
use std::collections::BTreeMap;

const CODE: u32 = 0x1000;
const DATA: u32 = 0x2000;
const SYSTEM: u32 = 0x1F;
const SUPERVISOR: u32 = 0x13;
const THUMB: u32 = 1 << 5;
const FLAG_N: u32 = 1 << 31;
const FLAG_Z: u32 = 1 << 30;
const FLAG_C: u32 = 1 << 29;
const FLAG_V: u32 = 1 << 28;
const FLAG_Q: u32 = 1 << 27;
const ARM_NOP: u32 = 0xE1A0_0000;
const THUMB_NOP: u16 = 0x46C0;
const MAIN_ID: u32 = 0x4105_9461;

struct FlatBus {
    memory: Vec<u8>,
    registers: BTreeMap<(u32, u32, u32, u32), u32>,
    writes: Vec<(CoprocessorRegister, u32)>,
    reads: Vec<CoprocessorRegister>,
    vector_base: u32,
}

impl FlatBus {
    fn new() -> Self {
        Self {
            memory: vec![0; 0x1_0000],
            registers: BTreeMap::new(),
            writes: Vec::new(),
            reads: Vec::new(),
            vector_base: 0,
        }
    }

    fn index(&self, addr: u32) -> usize {
        addr as usize & (self.memory.len() - 1)
    }

    fn get(&self, addr: u32, width: Width) -> u32 {
        let addr = addr & !(width.bytes() as u32 - 1);
        (0..width.bytes() as u32).fold(0, |value, byte| {
            value | u32::from(self.memory[self.index(addr + byte)]) << (8 * byte)
        })
    }

    fn put(&mut self, addr: u32, width: Width, value: u32) {
        let addr = addr & !(width.bytes() as u32 - 1);
        for byte in 0..width.bytes() as u32 {
            let index = self.index(addr + byte);
            self.memory[index] = (value >> (8 * byte)) as u8;
        }
    }

    fn word(&self, addr: u32) -> u32 {
        self.get(addr, Width::Word)
    }

    fn set_word(&mut self, addr: u32, value: u32) {
        self.put(addr, Width::Word, value);
    }
}

impl CpuBus for FlatBus {
    fn fetch(&mut self, addr: u32, width: Width, _access: Access) -> u32 {
        self.get(addr, width)
    }

    fn read(&mut self, addr: u32, width: Width, _access: Access) -> u32 {
        self.get(addr, width)
    }

    fn write(&mut self, addr: u32, width: Width, value: u32, _access: Access) {
        self.put(addr, width, value);
    }

    fn idle(&mut self) {}

    fn cycles(&self, _addr: u32, _width: Width, _access: Access) -> u32 {
        1
    }

    fn bus_value(&self) -> u32 {
        0
    }

    fn drive(&mut self, _value: u32) {}

    fn coprocessor_read(&mut self, register: CoprocessorRegister) -> Option<u32> {
        self.reads.push(register);
        if register.coprocessor != 15 {
            return None;
        }
        let key = (
            register.crn,
            register.crm,
            register.opcode1,
            register.opcode2,
        );
        self.registers.get(&key).copied()
    }

    fn coprocessor_write(&mut self, register: CoprocessorRegister, value: u32) -> bool {
        if register.coprocessor != 15 {
            return false;
        }
        self.writes.push((register, value));
        true
    }

    fn vector_base(&self) -> u32 {
        self.vector_base
    }
}

struct Rig<const V5: bool> {
    cpu: Core<V5>,
    bus: FlatBus,
}

struct Setup {
    regs: Vec<(usize, u32)>,
    cpsr: u32,
    spsr_svc: u32,
    irq: [u32; 2],
    bus: FlatBus,
}

impl Setup {
    fn new(cpsr: u32) -> Self {
        Self {
            regs: Vec::new(),
            cpsr,
            spsr_svc: 0,
            irq: [0; 2],
            bus: FlatBus::new(),
        }
    }

    fn reg(mut self, index: usize, value: u32) -> Self {
        self.regs.push((index, value));
        self
    }

    fn flags(mut self, flags: u32) -> Self {
        self.cpsr |= flags;
        self
    }

    fn word(mut self, addr: u32, value: u32) -> Self {
        self.bus.set_word(addr, value);
        self
    }

    fn half(mut self, addr: u32, value: u16) -> Self {
        self.bus.put(addr, Width::Half, u32::from(value));
        self
    }

    fn spsr_svc(mut self, value: u32) -> Self {
        self.spsr_svc = value;
        self
    }

    fn irq_bank(mut self, sp: u32, lr: u32) -> Self {
        self.irq = [sp, lr];
        self
    }

    fn arm<const V5: bool>(mut self, program: &[u32]) -> Rig<V5> {
        for (i, word) in program.iter().chain([ARM_NOP; 4].iter()).enumerate() {
            self.bus.set_word(CODE + 4 * i as u32, *word);
        }
        self.build(8, Width::Word)
    }

    fn thumb<const V5: bool>(mut self, program: &[u16]) -> Rig<V5> {
        self.cpsr |= THUMB;
        for (i, half) in program.iter().chain([THUMB_NOP; 4].iter()).enumerate() {
            self.bus
                .put(CODE + 2 * i as u32, Width::Half, u32::from(*half));
        }
        self.build(4, Width::Half)
    }

    fn build<const V5: bool>(self, ahead: u32, width: Width) -> Rig<V5> {
        let mut state = CpuState {
            cpsr: self.cpsr,
            ..CpuState::default()
        };
        state.spsr[1] = self.spsr_svc;
        state.irq = self.irq;
        for (index, value) in self.regs {
            state.r[index] = value;
        }
        let size = ahead / 2;
        state.r[15] = CODE + ahead;
        state.pipeline = [self.bus.get(CODE, width), self.bus.get(CODE + size, width)];
        state.next_fetch = Access::Sequential;
        Rig {
            cpu: Core::<V5>::from_state(&state),
            bus: self.bus,
        }
    }
}

fn system() -> Setup {
    Setup::new(SYSTEM)
}

impl<const V5: bool> Rig<V5> {
    fn step(mut self, count: usize) -> Self {
        for _ in 0..count {
            self.cpu.step(&mut self.bus);
        }
        self
    }

    fn reg(&self, index: usize) -> u32 {
        self.cpu.reg(index)
    }

    fn flags(&self) -> u32 {
        self.cpu.cpsr() & 0xF800_0000
    }
}

fn arm9(setup: Setup, program: &[u32]) -> Rig<true> {
    setup.arm::<true>(program).step(1)
}

fn arm7(setup: Setup, program: &[u32]) -> Rig<false> {
    setup.arm::<false>(program).step(1)
}

fn thumb9(setup: Setup, program: &[u16], steps: usize) -> Rig<true> {
    setup.thumb::<true>(program).step(steps)
}

const CLZ_R0_R1: u32 = 0xE16F_0F11;
const CLZNE_R0_R1: u32 = 0x116F_0F11;

#[test]
fn clz_of_zero_is_32() {
    let rig = arm9(system().reg(0, 7).reg(1, 0), &[CLZ_R0_R1]);
    assert_eq!(rig.reg(0), 32);
}

#[test]
fn clz_counts_the_zero_bits_above_the_highest_set_bit() {
    let rig = arm9(system().reg(1, 0x0000_8000), &[CLZ_R0_R1]);
    assert_eq!(rig.reg(0), 16);
}

#[test]
fn clz_of_a_set_top_bit_is_0() {
    let rig = arm9(system().reg(0, 7).reg(1, 0xFFFF_FFFF), &[CLZ_R0_R1]);
    assert_eq!(rig.reg(0), 0);
}

#[test]
fn clz_leaves_the_flags_alone() {
    let flags = FLAG_N | FLAG_C | FLAG_V;
    let rig = arm9(system().flags(flags).reg(1, 1), &[CLZ_R0_R1]);
    assert_eq!(rig.reg(0), 31);
    assert_eq!(rig.flags(), flags);
}

#[test]
fn clz_obeys_its_condition() {
    let rig = arm9(system().flags(FLAG_Z).reg(0, 7).reg(1, 1), &[CLZNE_R0_R1]);
    assert_eq!(rig.reg(0), 7);
}

const QADD_R0_R1_R2: u32 = 0xE102_0051;
const QSUB_R0_R1_R2: u32 = 0xE122_0051;
const QDADD_R0_R1_R2: u32 = 0xE142_0051;
const QDSUB_R0_R1_R2: u32 = 0xE162_0051;
const MSR_CPSR_F_0: u32 = 0xE328_F000;
const MRS_R0_CPSR: u32 = 0xE10F_0000;

fn q_op(op: u32, rm: u32, rn: u32) -> Rig<true> {
    arm9(system().reg(1, rm).reg(2, rn), &[op])
}

#[test]
fn qadd_without_overflow_adds_and_leaves_q_clear() {
    let rig = q_op(QADD_R0_R1_R2, 1, 2);
    assert_eq!(rig.reg(0), 3);
    assert_eq!(rig.flags(), 0);
}

#[test]
fn qadd_saturates_at_the_largest_positive_value_and_sets_q() {
    let rig = q_op(QADD_R0_R1_R2, 0x7FFF_FFFF, 1);
    assert_eq!(rig.reg(0), 0x7FFF_FFFF);
    assert_eq!(rig.flags(), FLAG_Q);
}

#[test]
fn qadd_saturates_at_the_largest_negative_value_and_sets_q() {
    let rig = q_op(QADD_R0_R1_R2, 0x8000_0000, 0xFFFF_FFFF);
    assert_eq!(rig.reg(0), 0x8000_0000);
    assert_eq!(rig.flags(), FLAG_Q);
}

#[test]
fn qsub_subtracts_rn_from_rm() {
    let rig = q_op(QSUB_R0_R1_R2, 5, 7);
    assert_eq!(rig.reg(0), (-2i32) as u32);
    assert_eq!(rig.flags(), 0);
}

#[test]
fn qsub_saturates_below_the_largest_negative_value() {
    let rig = q_op(QSUB_R0_R1_R2, 0x8000_0000, 1);
    assert_eq!(rig.reg(0), 0x8000_0000);
    assert_eq!(rig.flags(), FLAG_Q);
}

#[test]
fn qdadd_adds_twice_rn() {
    let rig = q_op(QDADD_R0_R1_R2, 1, 0x2000_0000);
    assert_eq!(rig.reg(0), 0x4000_0001);
    assert_eq!(rig.flags(), 0);
}

#[test]
fn qdadd_sets_q_when_the_doubling_saturates() {
    let rig = q_op(QDADD_R0_R1_R2, 0, 0x4000_0000);
    assert_eq!(rig.reg(0), 0x7FFF_FFFF);
    assert_eq!(rig.flags(), FLAG_Q);
}

#[test]
fn qdadd_saturates_the_sum_after_the_doubling() {
    let rig = q_op(QDADD_R0_R1_R2, 0x7000_0000, 0x1000_0000);
    assert_eq!(rig.reg(0), 0x7FFF_FFFF);
    assert_eq!(rig.flags(), FLAG_Q);
}

#[test]
fn qdsub_subtracts_the_saturated_double() {
    let rig = q_op(QDSUB_R0_R1_R2, 0, 0x4000_0000);
    assert_eq!(rig.reg(0), 0x8000_0001);
    assert_eq!(rig.flags(), FLAG_Q);
}

#[test]
fn qdsub_without_overflow_leaves_q_clear() {
    let rig = q_op(QDSUB_R0_R1_R2, 10, 3);
    assert_eq!(rig.reg(0), 4);
    assert_eq!(rig.flags(), 0);
}

#[test]
fn q_stays_set_after_an_operation_that_does_not_saturate() {
    let rig = arm9(system().flags(FLAG_Q).reg(1, 1).reg(2, 2), &[QADD_R0_R1_R2]);
    assert_eq!(rig.reg(0), 3);
    assert_eq!(rig.flags(), FLAG_Q);
}

#[test]
fn saturating_ops_leave_nzcv_alone() {
    let flags = FLAG_Z | FLAG_C;
    let rig = arm9(
        system().flags(flags).reg(1, 0x7FFF_FFFF).reg(2, 1),
        &[QADD_R0_R1_R2],
    );
    assert_eq!(rig.flags(), flags | FLAG_Q);
}

#[test]
fn msr_on_the_flags_clears_q_and_mrs_reads_it() {
    let rig = system()
        .flags(FLAG_Q)
        .arm::<true>(&[MRS_R0_CPSR, MSR_CPSR_F_0])
        .step(2);
    assert_eq!(rig.reg(0) & FLAG_Q, FLAG_Q);
    assert_eq!(rig.flags(), 0);
}

const SMULBB_R0_R1_R2: u32 = 0xE160_0281;
const SMULTB_R0_R1_R2: u32 = 0xE160_02A1;
const SMULBT_R0_R1_R2: u32 = 0xE160_02C1;
const SMULTT_R0_R1_R2: u32 = 0xE160_02E1;
const SMLABB_R0_R1_R2_R3: u32 = 0xE100_3281;
const SMLATT_R0_R1_R2_R3: u32 = 0xE100_32E1;
const SMULWB_R0_R1_R2: u32 = 0xE120_02A1;
const SMULWT_R0_R1_R2: u32 = 0xE120_02E1;
const SMLAWB_R0_R1_R2_R3: u32 = 0xE120_3281;
const SMLAWT_R0_R1_R2_R3: u32 = 0xE120_32C1;
const SMLALBB_R0_R1_R2_R3: u32 = 0xE141_0382;
const SMLALTB_R0_R1_R2_R3: u32 = 0xE141_03A2;

const HALVES_M: u32 = 0x0003_FFFE;
const HALVES_S: u32 = 0x7FFF_0005;

fn multiply(op: u32, m: u32, s: u32, n: u32) -> Rig<true> {
    arm9(system().reg(1, m).reg(2, s).reg(3, n), &[op])
}

#[test]
fn smulbb_multiplies_the_signed_bottom_halves() {
    let rig = multiply(SMULBB_R0_R1_R2, HALVES_M, HALVES_S, 0);
    assert_eq!(rig.reg(0), (-10i32) as u32);
}

#[test]
fn smultb_takes_the_top_half_of_rm() {
    let rig = multiply(SMULTB_R0_R1_R2, HALVES_M, HALVES_S, 0);
    assert_eq!(rig.reg(0), 15);
}

#[test]
fn smulbt_takes_the_top_half_of_rs() {
    let rig = multiply(SMULBT_R0_R1_R2, HALVES_M, HALVES_S, 0);
    assert_eq!(rig.reg(0), (-65534i32) as u32);
}

#[test]
fn smultt_multiplies_the_top_halves() {
    let rig = multiply(SMULTT_R0_R1_R2, HALVES_M, HALVES_S, 0);
    assert_eq!(rig.reg(0), 98301);
}

#[test]
fn smulbb_of_two_largest_negative_halves_is_positive_without_q() {
    let rig = multiply(SMULBB_R0_R1_R2, 0x8000, 0x8000, 0);
    assert_eq!(rig.reg(0), 0x4000_0000);
    assert_eq!(rig.flags(), 0);
}

#[test]
fn smlabb_adds_the_accumulator() {
    let rig = multiply(SMLABB_R0_R1_R2_R3, 2, 3, 10);
    assert_eq!(rig.reg(0), 16);
    assert_eq!(rig.flags(), 0);
}

#[test]
fn smlatt_adds_the_product_of_the_top_halves() {
    let rig = multiply(SMLATT_R0_R1_R2_R3, 0xFFFF_0000, 0x0004_0000, 10);
    assert_eq!(rig.reg(0), 6);
}

#[test]
fn smlabb_wraps_on_overflow_and_sets_q() {
    let rig = multiply(SMLABB_R0_R1_R2_R3, 0x8000, 0x8000, 0x4000_0000);
    assert_eq!(rig.reg(0), 0x8000_0000);
    assert_eq!(rig.flags(), FLAG_Q);
}

#[test]
fn smulwb_keeps_the_top_32_bits_of_the_48_bit_product() {
    let rig = multiply(SMULWB_R0_R1_R2, 0x0001_0000, 3, 0);
    assert_eq!(rig.reg(0), 3);
}

#[test]
fn smulwt_uses_the_top_half_of_rs_and_keeps_the_sign() {
    let rig = multiply(SMULWT_R0_R1_R2, 0xFFFF_0000, 0x0002_0000, 0);
    assert_eq!(rig.reg(0), (-2i32) as u32);
}

#[test]
fn smulwb_rounds_a_negative_fraction_down() {
    let rig = multiply(SMULWB_R0_R1_R2, 0xFFFF_FFFF, 1, 0);
    assert_eq!(rig.reg(0), 0xFFFF_FFFF);
}

#[test]
fn smlawb_adds_the_accumulator() {
    let rig = multiply(SMLAWB_R0_R1_R2_R3, 0x0002_0000, 4, 100);
    assert_eq!(rig.reg(0), 108);
    assert_eq!(rig.flags(), 0);
}

#[test]
fn smlawt_uses_the_top_half_of_rs() {
    let rig = multiply(SMLAWT_R0_R1_R2_R3, 0x0002_0000, 0x0004_0000, 100);
    assert_eq!(rig.reg(0), 108);
}

#[test]
fn smlawb_wraps_on_overflow_and_sets_q() {
    let rig = multiply(SMLAWB_R0_R1_R2_R3, 0x7FFF_FFFF, 0x7FFF, 0x7FFF_FFFF);
    assert_eq!(rig.reg(0), 0xBFFF_7FFE);
    assert_eq!(rig.flags(), FLAG_Q);
}

fn long_multiply(op: u32, lo: u32, hi: u32, m: u32, s: u32) -> Rig<true> {
    arm9(system().reg(0, lo).reg(1, hi).reg(2, m).reg(3, s), &[op])
}

#[test]
fn smlalbb_carries_into_the_high_word() {
    let rig = long_multiply(SMLALBB_R0_R1_R2_R3, 0xFFFF_FFFF, 0, 1, 1);
    assert_eq!((rig.reg(1), rig.reg(0)), (1, 0));
}

#[test]
fn smlalbb_sign_extends_a_negative_product_to_64_bits() {
    let rig = long_multiply(SMLALBB_R0_R1_R2_R3, 0, 0, 0xFFFF, 1);
    assert_eq!((rig.reg(1), rig.reg(0)), (0xFFFF_FFFF, 0xFFFF_FFFF));
}

#[test]
fn smlaltb_takes_the_top_half_of_rm() {
    let rig = long_multiply(SMLALTB_R0_R1_R2_R3, 1, 0, 0x0003_0000, 5);
    assert_eq!((rig.reg(1), rig.reg(0)), (0, 16));
}

#[test]
fn smlalbb_wraps_without_setting_q() {
    let rig = long_multiply(SMLALBB_R0_R1_R2_R3, 0xFFFF_FFFF, 0x7FFF_FFFF, 1, 1);
    assert_eq!((rig.reg(1), rig.reg(0)), (0x8000_0000, 0));
    assert_eq!(rig.flags(), 0);
}

const LDRD_R2_R0_8: u32 = 0xE1C0_20D8;
const STRD_R2_R0_8: u32 = 0xE1C0_20F8;
const LDRD_R2_R0_POST_MINUS_8: u32 = 0xE040_20D8;
const LDRD_R4_R0_R1_WRITEBACK: u32 = 0xE1A0_40D1;
const STRD_R2_R0_POST_R1: u32 = 0xE080_20F1;
const LDRD_R1_R0: u32 = 0xE1C0_10D0;

#[test]
fn ldrd_loads_two_words_into_an_even_odd_pair() {
    let setup = system()
        .reg(0, DATA)
        .word(DATA + 8, 0x1111_1111)
        .word(DATA + 12, 0x2222_2222);
    let rig = arm9(setup, &[LDRD_R2_R0_8]);
    assert_eq!((rig.reg(2), rig.reg(3)), (0x1111_1111, 0x2222_2222));
    assert_eq!(rig.reg(0), DATA);
}

#[test]
fn strd_stores_two_words_from_an_even_odd_pair() {
    let setup = system()
        .reg(0, DATA)
        .reg(2, 0x3333_3333)
        .reg(3, 0x4444_4444);
    let rig = arm9(setup, &[STRD_R2_R0_8]);
    assert_eq!(rig.bus.word(DATA + 8), 0x3333_3333);
    assert_eq!(rig.bus.word(DATA + 12), 0x4444_4444);
}

#[test]
fn ldrd_post_indexed_loads_at_the_base_and_writes_back_the_offset() {
    let setup = system()
        .reg(0, DATA + 8)
        .word(DATA + 8, 5)
        .word(DATA + 12, 6);
    let rig = arm9(setup, &[LDRD_R2_R0_POST_MINUS_8]);
    assert_eq!((rig.reg(2), rig.reg(3)), (5, 6));
    assert_eq!(rig.reg(0), DATA);
}

#[test]
fn ldrd_with_a_register_offset_writes_back_when_asked() {
    let setup = system()
        .reg(0, DATA)
        .reg(1, 0x10)
        .word(DATA + 0x10, 7)
        .word(DATA + 0x14, 8);
    let rig = arm9(setup, &[LDRD_R4_R0_R1_WRITEBACK]);
    assert_eq!((rig.reg(4), rig.reg(5)), (7, 8));
    assert_eq!(rig.reg(0), DATA + 0x10);
}

#[test]
fn strd_post_indexed_by_a_register_writes_back() {
    let setup = system().reg(0, DATA).reg(1, 0x20).reg(2, 9).reg(3, 10);
    let rig = arm9(setup, &[STRD_R2_R0_POST_R1]);
    assert_eq!((rig.bus.word(DATA), rig.bus.word(DATA + 4)), (9, 10));
    assert_eq!(rig.reg(0), DATA + 0x20);
}

#[test]
fn ldrd_with_an_odd_register_is_undefined() {
    let rig = arm9(system().reg(0, DATA), &[LDRD_R1_R0]);
    assert_eq!(rig.cpu.mode(), Some(Mode::Undefined));
    assert_eq!(rig.cpu.pc(), 0x04);
}

#[test]
fn the_ldrd_encoding_on_the_arm7_is_not_a_doubleword_load() {
    let setup = system()
        .reg(0, DATA)
        .reg(2, 0x1234_5678)
        .reg(3, 0x5555_5555)
        .word(DATA + 8, 0x1111_1111)
        .word(DATA + 12, 0x2222_2222);
    let rig = arm7(setup, &[LDRD_R2_R0_8]);
    assert_eq!((rig.reg(2), rig.reg(3)), (0x1234_5678, 0x5555_5555));
}

const BLX_PLUS_8: u32 = 0xFA00_0002;
const BLX_PLUS_10: u32 = 0xFB00_0002;
const BLX_R1: u32 = 0xE12F_FF31;
const BLX_LR: u32 = 0xE12F_FF3E;
const BLXEQ_R1: u32 = 0x012F_FF31;
const BX_R1: u32 = 0xE12F_FF11;

#[test]
fn blx_immediate_links_and_switches_to_thumb() {
    let rig = arm9(system(), &[BLX_PLUS_8]);
    assert!(rig.cpu.thumb());
    assert_eq!(rig.reg(14), CODE + 4);
    assert_eq!(rig.cpu.pc(), CODE + 0x10);
}

#[test]
fn blx_immediate_with_h_set_lands_on_the_halfword() {
    let rig = arm9(system(), &[BLX_PLUS_10]);
    assert!(rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), CODE + 0x12);
}

#[test]
fn blx_immediate_on_the_arm7_is_never_executed() {
    let rig = arm7(system().reg(14, 0xAAAA), &[BLX_PLUS_8]);
    assert!(!rig.cpu.thumb());
    assert_eq!(rig.reg(14), 0xAAAA);
    assert_eq!(rig.cpu.pc(), CODE + 4);
}

#[test]
fn blx_register_to_an_odd_address_links_and_enters_thumb() {
    let rig = arm9(system().reg(1, DATA | 1), &[BLX_R1]);
    assert!(rig.cpu.thumb());
    assert_eq!(rig.reg(14), CODE + 4);
    assert_eq!(rig.cpu.pc(), DATA);
}

#[test]
fn blx_register_to_an_even_address_stays_in_arm() {
    let rig = arm9(system().reg(1, DATA), &[BLX_R1]);
    assert!(!rig.cpu.thumb());
    assert_eq!(rig.reg(14), CODE + 4);
    assert_eq!(rig.cpu.pc(), DATA);
}

#[test]
fn blx_lr_branches_to_the_old_link_value() {
    let rig = arm9(system().reg(14, DATA), &[BLX_LR]);
    assert_eq!(rig.cpu.pc(), DATA);
    assert_eq!(rig.reg(14), CODE + 4);
}

#[test]
fn blx_register_obeys_its_condition() {
    let rig = arm9(system().reg(1, DATA).reg(14, 0xAAAA), &[BLXEQ_R1]);
    assert_eq!(rig.reg(14), 0xAAAA);
    assert_eq!(rig.cpu.pc(), CODE + 4);
}

#[test]
fn bx_on_the_arm9_does_not_link() {
    let rig = arm9(system().reg(1, DATA).reg(14, 0xAAAA), &[BX_R1]);
    assert_eq!(rig.reg(14), 0xAAAA);
    assert_eq!(rig.cpu.pc(), DATA);
}

const THUMB_BL_PREFIX_0: u16 = 0xF000;
const THUMB_BLX_SUFFIX_FC: u16 = 0xE87E;
const THUMB_BLX_SUFFIX_FE: u16 = 0xE87F;
const THUMB_BL_SUFFIX_FC: u16 = 0xF87E;
const THUMB_BLX_R1: u16 = 0x4788;
const THUMB_BX_R1: u16 = 0x4708;

#[test]
fn thumb_blx_pair_links_and_switches_to_arm() {
    let rig = thumb9(system(), &[THUMB_BL_PREFIX_0, THUMB_BLX_SUFFIX_FC], 2);
    assert!(!rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), CODE + 0x100);
    assert_eq!(rig.reg(14), (CODE + 4) | 1);
}

#[test]
fn thumb_blx_pair_aligns_the_target_to_a_word() {
    let rig = thumb9(system(), &[THUMB_BL_PREFIX_0, THUMB_BLX_SUFFIX_FE], 2);
    assert!(!rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), CODE + 0x100);
}

#[test]
fn thumb_bl_pair_on_the_arm9_stays_in_thumb() {
    let rig = thumb9(system(), &[THUMB_BL_PREFIX_0, THUMB_BL_SUFFIX_FC], 2);
    assert!(rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), CODE + 0x100);
    assert_eq!(rig.reg(14), (CODE + 4) | 1);
}

#[test]
fn thumb_blx_suffix_on_the_arm7_is_undefined() {
    let rig = system()
        .thumb::<false>(&[THUMB_BL_PREFIX_0, THUMB_BLX_SUFFIX_FC])
        .step(2);
    assert_eq!(rig.cpu.mode(), Some(Mode::Undefined));
}

#[test]
fn thumb_blx_register_links_and_follows_bit_0() {
    let rig = thumb9(system().reg(1, DATA), &[THUMB_BLX_R1], 1);
    assert!(!rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), DATA);
    assert_eq!(rig.reg(14), (CODE + 2) | 1);
}

#[test]
fn thumb_bx_on_the_arm9_does_not_link() {
    let rig = thumb9(system().reg(1, DATA).reg(14, 0xAAAA), &[THUMB_BX_R1], 1);
    assert_eq!(rig.reg(14), 0xAAAA);
    assert_eq!(rig.cpu.pc(), DATA);
}

const LDR_PC_R0: u32 = 0xE590_F000;
const LDMIA_R0_PC: u32 = 0xE890_8000;
const LDMIA_R0_PC_USER: u32 = 0xE8D0_8000;
const THUMB_POP_PC: u16 = 0xBD00;

#[test]
fn ldr_into_pc_with_bit_0_set_enters_thumb() {
    let setup = system().reg(0, DATA).word(DATA, (DATA + 0x40) | 1);
    let rig = arm9(setup, &[LDR_PC_R0]);
    assert!(rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), DATA + 0x40);
}

#[test]
fn ldr_into_pc_with_bit_0_clear_stays_in_arm() {
    let setup = system().reg(0, DATA).word(DATA, DATA + 0x40);
    let rig = arm9(setup, &[LDR_PC_R0]);
    assert!(!rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), DATA + 0x40);
}

#[test]
fn ldr_into_pc_on_the_arm7_stays_in_arm_with_bit_0_set() {
    let setup = system().reg(0, DATA).word(DATA, (DATA + 0x40) | 1);
    let rig = arm7(setup, &[LDR_PC_R0]);
    assert!(!rig.cpu.thumb());
    assert_eq!(rig.cpu.pc() & !3, DATA + 0x40);
}

#[test]
fn ldm_into_pc_with_bit_0_set_enters_thumb() {
    let setup = system().reg(0, DATA).word(DATA, (DATA + 0x40) | 1);
    let rig = arm9(setup, &[LDMIA_R0_PC]);
    assert!(rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), DATA + 0x40);
}

#[test]
fn ldm_into_pc_with_the_s_bit_takes_the_state_from_spsr() {
    let setup = Setup::new(SUPERVISOR)
        .spsr_svc(SYSTEM | THUMB)
        .reg(0, DATA)
        .word(DATA, DATA + 0x40);
    let rig = arm9(setup, &[LDMIA_R0_PC_USER]);
    assert_eq!(rig.cpu.mode(), Some(Mode::System));
    assert!(rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), DATA + 0x40);
}

#[test]
fn thumb_pop_pc_with_bit_0_clear_enters_arm() {
    let setup = system().reg(13, DATA).word(DATA, DATA + 0x40);
    let rig = thumb9(setup, &[THUMB_POP_PC], 1);
    assert!(!rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), DATA + 0x40);
    assert_eq!(rig.reg(13), DATA + 4);
}

#[test]
fn thumb_pop_pc_with_bit_0_set_stays_in_thumb() {
    let setup = system().reg(13, DATA).word(DATA, (DATA + 0x40) | 1);
    let rig = thumb9(setup, &[THUMB_POP_PC], 1);
    assert!(rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), DATA + 0x40);
}

#[test]
fn thumb_pop_pc_on_the_arm7_stays_in_thumb() {
    let setup = system().reg(13, DATA).word(DATA, DATA + 0x40);
    let rig = setup.thumb::<false>(&[THUMB_POP_PC]).step(1);
    assert!(rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), DATA + 0x40);
}

const LDMIA_R0_WB_R0_R1: u32 = 0xE8B0_0003;
const LDMIA_R1_WB_R0_R1: u32 = 0xE8B1_0003;
const LDMIA_R0_WB_R0: u32 = 0xE8B0_0001;
const STMIA_R1_WB_R0_R1: u32 = 0xE8A1_0003;

fn two_words() -> Setup {
    system().word(DATA, 0x1111).word(DATA + 4, 0x2222)
}

#[test]
fn ldm_writeback_wins_when_the_base_is_not_the_last_register() {
    let rig = arm9(two_words().reg(0, DATA), &[LDMIA_R0_WB_R0_R1]);
    assert_eq!(rig.reg(0), DATA + 8);
    assert_eq!(rig.reg(1), 0x2222);
}

#[test]
fn ldm_loaded_value_wins_when_the_base_is_the_last_register() {
    let rig = arm9(two_words().reg(1, DATA), &[LDMIA_R1_WB_R0_R1]);
    assert_eq!(rig.reg(0), 0x1111);
    assert_eq!(rig.reg(1), 0x2222);
}

#[test]
fn ldm_writeback_wins_when_the_base_is_the_only_register() {
    let rig = arm9(two_words().reg(0, DATA), &[LDMIA_R0_WB_R0]);
    assert_eq!(rig.reg(0), DATA + 4);
}

#[test]
fn ldm_on_the_arm7_lets_the_loaded_base_win() {
    let rig = arm7(two_words().reg(0, DATA), &[LDMIA_R0_WB_R0_R1]);
    assert_eq!(rig.reg(0), 0x1111);
}

#[test]
fn stm_with_writeback_stores_the_old_base_when_it_is_not_first() {
    let rig = arm9(system().reg(0, 0x77).reg(1, DATA), &[STMIA_R1_WB_R0_R1]);
    assert_eq!(rig.bus.word(DATA + 4), DATA);
    assert_eq!(rig.reg(1), DATA + 8);
}

#[test]
fn stm_on_the_arm7_stores_the_new_base_when_it_is_not_first() {
    let rig = arm7(system().reg(0, 0x77).reg(1, DATA), &[STMIA_R1_WB_R0_R1]);
    assert_eq!(rig.bus.word(DATA + 4), DATA + 8);
}

const LDMIA_R0_WB_NONE: u32 = 0xE8B0_0000;
const STMIA_R0_WB_NONE: u32 = 0xE8A0_0000;
const LDMIA_R13_WB_R14_USER: u32 = 0xE8FD_4000;
const IRQ: u32 = 0x12;

#[test]
fn ldm_with_an_empty_list_loads_nothing_and_moves_the_base_by_0x40() {
    let rig = arm9(two_words().reg(0, DATA), &[LDMIA_R0_WB_NONE]);
    assert_eq!(rig.reg(0), DATA + 0x40);
    assert_eq!(rig.cpu.pc(), CODE + 4);
}

#[test]
fn stm_with_an_empty_list_stores_nothing_and_moves_the_base_by_0x40() {
    let rig = arm9(two_words().reg(0, DATA), &[STMIA_R0_WB_NONE]);
    assert_eq!(rig.reg(0), DATA + 0x40);
    assert_eq!(rig.bus.word(DATA), 0x1111);
}

#[test]
fn a_user_bank_ldm_writes_the_base_back_to_the_mode_bank() {
    let setup = Setup::new(IRQ)
        .word(DATA, 0x1111)
        .reg(13, 0x77)
        .irq_bank(DATA, 0);
    let state = arm9(setup, &[LDMIA_R13_WB_R14_USER]).cpu.state();
    assert_eq!(state.irq, [DATA + 4, 0]);
    assert_eq!([state.r[13], state.r[14]], [0x77, 0x1111]);
}

const STMDB_R13_R13_R14_USER: u32 = 0xE94D_6000;

fn banked_base_stm() -> Setup {
    Setup::new(IRQ)
        .reg(13, 0x77)
        .reg(14, 0x88)
        .irq_bank(DATA + 8, 0)
}

#[test]
fn a_user_bank_stm_stores_the_user_copy_of_a_banked_base() {
    let nine = arm9(banked_base_stm(), &[STMDB_R13_R13_R14_USER]);
    assert_eq!([nine.bus.word(DATA), nine.bus.word(DATA + 4)], [0x77, 0x88]);
    let seven = arm7(banked_base_stm(), &[STMDB_R13_R13_R14_USER]);
    assert_eq!(
        [seven.bus.word(DATA), seven.bus.word(DATA + 4)],
        [0x77, 0x88]
    );
}

const LDRH_R1_R0: u32 = 0xE1D0_10B0;
const LDRSH_R1_R0: u32 = 0xE1D0_10F0;

fn odd_half() -> Setup {
    system().reg(0, DATA + 1).half(DATA, 0x80F0)
}

#[test]
fn ldrh_from_an_odd_address_reads_the_aligned_halfword() {
    let rig = arm9(odd_half(), &[LDRH_R1_R0]);
    assert_eq!(rig.reg(1), 0x80F0);
}

#[test]
fn ldrh_from_an_odd_address_on_the_arm7_rotates() {
    let rig = arm7(odd_half(), &[LDRH_R1_R0]);
    assert_eq!(rig.reg(1), 0xF000_0080);
}

#[test]
fn ldrsh_from_an_odd_address_sign_extends_the_aligned_halfword() {
    let rig = arm9(odd_half(), &[LDRSH_R1_R0]);
    assert_eq!(rig.reg(1), 0xFFFF_80F0);
}

#[test]
fn ldrsh_from_an_odd_address_on_the_arm7_sign_extends_the_high_byte() {
    let rig = arm7(odd_half(), &[LDRSH_R1_R0]);
    assert_eq!(rig.reg(1), 0xFFFF_FF80);
}

const MULS_R0_R1_R2: u32 = 0xE010_0291;
const UMULLS_R0_R1_R2_R3: u32 = 0xE091_0392;
const THUMB_MUL_R0_R1: u16 = 0x4348;

#[test]
fn muls_leaves_the_carry_set() {
    let rig = arm9(system().flags(FLAG_C).reg(1, 3).reg(2, 4), &[MULS_R0_R1_R2]);
    assert_eq!(rig.reg(0), 12);
    assert_eq!(rig.flags(), FLAG_C);
}

#[test]
fn muls_leaves_the_carry_clear() {
    let rig = arm9(
        system().reg(1, 0xFFFF_FFFF).reg(2, 0xFFFF_FFFF),
        &[MULS_R0_R1_R2],
    );
    assert_eq!(rig.reg(0), 1);
    assert_eq!(rig.flags(), 0);
}

#[test]
fn umulls_leaves_the_carry_and_sets_n_and_z_from_64_bits() {
    let rig = arm9(
        system().flags(FLAG_C).reg(2, 0x8000_0000).reg(3, 2),
        &[UMULLS_R0_R1_R2_R3],
    );
    assert_eq!((rig.reg(1), rig.reg(0)), (1, 0));
    assert_eq!(rig.flags(), FLAG_C);
}

#[test]
fn thumb_mul_leaves_the_carry() {
    let rig = thumb9(
        system().flags(FLAG_C).reg(0, 0).reg(1, 5),
        &[THUMB_MUL_R0_R1],
        1,
    );
    assert_eq!(rig.reg(0), 0);
    assert_eq!(rig.flags(), FLAG_Z | FLAG_C);
}

const BKPT_0: u32 = 0xE120_0070;
const THUMB_BKPT_0: u16 = 0xBE00;
const SWI_0: u32 = 0xEF00_0000;

#[test]
fn bkpt_enters_abort_at_the_prefetch_abort_vector() {
    let rig = arm9(system().flags(FLAG_N), &[BKPT_0]);
    assert_eq!(rig.cpu.mode(), Some(Mode::Abort));
    assert_eq!(rig.cpu.pc(), 0x0C);
    assert_eq!(rig.reg(14), CODE + 4);
    assert_eq!(rig.cpu.cpsr() & (1 << 7), 1 << 7);
}

#[test]
fn thumb_bkpt_enters_abort_in_arm_state() {
    let rig = thumb9(system(), &[THUMB_BKPT_0], 1);
    assert_eq!(rig.cpu.mode(), Some(Mode::Abort));
    assert!(!rig.cpu.thumb());
    assert_eq!(rig.cpu.pc(), 0x0C);
    assert_eq!(rig.reg(14), CODE + 4);
}

#[test]
fn thumb_bkpt_on_the_arm7_is_undefined() {
    let rig = system().thumb::<false>(&[THUMB_BKPT_0]).step(1);
    assert_eq!(rig.cpu.mode(), Some(Mode::Undefined));
}

#[test]
fn exceptions_jump_to_the_vector_base_the_bus_gives() {
    let mut setup = system();
    setup.bus.vector_base = 0xFFFF_0000;
    let rig = arm9(setup, &[SWI_0]);
    assert_eq!(rig.cpu.mode(), Some(Mode::Supervisor));
    assert_eq!(rig.cpu.pc(), 0xFFFF_0008);
}

#[test]
fn the_arm7_ignores_the_vector_base() {
    let mut setup = system();
    setup.bus.vector_base = 0xFFFF_0000;
    let rig = arm7(setup, &[SWI_0]);
    assert_eq!(rig.cpu.pc(), 0x08);
}

const MRC_P15_R0_C0_C0_0: u32 = 0xEE10_0F10;
const MRC_P15_R0_C9_C1_1: u32 = 0xEE19_0F31;
const MRC_P15_R15_C0_C0_0: u32 = 0xEE10_FF10;
const MCR_P15_R1_C1_C0_0: u32 = 0xEE01_1F10;
const MRC_P14_R0_C0_C0_0: u32 = 0xEE10_0E10;
const MCR_P14_R0_C0_C0_0: u32 = 0xEE00_0E10;

fn cp15(crn: u32, crm: u32, opcode2: u32) -> CoprocessorRegister {
    CoprocessorRegister {
        coprocessor: 15,
        opcode1: 0,
        crn,
        crm,
        opcode2,
    }
}

#[test]
fn mrc_reads_the_register_the_bus_answers() {
    let mut setup = system();
    setup.bus.registers.insert((0, 0, 0, 0), MAIN_ID);
    let rig = arm9(setup, &[MRC_P15_R0_C0_C0_0]);
    assert_eq!(rig.reg(0), MAIN_ID);
    assert_eq!(rig.bus.reads, [cp15(0, 0, 0)]);
    assert_eq!(rig.cpu.mode(), Some(Mode::System));
}

#[test]
fn mrc_passes_crn_crm_and_opcode2_to_the_bus() {
    let mut setup = system();
    setup.bus.registers.insert((9, 1, 0, 1), 0x0080_000A);
    let rig = arm9(setup, &[MRC_P15_R0_C9_C1_1]);
    assert_eq!(rig.reg(0), 0x0080_000A);
    assert_eq!(rig.bus.reads, [cp15(9, 1, 1)]);
}

#[test]
fn mrc_into_r15_sets_only_the_flags() {
    let mut setup = system();
    setup.bus.registers.insert((0, 0, 0, 0), 0xA000_00FF);
    let rig = arm9(setup, &[MRC_P15_R15_C0_C0_0]);
    assert_eq!(rig.cpu.cpsr(), 0xA000_0000 | SYSTEM);
    assert_eq!(rig.cpu.pc(), CODE + 4);
}

#[test]
fn mcr_hands_the_register_and_value_to_the_bus() {
    let rig = arm9(system().reg(1, 0x0005_2078), &[MCR_P15_R1_C1_C0_0]);
    assert_eq!(rig.bus.writes, [(cp15(1, 0, 0), 0x0005_2078)]);
    assert_eq!(rig.cpu.mode(), Some(Mode::System));
}

#[test]
fn mrc_the_bus_does_not_answer_is_undefined() {
    let rig = arm9(system(), &[MRC_P14_R0_C0_C0_0]);
    assert_eq!(rig.cpu.mode(), Some(Mode::Undefined));
    assert_eq!(rig.reg(14), CODE + 4);
}

#[test]
fn mcr_the_bus_refuses_is_undefined() {
    let rig = arm9(system(), &[MCR_P14_R0_C0_C0_0]);
    assert_eq!(rig.cpu.mode(), Some(Mode::Undefined));
}

#[test]
fn the_arm7_never_asks_the_bus_for_a_coprocessor() {
    let rig = arm7(system(), &[MRC_P15_R0_C0_C0_0]);
    assert!(rig.bus.reads.is_empty());
    assert_eq!(rig.cpu.mode(), Some(Mode::Undefined));
}

const PLD_R0: u32 = 0xF5D0_F000;
const UNCONDITIONAL_UNDEFINED: u32 = 0xF000_0000;

#[test]
fn pld_changes_nothing() {
    let rig = arm9(system().reg(0, DATA), &[PLD_R0]);
    assert_eq!(rig.cpu.mode(), Some(Mode::System));
    assert_eq!(rig.cpu.pc(), CODE + 4);
    assert_eq!(rig.reg(0), DATA);
}

#[test]
fn other_unconditional_instructions_are_undefined() {
    let rig = arm9(system(), &[UNCONDITIONAL_UNDEFINED]);
    assert_eq!(rig.cpu.mode(), Some(Mode::Undefined));
}

#[test]
fn the_aliases_name_both_variants() {
    let arm7: Cpu = Cpu::default();
    let arm9: Arm9 = Arm9::default();
    assert_eq!(arm7.state(), arm9.state());
}

const MRC_P15_R0_C1_C0_0: u32 = 0xEE11_0F10;
const MCR_P15_R1_C9_C1_0: u32 = 0xEE09_1F11;
const MRC_P15_R0_C9_C1_0: u32 = 0xEE19_0F11;
const MCR_P15_R0_C7_C0_4: u32 = 0xEE07_0F90;
const MCR_P15_R0_C7_C8_2: u32 = 0xEE07_0F58;
const MCR_P15_R0_C7_C5_0: u32 = 0xEE07_0F15;
const DISASSEMBLY: &str = include_str!("fixtures/armv5te-disasm.txt");

fn arm_line(op: u32) -> String {
    format!("{CODE:08X} {op:08X} {}", disasm::arm_v5(CODE, op).text())
}

fn thumb_line(ops: &[u16]) -> String {
    let words: Vec<String> = ops.iter().map(|op| format!("{op:04X}")).collect();
    let decoded = disasm::thumb_v5(CODE, ops[0], ops.get(1).copied());
    format!("{CODE:08X} {} {}", words.join(" "), decoded.text())
}

#[test]
fn the_cases_disassemble_to_the_committed_list() {
    let arm = [
        ARM_NOP,
        SWI_0,
        CLZ_R0_R1,
        CLZNE_R0_R1,
        QADD_R0_R1_R2,
        QSUB_R0_R1_R2,
        QDADD_R0_R1_R2,
        QDSUB_R0_R1_R2,
        MSR_CPSR_F_0,
        MRS_R0_CPSR,
        SMULBB_R0_R1_R2,
        SMULTB_R0_R1_R2,
        SMULBT_R0_R1_R2,
        SMULTT_R0_R1_R2,
        SMLABB_R0_R1_R2_R3,
        SMLATT_R0_R1_R2_R3,
        SMULWB_R0_R1_R2,
        SMULWT_R0_R1_R2,
        SMLAWB_R0_R1_R2_R3,
        SMLAWT_R0_R1_R2_R3,
        SMLALBB_R0_R1_R2_R3,
        SMLALTB_R0_R1_R2_R3,
        LDRD_R2_R0_8,
        STRD_R2_R0_8,
        LDRD_R2_R0_POST_MINUS_8,
        LDRD_R4_R0_R1_WRITEBACK,
        STRD_R2_R0_POST_R1,
        LDRD_R1_R0,
        BLX_PLUS_8,
        BLX_PLUS_10,
        BLX_R1,
        BLX_LR,
        BLXEQ_R1,
        BX_R1,
        LDR_PC_R0,
        LDMIA_R0_PC,
        LDMIA_R0_PC_USER,
        LDMIA_R0_WB_R0_R1,
        LDMIA_R1_WB_R0_R1,
        LDMIA_R0_WB_R0,
        STMIA_R1_WB_R0_R1,
        LDRH_R1_R0,
        LDRSH_R1_R0,
        MULS_R0_R1_R2,
        UMULLS_R0_R1_R2_R3,
        BKPT_0,
        MRC_P15_R0_C0_C0_0,
        MRC_P15_R0_C9_C1_1,
        MRC_P15_R15_C0_C0_0,
        MCR_P15_R1_C1_C0_0,
        MRC_P14_R0_C0_C0_0,
        MCR_P14_R0_C0_C0_0,
        MRC_P15_R0_C1_C0_0,
        MCR_P15_R1_C9_C1_0,
        MRC_P15_R0_C9_C1_0,
        MCR_P15_R0_C7_C0_4,
        MCR_P15_R0_C7_C8_2,
        MCR_P15_R0_C7_C5_0,
        PLD_R0,
        UNCONDITIONAL_UNDEFINED,
    ];
    let thumb: [&[u16]; 9] = [
        &[THUMB_NOP],
        &[THUMB_BL_PREFIX_0, THUMB_BLX_SUFFIX_FC],
        &[THUMB_BL_PREFIX_0, THUMB_BLX_SUFFIX_FE],
        &[THUMB_BL_PREFIX_0, THUMB_BL_SUFFIX_FC],
        &[THUMB_BLX_R1],
        &[THUMB_BX_R1],
        &[THUMB_POP_PC],
        &[THUMB_MUL_R0_R1],
        &[THUMB_BKPT_0],
    ];
    let mut lines: Vec<String> = arm.into_iter().map(arm_line).collect();
    lines.extend(thumb.into_iter().map(thumb_line));
    assert_eq!(lines, DISASSEMBLY.lines().collect::<Vec<_>>());
}
