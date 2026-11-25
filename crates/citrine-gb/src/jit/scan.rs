mod thumb;

pub use thumb::classify as classify_thumb;

const PC: u32 = 15;
const LR: u32 = 14;
const ALWAYS: u32 = 0xE;
const LOAD: u32 = 1 << 20;
const WRITEBACK: u32 = 1 << 21;
const UP: u32 = 1 << 23;
const PRE: u32 = 1 << 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Alu(u32),
    Transfer(Transfer),
    Block(Block),
    Multiply(Multiply),
    Constant { rd: u32, value: u32 },
    Branch(u32),
    Jump(Jump),
    Step(u32),
    Exit(u32),
}

impl Op {
    pub fn straight(self) -> bool {
        match self {
            Op::Alu(word) => {
                let opcode = (word >> 21) & 0xF;
                let rd = (word >> 12) & 0xF;
                word >> 28 == ALWAYS
                    && (word & (1 << 25) != 0 || word & 0x10 == 0)
                    && (rd != PC || (0x8..=0xB).contains(&opcode))
            }
            Op::Constant { rd, .. } => rd != PC,
            _ => false,
        }
    }

    pub fn condition(self) -> u32 {
        match self {
            Op::Alu(word) | Op::Branch(word) => word >> 28,
            Op::Transfer(transfer) => transfer.op >> 28,
            Op::Block(block) => block.condition,
            Op::Multiply(multiply) => multiply.condition,
            Op::Jump(jump) => jump.condition,
            Op::Constant { .. } | Op::Step(_) | Op::Exit(_) => ALWAYS,
        }
    }

    pub fn ends(self) -> bool {
        match self {
            Op::Block(block) => block.loads_pc(),
            op => matches!(op, Op::Branch(_) | Op::Jump(_) | Op::Exit(_)),
        }
    }

