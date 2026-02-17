use std::fmt::Display;

#[derive(Debug, Copy, Clone)]
pub enum Instruction {
    Nop,
    LDrrnn(R16),
    LDrrA(R16Mem),
    LDArr(R16Mem),
    LDnnSP,
}

impl Instruction {
    #[inline(always)]
    pub fn decode(opcode: u8) -> Self {
        match opcode {
            0b00_00_00_00 => Self::Nop,                  // 0x00
            0b00_00_00_01 => Self::LDrrnn(R16::BC),      // 0x01
            0b00_00_00_10 => Self::LDrrA(R16Mem::BC),    // 0x02
            0b00_00_10_00 => Self::LDnnSP,               // 0x08
            0b00_01_00_01 => Self::LDrrnn(R16::DE),      // 0x11
            0b00_01_00_10 => Self::LDrrA(R16Mem::DE),    // 0x12
            0b00_10_00_01 => Self::LDrrnn(R16::HL),      // 0x21
            0b00_10_00_10 => Self::LDrrA(R16Mem::HLinc), // 0x22
            0b00_11_00_01 => Self::LDrrnn(R16::SP),      // 0x31
            0b00_11_00_10 => Self::LDrrA(R16Mem::HLdec), // 0x32
            0b00_00_10_10 => Self::LDArr(R16Mem::BC),    // 0x0A
            0b00_01_10_10 => Self::LDArr(R16Mem::DE),    // 0x1A
            0b00_10_10_10 => Self::LDArr(R16Mem::HLinc), // 0x2A
            0b00_11_10_10 => Self::LDArr(R16Mem::HLdec), // 0x3A
            _ => Self::Nop,
        }
    }

    pub fn length(&self) -> usize {
        match self {
            Self::Nop | Self::LDrrA(_) | Self::LDArr(_) => 1,
            Self::LDrrnn(_) | Self::LDnnSP => 3,
        }
    }

    pub fn string_context(&self, context: &[u8]) -> String {
        let n1 = context.get(1).copied().unwrap_or(0);
        let n2 = context.get(2).copied().unwrap_or(0);
        let nn = u16::from_le_bytes([n1, n2]);

        match self {
            Self::Nop => String::from("NOP"),
            Self::LDrrnn(r16) => format!("LD {r16}, {nn:04X}"),
            Self::LDrrA(r16mem) => format!("LD {r16mem}, A"),
            Self::LDArr(r16mem) => format!("LD A, {r16mem}"),
            Self::LDnnSP => format!("LD {nn:04X}, SP"),
        }
    }
}

impl Display for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nop => write!(f, "NOP"),
            Self::LDrrnn(r16) => write!(f, "LD {r16}, nn"),
            Self::LDrrA(r16mem) => write!(f, "LD {r16mem}, A"),
            Self::LDArr(r16mem) => write!(f, "LD A, {r16mem}"),
            Self::LDnnSP => write!(f, "LD nn, SP"),
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

#[derive(Debug, Copy, Clone)]
pub enum R16Mem {
    BC = 0,
    DE = 1,
    HLinc = 2,
    HLdec = 3,
}

impl Display for R16Mem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
