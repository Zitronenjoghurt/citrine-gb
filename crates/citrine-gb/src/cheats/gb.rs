use super::ops::{Kind, Op, OpList, RomPatch};
use super::{Cheat, CheatError, Format, Program};

pub(crate) const BANK_SHIFT: u32 = 16;

pub(super) fn parse(lines: &[(usize, String)]) -> Result<Cheat, CheatError> {
    let mut list = OpList::default();
    let mut format = None;
    for (number, line) in lines {
        let found = if line.len() == 8 && !line.contains('-') {
            game_shark(line, &mut list)
        } else {
            game_genie(line, &mut list)
        }
        .ok_or(CheatError::Unreadable { line: *number })?;
        format.get_or_insert(found);
    }
    Ok(Cheat {
        format: format.ok_or(CheatError::Empty)?,
        program: Program::Ops(list),
    })
}

fn game_shark(line: &str, list: &mut OpList) -> Option<Format> {
    let op = u32::from_str_radix(line, 16).ok()?;
    let kind = (op >> 24) as u8;
    let value = (op >> 16) & 0xFF;
    let address = (op & 0xFF) << 8 | (op >> 8) & 0xFF;
    let bank = match kind & 0xF0 {
        0x90 if (0xD000..0xE000).contains(&address) => u32::from((kind & 0x0F).max(1)) + 1,
        _ => 0,
    };
    list.ops.push(Op::new(
        Kind::Assign,
        1,
        address | bank << BANK_SHIFT,
        value,
    ));
    Some(Format::GameShark)
}

fn game_genie(line: &str, list: &mut OpList) -> Option<Format> {
    let digits: String = line.chars().filter(|&c| c != '-').collect();
    if !matches!(digits.len(), 6 | 9) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let group = |at: usize| u16::from_str_radix(&digits[at..at + 3], 16).ok();
    let first = group(0)?;
    let second = group(3)?;
    let address = (first & 0xF) << 8 | (second >> 4) & 0xFF | ((second & 0xF) ^ 0xF) << 12;
    if address > 0x7FFF {
        return None;
    }
    let check = match digits.len() {
        9 => {
            let third = u32::from(group(6)?);
            let mut value = ((third & 0xF00) << 20 | (third & 0xF)).rotate_right(2);
            value |= value >> 24;
            Some((value ^ 0xBA) & 0xFF)
        }
        _ => None,
    };
    list.patches.push(RomPatch {
        address: u32::from(address),
        width: 1,
        value: u32::from(first >> 4),
        check,
    });
    Some(Format::GameGenie)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::ConsoleKind;

    fn list(code: &str) -> OpList {
        match Cheat::parse(ConsoleKind::Gbc, code).unwrap().program {
            Program::Ops(list) => list,
            #[cfg(feature = "nds")]
            Program::Ds(_) => unreachable!(),
        }
    }

    #[test]
    fn a_game_shark_code_writes_its_byte_to_its_little_endian_address() {
        let list = list("01FF47D3");
        assert_eq!(list.ops, [Op::new(Kind::Assign, 1, 0xD347, 0xFF)]);
        assert_eq!(
            Cheat::parse(ConsoleKind::Gb, "01FF47D3").unwrap().format(),
            Format::GameShark
        );
    }

    #[test]
    fn a_game_shark_code_of_type_9x_writes_that_work_ram_bank() {
        let list = list("9363C9D8");
        assert_eq!(list.ops[0].address, 0xD8C9 | 4 << BANK_SHIFT);
        assert_eq!(list.ops[0].operand, 0x63);
    }

    #[test]
    fn a_game_genie_code_with_a_compare_decodes_as_sameboy_and_mgba_do() {
        let list = list("00A-17B-C49");
        assert_eq!(
            list.patches,
            [RomPatch {
                address: 0x4A17,
                width: 1,
                value: 0x00,
                check: Some(0xC8),
            }]
        );
        assert_eq!(
            Cheat::parse(ConsoleKind::Gb, "00A-17B-C49")
                .unwrap()
                .format(),
            Format::GameGenie
        );
    }

    #[test]
    fn a_six_digit_game_genie_code_patches_without_a_compare() {
        let list = list("3EA-0BF");
        assert_eq!(
            list.patches,
            [RomPatch {
                address: 0x0A0B,
                width: 1,
                value: 0x3E,
                check: None,
            }]
        );
    }

    #[test]
    fn a_game_genie_code_past_the_rom_is_refused() {
        assert_eq!(
            Cheat::parse(ConsoleKind::Gb, "00A-170-C49"),
            Err(CheatError::Unreadable { line: 1 })
        );
    }

    #[test]
    fn every_line_of_a_cheat_must_read() {
        assert_eq!(
            Cheat::parse(ConsoleKind::Gb, "01FF47D3\nnope"),
            Err(CheatError::Unreadable { line: 2 })
        );
        assert_eq!(list("01FF47D3\n\n 01 05 48 D3 ").ops.len(), 2);
    }
}
