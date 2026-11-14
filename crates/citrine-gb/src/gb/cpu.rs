use crate::gb::GbModel;
use crate::gb::bus::{CpuBusInterface, HaltedRun};
use crate::gb::ic::ICInterface;
use crate::instructions::{Cond, Instruction, R8, R16, R16Mem, R16Stk};
use crate::utils::bit::{
    add_bytes, add_bytes_carry, add_word_signed_byte, add_words, get_bit, hi, lo,
    rotate_left_get_carry, rotate_left_through_carry, rotate_right_get_carry,
    rotate_right_through_carry, set_bit, sub_bytes, sub_bytes_carry, word,
};

#[cfg(not(feature = "debug"))]
pub trait Bus: CpuBusInterface + ICInterface {}
#[cfg(not(feature = "debug"))]
impl<T: CpuBusInterface + ICInterface> Bus for T {}
#[cfg(feature = "debug")]
pub trait Bus: CpuBusInterface + ICInterface + crate::debug::DebuggerInterface {}
#[cfg(feature = "debug")]
impl<T: CpuBusInterface + ICInterface + crate::debug::DebuggerInterface> Bus for T {}

const SPEED_SWITCH_HALT_CYCLES: u32 = 0x20000 / 4;
const HALT_SAMPLE_DOT: u8 = 2;

