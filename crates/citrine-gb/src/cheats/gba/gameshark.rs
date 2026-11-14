use super::{BASE_CART0, DEADFACE, Set, TAG_CODE, Version, address_is_real, if_button};
use crate::cheats::ops::{Kind, Op, RomPatch};

pub(super) const SEEDS: [u32; 4] = [0x09F4_FBBD, 0x9681_884A, 0x3520_27E9, 0xF3DE_E5A7];

const ASSIGN_1: u32 = 0x0;
const ASSIGN_2: u32 = 0x1;
const ASSIGN_4: u32 = 0x2;
const ASSIGN_LIST: u32 = 0x3;
const PATCH: u32 = 0x6;
const BUTTON: u32 = 0x8;
const IF_EQ: u32 = 0xD;
const IF_EQ_RANGE: u32 = 0xE;
const HOOK: u32 = 0xF;

const T1: [u8; 256] = [
    0x31, 0x1C, 0x23, 0xE5, 0x89, 0x8E, 0xA1, 0x37, 0x74, 0x6D, 0x67, 0xFC, 0x1F, 0xC0, 0xB1, 0x94,
    0x3B, 0x05, 0x56, 0x86, 0x00, 0x24, 0xF0, 0x17, 0x72, 0xA2, 0x3D, 0x1B, 0xE3, 0x17, 0xC5, 0x0B,
    0xB9, 0xE2, 0xBD, 0x58, 0x71, 0x1B, 0x2C, 0xFF, 0xE4, 0xC9, 0x4C, 0x5E, 0xC9, 0x55, 0x33, 0x45,
    0x7C, 0x3F, 0xB2, 0x51, 0xFE, 0x10, 0x7E, 0x75, 0x3C, 0x90, 0x8D, 0xDA, 0x94, 0x38, 0xC3, 0xE9,
    0x95, 0xEA, 0xCE, 0xA6, 0x06, 0xE0, 0x4F, 0x3F, 0x2A, 0xE3, 0x3A, 0xE4, 0x43, 0xBD, 0x7F, 0xDA,
    0x55, 0xF0, 0xEA, 0xCB, 0x2C, 0xA8, 0x47, 0x61, 0xA0, 0xEF, 0xCB, 0x13, 0x18, 0x20, 0xAF, 0x3E,
    0x4D, 0x9E, 0x1E, 0x77, 0x51, 0xC5, 0x51, 0x20, 0xCF, 0x21, 0xF9, 0x39, 0x94, 0xDE, 0xDD, 0x79,
    0x4E, 0x80, 0xC4, 0x9D, 0x94, 0xD5, 0x95, 0x01, 0x27, 0x27, 0xBD, 0x6D, 0x78, 0xB5, 0xD1, 0x31,
    0x6A, 0x65, 0x74, 0x74, 0x58, 0xB3, 0x7C, 0xC9, 0x5A, 0xED, 0x50, 0x03, 0xC4, 0xA2, 0x94, 0x4B,
    0xF0, 0x58, 0x09, 0x6F, 0x3E, 0x7D, 0xAE, 0x7D, 0x58, 0xA0, 0x2C, 0x91, 0xBB, 0xE1, 0x70, 0xEB,
    0x73, 0xA6, 0x9A, 0x44, 0x25, 0x90, 0x16, 0x62, 0x53, 0xAE, 0x08, 0xEB, 0xDC, 0xF0, 0xEE, 0x77,
    0xC2, 0xDE, 0x81, 0xE8, 0x30, 0x89, 0xDB, 0xFE, 0xBC, 0xC2, 0xDF, 0x26, 0xE9, 0x8B, 0xD6, 0x93,
    0xF0, 0xCB, 0x56, 0x90, 0xC0, 0x46, 0x68, 0x15, 0x43, 0xCB, 0xE9, 0x98, 0xE3, 0xAF, 0x31, 0x25,
    0x4D, 0x7B, 0xF3, 0xB1, 0x74, 0xE2, 0x64, 0xAC, 0xD9, 0xF6, 0xA0, 0xD5, 0x0B, 0x9B, 0x49, 0x52,
    0x69, 0x3B, 0x71, 0x00, 0x2F, 0xBB, 0xBA, 0x08, 0xB1, 0xAE, 0xBB, 0xB3, 0xE1, 0xC9, 0xA6, 0x7F,
    0x17, 0x97, 0x28, 0x72, 0x12, 0x6E, 0x91, 0xAE, 0x3A, 0xA2, 0x35, 0x46, 0x27, 0xF8, 0x12, 0x50,
];

