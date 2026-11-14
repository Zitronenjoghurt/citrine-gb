use super::super::memory::{Block, loaded_value, truncate};
use super::super::{Core, CpuBus};
use super::low;
use crate::gba::wait::{Access, Width};

impl<const V5: bool> Core<V5> {
    #[inline(always)]
    fn thumb_load(
        &mut self,
        bus: &mut impl CpuBus,
        rd: usize,
        addr: u32,
        width: Width,
        signed: bool,
    ) {
        if !V5 {
            return self.plain_thumb_load(bus, rd, addr, width, signed);
        }
        let pc = self.regs.r[15];
        let Some((fetched, next, word)) = self.near_thumb_next(bus, pc) else {
            return self.thumb_load_elsewhere(bus, rd, addr, width, signed);
        };
        let at = if width == Width::Half {
            addr & !1
        } else {
            addr
        };
        let Some((cycles, value)) = bus.near_read(at, width) else {
            return self.thumb_load_elsewhere(bus, rd, addr, width, signed);
        };
        if let Some(word) = word {
            self.word = word;
        }
        self.advanced(pc, Width::Half, fetched, next);
        self.data += cycles;
        self.next_fetch = Access::NonSequential;
        self.regs.r[rd] = loaded_value(value, addr, width, signed);
    }

    #[inline(never)]
    fn thumb_load_elsewhere(
        &mut self,
        bus: &mut impl CpuBus,
        rd: usize,
        addr: u32,
        width: Width,
        signed: bool,
    ) {
        self.plain_thumb_load(bus, rd, addr, width, signed);
    }

    #[inline(always)]
    fn plain_thumb_load(
        &mut self,
        bus: &mut impl CpuBus,
        rd: usize,
        addr: u32,
        width: Width,
        signed: bool,
    ) {
        self.prefetch_thumb(bus);
        let value = self.load_value(bus, addr, width, signed);
        self.load_idle(bus);
        self.regs.r[rd] = value;
    }

    #[inline(always)]
    fn thumb_store(&mut self, bus: &mut impl CpuBus, rd: usize, addr: u32, width: Width) {
        let value = truncate(self.regs.r[rd], width);
        if V5 {
            if !self.advance_quickly(bus, Width::Half) {
                return self.thumb_store_elsewhere(bus, addr, width, value);
            }
            if !self.store_quickly(bus, addr, width, value) {
                self.store_elsewhere(bus, addr, width, value);
            }
            return;
        }
        self.prefetch_thumb(bus);
        self.store(bus, addr, width, value, Access::NonSequential);
    }

    #[inline(never)]
    fn thumb_store_elsewhere(
        &mut self,
        bus: &mut impl CpuBus,
        addr: u32,
        width: Width,
        value: u32,
    ) {
        self.prefetch_thumb(bus);
        self.store(bus, addr, width, value, Access::NonSequential);
    }

    pub(in super::super) fn thumb_load_pc_relative(&mut self, bus: &mut impl CpuBus, op: u32) {
        let addr = (self.regs.r[15] & !2).wrapping_add((op & 0xFF) << 2);
        self.thumb_load(bus, low(op, 8), addr, Width::Word, false);
    }

    fn register_address(&self, op: u32) -> u32 {
        self.regs.r[low(op, 3)].wrapping_add(self.regs.r[low(op, 6)])
    }

    pub(in super::super) fn thumb_register_offset<B: CpuBus, const FORM: u32>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        let addr = self.register_address(op);
        let width = if FORM & 1 != 0 {
            Width::Byte
        } else {
            Width::Word
        };
        if FORM & 2 != 0 {
            self.thumb_load(bus, low(op, 0), addr, width, false);
        } else {
            self.thumb_store(bus, low(op, 0), addr, width);
        }
    }

    pub(in super::super) fn thumb_halfword_register_offset<B: CpuBus, const FORM: u32>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        let addr = self.register_address(op);
        let rd = low(op, 0);
        match FORM {
            0 => self.thumb_store(bus, rd, addr, Width::Half),
            1 => self.thumb_load(bus, rd, addr, Width::Byte, true),
            2 => self.thumb_load(bus, rd, addr, Width::Half, false),
            _ => self.thumb_load(bus, rd, addr, Width::Half, true),
        }
    }

    pub(in super::super) fn thumb_immediate_offset<B: CpuBus, const FORM: u32>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        let offset = (op >> 6) & 0x1F;
        let (width, offset) = if FORM & 2 != 0 {
            (Width::Byte, offset)
        } else {
            (Width::Word, offset << 2)
        };
        let addr = self.regs.r[low(op, 3)].wrapping_add(offset);
        if FORM & 1 != 0 {
            self.thumb_load(bus, low(op, 0), addr, width, false);
        } else {
            self.thumb_store(bus, low(op, 0), addr, width);
        }
    }

    pub(in super::super) fn thumb_halfword_immediate_offset<B: CpuBus, const LOAD: bool>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        let addr = self.regs.r[low(op, 3)].wrapping_add(((op >> 6) & 0x1F) << 1);
        if LOAD {
            self.thumb_load(bus, low(op, 0), addr, Width::Half, false);
        } else {
            self.thumb_store(bus, low(op, 0), addr, Width::Half);
        }
    }

    pub(in super::super) fn thumb_sp_relative<B: CpuBus, const LOAD: bool>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        let addr = self.regs.r[13].wrapping_add((op & 0xFF) << 2);
        if LOAD {
            self.thumb_load(bus, low(op, 8), addr, Width::Word, false);
        } else {
            self.thumb_store(bus, low(op, 8), addr, Width::Word);
        }
    }

    pub(in super::super) fn thumb_push_pop<B: CpuBus, const LOAD: bool>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        let extra = if op & (1 << 8) == 0 {
            0
        } else if LOAD {
            1 << 15
        } else {
            1 << 14
        };
        self.near_block(
            bus,
            Block {
                base: 13,
                list: (op & 0xFF) | extra,
                load: LOAD,
                up: LOAD,
                pre: !LOAD,
                writeback: true,
                user: false,
            },
            Width::Half,
        );
    }

    pub(in super::super) fn thumb_block_transfer<B: CpuBus, const LOAD: bool>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        self.near_block(
            bus,
            Block {
                base: low(op, 8),
                list: op & 0xFF,
                load: LOAD,
                up: true,
                pre: false,
                writeback: true,
                user: false,
            },
            Width::Half,
        );
    }
}
