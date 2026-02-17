use std::fmt::{Display, Formatter};

#[derive(Debug, Copy, Clone)]
pub enum Instruction {
    Nop,
    LdRrNn(R16),
}

impl Instruction {
    #[inline(always)]
    pub fn decode(opcode: u8) -> Self {
        match opcode {
            0b00_00_00_00 => Self::Nop,             // 0x00
            0b00_00_00_01 => Self::LdRrNn(R16::BC), // 0x01
            0b00_01_00_01 => Self::LdRrNn(R16::DE), // 0x11
            0b00_10_00_01 => Self::LdRrNn(R16::HL), // 0x21
            0b00_11_00_01 => Self::LdRrNn(R16::SP), // 0x31
            _ => Self::Nop,
        }
    }

    pub fn length(&self) -> usize {
        match self {
            Self::Nop => 1,
            Self::LdRrNn(_) => 3,
        }
    }

    pub fn string_context(&self, context: &[u8]) -> String {
        let n1 = context.get(1).copied().unwrap_or(0);
        let n2 = context.get(2).copied().unwrap_or(0);
        let nn = u16::from_le_bytes([n1, n2]);

        match self {
            Self::Nop => String::from("NOP"),
            Self::LdRrNn(r16) => format!("LD {r16}, {nn:04X}"),
        }
    }
}

impl Display for Instruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nop => write!(f, "NOP"),
            Self::LdRrNn(r16) => write!(f, "LD {:?}, NN", r16),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum R16 {
    BC = 0,
    DE = 1,
    HL = 2,
    SP = 3,
}

impl Display for R16 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
