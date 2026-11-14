use super::gameshark::{decrypt, reseed};
use super::{
    BASE_CART0, BASE_IO, DEADFACE, OFFSET_MASK, Set, TAG_CODE, Version, address_is_real, if_button,
};
use crate::cheats::ops::{Kind, Op, RomPatch};

pub(super) const SEEDS: [u32; 4] = [0x7AA9_648F, 0x7FAE_6994, 0xC0EF_AAD5, 0x4271_2C57];

const COND: u32 = 0x3800_0000;
const COND_EQ: u32 = 0x0800_0000;
const COND_NE: u32 = 0x1000_0000;
const COND_LT: u32 = 0x1800_0000;
const COND_GT: u32 = 0x2000_0000;
const COND_ULT: u32 = 0x2800_0000;
const COND_UGT: u32 = 0x3000_0000;
const COND_AND: u32 = 0x3800_0000;
const WIDTH: u32 = 0x0600_0000;
const WIDTH_BASE: u32 = 25;
const ACTION: u32 = 0xC000_0000;
const ACTION_NEXT: u32 = 0x0000_0000;
const ACTION_NEXT_TWO: u32 = 0x4000_0000;
const ACTION_BLOCK: u32 = 0x8000_0000;
const ACTION_DISABLE: u32 = 0xC000_0000;
const BASE: u32 = 0xC000_0000;
const BASE_ASSIGN: u32 = 0x0000_0000;
const BASE_INDIRECT: u32 = 0x4000_0000;
const BASE_ADD: u32 = 0x8000_0000;
const OTHER_END: u32 = 0x0000_0000;
const OTHER_SLOWDOWN: u32 = 0x0800_0000;
const OTHER_BUTTON_1: u32 = 0x1000_0000;
const OTHER_BUTTON_2: u32 = 0x1200_0000;
const OTHER_BUTTON_4: u32 = 0x1400_0000;
const OTHER_PATCH_1: u32 = 0x1800_0000;
const OTHER_PATCH_2: u32 = 0x1A00_0000;
const OTHER_PATCH_3: u32 = 0x1C00_0000;
const OTHER_PATCH_4: u32 = 0x1E00_0000;
const OTHER_ENDIF: u32 = 0x4000_0000;
const OTHER_ELSE: u32 = 0x6000_0000;
const OTHER_FILL_1: u32 = 0x8000_0000;
const OTHER_FILL_2: u32 = 0x8200_0000;
const OTHER_FILL_4: u32 = 0x8400_0000;

const T1: [u8; 256] = [
    0xD0, 0xFF, 0xBA, 0xE5, 0xC1, 0xC7, 0xDB, 0x5B, 0x16, 0xE3, 0x6E, 0x26, 0x62, 0x31, 0x2E, 0x2A,
    0xD1, 0xBB, 0x4A, 0xE6, 0xAE, 0x2F, 0x0A, 0x90, 0x29, 0x90, 0xB6, 0x67, 0x58, 0x2A, 0xB4, 0x45,
    0x7B, 0xCB, 0xF0, 0x73, 0x84, 0x30, 0x81, 0xC2, 0xD7, 0xBE, 0x89, 0xD7, 0x4E, 0x73, 0x5C, 0xC7,
    0x80, 0x1B, 0xE5, 0xE4, 0x43, 0xC7, 0x46, 0xD6, 0x6F, 0x7B, 0xBF, 0xED, 0xE5, 0x27, 0xD1, 0xB5,
    0xD0, 0xD8, 0xA3, 0xCB, 0x2B, 0x30, 0xA4, 0xF0, 0x84, 0x14, 0x72, 0x5C, 0xFF, 0xA4, 0xFB, 0x54,
    0x9D, 0x70, 0xE2, 0xFF, 0xBE, 0xE8, 0x24, 0x76, 0xE5, 0x15, 0xFB, 0x1A, 0xBC, 0x87, 0x02, 0x2A,
    0x58, 0x8F, 0x9A, 0x95, 0xBD, 0xAE, 0x8D, 0x0C, 0xA5, 0x4C, 0xF2, 0x5C, 0x7D, 0xAD, 0x51, 0xFB,
    0xB1, 0x22, 0x07, 0xE0, 0x29, 0x7C, 0xEB, 0x98, 0x14, 0xC6, 0x31, 0x97, 0xE4, 0x34, 0x8F, 0xCC,
    0x99, 0x56, 0x9F, 0x78, 0x43, 0x91, 0x85, 0x3F, 0xC2, 0xD0, 0xD1, 0x80, 0xD1, 0x77, 0xA7, 0xE2,
    0x43, 0x99, 0x1D, 0x2F, 0x8B, 0x6A, 0xE4, 0x66, 0x82, 0xF7, 0x2B, 0x0B, 0x65, 0x14, 0xC0, 0xC2,
    0x1D, 0x96, 0x78, 0x1C, 0xC4, 0xC3, 0xD2, 0xB1, 0x64, 0x07, 0xD7, 0x6F, 0x02, 0xE9, 0x44, 0x31,
    0xDB, 0x3C, 0xEB, 0x93, 0xED, 0x9A, 0x57, 0x05, 0xB9, 0x0E, 0xAF, 0x1F, 0x48, 0x11, 0xDC, 0x35,
    0x6C, 0xB8, 0xEE, 0x2A, 0x48, 0x2B, 0xBC, 0x89, 0x12, 0x59, 0xCB, 0xD1, 0x18, 0xEA, 0x72, 0x11,
    0x01, 0x75, 0x3B, 0xB5, 0x56, 0xF4, 0x8B, 0xA0, 0x41, 0x75, 0x86, 0x7B, 0x94, 0x12, 0x2D, 0x4C,
    0x0C, 0x22, 0xC9, 0x4A, 0xD8, 0xB1, 0x8D, 0xF0, 0x55, 0x2E, 0x77, 0x50, 0x1C, 0x64, 0x77, 0xAA,
    0x3E, 0xAC, 0xD3, 0x3D, 0xCE, 0x60, 0xCA, 0x5D, 0xA0, 0x92, 0x78, 0xC6, 0x51, 0xFE, 0xF9, 0x30,
];