const T2: [u8; 256] = [
    0xD8, 0x65, 0x04, 0xC2, 0x65, 0xD5, 0xB0, 0x0C, 0xDF, 0x9D, 0xF0, 0xC3, 0x9A, 0x17, 0xC9, 0xA6,
    0xE1, 0xAC, 0x0D, 0x14, 0x2F, 0x3C, 0x2C, 0x87, 0xA2, 0xBF, 0x4D, 0x5F, 0xAC, 0x2D, 0x9D, 0xE1,
    0x0C, 0x9C, 0xE7, 0x7F, 0xFC, 0xA8, 0x66, 0x59, 0xAC, 0x18, 0xD7, 0x05, 0xF0, 0xBF, 0xD1, 0x8B,
    0x35, 0x9F, 0x59, 0xB4, 0xBA, 0x55, 0xB2, 0x85, 0xFD, 0xB1, 0x72, 0x06, 0x73, 0xA4, 0xDB, 0x48,
    0x7B, 0x5F, 0x67, 0xA5, 0x95, 0xB9, 0xA5, 0x4A, 0xCF, 0xD1, 0x44, 0xF3, 0x81, 0xF5, 0x6D, 0xF6,
    0x3A, 0xC3, 0x57, 0x83, 0xFA, 0x8E, 0x15, 0x2A, 0xA2, 0x04, 0xB2, 0x9D, 0xA8, 0x0D, 0x7F, 0xB8,
    0x0F, 0xF6, 0xAC, 0xBE, 0x97, 0xCE, 0x16, 0xE6, 0x31, 0x10, 0x60, 0x16, 0xB5, 0x83, 0x45, 0xEE,
    0xD7, 0x5F, 0x2C, 0x08, 0x58, 0xB1, 0xFD, 0x7E, 0x79, 0x00, 0x34, 0xAD, 0xB5, 0x31, 0x34, 0x39,
    0xAF, 0xA8, 0xDD, 0x52, 0x6A, 0xB0, 0x60, 0x35, 0xB8, 0x1D, 0x52, 0xF5, 0xF5, 0x30, 0x00, 0x7B,
    0xF4, 0xBA, 0x03, 0xCB, 0x3A, 0x84, 0x14, 0x8A, 0x6A, 0xEF, 0x21, 0xBD, 0x01, 0xD8, 0xA0, 0xD4,
    0x43, 0xBE, 0x23, 0xE7, 0x76, 0x27, 0x2C, 0x3F, 0x4D, 0x3F, 0x43, 0x18, 0xA7, 0xC3, 0x47, 0xA5,
    0x7A, 0x1D, 0x02, 0x55, 0x09, 0xD1, 0xFF, 0x55, 0x5E, 0x17, 0xA0, 0x56, 0xF4, 0xC9, 0x6B, 0x90,
    0xB4, 0x80, 0xA5, 0x07, 0x22, 0xFB, 0x22, 0x0D, 0xD9, 0xC0, 0x5B, 0x08, 0x35, 0x05, 0xC1, 0x75,
    0x4F, 0xD0, 0x51, 0x2D, 0x2E, 0x5E, 0x69, 0xE7, 0x3B, 0xC2, 0xDA, 0xFF, 0xF6, 0xCE, 0x3E, 0x76,
    0xE8, 0x36, 0x8C, 0x39, 0xD8, 0xF3, 0xE9, 0xA6, 0x42, 0xE6, 0xC1, 0x4C, 0x05, 0xBE, 0x17, 0xF2,
    0x5C, 0x1B, 0x19, 0xDB, 0x0F, 0xF3, 0xF8, 0x49, 0xEB, 0x36, 0xF6, 0x40, 0x6F, 0xAD, 0xC1, 0x8C,
];

