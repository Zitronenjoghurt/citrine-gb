use super::super::memory::{Block, offset_base, rotate_word, truncate};
use super::super::shifter::Shift;
use super::super::{Core, CpuBus};
use crate::gba::wait::{Access, Width};

const PRE: u32 = 1 << 24;
const UP: u32 = 1 << 23;
const WRITEBACK: u32 = 1 << 21;
const LOAD: u32 = 1 << 20;
const BYTE: u32 = 1 << 22;
const REGISTER_OFFSET: u32 = 1 << 25;
const HALF_IMMEDIATE: u32 = 1 << 22;
pub(super) const FORM_BITS: u32 = PRE | UP | WRITEBACK;
pub(super) const OFFSET_UP: u32 = PRE | UP;
pub(super) const OFFSET_DOWN: u32 = PRE;
pub(super) const ANY_FORM: u32 = u32::MAX;

impl<const V5: bool> Core<V5> {
    pub(in super::super) fn arm_single_transfer(&mut self, bus: &mut impl CpuBus, op: u32) {
        let load = op & LOAD != 0;
        let byte = op & BYTE != 0;
        let register = op & REGISTER_OFFSET != 0;
        self.single_transfer(bus, op, load, byte, register);
    }

    pub(super) fn arm_load_store<
        B: CpuBus,
        const LOADS: bool,
        const BYTES: bool,
        const REGISTER: bool,
        const FORM: u32,
    >(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        let op = if FORM == ANY_FORM {
            op
        } else {
            (op & !FORM_BITS) | FORM
        };
        self.single_transfer(bus, op, LOADS, BYTES, REGISTER);
    }

    #[inline(always)]
    fn single_transfer(
        &mut self,
        bus: &mut impl CpuBus,
        op: u32,
        load: bool,
        byte: bool,
        register: bool,
    ) {
        let rn = ((op >> 16) & 0xF) as usize;
        let rd = ((op >> 12) & 0xF) as usize;
        let offset = if register {
            let shift = Shift::from_bits(op >> 5);
            let amount = (op >> 7) & 0x1F;
            let carry = self.regs.cpsr() & super::super::psr::FLAG_C != 0;
            shift
                .by_immediate(self.operand((op & 0xF) as usize), amount, carry)
                .0
        } else {
            op & 0xFFF
        };
        let width = if byte { Width::Byte } else { Width::Word };
        self.transfer(bus, op, load, rn, rd, offset, width, false);
    }

    pub(in super::super) fn arm_halfword_transfer(&mut self, bus: &mut impl CpuBus, op: u32) {
        self.halfword_transfer(
            bus,
            op,
            op & LOAD != 0,
            op & HALF_IMMEDIATE != 0,
            (op >> 5) & 3,
        );
    }

    pub(super) fn arm_halfword<
        B: CpuBus,
        const LOADS: bool,
        const IMMEDIATE: bool,
        const KIND: u32,
        const FORM: u32,
    >(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        let op = if FORM == ANY_FORM {
            op
        } else {
            (op & !FORM_BITS) | FORM
        };
        self.halfword_transfer(bus, op, LOADS, IMMEDIATE, KIND);
    }

