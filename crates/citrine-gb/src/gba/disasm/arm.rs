use super::{
    Decoded, LR, PC, Pc, SP, Table, conditional, imm, memory, name, operand, reg, register_list,
    sign_extend, signed, target,
};
use crate::debug::code::{InstrClass, Operand, OperandKind};
use crate::instructions::FlowControl;

const DATA: Table<16> = conds!(""; "AND", "EOR", "SUB", "RSB", "ADD", "ADC", "SBC", "RSC",
    "TST", "TEQ", "CMP", "CMN", "ORR", "MOV", "BIC", "MVN");
const DATA_S: Table<16> = conds!("S"; "AND", "EOR", "SUB", "RSB", "ADD", "ADC", "SBC", "RSC",
    "TST", "TEQ", "CMP", "CMN", "ORR", "MOV", "BIC", "MVN");
const MULTIPLY: Table<2> = conds!(""; "MUL", "MLA");
const MULTIPLY_S: Table<2> = conds!("S"; "MUL", "MLA");
const LONG: Table<4> = conds!(""; "UMULL", "UMLAL", "SMULL", "SMLAL");
const LONG_S: Table<4> = conds!("S"; "UMULL", "UMLAL", "SMULL", "SMLAL");
const SINGLE: [Table<2>; 4] = [
    conds!(""; "STR", "LDR"),
    conds!("B"; "STR", "LDR"),
    conds!("T"; "STR", "LDR"),
    conds!("BT"; "STR", "LDR"),
];
const HALF: Table<2> = conds!("H"; "STR", "LDR");
const SIGNED: Table<2> = [conds!("SB"; "LDR")[0], conds!("SH"; "LDR")[0]];
const BLOCK: [Table<2>; 4] = [
    conds!("DA"; "STM", "LDM"),
    conds!("IA"; "STM", "LDM"),
    conds!("DB"; "STM", "LDM"),
    conds!("IB"; "STM", "LDM"),
];
const SWAP: Table<2> = [conds!(""; "SWP")[0], conds!("B"; "SWP")[0]];
const COPROCESSOR: Table<2> = conds!(""; "STC", "LDC");
const COPROCESSOR_LONG: Table<2> = conds!("L"; "STC", "LDC");
const OTHER: Table<9> = conds!(""; "B", "BL", "BX", "SWI", "MRS", "MSR", "CDP", "MCR", "MRC");
const UNDEFINED: Table<1> = conds!(""; "UND");
const SHIFTS: [&str; 4] = ["LSL", "LSR", "ASR", "ROR"];

const B: usize = 0;
const BL: usize = 1;
const BX: usize = 2;
const SWI: usize = 3;
const MRS: usize = 4;
const MSR: usize = 5;
const CDP: usize = 6;
const MCR: usize = 7;
const MRC: usize = 8;

fn bit(op: u32, at: u32) -> bool {
    op & (1 << at) != 0
}

fn field(op: u32, at: u32) -> u32 {
    (op >> at) & 0xF
}

fn arm_pc(addr: u32) -> Pc {
    Pc { addr, thumb: false }
}

fn done(
    mnemonic: &'static str,
    operands: Vec<Operand>,
    class: InstrClass,
    flow: FlowControl<Pc>,
) -> Decoded {
    Decoded::new(4, mnemonic, operands, class, flow)
}