pub(super) fn decrypt(mut op1: u32, mut op2: u32, seeds: &[u32; 4]) -> (u32, u32) {
    let mut sum: u32 = 0xC6EF_3720;
    for _ in 0..32 {
        op2 = op2.wrapping_sub(
            (op1 << 4).wrapping_add(seeds[2])
                ^ op1.wrapping_add(sum)
                ^ (op1 >> 5).wrapping_add(seeds[3]),
        );
        op1 = op1.wrapping_sub(
            (op2 << 4).wrapping_add(seeds[0])
                ^ op2.wrapping_add(sum)
                ^ (op2 >> 5).wrapping_add(seeds[1]),
        );
        sum = sum.wrapping_sub(0x9E37_79B9);
    }
    (op1, op2)
}

pub(super) fn reseed(seeds: &mut [u32; 4], params: u16, t1: &[u8; 256], t2: &[u8; 256]) {
    let s0 = usize::from(params >> 8);
    let s1 = usize::from(params & 0xFF);
    for (y, seed) in seeds.iter_mut().enumerate() {
        for x in 0..4 {
            let z = t1[(s0 + x) & 0xFF].wrapping_add(t2[(s1 + y) & 0xFF]);
            *seed = *seed << 8 | u32::from(z);
        }
    }
}

impl Set {
    pub(super) fn add_gameshark(&mut self, op1: u32, op2: u32) -> bool {
        match self.gsa_version {
            Version::GsaV1Raw => self.add_gameshark_raw(op1, op2),
            Version::GsaV1 => {
                let (op1, op2) = decrypt(op1, op2, &self.gsa_seeds);
                self.add_gameshark_raw(op1, op2)
            }
            _ => {
                self.set_version(Version::GsaV1);
                let (op1, op2) = decrypt(op1, op2, &self.gsa_seeds);
                self.add_gameshark_raw(op1, op2)
            }
        }
    }

    pub(super) fn add_gameshark_raw(&mut self, op1: u32, op2: u32) -> bool {
        if let Some(index) = self.incomplete_cheat {
            let operand = self.list.ops.get(index).map_or(0, |op| op.operand);
            for address in [op1, op2] {
                if self.remaining_addresses > 0 {
                    self.push(Op::new(Kind::Assign, 4, address, operand));
                    self.remaining_addresses -= 1;
                }
            }
            if self.remaining_addresses == 0 {
                self.incomplete_cheat = None;
            }
            return true;
        }

        let address = op1 & 0x0FFF_FFFF;
        let op = match op1 >> 28 {
            ASSIGN_1 => Op::new(Kind::Assign, 1, address, op2),
            ASSIGN_2 => Op::new(Kind::Assign, 2, address, op2),
            ASSIGN_4 => Op::new(Kind::Assign, 4, address, op2),
            ASSIGN_LIST => {
                self.remaining_addresses = (op1 & 0xFFFF) as i32 - 1;
                let index = self.push(Op::new(Kind::Assign, 4, op2, op2));
                self.incomplete_cheat = Some(index);
                return true;
            }
            PATCH => {
                self.list.patches.push(RomPatch {
                    address: BASE_CART0 | (op1 & 0x00FF_FFFF) << 1,
                    width: 2,
                    value: op2,
                    check: None,
                });
                return true;
            }
            BUTTON => {
                let width = match op1 & 0x00F0_0000 {
                    0x0010_0000 => 1,
                    0x0020_0000 => 2,
                    _ => return false,
                };
                self.push(if_button());
                Op::new(Kind::Assign, width, op1 & 0x0F0F_FFFF, op2)
            }
            IF_EQ => {
                if op1 == DEADFACE {
                    reseed(&mut self.gsa_seeds, op2 as u16, &T1, &T2);
                    return true;
                }
                Op::new(Kind::IfEq, 2, address, op2)
            }
            IF_EQ_RANGE => {
                let mut op = Op::new(Kind::IfEq, 2, op2 & 0x0FFF_FFFF, op1 & 0xFFFF);
                op.repeat = (op1 >> 16) & 0xFF;
                op
            }
            HOOK => return self.take_hook(),
            _ => return false,
        };
        self.push(op);
        true
    }
}

