mod alu;
mod block;
mod branch;
mod memory;
mod multiply;
mod refetch;
mod thumb;
mod timing;
mod transfer;

use super::fast::{Fast, Spec, WINDOWS};
use super::helpers::{EXIT, Helpers};
use super::scan::{Op, Plan};
use super::{JitBus, QuickFetch};
use crate::gba::cpu::psr::{FLAG_C, FLAG_I, FLAG_N, FLAG_V, FLAG_Z};
use crate::gba::cpu::registers::{CPSR_OFFSET, Registers};
use crate::gba::cpu::{Core, END_LINE};
use crate::jit_cache::Reloc;
use cranelift_codegen::Context;
use cranelift_codegen::binemit::Reloc as Kind;
use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    AbiParam, AliasRegionData, Block, BlockCall, ExtFuncData, ExternalName, FuncRef, Function,
    InstBuilder, JumpTableData, MemFlagsData, SigRef, Signature, Type, UserExternalName,
    UserFuncName, Value, types,
};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::{MachReloc, RelocTarget};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use std::mem::offset_of;

const SEQUENTIAL: i64 = 1;
const DATA_OVERLAP: u32 = 6;
const NONSEQUENTIAL: i64 = 0;
const PC: u32 = 15;
const LR: usize = 14;
const RUN_LIMIT: u32 = 16;

struct Layout {
    r: i32,
    cpsr: i32,
    pipeline: i32,
    next_fetch: i32,
    cycles: i32,
    irq_masked: i32,
    irq_held: i32,
    landed: i32,
}

impl Layout {
    fn of<const V5: bool>() -> Self {
        let regs = offset_of!(Core<V5>, regs);
        let at = |offset: usize| offset as i32;
        Self {
            r: at(regs + offset_of!(Registers, r)),
            cpsr: at(regs + CPSR_OFFSET),
            pipeline: at(offset_of!(Core<V5>, pipeline)),
            next_fetch: at(offset_of!(Core<V5>, next_fetch)),
            cycles: at(offset_of!(Core<V5>, cycles)),
            irq_masked: at(offset_of!(Core<V5>, irq_masked)),
            irq_held: at(offset_of!(Core<V5>, irq_held)),
            landed: at(offset_of!(Core<V5>, landed)),
        }
    }

    fn reg(&self, index: usize) -> i32 {
        self.r + 4 * index as i32
    }
}

pub struct Emitter {
    isa: OwnedTargetIsa,
    context: Context,
    builder: FunctionBuilderContext,
}

pub struct Emitted {
    pub code: Vec<u8>,
    pub relocs: Vec<Reloc>,
    pub portable: bool,
}

impl Emitter {
    pub fn new() -> Option<Self> {
        let mut flags = settings::builder();
        flags.set("opt_level", "speed").ok()?;
        if cfg!(windows) {
            flags.set("enable_probestack", "true").ok()?;
            flags.set("probestack_strategy", "inline").ok()?;
        } else {
            flags.set("enable_probestack", "false").ok()?;
        }
        let verify = if cfg!(debug_assertions) {
            "true"
        } else {
            "false"
        };
        flags.set("enable_verifier", verify).ok()?;
        let isa = cranelift_native::builder()
            .ok()?
            .finish(settings::Flags::new(flags))
            .ok()?;
        Some(Self {
            isa,
            context: Context::new(),
            builder: FunctionBuilderContext::new(),
        })
    }

    pub fn target(&self) -> String {
        let flags = self.isa.isa_flags();
        let flags = flags.iter().map(ToString::to_string).collect::<Vec<_>>();
        format!("{} {}", self.isa.triple(), flags.join(","))
    }