macro_rules! by_opcode {
    ($cpu:expr, $bus:expr, $decode:path $(, $special:literal => $body:expr)?) => {
        by_opcode!(@arms $cpu, $bus, $decode; $($special => $body)?;
            0x00 0x01 0x02 0x03 0x04 0x05 0x06 0x07 0x08 0x09 0x0A 0x0B 0x0C 0x0D 0x0E 0x0F
            0x10 0x11 0x12 0x13 0x14 0x15 0x16 0x17 0x18 0x19 0x1A 0x1B 0x1C 0x1D 0x1E 0x1F
            0x20 0x21 0x22 0x23 0x24 0x25 0x26 0x27 0x28 0x29 0x2A 0x2B 0x2C 0x2D 0x2E 0x2F
            0x30 0x31 0x32 0x33 0x34 0x35 0x36 0x37 0x38 0x39 0x3A 0x3B 0x3C 0x3D 0x3E 0x3F
            0x40 0x41 0x42 0x43 0x44 0x45 0x46 0x47 0x48 0x49 0x4A 0x4B 0x4C 0x4D 0x4E 0x4F
            0x50 0x51 0x52 0x53 0x54 0x55 0x56 0x57 0x58 0x59 0x5A 0x5B 0x5C 0x5D 0x5E 0x5F
            0x60 0x61 0x62 0x63 0x64 0x65 0x66 0x67 0x68 0x69 0x6A 0x6B 0x6C 0x6D 0x6E 0x6F
            0x70 0x71 0x72 0x73 0x74 0x75 0x76 0x77 0x78 0x79 0x7A 0x7B 0x7C 0x7D 0x7E 0x7F
            0x80 0x81 0x82 0x83 0x84 0x85 0x86 0x87 0x88 0x89 0x8A 0x8B 0x8C 0x8D 0x8E 0x8F
            0x90 0x91 0x92 0x93 0x94 0x95 0x96 0x97 0x98 0x99 0x9A 0x9B 0x9C 0x9D 0x9E 0x9F
            0xA0 0xA1 0xA2 0xA3 0xA4 0xA5 0xA6 0xA7 0xA8 0xA9 0xAA 0xAB 0xAC 0xAD 0xAE 0xAF
            0xB0 0xB1 0xB2 0xB3 0xB4 0xB5 0xB6 0xB7 0xB8 0xB9 0xBA 0xBB 0xBC 0xBD 0xBE 0xBF
            0xC0 0xC1 0xC2 0xC3 0xC4 0xC5 0xC6 0xC7 0xC8 0xC9 0xCA 0xCB 0xCC 0xCD 0xCE 0xCF
            0xD0 0xD1 0xD2 0xD3 0xD4 0xD5 0xD6 0xD7 0xD8 0xD9 0xDA 0xDB 0xDC 0xDD 0xDE 0xDF
            0xE0 0xE1 0xE2 0xE3 0xE4 0xE5 0xE6 0xE7 0xE8 0xE9 0xEA 0xEB 0xEC 0xED 0xEE 0xEF
            0xF0 0xF1 0xF2 0xF3 0xF4 0xF5 0xF6 0xF7 0xF8 0xF9 0xFA 0xFB 0xFC 0xFD 0xFE 0xFF
        )
    };
    (@arms $cpu:expr, $bus:expr, $decode:path; $($special:literal => $body:expr)?; $($opcode:literal)*) => {
        match $cpu.ir {
            $($special => $body,)?
            $(
                #[allow(unreachable_patterns)]
                $opcode => (
                    $cpu.execute($bus, const { $decode($opcode) }),
                    const { $decode($opcode).may_halt() },
                ),
            )*
        }
    };
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cpu {
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub f: Flags,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    pub ir: u8,
    pub ime: bool,
    pub ime_next: bool,
    pub halted: bool,
    pub model: GbModel,
    pub invalid_opcode: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub speed_switch_halt: u32,
}

impl Cpu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_dmg(header_checksum: u8) -> Self {
        let flags = if header_checksum == 0x00 {
            Flags {
                zero: true,
                ..Default::default()
            }
        } else {
            Flags {
                zero: true,
                subtract: false,
                half_carry: true,
                carry: true,
            }
        };

        Self {
            a: 0x01,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            f: flags,
            h: 0x01,
            l: 0x4D,
            sp: 0xFFFE,
            pc: 0x0100,
            ir: 0x00,
            ime: false,
            ime_next: false,
            halted: false,
            model: GbModel::Dmg,
            invalid_opcode: false,
            speed_switch_halt: 0,
        }
    }

    pub fn new_cgb() -> Self {
        let flags = Flags {
            zero: true,
            ..Default::default()
        };

        Self {
            a: 0x11,
            b: 0x00,
            c: 0x00,
            d: 0xFF,
            e: 0x56,
            f: flags,
            h: 0x00,
            l: 0x0D,
            sp: 0xFFFE,
            pc: 0x100,
            ir: 0x00,
            ime: false,
            ime_next: false,
            halted: false,
            model: GbModel::Cgb,
            invalid_opcode: false,
            speed_switch_halt: 0,
        }
    }

    pub fn new_with_boot_rom(model: GbModel) -> Self {
        Self {
            model,
            ..Default::default()
        }
    }

    fn halted_cycles<B: Bus, const BOUNDED: bool>(&mut self, bus: &mut B, budget: u32) {
        let mut left = budget;
        if !B::LINKED {
            loop {
                let mut skip = self.speed_switch_halt == 0 && (!BOUNDED || left > 1);
                if self.speed_switch_halt == 0 {
                    match bus.run_halted::<BOUNDED>(&mut left, HALT_SAMPLE_DOT, self.model.is_cgb())
                    {
                        HaltedRun::Unhandled => {}
                        HaltedRun::Noisy | HaltedRun::Plugged => skip = false,
                        HaltedRun::Woke => {
                            self.halted = false;
                            bus.halt_ended();
                            return;
                        }
                        HaltedRun::Stopped => return,
                    }
                }
                if skip {
                    let skipped = bus.skip_halted(left - 1);
                    if BOUNDED {
                        left -= skipped;
                    }
                }
                self.halted_cycle(bus);
                if BOUNDED {
                    left = left.saturating_sub(1);
                }
                if !self.halted || (BOUNDED && left == 0) || !bus.keep_halting() {
                    return;
                }
            }
        }
        let mut plugged = false;
        loop {
            let mut skip = self.speed_switch_halt == 0 && (!BOUNDED || left > 1) && !plugged;
            if skip {
                match bus.run_halted::<BOUNDED>(&mut left, HALT_SAMPLE_DOT, self.model.is_cgb()) {
                    HaltedRun::Unhandled => {}
                    HaltedRun::Noisy => skip = false,
                    HaltedRun::Plugged => {
                        skip = false;
                        plugged = true;
                    }
                    HaltedRun::Woke => {
                        self.halted = false;
                        bus.halt_ended();
                        return;
                    }
                    HaltedRun::Stopped => return,
                }
            }
            if skip {
                let skipped = bus.skip_halted(left - 1);
                if BOUNDED {
                    left -= skipped;
                }
            }
            self.halted_cycle(bus);
            if BOUNDED {
                left = left.saturating_sub(1);
            }
            if !self.halted || (BOUNDED && left == 0) || !bus.keep_halting() {
                return;
            }
        }
    }

    fn halted_cycle<B: Bus>(&mut self, bus: &mut B) {
        let pending = if !B::LINKED {
            let pending_before = bus.has_pending_interrupt();
            let pending = bus
                .cycle_halted(HALT_SAMPLE_DOT)
                .unwrap_or_else(|| bus.has_pending_interrupt());
            if self.model.is_cgb() {
                pending_before
            } else {
                pending
            }
        } else if self.model.is_cgb() {
            let pending_before = bus.has_pending_interrupt();
            bus.cycle_halted(HALT_SAMPLE_DOT);
            pending_before
        } else {
            bus.cycle_halted(HALT_SAMPLE_DOT)
                .unwrap_or_else(|| bus.has_pending_interrupt())
        };
        if self.speed_switch_halt > 0 {
            self.speed_switch_halt -= 1;
            if self.speed_switch_halt == 0 {
                self.halted = false;
            }
        }
        if pending {
            self.halted = false;
            self.speed_switch_halt = 0;
        }
        if !self.halted {
            bus.halt_ended();
        }
    }

    pub fn step(&mut self, bus: &mut impl Bus) {
        self.step_halting(bus, 1);
    }

    pub fn step_halting(&mut self, bus: &mut impl Bus, budget: u32) {
        self.step_inline(bus, budget);
    }

    pub fn run_until<B: Bus>(&mut self, bus: &mut B, mut done: impl FnMut(&mut B, bool) -> bool) {
        loop {
            if self.invalid_opcode {
                bus.cycle();
            } else if self.halted {
                self.halted_cycles::<B, false>(bus, u32::MAX);
            } else {
                loop {
                    let may_halt = self.run_instruction::<false>(bus);
                    if may_halt && (self.halted || self.invalid_opcode) {
                        break;
                    }
                    if done(bus, !may_halt) {
                        return;
                    }
                }
            }
            if done(bus, false) {
                return;
            }
        }
    }

    #[inline(always)]
    fn step_inline(&mut self, bus: &mut impl Bus, budget: u32) {
        if self.invalid_opcode {
            bus.cycle();
            return;
        }

        if self.halted {
            self.halted_cycles::<_, true>(bus, budget);
            return;
        }

        self.run_instruction::<true>(bus);
    }

    #[inline(always)]
    fn run_instruction<const SERVICE_HDMA: bool>(&mut self, bus: &mut impl Bus) -> bool {
        self.interrupt_handler(bus);

        #[cfg(feature = "debug")]
        bus.on_instruction(self);

        let (fetch_next, may_halt) = by_opcode!(self, bus, Instruction::decode, 0xCB => {
            self.fetch(bus);
            by_opcode!(self, bus, Instruction::decode_prefixed)
        });
        if fetch_next {
            self.fetch(bus);
            if SERVICE_HDMA {
                bus.service_hdma();
            }
        } else if SERVICE_HDMA && !may_halt {
            bus.service_hdma();
        }
        may_halt
    }

    #[inline(always)]
    fn execute<B: Bus>(&mut self, bus: &mut B, instruction: Instruction) -> bool {
        match instruction {
            Instruction::NOP => {}
            Instruction::LD_rr_nn(r16) => self.ld_rr_nn(bus, r16),
            Instruction::LD_rr_A(r16mem) => self.ld_rr_a(bus, r16mem),
            Instruction::LD_A_rr(r16mem) => self.ld_a_rr(bus, r16mem),
            Instruction::LD_nn_SP => self.ld_nn_sp(bus),
            Instruction::INC_rr(r16) => self.inc_rr(bus, r16),
            Instruction::DEC_rr(r16) => self.dec_rr(bus, r16),
            Instruction::ADD_HL_rr(r16) => self.add_hl_rr(bus, r16),
            Instruction::INC_r(r8) => self.inc_r(bus, r8),
            Instruction::DEC_r(r8) => self.dec_r(bus, r8),
            Instruction::LD_r_n(r8) if !matches!(r8, R8::HL) => {
                return self.operand_then_fetch(bus, |cpu, bus, value| cpu.set_r8(bus, r8, value));
            }
            Instruction::LD_r_n(r8) => self.ld_r_n(bus, r8),
            Instruction::RLCA => self.rlca(),
            Instruction::RRCA => self.rrca(),
            Instruction::RLA => self.rla(),
            Instruction::RRA => self.rra(),
            Instruction::DAA => self.daa(),
            Instruction::CPL => self.cpl(),
            Instruction::SCF => self.scf(),
            Instruction::CCF => self.ccf(),
            Instruction::JR_n => return self.jr_then_fetch(bus),
            Instruction::JR_c_n(cond) if B::LINKED => self.jr_c_n(bus, cond),
            Instruction::JR_c_n(cond) => {
                if !self.f.cond_true(cond) {
                    return self.operand_then_fetch(bus, |_, _, _| {});
                }
                return self.jr_then_fetch(bus);
            }
            Instruction::STOP => {
                self.stop(bus);
                self.fetch_serviced(bus);
                return false;
            }
            Instruction::HALT | Instruction::LD_r_r(R8::HL, R8::HL) => {
                self.fetch(bus);
                if bus.has_pending_interrupt() {
                    self.pc = self.pc.wrapping_sub(1);
                } else {
                    self.halted = true;
                }
                bus.service_hdma();
                return false;
            }
            Instruction::LD_r_r(dest, src) => self.ld_r_r(bus, dest, src),
            Instruction::ADD_r(r8) => self.add_r(bus, r8),
            Instruction::ADC_r(r8) => self.adc_r(bus, r8),
            Instruction::SUB_r(r8) => self.sub_r(bus, r8),
            Instruction::SBC_r(r8) => self.sbc_r(bus, r8),
            Instruction::AND_r(r8) => self.and_r(bus, r8),
            Instruction::XOR_r(r8) => self.xor_r(bus, r8),
            Instruction::OR_r(r8) => self.or_r(bus, r8),
            Instruction::CP_r(r8) => self.cp_r(bus, r8),
            Instruction::ADD_n => return self.operand_then_fetch(bus, |cpu, _, n| cpu.add_a(n)),
            Instruction::ADC_n => return self.operand_then_fetch(bus, |cpu, _, n| cpu.adc_a(n)),
            Instruction::SUB_n => return self.operand_then_fetch(bus, |cpu, _, n| cpu.sub_a(n)),
            Instruction::SBC_n => return self.operand_then_fetch(bus, |cpu, _, n| cpu.sbc_a(n)),
            Instruction::AND_n => return self.operand_then_fetch(bus, |cpu, _, n| cpu.and_a(n)),
            Instruction::XOR_n => return self.operand_then_fetch(bus, |cpu, _, n| cpu.xor_a(n)),
            Instruction::OR_n => return self.operand_then_fetch(bus, |cpu, _, n| cpu.or_a(n)),
            Instruction::CP_n => return self.operand_then_fetch(bus, |cpu, _, n| cpu.cp_a(n)),
            Instruction::POP(stk) if B::LINKED => self.pop_rr(bus, stk),
            Instruction::POP(stk) => {
                let value = self.pop_word_and_fetch(bus);
                self.set_r16stk(stk, value);
                return false;
            }
            Instruction::PUSH(stk) => self.push_rr(bus, stk),
            Instruction::RET if B::LINKED => self.ret(bus),
            Instruction::RET => {
                self.return_and_fetch(bus);
                return false;
            }
            Instruction::RETI if B::LINKED => self.reti(bus),
            Instruction::RETI => {
                self.return_and_fetch(bus);
                self.ime = true;
                self.ime_next = true;
                return false;
            }
            Instruction::RET_c(cond) if B::LINKED => self.ret_c(bus, cond),
            Instruction::RET_c(cond) => {
                bus.cycle();
                if self.f.cond_true(cond) {
                    self.return_and_fetch(bus);
                    return false;
                }
            }
            Instruction::JP_nn => self.jump_nn(bus),
            Instruction::JP_c_nn(cond) => self.jump_c_nn(bus, cond),
            Instruction::JP_HL => self.jump_hl(),
            Instruction::CALL_nn => self.call_nn(bus),
            Instruction::CALL_c_nn(cond) => self.call_c_nn(bus, cond),
            Instruction::RST_n(tgt) => self.rst(bus, tgt),
            Instruction::LDH_C_A => self.ldh_c_a(bus),
            Instruction::LDH_A_C => self.ldh_a_c(bus),
            Instruction::LDH_n_A => self.ldh_n_a(bus),
            Instruction::LDH_A_n => self.ldh_a_n(bus),
            Instruction::LD_nn_A => self.ld_nn_a(bus),
            Instruction::LD_A_nn => self.ld_a_nn(bus),
            Instruction::ADD_SP_n => self.add_sp_n(bus),
            Instruction::LD_HL_SP_n => self.ld_hl_sp_n(bus),
            Instruction::LD_SP_HL => self.ld_sp_hl(bus),
            Instruction::DI => {
                self.ime_next = false;
                self.ime = false;
            }
            Instruction::EI => self.ime_next = true,
            Instruction::RLC_r(r8) => self.rlc_r(bus, r8),
            Instruction::RRC_r(r8) => self.rrc_r(bus, r8),
            Instruction::RL_r(r8) => self.rl_r(bus, r8),
            Instruction::RR_r(r8) => self.rr_r(bus, r8),
            Instruction::SLA_r(r8) => self.sla_r(bus, r8),
            Instruction::SRA_r(r8) => self.sra_r(bus, r8),
            Instruction::SWAP_r(r8) => self.swap_r(bus, r8),
            Instruction::SRL_r(r8) => self.srl_r(bus, r8),
            Instruction::BIT_r(index, r8) => self.bit_r(bus, r8, index),
            Instruction::RES_r(index, r8) => self.res_r(bus, r8, index),
            Instruction::SET_r(index, r8) => self.set_r(bus, r8, index),
            Instruction::Invalid(_) => {
                self.invalid_opcode = true;
                self.fetch_serviced(bus);
                return false;
            }
        }
        true
    }

    #[cold]
    #[inline(never)]
    fn fetch_serviced(&mut self, bus: &mut impl Bus) {
        self.fetch(bus);
        bus.service_hdma();
    }

    fn stop(&mut self, bus: &mut impl Bus) {
        if !bus.speed_switch(self.ime) || bus.has_pending_interrupt() {
            return;
        }
        self.read_program(bus);
        self.halted = true;
        self.speed_switch_halt = SPEED_SWITCH_HALT_CYCLES;
    }

    pub fn enter_dmg_compatibility(&mut self, header: &[u8]) {
        let byte = |addr: usize| header.get(addr).copied().unwrap_or(0);
        let nintendo = byte(0x14B) == 0x01
            || (byte(0x14B) == 0x33 && byte(0x144) == b'0' && byte(0x145) == b'1');
        let b = if nintendo {
            (0x134..=0x143).fold(0u8, |sum, addr| sum.wrapping_add(byte(addr)))
        } else {
            0x00
        };
        let hl = if b == 0x43 || b == 0x58 {
            0x991A
        } else {
            0x007C
        };

        self.b = b;
        self.c = 0x00;
        self.d = 0x00;
        self.e = 0x08;
        self.h = hi(hl);
        self.l = lo(hl);
    }

    #[cfg(feature = "debug")]
    pub fn trace_registers(&self) -> [u16; 5] {
        let pair = |hi: u8, lo: u8| u16::from_be_bytes([hi, lo]);
        [
            pair(self.a, u8::from(self.f)),
            pair(self.b, self.c),
            pair(self.d, self.e),
            pair(self.h, self.l),
            self.sp,
        ]
    }

    pub fn fetch(&mut self, bus: &mut impl Bus) {
        #[cfg(feature = "debug")]
        bus.on_fetch(self.pc);

        self.ir = self.read_program(bus);
    }

    pub fn decode(&mut self, bus: &mut impl Bus) -> Instruction {
        if self.ir != 0xCB {
            Instruction::decode(self.ir)
        } else {
            self.fetch(bus);
            Instruction::decode_prefixed(self.ir)
        }
    }

    #[inline(never)]
    fn dispatch_interrupt(&mut self, bus: &mut impl Bus) {
        if self.dispatch_interrupt_quiet(bus) {
            return;
        }
        self.dispatch_interrupt_open(bus);
    }

    #[inline(never)]
    fn dispatch_interrupt_open(&mut self, bus: &mut impl Bus) {
        self.ime = false;
        self.ime_next = false;

        bus.cycle();
        bus.cycle_with_address(self.sp);

        let return_address = self.pc.wrapping_sub(1);
        let enabled = if bus.push_quiet(self.sp, return_address) {
            self.sp = self.sp.wrapping_sub(2);
            bus.enabled_interrupts()
        } else {
            self.push(bus, hi(return_address));
            let enabled = bus.enabled_interrupts();
            self.push(bus, lo(return_address));
            enabled
        };
        let interrupt = bus.next_interrupt(enabled);
        self.pc = interrupt.map_or(0x0000, |interrupt| interrupt.vector());
        if let Some(interrupt) = interrupt {
            bus.acknowledge_in_next_cycle(interrupt);
        }

        self.fetch(bus);
    }

    #[inline(always)]
    fn dispatch_interrupt_quiet(&mut self, bus: &mut impl Bus) -> bool {
        let Some((vector, ir)) = bus.dispatch_quiet(self.sp, self.pc.wrapping_sub(1)) else {
            return false;
        };
        self.ime = false;
        self.ime_next = false;
        self.sp = self.sp.wrapping_sub(2);
        #[cfg(feature = "debug")]
        bus.on_fetch(vector);
        self.pc = vector.wrapping_add(1);
        self.ir = ir;
        true
    }

    #[inline(always)]
    fn interrupt_handler(&mut self, bus: &mut impl Bus) {
        if self.ime {
            if bus.has_pending_interrupt() {
                self.dispatch_interrupt(bus);
            }
            return;
        }

        // EI is delayed by one instruction
        self.ime = self.ime_next;
    }

    pub fn soft_reset(&mut self, header_checksum: u8) {
        match self.model {
            GbModel::Dmg => *self = Self::new_dmg(header_checksum),
            GbModel::Cgb => *self = Self::new_cgb(),
        }
    }
}

