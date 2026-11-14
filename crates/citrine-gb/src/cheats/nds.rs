use super::{Cheat, CheatError, CheatTarget, Format, Program};

const STEP_BUDGET: u32 = 1 << 22;
const ADDRESS_MASK: u32 = 0x0FFF_FFFF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArCode {
    words: Vec<u32>,
}

pub(super) fn parse(lines: &[(usize, String)]) -> Result<Cheat, CheatError> {
    let mut words = Vec::new();
    for (number, line) in lines {
        let unreadable = CheatError::Unreadable { line: *number };
        if line.is_empty() || line.len() % 16 != 0 || !line.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(unreadable);
        }
        for start in (0..line.len()).step_by(8) {
            let digits = line.get(start..start + 8).ok_or(unreadable)?;
            words.push(u32::from_str_radix(digits, 16).map_err(|_| unreadable)?);
        }
    }
    if words.is_empty() {
        return Err(CheatError::Empty);
    }
    Ok(Cheat {
        format: Format::ActionReplay,
        program: Program::Ds(ArCode { words }),
    })
}

pub(super) fn run(code: &ArCode, target: &mut (impl CheatTarget + ?Sized)) {
    Engine::new(&code.words, target).run();
}

struct Engine<'a, T: CheatTarget + ?Sized> {
    words: &'a [u32],
    target: &'a mut T,
    pc: usize,
    offset: u32,
    data: u32,
    cond: bool,
    cond_stack: u32,
    loop_start: usize,
    loop_count: u32,
    loop_cond: bool,
    loop_cond_stack: u32,
    c5_count: u32,
    budget: u32,
}

impl<'a, T: CheatTarget + ?Sized> Engine<'a, T> {
    fn new(words: &'a [u32], target: &'a mut T) -> Self {
        Self {
            words,
            target,
            pc: 0,
            offset: 0,
            data: 0,
            cond: true,
            cond_stack: 0,
            loop_start: 0,
            loop_count: 0,
            loop_cond: true,
            loop_cond_stack: 0,
            c5_count: 0,
            budget: STEP_BUDGET,
        }
    }

    fn run(&mut self) {
        while let Some(&[a, b]) = self.words.get(self.pc..self.pc.saturating_add(2)) {
            self.pc += 2;
            if !self.spend(1) || self.step(a, b).is_none() {
                return;
            }
        }
    }

    fn spend(&mut self, cost: u32) -> bool {
        match self.budget.checked_sub(cost) {
            Some(left) => {
                self.budget = left;
                true
            }
            None => {
                self.budget = 0;
                false
            }
        }
    }

    fn next_word(&mut self) -> Option<u32> {
        let word = *self.words.get(self.pc)?;
        self.pc += 1;
        Some(word)
    }

    fn push_cond(&mut self, cond: bool) {
        self.cond_stack = self.cond_stack << 1 | u32::from(self.cond);
        self.cond = cond;
    }

    fn compare_address(&self, a: u32) -> u32 {
        match a & ADDRESS_MASK {
            0 => self.offset,
            address => address,
        }
    }

    fn read32(&mut self, address: u32) -> u32 {
        self.target.cheat_read(address, 4)
    }

    fn read16(&mut self, address: u32) -> u16 {
        (self.target.cheat_read(address, 2) & 0xFFFF) as u16
    }

    fn read8(&mut self, address: u32) -> u8 {
        (self.target.cheat_read(address, 1) & 0xFF) as u8
    }

    fn masked_halfword(&mut self, a: u32, b: u32) -> (u16, u16) {
        let address = self.compare_address(a);
        let value = self.read16(address);
        let mask = !((b >> 16) as u16);
        ((b & 0xFFFF) as u16, mask & value)
    }

