use crate::game::{Game, Generation, Version};
use crate::rom::name_list;
use crate::scan::{Pattern, bank_of, banked, byte, same_bank, word};
use crate::text::{self, TERMINATOR};

const LANDMARKS: usize = 0x60;
const MAX_LANDMARK_BYTES: usize = 24;

pub(crate) fn type_names(game: Game, rom: &[u8]) -> Vec<String> {
    let count = match game.generation() {
        Generation::One => 0x1B,
        Generation::Two => 0x1C,
    };
    Pattern::new("87 21 ?? ?? 5F 16 00 19 2A 5F 56")
        .find_all(rom)
        .find_map(|at| {
            let table = same_bank(rom, at, 2)?;
            (0..count)
                .map(|i| {
                    let name = banked(rom, bank_of(table), word(rom, table + 2 * i)?)?;
                    if byte(rom, name)? == TERMINATOR {
                        return Some(String::new());
                    }
                    name_list(game, rom, name, 1)?.pop()
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn landmark_names(game: Game, rom: &[u8]) -> Vec<String> {
    if game.version != Version::Crystal {
        return Vec::new();
    }
    Pattern::new("6B 26 00 29 29 11 ?? ?? 19 2A 66 6F")
        .find_all(rom)
        .find_map(|at| {
            let entries = same_bank(rom, at, 6)?.checked_sub(2)?;
            (0..LANDMARKS)
                .map(|i| {
                    let pointer = word(rom, entries + 4 * i + 2)?;
                    landmark(game, rom, banked(rom, bank_of(entries), pointer)?)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn landmark(game: Game, rom: &[u8], at: usize) -> Option<String> {
    let mut bytes = Vec::new();
    for i in 0..MAX_LANDMARK_BYTES {
        match byte(rom, at + i)? {
            TERMINATOR => return text::decode_strict(game, &bytes),
            0x1F | 0x22 => bytes.push(0x7F),
            0x25 => {}
            other => bytes.push(other),
        }
    }
    None
}
