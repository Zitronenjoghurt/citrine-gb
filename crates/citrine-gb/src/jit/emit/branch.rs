use super::super::scan::{Jump, Target};
use super::{Builder, LR};
use cranelift_codegen::ir::{InstBuilder, Value};

impl Builder<'_, '_> {
    pub(super) fn branch(&mut self, index: usize, op: u32) {
        let offset = (((op << 8) as i32) >> 6) as u32;
        if op & (1 << 24) != 0 {
            let link = self.i32(self.link(index));
            self.set_reg(LR as u32, link);
        }
        let target = self.i32(self.pc_read(index).wrapping_add(offset));
        let fetch = self.branch_fetch(index);
        let reload = self.reload(target);
        let reload = self.plus(fetch, reload);
        self.leave(index, reload);
    }

    pub(super) fn jump(&mut self, index: usize, jump: Jump) {
        let target = match jump.target {
            Target::Fixed(target) => self.i32(target),
            Target::Register { rm, offset, mask } => {
                let value = self.reg(rm, self.pc_read(index));
                let value = self.b.ins().iadd_imm_u(value, i64::from(offset));
                self.b.ins().band_imm_u(value, i64::from(mask))
            }
        };
        if jump.link {
            let link = self.i32(self.link(index));
            self.set_reg(LR as u32, link);
        }
        let fetch = self.branch_fetch(index);
        let reload = if jump.exchange {
            self.reload_exchange(target)
        } else {
            self.reload(target)
        };
        let reload = self.plus(fetch, reload);
        self.leave(index, reload);
    }

    fn plus(&mut self, fetch: Option<Value>, reload: Value) -> Value {
        match fetch {
            Some(fetch) => self.b.ins().iadd(fetch, reload),
            None => reload,
        }
    }

    fn link(&self, index: usize) -> u32 {
        if self.plan.thumb {
            self.address(index + 1) | 1
        } else {
            self.address(index + 1)
        }
    }

    pub(super) fn leave(&mut self, index: usize, reload: Value) {
        self.add_cycles(reload);
        let retired = self.retired_after(index);
        self.b.def_var(self.vars.retired, retired);
        self.b.ins().jump(self.branch_exit, &[]);
    }
}
