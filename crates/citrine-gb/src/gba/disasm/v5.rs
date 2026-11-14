use super::arm::{
    address, arm, bit, coprocessor_register, coprocessor_transfer_operands, done, field,
    halfword_offset, immediate_offset, number, processor, register_offset,
};
use super::thumb::{relative, thumb};
use super::{Decoded, NEVER, Pc, Table, operand, reg, sign_extend, target};
use crate::debug::code::{InstrClass, Operand, OperandKind};
use crate::instructions::FlowControl;

const CLZ: Table<1> = conds!(""; "CLZ");
const SATURATING: Table<4> = conds!(""; "QADD", "QSUB", "QDADD", "QDSUB");
const HALVES: [Table<4>; 4] = [
    conds!(""; "SMLABB", "SMLATB", "SMLABT", "SMLATT"),
    conds!(""; "SMLAWB", "SMULWB", "SMLAWT", "SMULWT"),
    conds!(""; "SMLALBB", "SMLALTB", "SMLALBT", "SMLALTT"),
    conds!(""; "SMULBB", "SMULTB", "SMULBT", "SMULTT"),
];
const DOUBLE: Table<2> = conds!("D"; "LDR", "STR");
const BLX: Table<1> = conds!(""; "BLX");
const PAIR: Table<2> = conds!(""; "MCRR", "MRRC");
const COPROCESSOR: Table<3> = conds!(""; "CDP", "MCR", "MRC");
const UNCONDITIONAL: [&str; 3] = ["CDP2", "MCR2", "MRC2"];
const TRANSFER: [[&str; 2]; 2] = [["STC2", "LDC2"], ["STC2L", "LDC2L"]];

fn plain(mnemonic: &'static str, operands: Vec<Operand>, class: InstrClass) -> Decoded {
    done(mnemonic, operands, class, FlowControl::Continue)
}

fn undefined() -> Decoded {
    done("UND", Vec::new(), InstrClass::Invalid, FlowControl::Invalid)
}

pub fn arm_v5(addr: u32, op: u32) -> Decoded {
    let cond = (op >> 28) as usize;
    let kind = ((op >> 21) & 3) as usize;
    if cond == NEVER {
        unconditional(addr, op)
    } else if op & 0x0FF0_00F0 == 0x0160_0010 {
        plain(
            CLZ[0][cond],
            vec![reg(field(op, 12)), reg(op & 0xF)],
            InstrClass::Bits,
        )
    } else if op & 0x0F90_00F0 == 0x0100_0050 {
        plain(
            SATURATING[kind][cond],
            vec![reg(field(op, 12)), reg(op & 0xF), reg(field(op, 16))],
            InstrClass::Arithmetic,
        )
    } else if op & 0x0F90_0090 == 0x0100_0080 {
        halves(op, cond, kind)
    } else if op & 0x0FF0_00F0 == 0x0120_0030 {
        plain(BLX[0][cond], vec![reg(op & 0xF)], InstrClass::Call)
    } else if op & 0x0FF0_00F0 == 0x0120_0070 {
        let value = (op >> 4) & 0xFFF0 | op & 0xF;
        plain(
            "BKPT",
            vec![operand(format!("0x{value:04X}"), OperandKind::Immediate)],
            InstrClass::Control,
        )
    } else if op & 0x0E10_00D0 == 0x0000_00D0 {
        plain(
            DOUBLE[usize::from(bit(op, 5))][cond],
            vec![reg(field(op, 12)), address(op, halfword_offset(op))],
            InstrClass::Load,
        )
    } else if op & 0x0FE0_0000 == 0x0C40_0000 {
        plain(
            PAIR[usize::from(bit(op, 20))][cond],
            vec![
                processor(op),
                number((op >> 4) & 0xF),
                reg(field(op, 12)),
                reg(field(op, 16)),
                coprocessor_register(op & 0xF),
            ],
            InstrClass::Control,
        )
    } else if (op >> 24) & 0xF == 0xE {
        coprocessor(op, COPROCESSOR.map(|names| names[cond]))
    } else {
        arm(addr, op)
    }
}

