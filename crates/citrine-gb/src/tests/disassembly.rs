use crate::debug::disassembly::{Confidence, Disassembly};
use crate::gb::{GameBoy, GbModel};
use crate::rom::Rom;

fn bytes_at(disassembly: &Disassembly, offset: u32) -> Vec<u8> {
    disassembly
        .iter()
        .find(|entry| entry.at.offset == offset)
        .map(|entry| entry.instruction_bytes().to_vec())
        .unwrap_or_else(|| panic!("no entry at {offset:04X}"))
}

#[test]
fn instruction_bytes_hold_the_whole_instruction() {
    let mut data = vec![0u8; 0x8000];
    data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    data[0x0150..0x015A]
        .copy_from_slice(&[0x3E, 0x12, 0xEA, 0x00, 0xC0, 0xCB, 0x37, 0x18, 0xF7, 0x00]);
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&Rom::new(&data)).expect("load");

    let mut disassembly = Disassembly::new();
    disassembly.analyze(&gb.cartridge, 0x0100, Confidence::Fetched);

    assert_eq!(bytes_at(&disassembly, 0x0100), [0x00]);
    assert_eq!(bytes_at(&disassembly, 0x0101), [0xC3, 0x50, 0x01]);
    assert_eq!(bytes_at(&disassembly, 0x0150), [0x3E, 0x12]);
    assert_eq!(bytes_at(&disassembly, 0x0152), [0xEA, 0x00, 0xC0]);
    assert_eq!(bytes_at(&disassembly, 0x0155), [0xCB, 0x37]);
    assert_eq!(bytes_at(&disassembly, 0x0157), [0x18, 0xF7]);
}

fn overlap_rom() -> GameBoy {
    let mut data = vec![0u8; 0x8000];
    data[0x0158..0x015C].copy_from_slice(&[0xCB, 0x37, 0x18, 0xFE]);
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&Rom::new(&data)).expect("load");
    gb
}

fn offsets_from(disassembly: &Disassembly, start: u32, end: u32) -> Vec<u32> {
    disassembly
        .iter()
        .map(|entry| entry.at.offset)
        .filter(|offset| (start..end).contains(offset))
        .collect()
}

#[test]
fn fetched_entry_survives_a_later_overlapping_analysis() {
    let gb = overlap_rom();
    let mut disassembly = Disassembly::new();
    disassembly.on_fetch(&gb.cartridge, 0x0158);
    disassembly.analyze(&gb.cartridge, 0x0159, Confidence::Unconditional);

    assert_eq!(offsets_from(&disassembly, 0x0158, 0x015C), [0x0158, 0x015A]);
    assert_eq!(bytes_at(&disassembly, 0x0158), [0xCB, 0x37]);
}

#[test]
fn fetched_entry_replaces_an_earlier_overlapping_analysis() {
    let gb = overlap_rom();
    let mut disassembly = Disassembly::new();
    disassembly.analyze(&gb.cartridge, 0x0159, Confidence::Unconditional);
    disassembly.on_fetch(&gb.cartridge, 0x0158);

    assert_eq!(offsets_from(&disassembly, 0x0158, 0x015C), [0x0158, 0x015A]);
    assert_eq!(bytes_at(&disassembly, 0x0158), [0xCB, 0x37]);
}