// Program helpers
impl Cpu {
    pub fn read_program(&mut self, bus: &mut impl Bus) -> u8 {
        let byte = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        byte
    }

    #[inline(always)]
    pub fn read_program_lh<B: Bus>(&mut self, bus: &mut B) -> (u8, u8) {
        if B::LINKED {
            return self.read_program_pair(bus);
        }
        if let Some(value) = bus.read_pair_quiet(self.pc) {
            self.pc = self.pc.wrapping_add(2);
            return (lo(value), hi(value));
        }
        (self.read_program(bus), self.read_program(bus))
    }

    #[inline(always)]
    pub fn read_program_nn<B: Bus>(&mut self, bus: &mut B) -> u16 {
        if B::LINKED {
            let (low, high) = self.read_program_pair(bus);
            return word(low, high);
        }
        if let Some(value) = bus.read_pair_quiet(self.pc) {
            self.pc = self.pc.wrapping_add(2);
            return value;
        }
        self.read_program_nn_open(bus)
    }

    #[inline(never)]
    fn read_program_nn_open(&mut self, bus: &mut impl Bus) -> u16 {
        word(self.read_program(bus), self.read_program(bus))
    }

    #[inline(always)]
    fn read_program_pair(&mut self, bus: &mut impl Bus) -> (u8, u8) {
        let pair = bus.read_pair_open(self.pc);
        self.pc = self.pc.wrapping_add(2);
        pair
    }

