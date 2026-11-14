use super::{BASE_IO, Set};
use crate::cheats::ops::{Kind, Op};

pub(super) const TABLE_LEN: usize = 0x30;

const GAME_ID: u32 = 0x0;
const HOOK: u32 = 0x1;
const OR_2: u32 = 0x2;
const ASSIGN_1: u32 = 0x3;
const FILL: u32 = 0x4;
const FILL_8: u32 = 0x5;
const AND_2: u32 = 0x6;
const IF_EQ: u32 = 0x7;
const ASSIGN_2: u32 = 0x8;
const ENCRYPT: u32 = 0x9;
const IF_NE: u32 = 0xA;
const IF_GT: u32 = 0xB;
const IF_LT: u32 = 0xC;
const IF_SPECIAL: u32 = 0xD;
const ADD_2: u32 = 0xE;
const IF_AND: u32 = 0xF;
const REG_KEYINPUT: u32 = 0x130;

fn load(op1: u32, op2: u16) -> [u8; 6] {
    let [a, b, c, d] = op1.to_be_bytes();
    let [e, f] = op2.to_be_bytes();
    [a, b, c, d, e, f]
}

fn store([a, b, c, d, e, f]: [u8; 6]) -> (u32, u16) {
    (u32::from_be_bytes([a, b, c, d]), u16::from_be_bytes([e, f]))
}

impl Set {
    pub(super) fn add_codebreaker(&mut self, op1: u32, op2: u16) -> bool {
        let (op1, op2) = if self.cb_master != 0 {
            self.cb_decrypt(op1, op2)
        } else {
            (op1, op2)
        };

        if let Some(index) = self.incomplete_cheat.take() {
            if let Some(op) = self.list.ops.get_mut(index) {
                op.repeat = op1 & 0xFFFF;
                op.address_offset = i32::from(op2);
                op.operand_offset = (op1 >> 16) as i32;
            }
            return true;
        }

        let kind = match op1 >> 28 {
            GAME_ID => return true,
            HOOK => return self.take_hook(),
            OR_2 => (Kind::Or, 2),
            ASSIGN_1 => (Kind::Assign, 1),
            FILL => (Kind::Assign, 2),
            FILL_8 => return false,
            AND_2 => (Kind::And, 2),
            IF_EQ => (Kind::IfEq, 2),
            ASSIGN_2 => (Kind::Assign, 2),
            ENCRYPT => {
                self.cb_reseed(op1, op2);
                return true;
            }
            IF_NE => (Kind::IfNe, 2),
            IF_GT => (Kind::IfGt, 2),
            IF_LT => (Kind::IfLt, 2),
            IF_SPECIAL => {
                if op1 & 0x0FFF_FFFF != 0x20 {
                    return false;
                }
                self.push(Op::new(
                    Kind::IfNand,
                    2,
                    BASE_IO | REG_KEYINPUT,
                    u32::from(op2),
                ));
                return true;
            }
            ADD_2 => (Kind::Add, 2),
            IF_AND => (Kind::IfAnd, 2),
            _ => return false,
        };
        let index = self.push(Op::new(kind.0, kind.1, op1 & 0x0FFF_FFFF, u32::from(op2)));
        if op1 >> 28 == FILL {
            self.incomplete_cheat = Some(index);
        }
        true
    }

    fn cb_decrypt(&self, op1: u32, op2: u16) -> (u32, u16) {
        let mut buffer = load(op1, op2);
        for i in (0..TABLE_LEN).rev() {
            let entry = usize::from(self.cb_table[i]);
            let (offset_x, bit_x) = (i >> 3, i & 7);
            let (offset_y, bit_y) = (entry >> 3, entry & 7);
            let (Some(&bx), Some(&by)) = (buffer.get(offset_x), buffer.get(offset_y)) else {
                continue;
            };
            let x = (bx >> bit_x) & 1;
            let y = (by >> bit_y) & 1;
            buffer[offset_x] = bx & !(1 << bit_x) | y << bit_x;
            buffer[offset_y] = buffer[offset_y] & !(1 << bit_y) | x << bit_y;
        }
        let (op1, op2) = store(buffer);
        let op1 = op1 ^ self.cb_seeds[0];
        let op2 = op2 ^ self.cb_seeds[1] as u16;

        let mut buffer = load(op1, op2);
        let master = self.cb_master;
        for i in 0..5 {
            buffer[i] ^= (master >> 8) as u8 ^ buffer[i + 1];
        }
        buffer[5] ^= (master >> 8) as u8;
        for i in (1..6).rev() {
            buffer[i] ^= master as u8 ^ buffer[i - 1];
        }
        buffer[0] ^= master as u8;
        let (op1, op2) = store(buffer);
        (op1 ^ self.cb_seeds[2], op2 ^ self.cb_seeds[3] as u16)
    }

