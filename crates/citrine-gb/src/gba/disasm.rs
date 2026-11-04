macro_rules! conds {
    ($suffix:literal; $($op:literal),+ $(,)?) => {
        [$([
            concat!($op, "EQ", $suffix),
            concat!($op, "NE", $suffix),
            concat!($op, "CS", $suffix),
            concat!($op, "CC", $suffix),
            concat!($op, "MI", $suffix),
            concat!($op, "PL", $suffix),
            concat!($op, "VS", $suffix),
            concat!($op, "VC", $suffix),
            concat!($op, "HI", $suffix),
            concat!($op, "LS", $suffix),
            concat!($op, "GE", $suffix),
            concat!($op, "LT", $suffix),
            concat!($op, "GT", $suffix),
            concat!($op, "LE", $suffix),
            concat!($op, $suffix),
            concat!($op, "NV", $suffix),
        ]),+]
    };
}

mod arm;
mod map;
#[cfg(test)]
mod tests;
mod thumb;

pub use arm::arm;
pub use map::{CodeMap, CodeSource, GbaCode, Pc, code_address, decode_at, line, lines};
pub use thumb::thumb;

use crate::debug::code::{InstrClass, Operand, OperandKind, text};
use crate::instructions::FlowControl;

type Table<const N: usize> = [[&'static str; 16]; N];

const ALWAYS: usize = 14;
const NEVER: usize = 15;
const SP: u32 = 13;
const LR: u32 = 14;
const PC: u32 = 15;

const REGISTERS: [&str; 16] = [
    "R0", "R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10", "R11", "R12", "SP", "LR",
    "PC",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub mnemonic: &'static str,
    pub operands: Vec<Operand>,
    pub class: InstrClass,
    pub flow: FlowControl<Pc>,
    pub len: u8,
}

impl Decoded {
    fn new(
        len: u8,
        mnemonic: &'static str,
        operands: Vec<Operand>,
        class: InstrClass,
        flow: FlowControl<Pc>,
    ) -> Self {
        Self {
            mnemonic,
            operands,
            class,
            flow,
            len,
        }
    }

    pub fn text(&self) -> String {
        text(self.mnemonic, &self.operands)
    }
}

fn operand(text: impl Into<String>, kind: OperandKind) -> Operand {
    Operand {
        text: text.into(),
        kind,
    }
}

fn reg(index: u32) -> Operand {
    operand(name(index), OperandKind::Register)
}

fn name(index: u32) -> &'static str {
    REGISTERS[(index & 0xF) as usize]
}

fn imm(value: u32) -> Operand {
    operand(format!("#0x{value:X}"), OperandKind::Immediate)
}

fn signed(up: bool, value: u32) -> String {
    if up {
        format!("#0x{value:X}")
    } else {
        format!("#-0x{value:X}")
    }
}

fn target(addr: u32) -> Operand {
    operand(format!("{addr:08X}"), OperandKind::Address)
}

fn memory(text: String) -> Operand {
    operand(text, OperandKind::Memory)
}

fn register_list(list: u32) -> Operand {
    let mut parts = Vec::new();
    let mut index = 0;
    while index < 16 {
        if list & (1 << index) == 0 {
            index += 1;
            continue;
        }
        let mut end = index;
        while end < 12 && list & (1 << (end + 1)) != 0 {
            end += 1;
        }
        match end - index {
            0 => parts.push(name(index).to_string()),
            1 => {
                parts.push(name(index).to_string());
                parts.push(name(end).to_string());
            }
            _ => parts.push(format!("{}-{}", name(index), name(end))),
        }
        index = end + 1;
    }
    operand(format!("{{{}}}", parts.join(", ")), OperandKind::Register)
}

fn conditional(cond: usize, flow: FlowControl<Pc>) -> FlowControl<Pc> {
    match (cond, flow) {
        (ALWAYS, flow) => flow,
        (NEVER, _) => FlowControl::Continue,
        (_, FlowControl::Jump(to)) => FlowControl::ConditionalJump(to),
        (_, FlowControl::Return | FlowControl::UnknownJump) => FlowControl::ConditionalReturn,
        (_, flow) => flow,
    }
}

fn sign_extend(value: u32, bits: u32) -> u32 {
    let shift = 32 - bits;
    (((value << shift) as i32) >> shift) as u32
}
