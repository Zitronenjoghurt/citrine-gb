use crate::game::{Game, Generation};
use crate::rom::name_list;
use crate::scan::{Pattern, bank_of, banked, byte, same_bank, word};
use crate::text::{self, TERMINATOR};

const MAX_LANDMARK_BYTES: usize = 24;
const HYPHEN: u8 = 0xE3;

pub(crate) fn type_names(game: Game, rom: &[u8]) -> Vec<String> {
    let count = match game.generation() {
        Generation::One => 0x1B,
        Generation::Two => 0x1C,
        Generation::Three => return Vec::new(),
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

pub(crate) fn landmark(game: Game, rom: &[u8], at: usize) -> Option<String> {
    let mut bytes = Vec::new();
    for i in 0..MAX_LANDMARK_BYTES {
        match byte(rom, at + i)? {
            TERMINATOR => return text::decode_strict(game, &bytes),
            0x1F | 0x22 if bytes.last() == Some(&HYPHEN) => {}
            0x1F | 0x22 => bytes.push(0x7F),
            0x25 => {}
            other => bytes.push(other),
        }
    }
    None
}
