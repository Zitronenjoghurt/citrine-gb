use std::sync::Arc;

pub const ARM9_RAM: u32 = 0x0200_0000;
pub const ARM7_RAM: u32 = 0x0380_0000;
pub const BRANCH_TO_SELF: u32 = 0xEAFF_FFFE;

pub const SLEEP_ON_LID: [u32; 10] = [
    0xE3A0_0301,
    0xE3A0_1840,
    0xE580_1210,
    0xE3A0_10C0,
    0xE5C0_1301,
    0xE3A0_2042,
    0xE58F_2000,
    BRANCH_TO_SELF,
    0,
    0,
];

pub fn rom(arm9: &[u32], arm7: &[u32]) -> Arc<[u8]> {
    let mut rom = vec![0; 0x1000];
    rom[..4].copy_from_slice(b"TEST");
    let words = |code: &[u32]| -> Vec<u8> { code.iter().flat_map(|w| w.to_le_bytes()).collect() };
    for (at, offset, ram, code) in [
        (0x20, 0x200, ARM9_RAM, words(arm9)),
        (0x30, 0x800, ARM7_RAM, words(arm7)),
    ] {
        for (index, value) in [offset, ram, ram, code.len() as u32]
            .into_iter()
            .enumerate()
        {
            rom[at + 4 * index..at + 4 * index + 4].copy_from_slice(&value.to_le_bytes());
        }
        rom[offset as usize..offset as usize + code.len()].copy_from_slice(&code);
    }
    rom.into()
}