    pub fn emit<const V5: bool, B: JitBus>(&mut self, plan: &Plan) -> Option<Emitted> {
        self.context.clear();
        let pointer = self.isa.pointer_type();
        let call = self.isa.default_call_conv();
        let signature = |params: &[Type], returns: &[Type]| {
            let mut signature = Signature::new(call);
            signature
                .params
                .extend(params.iter().map(|&ty| AbiParam::new(ty)));
            signature
                .returns
                .extend(returns.iter().map(|&ty| AbiParam::new(ty)));
            signature
        };
        let (i32t, i64t) = (types::I32, types::I64);
        self.context.func = Function::with_name_signature(
            UserFuncName::default(),
            signature(&[pointer, pointer, i32t, i32t, pointer, pointer], &[i64t]),
        );
        let mut b = FunctionBuilder::new(&mut self.context.func, &mut self.builder);
        let sigs = Sigs {
            load: b.import_signature(signature(&[pointer, pointer, i32t, i32t], &[i64t])),
            store: b.import_signature(signature(&[pointer, pointer, i32t, i32t, i32t], &[i64t])),
            branch: b.import_signature(signature(&[pointer, pointer, i32t], &[i32t])),
            step: b.import_signature(signature(&[pointer, pointer], &[i64t])),
            shift: b.import_signature(signature(&[i32t, i32t, i32t, i32t], &[i64t])),
            load_word: b.import_signature(signature(&[pointer, pointer, i32t, i32t], &[i64t])),
            load_pc: b.import_signature(signature(&[pointer, pointer, i32t], &[i64t])),
            store_word: b
                .import_signature(signature(&[pointer, pointer, i32t, i32t, i32t], &[i64t])),
            fetch: b.import_signature(signature(&[pointer, i32t, i32t], &[i32t])),
            fetch_run: b
                .import_signature(signature(&[pointer, i32t, i32t, i32t, i32t, i32t], &[i64t])),
        };
        let mut block = Builder::new(
            b,
            pointer,
            plan,
            sigs,
            Helpers::of::<V5, B>(),
            Layout::of::<V5>(),
            B::SPECS,
            V5,
            B::TIMED_FETCH,
        );
        block.quick = B::QUICK_FETCH.filter(|_| B::TIMED_FETCH);
        block.build();
        let portable = !block.fixed;
        block.b.seal_all_blocks();
        block.b.finalize(self.isa.frontend_config());
        let code = self
            .context
            .compile(&*self.isa, &mut ControlPlane::default())
            .ok()?;
        let bytes = code.code_buffer().to_vec();
        let found = code.buffer.relocs().to_vec();
        let names = self.context.func.params.user_named_funcs();
        let relocs = found
            .iter()
            .map(|reloc| relocation(reloc, |name| names.get(name).map(|name| name.index)))
            .collect::<Option<Vec<_>>>()?;
        Some(Emitted {
            code: bytes,
            relocs,
            portable,
        })
    }
}

fn relocation(
    reloc: &MachReloc,
    index: impl Fn(cranelift_codegen::ir::UserExternalNameRef) -> Option<u32>,
) -> Option<Reloc> {
    let RelocTarget::ExternalName(ExternalName::User(name)) = &reloc.target else {
        return None;
    };
    (reloc.kind == Kind::Abs8 && reloc.addend == 0).then_some(Reloc {
        offset: reloc.offset,
        helper: index(*name)?,
    })
}

