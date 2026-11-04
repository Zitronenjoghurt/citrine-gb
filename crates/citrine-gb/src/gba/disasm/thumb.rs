use super::{
    Decoded, LR, PC, Pc, Table, imm, memory, name, operand, reg, register_list, sign_extend,
    signed, target,
};
use crate::debug::code::{InstrClass, Operand, OperandKind};
use crate::instructions::FlowControl;

const BRANCH: Table<1> = conds!(""; "B");
const SHIFTS: [&str; 3] = ["LSL", "LSR", "ASR"];
const IMMEDIATE: [&str; 4] = ["MOV", "CMP", "ADD", "SUB"];
const ALU: [(&str, InstrClass); 16] = [
    ("AND", InstrClass::Bits),
    ("EOR", InstrClass::Bits),
    ("LSL", InstrClass::Bits),
    ("LSR", InstrClass::Bits),
    ("ASR", InstrClass::Bits),
    ("ADC", InstrClass::Arithmetic),
    ("SBC", InstrClass::Arithmetic),
    ("ROR", InstrClass::Bits),
    ("TST", InstrClass::Bits),
    ("NEG", InstrClass::Arithmetic),
    ("CMP", InstrClass::Arithmetic),
    ("CMN", InstrClass::Arithmetic),
    ("ORR", InstrClass::Bits),
    ("MUL", InstrClass::Arithmetic),
    ("BIC", InstrClass::Bits),
    ("MVN", InstrClass::Bits),
];
const REGISTER_OFFSET: [&str; 4] = ["STR", "STRB", "LDR", "LDRB"];
const SIGN_EXTENDED: [&str; 4] = ["STRH", "LDSB", "LDRH", "LDSH"];

fn low(op: u32, at: u32) -> u32 {
    (op >> at) & 7
}

fn bit(op: u32, at: u32) -> bool {
    op & (1 << at) != 0
}

fn thumb_pc(addr: u32) -> Pc {
    Pc { addr, thumb: true }
}

fn done(
    mnemonic: &'static str,
    operands: Vec<Operand>,
    class: InstrClass,
    flow: FlowControl<Pc>,
) -> Decoded {
    Decoded::new(2, mnemonic, operands, class, flow)
}

fn plain(mnemonic: &'static str, operands: Vec<Operand>, class: InstrClass) -> Decoded {
    done(mnemonic, operands, class, FlowControl::Continue)
}

fn undefined() -> Decoded {
    done("UND", Vec::new(), InstrClass::Invalid, FlowControl::Invalid)
}

fn offset(base: &str, value: u32) -> Operand {
    if value == 0 {
        memory(format!("[{base}]"))
    } else {
        memory(format!("[{base}, #0x{value:X}]"))
    }
}

