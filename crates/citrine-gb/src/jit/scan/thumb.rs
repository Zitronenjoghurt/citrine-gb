use super::{
    ALWAYS, Block, Jump, LOAD, LR, Multiply, Offset, Op, PC, PRE, Target, Transfer, UP, Width,
};

const SP: u32 = 13;
const IMMEDIATE: u32 = 1 << 25;
const AND: u32 = 0x0;
const EOR: u32 = 0x1;
const SUB: u32 = 0x2;
const RSB: u32 = 0x3;
const ADD: u32 = 0x4;
const ADC: u32 = 0x5;
const SBC: u32 = 0x6;
const TST: u32 = 0x8;
const CMP: u32 = 0xA;
const CMN: u32 = 0xB;
const ORR: u32 = 0xC;
const MOV: u32 = 0xD;
const BIC: u32 = 0xE;
const MVN: u32 = 0xF;

fn low(op: u32, at: u32) -> u32 {
    (op >> at) & 7
}

fn sign_extend(value: u32, bits: u32) -> u32 {
    let shift = 32 - bits;
    (((value << shift) as i32) >> shift) as u32
}

fn data(opcode: u32, set_flags: bool, rn: u32, rd: u32, operand: u32) -> Op {
    Op::Alu(
        ALWAYS << 28 | opcode << 21 | u32::from(set_flags) << 20 | rn << 16 | rd << 12 | operand,
    )
}

fn shifted(rm: u32, shift: u32, amount: u32) -> u32 {
    amount << 7 | shift << 5 | rm
}

fn by_register(rm: u32, shift: u32, rs: u32) -> u32 {
    rs << 8 | shift << 5 | 1 << 4 | rm
}

fn immediate(value: u32) -> u32 {
    IMMEDIATE | value
}

fn words(value: u32) -> u32 {
    IMMEDIATE | 15 << 8 | value >> 2
}

pub fn classify<const V5: bool>(op: u32, addr: u32) -> Op {
    compiled::<V5>(op, addr.wrapping_add(4)).unwrap_or(Op::Exit(op))
}

fn compiled<const V5: bool>(op: u32, pc: u32) -> Option<Op> {
    match op >> 11 {
        0b00000..=0b00010 => {
            let operand = shifted(low(op, 3), op >> 11, (op >> 6) & 0x1F);
            Some(data(MOV, true, 0, low(op, 0), operand))
        }
        0b00011 => Some(add_sub(op)),
        0b00100..=0b00111 => Some(immediate_op(op)),
        0b01000 if op & (1 << 10) == 0 => Some(alu::<V5>(op)),
        0b01000 => hi_register::<V5>(op),
        0b01001 => Some(literal(op, pc)),
        0b01010 | 0b01011 => Some(register_offset(op)),
        0b01100..=0b01111 => {
            let (width, offset) = if op & (1 << 12) != 0 {
                (Width::Byte, (op >> 6) & 0x1F)
            } else {
                (Width::Word, ((op >> 6) & 0x1F) << 2)
            };
            let load = op & (1 << 11) != 0;
            Some(transfer(load, low(op, 3), low(op, 0), width, false, offset))
        }
        0b10000 | 0b10001 => {
            let load = op & (1 << 11) != 0;
            let offset = ((op >> 6) & 0x1F) << 1;
            Some(transfer(
                load,
                low(op, 3),
                low(op, 0),
                Width::Half,
                false,
                offset,
            ))
        }
        0b10010 | 0b10011 => {
            let load = op & (1 << 11) != 0;
            Some(transfer(
                load,
                SP,
                low(op, 8),
                Width::Word,
                false,
                (op & 0xFF) << 2,
            ))
        }
        0b10100 => Some(Op::Constant {
            rd: low(op, 8),
            value: (pc & !2).wrapping_add((op & 0xFF) << 2),
        }),
        0b10101 => Some(data(ADD, false, SP, low(op, 8), words((op & 0xFF) << 2))),
        0b10110 | 0b10111 => match (op >> 8) & 0xF {
            0b0000 => {
                let opcode = if op & (1 << 7) != 0 { SUB } else { ADD };
                Some(data(opcode, false, SP, SP, words((op & 0x7F) << 2)))
            }
            0b0100 | 0b0101 => {
                let list = op & 0xFF | (op & (1 << 8)) << 6;
                block::<V5>(op, SP, list, false)
            }
            0b1100 | 0b1101 => {
                let list = op & 0xFF | (op & (1 << 8)) << 7;
                block::<V5>(op, SP, list, true)
            }
            _ => None,
        },
        0b11000 | 0b11001 => {
            let load = op & (1 << 11) != 0;
            block::<V5>(op, low(op, 8), op & 0xFF, load)
        }
        0b11010 | 0b11011 => match (op >> 8) & 0xF {
            0xE | 0xF => None,
            condition => Some(jump(
                condition,
                Target::Fixed(pc.wrapping_add(sign_extend(op & 0xFF, 8) << 1)),
                false,
                false,
            )),
        },
        0b11100 => Some(jump(
            ALWAYS,
            Target::Fixed(pc.wrapping_add(sign_extend(op & 0x7FF, 11) << 1)),
            false,
            false,
        )),
        0b11101 if V5 => Some(jump(ALWAYS, link_target(op, !3), true, true)),
        0b11110 => Some(Op::Constant {
            rd: LR,
            value: pc.wrapping_add(sign_extend(op & 0x7FF, 11) << 12),
        }),
        0b11111 => Some(jump(ALWAYS, link_target(op, !0), true, false)),
        _ => None,
    }
}

