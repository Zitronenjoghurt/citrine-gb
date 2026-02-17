use std::fmt::Display;

#[allow(non_camel_case_types)]
#[derive(Debug, Copy, Clone)]
pub enum Instruction {
    Nop,
    LD_rr_nn(R16),
    LD_rr_A(R16Mem),
    LD_A_rr(R16Mem),
    LD_nn_SP,
    INC_R16(R16),
    DEC_R16(R16),
    ADD_HL_R16(R16),
}

impl Instruction {
    #[inline(always)]
    pub fn decode(opcode: u8) -> Self {
        match opcode {
            0b00_00_00_00 => Self::Nop,                    // 0x00
            0b00_00_00_01 => Self::LD_rr_nn(R16::BC),      // 0x01
            0b00_00_00_10 => Self::LD_rr_A(R16Mem::BC),    // 0x02
            0b00_00_00_11 => Self::INC_R16(R16::BC),       // 0x03
            0b00_00_10_00 => Self::LD_nn_SP,               // 0x08
            0b00_00_10_01 => Self::ADD_HL_R16(R16::BC),    // 0x09
            0b00_00_10_10 => Self::LD_A_rr(R16Mem::BC),    // 0x0A
            0b00_00_10_11 => Self::DEC_R16(R16::BC),       // 0x0B
            0b00_01_00_01 => Self::LD_rr_nn(R16::DE),      // 0x11
            0b00_01_00_10 => Self::LD_rr_A(R16Mem::DE),    // 0x12
            0b00_01_00_11 => Self::INC_R16(R16::DE),       // 0x13
            0b00_01_10_01 => Self::ADD_HL_R16(R16::DE),    // 0x19
            0b00_01_10_10 => Self::LD_A_rr(R16Mem::DE),    // 0x1A
            0b00_01_10_11 => Self::DEC_R16(R16::DE),       // 0x1B
            0b00_10_00_01 => Self::LD_rr_nn(R16::HL),      // 0x21
            0b00_10_00_10 => Self::LD_rr_A(R16Mem::HLinc), // 0x22
            0b00_10_00_11 => Self::INC_R16(R16::HL),       // 0x23
            0b00_10_10_01 => Self::ADD_HL_R16(R16::HL),    // 0x29
            0b00_10_10_10 => Self::LD_A_rr(R16Mem::HLinc), // 0x2A
            0b00_10_10_11 => Self::DEC_R16(R16::HL),       // 0x2B
            0b00_11_00_01 => Self::LD_rr_nn(R16::SP),      // 0x31
            0b00_11_00_10 => Self::LD_rr_A(R16Mem::HLdec), // 0x32
            0b00_11_00_11 => Self::INC_R16(R16::SP),       // 0x33
            0b00_11_10_01 => Self::ADD_HL_R16(R16::SP),    // 0x39
            0b00_11_10_10 => Self::LD_A_rr(R16Mem::HLdec), // 0x3A
            0b00_11_10_11 => Self::DEC_R16(R16::SP),       // 0x3B
            _ => Self::Nop,
        }
    }

    pub fn length(&self) -> usize {
        match self {
            Self::Nop
            | Self::LD_rr_A(_)
            | Self::LD_A_rr(_)
            | Self::INC_R16(_)
            | Self::DEC_R16(_)
            | Self::ADD_HL_R16(_) => 1,
            Self::LD_rr_nn(_) | Self::LD_nn_SP => 3,
        }
    }

    pub fn string_context(&self, context: &[u8]) -> String {
        let n1 = context.get(1).copied().unwrap_or(0);
        let n2 = context.get(2).copied().unwrap_or(0);
        let nn = u16::from_le_bytes([n1, n2]);

        match self {
            Self::Nop => String::from("NOP"),
            Self::LD_rr_nn(r16) => format!("LD {r16}, {nn:04X}"),
            Self::LD_rr_A(r16mem) => format!("LD {r16mem}, A"),
            Self::LD_A_rr(r16mem) => format!("LD A, {r16mem}"),
            Self::LD_nn_SP => format!("LD {nn:04X}, SP"),
            Self::INC_R16(r16) => format!("INC {r16}"),
            Self::DEC_R16(r16) => format!("DEC {r16}"),
            Self::ADD_HL_R16(r16) => format!("ADD HL, {r16}"),
        }
    }
}

impl Display for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nop => write!(f, "NOP"),
            Self::LD_rr_nn(r16) => write!(f, "LD {r16}, nn"),
            Self::LD_rr_A(r16mem) => write!(f, "LD {r16mem}, A"),
            Self::LD_A_rr(r16mem) => write!(f, "LD A, {r16mem}"),
            Self::LD_nn_SP => write!(f, "LD nn, SP"),
            Self::INC_R16(r16) => write!(f, "INC {r16}"),
            Self::DEC_R16(r16) => write!(f, "DEC {r16}"),
            Self::ADD_HL_R16(r16) => write!(f, "ADD HL, {r16}"),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum R8 {
    B = 0,
    C = 1,
    D = 2,
    E = 3,
    H = 4,
    L = 5,
    HL = 6,
    A = 7,
}

impl Display for R8 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
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
