use crate::game::{Game, Generation};

const GEN1_RECORD: usize = 6;
const GEN2_RECORD: usize = 7;
const GEN1_MOVES: usize = 165;
const GEN2_MOVES: usize = 251;
const MAX_PP_BONUS: u8 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveData {
    pub type_id: u8,
    pub power: u8,
    pub accuracy: u8,
    pub pp: u8,
}

impl MoveData {
    pub fn max_pp(self, pp_ups: u8) -> u8 {
        self.pp + (self.pp / 5).min(MAX_PP_BONUS) * pp_ups.min(3)
    }

    pub fn accuracy_percent(self) -> u8 {
        ((u16::from(self.accuracy) * 100 + 127) / 255) as u8
    }
}

pub(crate) fn load(game: Game, rom: &[u8]) -> Vec<MoveData> {
    let (record, count) = match game.generation() {
        Generation::One => (GEN1_RECORD, GEN1_MOVES),
        Generation::Two => (GEN2_RECORD, GEN2_MOVES),
        Generation::Three => return Vec::new(),
    };
    let span = record * count;
    let Some(at) = (0..rom.len().saturating_sub(span)).find(|&at| {
        (0..count).all(|i| usize::from(rom[at + i * record]) == i + 1)
            && (0..count).all(|i| rom[at + i * record + 5] <= 40)
    }) else {
        return Vec::new();
    };
    rom[at..at + span]
        .chunks_exact(record)
        .map(|r| MoveData {
            type_id: r[3],
            power: r[2],
            accuracy: r[4],
            pp: r[5],
        })
        .collect()
}