pub fn arm(addr: u32, op: u32) -> Decoded {
    let cond = (op >> 28) as usize;
    let decoded = match (op >> 25) & 7 {
        0b000 => {
            if op & 0x0FFF_FFF0 == 0x012F_FF10 {
                branch_exchange(op, cond)
            } else if op & 0x0FB0_0FF0 == 0x0100_0090 {
                swap(op, cond)
            } else if op & 0x0FC0_00F0 == 0x0000_0090 {
                multiply(op, cond)
            } else if op & 0x0F80_00F0 == 0x0080_0090 {
                multiply_long(op, cond)
            } else if op & 0x0000_0090 == 0x0000_0090 {
                halfword(op, cond)
            } else if op & 0x0190_0000 == 0x0100_0000 {
                psr_transfer(op, cond)
            } else {
                data_processing(op, cond)
            }
        }
        0b001 if op & 0x0190_0000 == 0x0100_0000 => psr_transfer(op, cond),
        0b001 => data_processing(op, cond),
        0b010 => single(op, cond),
        0b011 if op & 0x10 == 0 => single(op, cond),
        0b100 => block(op, cond),
        0b101 => branch(addr, op, cond),
        0b110 => coprocessor_transfer(op, cond),
        0b111 if bit(op, 24) => done(
            OTHER[SWI][cond],
            vec![operand(
                format!("0x{:06X}", op & 0x00FF_FFFF),
                OperandKind::Immediate,
            )],
            InstrClass::Call,
            FlowControl::Continue,
        ),
        0b111 => coprocessor(op, cond),
        _ => done(
            UNDEFINED[0][cond],
            Vec::new(),
            InstrClass::Invalid,
            FlowControl::Invalid,
        ),
    };
    Decoded {
        flow: conditional(cond, decoded.flow),
        ..decoded
    }
}

fn branch(addr: u32, op: u32, cond: usize) -> Decoded {
    let to = addr
        .wrapping_add(8)
        .wrapping_add(sign_extend(op & 0x00FF_FFFF, 24) << 2);
    if bit(op, 24) {
        done(
            OTHER[BL][cond],
            vec![target(to)],
            InstrClass::Call,
            FlowControl::Call(arm_pc(to)),
        )
    } else {
        done(
            OTHER[B][cond],
            vec![target(to)],
            InstrClass::Jump,
            FlowControl::Jump(arm_pc(to)),
        )
    }
}

fn branch_exchange(op: u32, cond: usize) -> Decoded {
    let rm = op & 0xF;
    let (class, flow) = leave(rm == LR);
    done(OTHER[BX][cond], vec![reg(rm)], class, flow)
}

fn leave(returns: bool) -> (InstrClass, FlowControl<Pc>) {
    if returns {
        (InstrClass::Return, FlowControl::Return)
    } else {
        (InstrClass::Jump, FlowControl::UnknownJump)
    }
}

fn data_processing(op: u32, cond: usize) -> Decoded {
    let opcode = field(op, 21) as usize;
    let rn = field(op, 16);
    let rd = field(op, 12);
    let test = (8..=11).contains(&opcode);
    let table = if bit(op, 20) && !test { DATA_S } else { DATA };
    let mut operands = match opcode {
        13 | 15 => vec![reg(rd)],
        8..=11 => vec![reg(rn)],
        _ => vec![reg(rd), reg(rn)],
    };
    let immediate = bit(op, 25);
    if immediate {
        operands.push(imm((op & 0xFF).rotate_right(field(op, 8) * 2)));
    } else {
        operands.push(reg(op & 0xF));
        operands.extend(shift(op));
    }
    let mut class = match opcode {
        13 => InstrClass::Load,
        0 | 1 | 8 | 9 | 12 | 14 | 15 => InstrClass::Bits,
        _ => InstrClass::Arithmetic,
    };
    let mut flow = FlowControl::Continue;
    if rd == PC && !test {
        let moves_lr = opcode == 13 && !immediate && op & 0xFF0 == 0 && op & 0xF == LR;
        let subtracts_lr = opcode == 2 && rn == LR && immediate;
        (class, flow) = leave(moves_lr || subtracts_lr);
    }
    done(table[opcode][cond], operands, class, flow)
}

fn shift(op: u32) -> Option<Operand> {
    let kind = ((op >> 5) & 3) as usize;
    if bit(op, 4) {
        return Some(operand(
            format!("{} {}", SHIFTS[kind], name(field(op, 8))),
            OperandKind::Register,
        ));
    }
    let amount = (op >> 7) & 0x1F;
    let text = match (kind, amount) {
        (0, 0) => return None,
        (3, 0) => "RRX".to_string(),
        (1 | 2, 0) => format!("{} #32", SHIFTS[kind]),
        _ => format!("{} #{amount}", SHIFTS[kind]),
    };
    Some(operand(text, OperandKind::Immediate))
}