    fn step(&mut self, a: u32, b: u32) -> Option<()> {
        let op = (a >> 24) as u8;
        if ((op < 0xD0 && op != 0xC5) || op > 0xD2) && !self.cond {
            if op & 0xF0 == 0xE0 {
                let pairs = usize::try_from(b.div_ceil(8)).unwrap_or(usize::MAX);
                self.pc = self.pc.saturating_add(pairs.saturating_mul(2));
            }
            return Some(());
        }
        let address = (a & ADDRESS_MASK).wrapping_add(self.offset);
        match op {
            0x00..=0x0F => self.target.cheat_write(address, 4, b),
            0x10..=0x1F => self.target.cheat_write(address, 2, b & 0xFFFF),
            0x20..=0x2F => self.target.cheat_write(address, 1, b & 0xFF),
            0x30..=0x6F => {
                let address = self.compare_address(a);
                let check = self.read32(address);
                let cond = match op >> 4 {
                    3 => b > check,
                    4 => b < check,
                    5 => b == check,
                    _ => b != check,
                };
                self.push_cond(cond);
            }
            0x70..=0xAF => {
                let (value, check) = self.masked_halfword(a, b);
                let cond = match op >> 4 {
                    7 => value > check,
                    8 => value < check,
                    9 => value == check,
                    _ => value != check,
                };
                self.push_cond(cond);
            }
            0xB0..=0xBF => self.offset = self.read32(address),
            0xC0 => {
                self.loop_start = self.pc;
                self.loop_count = b;
                self.loop_cond = self.cond;
                self.loop_cond_stack = self.cond_stack;
            }
            0xC5 => {
                self.c5_count = self.c5_count.wrapping_add(1);
                if self.cond {
                    let mask = b & 0xFFFF;
                    let check = b >> 16;
                    self.push_cond(self.c5_count & mask == check);
                }
            }
            0xC6 => self.target.cheat_write(b, 4, self.offset),
            0xD0 => {
                self.cond = self.cond_stack & 1 != 0;
                self.cond_stack >>= 1;
            }
            0xD1 | 0xD2 => {
                if self.loop_count > 0 {
                    self.loop_count -= 1;
                    self.pc = self.loop_start;
                } else if op == 0xD1 {
                    self.cond = self.loop_cond;
                    self.cond_stack = self.loop_cond_stack;
                } else {
                    self.offset = 0;
                    self.data = 0;
                    self.cond_stack = 0;
                    self.cond = true;
                }
            }
            0xD3 => self.offset = b,
            0xD4 => self.data_op(a & 0xFF, b),
            0xD5 => self.data = b,
            0xD6 => {
                self.target
                    .cheat_write(b.wrapping_add(self.offset), 4, self.data);
                self.offset = self.offset.wrapping_add(4);
            }
            0xD7 => {
                self.target
                    .cheat_write(b.wrapping_add(self.offset), 2, self.data & 0xFFFF);
                self.offset = self.offset.wrapping_add(2);
            }
            0xD8 => {
                self.target
                    .cheat_write(b.wrapping_add(self.offset), 1, self.data & 0xFF);
                self.offset = self.offset.wrapping_add(1);
            }
            0xD9 => self.data = self.read32(b.wrapping_add(self.offset)),
            0xDA => self.data = u32::from(self.read16(b.wrapping_add(self.offset))),
            0xDB => self.data = u32::from(self.read8(b.wrapping_add(self.offset))),
            0xDC => self.offset = self.offset.wrapping_add(b),
            0xE0..=0xEF => self.patch(address, b)?,
            0xF0..=0xFF => self.copy(a & ADDRESS_MASK, b)?,
            _ => return None,
        }
        Some(())
    }

    fn data_op(&mut self, kind: u32, b: u32) {
        let shift = b & 0xFF;
        self.data = match kind {
            0x00 => self.data.wrapping_add(b),
            0x01 => self.data | b,
            0x02 => self.data & b,
            0x03 => self.data ^ b,
            0x04 if shift > 31 => 0,
            0x04 => self.data << shift,
            0x05 if shift > 31 => 0,
            0x05 => self.data >> shift,
            0x06 => self.data.rotate_right(b & 0x1F),
            0x07 => ((self.data as i32) >> shift.min(31)) as u32,
            0x08 => self.data.wrapping_mul(b),
            _ => self.data,
        };
    }

    fn patch(&mut self, mut destination: u32, length: u32) -> Option<()> {
        let mut left = length;
        while left >= 8 {
            if !self.spend(2) {
                return None;
            }
            for _ in 0..2 {
                let word = self.next_word()?;
                self.target.cheat_write(destination, 4, word);
                destination = destination.wrapping_add(4);
            }
            left -= 8;
        }
        if left == 0 {
            return Some(());
        }
        let low = self.next_word()?;
        let high = self.next_word()?;
        let mut bytes = [0u8; 8];
        bytes[..4].copy_from_slice(&low.to_le_bytes());
        bytes[4..].copy_from_slice(&high.to_le_bytes());
        let mut index = 0;
        if left >= 4 {
            self.target.cheat_write(destination, 4, low);
            destination = destination.wrapping_add(4);
            index = 4;
            left -= 4;
        }
        for &byte in bytes.iter().skip(index).take(left as usize) {
            self.target.cheat_write(destination, 1, u32::from(byte));
            destination = destination.wrapping_add(1);
        }
        Some(())
    }

