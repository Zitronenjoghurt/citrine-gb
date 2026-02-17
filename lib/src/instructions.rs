use std::fmt::Display;

#[allow(non_camel_case_types)]
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Copy, Clone)]
pub enum Instruction {
    NOP,
    LD_rr_nn(R16),
    LD_rr_A(R16Mem),
    LD_A_rr(R16Mem),
    LD_nn_SP,
    INC_rr(R16),
    DEC_rr(R16),
    ADD_HL_rr(R16),
    INC_r(R8),
    DEC_r(R8),
    LD_r_n(R8),
    /// Rotate left circular accumulator
    RLCA,
    /// Rotate right circular accumulator
    RRCA,
    /// Rotate left accumulator
    RLA,
    /// Rotate right accumulator
    RRA,
    /// Decimal adjust accumulator
    DAA,
    /// Complement accumulator
    CPL,
    /// Set carry flag
    SCF,
    /// Clear carry flag
    CCF,
    /// Jump relative
    JR_n,
    /// Jump relative with condition
    JR_c_n(Cond),
    STOP,
}

impl Instruction {
    #[inline(always)]
    pub fn decode(opcode: u8) -> Self {
        match opcode {
            0b00_00_00_00 => Self::NOP,                    // 0x00
            0b00_00_00_01 => Self::LD_rr_nn(R16::BC),      // 0x01
            0b00_00_00_10 => Self::LD_rr_A(R16Mem::BC),    // 0x02
            0b00_00_00_11 => Self::INC_rr(R16::BC),        // 0x03
            0b00_00_01_00 => Self::INC_r(R8::B),           // 0x04
            0b00_00_01_01 => Self::DEC_r(R8::B),           // 0x05
            0b00_00_01_10 => Self::LD_r_n(R8::B),          // 0x06
            0b00_00_01_11 => Self::RLCA,                   // 0x07
            0b00_00_10_00 => Self::LD_nn_SP,               // 0x08
            0b00_00_10_01 => Self::ADD_HL_rr(R16::BC),     // 0x09
            0b00_00_10_10 => Self::LD_A_rr(R16Mem::BC),    // 0x0A
            0b00_00_10_11 => Self::DEC_rr(R16::BC),        // 0x0B
            0b00_00_11_00 => Self::INC_r(R8::C),           // 0x0C
            0b00_00_11_01 => Self::DEC_r(R8::C),           // 0x0D
            0b00_00_11_10 => Self::LD_r_n(R8::C),          // 0x0E
            0b00_00_11_11 => Self::RRCA,                   // 0x0F
            0b00_01_00_00 => Self::STOP,                   // 0x10
            0b00_01_00_01 => Self::LD_rr_nn(R16::DE),      // 0x11
            0b00_01_00_10 => Self::LD_rr_A(R16Mem::DE),    // 0x12
            0b00_01_00_11 => Self::INC_rr(R16::DE),        // 0x13
            0b00_01_01_00 => Self::INC_r(R8::D),           // 0x14
            0b00_01_01_01 => Self::DEC_r(R8::D),           // 0x15
            0b00_01_01_10 => Self::LD_r_n(R8::D),          // 0x16
            0b00_01_01_11 => Self::RLA,                    // 0x17
            0b00_01_10_00 => Self::JR_n,                   // 0x18
            0b00_01_10_01 => Self::ADD_HL_rr(R16::DE),     // 0x19
            0b00_01_10_10 => Self::LD_A_rr(R16Mem::DE),    // 0x1A
            0b00_01_10_11 => Self::DEC_rr(R16::DE),        // 0x1B
            0b00_01_11_00 => Self::INC_r(R8::E),           // 0x1C
            0b00_01_11_01 => Self::DEC_r(R8::E),           // 0x1D
            0b00_01_11_10 => Self::LD_r_n(R8::E),          // 0x1E
            0b00_01_11_11 => Self::RRA,                    // 0x1F
            0b00_10_00_00 => Self::JR_c_n(Cond::NZ),       // 0x20
            0b00_10_00_01 => Self::LD_rr_nn(R16::HL),      // 0x21
            0b00_10_00_10 => Self::LD_rr_A(R16Mem::HLinc), // 0x22
            0b00_10_00_11 => Self::INC_rr(R16::HL),        // 0x23
            0b00_10_01_00 => Self::INC_r(R8::H),           // 0x24
            0b00_10_01_01 => Self::DEC_r(R8::H),           // 0x25
            0b00_10_01_10 => Self::LD_r_n(R8::H),          // 0x26
            0b00_10_01_11 => Self::DAA,                    // 0x27
            0b00_10_10_00 => Self::JR_c_n(Cond::Z),        // 0x28
            0b00_10_10_01 => Self::ADD_HL_rr(R16::HL),     // 0x29
            0b00_10_10_10 => Self::LD_A_rr(R16Mem::HLinc), // 0x2A
            0b00_10_10_11 => Self::DEC_rr(R16::HL),        // 0x2B
            0b00_10_11_00 => Self::INC_r(R8::L),           // 0x2C
            0b00_10_11_01 => Self::DEC_r(R8::L),           // 0x2D
            0b00_10_11_10 => Self::LD_r_n(R8::L),          // 0x2E
            0b00_10_11_11 => Self::CPL,                    // 0x2F
            0b00_11_00_00 => Self::JR_c_n(Cond::NC),       // 0x30
            0b00_11_00_01 => Self::LD_rr_nn(R16::SP),      // 0x31
            0b00_11_00_10 => Self::LD_rr_A(R16Mem::HLdec), // 0x32
            0b00_11_00_11 => Self::INC_rr(R16::SP),        // 0x33
            0b00_11_01_00 => Self::INC_r(R8::HL),          // 0x34
            0b00_11_01_01 => Self::DEC_r(R8::HL),          // 0x35
            0b00_11_01_10 => Self::LD_r_n(R8::HL),         // 0x36
            0b00_11_01_11 => Self::SCF,                    // 0x37
            0b00_11_10_00 => Self::JR_c_n(Cond::C),        // 0x38
            0b00_11_10_01 => Self::ADD_HL_rr(R16::SP),     // 0x39
            0b00_11_10_10 => Self::LD_A_rr(R16Mem::HLdec), // 0x3A
            0b00_11_10_11 => Self::DEC_rr(R16::SP),        // 0x3B
            0b00_11_11_00 => Self::INC_r(R8::A),           // 0x3C
            0b00_11_11_01 => Self::DEC_r(R8::A),           // 0x3D
            0b00_11_11_10 => Self::LD_r_n(R8::A),          // 0x3E
            0b00_11_11_11 => Self::CCF,                    // 0x3F
            _ => Self::NOP,
        }
    }