fn address(op: u32, offset: Option<String>) -> Operand {
    let rn = name(field(op, 16));
    let pre = bit(op, 24);
    let writeback = if bit(op, 21) { "!" } else { "" };
    let text = match (pre, offset) {
        (true, None) => format!("[{rn}]{writeback}"),
        (true, Some(offset)) => format!("[{rn}, {offset}]{writeback}"),
        (false, offset) => format!("[{rn}], {}", offset.unwrap_or_else(|| "#0x0".into())),
    };
    memory(text)
}

fn immediate_offset(op: u32, value: u32) -> Option<String> {
    let up = bit(op, 23);
    let bare = value == 0 && up && bit(op, 24) && !bit(op, 21);
    (!bare).then(|| signed(up, value))
}

fn register_offset(op: u32) -> String {
    let sign = if bit(op, 23) { "" } else { "-" };
    let rm = name(op & 0xF);
    match shift(op) {
        Some(shift) => format!("{sign}{rm}, {}", shift.text),
        None => format!("{sign}{rm}"),
    }
}

fn load_class(load: bool, rd: u32, returns: bool) -> (InstrClass, FlowControl<Pc>) {
    if load && rd == PC {
        leave(returns)
    } else {
        (InstrClass::Load, FlowControl::Continue)
    }
}

fn single(op: u32, cond: usize) -> Decoded {
    let load = bit(op, 20);
    let rd = field(op, 12);
    let user = !bit(op, 24) && bit(op, 21);
    let table = SINGLE[usize::from(bit(op, 22)) | usize::from(user) << 1];
    let offset = if bit(op, 25) {
        Some(register_offset(op))
    } else {
        immediate_offset(op, op & 0xFFF)
    };
    let pops = field(op, 16) == SP && op & 0x03E0_0FFF == 0x0080_0004;
    let (class, flow) = load_class(load, rd, pops);
    done(
        table[usize::from(load)][cond],
        vec![reg(rd), address(op, offset)],
        class,
        flow,
    )
}

fn halfword(op: u32, cond: usize) -> Decoded {
    let load = bit(op, 20);
    let rd = field(op, 12);
    let mnemonic = match (load, (op >> 5) & 3) {
        (false, 1) => HALF[0][cond],
        (true, 1) => HALF[1][cond],
        (true, 2) => SIGNED[0][cond],
        (true, 3) => SIGNED[1][cond],
        _ => {
            return done(
                UNDEFINED[0][cond],
                Vec::new(),
                InstrClass::Invalid,
                FlowControl::Invalid,
            );
        }
    };
    let offset = if bit(op, 22) {
        immediate_offset(op, ((op >> 4) & 0xF0) | (op & 0xF))
    } else {
        let sign = if bit(op, 23) { "" } else { "-" };
        Some(format!("{sign}{}", name(op & 0xF)))
    };
    let (class, flow) = load_class(load, rd, false);
    done(mnemonic, vec![reg(rd), address(op, offset)], class, flow)
}

fn block(op: u32, cond: usize) -> Decoded {
    let load = bit(op, 20);
    let rn = field(op, 16);
    let writeback = bit(op, 21);
    let mode = usize::from(bit(op, 24)) << 1 | usize::from(bit(op, 23));
    let base = if writeback {
        format!("{}!", name(rn))
    } else {
        name(rn).to_string()
    };
    let mut list = register_list(op & 0xFFFF);
    if bit(op, 22) {
        list.text.push('^');
    }
    let (class, flow) = if load && bit(op, 15) {
        (InstrClass::Return, FlowControl::Return)
    } else if rn == SP && writeback {
        (InstrClass::Stack, FlowControl::Continue)
    } else {
        (InstrClass::Load, FlowControl::Continue)
    };
    done(
        BLOCK[mode][usize::from(load)][cond],
        vec![operand(base, OperandKind::Register), list],
        class,
        flow,
    )
}

fn swap(op: u32, cond: usize) -> Decoded {
    done(
        SWAP[usize::from(bit(op, 22))][cond],
        vec![
            reg(field(op, 12)),
            reg(op & 0xF),
            memory(format!("[{}]", name(field(op, 16)))),
        ],
        InstrClass::Load,
        FlowControl::Continue,
    )
}