fn add_sub(op: u32) -> Op {
    let operand = if op & (1 << 10) != 0 {
        immediate(low(op, 6))
    } else {
        low(op, 6)
    };
    let opcode = if op & (1 << 9) != 0 { SUB } else { ADD };
    data(opcode, true, low(op, 3), low(op, 0), operand)
}

fn immediate_op(op: u32) -> Op {
    let rd = low(op, 8);
    let operand = immediate(op & 0xFF);
    match (op >> 11) & 3 {
        0 => data(MOV, true, 0, rd, operand),
        1 => data(CMP, true, rd, 0, operand),
        2 => data(ADD, true, rd, rd, operand),
        _ => data(SUB, true, rd, rd, operand),
    }
}

fn alu<const V5: bool>(op: u32) -> Op {
    let (rd, rs) = (low(op, 0), low(op, 3));
    match (op >> 6) & 0xF {
        0x0 => data(AND, true, rd, rd, rs),
        0x1 => data(EOR, true, rd, rd, rs),
        0x2 => data(MOV, true, 0, rd, by_register(rd, 0, rs)),
        0x3 => data(MOV, true, 0, rd, by_register(rd, 1, rs)),
        0x4 => data(MOV, true, 0, rd, by_register(rd, 2, rs)),
        0x5 => data(ADC, true, rd, rd, rs),
        0x6 => data(SBC, true, rd, rd, rs),
        0x7 => data(MOV, true, 0, rd, by_register(rd, 3, rs)),
        0x8 => data(TST, true, rd, 0, rs),
        0x9 => data(RSB, true, rs, rd, immediate(0)),
        0xA => data(CMP, true, rd, 0, rs),
        0xB => data(CMN, true, rd, 0, rs),
        0xC => data(ORR, true, rd, rd, rs),
        0xD if V5 => Op::Multiply(Multiply {
            condition: ALWAYS,
            rd,
            rm: rs,
            rs: rd,
            accumulate: None,
            set_flags: true,
            long: None,
        }),
        0xD => Op::Step(op),
        0xE => data(BIC, true, rd, rd, rs),
        _ => data(MVN, true, 0, rd, rs),
    }
}

