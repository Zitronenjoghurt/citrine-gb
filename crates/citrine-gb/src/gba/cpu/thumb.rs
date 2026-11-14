mod alu;
mod transfer;

use super::exception::Exception;
use super::psr::{FLAG_T, Psr};
use super::{Core, CpuBus};

fn low(op: u32, at: u32) -> usize {
    ((op >> at) & 7) as usize
}

fn sign_extend(value: u32, bits: u32) -> u32 {
    let shift = 32 - bits;
    (((value << shift) as i32) >> shift) as u32
}

pub(super) type ThumbHandler<const V5: bool, B> = fn(&mut Core<V5>, &mut B, u32);

const ENTRIES: usize = 256;

impl<const V5: bool> Core<V5> {
    #[inline(always)]
    pub(super) fn execute_thumb<B: CpuBus>(&mut self, bus: &mut B, op: u32) {
        let table: &[ThumbHandler<V5, B>; ENTRIES] = const { &Self::thumb_table::<B>() };
        table[((op >> 8) & 0xFF) as usize](self, bus, op);
    }

    const fn thumb_table<B: CpuBus>() -> [ThumbHandler<V5, B>; ENTRIES] {
        let undefined: ThumbHandler<V5, B> = Self::thumb_undefined;
        let mut table = [undefined; ENTRIES];
        let mut index = 0;
        while index < ENTRIES {
            table[index] = Self::thumb_handler::<B>((index as u32) << 8);
            index += 1;
        }
        table
    }

    const fn thumb_handler<B: CpuBus>(op: u32) -> ThumbHandler<V5, B> {
        match op >> 11 {
            0b00000 => Self::thumb_shift::<B, 0>,
            0b00001 => Self::thumb_shift::<B, 1>,
            0b00010 => Self::thumb_shift::<B, 2>,
            0b00011 => match (op >> 9) & 3 {
                0 => Self::thumb_add_sub::<B, 0>,
                1 => Self::thumb_add_sub::<B, 1>,
                2 => Self::thumb_add_sub::<B, 2>,
                _ => Self::thumb_add_sub::<B, 3>,
            },
            0b00100 => Self::thumb_immediate::<B, 0>,
            0b00101 => Self::thumb_immediate::<B, 1>,
            0b00110 => Self::thumb_immediate::<B, 2>,
            0b00111 => Self::thumb_immediate::<B, 3>,
            0b01000 => match (op >> 8) & 7 {
                0 => Self::thumb_alu::<B, 0>,
                1 => Self::thumb_alu::<B, 1>,
                2 => Self::thumb_alu::<B, 2>,
                3 => Self::thumb_alu::<B, 3>,
                4 => Self::thumb_hi_register::<B, 0>,
                5 => Self::thumb_hi_register::<B, 1>,
                6 => Self::thumb_hi_register::<B, 2>,
                _ => Self::thumb_hi_register::<B, 3>,
            },
            0b01001 => Self::thumb_load_pc_relative,
            0b01010 | 0b01011 if op & (1 << 9) == 0 => match (op >> 10) & 3 {
                0 => Self::thumb_register_offset::<B, 0>,
                1 => Self::thumb_register_offset::<B, 1>,
                2 => Self::thumb_register_offset::<B, 2>,
                _ => Self::thumb_register_offset::<B, 3>,
            },
            0b01010 | 0b01011 => match (op >> 10) & 3 {
                0 => Self::thumb_halfword_register_offset::<B, 0>,
                1 => Self::thumb_halfword_register_offset::<B, 1>,
                2 => Self::thumb_halfword_register_offset::<B, 2>,
                _ => Self::thumb_halfword_register_offset::<B, 3>,
            },
            0b01100 => Self::thumb_immediate_offset::<B, 0>,
            0b01101 => Self::thumb_immediate_offset::<B, 1>,
            0b01110 => Self::thumb_immediate_offset::<B, 2>,
            0b01111 => Self::thumb_immediate_offset::<B, 3>,
            0b10000 => Self::thumb_halfword_immediate_offset::<B, false>,
            0b10001 => Self::thumb_halfword_immediate_offset::<B, true>,
            0b10010 => Self::thumb_sp_relative::<B, false>,
            0b10011 => Self::thumb_sp_relative::<B, true>,
            0b10100 | 0b10101 => Self::thumb_load_address,
            0b10110 | 0b10111 => match (op >> 8) & 0xF {
                0b0000 => Self::thumb_adjust_sp,
                0b0100 | 0b0101 => Self::thumb_push_pop::<B, false>,
                0b1100 | 0b1101 => Self::thumb_push_pop::<B, true>,
                0b1110 if V5 => Self::thumb_breakpoint,
                _ => Self::thumb_undefined,
            },
            0b11000 => Self::thumb_block_transfer::<B, false>,
            0b11001 => Self::thumb_block_transfer::<B, true>,
            0b11010 | 0b11011 => match (op >> 8) & 0xF {
                0x0 => Self::thumb_conditional_branch::<B, 0x0>,
                0x1 => Self::thumb_conditional_branch::<B, 0x1>,
                0x2 => Self::thumb_conditional_branch::<B, 0x2>,
                0x3 => Self::thumb_conditional_branch::<B, 0x3>,
                0x4 => Self::thumb_conditional_branch::<B, 0x4>,
                0x5 => Self::thumb_conditional_branch::<B, 0x5>,
                0x6 => Self::thumb_conditional_branch::<B, 0x6>,
                0x7 => Self::thumb_conditional_branch::<B, 0x7>,
                0x8 => Self::thumb_conditional_branch::<B, 0x8>,
                0x9 => Self::thumb_conditional_branch::<B, 0x9>,
                0xA => Self::thumb_conditional_branch::<B, 0xA>,
                0xB => Self::thumb_conditional_branch::<B, 0xB>,
                0xC => Self::thumb_conditional_branch::<B, 0xC>,
                0xD => Self::thumb_conditional_branch::<B, 0xD>,
                0xE => Self::thumb_undefined,
                _ => Self::thumb_software_interrupt,
            },
            0b11100 => Self::thumb_unconditional_branch,
            0b11101 if V5 => Self::thumb_long_branch_exchange,
            0b11110 => Self::thumb_long_branch_prefix,
            0b11111 => Self::thumb_long_branch_suffix,
            _ => Self::thumb_undefined,
        }
    }

