use super::{Builder, SEQUENTIAL};
use cranelift_codegen::ir::Value;

impl Builder<'_, '_> {
    pub(super) fn constant(&mut self, index: usize, rd: u32, value: u32, cost: Value) {
        let cost = self.fetched(index, cost);
        let value = self.i32(value);
        self.set_reg(rd, value);
        self.add_cycles(cost);
        self.set_force(None);
        self.set_next_fetch(SEQUENTIAL);
    }
}