    fn cb_rand(&mut self) -> u32 {
        let step = |state: u32| state.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        let roll = step(self.cb_rng_state);
        let roll2 = step(roll);
        let roll3 = step(roll2);
        let mix = (roll << 14) & 0xC000_0000 | (roll2 >> 1) & 0x3FFF_8000 | (roll3 >> 16) & 0x7FFF;
        self.cb_rng_state = roll3;
        mix
    }

    fn cb_swap_index(&mut self) -> usize {
        let mut roll = self.cb_rand();
        let mut count = TABLE_LEN as u32;
        if roll == count {
            roll = 0;
        }
        if roll < count {
            return roll as usize;
        }

        let mut bit: u32 = 1;
        while count < 0x1000_0000 && count < roll {
            count <<= 4;
            bit <<= 4;
        }
        while count < 0x8000_0000 && count < roll {
            count <<= 1;
            bit <<= 1;
        }

        let mut mask;
        loop {
            mask = 0;
            if roll >= count {
                roll -= count;
            }
            for shift in 1..4 {
                if roll >= count >> shift {
                    roll -= count >> shift;
                    mask |= bit.rotate_right(shift);
                }
            }
            if roll == 0 || bit >> 4 == 0 {
                break;
            }
            bit >>= 4;
            count >>= 4;
        }

        mask &= 0xE000_0000;
        if mask == 0 || bit & 7 == 0 {
            return roll as usize;
        }
        for shift in (1..4).rev() {
            if mask & bit.rotate_right(shift) != 0 {
                roll = roll.wrapping_add(count >> shift);
            }
        }
        roll as usize
    }

    fn cb_reseed(&mut self, op1: u32, op2: u16) {
        self.cb_rng_state = u32::from(op2 & 0xFF) ^ 0x1111;
        for (index, entry) in self.cb_table.iter_mut().enumerate() {
            *entry = index as u8;
        }
        for _ in 0..0x50 {
            let x = self.cb_swap_index();
            let y = self.cb_swap_index();
            if x < TABLE_LEN && y < TABLE_LEN {
                self.cb_table.swap(x, y);
            }
        }

        self.cb_rng_state = 0x4EFA_D1C3;
        for _ in 0..((op1 >> 24) & 0xF) {
            self.cb_rng_state = self.cb_rand();
        }
        self.cb_seeds[2] = self.cb_rand();
        self.cb_seeds[3] = self.cb_rand();

        self.cb_rng_state = u32::from(op2 >> 8) ^ 0xF254;
        for _ in 0..(op2 >> 8) {
            self.cb_rng_state = self.cb_rand();
        }
        self.cb_seeds[0] = self.cb_rand();
        self.cb_seeds[1] = self.cb_rand();

        self.cb_master = op1;
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Memory, split};
    use super::*;
    use crate::cheats::{Cheat, Format, Program};
    use crate::console::ConsoleKind;

    fn set(lines: &[&str]) -> Set {
        let mut set = Set::new();
        for line in lines {
            let (op1, op2) = split(line);
            assert!(set.add_codebreaker(op1, op2 as u16), "{line}");
        }
        set
    }

    fn refused(lines: &[&str], last: &str) -> bool {
        let mut set = set(lines);
        let (op1, op2) = split(last);
        !set.add_codebreaker(op1, op2 as u16)
    }

    #[test]
    fn reseeding_builds_the_same_table_and_seeds_as_mgba() {
        let set = set(&["9A1B2C3D 5E6F"]);
        assert_eq!(
            set.cb_seeds,
            [0xC9DE_8742, 0x5E59_A985, 0x50BB_6344, 0xD888_8A77]
        );
        assert_eq!(set.cb_master, 0x9A1B_2C3D);
        assert_eq!(
            set.cb_table,
            [
                46, 4, 21, 3, 22, 2, 13, 12, 41, 14, 20, 40, 9, 15, 35, 5, 17, 42, 43, 25, 7, 31,
                29, 45, 24, 11, 33, 18, 28, 8, 0, 23, 16, 27, 1, 36, 19, 32, 30, 37, 10, 26, 47,
                38, 6, 34, 44, 39,
            ]
        );
        assert_eq!(set.cb_decrypt(0x3ABC_DEF0, 0x1234), (0xD1FD_227B, 0xE4A7));
    }