fn hi_register<const V5: bool>(op: u32) -> Option<Op> {
    let rd = op & 7 | (op >> 4) & 8;
    let rm = (op >> 3) & 0xF;
    match (op >> 8) & 3 {
        0 => (rd != PC).then(|| data(ADD, false, rd, rd, rm)),
        1 => Some(data(CMP, true, rd, 0, rm)),
        2 => (rd != PC).then(|| data(MOV, false, 0, rd, rm)),
        _ => (rm != PC).then(|| {
            let target = Target::Register {
                rm,
                offset: 0,
                mask: !0,
            };
            jump(ALWAYS, target, V5 && op & (1 << 7) != 0, true)
        }),
    }
}

fn literal(op: u32, pc: u32) -> Op {
    let target = (pc & !2).wrapping_add((op & 0xFF) << 2);
    let ahead = target.wrapping_sub(pc) as i32;
    Op::Transfer(Transfer {
        op: ALWAYS << 28 | LOAD | PRE | if ahead >= 0 { UP } else { 0 },
        rn: PC,
        rd: low(op, 8),
        width: Width::Word,
        signed: false,
        offset: Offset::Immediate(ahead.unsigned_abs()),
    })
}

fn register_offset(op: u32) -> Op {
    let (load, width, signed) = if op & (1 << 9) == 0 {
        let width = if op & (1 << 10) != 0 {
            Width::Byte
        } else {
            Width::Word
        };
        (op & (1 << 11) != 0, width, false)
    } else {
        match (op >> 10) & 3 {
            0 => (false, Width::Half, false),
            1 => (true, Width::Byte, true),
            2 => (true, Width::Half, false),
            _ => (true, Width::Half, true),
        }
    };
    Op::Transfer(Transfer {
        op: ALWAYS << 28 | PRE | UP | if load { LOAD } else { 0 },
        rn: low(op, 3),
        rd: low(op, 0),
        width,
        signed,
        offset: Offset::Register(low(op, 6)),
    })
}

fn transfer(load: bool, rn: u32, rd: u32, width: Width, signed: bool, offset: u32) -> Op {
    Op::Transfer(Transfer {
        op: ALWAYS << 28 | PRE | UP | if load { LOAD } else { 0 },
        rn,
        rd,
        width,
        signed,
        offset: Offset::Immediate(offset),
    })
}

fn block<const V5: bool>(op: u32, rn: u32, list: u32, load: bool) -> Option<Op> {
    if list == 0 {
        return V5.then_some(Op::Step(op));
    }
    if !V5 && list & 1 << PC != 0 {
        return None;
    }
    let push = rn == SP && !load;
    Some(Op::Block(Block {
        condition: ALWAYS,
        rn,
        list: list as u16,
        load,
        up: !push,
        pre: push,
        writeback: true,
    }))
}

fn jump(condition: u32, target: Target, link: bool, exchange: bool) -> Op {
    Op::Jump(Jump {
        condition,
        target,
        link,
        exchange,
    })
}