    fn thumb_conditional_branch<B: CpuBus, const CONDITION: u32>(&mut self, bus: &mut B, op: u32) {
        if Psr(self.regs.cpsr()).passes(CONDITION) {
            self.thumb_branch(bus, sign_extend(op & 0xFF, 8) << 1);
        } else {
            self.prefetch_thumb(bus);
        }
    }

    fn thumb_unconditional_branch(&mut self, bus: &mut impl CpuBus, op: u32) {
        self.thumb_branch(bus, sign_extend(op & 0x7FF, 11) << 1);
    }

    fn thumb_branch(&mut self, bus: &mut impl CpuBus, offset: u32) {
        if !V5 {
            return self.plain_thumb_branch(bus, offset);
        }
        let pc = self.regs.r[15];
        if !self.jump_thumb_nearby(bus, pc, pc.wrapping_add(offset)) {
            self.thumb_branch_elsewhere(bus, offset);
        }
    }

    #[inline(never)]
    fn thumb_branch_elsewhere(&mut self, bus: &mut impl CpuBus, offset: u32) {
        self.plain_thumb_branch(bus, offset);
    }

    #[inline(always)]
    fn plain_thumb_branch(&mut self, bus: &mut impl CpuBus, offset: u32) {
        let target = self.regs.r[15].wrapping_add(offset);
        self.prefetch_thumb_before_jump(bus);
        self.regs.r[15] = target;
        self.jump_thumb_aligned(bus);
    }

    fn thumb_long_branch_prefix(&mut self, bus: &mut impl CpuBus, op: u32) {
        let high = sign_extend(op & 0x7FF, 11) << 12;
        self.regs.r[14] = self.regs.r[15].wrapping_add(high);
        self.prefetch_thumb(bus);
    }

    fn thumb_long_branch_suffix(&mut self, bus: &mut impl CpuBus, op: u32) {
        if !V5 {
            return self.plain_long_branch_suffix(bus, op);
        }
        let target = self.regs.r[14].wrapping_add((op & 0x7FF) << 1);
        let pc = self.regs.r[15];
        if !self.jump_thumb_nearby(bus, pc, target & !1) {
            return self.long_branch_suffix_elsewhere(bus, op);
        }
        self.regs.r[14] = pc.wrapping_sub(2) | 1;
    }

    #[inline(never)]
    fn long_branch_suffix_elsewhere(&mut self, bus: &mut impl CpuBus, op: u32) {
        self.plain_long_branch_suffix(bus, op);
    }

    #[inline(always)]
    fn plain_long_branch_suffix(&mut self, bus: &mut impl CpuBus, op: u32) {
        let target = self.regs.r[14].wrapping_add((op & 0x7FF) << 1);
        let link = self.regs.r[15].wrapping_sub(2) | 1;
        self.prefetch_thumb_before_jump(bus);
        self.regs.r[14] = link;
        self.regs.r[15] = target;
        self.jump_thumb(bus);
    }

    fn thumb_long_branch_exchange(&mut self, bus: &mut impl CpuBus, op: u32) {
        if !V5 {
            return self.plain_long_branch_exchange(bus, op);
        }
        let target = self.regs.r[14].wrapping_add((op & 0x7FF) << 1) & !3;
        let pc = self.regs.r[15];
        if !self.exchange_nearby(bus, pc, target) {
            return self.long_branch_exchange_elsewhere(bus, op);
        }
        self.regs.r[14] = pc.wrapping_sub(2) | 1;
    }

    #[inline(never)]
    fn long_branch_exchange_elsewhere(&mut self, bus: &mut impl CpuBus, op: u32) {
        self.plain_long_branch_exchange(bus, op);
    }

    #[inline(always)]
    fn plain_long_branch_exchange(&mut self, bus: &mut impl CpuBus, op: u32) {
        let target = self.regs.r[14].wrapping_add((op & 0x7FF) << 1) & !3;
        let link = self.regs.r[15].wrapping_sub(2) | 1;
        self.prefetch_thumb_before_jump(bus);
        self.regs.r[14] = link;
        let cpsr = self.regs.cpsr();
        self.regs.set_cpsr(cpsr & !FLAG_T);
        self.regs.r[15] = target;
        self.jump_arm(bus);
    }

    fn thumb_breakpoint(&mut self, bus: &mut impl CpuBus, _op: u32) {
        let link = self.regs.r[15];
        self.prefetch_thumb(bus);
        self.enter(bus, Exception::PrefetchAbort, link);
    }

    fn thumb_software_interrupt(&mut self, bus: &mut impl CpuBus, _op: u32) {
        let link = self.regs.r[15].wrapping_sub(2);
        self.prefetch_thumb(bus);
        self.enter(bus, Exception::SoftwareInterrupt, link);
    }

    fn thumb_undefined(&mut self, bus: &mut impl CpuBus, _op: u32) {
        let link = self.regs.r[15].wrapping_sub(2);
        self.prefetch_thumb(bus);
        self.enter(bus, Exception::Undefined, link);
    }
}
