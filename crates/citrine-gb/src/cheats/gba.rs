mod codebreaker;
mod gameshark;
mod parv3;

use super::ops::{Kind, Op, OpList};
use super::{Cheat, CheatError, Format, Program};

const BASE_IO: u32 = 0x0400_0000;
const BASE_CART0: u32 = 0x0800_0000;
const OFFSET_MASK: u32 = 0x00FF_FFFF;
const TAG_CODE: u32 = 0x001D_C0DE;
const DEADFACE: u32 = 0xDEAD_FACE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Version {
    NotSet,
    GsaV1,
    GsaV1Raw,
    ParV3,
    ParV3Raw,
}

#[derive(Debug, Clone)]
pub(super) struct Set {
    list: OpList,
    hook: bool,
    incomplete_cheat: Option<usize>,
    incomplete_patch: Option<usize>,
    current_block: Option<usize>,
    remaining_addresses: i32,
    gsa_version: Version,
    gsa_seeds: [u32; 4],
    cb_rng_state: u32,
    cb_master: u32,
    cb_table: [u8; codebreaker::TABLE_LEN],
    cb_seeds: [u32; 4],
}

pub(super) fn parse(lines: &[(usize, String)]) -> Result<Cheat, CheatError> {
    let mut set = Set::new();
    let mut format = None;
    for (number, line) in lines {
        let found = set
            .add_line(line)
            .ok_or(CheatError::Unreadable { line: *number })?;
        format.get_or_insert(found);
    }
    Ok(Cheat {
        format: format.ok_or(CheatError::Empty)?,
        program: Program::Ops(set.list),
    })
}

impl Set {
    pub(super) fn new() -> Self {
        Self {
            list: OpList::default(),
            hook: false,
            incomplete_cheat: None,
            incomplete_patch: None,
            current_block: None,
            remaining_addresses: 0,
            gsa_version: Version::NotSet,
            gsa_seeds: [0; 4],
            cb_rng_state: 0,
            cb_master: 0,
            cb_table: [0; codebreaker::TABLE_LEN],
            cb_seeds: [0; 4],
        }
    }

    fn add_line(&mut self, line: &str) -> Option<Format> {
        if !line.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let op1 = u32::from_str_radix(line.get(..8)?, 16).ok()?;
        match line.len() {
            12 => {
                let op2 = u16::from_str_radix(line.get(8..)?, 16).ok()?;
                self.add_codebreaker(op1, op2)
                    .then_some(Format::CodeBreaker)
            }
            16 => {
                let op2 = u32::from_str_radix(line.get(8..)?, 16).ok()?;
                self.add_autodetect(op1, op2)?;
                match self.gsa_version {
                    Version::ParV3 | Version::ParV3Raw => Some(Format::ActionReplay),
                    _ => Some(Format::GameShark),
                }
            }
            _ => None,
        }
    }

    fn add_autodetect(&mut self, op1: u32, op2: u32) -> Option<()> {
        let added = match self.gsa_version {
            Version::NotSet => {
                let mut best = i32::MIN;
                let candidates = [
                    (
                        gameshark::probability(gameshark::decrypt(op1, op2, &gameshark::SEEDS)),
                        Version::GsaV1,
                    ),
                    (
                        parv3::probability(gameshark::decrypt(op1, op2, &parv3::SEEDS)),
                        Version::ParV3,
                    ),
                    (gameshark::probability((op1, op2)), Version::GsaV1Raw),
                    (parv3::probability((op1, op2)), Version::ParV3Raw),
                ];
                for (probability, version) in candidates {
                    if probability > best {
                        best = probability;
                        self.set_version(version);
                    }
                }
                match self.gsa_version {
                    Version::GsaV1 | Version::GsaV1Raw => self.add_gameshark(op1, op2),
                    _ => self.add_par(op1, op2),
                }
            }
            Version::GsaV1 | Version::GsaV1Raw => self.add_gameshark(op1, op2),
            Version::ParV3 | Version::ParV3Raw => self.add_par(op1, op2),
        };
        added.then_some(())
    }