struct Sigs {
    load: SigRef,
    store: SigRef,
    branch: SigRef,
    step: SigRef,
    shift: SigRef,
    load_word: SigRef,
    store_word: SigRef,
    load_pc: SigRef,
    fetch: SigRef,
    fetch_run: SigRef,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fetch {
    Known(i64),
    Runtime,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Merge {
    Alone,
    First { end: usize },
    Member { end: usize },
}

struct Vars {
    cpsr: Variable,
    total: Variable,
    next_fetch: Variable,
    last: Variable,
    force: Variable,
    retired: Variable,
    at: Variable,
    debt: Variable,
    stop: Variable,
}

struct Builder<'a, 'f> {
    b: FunctionBuilder<'f>,
    pointer: Type,
    plan: &'a Plan<'a>,
    sigs: Sigs,
    helpers: Helpers,
    table: Vec<usize>,
    funcs: Vec<Option<FuncRef>>,
    fixed: bool,
    layout: Layout,
    vars: Vars,
    core: Value,
    bus: Value,
    budget: Value,
    start: Value,
    skipped: Value,
    heads: Vec<Block>,
    regs: MemFlagsData,
    fetch: Fetch,
    exit: Block,
    branch_exit: Block,
    v5: bool,
    timed: bool,
    debt: Value,
    fast: Value,
    words: Value,
    specs: [Spec; WINDOWS],
    cost: Option<u32>,
    run: u32,
    costs: Vec<Option<u32>>,
    ahead: Vec<(Value, usize)>,
    merge: Vec<Merge>,
    merged: bool,
    quick: Option<QuickFetch>,
}

impl<'a, 'f> Builder<'a, 'f> {
    #[expect(clippy::too_many_arguments, reason = "one compiled block")]
    fn new(
        mut b: FunctionBuilder<'f>,
        pointer: Type,
        plan: &'a Plan<'a>,
        sigs: Sigs,
        helpers: Helpers,
        layout: Layout,
        specs: [Spec; WINDOWS],
        v5: bool,
        timed: bool,
    ) -> Self {
        let (i32t, i64t) = (types::I32, types::I64);
        let vars = Vars {
            cpsr: b.declare_var(i32t),
            total: b.declare_var(i32t),
            next_fetch: b.declare_var(i32t),
            last: b.declare_var(i32t),
            force: b.declare_var(types::I8),
            retired: b.declare_var(i64t),
            at: b.declare_var(i32t),
            debt: b.declare_var(i32t),
            stop: b.declare_var(i32t),
        };
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let params = b.block_params(entry);
        let (core, bus, budget, start, fast, words) = (
            params[0], params[1], params[2], params[3], params[4], params[5],
        );
        let debt = b.ins().load(
            pointer,
            MemFlagsData::trusted().with_readonly().with_can_move(),
            fast,
            offset_of!(Fast, debt) as i32,
        );
        let skipped = b.ins().uextend(i64t, start);
        let skipped = b.ins().ishl_imm_u(skipped, 32);
        let exit = b.create_block();
        let branch_exit = b.create_block();
        b.set_cold_block(exit);
        b.set_cold_block(branch_exit);
        let region = b.func.dfg.alias_regions.insert(AliasRegionData {
            user_id: 1,
            description: "guest registers".into(),
        });
        let regs = MemFlagsData::trusted().with_alias_region(Some(region));
        let table = helpers.table();
        Self {
            b,
            pointer,
            plan,
            sigs,
            helpers,
            funcs: vec![None; table.len()],
            table,
            fixed: false,
            layout,
            vars,
            core,
            bus,
            budget,
            start,
            skipped,
            heads: Vec::new(),
            regs,
            fetch: Fetch::Runtime,
            exit,
            branch_exit,
            v5,
            timed,
            debt,
            fast,
            words,
            specs,
            cost: None,
            run: 0,
            costs: Vec::new(),
            ahead: Vec::new(),
            merge: Vec::new(),
            merged: false,
            quick: None,
        }
    }

    fn flags() -> MemFlagsData {
        MemFlagsData::trusted()
    }

    fn i32(&mut self, value: u32) -> Value {
        self.b.ins().iconst(types::I32, i64::from(value))
    }

    fn load32(&mut self, offset: i32) -> Value {
        self.b
            .ins()
            .load(types::I32, Self::flags(), self.core, offset)
    }

    fn store32(&mut self, value: Value, offset: i32) {
        self.b.ins().store(Self::flags(), value, self.core, offset);
    }

    fn store8(&mut self, value: Value, offset: i32) {
        if self.b.func.dfg.value_type(value) == types::I8 {
            self.b.ins().store(Self::flags(), value, self.core, offset);
        } else {
            self.b
                .ins()
                .istore8(Self::flags(), value, self.core, offset);
        }
    }

    fn load_regs(&mut self) {
        let cpsr = self.load32(self.layout.cpsr);
        self.b.def_var(self.vars.cpsr, cpsr);
    }

    fn store_regs(&mut self) {
        let cpsr = self.b.use_var(self.vars.cpsr);
        self.store32(cpsr, self.layout.cpsr);
    }

    fn address(&self, index: usize) -> u32 {
        let size = if self.plan.thumb { 2 } else { 4 };
        self.plan.pc.wrapping_add(index as u32 * size)
    }

    fn pc_read(&self, index: usize) -> u32 {
        self.address(index + 2)
    }

    fn reg(&mut self, index: u32, pc: u32) -> Value {
        if index == PC {
            self.i32(pc)
        } else {
            let offset = self.layout.reg(index as usize);
            self.b.ins().load(types::I32, self.regs, self.core, offset)
        }
    }

    fn set_reg(&mut self, index: u32, value: Value) {
        debug_assert!(index != PC);
        let offset = self.layout.reg(index as usize);
        self.b.ins().store(self.regs, value, self.core, offset);
    }

    fn build(&mut self) {
        self.load_regs();
        self.load_debt();
        self.load_next_fetch();
        self.set_force(None);
        let zero = self.i32(0);
        self.b.def_var(self.vars.total, zero);
        self.b.def_var(self.vars.last, zero);
        self.b.def_var(self.vars.stop, zero);
        let count = self.plan.ops.len();
        if self.timed && !self.v5 {
            self.merge = timing::merges(self.plan.ops);
        }
        let dispatch = self.b.create_block();
        self.b.ins().jump(dispatch, &[]);
        self.heads = (0..count).map(|_| self.b.create_block()).collect();
        self.costs = vec![None; count];
        for index in 0..count {
            self.b.switch_to_block(self.heads[index]);
            self.fetch = Fetch::Runtime;
            self.cost = Some(0);
            self.instruction(index);
        }
        self.build_exit();
        self.build_branch_exit();
        self.build_dispatch(dispatch);
    }

    fn build_dispatch(&mut self, dispatch: Block) {
        let count = self.plan.ops.len();
        let mut ahead = vec![0; count + 1];
        for index in (0..count).rev() {
            ahead[index] = self.costs[index].map_or(0, |cost| cost + ahead[index + 1]);
        }
        for (value, index) in std::mem::take(&mut self.ahead) {
            let inst = self.b.func.dfg.value_def(value).unwrap_inst();
            self.b
                .func
                .replace(inst)
                .iconst(types::I32, i64::from(ahead[index]));
        }
        let near = self.b.create_block();
        self.b.set_cold_block(near);
        let targets: Vec<Block> = (0..count)
            .map(|index| match self.merge.get(index) {
                Some(Merge::Member { .. }) => self.b.create_block(),
                _ => self.heads[index],
            })
            .collect();
        let entries: Vec<Block> = (0..count)
            .map(|index| {
                if ahead[index] == 0 {
                    targets[index]
                } else {
                    self.b.create_block()
                }
            })
            .collect();
        self.b.switch_to_block(dispatch);
        let calls: Vec<BlockCall> = entries
            .iter()
            .map(|&entry| self.b.func.dfg.block_call(entry, &[]))
            .collect();
        let table = self
            .b
            .create_jump_table(JumpTableData::new(calls[0], &calls));
        self.b.ins().br_table(self.start, table);
        for (index, &entry) in entries.iter().enumerate() {
            if ahead[index] == 0 {
                continue;
            }
            self.b.switch_to_block(entry);
            let short = self.b.ins().icmp_imm_u(
                IntCC::UnsignedLessThanOrEqual,
                self.budget,
                i64::from(ahead[index]),
            );
            self.b.ins().brif(short, near, &[], targets[index], &[]);
        }
        for (index, &target) in targets.iter().enumerate() {
            if let Some(&Merge::Member { end }) = self.merge.get(index) {
                self.b.switch_to_block(target);
                self.fetch = Fetch::Runtime;
                let access = self.fetch_access();
                self.fetch_run(index, end, access);
                self.b.ins().jump(self.heads[index], &[]);
            }
        }
        self.b.switch_to_block(near);
        let packed = self.b.ins().iconst(types::I64, EXIT as i64);
        self.b.ins().return_(&[packed]);
    }

    fn passes(&mut self, condition: u32) -> Value {
        let cpsr = self.b.use_var(self.vars.cpsr);
        let flag = |s: &mut Self, flag: u32| {
            let bit = s.b.ins().band_imm_u(cpsr, i64::from(flag));
            s.b.ins().icmp_imm_u(IntCC::NotEqual, bit, 0)
        };
        let n = flag(self, FLAG_N);
        let z = flag(self, FLAG_Z);
        let c = flag(self, FLAG_C);
        let v = flag(self, FLAG_V);
        let not = |s: &mut Self, value: Value| s.b.ins().bxor_imm_u(value, 1);
        match condition {
            0x0 => z,
            0x1 => not(self, z),
            0x2 => c,
            0x3 => not(self, c),
            0x4 => n,
            0x5 => not(self, n),
            0x6 => v,
            0x7 => not(self, v),
            0x8 => {
                let not_z = not(self, z);
                self.b.ins().band(c, not_z)
            }
            0x9 => {
                let not_c = not(self, c);
                self.b.ins().bor(not_c, z)
            }
            0xA => self.b.ins().icmp(IntCC::Equal, n, v),
            0xB => self.b.ins().icmp(IntCC::NotEqual, n, v),
            0xC => {
                let not_z = not(self, z);
                let same = self.b.ins().icmp(IntCC::Equal, n, v);
                self.b.ins().band(not_z, same)
            }
            0xD => {
                let differ = self.b.ins().icmp(IntCC::NotEqual, n, v);
                self.b.ins().bor(z, differ)
            }
            _ => self.b.ins().iconst(types::I8, 1),
        }
    }

    fn instruction(&mut self, index: usize) {
        let op = self.plan.ops[index];
        let cost = self.fetch_cost(index);
        let condition = op.condition();
        let falls = !op.ends();
        let join = (falls || condition != 0xE).then(|| self.b.create_block());
        if let Some(join) = join.filter(|_| condition != 0xE) {
            let body = self.b.create_block();
            let skip = self.b.create_block();
            let passes = self.passes(condition);
            self.b.ins().brif(passes, body, &[], skip, &[]);
            self.b.switch_to_block(skip);
            let entry = self.fetch;
            let cost = self.fetched(index, cost);
            self.add_cycles(cost);
            self.set_force(None);
            self.set_next_fetch(SEQUENTIAL);
            self.b.ins().jump(join, &[]);
            self.b.switch_to_block(body);
            self.fetch = entry;
        }
        match op {
            Op::Step(word) => self.step(index, word),
            Op::Exit(word) => self.step_out(index, word),
            Op::Alu(word) => self.alu(index, word, cost),
            Op::Transfer(transfer) => self.transfer(index, transfer, cost),
            Op::Block(block) => self.block(index, block, cost),
            Op::Multiply(multiply) => self.multiply(index, multiply, cost),
            Op::Constant { rd, value } => self.constant(index, rd, value, cost),
            Op::Branch(word) => self.branch(index, word),
            Op::Jump(jump) => self.jump(index, jump),
        }
        let Some(join) = join else {
            return;
        };
        if falls {
            self.b.ins().jump(join, &[]);
        }
        if condition != 0xE && (!falls || self.fetch == Fetch::Known(SEQUENTIAL)) {
            self.fetch = Fetch::Known(SEQUENTIAL);
        } else if condition != 0xE {
            self.fetch = Fetch::Runtime;
        }
        self.b.switch_to_block(join);
        self.check(index);
    }

    fn check(&mut self, index: usize) {
        if let Fetch::Known(access) = self.fetch {
            let value = self.b.ins().iconst(types::I32, access);
            self.b.def_var(self.vars.next_fetch, value);
        }
        let last = index + 1 == self.plan.ops.len();
        if !last
            && self.timed
            && let Some(cost) = self.cost
            && self.run + cost <= RUN_LIMIT
        {
            self.run += cost;
            self.costs[index] = Some(cost);
            let next = self.heads[index + 1];
            self.b.ins().jump(next, &[]);
            return;
        }
        let size = if self.plan.thumb { 2 } else { 4 };
        let next = self.plan.pc.wrapping_add(size * (index as u32 + 1));
        let ends = self.timed || next.is_multiple_of(END_LINE);
        if !last && !ends && self.cost.is_some() {
            let next = self.heads[index + 1];
            self.b.ins().jump(next, &[]);
            return;
        }
        self.run = 0;
        let at = self.i32(index as u32 + 1);
        self.b.def_var(self.vars.at, at);
        if last {
            self.b.ins().jump(self.exit, &[]);
            return;
        }
        if self
            .merge
            .get(index)
            .is_some_and(|&merge| merge != Merge::Alone)
        {
            let stop = self.b.use_var(self.vars.stop);
            let leave = self
                .b
                .ins()
                .icmp_imm_u(IntCC::Equal, stop, index as i64 + 1);
            let next = self.heads[index + 1];
            self.b.ins().brif(leave, self.exit, &[], next, &[]);
            return;
        }
        let force = self.b.use_var(self.vars.force);
        let leave = if ends {
            let total = self.b.use_var(self.vars.total);
            let total = if self.timed {
                let ahead = self.i32(0);
                self.ahead.push((ahead, index + 1));
                self.b.ins().iadd(total, ahead)
            } else {
                total
            };
            let spent = self
                .b
                .ins()
                .icmp(IntCC::UnsignedGreaterThanOrEqual, total, self.budget);
            self.b.ins().bor(spent, force)
        } else {
            force
        };
        let next = self.heads[index + 1];
        self.b.ins().brif(leave, self.exit, &[], next, &[]);
    }

    fn retired_after(&mut self, index: usize) -> Value {
        let done = self.b.ins().iconst(types::I64, (index as i64 + 1) << 32);
        self.b.ins().isub(done, self.skipped)
    }

    fn call(&mut self, sig: SigRef, helper: usize, args: &[Value]) -> Value {
        let call = match self.table.iter().position(|&known| known == helper) {
            Some(index) => {
                let func = self.helper(sig, index);
                self.b.ins().call(func, args)
            }
            None => {
                self.fixed = true;
                let callee = self.b.ins().iconst(self.pointer, helper as i64);
                self.b.ins().call_indirect(sig, callee, args)
            }
        };
        self.b.inst_results(call)[0]
    }

    fn helper(&mut self, sig: SigRef, index: usize) -> FuncRef {
        if let Some(func) = self.funcs[index] {
            return func;
        }
        let name = UserExternalName::new(0, index as u32);
        let name = self.b.func.declare_imported_user_function(name);
        let func = self.b.import_function(ExtFuncData {
            name: ExternalName::User(name),
            signature: sig,
            colocated: false,
            patchable: false,
        });
        self.funcs[index] = Some(func);
        func
    }

    fn split(&mut self, packed: Value) -> (Value, Value) {
        let low = self.b.ins().ireduce(types::I32, packed);
        let cycles = self.b.ins().band_imm_u(low, (EXIT - 1) as i64);
        let exit = self.b.ins().ushr_imm_u(low, 31);
        let force = self.b.ins().ireduce(types::I8, exit);
        (cycles, force)
    }

    fn step(&mut self, index: usize, word: u32) {
        self.store_regs();
        let r15 = self.i32(self.pc_read(index));
        self.store32(r15, self.layout.reg(15));
        let first = self.i32(word);
        self.store32(first, self.layout.pipeline);
        let second = self.i32(self.plan.words[index + 1]);
        self.store32(second, self.layout.pipeline + 4);
        let next_fetch = self.b.use_var(self.vars.next_fetch);
        self.store8(next_fetch, self.layout.next_fetch);
        self.store_debt();
        let (core, bus) = (self.core, self.bus);
        let packed = self.call(self.sigs.step, self.helpers.step, &[core, bus]);
        let (cycles, force) = self.split(packed);
        self.load_regs();
        self.load_debt();
        self.load_next_fetch();
        self.add_settled(cycles);
        self.set_force(Some(force));
        if self.timed {
            self.leave_on(index, force);
        }
    }

    fn leave_on(&mut self, index: usize, force: Value) {
        let leave = self.b.create_block();
        let stay = self.b.create_block();
        self.b.set_cold_block(leave);
        self.b.ins().brif(force, leave, &[], stay, &[]);
        self.b.switch_to_block(leave);
        let retired = self.retired_after(index);
        self.b.def_var(self.vars.retired, retired);
        self.b.ins().jump(self.branch_exit, &[]);
        self.b.switch_to_block(stay);
    }

    fn step_out(&mut self, index: usize, word: u32) {
        self.step(index, word);
        let retired = self.retired_after(index);
        self.b.def_var(self.vars.retired, retired);
        self.b.ins().jump(self.branch_exit, &[]);
    }

    fn finish(&mut self) -> Value {
        let retired = self.b.use_var(self.vars.retired);
        let total = self.b.use_var(self.vars.total);
        let total = self.b.ins().uextend(types::I64, total);
        let packed = self.b.ins().bor(retired, total);
        let force = self.b.use_var(self.vars.force);
        let force = self.b.ins().uextend(types::I64, force);
        let force = self.b.ins().ishl_imm_u(force, 31);
        self.b.ins().bor(packed, force)
    }

    fn build_exit(&mut self) {
        self.b.switch_to_block(self.exit);
        self.store_regs();
        self.store_debt();
        let at = self.b.use_var(self.vars.at);
        let wide = self.b.ins().uextend(types::I64, at);
        let done = self.b.ins().ishl_imm_u(wide, 32);
        let retired = self.b.ins().isub(done, self.skipped);
        self.b.def_var(self.vars.retired, retired);
        let shift = if self.plan.thumb { 1 } else { 2 };
        let step = self.b.ins().ishl_imm_u(at, shift);
        let base = i64::from(self.pc_read(0));
        let r15 = self.b.ins().iadd_imm_u(step, base);
        let index = self.b.ins().uextend(self.pointer, at);
        let offset = self.b.ins().ishl_imm_u(index, 2);
        let word = self.b.ins().iadd(self.words, offset);
        let flags = MemFlagsData::trusted().with_readonly();
        let first = self.b.ins().load(types::I32, flags, word, 0);
        let second = self.b.ins().load(types::I32, flags, word, 4);
        self.store32(r15, self.layout.reg(15));
        self.store32(first, self.layout.pipeline);
        self.store32(second, self.layout.pipeline + 4);
        let next_fetch = self.b.use_var(self.vars.next_fetch);
        self.store8(next_fetch, self.layout.next_fetch);
        let last = self.b.use_var(self.vars.last);
        self.store32(last, self.layout.cycles);
        let cpsr = self.b.use_var(self.vars.cpsr);
        let masked = self.b.ins().band_imm_u(cpsr, i64::from(FLAG_I));
        let masked = self.b.ins().icmp_imm_u(IntCC::NotEqual, masked, 0);
        self.store8(masked, self.layout.irq_masked);
        let held = self.b.ins().iconst(types::I8, 0);
        self.store8(held, self.layout.irq_held);
        let packed = self.finish();
        self.b.ins().return_(&[packed]);
    }

    fn build_branch_exit(&mut self) {
        self.b.switch_to_block(self.branch_exit);
        self.store_regs();
        self.store_debt();
        let last = self.b.use_var(self.vars.last);
        self.store32(last, self.layout.cycles);
        let held = self.b.ins().iconst(types::I8, 0);
        self.store8(held, self.layout.irq_held);
        let packed = self.finish();
        self.b.ins().return_(&[packed]);
    }
}