    #[inline(always)]
    fn operand_then_fetch<B: Bus>(
        &mut self,
        bus: &mut B,
        apply: impl FnOnce(&mut Self, &mut B, u8),
    ) -> bool {
        if let Some(pair) = bus.read_pair_quiet(self.pc) {
            apply(self, bus, lo(pair));
            #[cfg(feature = "debug")]
            bus.on_fetch(self.pc.wrapping_add(1));
            self.ir = hi(pair);
            self.pc = self.pc.wrapping_add(2);
            return false;
        }
        let value = self.read_program(bus);
        apply(self, bus, value);
        true
    }

    #[inline(always)]
    fn jr_then_fetch(&mut self, bus: &mut impl Bus) -> bool {
        if let Some((target, ir)) = bus.jump_relative_quiet(self.pc) {
            #[cfg(feature = "debug")]
            bus.on_fetch(target);
            self.pc = target.wrapping_add(1);
            self.ir = ir;
            return false;
        }
        self.jr_n(bus);
        true
    }
}

// Stack helpers
impl Cpu {
    pub fn pop(&mut self, bus: &mut impl Bus) -> u8 {
        let value = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        value
    }

    #[inline(never)]
    pub fn pop_word<B: Bus>(&mut self, bus: &mut B) -> u16 {
        if B::LINKED {
            let (low, high) = bus.pop_pair_open(self.sp);
            self.sp = self.sp.wrapping_add(2);
            return word(low, high);
        }
        if let Some(value) = bus.pop_quiet(self.sp) {
            self.sp = self.sp.wrapping_add(2);
            return value;
        }
        self.pop_word_open(bus)
    }

    #[inline(never)]
    fn pop_word_and_fetch(&mut self, bus: &mut impl Bus) -> u16 {
        if let Some((value, ir)) = bus.pop_and_fetch_quiet(self.sp, self.pc) {
            self.sp = self.sp.wrapping_add(2);
            let Some(ir) = ir else {
                return self.fetch_returning(bus, value);
            };
            #[cfg(feature = "debug")]
            bus.on_fetch(self.pc);
            self.ir = ir;
            self.pc = self.pc.wrapping_add(1);
            return value;
        }
        if let Some(value) = bus.pop_quiet(self.sp) {
            self.sp = self.sp.wrapping_add(2);
            return self.fetch_returning(bus, value);
        }
        self.pop_word_open_then_fetch(bus)
    }