    pub fn stops(self) -> bool {
        match self {
            Op::Branch(_) => self.condition() >= ALWAYS,
            op => op.ends(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    pub condition: u32,
    pub rn: u32,
    pub list: u16,
    pub load: bool,
    pub up: bool,
    pub pre: bool,
    pub writeback: bool,
}

impl Block {
    pub fn registers(self) -> impl Iterator<Item = u32> {
        (0..16).filter(move |index| self.list & (1 << index) != 0)
    }

    pub fn loads_pc(self) -> bool {
        self.load && self.list & 1 << PC != 0
    }

    pub fn keeps_loaded_base(self) -> bool {
        let list = u32::from(self.list);
        self.load && list != 1 << self.rn && list >> self.rn == 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Multiply {
    pub condition: u32,
    pub rd: u32,
    pub rm: u32,
    pub rs: u32,
    pub accumulate: Option<u32>,
    pub set_flags: bool,
    pub long: Option<Long>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Long {
    pub low: u32,
    pub signed: bool,
    pub accumulate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jump {
    pub condition: u32,
    pub target: Target,
    pub link: bool,
    pub exchange: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Fixed(u32),
    Register { rm: u32, offset: u32, mask: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    Byte,
    Half,
    Word,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offset {
    Immediate(u32),
    Shifted { rm: u32, shift: u32, amount: u32 },
    Register(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transfer {
    pub op: u32,
    pub rn: u32,
    pub rd: u32,
    pub width: Width,
    pub signed: bool,
    pub offset: Offset,
}

impl Transfer {
    pub fn load(self) -> bool {
        self.op & LOAD != 0
    }

    pub fn pre(self) -> bool {
        self.op & PRE != 0
    }

    pub fn up(self) -> bool {
        self.op & UP != 0
    }

    pub fn writeback(self) -> bool {
        !self.pre() || self.op & WRITEBACK != 0
    }

    pub fn kind(self) -> u32 {
        let width = match self.width {
            Width::Byte => 0,
            Width::Half => 1,
            Width::Word => 2,
        };
        width | u32::from(self.signed) << 2
    }

    pub fn pc_literal(self, pc: u32) -> Option<u32> {
        let Offset::Immediate(imm) = self.offset else {
            return None;
        };
        let literal = self.load()
            && self.rn == PC
            && self.width == Width::Word
            && !self.signed
            && self.pre()
            && !self.writeback();
        if !literal {
            return None;
        }
        Some(if self.up() {
            pc.wrapping_add(imm)
        } else {
            pc.wrapping_sub(imm)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Literal {
    pub index: u32,
    pub addr: u32,
    pub word: u32,
}

impl Literal {
    pub fn key(self) -> [u32; 2] {
        [self.index, self.word]
    }
}

pub struct Plan<'a> {
    pub pc: u32,
    pub thumb: bool,
    pub words: &'a [u32],
    pub ops: &'a [Op],
    pub fetch: &'a [[u32; 2]],
    pub literals: &'a [Literal],
}

impl Plan<'_> {
    pub fn literal(&self, index: usize) -> Option<Literal> {
        self.literals
            .iter()
            .find(|literal| literal.index as usize == index)
            .copied()
    }
}

fn reg(op: u32, at: u32) -> u32 {
    (op >> at) & 0xF
}

pub fn classify<const V5: bool>(op: u32) -> Op {
    compiled::<V5>(op).unwrap_or(Op::Exit(op))
}

fn compiled<const V5: bool>(op: u32) -> Option<Op> {
    if op >> 28 == 0xF {
        return None;
    }
    match (op >> 25) & 7 {
        0b000 => classify_low::<V5>(op),
        0b001 if op & 0x0190_0000 == 0x0100_0000 => None,
        0b001 => alu(op),
        0b010 => single(op),
        0b011 if op & 0x10 == 0 => single(op),
        0b100 => block::<V5>(op),
        0b101 => Some(Op::Branch(op)),
        _ => None,
    }
}

fn classify_low<const V5: bool>(op: u32) -> Option<Op> {
    if op & 0x0FFF_FFF0 == 0x012F_FF10 {
        return exchange(op, false);
    }
    if op & 0x0FB0_0FF0 == 0x0100_0090 {
        return (reg(op, 12) != PC).then_some(Op::Step(op));
    }
    if op & 0x0FC0_00F0 == 0x0000_0090 {
        return (reg(op, 16) != PC).then(|| multiply::<V5>(op, false));
    }
    if op & 0x0F80_00F0 == 0x0080_0090 {
        return (reg(op, 16) != PC && reg(op, 12) != PC).then(|| multiply::<V5>(op, true));
    }
    if op & 0x0000_0090 == 0x0000_0090 {
        return halfword::<V5>(op);
    }
    if op & 0x0190_0000 == 0x0100_0000 {
        return if V5 && op & 0xF0 != 0 {
            misc(op)
        } else if op & (1 << 21) == 0 {
            (reg(op, 12) != PC).then_some(Op::Step(op))
        } else {
            None
        };
    }
    alu(op)
}

fn misc(op: u32) -> Option<Op> {
    let quiet = reg(op, 12) != PC && reg(op, 16) != PC;
    match ((op >> 4) & 0xF, (op >> 21) & 3) {
        (0b0011, 0b01) => exchange(op, true),
        (0b0001, 0b11) | (0b0101, _) => (reg(op, 12) != PC).then_some(Op::Step(op)),
        (bits, _) if bits & 0b1001 == 0b1000 => quiet.then_some(Op::Step(op)),
        _ => None,
    }
}

fn exchange(op: u32, link: bool) -> Option<Op> {
    let rm = reg(op, 0);
    (rm != PC).then_some(Op::Jump(Jump {
        condition: op >> 28,
        target: Target::Register {
            rm,
            offset: 0,
            mask: !0,
        },
        link,
        exchange: true,
    }))
}

fn multiply<const V5: bool>(op: u32, long: bool) -> Op {
    let accumulate = op & (1 << 21) != 0;
    let (rd, rn, rs, rm) = (reg(op, 16), reg(op, 12), reg(op, 8), reg(op, 0));
    let reads_pc = rs == PC || rm == PC || accumulate && rn == PC;
    let set_flags = op & (1 << 20) != 0;
    if reads_pc || !V5 && set_flags {
        return Op::Step(op);
    }
    Op::Multiply(Multiply {
        condition: op >> 28,
        rd,
        rm,
        rs,
        accumulate: (accumulate && !long).then_some(rn),
        set_flags,
        long: long.then_some(Long {
            low: rn,
            signed: op & (1 << 22) != 0,
            accumulate,
        }),
    })
}

fn alu(op: u32) -> Option<Op> {
    (reg(op, 12) != PC).then_some(Op::Alu(op))
}

fn single(op: u32) -> Option<Op> {
    let offset = if op & (1 << 25) != 0 {
        Offset::Shifted {
            rm: reg(op, 0),
            shift: (op >> 5) & 3,
            amount: (op >> 7) & 0x1F,
        }
    } else {
        Offset::Immediate(op & 0xFFF)
    };
    let width = if op & (1 << 22) != 0 {
        Width::Byte
    } else {
        Width::Word
    };
    transfer(op, width, false, offset)
}

fn halfword<const V5: bool>(op: u32) -> Option<Op> {
    let offset = if op & (1 << 22) != 0 {
        Offset::Immediate(((op >> 4) & 0xF0) | (op & 0xF))
    } else {
        Offset::Register(reg(op, 0))
    };
    if op & LOAD == 0 && op & (1 << 6) != 0 {
        if !V5 {
            return None;
        }
        let rd = reg(op, 12);
        let writes_pc = reg(op, 16) == PC && (op & PRE == 0 || op & WRITEBACK != 0);
        return (rd & 1 == 0 && rd != 14 && !writes_pc).then_some(Op::Step(op));
    }
    let (width, signed) = match (op >> 5) & 3 {
        1 => (Width::Half, false),
        2 => (Width::Byte, true),
        _ => (Width::Half, true),
    };
    transfer(op, width, signed, offset)
}

fn transfer(op: u32, width: Width, signed: bool, offset: Offset) -> Option<Op> {
    let transfer = Transfer {
        op,
        rn: reg(op, 16),
        rd: reg(op, 12),
        width,
        signed,
        offset,
    };
    if transfer.writeback() && transfer.rn == PC {
        return None;
    }
    if transfer.load() && transfer.rd == PC {
        return None;
    }
    Some(Op::Transfer(transfer))
}

fn block<const V5: bool>(op: u32) -> Option<Op> {
    let user = op & (1 << 22) != 0;
    let list = op & 0xFFFF;
    let rn = reg(op, 16);
    let loads = op & LOAD != 0;
    if user || rn == PC || list & (1 << PC) != 0 && (!V5 || !loads) {
        return None;
    }
    if list == 0 {
        return V5.then_some(Op::Step(op));
    }
    Some(Op::Block(Block {
        condition: op >> 28,
        rn,
        list: list as u16,
        load: op & LOAD != 0,
        up: op & UP != 0,
        pre: op & PRE != 0,
        writeback: op & WRITEBACK != 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOV_R0_1: u32 = 0xE3A0_0001;
    const ADDS_R1_R1_R2_LSL_R3: u32 = 0xE091_1312;
    const MOV_PC_LR: u32 = 0xE1A0_F00E;
    const LDR_R0_R1_4: u32 = 0xE591_0004;
    const LDR_PC_SP_4: u32 = 0xE49D_F004;
    const STRH_R2_R3_2: u32 = 0xE1C3_20B2;
    const LDRSB_R4_R5_R6: u32 = 0xE195_40D6;
    const BL_BACK: u32 = 0xEBFF_FFFE;
    const BX_LR: u32 = 0xE12F_FF1E;
    const MSR_CPSR_C_R0: u32 = 0xE121_F000;
    const MRS_R0_CPSR: u32 = 0xE10F_0000;
    const MUL_R0_R1_R2: u32 = 0xE000_0291;
    const MULS_R0_R1_R2: u32 = 0xE010_0291;
    const UMULL_R0_R1_R2_R3: u32 = 0xE081_0392;
    const PUSH_R4_LR: u32 = 0xE92D_4010;
    const POP_R4_PC: u32 = 0xE8BD_8010;
    const SWI_0: u32 = 0xEF00_0000;
    const MCR_P15: u32 = 0xEE07_0F9A;
    const BLX_IMM: u32 = 0xFA00_0000;
    const CLZ_R0_R1: u32 = 0xE16F_0F11;
    const LDRD_R2_R0: u32 = 0xE1C0_20D0;

    #[test]
    fn data_processing_compiles_unless_it_writes_pc() {
        assert_eq!(classify::<true>(MOV_R0_1), Op::Alu(MOV_R0_1));
        assert_eq!(
            classify::<true>(ADDS_R1_R1_R2_LSL_R3),
            Op::Alu(ADDS_R1_R1_R2_LSL_R3)
        );
        assert_eq!(classify::<true>(MOV_PC_LR), Op::Exit(MOV_PC_LR));
    }

    #[test]
    fn loads_and_stores_compile_unless_they_load_pc() {
        let Op::Transfer(ldr) = classify::<true>(LDR_R0_R1_4) else {
            panic!("ldr is not a transfer");
        };
        assert_eq!((ldr.rn, ldr.rd, ldr.width), (1, 0, Width::Word));
        assert_eq!(ldr.offset, Offset::Immediate(4));
        assert!(ldr.load() && ldr.pre() && ldr.up() && !ldr.writeback());
        assert_eq!(classify::<true>(LDR_PC_SP_4), Op::Exit(LDR_PC_SP_4));
        let Op::Transfer(strh) = classify::<true>(STRH_R2_R3_2) else {
            panic!("strh is not a transfer");
        };
        assert_eq!(
            (strh.width, strh.signed, strh.load()),
            (Width::Half, false, false)
        );
        let Op::Transfer(ldrsb) = classify::<true>(LDRSB_R4_R5_R6) else {
            panic!("ldrsb is not a transfer");
        };
        assert_eq!((ldrsb.width, ldrsb.signed), (Width::Byte, true));
        assert_eq!(ldrsb.offset, Offset::Register(6));
    }

    #[test]
    fn branches_and_pc_or_mode_writes_end_the_block() {
        assert_eq!(classify::<true>(BL_BACK), Op::Branch(BL_BACK));
        let Op::Jump(bx) = classify::<true>(BX_LR) else {
            panic!("bx is not a jump");
        };
        assert!(
            bx.exchange
                && bx.target
                    == Target::Register {
                        rm: 14,
                        offset: 0,
                        mask: !0
                    }
        );
        let Op::Block(pop) = classify::<true>(POP_R4_PC) else {
            panic!("pop is not a block");
        };
        assert!(pop.loads_pc() && Op::Block(pop).ends());
        for op in [MSR_CPSR_C_R0, SWI_0, MCR_P15, BLX_IMM] {
            assert_eq!(classify::<true>(op), Op::Exit(op), "{op:08X}");
        }
    }

    #[test]
    fn multiplies_and_block_transfers_compile_on_the_arm9() {
        let Op::Multiply(mul) = classify::<true>(MUL_R0_R1_R2) else {
            panic!("mul is not a multiply");
        };
        assert_eq!((mul.rd, mul.rm, mul.rs, mul.long), (0, 1, 2, None));
        let Op::Multiply(umull) = classify::<true>(UMULL_R0_R1_R2_R3) else {
            panic!("umull is not a multiply");
        };
        assert_eq!(umull.long.map(|long| long.low), Some(0));
        let Op::Block(push) = classify::<true>(PUSH_R4_LR) else {
            panic!("push is not a block");
        };
        assert_eq!((push.rn, push.list), (13, 0x4010));
        assert!(!push.load && !push.up && push.pre && push.writeback);
    }

    #[test]
    fn the_arm7_steps_flag_multiplies_and_leaves_on_pc_loads_and_armv5_ops() {
        assert!(matches!(classify::<false>(MUL_R0_R1_R2), Op::Multiply(_)));
        assert!(matches!(classify::<false>(PUSH_R4_LR), Op::Block(_)));
        assert!(matches!(classify::<false>(STRH_R2_R3_2), Op::Transfer(_)));
        assert_eq!(classify::<false>(MULS_R0_R1_R2), Op::Step(MULS_R0_R1_R2));
        for op in [POP_R4_PC, BLX_IMM, CLZ_R0_R1, LDRD_R2_R0] {
            assert_eq!(classify::<false>(op), Op::Exit(op), "{op:08X}");
        }
    }

    #[test]
    fn a_loaded_base_stays_only_when_it_is_the_last_of_several() {
        let block = |rn: u32, list: u16| Block {
            condition: ALWAYS,
            rn,
            list,
            load: true,
            up: true,
            pre: false,
            writeback: true,
        };
        assert!(block(2, 0b0110).keeps_loaded_base());
        assert!(!block(1, 0b0110).keeps_loaded_base());
        assert!(!block(2, 0b0100).keeps_loaded_base());
    }

    #[test]
    fn the_rest_runs_through_the_interpreter_inside_the_block() {
        for op in [MRS_R0_CPSR, CLZ_R0_R1, LDRD_R2_R0] {
            assert_eq!(classify::<true>(op), Op::Step(op), "{op:08X}");
        }
    }
}