fn halves(op: u32, cond: usize, kind: usize) -> Decoded {
    let index = usize::from(bit(op, 5)) | usize::from(bit(op, 6)) << 1;
    let (rd, rn, rs, rm) = (field(op, 16), field(op, 12), field(op, 8), op & 0xF);
    let accumulates = match kind {
        1 => !bit(op, 5),
        3 => false,
        _ => true,
    };
    let operands = match (kind, accumulates) {
        (2, _) => vec![reg(rn), reg(rd), reg(rm), reg(rs)],
        (_, true) => vec![reg(rd), reg(rm), reg(rs), reg(rn)],
        (_, false) => vec![reg(rd), reg(rm), reg(rs)],
    };
    plain(HALVES[kind][index][cond], operands, InstrClass::Arithmetic)
}

fn coprocessor(op: u32, names: [&'static str; 3]) -> Decoded {
    let (mnemonic, mut operands) = if bit(op, 4) {
        (
            names[1 + usize::from(bit(op, 20))],
            vec![processor(op), number((op >> 21) & 7), reg(field(op, 12))],
        )
    } else {
        (
            names[0],
            vec![
                processor(op),
                number(field(op, 20)),
                coprocessor_register(field(op, 12)),
            ],
        )
    };
    operands.extend([
        coprocessor_register(field(op, 16)),
        coprocessor_register(op & 0xF),
        number((op >> 5) & 7),
    ]);
    plain(mnemonic, operands, InstrClass::Control)
}

fn unconditional(addr: u32, op: u32) -> Decoded {
    match (op >> 25) & 7 {
        0b101 => {
            let to = addr
                .wrapping_add(8)
                .wrapping_add(sign_extend(op & 0x00FF_FFFF, 24) << 2)
                .wrapping_add(u32::from(bit(op, 24)) << 1);
            done(
                "BLX",
                vec![target(to)],
                InstrClass::Call,
                FlowControl::Call(Pc {
                    addr: to,
                    thumb: true,
                }),
            )
        }
        0b010 | 0b011 if op & 0x0D70_F000 == 0x0550_F000 => {
            let offset = if bit(op, 25) {
                Some(register_offset(op))
            } else {
                immediate_offset(op, op & 0xFFF)
            };
            plain("PLD", vec![address(op, offset)], InstrClass::Load)
        }
        0b110 => plain(
            TRANSFER[usize::from(bit(op, 22))][usize::from(bit(op, 20))],
            coprocessor_transfer_operands(op),
            InstrClass::Control,
        ),
        0b111 if !bit(op, 24) => coprocessor(op, UNCONDITIONAL),
        _ => undefined(),
    }
}

pub fn thumb_v5(addr: u32, op: u16, next: Option<u16>) -> Decoded {
    let wide = u32::from(op);
    match wide >> 11 {
        0b01000 if wide & 0xFF80 == 0x4780 => Decoded::new(
            2,
            "BLX",
            vec![reg((wide >> 3) & 0xF)],
            InstrClass::Call,
            FlowControl::Continue,
        ),
        0b10111 if wide >> 8 == 0xBE => Decoded::new(
            2,
            "BKPT",
            vec![operand(
                format!("0x{:02X}", wide & 0xFF),
                OperandKind::Immediate,
            )],
            InstrClass::Control,
            FlowControl::Continue,
        ),
        0b11110 => match next.map(u32::from) {
            Some(low) if low >> 11 == 0b11101 => {
                let to = addr
                    .wrapping_add(4)
                    .wrapping_add(sign_extend(wide & 0x7FF, 11) << 12)
                    .wrapping_add((low & 0x7FF) << 1)
                    & !3;
                Decoded::new(
                    4,
                    "BLX",
                    vec![target(to)],
                    InstrClass::Call,
                    FlowControl::Call(Pc {
                        addr: to,
                        thumb: false,
                    }),
                )
            }
            _ => thumb(addr, op, next),
        },
        0b11101 => Decoded::new(
            2,
            "BLX",
            vec![relative("LR", (wide & 0x7FF) << 1)],
            InstrClass::Call,
            FlowControl::Continue,
        ),
        _ => thumb(addr, op, next),
    }
}

#[cfg(test)]
mod tests {
    use super::{arm_v5, thumb_v5};
    use crate::gba::disasm::{Pc, arm, thumb};
    use crate::instructions::FlowControl;

    const CODE: u32 = 0x1000;

    #[test]
    fn arm_forms_follow_the_arm_arm() {
        let cases: &[(u32, &str)] = &[
            (0xE10032A1, "SMLATB R0, R1, R2, R3"),
            (0xE10032C1, "SMLABT R0, R1, R2, R3"),
            (0xE14103C2, "SMLALBT R0, R1, R2, R3"),
            (0xE14103E2, "SMLALTT R0, R1, R2, R3"),
            (0x01600281, "SMULBBEQ R0, R1, R2"),
            (0xE1212374, "BKPT 0x1234"),
            (0x11C020D8, "LDRNED R2, [R0, #0x8]"),
            (0xE10020F1, "STRD R2, [R0, -R1]"),
            (0xE1E020D4, "LDRD R2, [R0, #0x4]!"),
            (0xEC410F02, "MCRR p15, 0, R0, R1, c2"),
            (0xEC510F02, "MRRC p15, 0, R0, R1, c2"),
            (0xEE123405, "CDP p4, 1, c3, c2, c5, 0"),
            (0x1E011F10, "MCRNE p15, 0, R1, c1, c0, 0"),
            (0xFE123405, "CDP2 p4, 1, c3, c2, c5, 0"),
            (0xFE012310, "MCR2 p3, 0, R2, c1, c0, 0"),
            (0xFE112370, "MRC2 p3, 0, R2, c1, c0, 3"),
            (0xFD911402, "LDC2 p4, c1, [R1, #0x8]"),
            (0xFDC11402, "STC2L p4, c1, [R1, #0x8]"),
            (0xF751F102, "PLD [R1, -R2, LSL #2]"),
            (0xF550F010, "PLD [R0, #-0x10]"),
            (0xFAFFFFFE, "BLX 00001000"),
            (0xF1A00000, "UND"),
            (0xE3A00001, "MOV R0, #0x1"),
        ];
        for &(op, text) in cases {
            assert_eq!(arm_v5(CODE, op).text(), text, "{op:08X}");
        }
    }

    #[test]
    fn thumb_forms_follow_the_arm_arm() {
        assert_eq!(thumb_v5(CODE, 0xE87E, None).text(), "BLX LR+0xFC");
        assert_eq!(thumb_v5(CODE, 0xBEAB, None).text(), "BKPT 0xAB");
        assert_eq!(thumb_v5(CODE, 0x47F0, None).text(), "BLX LR");
        assert_eq!(thumb_v5(CODE, 0x2001, None).text(), "MOV R0, #0x1");
    }

    #[test]
    fn blx_switches_the_state_of_its_target() {
        let arm_blx = arm_v5(CODE, 0xFA000002);
        assert_eq!(
            arm_blx.flow,
            FlowControl::Call(Pc {
                addr: 0x1010,
                thumb: true
            })
        );
        let thumb_blx = thumb_v5(CODE, 0xF000, Some(0xE87E));
        assert_eq!(thumb_blx.len, 4);
        assert_eq!(
            thumb_blx.flow,
            FlowControl::Call(Pc {
                addr: 0x1100,
                thumb: false
            })
        );
        assert_eq!(arm_v5(CODE, 0x0A000000).flow, arm(CODE, 0x0A000000).flow);
    }

    #[test]
    fn the_armv4t_decoder_keeps_its_own_names() {
        assert_eq!(arm(CODE, 0xEE011F10).text(), "MCR p15, 0, R1, c1, c0");
        assert_eq!(thumb(CODE, 0xBE00, None).text(), "UND");
    }
}