    #[cold]
    #[inline(never)]
    fn fetch_returning(&mut self, bus: &mut impl Bus, value: u16) -> u16 {
        self.fetch(bus);
        value
    }

    #[cold]
    #[inline(never)]
    fn pop_word_open_then_fetch(&mut self, bus: &mut impl Bus) -> u16 {
        let low = bus.read_increment(self.sp);
        self.sp = self.sp.wrapping_add(1);
        let value = word(low, self.pop(bus));
        self.fetch(bus);
        value
    }

    #[inline(never)]
    fn return_and_fetch(&mut self, bus: &mut impl Bus) {
        if let Some((address, ir)) = bus.return_and_fetch_quiet(self.sp) {
            self.sp = self.sp.wrapping_add(2);
            let Some(ir) = ir else {
                return self.fetch_at(bus, address);
            };
            #[cfg(feature = "debug")]
            bus.on_fetch(address);
            self.pc = address.wrapping_add(1);
            self.ir = ir;
            return;
        }
        self.return_open_then_fetch(bus);
    }

    #[inline(never)]
    fn return_open_then_fetch(&mut self, bus: &mut impl Bus) {
        let address = if let Some(value) = bus.pop_quiet(self.sp) {
            self.sp = self.sp.wrapping_add(2);
            value
        } else {
            self.pop_word_open(bus)
        };
        bus.cycle();
        self.pc = address;
        self.fetch(bus);
    }

    #[cold]
    #[inline(never)]
    fn fetch_at(&mut self, bus: &mut impl Bus, address: u16) {
        self.pc = address;
        self.fetch(bus);
    }

    #[cold]
    #[inline(never)]
    fn pop_word_open(&mut self, bus: &mut impl Bus) -> u16 {
        let low = bus.read_increment(self.sp);
        self.sp = self.sp.wrapping_add(1);
        word(low, self.pop(bus))
    }

    pub fn push(&mut self, bus: &mut impl Bus, value: u8) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, value);
    }

    #[inline(never)]
    pub fn push_word<B: Bus>(&mut self, bus: &mut B, value: u16) {
        if B::LINKED {
            bus.push_pair_open(self.sp, value);
            self.sp = self.sp.wrapping_sub(2);
            return;
        }
        if bus.push_quiet(self.sp, value) {
            self.sp = self.sp.wrapping_sub(2);
            return;
        }
        self.push_word_open(bus, value);
    }

    #[cold]
    #[inline(never)]
    fn push_word_open(&mut self, bus: &mut impl Bus, value: u16) {
        self.push(bus, hi(value));
        self.push(bus, lo(value));
    }
}

// Instruction execution
impl Cpu {
    #[inline(always)]
    pub fn ld_rr_nn(&mut self, bus: &mut impl Bus, r16: R16) {
        let (low, high) = self.read_program_lh(bus);
        self.set_r16_lh(r16, low, high);
    }

    pub fn ld_rr_a(&mut self, bus: &mut impl Bus, r16_mem: R16Mem) {
        let address = self.get_r16mem(r16_mem);
        bus.write(address, self.a);
    }

    #[inline(always)]
    pub fn ld_a_rr(&mut self, bus: &mut impl Bus, r16_mem: R16Mem) {
        let address = self.get_r16mem(r16_mem);
        self.a = match r16_mem {
            R16Mem::HLinc | R16Mem::HLdec => bus.read_increment(address),
            _ => bus.read(address),
        };
    }

    pub fn ld_nn_sp(&mut self, bus: &mut impl Bus) {
        let address = self.read_program_nn(bus);
        bus.write_word(address, self.sp);
    }

    pub fn inc_rr(&mut self, bus: &mut impl Bus, r16: R16) {
        bus.cycle_with_address(self.get_r16_nn(r16));

        self.set_r16_nn(r16, self.get_r16_nn(r16).wrapping_add(1));
    }

    pub fn dec_rr(&mut self, bus: &mut impl Bus, r16: R16) {
        bus.cycle_with_address(self.get_r16_nn(r16));

        self.set_r16_nn(r16, self.get_r16_nn(r16).wrapping_sub(1));
    }

    #[inline(always)]
    pub fn add_hl_rr(&mut self, bus: &mut impl Bus, r16: R16) {
        bus.cycle();

        let (result, hc, c) = add_words(self.get_r16_nn(R16::HL), self.get_r16_nn(r16));
        self.set_r16_nn(R16::HL, result);
        self.f.subtract = false;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn inc_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let (result, hc, _) = add_bytes(self.get_r8(bus, r8), 1);
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = hc;
    }