const T2: [u8; 256] = [
    0xAA, 0xAF, 0xF0, 0x72, 0x90, 0xF7, 0x71, 0x27, 0x06, 0x11, 0xEB, 0x9C, 0x37, 0x12, 0x72, 0xAA,
    0x65, 0xBC, 0x0D, 0x4A, 0x76, 0xF6, 0x5C, 0xAA, 0xB0, 0x7A, 0x7D, 0x81, 0xC1, 0xCE, 0x2F, 0x9F,
    0x02, 0x75, 0x38, 0xC8, 0xFC, 0x66, 0x05, 0xC2, 0x2C, 0xBD, 0x91, 0xAD, 0x03, 0xB1, 0x88, 0x93,
    0x31, 0xC6, 0xAB, 0x40, 0x23, 0x43, 0x76, 0x54, 0xCA, 0xE7, 0x00, 0x96, 0x9F, 0xD8, 0x24, 0x8B,
    0xE4, 0xDC, 0xDE, 0x48, 0x2C, 0xCB, 0xF7, 0x84, 0x1D, 0x45, 0xE5, 0xF1, 0x75, 0xA0, 0xED, 0xCD,
    0x4B, 0x24, 0x8A, 0xB3, 0x98, 0x7B, 0x12, 0xB8, 0xF5, 0x63, 0x97, 0xB3, 0xA6, 0xA6, 0x0B, 0xDC,
    0xD8, 0x4C, 0xA8, 0x99, 0x27, 0x0F, 0x8F, 0x94, 0x63, 0x0F, 0xB0, 0x11, 0x94, 0xC7, 0xE9, 0x7F,
    0x3B, 0x40, 0x72, 0x4C, 0xDB, 0x84, 0x78, 0xFE, 0xB8, 0x56, 0x08, 0x80, 0xDF, 0x20, 0x2F, 0xB9,
    0x66, 0x2D, 0x60, 0x63, 0xF5, 0x18, 0x15, 0x1B, 0x86, 0x85, 0xB9, 0xB4, 0x68, 0x0E, 0xC6, 0xD1,
    0x8A, 0x81, 0x2B, 0xB3, 0xF6, 0x48, 0xF0, 0x4F, 0x9C, 0x28, 0x1C, 0xA4, 0x51, 0x2F, 0xD7, 0x4B,
    0x17, 0xE7, 0xCC, 0x50, 0x9F, 0xD0, 0xD1, 0x40, 0x0C, 0x0D, 0xCA, 0x83, 0xFA, 0x5E, 0xCA, 0xEC,
    0xBF, 0x4E, 0x7C, 0x8F, 0xF0, 0xAE, 0xC2, 0xD3, 0x28, 0x41, 0x9B, 0xC8, 0x04, 0xB9, 0x4A, 0xBA,
    0x72, 0xE2, 0xB5, 0x06, 0x2C, 0x1E, 0x0B, 0x2C, 0x7F, 0x11, 0xA9, 0x26, 0x51, 0x9D, 0x3F, 0xF8,
    0x62, 0x11, 0x2E, 0x89, 0xD2, 0x9D, 0x35, 0xB1, 0xE4, 0x0A, 0x4D, 0x93, 0x01, 0xA7, 0xD1, 0x2D,
    0x00, 0x87, 0xE2, 0x2D, 0xA4, 0xE9, 0x0A, 0x06, 0x66, 0xF8, 0x1F, 0x44, 0x75, 0xB5, 0x6B, 0x1C,
    0xFC, 0x31, 0x09, 0x48, 0xA3, 0xFF, 0x92, 0x12, 0x58, 0xE9, 0xFA, 0xAE, 0x4F, 0xE2, 0xB4, 0xCC,
];