    #[inline(always)]
    fn halfword_transfer(
        &mut self,
        bus: &mut impl CpuBus,
        op: u32,
        load: bool,
        immediate: bool,
        kind: u32,
    ) {
        let rn = ((op >> 16) & 0xF) as usize;
        let rd = ((op >> 12) & 0xF) as usize;
        let offset = if immediate {
            ((op >> 4) & 0xF0) | (op & 0xF)
        } else {
            self.regs.r[(op & 0xF) as usize]
        };
        if V5 && !load && kind & 2 != 0 {
            self.doubleword_transfer(bus, op, rn, rd, offset);
            return;
        }
        let (width, signed) = match kind {
            1 => (Width::Half, false),
            2 => (Width::Byte, true),
            _ => (Width::Half, true),
        };
        self.transfer(bus, op, load, rn, rd, offset, width, signed);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one transfer path for both encodings"
    )]
    #[inline(always)]
    fn transfer(
        &mut self,
        bus: &mut impl CpuBus,
        op: u32,
        load: bool,
        rn: usize,
        rd: usize,
        offset: u32,
        width: Width,
        signed: bool,
    ) {
        if V5 && !load {
            return self.quick_store(bus, op, rn, rd, offset, width);
        }
        self.plain_transfer(bus, op, load, rn, rd, offset, width, signed);
    }

    #[inline(always)]
    fn quick_store(
        &mut self,
        bus: &mut impl CpuBus,
        op: u32,
        rn: usize,
        rd: usize,
        offset: u32,
        width: Width,
    ) {
        let pre = op & PRE != 0;
        let base = self.operand(rn);
        let moved = offset_base(base, offset, op & UP != 0);
        let addr = if pre { moved } else { base };
        let writeback = !pre || op & WRITEBACK != 0;
        if writeback && rn == 15 || !self.advance_quickly(bus, Width::Word) {
            return self.arm_store_elsewhere(bus, op, offset, width);
        }
        let value = truncate(self.regs.r[rd], width);
        if writeback {
            self.regs.r[rn] = moved;
        }
        if !self.store_quickly(bus, addr, width, value) {
            self.store_elsewhere(bus, addr, width, value);
        }
    }

    #[inline(never)]
    fn arm_store_elsewhere(&mut self, bus: &mut impl CpuBus, op: u32, offset: u32, width: Width) {
        let rn = ((op >> 16) & 0xF) as usize;
        let rd = ((op >> 12) & 0xF) as usize;
        self.plain_transfer(bus, op, false, rn, rd, offset, width, false);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one transfer path for both encodings"
    )]
    #[inline(always)]
    fn plain_transfer(
        &mut self,
        bus: &mut impl CpuBus,
        op: u32,
        load: bool,
        rn: usize,
        rd: usize,
        offset: u32,
        width: Width,
        signed: bool,
    ) {
        let pre = op & PRE != 0;
        let base = self.operand(rn);
        let moved = offset_base(base, offset, op & UP != 0);
        let addr = if pre { moved } else { base };
        let writeback = !pre || op & WRITEBACK != 0;
        self.prefetch_arm(bus);

        if load {
            let value = self.load_value(bus, addr, width, signed);
            self.load_idle(bus);
            if writeback && rn == 15 && rd != 15 {
                self.write_back_pc(bus, moved);
                self.regs.r[rd] = value;
                return;
            }
            if writeback {
                self.regs.r[rn] = moved;
            }
            if V5 && rd == 15 {
                self.load_pc_exchange(bus, value);
            } else {
                self.write_pc_or_reg(bus, rd, value);
            }
        } else {
            let value = truncate(self.regs.r[rd], width);
            self.store(bus, addr, width, value, Access::NonSequential);
            if writeback && rn == 15 {
                self.write_back_pc(bus, moved);
            } else if writeback {
                self.regs.r[rn] = moved;
            }
        }
    }

    fn doubleword_transfer(
        &mut self,
        bus: &mut impl CpuBus,
        op: u32,
        rn: usize,
        rd: usize,
        offset: u32,
    ) {
        if rd & 1 != 0 {
            self.arm_undefined(bus);
            return;
        }
        let pre = op & PRE != 0;
        let base = self.operand(rn);
        let moved = offset_base(base, offset, op & UP != 0);
        let addr = if pre { moved } else { base } & !3;
        let writeback = !pre || op & WRITEBACK != 0;
        self.prefetch_arm(bus);
        if op & (1 << 5) == 0 {
            let low = self.load(bus, addr, Width::Word, Access::NonSequential);
            let high = self.load(bus, addr.wrapping_add(4), Width::Word, Access::Sequential);
            self.load_idle(bus);
            if writeback {
                self.regs.r[rn] = moved;
            }
            self.regs.r[rd] = low;
            self.write_pc_or_reg(bus, rd + 1, high);
        } else {
            let low = self.regs.r[rd];
            let high = self.regs.r[rd + 1];
            self.store(bus, addr, Width::Word, low, Access::NonSequential);
            self.store(
                bus,
                addr.wrapping_add(4),
                Width::Word,
                high,
                Access::Sequential,
            );
            if writeback {
                self.regs.r[rn] = moved;
            }
        }
    }

    fn write_back_pc(&mut self, bus: &mut impl CpuBus, base: u32) {
        self.regs.r[15] = base.wrapping_add(4);
        self.reload(bus);
    }

    pub(in super::super) fn arm_swap(&mut self, bus: &mut impl CpuBus, op: u32) {
        let byte = op & (1 << 22) != 0;
        let rd = ((op >> 12) & 0xF) as usize;
        let rm = (op & 0xF) as usize;
        self.prefetch_arm(bus);
        let addr = self.operand(((op >> 16) & 0xF) as usize);
        let (width, value) = if byte {
            (Width::Byte, self.regs.r[rm] & 0xFF)
        } else {
            (Width::Word, self.regs.r[rm])
        };
        let loaded = self.load(bus, addr, width, Access::NonSequential);
        self.store(bus, addr, width, value, Access::NonSequential);
        self.load_idle(bus);
        let loaded = if byte {
            loaded
        } else {
            rotate_word(loaded, addr)
        };
        self.write_pc_or_reg(bus, rd, loaded);
    }

    pub(in super::super) fn arm_block_transfer(&mut self, bus: &mut impl CpuBus, op: u32) {
        self.block_transfer(
            bus,
            Block {
                base: ((op >> 16) & 0xF) as usize,
                list: op & 0xFFFF,
                load: op & LOAD != 0,
                up: op & UP != 0,
                pre: op & PRE != 0,
                writeback: op & WRITEBACK != 0,
                user: op & (1 << 22) != 0,
            },
            Width::Word,
        );
    }

    pub(super) fn arm_block<
        B: CpuBus,
        const LOADS: bool,
        const INCREMENTS: bool,
        const BEFORE: bool,
        const WRITES_BACK: bool,
    >(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        self.near_block(
            bus,
            Block {
                base: ((op >> 16) & 0xF) as usize,
                list: op & 0xFFFF,
                load: LOADS,
                up: INCREMENTS,
                pre: BEFORE,
                writeback: WRITES_BACK,
                user: false,
            },
            Width::Word,
        );
    }
}