    pub fn dec_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let (result, hc, _) = sub_bytes(self.get_r8(bus, r8), 1);
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = true;
        self.f.half_carry = hc;
    }

    #[inline(always)]
    pub fn ld_r_n(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.read_program(bus);
        self.set_r8(bus, r8, value);
    }

    pub fn rlca(&mut self) {
        let (result, c) = rotate_left_get_carry(self.a);
        self.a = result;
        self.f.carry = c;
        self.f.zero = false;
        self.f.subtract = false;
        self.f.half_carry = false;
    }

    pub fn rrca(&mut self) {
        let (result, c) = rotate_right_get_carry(self.a);
        self.a = result;
        self.f.carry = c;
        self.f.zero = false;
        self.f.subtract = false;
        self.f.half_carry = false;
    }

    pub fn rla(&mut self) {
        let (result, c) = rotate_left_through_carry(self.a, self.f.carry);
        self.a = result;
        self.f.carry = c;
        self.f.zero = false;
        self.f.subtract = false;
        self.f.half_carry = false;
    }

    pub fn rra(&mut self) {
        let (result, c) = rotate_right_through_carry(self.a, self.f.carry);
        self.a = result;
        self.f.carry = c;
        self.f.zero = false;
        self.f.subtract = false;
        self.f.half_carry = false;
    }

    pub fn daa(&mut self) {
        let mut carry = self.f.carry;
        let mut adjust: u8 = 0;

        let result = if self.f.subtract {
            if self.f.half_carry {
                adjust += 0x06;
            }
            if self.f.carry {
                adjust += 0x60;
            }
            self.a.wrapping_sub(adjust)
        } else {
            if self.f.half_carry || self.a & 0x0F > 0x09 {
                adjust += 0x06;
            }
            if self.f.carry || self.a > 0x99 {
                adjust += 0x60;
                carry = true;
            }
            self.a.wrapping_add(adjust)
        };

        self.a = result;
        self.f.carry = carry;
        self.f.zero = result == 0;
        self.f.half_carry = false;
    }

    pub fn cpl(&mut self) {
        self.a = !self.a;
        self.f.subtract = true;
        self.f.half_carry = true;
    }

    pub fn scf(&mut self) {
        self.f.carry = true;
        self.f.subtract = false;
        self.f.half_carry = false;
    }

    pub fn ccf(&mut self) {
        self.f.carry = !self.f.carry;
        self.f.subtract = false;
        self.f.half_carry = false;
    }

    #[inline(always)]
    pub fn jr_n(&mut self, bus: &mut impl Bus) {
        let offset = if let Some(offset) = bus.read_program_byte_and_cycle(self.pc) {
            self.pc = self.pc.wrapping_add(1);
            offset as i8
        } else {
            let offset = self.read_program(bus) as i8;
            bus.cycle();
            offset
        };
        let (new_pc, _, _) = add_word_signed_byte(self.pc, offset);

        self.pc = new_pc;
    }

    #[inline(always)]
    pub fn jr_c_n<B: Bus>(&mut self, bus: &mut B, cond: Cond) {
        if B::LINKED {
            let offset = self.read_program(bus) as i8;
            if self.f.cond_true(cond) {
                bus.cycle();
                let (new_pc, _, _) = add_word_signed_byte(self.pc, offset);
                self.pc = new_pc;
            }
            return;
        }
        if self.f.cond_true(cond) {
            self.jr_n(bus);
        } else {
            self.read_program(bus);
        }
    }

    #[inline(always)]
    pub fn ld_r_r(&mut self, bus: &mut impl Bus, dest: R8, src: R8) {
        let value = self.get_r8(bus, src);
        self.set_r8(bus, dest, value);
    }

    pub fn add_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.add_a(value);
    }

    pub fn adc_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.adc_a(value);
    }

    pub fn sub_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.sub_a(value);
    }

    pub fn sbc_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.sbc_a(value);
    }

    pub fn and_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.and_a(value);
    }

    pub fn xor_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.xor_a(value);
    }

    #[inline(always)]
    pub fn or_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.or_a(value);
    }

    pub fn cp_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.cp_a(value);
    }

    pub fn add_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.add_a(value);
    }

    pub fn adc_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.adc_a(value);
    }

    pub fn sub_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.sub_a(value);
    }

    pub fn sbc_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.sbc_a(value);
    }

    pub fn and_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.and_a(value);
    }

    pub fn xor_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.xor_a(value);
    }

    pub fn or_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.or_a(value);
    }

    #[inline(always)]
    pub fn cp_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.cp_a(value);
    }

    #[inline(always)]
    pub fn pop_rr(&mut self, bus: &mut impl Bus, r16stk: R16Stk) {
        let value = self.pop_word(bus);
        self.set_r16stk(r16stk, value);
    }

    #[inline(always)]
    pub fn push_rr(&mut self, bus: &mut impl Bus, r16stk: R16Stk) {
        // Internal cycle: CPU computes decremented SP before first write
        bus.cycle_with_address(self.sp);

        let value = self.get_r16stk(r16stk);
        self.push_word(bus, value);
    }

    #[inline(always)]
    pub fn ret(&mut self, bus: &mut impl Bus) {
        let address = self.pop_word(bus);
        bus.cycle();

        self.pc = address;
    }

    pub fn reti(&mut self, bus: &mut impl Bus) {
        self.ret(bus);
        self.ime = true;
        self.ime_next = true;
    }

    #[inline(always)]
    pub fn ret_c(&mut self, bus: &mut impl Bus, cond: Cond) {
        bus.cycle();

        if self.f.cond_true(cond) {
            self.ret(bus);
        }
    }

    #[inline(always)]
    pub fn jump_nn(&mut self, bus: &mut impl Bus) {
        let address = self.read_program_nn(bus);

        bus.cycle();
        self.pc = address;
    }

    #[inline(always)]
    pub fn jump_c_nn(&mut self, bus: &mut impl Bus, cond: Cond) {
        let address = self.read_program_nn(bus);

        if self.f.cond_true(cond) {
            bus.cycle();
            self.pc = address;
        }
    }

    pub fn jump_hl(&mut self) {
        self.pc = self.get_r16_nn(R16::HL);
    }

    #[inline(always)]
    pub fn call_nn(&mut self, bus: &mut impl Bus) {
        let address = self.read_program_nn(bus);

        bus.cycle_with_address(self.sp);
        self.push_word(bus, self.pc);

        self.pc = address;
    }

    pub fn call_c_nn(&mut self, bus: &mut impl Bus, cond: Cond) {
        let address = self.read_program_nn(bus);

        if self.f.cond_true(cond) {
            bus.cycle_with_address(self.sp);
            self.push_word(bus, self.pc);
            self.pc = address;
        }
    }

    pub fn rst(&mut self, bus: &mut impl Bus, address_lsb: u8) {
        bus.cycle_with_address(self.sp);
        self.push_word(bus, self.pc);
        self.pc = word(address_lsb, 0x00);
    }

    pub fn ldh_c_a(&mut self, bus: &mut impl Bus) {
        bus.write(word(self.c, 0xFF), self.a);
    }

    pub fn ldh_a_c(&mut self, bus: &mut impl Bus) {
        self.a = bus.read(word(self.c, 0xFF));
    }

    #[inline(always)]
    pub fn ldh_n_a(&mut self, bus: &mut impl Bus) {
        let address_low = self.read_program(bus);
        bus.write(word(address_low, 0xFF), self.a);
    }

    #[inline(always)]
    pub fn ldh_a_n(&mut self, bus: &mut impl Bus) {
        let address_low = self.read_program(bus);
        self.a = bus.read(word(address_low, 0xFF));
    }

    #[inline(always)]
    pub fn ld_nn_a(&mut self, bus: &mut impl Bus) {
        let address_low = self.read_program(bus);
        let address_high = self.read_program(bus);
        bus.write(word(address_low, address_high), self.a);
    }

    #[inline(always)]
    pub fn ld_a_nn(&mut self, bus: &mut impl Bus) {
        let address_low = self.read_program(bus);
        let address_high = self.read_program(bus);
        self.a = bus.read(word(address_low, address_high));
    }

    pub fn add_sp_n(&mut self, bus: &mut impl Bus) {
        let offset = self.read_program(bus) as i8;

        bus.cycle();
        let (result, hc, c) = add_word_signed_byte(self.sp, offset);

        bus.cycle();
        self.sp = result;
        self.f.zero = false;
        self.f.subtract = false;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn ld_hl_sp_n(&mut self, bus: &mut impl Bus) {
        let offset = self.read_program(bus) as i8;

        bus.cycle();
        let (result, hc, c) = add_word_signed_byte(self.sp, offset);

        self.set_r16_nn(R16::HL, result);
        self.f.zero = false;
        self.f.subtract = false;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn ld_sp_hl(&mut self, bus: &mut impl Bus) {
        bus.cycle_with_address(self.get_r16_nn(R16::HL));
        self.sp = self.get_r16_nn(R16::HL);
    }

    pub fn rlc_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let (result, c) = rotate_left_get_carry(self.get_r8(bus, r8));
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = c;
    }

    pub fn rrc_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let (result, c) = rotate_right_get_carry(self.get_r8(bus, r8));
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = c;
    }

    pub fn rl_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let (result, c) = rotate_left_through_carry(self.get_r8(bus, r8), self.f.carry);
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = c;
    }

    pub fn rr_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let (result, c) = rotate_right_through_carry(self.get_r8(bus, r8), self.f.carry);
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = c;
    }

    pub fn sla_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        let carry = get_bit(value, 7);
        let result = value << 1;

        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = carry;
    }

    pub fn sra_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        let carry = get_bit(value, 0);
        // Shift right while persisting the sign bit
        let result = set_bit(value >> 1, 7, get_bit(value, 7));

        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = carry;
    }

    pub fn swap_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        let result = value.rotate_right(4);

        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = false;
    }

    pub fn srl_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        let carry = get_bit(value, 0);
        let result = value >> 1;

        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = carry;
    }

    pub fn bit_r(&mut self, bus: &mut impl Bus, r8: R8, index: u8) {
        let value = self.get_r8(bus, r8);
        self.f.zero = !get_bit(value, (index & 0b111) as usize);
        self.f.subtract = false;
        self.f.half_carry = true;
    }

    pub fn set_r(&mut self, bus: &mut impl Bus, r8: R8, index: u8) {
        let value = self.get_r8(bus, r8);
        self.set_r8(bus, r8, set_bit(value, (index & 0b111) as usize, true));
    }

    pub fn res_r(&mut self, bus: &mut impl Bus, r8: R8, index: u8) {
        let value = self.get_r8(bus, r8);
        self.set_r8(bus, r8, set_bit(value, (index & 0b111) as usize, false));
    }
}

