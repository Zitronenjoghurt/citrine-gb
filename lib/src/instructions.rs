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
    HALT,
    LD_r_r(R8, R8),
    ADD_r(R8),
    ADC_r(R8),
    SUB_r(R8),
    SBC_r(R8),
    AND_r(R8),
    XOR_r(R8),
    OR_r(R8),
    CP_r(R8),
    ADD_n,
    ADC_n,
    SUB_n,
    SBC_n,
    AND_n,
    XOR_n,
    OR_n,
    CP_n,
    POP(R16Stk),
    PUSH(R16Stk),
    RET_c(Cond),
    RET,
    RETI,
    JP_c_nn(Cond),
    JP_nn,
    JP_HL,
    CALL_c_nn(Cond),
    CALL_nn,
    RST_n(u8),
    LDH_C_A,
    LDH_A_C,
    LDH_n_A,
    LDH_A_n,
    LD_nn_A,
    LD_A_nn,
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
            0b01_00_00_00 => Self::LD_r_r(R8::B, R8::B),   // 0x40
            0b01_00_00_01 => Self::LD_r_r(R8::B, R8::C),   // 0x41
            0b01_00_00_10 => Self::LD_r_r(R8::B, R8::D),   // 0x42
            0b01_00_00_11 => Self::LD_r_r(R8::B, R8::E),   // 0x43
            0b01_00_01_00 => Self::LD_r_r(R8::B, R8::H),   // 0x44
            0b01_00_01_01 => Self::LD_r_r(R8::B, R8::L),   // 0x45
            0b01_00_01_10 => Self::LD_r_r(R8::B, R8::HL),  // 0x46
            0b01_00_01_11 => Self::LD_r_r(R8::B, R8::A),   // 0x47
            0b01_00_10_00 => Self::LD_r_r(R8::C, R8::B),   // 0x48
            0b01_00_10_01 => Self::LD_r_r(R8::C, R8::C),   // 0x49
            0b01_00_10_10 => Self::LD_r_r(R8::C, R8::D),   // 0x4A
            0b01_00_10_11 => Self::LD_r_r(R8::C, R8::E),   // 0x4B
            0b01_00_11_00 => Self::LD_r_r(R8::C, R8::H),   // 0x4C
            0b01_00_11_01 => Self::LD_r_r(R8::C, R8::L),   // 0x4D
            0b01_00_11_10 => Self::LD_r_r(R8::C, R8::HL),  // 0x4E
            0b01_00_11_11 => Self::LD_r_r(R8::C, R8::A),   // 0x4F
            0b01_01_00_00 => Self::LD_r_r(R8::D, R8::B),   // 0x50
            0b01_01_00_01 => Self::LD_r_r(R8::D, R8::C),   // 0x51
            0b01_01_00_10 => Self::LD_r_r(R8::D, R8::D),   // 0x52
            0b01_01_00_11 => Self::LD_r_r(R8::D, R8::E),   // 0x53
            0b01_01_01_00 => Self::LD_r_r(R8::D, R8::H),   // 0x54
            0b01_01_01_01 => Self::LD_r_r(R8::D, R8::L),   // 0x55
            0b01_01_01_10 => Self::LD_r_r(R8::D, R8::HL),  // 0x56
            0b01_01_01_11 => Self::LD_r_r(R8::D, R8::A),   // 0x57
            0b01_01_10_00 => Self::LD_r_r(R8::E, R8::B),   // 0x58
            0b01_01_10_01 => Self::LD_r_r(R8::E, R8::C),   // 0x59
            0b01_01_10_10 => Self::LD_r_r(R8::E, R8::D),   // 0x5A
            0b01_01_10_11 => Self::LD_r_r(R8::E, R8::E),   // 0x5B
            0b01_01_11_00 => Self::LD_r_r(R8::E, R8::H),   // 0x5C
            0b01_01_11_01 => Self::LD_r_r(R8::E, R8::L),   // 0x5D
            0b01_01_11_10 => Self::LD_r_r(R8::E, R8::HL),  // 0x5E
            0b01_01_11_11 => Self::LD_r_r(R8::E, R8::A),   // 0x5F
            0b01_10_00_00 => Self::LD_r_r(R8::H, R8::B),   // 0x60
            0b01_10_00_01 => Self::LD_r_r(R8::H, R8::C),   // 0x61
            0b01_10_00_10 => Self::LD_r_r(R8::H, R8::D),   // 0x62
            0b01_10_00_11 => Self::LD_r_r(R8::H, R8::E),   // 0x63
            0b01_10_01_00 => Self::LD_r_r(R8::H, R8::H),   // 0x64
            0b01_10_01_01 => Self::LD_r_r(R8::H, R8::L),   // 0x65
            0b01_10_01_10 => Self::LD_r_r(R8::H, R8::HL),  // 0x66
            0b01_10_01_11 => Self::LD_r_r(R8::H, R8::A),   // 0x67
            0b01_10_10_00 => Self::LD_r_r(R8::L, R8::B),   // 0x68
            0b01_10_10_01 => Self::LD_r_r(R8::L, R8::C),   // 0x69
            0b01_10_10_10 => Self::LD_r_r(R8::L, R8::D),   // 0x6A
            0b01_10_10_11 => Self::LD_r_r(R8::L, R8::E),   // 0x6B
            0b01_10_11_00 => Self::LD_r_r(R8::L, R8::H),   // 0x6C
            0b01_10_11_01 => Self::LD_r_r(R8::L, R8::L),   // 0x6D
            0b01_10_11_10 => Self::LD_r_r(R8::L, R8::HL),  // 0x6E
            0b01_10_11_11 => Self::LD_r_r(R8::L, R8::A),   // 0x6F
            0b01_11_00_00 => Self::LD_r_r(R8::HL, R8::B),  // 0x70
            0b01_11_00_01 => Self::LD_r_r(R8::HL, R8::C),  // 0x71
            0b01_11_00_10 => Self::LD_r_r(R8::HL, R8::D),  // 0x72
            0b01_11_00_11 => Self::LD_r_r(R8::HL, R8::E),  // 0x73
            0b01_11_01_00 => Self::LD_r_r(R8::HL, R8::H),  // 0x74
            0b01_11_01_01 => Self::LD_r_r(R8::HL, R8::L),  // 0x75
            0b01_11_01_10 => Self::HALT,                   // 0x76
            0b01_11_01_11 => Self::LD_r_r(R8::HL, R8::A),  // 0x77
            0b01_11_10_00 => Self::LD_r_r(R8::A, R8::B),   // 0x78
            0b01_11_10_01 => Self::LD_r_r(R8::A, R8::C),   // 0x79
            0b01_11_10_10 => Self::LD_r_r(R8::A, R8::D),   // 0x7A
            0b01_11_10_11 => Self::LD_r_r(R8::A, R8::E),   // 0x7B
            0b01_11_11_00 => Self::LD_r_r(R8::A, R8::H),   // 0x7C
            0b01_11_11_01 => Self::LD_r_r(R8::A, R8::L),   // 0x7D
            0b01_11_11_10 => Self::LD_r_r(R8::A, R8::HL),  // 0x7E
            0b01_11_11_11 => Self::LD_r_r(R8::A, R8::A),   // 0x7F
            0b10_00_00_00 => Self::ADD_r(R8::B),           // 0x80
            0b10_00_00_01 => Self::ADD_r(R8::C),           // 0x81
            0b10_00_00_10 => Self::ADD_r(R8::D),           // 0x82
            0b10_00_00_11 => Self::ADD_r(R8::E),           // 0x83
            0b10_00_01_00 => Self::ADD_r(R8::H),           // 0x84
            0b10_00_01_01 => Self::ADD_r(R8::L),           // 0x85
            0b10_00_01_10 => Self::ADD_r(R8::HL),          // 0x86
            0b10_00_01_11 => Self::ADD_r(R8::A),           // 0x87
            0b10_00_10_00 => Self::ADC_r(R8::B),           // 0x88
            0b10_00_10_01 => Self::ADC_r(R8::C),           // 0x89
            0b10_00_10_10 => Self::ADC_r(R8::D),           // 0x8A
            0b10_00_10_11 => Self::ADC_r(R8::E),           // 0x8B
            0b10_00_11_00 => Self::ADC_r(R8::H),           // 0x8C
            0b10_00_11_01 => Self::ADC_r(R8::L),           // 0x8D
            0b10_00_11_10 => Self::ADC_r(R8::HL),          // 0x8E
            0b10_00_11_11 => Self::ADC_r(R8::A),           // 0x8F
            0b10_01_00_00 => Self::SUB_r(R8::B),           // 0x90
            0b10_01_00_01 => Self::SUB_r(R8::C),           // 0x91
            0b10_01_00_10 => Self::SUB_r(R8::D),           // 0x92
            0b10_01_00_11 => Self::SUB_r(R8::E),           // 0x93
            0b10_01_01_00 => Self::SUB_r(R8::H),           // 0x94
            0b10_01_01_01 => Self::SUB_r(R8::L),           // 0x95
            0b10_01_01_10 => Self::SUB_r(R8::HL),          // 0x96
            0b10_01_01_11 => Self::SUB_r(R8::A),           // 0x97
            0b10_01_10_00 => Self::SBC_r(R8::B),           // 0x98
            0b10_01_10_01 => Self::SBC_r(R8::C),           // 0x99
            0b10_01_10_10 => Self::SBC_r(R8::D),           // 0x9A
            0b10_01_10_11 => Self::SBC_r(R8::E),           // 0x9B
            0b10_01_11_00 => Self::SBC_r(R8::H),           // 0x9C
            0b10_01_11_01 => Self::SBC_r(R8::L),           // 0x9D
            0b10_01_11_10 => Self::SBC_r(R8::HL),          // 0x9E
            0b10_01_11_11 => Self::SBC_r(R8::A),           // 0x9F
            0b10_10_00_00 => Self::AND_r(R8::B),           // 0xA0
            0b10_10_00_01 => Self::AND_r(R8::C),           // 0xA1
            0b10_10_00_10 => Self::AND_r(R8::D),           // 0xA2
            0b10_10_00_11 => Self::AND_r(R8::E),           // 0xA3
            0b10_10_01_00 => Self::AND_r(R8::H),           // 0xA4
            0b10_10_01_01 => Self::AND_r(R8::L),           // 0xA5
            0b10_10_01_10 => Self::AND_r(R8::HL),          // 0xA6
            0b10_10_01_11 => Self::AND_r(R8::A),           // 0xA7
            0b10_10_10_00 => Self::XOR_r(R8::B),           // 0xA8
            0b10_10_10_01 => Self::XOR_r(R8::C),           // 0xA9
            0b10_10_10_10 => Self::XOR_r(R8::D),           // 0xAA
            0b10_10_10_11 => Self::XOR_r(R8::E),           // 0xAB
            0b10_10_11_00 => Self::XOR_r(R8::H),           // 0xAC
            0b10_10_11_01 => Self::XOR_r(R8::L),           // 0xAD
            0b10_10_11_10 => Self::XOR_r(R8::HL),          // 0xAE
            0b10_10_11_11 => Self::XOR_r(R8::A),           // 0xAF
            0b10_11_00_00 => Self::OR_r(R8::B),            // 0xB0
            0b10_11_00_01 => Self::OR_r(R8::C),            // 0xB1
            0b10_11_00_10 => Self::OR_r(R8::D),            // 0xB2
            0b10_11_00_11 => Self::OR_r(R8::E),            // 0xB3
            0b10_11_01_00 => Self::OR_r(R8::H),            // 0xB4
            0b10_11_01_01 => Self::OR_r(R8::L),            // 0xB5
            0b10_11_01_10 => Self::OR_r(R8::HL),           // 0xB6
            0b10_11_01_11 => Self::OR_r(R8::A),            // 0xB7
            0b10_11_10_00 => Self::CP_r(R8::B),            // 0xB8
            0b10_11_10_01 => Self::CP_r(R8::C),            // 0xB9
            0b10_11_10_10 => Self::CP_r(R8::D),            // 0xBA
            0b10_11_10_11 => Self::CP_r(R8::E),            // 0xBB
            0b10_11_11_00 => Self::CP_r(R8::H),            // 0xBC
            0b10_11_11_01 => Self::CP_r(R8::L),            // 0xBD
            0b10_11_11_10 => Self::CP_r(R8::HL),           // 0xBE
            0b10_11_11_11 => Self::CP_r(R8::A),            // 0xBF
            0b11_00_00_00 => Self::RET_c(Cond::NZ),        // 0xC0
            0b11_00_00_01 => Self::POP(R16Stk::BC),        // 0xC1
            0b11_00_00_10 => Self::JP_c_nn(Cond::NZ),      // 0xC2
            0b11_00_00_11 => Self::JP_nn,                  // 0xC3
            0b11_00_01_00 => Self::CALL_c_nn(Cond::NZ),    // 0xC4
            0b11_00_01_01 => Self::PUSH(R16Stk::BC),       // 0xC5
            0b11_00_01_10 => Self::ADD_n,                  // 0xC6
            0b11_00_10_00 => Self::RET_c(Cond::Z),         // 0xC8
            0b11_00_10_01 => Self::RET,                    // 0xC9
            0b11_00_10_10 => Self::JP_c_nn(Cond::Z),       // 0xCA
            0b11_00_11_00 => Self::CALL_c_nn(Cond::Z),     // 0xCC
            0b11_00_11_01 => Self::CALL_nn,                // 0xCD
            0b11_00_11_10 => Self::ADC_n,                  // 0xCE
            0b11_01_00_00 => Self::RET_c(Cond::NC),        // 0xD0
            0b11_01_00_01 => Self::POP(R16Stk::DE),        // 0xD1
            0b11_01_00_10 => Self::JP_c_nn(Cond::NC),      // 0xD2
            0b11_01_01_00 => Self::CALL_c_nn(Cond::NC),    // 0xD4
            0b11_01_01_01 => Self::PUSH(R16Stk::DE),       // 0xD5
            0b11_01_01_10 => Self::SUB_n,                  // 0xD6
            0b11_01_10_00 => Self::RET_c(Cond::C),         // 0xD8
            0b11_01_10_01 => Self::RETI,                   // 0xD9
            0b11_01_10_10 => Self::JP_c_nn(Cond::C),       // 0xDA
            0b11_01_11_00 => Self::CALL_c_nn(Cond::C),     // 0xDC
            0b11_01_11_10 => Self::SBC_n,                  // 0xDE
            0b11_10_00_00 => Self::LDH_n_A,                // 0xE0
            0b11_10_00_01 => Self::POP(R16Stk::HL),        // 0xE1
            0b11_10_00_10 => Self::LDH_C_A,                // 0xE2
            0b11_10_01_01 => Self::PUSH(R16Stk::HL),       // 0xE5
            0b11_10_01_10 => Self::AND_n,                  // 0xE6
            0b11_10_10_01 => Self::JP_HL,                  // 0xE9
            0b11_10_10_10 => Self::LD_nn_A,                // 0xEA
            0b11_10_11_10 => Self::XOR_n,                  // 0xEE
            0b11_11_00_00 => Self::LDH_A_n,                // 0xF0
            0b11_11_00_01 => Self::POP(R16Stk::AF),        // 0xF1
            0b11_11_00_10 => Self::LDH_A_C,                // 0xF2
            0b11_11_01_01 => Self::PUSH(R16Stk::AF),       // 0xF5
            0b11_11_01_10 => Self::OR_n,                   // 0xF6
            0b11_11_10_10 => Self::LD_A_nn,                // 0xFA
            0b11_11_11_10 => Self::CP_n,                   // 0xFE
            0b11_00_01_11 => Self::RST_n(0x00),            // 0xC7
            0b11_00_11_11 => Self::RST_n(0x08),            // 0xCF
            0b11_01_01_11 => Self::RST_n(0x10),            // 0xD7
            0b11_01_11_11 => Self::RST_n(0x18),            // 0xDF
            0b11_10_01_11 => Self::RST_n(0x20),            // 0xE7
            0b11_10_11_11 => Self::RST_n(0x28),            // 0xEF
            0b11_11_01_11 => Self::RST_n(0x30),            // 0xF7
            0b11_11_11_11 => Self::RST_n(0x38),            // 0xFF
            _ => panic!("Invalid unprefixed opcode: {:02X}", opcode),
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
            | Self::STOP
            | Self::HALT
            | Self::LD_r_r(_, _)
            | Self::ADD_r(_)
            | Self::ADC_r(_)
            | Self::SUB_r(_)
            | Self::SBC_r(_)
            | Self::AND_r(_)
            | Self::XOR_r(_)
            | Self::OR_r(_)
            | Self::CP_r(_)
            | Self::POP(_)
            | Self::PUSH(_)
            | Self::RET_c(_)
            | Self::RET
            | Self::RETI
            | Self::JP_HL
            | Self::RST_n(_)
            | Self::LDH_C_A
            | Self::LDH_A_C => 1,
            Self::LD_r_n(_)
            | Self::JR_n
            | Self::JR_c_n(_)
            | Self::ADD_n
            | Self::ADC_n
            | Self::SUB_n
            | Self::SBC_n
            | Self::AND_n
            | Self::XOR_n
            | Self::OR_n
            | Self::CP_n
            | Self::LDH_n_A
            | Self::LDH_A_n => 2,
            Self::LD_rr_nn(_)
            | Self::LD_nn_SP
            | Self::JP_c_nn(_)
            | Self::JP_nn
            | Self::CALL_c_nn(_)
            | Self::CALL_nn
            | Self::LD_nn_A
            | Self::LD_A_nn => 3,
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
            Self::HALT => String::from("HALT"),
            Self::LD_r_r(r81, r82) => format!("LD {r81}, {r82}"),
            Self::ADD_r(r8) => format!("ADD {r8}"),
            Self::ADC_r(r8) => format!("ADC {r8}"),
            Self::SUB_r(r8) => format!("SUB {r8}"),
            Self::SBC_r(r8) => format!("SBC {r8}"),
            Self::AND_r(r8) => format!("AND {r8}"),
            Self::XOR_r(r8) => format!("XOR {r8}"),
            Self::OR_r(r8) => format!("OR {r8}"),
            Self::CP_r(r8) => format!("CP {r8}"),
            Self::ADD_n => format!("ADD {n1:02X}"),
            Self::ADC_n => format!("ADC {n1:02X}"),
            Self::SUB_n => format!("SUB {n1:02X}"),
            Self::SBC_n => format!("SBC {n1:02X}"),
            Self::AND_n => format!("AND {n1:02X}"),
            Self::XOR_n => format!("XOR {n1:02X}"),
            Self::OR_n => format!("OR {n1:02X}"),
            Self::CP_n => format!("CP {n1:02X}"),
            Self::POP(stk) => format!("POP {stk}"),
            Self::PUSH(stk) => format!("PUSH {stk}"),
            Self::RET_c(cond) => format!("RET {cond}"),
            Self::RET => String::from("RET"),
            Self::RETI => String::from("RETI"),
            Self::JP_c_nn(cond) => format!("JP {cond}, {nn:04X}"),
            Self::JP_nn => format!("JP {nn:04X}"),
            Self::JP_HL => String::from("JP HL"),
            Self::CALL_c_nn(cond) => format!("CALL {cond}, {nn:04X}"),
            Self::CALL_nn => format!("CALL {nn:04X}"),
            Self::RST_n(tgt) => format!("RST {tgt:02X}"),
            Self::LDH_C_A => String::from("LDH C, A"),
            Self::LDH_A_C => String::from("LDH A, C"),
            Self::LDH_n_A => format!("LDH {n1:02X}, A"),
            Self::LDH_A_n => format!("LDH A, {n1:02X}"),
            Self::LD_nn_A => format!("LD {nn:04X}, A"),
            Self::LD_A_nn => format!("LD A, {nn:04X}"),
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
            Self::HALT => write!(f, "HALT"),
            Self::LD_r_r(r81, r82) => write!(f, "LD {r81}, {r82}"),
            Self::ADD_r(r8) => write!(f, "ADD {r8}"),
            Self::ADC_r(r8) => write!(f, "ADC {r8}"),
            Self::SUB_r(r8) => write!(f, "SUB {r8}"),
            Self::SBC_r(r8) => write!(f, "SBC {r8}"),
            Self::AND_r(r8) => write!(f, "AND {r8}"),
            Self::XOR_r(r8) => write!(f, "XOR {r8}"),
            Self::OR_r(r8) => write!(f, "OR {r8}"),
            Self::CP_r(r8) => write!(f, "CP {r8}"),
            Self::ADD_n => write!(f, "ADD n"),
            Self::ADC_n => write!(f, "ADC n"),
            Self::SUB_n => write!(f, "SUB n"),
            Self::SBC_n => write!(f, "SBC n"),
            Self::AND_n => write!(f, "AND n"),
            Self::XOR_n => write!(f, "XOR n"),
            Self::OR_n => write!(f, "OR n"),
            Self::CP_n => write!(f, "CP n"),
            Self::POP(stk) => write!(f, "POP {stk}"),
            Self::PUSH(stk) => write!(f, "PUSH {stk}"),
            Self::RET_c(cond) => write!(f, "RET {cond}"),
            Self::RET => write!(f, "RET"),
            Self::RETI => write!(f, "RETI"),
            Self::JP_c_nn(cond) => write!(f, "JP {cond}, nn"),
            Self::JP_nn => write!(f, "JP nn"),
            Self::JP_HL => write!(f, "JP HL"),
            Self::CALL_c_nn(cond) => write!(f, "CALL {cond}, nn"),
            Self::CALL_nn => write!(f, "CALL nn"),
            Self::RST_n(tgt) => write!(f, "RST {tgt:02X}"),
            Self::LDH_C_A => write!(f, "LDH C, A"),
            Self::LDH_A_C => write!(f, "LDH A, C"),
            Self::LDH_n_A => write!(f, "LDH n, A"),
            Self::LDH_A_n => write!(f, "LDH A, n"),
            Self::LD_nn_A => write!(f, "LD nn, A"),
            Self::LD_A_nn => write!(f, "LD A, nn"),
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
pub enum R16Stk {
    BC = 0,
    DE = 1,
    HL = 2,
    AF = 3,
}

impl Display for R16Stk {
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