pub(super) fn probability((op1, op2): (u32, u32)) -> i32 {
    if op2 == TAG_CODE {
        return 0x100;
    }
    let address = op1 & 0x0FFF_FFFF;
    let mut probability = 0;
    match op1 >> 28 {
        ASSIGN_1 => {
            probability += 0x20;
            if op2 & 0xFFFF_FF00 != 0 {
                probability -= 0x10;
            }
            probability += address_is_real(address);
        }
        ASSIGN_2 | IF_EQ => {
            probability += 0x20;
            if op2 & 0xFFFF_0000 != 0 {
                probability -= 0x10;
            }
            probability += address_is_real(address);
        }
        ASSIGN_4 => {
            probability += 0x20;
            probability += address_is_real(address);
        }
        PATCH => {
            probability += 0x20;
            if op2 & 0xCFFF_0000 != 0 {
                probability -= 0x10;
            }
        }
        BUTTON => probability += 0x10,
        IF_EQ_RANGE => {
            probability += 0x20;
            probability += address_is_real(op2);
            if op1 & 0x0F00_0000 != 0 {
                probability -= 0x10;
            }
        }
        HOOK => {
            probability += 0x20;
            if op2 & 0xFFFF_0000 != 0 {
                probability -= 0x10;
            }
        }
        _ => probability -= 0x40,
    }
    probability
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::tests::{Memory, split};
    use super::*;
    use crate::cheats::CheatTarget;

    pub(in crate::cheats::gba) fn encrypt(
        mut op1: u32,
        mut op2: u32,
        seeds: &[u32; 4],
    ) -> (u32, u32) {
        let mut sum: u32 = 0;
        for _ in 0..32 {
            sum = sum.wrapping_add(0x9E37_79B9);
            op1 = op1.wrapping_add(
                (op2 << 4).wrapping_add(seeds[0])
                    ^ op2.wrapping_add(sum)
                    ^ (op2 >> 5).wrapping_add(seeds[1]),
            );
            op2 = op2.wrapping_add(
                (op1 << 4).wrapping_add(seeds[2])
                    ^ op1.wrapping_add(sum)
                    ^ (op1 >> 5).wrapping_add(seeds[3]),
            );
        }
        (op1, op2)
    }

    fn raw(lines: &[&str]) -> Set {
        let mut set = Set::new();
        set.set_version(Version::GsaV1Raw);
        for line in lines {
            let (op1, op2) = split(line);
            assert!(set.add_gameshark(op1, op2), "{line}");
        }
        set
    }

    fn refused(lines: &[&str], last: &str) -> bool {
        let mut set = raw(lines);
        let (op1, op2) = split(last);
        !set.add_gameshark(op1, op2)
    }

    #[test]
    fn decryption_matches_mgba_for_both_seed_sets() {
        assert_eq!(
            decrypt(0x1234_5678, 0x9ABC_DEF0, &SEEDS),
            (0x1209_2EBB, 0x334B_CAF7)
        );
        assert_eq!(
            decrypt(0x1234_5678, 0x9ABC_DEF0, &super::super::parv3::SEEDS),
            (0xFAA0_51AB, 0x9468_8C5F)
        );
    }

    #[test]
    fn encrypting_then_decrypting_gives_the_code_back() {
        let (op1, op2) = encrypt(0x0200_0100, 0x0000_00FF, &SEEDS);
        assert_eq!(decrypt(op1, op2, &SEEDS), (0x0200_0100, 0x0000_00FF));
    }

    #[test]
    fn a_deadface_line_reseeds_like_mgba() {
        let set = raw(&["DEADFACE 00001234"]);
        assert_eq!(
            set.gsa_seeds,
            [0x1040_BADE, 0xABDB_5579, 0x0838_B2D6, 0xDB0B_85A9]
        );
        assert!(set.list.ops.is_empty());
    }

    #[test]
    fn an_encrypted_line_after_deadface_uses_the_new_seeds() {
        let mut set = Set::new();
        set.set_version(Version::GsaV1);
        let (op1, op2) = encrypt(0xDEAD_FACE, 0x0000_1234, &SEEDS);
        assert!(set.add_gameshark(op1, op2));
        let seeds = set.gsa_seeds;
        let (op1, op2) = encrypt(0x1200_0010, 0x0000_BEEF, &seeds);
        assert!(set.add_gameshark(op1, op2));
        assert_eq!(
            set.list.ops,
            [Op::new(Kind::Assign, 2, 0x0200_0010, 0xBEEF)]
        );
    }

    #[test]
    fn assign_codes_write_one_two_or_four_bytes() {
        let set = raw(&[
            "02000000 00000012",
            "12000002 00003456",
            "22000004 789ABCDE",
        ]);
        let mut memory = Memory::default();
        memory.run(&set);
        assert_eq!(memory.cheat_read(0x0200_0000, 1), 0x12);
        assert_eq!(memory.cheat_read(0x0200_0002, 2), 0x3456);
        assert_eq!(memory.cheat_read(0x0200_0004, 4), 0x789A_BCDE);
    }

    #[test]
    fn an_address_list_takes_its_extra_addresses_from_the_next_lines_with_the_first_operand() {
        let set = raw(&[
            "30000004 02000000",
            "02000010 02000020",
            "02000030 00000000",
        ]);
        let addresses: Vec<u32> = set.list.ops.iter().map(|op| op.address).collect();
        assert_eq!(
            addresses,
            [0x0200_0000, 0x0200_0010, 0x0200_0020, 0x0200_0030]
        );
        assert!(
            set.list
                .ops
                .iter()
                .all(|op| op.operand == 0x0200_0000 && op.width == 4)
        );
        assert_eq!(set.incomplete_cheat, None);
    }

    #[test]
    fn a_button_code_waits_on_the_cheat_button() {
        let set = raw(&["8213C000 00000063"]);
        assert_eq!(
            set.list.ops,
            [
                Op::new(Kind::IfButton, 0, 0, 0),
                Op::new(Kind::Assign, 1, 0x0203_C000, 0x63),
            ]
        );
        let mut memory = Memory::default();
        memory.run(&set);
        assert_eq!(memory.cheat_read(0x0203_C000, 1), 0);
        assert!(refused(&[], "8303C000 00000063"));
    }

    #[test]
    fn conditions_skip_the_next_line_when_false() {
        let set = raw(&["D2000000 00000001", "02000001 00000055"]);
        let mut memory = Memory::default();
        memory.run(&set);
        assert_eq!(memory.byte(0x0200_0001), 0);
        let mut memory = Memory::with(&[(0x0200_0000, 1)]);
        memory.run(&set);
        assert_eq!(memory.byte(0x0200_0001), 0x55);
    }

    #[test]
    fn a_range_condition_covers_its_count_of_lines() {
        let set = raw(&[
            "E2020001 02000000",
            "02000002 00000022",
            "02000003 00000033",
        ]);
        let mut expected = Op::new(Kind::IfEq, 2, 0x0200_0000, 1);
        expected.repeat = 2;
        assert_eq!(set.list.ops[0], expected);
        let mut memory = Memory::default();
        memory.run(&set);
        assert_eq!((memory.byte(0x0200_0002), memory.byte(0x0200_0003)), (0, 0));
    }

    #[test]
    fn a_rom_patch_code_patches_two_bytes_on_the_cartridge_bus() {
        let set = raw(&["60000100 00004770"]);
        assert_eq!(
            set.list.patches,
            [RomPatch {
                address: 0x0800_0200,
                width: 2,
                value: 0x4770,
                check: None,
            }]
        );
    }

    #[test]
    fn a_master_code_hook_is_accepted_once_and_adds_no_op() {
        let set = raw(&["F00000C0 00000000"]);
        assert!(set.list.ops.is_empty());
        assert!(refused(&["F00000C0 00000000"], "F00000C4 00000000"));
    }

    #[test]
    fn unknown_code_types_are_refused() {
        assert!(refused(&[], "40000000 00000000"));
        assert!(refused(&[], "90000000 00000000"));
    }

    #[test]
    fn probabilities_follow_mgba() {
        assert_eq!(probability((0x1234_5678, TAG_CODE)), 0x100);
        assert_eq!(probability((0x0200_0000, 0xFF)), 0x40);
        assert_eq!(probability((0x0200_0000, 0x1FF)), 0x30);
        assert_eq!(probability((0x0000_0000, 0x1)), -0x60);
        assert_eq!(probability((0x4000_0000, 0x1)), -0x40);
    }
}