// 8-Bit Arithmetics
impl Cpu {
    pub fn add_a(&mut self, value: u8) {
        let (result, hc, c) = add_bytes(self.a, value);
        self.a = result;
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn adc_a(&mut self, value: u8) {
        let (result, hc, c) = add_bytes_carry(self.a, value, self.f.carry);
        self.a = result;
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn sub_a(&mut self, value: u8) {
        let (result, hc, c) = sub_bytes(self.a, value);
        self.a = result;
        self.f.zero = result == 0;
        self.f.subtract = true;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn sbc_a(&mut self, value: u8) {
        let (result, hc, c) = sub_bytes_carry(self.a, value, self.f.carry);
        self.a = result;
        self.f.zero = result == 0;
        self.f.subtract = true;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn and_a(&mut self, value: u8) {
        self.a &= value;
        self.f.zero = self.a == 0;
        self.f.subtract = false;
        self.f.half_carry = true;
        self.f.carry = false;
    }

    pub fn xor_a(&mut self, value: u8) {
        self.a ^= value;
        self.f.zero = self.a == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = false;
    }

    pub fn or_a(&mut self, value: u8) {
        self.a |= value;
        self.f.zero = self.a == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = false;
    }

    pub fn cp_a(&mut self, value: u8) {
        let (result, hc, c) = sub_bytes(self.a, value);
        self.f.zero = result == 0;
        self.f.subtract = true;
        self.f.half_carry = hc;
        self.f.carry = c;
    }
}

// Register helpers
impl Cpu {
    #[inline(always)]
    pub fn get_r8(&self, bus: &mut impl Bus, r8: R8) -> u8 {
        match r8 {
            R8::B => self.b,
            R8::C => self.c,
            R8::D => self.d,
            R8::E => self.e,
            R8::H => self.h,
            R8::L => self.l,
            R8::HL => bus.read(self.get_r16_nn(R16::HL)),
            R8::A => self.a,
        }
    }

    #[inline(always)]
    pub fn set_r8(&mut self, bus: &mut impl Bus, r8: R8, value: u8) {
        match r8 {
            R8::B => self.b = value,
            R8::C => self.c = value,
            R8::D => self.d = value,
            R8::E => self.e = value,
            R8::H => self.h = value,
            R8::L => self.l = value,
            R8::HL => bus.write(self.get_r16_nn(R16::HL), value),
            R8::A => self.a = value,
        }
    }

    pub fn set_r16_lh(&mut self, r16: R16, low: u8, high: u8) {
        match r16 {
            R16::BC => {
                self.c = low;
                self.b = high;
            }
            R16::DE => {
                self.e = low;
                self.d = high;
            }
            R16::HL => {
                self.l = low;
                self.h = high;
            }
            R16::SP => self.sp = word(low, high),
        }
    }

    pub fn set_r16_nn(&mut self, r16: R16, value: u16) {
        self.set_r16_lh(r16, lo(value), hi(value))
    }

    pub fn get_r16_nn(&self, r16: R16) -> u16 {
        match r16 {
            R16::BC => word(self.c, self.b),
            R16::DE => word(self.e, self.d),
            R16::HL => word(self.l, self.h),
            R16::SP => self.sp,
        }
    }

    pub fn get_r16mem(&mut self, r16mem: R16Mem) -> u16 {
        match r16mem {
            R16Mem::BC => word(self.c, self.b),
            R16Mem::DE => word(self.e, self.d),
            R16Mem::HLinc => {
                let value = word(self.l, self.h);
                self.set_r16_nn(R16::HL, value.wrapping_add(1));
                value
            }
            R16Mem::HLdec => {
                let value = word(self.l, self.h);
                self.set_r16_nn(R16::HL, value.wrapping_sub(1));
                value
            }
        }
    }

    pub fn get_r16stk(&mut self, r16stk: R16Stk) -> u16 {
        match r16stk {
            R16Stk::BC => word(self.c, self.b),
            R16Stk::DE => word(self.e, self.d),
            R16Stk::HL => word(self.l, self.h),
            R16Stk::AF => word(self.f.into(), self.a),
        }
    }

    pub fn set_r16stk(&mut self, r16stk: R16Stk, value: u16) {
        match r16stk {
            R16Stk::BC => self.set_r16_nn(R16::BC, value),
            R16Stk::DE => self.set_r16_nn(R16::DE, value),
            R16Stk::HL => self.set_r16_nn(R16::HL, value),
            R16Stk::AF => {
                self.f = Flags::from(lo(value));
                self.a = hi(value);
            }
        }
    }
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Flags {
    /// Z = Set to true if the result of the operation is equal to 0
    pub zero: bool,
    /// N = Set to true if the operation was a subtraction
    pub subtract: bool,
    /// H = Set to true if there was an overflow from the lower 4 bits to the upper 4 bits
    pub half_carry: bool,
    /// C = Set to true if the operation resulted in an overflow
    pub carry: bool,
}

impl Flags {
    pub fn cond_true(&self, cond: Cond) -> bool {
        match cond {
            Cond::NZ => !self.zero,
            Cond::Z => self.zero,
            Cond::NC => !self.carry,
            Cond::C => self.carry,
        }
    }
}

static FLAGS_BY_HIGH_NIBBLE: [Flags; 16] = {
    let mut table = [Flags {
        zero: false,
        subtract: false,
        half_carry: false,
        carry: false,
    }; 16];
    let mut nibble = 0;
    while nibble < 16 {
        table[nibble] = Flags {
            zero: nibble & 0b1000 != 0,
            subtract: nibble & 0b0100 != 0,
            half_carry: nibble & 0b0010 != 0,
            carry: nibble & 0b0001 != 0,
        };
        nibble += 1;
    }
    table
};

impl From<Flags> for u8 {
    fn from(value: Flags) -> Self {
        let bytes = u32::from_le_bytes([
            value.zero.into(),
            value.subtract.into(),
            value.half_carry.into(),
            value.carry.into(),
        ]);
        (bytes.wrapping_mul(0x8040_2010) >> 24) as u8
    }
}

impl From<u8> for Flags {
    fn from(value: u8) -> Self {
        FLAGS_BY_HIGH_NIBBLE[usize::from(value >> 4)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gb::bus::CpuBusInterface;
    use crate::gb::cpu::Cpu;
    use crate::gb::ic::ICInterface;
    use crate::gb::ic::Interrupt;

    struct WakeBus {
        mem: Vec<u8>,
        cycles: u32,
        raised_in_cycle: u32,
    }

    impl WakeBus {
        fn raised(&self) -> bool {
            self.cycles >= self.raised_in_cycle
        }
    }

    impl CpuBusInterface for WakeBus {
        fn cycle(&mut self) {
            self.cycles += 1;
        }

        fn cycle_halted(&mut self, _sample_dot: u8) -> Option<bool> {
            self.cycles += 1;
            Some(self.raised())
        }

        fn read(&mut self, addr: u16) -> u8 {
            self.mem[addr as usize]
        }

        fn write(&mut self, addr: u16, value: u8) {
            self.mem[addr as usize] = value;
        }
    }

    impl ICInterface for WakeBus {
        fn request_interrupt(&mut self, _interrupt: Interrupt) {}

        fn has_pending_interrupt(&self) -> bool {
            self.raised()
        }
    }

    #[cfg(feature = "debug")]
    impl crate::debug::DebuggerInterface for WakeBus {}

    fn halted_steps_until_wake(model: GbModel) -> u32 {
        let mut bus = WakeBus {
            mem: vec![0; 0x10000],
            cycles: 0,
            raised_in_cycle: 4,
        };
        let mut cpu = Cpu::new_with_boot_rom(model);
        cpu.halted = true;
        let mut steps = 0;
        while cpu.halted {
            cpu.step(&mut bus);
            steps += 1;
            assert!(steps < 100);
        }
        steps
    }

    #[test]
    fn dmg_wakes_in_the_cycle_an_interrupt_is_raised() {
        assert_eq!(halted_steps_until_wake(GbModel::Dmg), 4);
    }

    #[test]
    fn cgb_wakes_one_cycle_after_an_interrupt_is_raised() {
        assert_eq!(halted_steps_until_wake(GbModel::Cgb), 5);
    }

    fn halted_cycles_until_wake(speed_switch_halt: u32) -> u32 {
        let mut bus = WakeBus {
            mem: vec![0; 0x10000],
            cycles: 0,
            raised_in_cycle: 4,
        };
        let mut cpu = Cpu::new_with_boot_rom(GbModel::Cgb);
        cpu.halted = true;
        cpu.speed_switch_halt = speed_switch_halt;
        while cpu.halted {
            cpu.step(&mut bus);
            assert!(bus.cycles < 100);
        }
        bus.cycles
    }

    #[test]
    fn an_interrupt_ends_the_speed_switch_halt_like_a_halt() {
        assert_eq!(
            halted_cycles_until_wake(0x8000),
            halted_cycles_until_wake(0)
        );
    }

    #[test]
    fn flag_bytes_round_trip_through_every_value() {
        for value in 0..=u8::MAX {
            let flags = Flags::from(value);
            assert_eq!(flags.zero, value & 0x80 != 0);
            assert_eq!(flags.subtract, value & 0x40 != 0);
            assert_eq!(flags.half_carry, value & 0x20 != 0);
            assert_eq!(flags.carry, value & 0x10 != 0);
            assert_eq!(u8::from(flags), value & 0xF0);
        }
    }

    struct HaltBus {
        mem: Vec<u8>,
        pending: bool,
    }

    impl HaltBus {
        fn new(pending: bool) -> Self {
            Self {
                mem: vec![0; 0x10000],
                pending,
            }
        }
    }

    impl CpuBusInterface for HaltBus {
        fn cycle(&mut self) {}

        fn read(&mut self, addr: u16) -> u8 {
            self.mem[addr as usize]
        }

        fn write(&mut self, addr: u16, value: u8) {
            self.mem[addr as usize] = value;
        }
    }

    impl ICInterface for HaltBus {
        fn request_interrupt(&mut self, _interrupt: Interrupt) {}

        fn has_pending_interrupt(&self) -> bool {
            self.pending
        }
    }

    #[cfg(feature = "debug")]
    impl crate::debug::DebuggerInterface for HaltBus {}

    fn run(pending: bool) -> Cpu {
        let mut bus = HaltBus::new(pending);
        bus.mem[0x0200] = 0x76;
        bus.mem[0x0201] = 0x3C;
        bus.mem[0x0202] = 0x00;

        let mut cpu = Cpu::new_dmg(0x01);
        cpu.a = 0x00;
        cpu.ime = false;
        cpu.pc = 0x0201;
        cpu.ir = 0x76;

        cpu.step(&mut bus);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        cpu
    }

    #[test]
    fn halt_bug_executes_following_opcode_twice() {
        let cpu = run(true);
        assert_eq!(cpu.a, 0x02, "INC A after HALT should execute twice");
        assert!(!cpu.halted);
    }

    #[test]
    fn no_halt_bug_without_pending_interrupt() {
        let cpu = run(false);
        assert!(cpu.halted, "HALT without a pending interrupt should halt");
    }
}
