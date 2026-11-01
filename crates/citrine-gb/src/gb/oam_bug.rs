use crate::gb::ppu::Ppu;
use crate::gb::ppu::types::mode::PpuMode;
use crate::{ReadMemory, WriteMemory};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OamAccess {
    Read,
    Write,
    ReadIncrement,
}

fn accessed_row(ppu: &Ppu) -> Option<u16> {
    if !ppu.lcdc.lcd_enabled || ppu.stat.ppu_mode != PpuMode::OamScan {
        return None;
    }
    let row = ppu.line_dot_counter.saturating_sub(2) / 4;
    (1..20).contains(&row).then_some(row)
}

fn word(ppu: &Ppu, row: u16, index: u16) -> u16 {
    let addr = 0xFE00 + row * 8 + index * 2;
    u16::from_le_bytes([ppu.read_naive(addr), ppu.read_naive(addr + 1)])
}

fn set_word(ppu: &mut Ppu, row: u16, index: u16, value: u16) {
    let addr = 0xFE00 + row * 8 + index * 2;
    let [lo, hi] = value.to_le_bytes();
    ppu.write_naive(addr, lo);
    ppu.write_naive(addr + 1, hi);
}

fn copy_words(ppu: &mut Ppu, from: u16, to: u16, first: u16) {
    for index in first..4 {
        let value = word(ppu, from, index);
        set_word(ppu, to, index, value);
    }
}

fn corrupt_row(ppu: &mut Ppu, row: u16, glitch: fn(u16, u16, u16) -> u16) {
    let a = word(ppu, row, 0);
    let b = word(ppu, row - 1, 0);
    let c = word(ppu, row - 1, 2);
    set_word(ppu, row, 0, glitch(a, b, c));
    copy_words(ppu, row - 1, row, 1);
}

pub fn corrupt(ppu: &mut Ppu, access: OamAccess) {
    let Some(row) = accessed_row(ppu) else {
        return;
    };

    match access {
        OamAccess::Write => corrupt_row(ppu, row, |a, b, c| ((a ^ c) & (b ^ c)) ^ c),
        OamAccess::Read => corrupt_row(ppu, row, |a, b, c| b | (a & c)),
        OamAccess::ReadIncrement => {
            if (4..19).contains(&row) {
                let a = word(ppu, row - 2, 0);
                let b = word(ppu, row - 1, 0);
                let c = word(ppu, row, 0);
                let d = word(ppu, row - 1, 2);
                set_word(ppu, row - 1, 0, (b & (a | c | d)) | (a & c & d));
                copy_words(ppu, row - 1, row, 0);
                copy_words(ppu, row - 1, row - 2, 0);
            }
            corrupt_row(ppu, row, |a, b, c| b | (a & c));
        }
    }
}