    pub(super) fn set_version(&mut self, version: Version) {
        self.gsa_version = version;
        match version {
            Version::GsaV1 | Version::GsaV1Raw => self.gsa_seeds = gameshark::SEEDS,
            Version::ParV3 | Version::ParV3Raw => self.gsa_seeds = parv3::SEEDS,
            Version::NotSet => {}
        }
    }

    fn push(&mut self, op: Op) -> usize {
        self.list.ops.push(op);
        self.list.ops.len() - 1
    }

    fn take_hook(&mut self) -> bool {
        !std::mem::replace(&mut self.hook, true)
    }
}

fn if_button() -> Op {
    Op::new(Kind::IfButton, 0, 0, 0)
}

fn address_is_real(address: u32) -> i32 {
    let offset = address & OFFSET_MASK;
    let within = |size: u32, inside: i32, outside: i32| {
        if offset > size { outside } else { inside }
    };
    match address >> 24 {
        0x0 => -0x80,
        0x2 => within(0x0004_0000, 0x20, -0x40),
        0x3 => within(0x0000_8000, 0x20, -0x40),
        0x4 => within(0x0000_0400, 0x10, -0x80),
        0x5 => within(0x0000_0400, -0x8, -0x80),
        0x6 => within(0x0001_8000, -0x8, -0x80),
        0x7 => within(0x0000_0400, -0x8, -0x80),
        0x8..=0xD => -0x8,
        0xE | 0xF => within(0x0001_0000, -0x8, -0x80),
        _ => -0xC0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cheats::CheatTarget;
    use crate::cheats::ops::{self, RomPatch};
    use crate::console::ConsoleKind;
    use std::collections::HashMap;

    #[derive(Default)]
    pub(super) struct Memory {
        pub(super) bytes: HashMap<u32, u8>,
        pub(super) rom: Vec<u8>,
        pub(super) patches: Vec<(usize, u8)>,
    }

    impl Memory {
        pub(super) fn with(bytes: &[(u32, u8)]) -> Self {
            Self {
                bytes: bytes.iter().copied().collect(),
                ..Self::default()
            }
        }

        pub(super) fn byte(&self, address: u32) -> u8 {
            self.bytes.get(&address).copied().unwrap_or(0)
        }

        pub(super) fn run(&mut self, set: &Set) {
            ops::run(&set.list.ops, self);
        }
    }

    impl CheatTarget for Memory {
        fn cheat_read(&mut self, address: u32, width: u8) -> u32 {
            (0..u32::from(width)).rev().fold(0, |value, index| {
                value << 8 | u32::from(self.byte(address + index))
            })
        }

        fn cheat_write(&mut self, address: u32, width: u8, value: u32) {
            for index in 0..u32::from(width) {
                self.bytes
                    .insert(address + index, (value >> (index * 8)) as u8);
            }
        }

        fn cheat_rom(&self) -> &[u8] {
            &self.rom
        }

        fn rom_offsets(&self, address: u32, out: &mut Vec<usize>) {
            if let Some(offset) = address.checked_sub(BASE_CART0)
                && (offset as usize) < self.rom.len()
            {
                out.push(offset as usize);
            }
        }

        fn set_rom_patches(&mut self, patches: &[(usize, u8)]) {
            self.patches = patches.to_vec();
        }
    }

    pub(super) fn split(line: &str) -> (u32, u32) {
        let digits: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        let op1 = u32::from_str_radix(&digits[..8], 16).unwrap();
        let op2 = u32::from_str_radix(&digits[8..], 16).unwrap();
        (op1, op2)
    }

    fn list(code: &str) -> OpList {
        match Cheat::parse(ConsoleKind::Gba, code).unwrap().program {
            Program::Ops(list) => list,
            #[cfg(feature = "nds")]
            Program::Ds(_) => unreachable!(),
        }
    }

    fn format(code: &str) -> Format {
        Cheat::parse(ConsoleKind::Gba, code).unwrap().format()
    }

    #[test]
    fn a_raw_par_v3_line_is_detected_and_writes_memory() {
        let code = "00300000 00000078\n02300002 00005678\n04300004 12345678";
        assert_eq!(format(code), Format::ActionReplay);
        let list = list(code);
        let mut memory = Memory::default();
        ops::run(&list.ops, &mut memory);
        assert_eq!(memory.cheat_read(0x0300_0000, 1), 0x78);
        assert_eq!(memory.cheat_read(0x0300_0002, 2), 0x5678);
        assert_eq!(memory.cheat_read(0x0300_0004, 4), 0x1234_5678);
    }

    #[test]
    fn a_raw_game_shark_line_is_detected_as_game_shark() {
        let code = "22000100 0000ABCD";
        assert_eq!(format(code), Format::GameShark);
        assert_eq!(
            list(code).ops,
            [Op::new(Kind::Assign, 4, 0x0200_0100, 0xABCD)]
        );
    }

    #[test]
    fn an_encrypted_game_shark_line_is_detected_and_decrypted() {
        let (op1, op2) = gameshark::tests::encrypt(0x0200_0100, 0x0000_00FF, &gameshark::SEEDS);
        let code = format!("{op1:08X}{op2:08X}");
        assert_eq!(format(&code), Format::GameShark);
        assert_eq!(
            list(&code).ops,
            [Op::new(Kind::Assign, 1, 0x0200_0100, 0xFF)]
        );
    }

    #[test]
    fn an_encrypted_par_v3_line_is_detected_and_decrypted() {
        let (op1, op2) = gameshark::tests::encrypt(0x0030_0001, 0x0000_0042, &parv3::SEEDS);
        let code = format!("{op1:08X}{op2:08X}");
        assert_eq!(format(&code), Format::ActionReplay);
        let mut op = Op::new(Kind::Assign, 1, 0x0300_0001, 0x42);
        op.address_offset = 1;
        assert_eq!(list(&code).ops, [op]);
    }

    #[test]
    fn the_first_sixteen_digit_line_locks_the_format_for_the_rest() {
        let code = "00300000 00000078\n02300100 0000ABCD";
        let list = list(code);
        assert_eq!(list.ops.len(), 2);
        assert_eq!(list.ops[1].address, 0x0300_0100);
        assert_eq!(list.ops[1].width, 2);
    }

    #[test]
    fn a_twelve_digit_line_is_codebreaker() {
        let code = "32000100 0063";
        assert_eq!(format(code), Format::CodeBreaker);
        assert_eq!(
            list(code).ops,
            [Op::new(Kind::Assign, 1, 0x0200_0100, 0x63)]
        );
    }

    #[test]
    fn a_game_shark_rom_patch_targets_the_cartridge_bus() {
        let code = "60012345 00001234";
        assert_eq!(
            list(code).patches,
            [RomPatch {
                address: 0x0802_468A,
                width: 2,
                value: 0x1234,
                check: None,
            }]
        );
    }

    #[test]
    fn lines_that_are_not_codes_are_refused_with_their_line_number() {
        assert_eq!(
            Cheat::parse(ConsoleKind::Gba, "32000100 0063\n1234"),
            Err(CheatError::Unreadable { line: 2 })
        );
        assert_eq!(
            Cheat::parse(ConsoleKind::Gba, "0200010G 0000ABCD"),
            Err(CheatError::Unreadable { line: 1 })
        );
        assert_eq!(
            Cheat::parse(ConsoleKind::Gba, "02000100 0000ABCD 12"),
            Err(CheatError::Unreadable { line: 1 })
        );
    }

    #[test]
    fn address_scores_follow_mgba_regions() {
        assert_eq!(address_is_real(0x0200_0000), 0x20);
        assert_eq!(address_is_real(0x0204_0001), -0x40);
        assert_eq!(address_is_real(0x0400_0130), 0x10);
        assert_eq!(address_is_real(0x0800_0000), -0x8);
        assert_eq!(address_is_real(0x1000_0000), -0xC0);
    }
}