fn par_address(x: u32) -> u32 {
    (x & 0x000F_FFFF) | ((x << 4) & 0x0F00_0000)
}

fn mask(width: u32) -> u32 {
    match width {
        0 => 0,
        1..4 => u32::MAX >> ((4 - width) * 8),
        _ => u32::MAX,
    }
}

impl Set {
    pub(super) fn add_par(&mut self, op1: u32, op2: u32) -> bool {
        match self.gsa_version {
            Version::ParV3Raw => self.add_par_raw(op1, op2),
            Version::ParV3 => {
                let (op1, op2) = decrypt(op1, op2, &self.gsa_seeds);
                self.add_par_raw(op1, op2)
            }
            _ => {
                self.set_version(Version::ParV3);
                let (op1, op2) = decrypt(op1, op2, &self.gsa_seeds);
                self.add_par_raw(op1, op2)
            }
        }
    }

    pub(super) fn add_par_raw(&mut self, op1: u32, op2: u32) -> bool {
        if let Some(index) = self.incomplete_patch.take() {
            if let Some(patch) = self.list.patches.get_mut(index) {
                patch.value = op1;
            }
            return true;
        }
        if let Some(index) = self.incomplete_cheat.take() {
            let after_button = index
                .checked_sub(1)
                .and_then(|before| self.list.ops.get(before))
                .is_some_and(|op| op.kind == Kind::IfButton);
            if let Some(op) = self.list.ops.get_mut(index) {
                op.operand = op1 & mask(u32::from(op.width));
                if !after_button {
                    op.operand_offset = (op2 >> 24) as i32;
                    op.repeat = (op2 >> 16) & 0xFF;
                    op.address_offset = (op2 & 0xFFFF).wrapping_mul(u32::from(op.width)) as i32;
                }
            }
            return true;
        }

        if op2 == TAG_CODE {
            return true;
        }
        match op1 {
            0 => return self.add_par_special(op2),
            DEADFACE => {
                reseed(&mut self.gsa_seeds, op2 as u16, &T1, &T2);
                return true;
            }
            _ => {}
        }
        if op1 >> 24 == 0xC4 {
            return self.take_hook();
        }
        if op1 & COND != 0 {
            return self.add_par_cond(op1, op2);
        }

        let mut width = 1 << ((op1 & WIDTH) >> WIDTH_BASE);
        let mut op = Op::new(Kind::Assign, 0, par_address(op1), 0);
        match op1 & BASE {
            BASE_ASSIGN => {
                op.address_offset = width as i32;
                if width < 4 {
                    op.repeat = (op2 >> (width * 8)).wrapping_add(1);
                }
            }
            BASE_INDIRECT => {
                op.kind = Kind::AssignIndirect;
                if width < 4 {
                    op.address_offset = (op2 >> (width * 8)).wrapping_mul(width) as i32;
                }
            }
            BASE_ADD => op.kind = Kind::Add,
            _ => {
                width = ((op1 >> 24) & 1) + 1;
                op.address = BASE_IO | (op1 & OFFSET_MASK);
            }
        }
        if op1 & 0x0100_0000 != 0 && op1 & 0xFE00_0000 != 0xC600_0000 {
            return false;
        }
        op.width = width as u8;
        op.operand = op2 & mask(width);
        self.push(op);
        true
    }

    fn add_par_cond(&mut self, op1: u32, op2: u32) -> bool {
        let width = 1 << ((op1 & WIDTH) >> WIDTH_BASE);
        if op1 & ACTION == ACTION_DISABLE {
            return false;
        }
        let kind = match op1 & COND {
            COND_EQ => Kind::IfEq,
            COND_NE => Kind::IfNe,
            COND_LT => Kind::IfLt,
            COND_GT => Kind::IfGt,
            COND_ULT => Kind::IfUlt,
            COND_UGT => Kind::IfUgt,
            COND_AND => Kind::IfAnd,
            _ => Kind::IfLand,
        };
        let mut op = Op::new(kind, width as u8, par_address(op1), op2 & mask(width));
        if width > 4 {
            op.width = 0;
            op.kind = Kind::Never;
        }
        match op1 & ACTION {
            ACTION_NEXT => op.repeat = 1,
            ACTION_NEXT_TWO => op.repeat = 2,
            ACTION_BLOCK => op.repeat = 0,
            _ => {}
        }
        let index = self.push(op);
        if op1 & ACTION == ACTION_BLOCK {
            if self.current_block.is_some() {
                self.end_block();
            }
            self.current_block = Some(index);
        }
        true
    }