    #[test]
    fn the_real_pokemon_fire_red_master_code_decrypts_to_game_id_and_hook() {
        let mut set = set(&["9820F6AE 8203"]);
        assert_eq!(set.cb_decrypt(0x2370_5BFC, 0x2025), (0x0000_295F, 0x000A));
        assert_eq!(set.cb_decrypt(0xCC39_1336, 0xE64B), (0x101D_C9D4, 0x0007));
        assert!(set.add_codebreaker(0x2370_5BFC, 0x2025));
        assert!(set.add_codebreaker(0xCC39_1336, 0xE64B));
        assert!(set.list.ops.is_empty());
    }

    #[test]
    fn the_real_pokemon_fire_red_encounter_less_code_writes_one_byte() {
        let cheat = Cheat::parse(
            ConsoleKind::Gba,
            "9820F6AE8203\n23705BFC2025\nCC391336E64B\nA83F9AD9A2F7",
        )
        .unwrap();
        assert_eq!(cheat.format(), Format::CodeBreaker);
        match cheat.program {
            Program::Ops(list) => {
                assert_eq!(list.ops, [Op::new(Kind::Assign, 1, 0x0203_86D8, 0)])
            }
            #[cfg(feature = "nds")]
            Program::Ds(_) => unreachable!(),
        }
    }

    #[test]
    fn plain_codes_map_to_their_operations() {
        let set = set(&[
            "00000F6D 000A",
            "100008B4 0007",
            "83000138 0100",
            "33001249 0001",
            "22000000 00F0",
            "62000002 0F00",
            "E2000004 0003",
            "72000006 0001",
            "A2000006 0001",
            "B2000006 0001",
            "C2000006 0001",
            "F2000006 0001",
        ]);
        let expected = [
            Op::new(Kind::Assign, 2, 0x0300_0138, 0x0100),
            Op::new(Kind::Assign, 1, 0x0300_1249, 0x01),
            Op::new(Kind::Or, 2, 0x0200_0000, 0xF0),
            Op::new(Kind::And, 2, 0x0200_0002, 0x0F00),
            Op::new(Kind::Add, 2, 0x0200_0004, 3),
            Op::new(Kind::IfEq, 2, 0x0200_0006, 1),
            Op::new(Kind::IfNe, 2, 0x0200_0006, 1),
            Op::new(Kind::IfGt, 2, 0x0200_0006, 1),
            Op::new(Kind::IfLt, 2, 0x0200_0006, 1),
            Op::new(Kind::IfAnd, 2, 0x0200_0006, 1),
        ];
        assert_eq!(set.list.ops, expected);
    }

    #[test]
    fn a_key_condition_tests_keyinput() {
        let set = set(&["D0000020 0005", "82000000 0063"]);
        assert_eq!(
            set.list.ops[0],
            Op::new(Kind::IfNand, 2, 0x0400_0130, 0x0005)
        );
        let mut memory = Memory::with(&[(0x0400_0130, 0xFF), (0x0400_0131, 0x03)]);
        memory.run(&set);
        assert_eq!(memory.byte(0x0200_0000), 0);
        let mut memory = Memory::with(&[(0x0400_0130, 0xFA), (0x0400_0131, 0x03)]);
        memory.run(&set);
        assert_eq!(memory.byte(0x0200_0000), 0x63);
    }

    #[test]
    fn a_fill_takes_count_and_steps_from_the_next_line() {
        let set = set(&["42000000 0010", "00010003 0002"]);
        let mut expected = Op::new(Kind::Assign, 2, 0x0200_0000, 0x10);
        expected.repeat = 3;
        expected.address_offset = 2;
        expected.operand_offset = 1;
        assert_eq!(set.list.ops, [expected]);
        let mut memory = Memory::default();
        memory.run(&set);
        let values: Vec<u8> = (0..3).map(|at| memory.byte(0x0200_0000 + at * 2)).collect();
        assert_eq!(values, [0x10, 0x11, 0x12]);
    }

    #[test]
    fn unsupported_codes_and_a_second_hook_are_refused() {
        assert!(refused(&[], "52000000 0001"));
        assert!(refused(&[], "D0000010 0001"));
        assert!(refused(&["10000100 0007"], "10000200 0007"));
    }
}