fn multiply(op: u32, cond: usize) -> Decoded {
    let accumulate = bit(op, 21);
    let table = if bit(op, 20) { MULTIPLY_S } else { MULTIPLY };
    let mut operands = vec![reg(field(op, 16)), reg(op & 0xF), reg(field(op, 8))];
    if accumulate {
        operands.push(reg(field(op, 12)));
    }
    done(
        table[usize::from(accumulate)][cond],
        operands,
        InstrClass::Arithmetic,
        FlowControl::Continue,
    )
}

fn multiply_long(op: u32, cond: usize) -> Decoded {
    let table = if bit(op, 20) { LONG_S } else { LONG };
    let kind = usize::from(bit(op, 22)) << 1 | usize::from(bit(op, 21));
    done(
        table[kind][cond],
        vec![
            reg(field(op, 12)),
            reg(field(op, 16)),
            reg(op & 0xF),
            reg(field(op, 8)),
        ],
        InstrClass::Arithmetic,
        FlowControl::Continue,
    )
}

fn psr_transfer(op: u32, cond: usize) -> Decoded {
    let psr = if bit(op, 22) { "SPSR" } else { "CPSR" };
    if !bit(op, 21) {
        return done(
            OTHER[MRS][cond],
            vec![reg(field(op, 12)), operand(psr, OperandKind::Register)],
            InstrClass::Load,
            FlowControl::Continue,
        );
    }
    let fields = match field(op, 16) {
        0b1001 => String::new(),
        0b1000 => "_flg".into(),
        0b0001 => "_ctl".into(),
        mask => {
            let letters: String = [(8, 'f'), (4, 's'), (2, 'x'), (1, 'c')]
                .into_iter()
                .filter(|&(bit, _)| mask & bit != 0)
                .map(|(_, letter)| letter)
                .collect();
            format!("_{letters}")
        }
    };
    let source = if bit(op, 25) {
        imm((op & 0xFF).rotate_right(field(op, 8) * 2))
    } else {
        reg(op & 0xF)
    };
    done(
        OTHER[MSR][cond],
        vec![
            operand(format!("{psr}{fields}"), OperandKind::Register),
            source,
        ],
        InstrClass::Control,
        FlowControl::Continue,
    )
}

fn coprocessor_register(index: u32) -> Operand {
    operand(format!("c{index}"), OperandKind::Register)
}

fn number(value: u32) -> Operand {
    operand(value.to_string(), OperandKind::Immediate)
}

fn processor(op: u32) -> Operand {
    operand(format!("p{}", field(op, 8)), OperandKind::Register)
}

fn coprocessor_transfer(op: u32, cond: usize) -> Decoded {
    let table = if bit(op, 22) {
        COPROCESSOR_LONG
    } else {
        COPROCESSOR
    };
    let offset = immediate_offset(op, (op & 0xFF) << 2);
    done(
        table[usize::from(bit(op, 20))][cond],
        vec![
            processor(op),
            coprocessor_register(field(op, 12)),
            address(op, offset),
        ],
        InstrClass::Control,
        FlowControl::Continue,
    )
}

fn coprocessor(op: u32, cond: usize) -> Decoded {
    let info = (op >> 5) & 7;
    let (mnemonic, mut operands) = if bit(op, 4) {
        let mnemonic = if bit(op, 20) { OTHER[MRC] } else { OTHER[MCR] };
        (
            mnemonic[cond],
            vec![
                processor(op),
                number((op >> 21) & 7),
                reg(field(op, 12)),
                coprocessor_register(field(op, 16)),
                coprocessor_register(op & 0xF),
            ],
        )
    } else {
        (
            OTHER[CDP][cond],
            vec![
                processor(op),
                number(field(op, 20)),
                coprocessor_register(field(op, 12)),
                coprocessor_register(field(op, 16)),
                coprocessor_register(op & 0xF),
            ],
        )
    };
    if info != 0 {
        operands.push(number(info));
    }
    done(
        mnemonic,
        operands,
        InstrClass::Control,
        FlowControl::Continue,
    )
}