    fn add_par_special(&mut self, op2: u32) -> bool {
        let address = par_address(op2);
        match op2 & 0xFF00_0000 {
            OTHER_SLOWDOWN => false,
            OTHER_BUTTON_1 | OTHER_BUTTON_2 | OTHER_BUTTON_4 => {
                let width = 1 << ((op2 & WIDTH) >> WIDTH_BASE);
                self.push(if_button());
                let index = self.push(Op::new(Kind::Assign, width, address, 0));
                self.incomplete_cheat = Some(index);
                true
            }
            OTHER_PATCH_1 | OTHER_PATCH_2 | OTHER_PATCH_3 | OTHER_PATCH_4 => {
                self.list.patches.push(RomPatch {
                    address: BASE_CART0 | (op2 & 0x00FF_FFFF) << 1,
                    width: 2,
                    value: 0,
                    check: None,
                });
                self.incomplete_patch = Some(self.list.patches.len() - 1);
                true
            }
            OTHER_ENDIF => {
                let open = self.current_block.is_some();
                if open {
                    self.end_block();
                }
                open
            }
            OTHER_ELSE => match self.current_block {
                Some(block) => {
                    let size = self.block_size(block);
                    if let Some(op) = self.list.ops.get_mut(block) {
                        op.repeat = size;
                    }
                    true
                }
                None => false,
            },
            OTHER_FILL_1 | OTHER_FILL_2 | OTHER_FILL_4 => {
                let width = 1 << ((op2 & WIDTH) >> WIDTH_BASE);
                let index = self.push(Op::new(Kind::Assign, width, address, 0));
                self.incomplete_cheat = Some(index);
                true
            }
            _ => true,
        }
    }

    fn block_size(&self, block: usize) -> u32 {
        self.list.ops.len().wrapping_sub(block).wrapping_sub(1) as u32
    }

    fn end_block(&mut self) {
        if let Some(block) = self.current_block.take() {
            let size = self.block_size(block);
            if let Some(op) = self.list.ops.get_mut(block) {
                if op.repeat != 0 {
                    op.negative_repeat = size.wrapping_sub(op.repeat);
                } else {
                    op.repeat = size;
                }
            }
        }
    }
}