    fn copy(&mut self, mut destination: u32, length: u32) -> Option<()> {
        let mut source = self.offset;
        let mut left = length;
        while left >= 4 {
            if !self.spend(1) {
                return None;
            }
            let word = self.read32(source);
            self.target.cheat_write(destination, 4, word);
            source = source.wrapping_add(4);
            destination = destination.wrapping_add(4);
            left -= 4;
        }
        while left > 0 {
            let byte = u32::from(self.read8(source));
            self.target.cheat_write(destination, 1, byte);
            source = source.wrapping_add(1);
            destination = destination.wrapping_add(1);
            left -= 1;
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::ConsoleKind;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Bus {
        memory: HashMap<u32, u8>,
        writes: Vec<(u32, u8, u32)>,
    }

    impl Bus {
        fn with(words: &[(u32, u32)]) -> Self {
            let mut bus = Self::default();
            for &(address, value) in words {
                bus.store(address, 4, value);
            }
            bus
        }

        fn store(&mut self, address: u32, width: u8, value: u32) {
            for (index, byte) in value
                .to_le_bytes()
                .iter()
                .take(usize::from(width))
                .enumerate()
            {
                self.memory
                    .insert(address.wrapping_add(index as u32), *byte);
            }
        }

        fn load(&self, address: u32, width: u8) -> u32 {
            (0..u32::from(width)).rev().fold(0, |value, index| {
                value << 8 | u32::from(*self.memory.get(&address.wrapping_add(index)).unwrap_or(&0))
            })
        }
    }

    impl CheatTarget for Bus {
        fn cheat_read(&mut self, address: u32, width: u8) -> u32 {
            self.load(address, width)
        }

        fn cheat_write(&mut self, address: u32, width: u8, value: u32) {
            self.writes.push((address, width, value));
            self.store(address, width, value);
        }

        fn cheat_rom(&self) -> &[u8] {
            &[]
        }

        fn rom_offsets(&self, _address: u32, _out: &mut Vec<usize>) {}

        fn set_rom_patches(&mut self, _patches: &[(usize, u8)]) {}
    }

    #[derive(Default)]
    struct Counter {
        accesses: u64,
    }

    impl CheatTarget for Counter {
        fn cheat_read(&mut self, _address: u32, _width: u8) -> u32 {
            self.accesses += 1;
            0
        }

        fn cheat_write(&mut self, _address: u32, _width: u8, _value: u32) {
            self.accesses += 1;
        }

        fn cheat_rom(&self) -> &[u8] {
            &[]
        }

        fn rom_offsets(&self, _address: u32, _out: &mut Vec<usize>) {}

        fn set_rom_patches(&mut self, _patches: &[(usize, u8)]) {}
    }

    fn code(text: &str) -> ArCode {
        match Cheat::parse(ConsoleKind::Nds, text).unwrap().program {
            Program::Ds(code) => code,
            Program::Ops(_) => unreachable!(),
        }
    }

    fn run_on(bus: &mut Bus, text: &str) {
        run(&code(text), bus);
    }

    fn writes(text: &str, memory: &[(u32, u32)]) -> Vec<(u32, u8, u32)> {
        let mut bus = Bus::with(memory);
        run_on(&mut bus, text);
        bus.writes
    }

    #[test]
    fn parse_reads_word_pairs_and_lines_with_several_pairs() {
        let cheat = Cheat::parse(
            ConsoleKind::Nds,
            "02000000 0000ABCD\n\n12000004 00001234 d2000000 00000000",
        )
        .unwrap();
        assert_eq!(cheat.format(), Format::ActionReplay);
        assert_eq!(
            code("02000000 0000ABCD\n12000004 00001234 D2000000 00000000").words,
            [
                0x0200_0000,
                0x0000_ABCD,
                0x1200_0004,
                0x0000_1234,
                0xD200_0000,
                0
            ]
        );
    }

    #[test]
    fn parse_refuses_odd_words_short_lines_and_non_hex() {
        for (text, line) in [
            ("02000000 00000001\n02000000", 2),
            ("02000000 00000001 02000004", 1),
            ("0200000 00000001", 1),
            ("0200000G 00000001", 1),
            ("+2000000 00000001", 1),
            ("02000000 00000001\nnope", 2),
        ] {
            assert_eq!(
                Cheat::parse(ConsoleKind::Nds, text),
                Err(CheatError::Unreadable { line }),
                "{text}"
            );
        }
        assert_eq!(
            Cheat::parse(ConsoleKind::Nds, " \n"),
            Err(CheatError::Empty)
        );
    }

    #[test]
    fn types_0_1_2_write_32_16_and_8_bits_and_ignore_the_top_nibble() {
        assert_eq!(
            writes(
                "02000000 12345678\n12000010 ABCD1234\n2A000020 FFFFFF56",
                &[]
            ),
            [
                (0x0200_0000, 4, 0x1234_5678),
                (0x0200_0010, 2, 0x1234),
                (0x0A00_0020, 1, 0x56),
            ]
        );
    }

    #[test]
    fn types_3_to_6_compare_a_word_and_gate_until_d0() {
        let memory = [(0x0200_0100, 50)];
        let gated = |line: &str| {
            !writes(
                &format!("{line}\n02000000 00000001\nD0000000 00000000"),
                &memory,
            )
            .is_empty()
        };
        assert!(gated("32000100 00000051"));
        assert!(!gated("32000100 00000032"));
        assert!(gated("42000100 00000031"));
        assert!(!gated("42000100 00000032"));
        assert!(gated("52000100 00000032"));
        assert!(!gated("52000100 00000033"));
        assert!(gated("62000100 00000033"));
        assert!(!gated("62000100 00000032"));
    }

    #[test]
    fn types_7_to_a_compare_a_masked_halfword() {
        let memory = [(0x0200_0100, 0x0000_12F0)];
        let gated = |line: &str| {
            !writes(
                &format!("{line}\n02000000 00000001\nD0000000 00000000"),
                &memory,
            )
            .is_empty()
        };
        assert!(gated("92000100 FF0000F0"));
        assert!(!gated("92000100 000012F1"));
        assert!(gated("72000100 FF0000F1"));
        assert!(!gated("72000100 FF0000F0"));
        assert!(gated("82000100 FF0000EF"));
        assert!(!gated("82000100 FF0000F0"));
        assert!(gated("A2000100 FF0000F1"));
        assert!(!gated("A2000100 FF0000F0"));
    }

    #[test]
    fn a_compare_at_address_zero_reads_the_offset_register_and_others_ignore_it() {
        let memory = [(0x0200_0100, 7), (0x0200_0200, 9)];
        assert_eq!(
            writes(
                "D3000000 02000100\n50000000 00000007\n00000000 00000001\nD0000000 00000000",
                &memory
            ),
            [(0x0200_0100, 4, 1)]
        );
        assert_eq!(
            writes(
                "D3000000 00000100\n52000200 00000009\n02000000 00000001\nD0000000 00000000",
                &memory
            ),
            [(0x0200_0100, 4, 1)]
        );
    }

    #[test]
    fn nested_ifs_pop_the_condition_stack_like_melonds() {
        let text = "\
            52000100 00000001\n\
            02000000 0000000A\n\
            52000104 00000002\n\
            02000004 0000000B\n\
            D0000000 00000000\n\
            02000008 0000000C\n\
            D0000000 00000000\n\
            0200000C 0000000D";
        let all = writes(text, &[(0x0200_0100, 1), (0x0200_0104, 2)]);
        assert_eq!(
            all.iter().map(|w| w.2).collect::<Vec<_>>(),
            [0xA, 0xB, 0xC, 0xD]
        );
        let inner_false = writes(text, &[(0x0200_0100, 1), (0x0200_0104, 3)]);
        assert_eq!(
            inner_false.iter().map(|w| w.2).collect::<Vec<_>>(),
            [0xA, 0xC, 0xD]
        );
        let outer_false = writes(text, &[(0x0200_0100, 0), (0x0200_0104, 2)]);
        assert_eq!(outer_false.iter().map(|w| w.2).collect::<Vec<_>>(), [0xC]);
    }

    #[test]
    fn a_c0_loop_runs_its_body_count_plus_one_times() {
        let all = writes(
            "D5000000 00000007\nD3000000 02000000\nC0000000 00000003\nD6000000 00000010\nD2000000 00000000\nD6000000 00000000",
            &[],
        );
        assert_eq!(
            all,
            [
                (0x0200_0010, 4, 7),
                (0x0200_0014, 4, 7),
                (0x0200_0018, 4, 7),
                (0x0200_001C, 4, 7),
                (0, 4, 0),
            ]
        );
    }

    #[test]
    fn d1_ends_a_loop_by_restoring_the_condition_from_its_start() {
        let all = writes(
            "C0000000 00000001\n52000100 00000005\nD1000000 00000000\n02000000 00000001",
            &[],
        );
        assert_eq!(all, [(0x0200_0000, 4, 1)]);
    }

    #[test]
    fn c5_counts_from_zero_on_every_run_and_skips_its_push_when_false() {
        let mut bus = Bus::default();
        let text = "C5000000 00010001\n02000000 00000001\nD0000000 00000000";
        run_on(&mut bus, text);
        run_on(&mut bus, text);
        assert_eq!(bus.writes.len(), 2);
        assert!(
            writes(
                "C5000000 00000001\n02000000 00000001\nD0000000 00000000",
                &[]
            )
            .is_empty()
        );
        assert_eq!(
            writes(
                "52000100 00000001\nC5000000 00000000\nD0000000 00000000\n02000000 00000001",
                &[]
            ),
            [(0x0200_0000, 4, 1)]
        );
    }

    #[test]
    fn c4_and_unknown_codes_stop_the_cheat_unless_skipped() {
        assert!(writes("C4000000 00000000\n02000000 00000001", &[]).is_empty());
        assert!(writes("C1000000 00000000\n02000000 00000001", &[]).is_empty());
        assert_eq!(
            writes(
                "52000100 00000001\nC1000000 00000000\nD0000000 00000000\n02000000 00000001",
                &[]
            ),
            [(0x0200_0000, 4, 1)]
        );
    }

    #[test]
    fn c6_stores_the_offset_register() {
        assert_eq!(
            writes("D3000000 02001234\nC6000000 02000100", &[]),
            [(0x0200_0100, 4, 0x0200_1234)]
        );
    }

    #[test]
    fn b_loads_the_offset_through_a_pointer_and_d3_dc_set_and_add_it() {
        let memory = [(0x0200_0100, 0x0220_0000)];
        assert_eq!(
            writes("B2000100 00000000\n00000010 00000005", &memory),
            [(0x0220_0010, 4, 5)]
        );
        assert_eq!(
            writes(
                "D3000000 00000100\nB2000000 00000000\n10000004 00000006",
                &memory
            ),
            [(0x0220_0004, 2, 6)]
        );
        assert_eq!(
            writes(
                "D3000000 02000000\nDC000000 00000040\n20000001 00000007",
                &[]
            ),
            [(0x0200_0041, 1, 7)]
        );
    }

    #[test]
    fn d4_data_operations_follow_melonds() {
        let data =
            |ops: &str| writes(&format!("D5000000 F0000010\n{ops}\nD6000000 02000000"), &[])[0].2;
        assert_eq!(data("D4000000 00000001"), 0xF000_0011);
        assert_eq!(data("D4000001 0000000F"), 0xF000_001F);
        assert_eq!(data("D4000002 000000F0"), 0x0000_0010);
        assert_eq!(data("D4000003 F0000000"), 0x0000_0010);
        assert_eq!(data("D4000004 00000004"), 0x0000_0100);
        assert_eq!(data("D4000004 00000020"), 0);
        assert_eq!(data("D4000005 00000004"), 0x0F00_0001);
        assert_eq!(data("D4000005 00000120"), 0);
        assert_eq!(data("D4000006 00000024"), 0x0F00_0001);
        assert_eq!(data("D4000007 00000004"), 0xFF00_0001);
        assert_eq!(data("D4000007 00000040"), 0xFFFF_FFFF);
        assert_eq!(data("D4000008 00000002"), 0xE000_0020);
        assert_eq!(data("D4000009 00000002"), 0xF000_0010);
    }

    #[test]
    fn d6_d7_d8_store_the_data_register_and_advance_the_offset() {
        assert_eq!(
            writes(
                "D5000000 11223344\nD3000000 02000000\nD6000000 00000000\nD7000000 00000000\nD8000000 00000000\nD8000000 00000000",
                &[]
            ),
            [
                (0x0200_0000, 4, 0x1122_3344),
                (0x0200_0004, 2, 0x3344),
                (0x0200_0006, 1, 0x44),
                (0x0200_0007, 1, 0x44),
            ]
        );
    }

    #[test]
    fn d9_da_db_load_the_data_register() {
        let memory = [(0x0200_0100, 0xAABB_CCDD)];
        let loaded = |line: &str| {
            writes(
                &format!("D3000000 02000000\n{line}\nD3000000 00000000\nD6000000 02000000"),
                &memory,
            )[0]
            .2
        };
        assert_eq!(loaded("D9000000 00000100"), 0xAABB_CCDD);
        assert_eq!(loaded("DA000000 00000100"), 0xCCDD);
        assert_eq!(loaded("DB000000 00000101"), 0xCC);
    }

    #[test]
    fn e_patch_with_an_odd_length_writes_words_then_bytes_and_resumes_after_its_data() {
        assert_eq!(
            writes(
                "D3000000 00000100\nE2000000 0000000D\n11111111 22222222\n33333333 44332211\n02000000 00000009",
                &[]
            ),
            [
                (0x0200_0100, 4, 0x1111_1111),
                (0x0200_0104, 4, 0x2222_2222),
                (0x0200_0108, 4, 0x3333_3333),
                (0x0200_010C, 1, 0x11),
                (0x0200_0100, 4, 9),
            ]
        );
        assert_eq!(
            writes("E2000000 00000003\n00CCBBAA 00000000", &[]),
            [
                (0x0200_0000, 1, 0xAA),
                (0x0200_0001, 1, 0xBB),
                (0x0200_0002, 1, 0xCC)
            ]
        );
    }

    #[test]
    fn a_skipped_e_patch_skips_its_data() {
        assert_eq!(
            writes(
                "52000100 00000001\nE2000000 00000009\nD0000000 00000000\nD0000000 00000000\n02000000 00000005\nD0000000 00000000\n02000004 00000006",
                &[]
            ),
            [(0x0200_0004, 4, 6)]
        );
    }

    #[test]
    fn f_copies_from_the_offset_to_its_address() {
        let memory = [(0x0200_0100, 0x4433_2211), (0x0200_0104, 0x0000_6655)];
        let mut bus = Bus::with(&memory);
        run_on(&mut bus, "D3000000 02000100\nF2000200 00000006");
        assert_eq!(
            bus.writes,
            [
                (0x0200_0200, 4, 0x4433_2211),
                (0x0200_0204, 1, 0x55),
                (0x0200_0205, 1, 0x66)
            ]
        );
    }

    #[test]
    fn huge_lengths_and_loop_counts_stay_bounded() {
        for text in [
            "E2000000 FFFFFFFF\n00000000 00000000",
            "52000100 00000001\nE2000000 FFFFFFFF\nD0000000 00000000",
            "F2000000 FFFFFFFF",
            "C0000000 FFFFFFFF\n02000000 00000001\nD1000000 00000000",
        ] {
            let mut counter = Counter::default();
            run(&code(text), &mut counter);
            assert!(counter.accesses <= u64::from(STEP_BUDGET) * 2, "{text}");
        }
    }

    #[test]
    fn pokemon_platinum_us_max_cash_chapter_cheats_writes_999999_through_the_save_pointer() {
        let text = "94000130 fcff0000\n62101d40 00000000\nb2101d40 00000000\n00000090 000f423f\nd2000000 00000000";
        let save = 0x0227_1234;
        let mut held = Bus::with(&[(0x0210_1D40, save), (0x0400_0130, 0x0000_00FF)]);
        run_on(&mut held, text);
        assert_eq!(held.writes, [(save + 0x90, 4, 999_999)]);
        let mut released = Bus::with(&[(0x0210_1D40, save), (0x0400_0130, 0x0000_03FF)]);
        run_on(&mut released, text);
        assert!(released.writes.is_empty());
        let mut no_save = Bus::with(&[(0x0400_0130, 0x0000_00FF)]);
        run_on(&mut no_save, text);
        assert!(no_save.writes.is_empty());
    }
}
