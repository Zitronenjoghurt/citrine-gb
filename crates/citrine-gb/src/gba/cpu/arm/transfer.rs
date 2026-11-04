use super::super::memory::{Block, offset_base, rotate_word, truncate};
use super::super::shifter::Shift;
use super::super::{Cpu, CpuBus};
use crate::gba::wait::{Access, Width};

const PRE: u32 = 1 << 24;
const UP: u32 = 1 << 23;
const WRITEBACK: u32 = 1 << 21;
const LOAD: u32 = 1 << 20;

impl Cpu {
    pub(in super::super) fn arm_single_transfer(&mut self, bus: &mut impl CpuBus, op: u32) {
        let rn = ((op >> 16) & 0xF) as usize;
        let rd = ((op >> 12) & 0xF) as usize;
        let byte = op & (1 << 22) != 0;
        let offset = if op & (1 << 25) != 0 {
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
        self.transfer(bus, op, rn, rd, offset, width, false);
    }

    pub(in super::super) fn arm_halfword_transfer(&mut self, bus: &mut impl CpuBus, op: u32) {
        let rn = ((op >> 16) & 0xF) as usize;
        let rd = ((op >> 12) & 0xF) as usize;
        let offset = if op & (1 << 22) != 0 {
            ((op >> 4) & 0xF0) | (op & 0xF)
        } else {
            self.regs.r[(op & 0xF) as usize]
        };
        let (width, signed) = match (op >> 5) & 3 {
            1 => (Width::Half, false),
            2 => (Width::Byte, true),
            _ => (Width::Half, true),
        };
        self.transfer(bus, op, rn, rd, offset, width, signed);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one transfer path for both encodings"
    )]
    fn transfer(
        &mut self,
        bus: &mut impl CpuBus,
        op: u32,
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
        self.prefetch(bus);

        if op & LOAD != 0 {
            let value = self.load_value(bus, addr, width, signed);
            self.idle(bus);
            if writeback && rn == 15 && rd != 15 {
                self.write_back_pc(bus, moved);
                self.regs.r[rd] = value;
                return;
            }
            if writeback {
                self.regs.r[rn] = moved;
            }
            self.write_pc_or_reg(bus, rd, value);
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

    fn write_back_pc(&mut self, bus: &mut impl CpuBus, base: u32) {
        self.regs.r[15] = base.wrapping_add(4);
        self.reload(bus);
    }

    pub(in super::super) fn arm_swap(&mut self, bus: &mut impl CpuBus, op: u32) {
        let byte = op & (1 << 22) != 0;
        let rd = ((op >> 12) & 0xF) as usize;
        let rm = (op & 0xF) as usize;
        self.prefetch(bus);
        let addr = self.operand(((op >> 16) & 0xF) as usize);
        let (width, value) = if byte {
            (Width::Byte, self.regs.r[rm] & 0xFF)
        } else {
            (Width::Word, self.regs.r[rm])
        };
        let loaded = self.load(bus, addr, width, Access::NonSequential);
        self.store(bus, addr, width, value, Access::NonSequential);
        self.idle(bus);
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
        );
    }
}
