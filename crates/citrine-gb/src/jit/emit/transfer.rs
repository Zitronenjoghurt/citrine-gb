use super::super::scan::{Offset, Transfer, Width};
use super::{Builder, NONSEQUENTIAL};
use crate::gba::wait::Access;
use cranelift_codegen::ir::{InstBuilder, Value};

impl Builder<'_, '_> {
    pub(super) fn transfer(&mut self, index: usize, transfer: Transfer, cost: Value) {
        let pc = self.pc_read(index);
        let base = self.reg(transfer.rn, pc);
        let offset = match transfer.offset {
            Offset::Immediate(value) => self.i32(value),
            Offset::Register(rm) => self.reg(rm, pc),
            Offset::Shifted { rm, shift, amount } => {
                let value = self.reg(rm, pc);
                self.shift_immediate(value, shift, amount).0
            }
        };
        let moved = if transfer.up() {
            self.b.ins().iadd(base, offset)
        } else {
            self.b.ins().isub(base, offset)
        };
        let addr = if transfer.pre() { moved } else { base };
        let kind = self.i32(transfer.kind());
        let cost = self.store_fetch(index, cost);
        let (core, bus) = (self.core, self.bus);
        let (cycles, force) = if transfer.load() {
            let (value, cycles, force) = self.load_memory(
                addr,
                transfer.width,
                transfer.signed,
                true,
                Access::NonSequential,
                |s| s.call(s.sigs.load, s.helpers.load, &[core, bus, addr, kind]),
            );
            if transfer.writeback() {
                self.set_reg(transfer.rn, moved);
            }
            self.set_reg(transfer.rd, value);
            (cycles, force)
        } else {
            let value = self.reg(transfer.rd, pc.wrapping_add(4));
            let value = match transfer.width {
                Width::Byte => self.b.ins().band_imm_u(value, 0xFF),
                Width::Half => self.b.ins().band_imm_u(value, 0xFFFF),
                Width::Word => value,
            };
            let (cycles, force) =
                self.store_memory(addr, value, transfer.width, Access::NonSequential, |s| {
                    s.call(
                        s.sigs.store,
                        s.helpers.store,
                        &[core, bus, addr, value, kind],
                    )
                });
            if transfer.writeback() {
                self.set_reg(transfer.rn, moved);
            }
            (cycles, force)
        };
        let cycles = self.settled(cost, cycles);
        self.add_cycles(cycles);
        self.set_force(Some(force));
        self.set_next_fetch(NONSEQUENTIAL);
    }
}