pub(super) fn probability((op1, op2): (u32, u32)) -> i32 {
    if op2 == TAG_CODE {
        return 0x100;
    }
    if op1 == DEADFACE && op2 & 0xFFFF_0000 == 0 {
        return 0x100;
    }
    if op1 == 0 {
        let mut probability = 0x40;
        match op2 & 0xFE00_0000 {
            OTHER_FILL_1 | OTHER_FILL_2 | OTHER_FILL_4 => {
                probability += address_is_real(par_address(op2));
            }
            OTHER_PATCH_1 | OTHER_PATCH_2 | OTHER_PATCH_3 | OTHER_PATCH_4 => {}
            OTHER_END | OTHER_SLOWDOWN | OTHER_BUTTON_1 | OTHER_BUTTON_2 | OTHER_BUTTON_4
            | OTHER_ENDIF | OTHER_ELSE => {
                if op2 & 0x0100_0000 != 0 {
                    probability -= 0x40;
                }
            }
            _ => probability -= 0x40,
        }
        return probability;
    }
    let width = (op1 & WIDTH) >> (WIDTH_BASE - 3);
    let mut probability = 0x20;
    if op1 & COND != 0 {
        if width >= 24 {
            return 0;
        }
        if op2 & !((1 << width) - 1) != 0 {
            probability -= 0x10;
        }
        return probability;
    }
    match op1 & BASE {
        BASE_ASSIGN | BASE_INDIRECT | BASE_ADD => {
            if op1 & BASE == BASE_ADD && op2 & !((1u32 << width) - 1) != 0 {
                probability -= 0x10;
            }
            probability += address_is_real(par_address(op1));
            if op1 & 0x0100_0000 != 0 {
                return 0;
            }
        }
        _ => {}
    }
    probability
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Memory, split};
    use super::*;
    use crate::cheats::CheatTarget;
    use crate::cheats::{Cheat, Format, Program};
    use crate::console::ConsoleKind;

    fn raw(lines: &[&str]) -> Set {
        let mut set = Set::new();
        set.set_version(Version::ParV3Raw);
        for line in lines {
            let (op1, op2) = split(line);
            assert!(set.add_par(op1, op2), "{line}");
        }
        set
    }

    fn refused(lines: &[&str], last: &str) -> bool {
        let mut set = raw(lines);
        let (op1, op2) = split(last);
        !set.add_par(op1, op2)
    }

    fn bytes(set: &Set, ones: &[u32], count: u32) -> Vec<u8> {
        let preset: Vec<(u32, u8)> = ones.iter().map(|&at| (0x0300_0000 + at, 1)).collect();
        let mut memory = Memory::with(&preset);
        memory.run(set);
        (0..count).map(|at| memory.byte(0x0300_0000 + at)).collect()
    }

    fn halves(set: &Set, step: u32) -> Vec<u32> {
        let mut memory = Memory::default();
        memory.run(set);
        (0..6)
            .map(|index| memory.cheat_read(0x0300_0000 + index * step, 2))
            .collect()
    }

    fn list(code: &str) -> (Format, crate::cheats::ops::OpList) {
        let cheat = Cheat::parse(ConsoleKind::Gba, code).unwrap();
        match cheat.program {
            Program::Ops(list) => (cheat.format, list),
            #[cfg(feature = "nds")]
            Program::Ds(_) => unreachable!(),
        }
    }

    #[test]
    fn an_add_line_is_accepted_and_a_stray_bit_24_is_refused() {
        assert!(!refused(&[], "80000000 00000000"));
        assert!(refused(&[], "43000000 00000000"));
    }

    #[test]
    fn assign_lines_write_one_two_and_four_bytes() {
        let set = raw(&[
            "00300000 00000078",
            "02300002 00005678",
            "04300004 12345678",
        ]);
        let mut memory = Memory::default();
        memory.run(&set);
        assert_eq!(memory.cheat_read(0x0300_0000, 1), 0x78);
        assert_eq!(memory.cheat_read(0x0300_0002, 2), 0x5678);
        assert_eq!(memory.cheat_read(0x0300_0004, 4), 0x1234_5678);
    }

    #[test]
    fn a_byte_fill_slides_address_and_value() {
        let set = raw(&["00000000 80300000", "00000001 01020002"]);
        assert_eq!(bytes(&set, &[], 6), [1, 0, 2, 0, 0, 0]);
    }

    #[test]
    fn a_half_word_fill_slides_address_and_value() {
        let set = raw(&["00000000 82300000", "00000001 01020002"]);
        assert_eq!(halves(&set, 2), [1, 0, 2, 0, 0, 0]);
    }

    #[test]
    fn a_word_fill_slides_address_and_value() {
        let set = raw(&["00000000 84300000", "00000001 01020002"]);
        assert_eq!(halves(&set, 4), [1, 0, 2, 0, 0, 0]);
    }

    #[test]
    fn an_if_guards_the_next_line() {
        let set = raw(&[
            "00300001 00000011",
            "08300000 00000000",
            "00300001 00000012",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 2), [1, 0x12]);
        assert_eq!(bytes(&set, &[0], 2), [1, 0x11]);
    }

    #[test]
    fn two_single_line_ifs_guard_their_own_lines() {
        let set = raw(&[
            "00300002 00000021",
            "00300003 00000031",
            "08300000 00000000",
            "00300002 00000022",
            "08300001 00000000",
            "00300003 00000032",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 4), [1, 0, 0x22, 0x32]);
        assert_eq!(bytes(&set, &[0], 4), [1, 0, 0x21, 0x32]);
        assert_eq!(bytes(&set, &[1], 4), [1, 1, 0x22, 0x31]);
        assert_eq!(bytes(&set, &[0, 1], 4), [1, 1, 0x21, 0x31]);
    }

    #[test]
    fn a_two_line_if_guards_the_next_two_lines() {
        let set = raw(&[
            "00300001 00000011",
            "00300002 00000021",
            "48300000 00000000",
            "00300001 00000012",
            "00300002 00000022",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 3), [1, 0x12, 0x22]);
        assert_eq!(bytes(&set, &[0], 3), [1, 0x11, 0x21]);
    }

    #[test]
    fn two_two_line_ifs_guard_their_own_pairs() {
        let set = raw(&[
            "00300002 00000021",
            "00300003 00000031",
            "00300004 00000041",
            "00300005 00000051",
            "48300000 00000000",
            "00300002 00000022",
            "00300003 00000032",
            "48300001 00000000",
            "00300004 00000042",
            "00300005 00000052",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 6), [1, 0, 0x22, 0x32, 0x42, 0x52]);
        assert_eq!(bytes(&set, &[0], 6), [1, 0, 0x21, 0x31, 0x42, 0x52]);
        assert_eq!(bytes(&set, &[1], 6), [1, 1, 0x22, 0x32, 0x41, 0x51]);
        assert_eq!(bytes(&set, &[0, 1], 6), [1, 1, 0x21, 0x31, 0x41, 0x51]);
    }

    #[test]
    fn a_two_line_if_can_hold_a_one_line_if() {
        let set = raw(&[
            "00300002 00000021",
            "48300000 00000000",
            "08300001 00000000",
            "00300002 00000022",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 3), [1, 0, 0x22]);
        assert_eq!(bytes(&set, &[0], 3), [1, 0, 0x21]);
        assert_eq!(bytes(&set, &[1], 3), [1, 1, 0x21]);
        assert_eq!(bytes(&set, &[0, 1], 3), [1, 1, 0x21]);
    }

    #[test]
    fn a_block_if_runs_until_its_endif() {
        let set = raw(&[
            "00300001 00000011",
            "88300000 00000000",
            "00300001 00000012",
            "00000000 40000000",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 2), [1, 0x12]);
        assert_eq!(bytes(&set, &[0], 2), [1, 0x11]);
    }

    #[test]
    fn two_block_ifs_in_a_row_each_end_at_their_endif() {
        let set = raw(&[
            "00300002 00000021",
            "00300003 00000031",
            "00300004 00000041",
            "88300000 00000000",
            "00300002 00000022",
            "00000000 40000000",
            "00300003 00000032",
            "88300001 00000000",
            "00300004 00000042",
            "00000000 40000000",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 5), [1, 0, 0x22, 0x32, 0x42]);
        assert_eq!(bytes(&set, &[0], 5), [1, 0, 0x21, 0x32, 0x42]);
        assert_eq!(bytes(&set, &[1], 5), [1, 1, 0x22, 0x32, 0x41]);
        assert_eq!(bytes(&set, &[0, 1], 5), [1, 1, 0x21, 0x32, 0x41]);
    }

    #[test]
    fn a_block_if_with_an_else_runs_one_side() {
        let set = raw(&[
            "00300001 00000011",
            "00300002 00000021",
            "88300000 00000000",
            "00300001 00000012",
            "00000000 60000000",
            "00300002 00000022",
            "00000000 40000000",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 3), [1, 0x12, 0x21]);
        assert_eq!(bytes(&set, &[0], 3), [1, 0x11, 0x22]);
    }

    #[test]
    fn an_if_else_block_then_a_plain_block() {
        let set = raw(&[
            "00300002 00000021",
            "00300003 00000031",
            "00300004 00000041",
            "00300005 00000051",
            "88300000 00000000",
            "00300002 00000022",
            "00000000 60000000",
            "00300003 00000032",
            "00000000 40000000",
            "00300004 00000042",
            "88300001 00000000",
            "00300005 00000052",
            "00000000 40000000",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 6), [1, 0, 0x22, 0x31, 0x42, 0x52]);
        assert_eq!(bytes(&set, &[0], 6), [1, 0, 0x21, 0x32, 0x42, 0x52]);
        assert_eq!(bytes(&set, &[1], 6), [1, 1, 0x22, 0x31, 0x42, 0x51]);
        assert_eq!(bytes(&set, &[0, 1], 6), [1, 1, 0x21, 0x32, 0x42, 0x51]);
    }

    #[test]
    fn two_if_else_blocks_in_a_row() {
        let set = raw(&[
            "00300002 00000021",
            "00300003 00000031",
            "00300004 00000041",
            "00300005 00000051",
            "00300006 00000061",
            "88300000 00000000",
            "00300002 00000022",
            "00000000 60000000",
            "00300003 00000032",
            "00000000 40000000",
            "00300004 00000042",
            "88300001 00000000",
            "00300005 00000052",
            "00000000 60000000",
            "00300006 00000062",
            "00000000 40000000",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 6), [1, 0, 0x22, 0x31, 0x42, 0x52]);
        assert_eq!(bytes(&set, &[0], 7), [1, 0, 0x21, 0x32, 0x42, 0x52, 0x61]);
        assert_eq!(bytes(&set, &[1], 7), [1, 1, 0x22, 0x31, 0x42, 0x51, 0x62]);
        assert_eq!(
            bytes(&set, &[0, 1], 7),
            [1, 1, 0x21, 0x32, 0x42, 0x51, 0x62]
        );
    }

    #[test]
    fn a_block_if_can_hold_a_one_line_if() {
        let set = raw(&[
            "00300002 00000021",
            "00300003 00000031",
            "00300004 00000041",
            "88300000 00000000",
            "00300002 00000022",
            "08300001 00000000",
            "00300003 00000032",
            "00300004 00000042",
            "00000000 40000000",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 5), [1, 0, 0x22, 0x32, 0x42]);
        assert_eq!(bytes(&set, &[0], 5), [1, 0, 0x21, 0x31, 0x41]);
        assert_eq!(bytes(&set, &[1], 5), [1, 1, 0x22, 0x31, 0x42]);
        assert_eq!(bytes(&set, &[0, 1], 5), [1, 1, 0x21, 0x31, 0x41]);
    }

    #[test]
    fn a_one_line_if_inside_the_then_side_of_an_if_else_block() {
        let set = raw(&[
            "00300002 00000021",
            "00300003 00000031",
            "00300004 00000041",
            "00300005 00000051",
            "88300000 00000000",
            "00300002 00000022",
            "08300001 00000000",
            "00300003 00000032",
            "00300004 00000042",
            "00000000 60000000",
            "00300005 00000052",
            "00000000 40000000",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 6), [1, 0, 0x22, 0x32, 0x42, 0x51]);
        assert_eq!(bytes(&set, &[0], 6), [1, 0, 0x21, 0x31, 0x41, 0x52]);
        assert_eq!(bytes(&set, &[1], 6), [1, 1, 0x22, 0x31, 0x42, 0x51]);
        assert_eq!(bytes(&set, &[0, 1], 6), [1, 1, 0x21, 0x31, 0x41, 0x52]);
    }

    #[test]
    fn a_one_line_if_inside_the_else_side_of_an_if_else_block() {
        let set = raw(&[
            "00300002 00000021",
            "00300003 00000031",
            "00300004 00000041",
            "00300005 00000051",
            "88300000 00000000",
            "00300002 00000022",
            "00000000 60000000",
            "00300003 00000032",
            "08300001 00000000",
            "00300004 00000042",
            "00300005 00000052",
            "00000000 40000000",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 6), [1, 0, 0x22, 0x31, 0x41, 0x51]);
        assert_eq!(bytes(&set, &[0], 6), [1, 0, 0x21, 0x32, 0x42, 0x52]);
        assert_eq!(bytes(&set, &[1], 6), [1, 1, 0x22, 0x31, 0x41, 0x51]);
        assert_eq!(bytes(&set, &[0, 1], 6), [1, 1, 0x21, 0x32, 0x41, 0x52]);
    }

    #[test]
    fn one_line_ifs_inside_both_sides_of_an_if_else_block() {
        let set = raw(&[
            "00300003 00000031",
            "00300004 00000041",
            "00300005 00000051",
            "00300006 00000061",
            "00300007 00000071",
            "00300008 00000081",
            "88300000 00000000",
            "00300003 00000032",
            "08300001 00000000",
            "00300004 00000042",
            "00300005 00000052",
            "00000000 60000000",
            "00300006 00000062",
            "08300002 00000000",
            "00300007 00000072",
            "00300008 00000082",
            "00000000 40000000",
            "00300000 00000001",
        ]);
        let cases: [(&[u32], [u8; 9]); 8] = [
            (&[], [1, 0, 0, 0x32, 0x42, 0x52, 0x61, 0x71, 0x81]),
            (&[0], [1, 0, 0, 0x31, 0x41, 0x51, 0x62, 0x72, 0x82]),
            (&[1], [1, 1, 0, 0x32, 0x41, 0x52, 0x61, 0x71, 0x81]),
            (&[2], [1, 0, 1, 0x32, 0x42, 0x52, 0x61, 0x71, 0x81]),
            (&[0, 1], [1, 1, 0, 0x31, 0x41, 0x51, 0x62, 0x72, 0x82]),
            (&[0, 2], [1, 0, 1, 0x31, 0x41, 0x51, 0x62, 0x71, 0x82]),
            (&[1, 2], [1, 1, 1, 0x32, 0x41, 0x52, 0x61, 0x71, 0x81]),
            (&[0, 1, 2], [1, 1, 1, 0x31, 0x41, 0x51, 0x62, 0x71, 0x82]),
        ];
        for (ones, expected) in cases {
            assert_eq!(bytes(&set, ones, 9), expected, "{ones:?}");
        }
    }

    #[test]
    fn a_button_code_never_fires_without_a_cheat_button() {
        let set = raw(&["00000000 10300000", "00000001 00000000"]);
        let assign = Op::new(Kind::Assign, 1, 0x0300_0000, 1);
        assert_eq!(set.list.ops, [if_button(), assign]);
        assert_eq!(bytes(&set, &[], 1), [0]);
    }

    #[test]
    fn a_one_line_if_wider_than_a_word_never_holds() {
        let set = raw(&[
            "00300001 00000011",
            "0E300000 00000000",
            "00300001 00000012",
            "00300000 00000001",
        ]);
        assert_eq!(set.list.ops[1].kind, Kind::Never);
        assert_eq!(set.list.ops[1].width, 0);
        assert_eq!(bytes(&set, &[], 2), [1, 0x11]);
        assert_eq!(bytes(&set, &[0], 2), [1, 0x11]);
    }

    #[test]
    fn a_two_line_if_wider_than_a_word_never_holds() {
        let set = raw(&[
            "00300001 00000011",
            "00300002 00000021",
            "4E300000 00000000",
            "00300001 00000012",
            "00300002 00000022",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 3), [1, 0x11, 0x21]);
        assert_eq!(bytes(&set, &[0], 3), [1, 0x11, 0x21]);
    }

    #[test]
    fn a_block_if_wider_than_a_word_never_holds() {
        let set = raw(&[
            "00300001 00000011",
            "8E300000 00000000",
            "00300001 00000012",
            "00000000 40000000",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 2), [1, 0x11]);
        assert_eq!(bytes(&set, &[0], 2), [1, 0x11]);
    }

    #[test]
    fn a_never_block_runs_its_else_side() {
        let set = raw(&[
            "00300001 00000011",
            "00300002 00000021",
            "8E300000 00000000",
            "00300001 00000012",
            "00000000 60000000",
            "00300002 00000022",
            "00000000 40000000",
            "00300000 00000001",
        ]);
        assert_eq!(bytes(&set, &[], 3), [1, 0x11, 0x22]);
        assert_eq!(bytes(&set, &[0], 3), [1, 0x11, 0x22]);
    }

    #[test]
    fn a_rom_patch_takes_its_value_from_the_next_line() {
        let set = raw(&["00000000 18000100", "00004770 00000000"]);
        assert_eq!(
            set.list.patches,
            [RomPatch {
                address: 0x0800_0200,
                width: 2,
                value: 0x4770,
                check: None,
            }]
        );
        assert_eq!(set.incomplete_patch, None);
    }

    #[test]
    fn an_io_line_writes_the_io_register_with_its_width() {
        let set = raw(&["C7000130 000003FF", "C6000208 00000101"]);
        assert_eq!(
            set.list.ops,
            [
                Op::new(Kind::Assign, 2, 0x0400_0130, 0x03FF),
                Op::new(Kind::Assign, 1, 0x0400_0208, 0x01),
            ]
        );
        assert!(refused(&[], "C5000130 000003FF"));
    }

    #[test]
    fn an_indirect_line_writes_through_a_pointer() {
        let set = raw(&["40300000 00000255"]);
        let mut memory = Memory::with(&[(0x0300_0000, 0x10), (0x0300_0003, 0x02)]);
        memory.run(&set);
        assert_eq!(memory.byte(0x0200_0012), 0x55);
    }

    #[test]
    fn hooks_tags_and_master_lines_add_no_op_and_a_second_hook_is_refused() {
        let set = raw(&["C4000430 00008401", "45565841 001DC0DE"]);
        assert!(set.list.ops.is_empty());
        assert!(refused(&["C4000430 00008401"], "C4000500 00008401"));
    }

    #[test]
    fn disable_slowdown_and_stray_endif_or_else_lines_are_refused() {
        assert!(refused(&[], "C8300000 00000000"));
        assert!(refused(&[], "00000000 08000000"));
        assert!(refused(&[], "00000000 40000000"));
        assert!(refused(&[], "00000000 60000000"));
    }

    #[test]
    fn a_deadface_line_reseeds_like_mgba() {
        let set = raw(&["DEADFACE 0000BEEF"]);
        assert_eq!(
            set.gsa_seeds,
            [0xF851_88D4, 0xD831_68B4, 0x0D66_9DE9, 0xE53E_75C1]
        );
    }

    #[test]
    fn the_real_pokemon_ruby_master_code_decrypts_to_a_hook_and_the_game_id() {
        let (format, list) = list("DE00AAFD 2EBD05D0\n530823D9 16558191");
        assert_eq!(format, Format::ActionReplay);
        assert!(list.ops.is_empty() && list.patches.is_empty());
        assert_eq!(
            decrypt(0xDE00_AAFD, 0x2EBD_05D0, &SEEDS),
            (0xC400_0430, 0x0000_8401)
        );
        assert_eq!(
            decrypt(0x5308_23D9, 0x1655_8191, &SEEDS),
            (0x4556_5841, TAG_CODE)
        );
    }

    #[test]
    fn the_real_pokemon_ruby_pokerus_code_patches_the_rom() {
        let (_, list) =
            list("DE00AAFD 2EBD05D0\n530823D9 16558191\nC784FC61 63EBC3B6\n117279A9 85478753");
        assert_eq!(
            list.patches,
            [RomPatch {
                address: 0x0804_005A,
                width: 2,
                value: 0x4280,
                check: None,
            }]
        );
    }

    #[test]
    fn the_real_pokemon_fire_red_master_and_dma_fix_decode() {
        let (format, list) =
            list("72BC6DFB E9CA5465\nA47FB2DC 1AF3CA86\n8D671FD9 6F6BEFF2\n78DA95DF 44018CB4");
        assert_eq!(format, Format::ActionReplay);
        assert!(list.ops.is_empty());
        assert_eq!(
            list.patches,
            [RomPatch {
                address: 0x0804_C064,
                width: 2,
                value: 0x2100,
                check: None,
            }]
        );
    }

    #[test]
    fn a_raw_par_master_code_is_detected_as_action_replay() {
        let (format, list) = list("C47EF48C 00008411");
        assert_eq!(format, Format::ActionReplay);
        assert!(list.ops.is_empty());
    }

    #[test]
    fn probabilities_follow_mgba() {
        assert_eq!(probability((0x1234_5678, TAG_CODE)), 0x100);
        assert_eq!(probability((DEADFACE, 0x1234)), 0x100);
        assert_eq!(probability((0, 0x8030_0000)), 0x60);
        assert_eq!(probability((0, 0x1800_0000)), 0x40);
        assert_eq!(probability((0, 0x2000_0000)), 0);
        assert_eq!(probability((0x0030_0000, 0x78)), 0x40);
        assert_eq!(probability((0x0E30_0000, 0)), 0);
        assert_eq!(probability((0x8030_0000, 0x1FF)), 0x30);
    }
}
