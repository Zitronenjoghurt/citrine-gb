use super::super::memory::{Block, truncate};
use super::super::{Cpu, CpuBus};
use super::low;
use crate::gba::wait::{Access, Width};

impl Cpu {
    fn thumb_load(
        &mut self,
        bus: &mut impl CpuBus,
        rd: usize,
        addr: u32,
        width: Width,
        signed: bool,
    ) {
        self.prefetch(bus);
        let value = self.load_value(bus, addr, width, signed);
        self.idle(bus);
        self.regs.r[rd] = value;
    }

    fn thumb_store(&mut self, bus: &mut impl CpuBus, rd: usize, addr: u32, width: Width) {
        let value = truncate(self.regs.r[rd], width);
        self.prefetch(bus);
        self.store(bus, addr, width, value, Access::NonSequential);
    }

    pub(in super::super) fn thumb_load_pc_relative(&mut self, bus: &mut impl CpuBus, op: u32) {
        let addr = (self.regs.r[15] & !2).wrapping_add((op & 0xFF) << 2);
        self.thumb_load(bus, low(op, 8), addr, Width::Word, false);
    }

    fn register_address(&self, op: u32) -> u32 {
        self.regs.r[low(op, 3)].wrapping_add(self.regs.r[low(op, 6)])
    }

    pub(in super::super) fn thumb_register_offset(&mut self, bus: &mut impl CpuBus, op: u32) {
        let addr = self.register_address(op);
        let width = if op & (1 << 10) != 0 {
            Width::Byte
        } else {
            Width::Word
        };
        if op & (1 << 11) != 0 {
            self.thumb_load(bus, low(op, 0), addr, width, false);
        } else {
            self.thumb_store(bus, low(op, 0), addr, width);
        }
    }

    pub(in super::super) fn thumb_halfword_register_offset(
        &mut self,
        bus: &mut impl CpuBus,
        op: u32,
    ) {
        let addr = self.register_address(op);
        let rd = low(op, 0);
        match (op >> 10) & 3 {
            0 => self.thumb_store(bus, rd, addr, Width::Half),
            1 => self.thumb_load(bus, rd, addr, Width::Byte, true),
            2 => self.thumb_load(bus, rd, addr, Width::Half, false),
            _ => self.thumb_load(bus, rd, addr, Width::Half, true),
        }
    }

    pub(in super::super) fn thumb_immediate_offset(&mut self, bus: &mut impl CpuBus, op: u32) {
        let offset = (op >> 6) & 0x1F;
        let (width, offset) = if op & (1 << 12) != 0 {
            (Width::Byte, offset)
        } else {
            (Width::Word, offset << 2)
        };
        let addr = self.regs.r[low(op, 3)].wrapping_add(offset);
        if op & (1 << 11) != 0 {
            self.thumb_load(bus, low(op, 0), addr, width, false);
        } else {
            self.thumb_store(bus, low(op, 0), addr, width);
        }
    }

    pub(in super::super) fn thumb_halfword_immediate_offset(
        &mut self,
        bus: &mut impl CpuBus,
        op: u32,
    ) {
        let addr = self.regs.r[low(op, 3)].wrapping_add(((op >> 6) & 0x1F) << 1);
        if op & (1 << 11) != 0 {
            self.thumb_load(bus, low(op, 0), addr, Width::Half, false);
        } else {
            self.thumb_store(bus, low(op, 0), addr, Width::Half);
        }
    }

    pub(in super::super) fn thumb_sp_relative(&mut self, bus: &mut impl CpuBus, op: u32) {
        let addr = self.regs.r[13].wrapping_add((op & 0xFF) << 2);
        if op & (1 << 11) != 0 {
            self.thumb_load(bus, low(op, 8), addr, Width::Word, false);
        } else {
            self.thumb_store(bus, low(op, 8), addr, Width::Word);
        }
    }

    pub(in super::super) fn thumb_push_pop(&mut self, bus: &mut impl CpuBus, op: u32) {
        let load = op & (1 << 11) != 0;
        let extra = if op & (1 << 8) == 0 {
            0
        } else if load {
            1 << 15
        } else {
            1 << 14
        };
        self.block_transfer(
            bus,
            Block {
                base: 13,
                list: (op & 0xFF) | extra,
                load,
                up: load,
                pre: !load,
                writeback: true,
                user: false,
            },
        );
    }

    pub(in super::super) fn thumb_block_transfer(&mut self, bus: &mut impl CpuBus, op: u32) {
        self.block_transfer(
            bus,
            Block {
                base: low(op, 8),
                list: op & 0xFF,
                load: op & (1 << 11) != 0,
                up: true,
                pre: false,
                writeback: true,
                user: false,
            },
        );
    }
}