    pub fn length(&self) -> usize {
        match self {
            Self::NOP
            | Self::LD_rr_A(_)
            | Self::LD_A_rr(_)
            | Self::INC_rr(_)
            | Self::DEC_rr(_)
            | Self::ADD_HL_rr(_)
            | Self::INC_r(_)
            | Self::DEC_r(_)
            | Self::RLCA
            | Self::RRCA
            | Self::RLA
            | Self::RRA
            | Self::DAA
            | Self::CPL
            | Self::SCF
            | Self::CCF
            | Self::STOP => 1,
            Self::LD_r_n(_) | Self::JR_n | Self::JR_c_n(_) => 2,
            Self::LD_rr_nn(_) | Self::LD_nn_SP => 3,
        }
    }

    pub fn string_context(&self, context: &[u8]) -> String {
        let n1 = context.get(1).copied().unwrap_or(0);
        let n2 = context.get(2).copied().unwrap_or(0);
        let nn = u16::from_le_bytes([n1, n2]);

        match self {
            Self::NOP => String::from("NOP"),
            Self::LD_rr_nn(r16) => format!("LD {r16}, {nn:04X}"),
            Self::LD_rr_A(r16mem) => format!("LD {r16mem}, A"),
            Self::LD_A_rr(r16mem) => format!("LD A, {r16mem}"),
            Self::LD_nn_SP => format!("LD {nn:04X}, SP"),
            Self::INC_rr(r16) => format!("INC {r16}"),
            Self::DEC_rr(r16) => format!("DEC {r16}"),
            Self::ADD_HL_rr(r16) => format!("ADD HL, {r16}"),
            Self::INC_r(r8) => format!("INC {r8}"),
            Self::DEC_r(r8) => format!("DEC {r8}"),
            Self::LD_r_n(r8) => format!("LD {r8}, {n1:02X}"),
            Self::RLCA => String::from("RLCA"),
            Self::RRCA => String::from("RRCA"),
            Self::RLA => String::from("RLA"),
            Self::RRA => String::from("RRA"),
            Self::DAA => String::from("DAA"),
            Self::CPL => String::from("CPL"),
            Self::SCF => String::from("SCF"),
            Self::CCF => String::from("CCF"),
            Self::JR_n => format!("JR {n1:02X}"),
            Self::JR_c_n(cond) => format!("JR {cond}, {n1:02X}"),
            Self::STOP => String::from("STOP"),
        }
    }
}

impl Display for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NOP => write!(f, "NOP"),
            Self::LD_rr_nn(r16) => write!(f, "LD {r16}, nn"),
            Self::LD_rr_A(r16mem) => write!(f, "LD {r16mem}, A"),
            Self::LD_A_rr(r16mem) => write!(f, "LD A, {r16mem}"),
            Self::LD_nn_SP => write!(f, "LD nn, SP"),
            Self::INC_rr(r16) => write!(f, "INC {r16}"),
            Self::DEC_rr(r16) => write!(f, "DEC {r16}"),
            Self::ADD_HL_rr(r16) => write!(f, "ADD HL, {r16}"),
            Self::INC_r(r8) => write!(f, "INC {r8}"),
            Self::DEC_r(r8) => write!(f, "DEC {r8}"),
            Self::LD_r_n(r8) => write!(f, "LD {r8}, n"),
            Self::RLCA => write!(f, "RLCA"),
            Self::RRCA => write!(f, "RRCA"),
            Self::RLA => write!(f, "RLA"),
            Self::RRA => write!(f, "RRA"),
            Self::DAA => write!(f, "DAA"),
            Self::CPL => write!(f, "CPL"),
            Self::SCF => write!(f, "SCF"),
            Self::CCF => write!(f, "CCF"),
            Self::JR_n => write!(f, "JR n"),
            Self::JR_c_n(cond) => write!(f, "JR {cond}, n"),
            Self::STOP => write!(f, "STOP"),
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

#[derive(Debug, Copy, Clone)]
pub enum Cond {
    NZ = 0,
    Z = 1,
    NC = 2,
    C = 3,
}

impl Display for Cond {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
