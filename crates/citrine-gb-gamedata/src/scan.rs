use crate::rom::{BANK_SIZE, in_rom_bank, rom_offset};

pub(crate) struct Pattern(Vec<Option<u8>>);

impl Pattern {
    pub fn new(text: &str) -> Self {
        Self(
            text.split_whitespace()
                .map(|part| u8::from_str_radix(part, 16).ok())
                .collect(),
        )
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn matches(&self, rom: &[u8], at: usize) -> bool {
        rom.get(at..at + self.0.len()).is_some_and(|window| {
            window
                .iter()
                .zip(&self.0)
                .all(|(byte, want)| want.is_none_or(|want| *byte == want))
        })
    }

    pub fn find_all<'a>(&'a self, rom: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
        (0..rom.len().saturating_sub(self.0.len())).filter(|&at| self.matches(rom, at))
    }
}

pub(crate) fn byte(rom: &[u8], at: usize) -> Option<u8> {
    rom.get(at).copied()
}

pub(crate) fn word(rom: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([byte(rom, at)?, byte(rom, at + 1)?]))
}

pub(crate) fn bank_of(at: usize) -> usize {
    at / BANK_SIZE
}

pub(crate) fn banked(rom: &[u8], bank: usize, addr: u16) -> Option<usize> {
    if !in_rom_bank(addr) && bank != 0 {
        return None;
    }
    let at = rom_offset(bank, addr);
    (at < rom.len()).then_some(at)
}

pub(crate) fn same_bank(rom: &[u8], code_at: usize, operand: usize) -> Option<usize> {
    let addr = word(rom, code_at + operand)?;
    if addr < 0x4000 {
        return Some(usize::from(addr));
    }
    banked(rom, bank_of(code_at), addr)
}