pub fn thumb(addr: u32, op: u16, next: Option<u16>) -> Decoded {
    let op = u32::from(op);
    match op >> 11 {
        0b00000..=0b00010 => {
            let kind = (op >> 11) as usize;
            let amount = match (op >> 6) & 0x1F {
                0 if kind != 0 => 32,
                amount => amount,
            };
            plain(
                SHIFTS[kind],
                vec![
                    reg(low(op, 0)),
                    reg(low(op, 3)),
                    operand(format!("#{amount}"), OperandKind::Immediate),
                ],
                InstrClass::Bits,
            )
        }
        0b00011 => {
            let mnemonic = if bit(op, 9) { "SUB" } else { "ADD" };
            let last = if bit(op, 10) {
                imm(low(op, 6))
            } else {
                reg(low(op, 6))
            };
            plain(
                mnemonic,
                vec![reg(low(op, 0)), reg(low(op, 3)), last],
                InstrClass::Arithmetic,
            )
        }
        0b00100..=0b00111 => {
            let kind = ((op >> 11) & 3) as usize;
            let class = if kind == 0 {
                InstrClass::Load
            } else {
                InstrClass::Arithmetic
            };
            plain(
                IMMEDIATE[kind],
                vec![reg(low(op, 8)), imm(op & 0xFF)],
                class,
            )
        }
        0b01000 if !bit(op, 10) => {
            let (mnemonic, class) = ALU[((op >> 6) & 0xF) as usize];
            plain(mnemonic, vec![reg(low(op, 0)), reg(low(op, 3))], class)
        }
        0b01000 => hi_register(op),
        0b01001 => plain(
            "LDR",
            vec![reg(low(op, 8)), offset("PC", (op & 0xFF) << 2)],
            InstrClass::Load,
        ),
        0b01010 | 0b01011 => {
            let table = if bit(op, 9) {
                SIGN_EXTENDED
            } else {
                REGISTER_OFFSET
            };
            plain(
                table[((op >> 10) & 3) as usize],
                vec![
                    reg(low(op, 0)),
                    memory(format!("[{}, {}]", name(low(op, 3)), name(low(op, 6)))),
                ],
                InstrClass::Load,
            )
        }
        0b01100..=0b01111 => {
            let byte = bit(op, 12);
            let load = bit(op, 11);
            let mnemonic = REGISTER_OFFSET[usize::from(load) << 1 | usize::from(byte)];
            let value = (op >> 6) & 0x1F;
            let value = if byte { value } else { value << 2 };
            plain(
                mnemonic,
                vec![reg(low(op, 0)), offset(name(low(op, 3)), value)],
                InstrClass::Load,
            )
        }
        0b10000 | 0b10001 => plain(
            if bit(op, 11) { "LDRH" } else { "STRH" },
            vec![
                reg(low(op, 0)),
                offset(name(low(op, 3)), ((op >> 6) & 0x1F) << 1),
            ],
            InstrClass::Load,
        ),
        0b10010 | 0b10011 => plain(
            if bit(op, 11) { "LDR" } else { "STR" },
            vec![reg(low(op, 8)), offset("SP", (op & 0xFF) << 2)],
            InstrClass::Load,
        ),
        0b10100 | 0b10101 => plain(
            "ADD",
            vec![
                reg(low(op, 8)),
                operand(if bit(op, 11) { "SP" } else { "PC" }, OperandKind::Register),
                imm((op & 0xFF) << 2),
            ],
            InstrClass::Arithmetic,
        ),
        0b10110 | 0b10111 => match (op >> 8) & 0xF {
            0b0000 => plain(
                "ADD",
                vec![
                    operand("SP", OperandKind::Register),
                    operand(
                        signed(!bit(op, 7), (op & 0x7F) << 2),
                        OperandKind::Immediate,
                    ),
                ],
                InstrClass::Arithmetic,
            ),
            0b0100 | 0b0101 => plain(
                "PUSH",
                vec![register_list((op & 0xFF) | u32::from(bit(op, 8)) << LR)],
                InstrClass::Stack,
            ),
            0b1100 => plain("POP", vec![register_list(op & 0xFF)], InstrClass::Stack),
            0b1101 => done(
                "POP",
                vec![register_list((op & 0xFF) | 1 << PC)],
                InstrClass::Return,
                FlowControl::Return,
            ),
            _ => undefined(),
        },
        0b11000 | 0b11001 => plain(
            if bit(op, 11) { "LDMIA" } else { "STMIA" },
            vec![
                operand(format!("{}!", name(low(op, 8))), OperandKind::Register),
                register_list(op & 0xFF),
            ],
            InstrClass::Load,
        ),
        0b11010 | 0b11011 => match (op >> 8) & 0xF {
            0xE => undefined(),
            0xF => plain(
                "SWI",
                vec![operand(
                    format!("0x{:02X}", op & 0xFF),
                    OperandKind::Immediate,
                )],
                InstrClass::Call,
            ),
            cond => {
                let to = addr
                    .wrapping_add(4)
                    .wrapping_add(sign_extend(op & 0xFF, 8) << 1);
                done(
                    BRANCH[0][cond as usize],
                    vec![target(to)],
                    InstrClass::Jump,
                    FlowControl::ConditionalJump(thumb_pc(to)),
                )
            }
        },
        0b11100 => {
            let to = addr
                .wrapping_add(4)
                .wrapping_add(sign_extend(op & 0x7FF, 11) << 1);
            done(
                "B",
                vec![target(to)],
                InstrClass::Jump,
                FlowControl::Jump(thumb_pc(to)),
            )
        }
        0b11110 => {
            let high = sign_extend(op & 0x7FF, 11) << 12;
            match next.map(u32::from) {
                Some(low) if low >> 11 == 0b11111 => {
                    let to = addr
                        .wrapping_add(4)
                        .wrapping_add(high)
                        .wrapping_add((low & 0x7FF) << 1);
                    Decoded::new(
                        4,
                        "BL",
                        vec![target(to)],
                        InstrClass::Call,
                        FlowControl::Call(thumb_pc(to)),
                    )
                }
                _ => plain("BL", vec![relative("PC", high)], InstrClass::Call),
            }
        }
        0b11111 => plain(
            "BL",
            vec![relative("LR", (op & 0x7FF) << 1)],
            InstrClass::Call,
        ),
        _ => undefined(),
    }
}

fn relative(base: &str, offset: u32) -> Operand {
    let text = if (offset as i32) < 0 {
        format!("{base}-0x{:X}", offset.wrapping_neg())
    } else {
        format!("{base}+0x{offset:X}")
    };
    operand(text, OperandKind::Offset)
}

fn hi_register(op: u32) -> Decoded {
    let rd = low(op, 0) | u32::from(bit(op, 7)) << 3;
    let rs = (op >> 3) & 0xF;
    let leaves = |returns: bool| {
        if returns {
            (InstrClass::Return, FlowControl::Return)
        } else {
            (InstrClass::Jump, FlowControl::UnknownJump)
        }
    };
    let (mnemonic, operands, (class, flow)) = match (op >> 8) & 3 {
        0 => (
            "ADD",
            vec![reg(rd), reg(rs)],
            if rd == PC {
                leaves(false)
            } else {
                (InstrClass::Arithmetic, FlowControl::Continue)
            },
        ),
        1 => (
            "CMP",
            vec![reg(rd), reg(rs)],
            (InstrClass::Arithmetic, FlowControl::Continue),
        ),
        2 => (
            "MOV",
            vec![reg(rd), reg(rs)],
            if rd == PC {
                leaves(rs == LR)
            } else {
                (InstrClass::Load, FlowControl::Continue)
            },
        ),
        _ => ("BX", vec![reg(rs)], leaves(rs == LR)),
    };
    done(mnemonic, operands, class, flow)
}