fn link_target(op: u32, mask: u32) -> Target {
    Target::Register {
        rm: LR,
        offset: (op & 0x7FF) << 1,
        mask,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOVS_R0_1: u32 = 0x2001;
    const LSLS_R1_R2_3: u32 = 0x00D1;
    const ADD_R8_R1: u32 = 0x4488;
    const MOV_PC_LR: u32 = 0x46F7;
    const LDR_R0_PC_4: u32 = 0x4801;
    const LDR_R1_R2_8: u32 = 0x6891;
    const PUSH_R4_LR: u32 = 0xB510;
    const POP_R4_PC: u32 = 0xBD10;
    const POP_R4: u32 = 0xBC10;
    const STMIA_R0_R1_R2: u32 = 0xC006;
    const MULS_R0_R1: u32 = 0x4348;
    const BX_LR: u32 = 0x4770;
    const BEQ_BACK: u32 = 0xD0FE;
    const BL_HIGH: u32 = 0xF000;
    const BL_LOW: u32 = 0xF802;
    const ADD_R0_PC_8: u32 = 0xA002;
    const SWI_0: u32 = 0xDF00;

    #[test]
    fn data_processing_becomes_the_arm_instruction() {
        assert_eq!(classify::<true>(MOVS_R0_1, 0), Op::Alu(0xE3B0_0001));
        assert_eq!(classify::<true>(LSLS_R1_R2_3, 0), Op::Alu(0xE1B0_1182));
        assert_eq!(classify::<true>(ADD_R8_R1, 0), Op::Alu(0xE088_8001));
        assert_eq!(classify::<true>(MOV_PC_LR, 0), Op::Exit(MOV_PC_LR));
    }

    #[test]
    fn a_literal_load_folds_the_word_alignment_into_its_offset() {
        let Op::Transfer(aligned) = classify::<true>(LDR_R0_PC_4, 0x0200_0000) else {
            panic!("ldr is not a transfer");
        };
        assert_eq!((aligned.rn, aligned.offset), (PC, Offset::Immediate(4)));
        let Op::Transfer(odd) = classify::<true>(0x4800, 0x0200_0002) else {
            panic!("ldr is not a transfer");
        };
        assert_eq!(odd.offset, Offset::Immediate(2));
        assert!(!odd.up());
        let Op::Transfer(plain) = classify::<true>(LDR_R1_R2_8, 0) else {
            panic!("ldr is not a transfer");
        };
        assert_eq!((plain.rn, plain.rd), (2, 1));
        assert_eq!(plain.offset, Offset::Immediate(8));
    }

    #[test]
    fn push_and_pop_become_blocks() {
        let Op::Block(push) = classify::<true>(PUSH_R4_LR, 0) else {
            panic!("push is not a block");
        };
        assert_eq!(push.list, 1 << 4 | 1 << 14);
        assert!(!push.load && !push.up && push.pre && push.writeback);
        let Op::Block(pop) = classify::<true>(POP_R4, 0) else {
            panic!("pop is not a block");
        };
        assert!(pop.load && pop.up && !pop.pre);
        let Op::Block(ret) = classify::<true>(POP_R4_PC, 0) else {
            panic!("pop pc is not a block");
        };
        assert!(ret.loads_pc() && Op::Block(ret).ends());
        assert_eq!(classify::<false>(POP_R4_PC, 0), Op::Exit(POP_R4_PC));
        let Op::Block(stmia) = classify::<true>(STMIA_R0_R1_R2, 0) else {
            panic!("stmia is not a block");
        };
        assert_eq!((stmia.rn, stmia.list), (0, 0b110));
    }

    #[test]
    fn mul_multiplies_by_the_destination() {
        let Op::Multiply(mul) = classify::<true>(MULS_R0_R1, 0) else {
            panic!("mul is not a multiply");
        };
        assert_eq!((mul.rd, mul.rm, mul.rs, mul.set_flags), (0, 1, 0, true));
        assert_eq!(classify::<false>(MULS_R0_R1, 0), Op::Step(MULS_R0_R1));
    }

    #[test]
    fn branches_know_their_targets() {
        let Op::Jump(beq) = classify::<true>(BEQ_BACK, 0x0200_0010) else {
            panic!("beq is not a jump");
        };
        assert_eq!((beq.condition, beq.target), (0, Target::Fixed(0x0200_0010)));
        assert_eq!(
            classify::<true>(BL_HIGH, 0x0200_0000),
            Op::Constant {
                rd: LR,
                value: 0x0200_0004
            }
        );
        let Op::Jump(bl) = classify::<true>(BL_LOW, 0x0200_0002) else {
            panic!("bl is not a jump");
        };
        assert!(bl.link && !bl.exchange);
        let Op::Jump(bx) = classify::<true>(BX_LR, 0) else {
            panic!("bx is not a jump");
        };
        assert!(bx.exchange && !bx.link);
        assert_eq!(classify::<true>(SWI_0, 0), Op::Exit(SWI_0));
    }

    #[test]
    fn an_address_from_pc_is_a_constant() {
        assert_eq!(
            classify::<true>(ADD_R0_PC_8, 0x0200_0002),
            Op::Constant {
                rd: 0,
                value: 0x0200_000C
            }
        );
    }
}
