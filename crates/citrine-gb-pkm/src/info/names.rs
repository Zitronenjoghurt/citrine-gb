use crate::pkm::Gen;
use crate::tables::{ExtractError, Game, gen4_messages, gen5_messages};
use citrine_gb_nds_files::{Narc, NdsRom};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Names {
    pub moves: Vec<String>,
    pub items: Vec<String>,
    pub abilities: Vec<String>,
    pub types: Vec<String>,
    pub places: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub type_id: u8,
    pub category: u8,
    pub power: u8,
    pub accuracy: u8,
    pub pp: u8,
}

struct Files {
    moves: usize,
    items: usize,
    abilities: usize,
    types: usize,
    places: usize,
}

const GEN4_MOVE_LEN: usize = 16;
const GEN5_MOVE_LEN: usize = 14;

fn files(game: Game) -> Files {
    match game {
        Game::Diamond | Game::Pearl => Files {
            moves: 588,
            items: 344,
            abilities: 552,
            types: 565,
            places: 382,
        },
        Game::Platinum => Files {
            moves: 647,
            items: 392,
            abilities: 610,
            types: 624,
            places: 433,
        },
        Game::HeartGold | Game::SoulSilver => Files {
            moves: 750,
            items: 222,
            abilities: 720,
            types: 735,
            places: 279,
        },
        Game::Black | Game::White => Files {
            moves: 203,
            items: 54,
            abilities: 182,
            types: 287,
            places: 89,
        },
        Game::Black2 | Game::White2 => Files {
            moves: 403,
            items: 64,
            abilities: 374,
            types: 489,
            places: 109,
        },
    }
}

fn move_data_path(game: Game) -> &'static str {
    match game {
        Game::Diamond | Game::Pearl => "poketool/waza/waza_tbl.narc",
        Game::Platinum => "poketool/waza/pl_waza_tbl.narc",
        Game::HeartGold | Game::SoulSilver => "a/0/1/1",
        Game::Black | Game::White | Game::Black2 | Game::White2 => "a/0/2/1",
    }
}

pub(super) fn read(game: Game, rom: &NdsRom<'_>) -> Result<Names, ExtractError> {
    let messages = Narc::parse(rom.file(game.paths().messages)?)?;
    let files = files(game);
    let text = |file: usize| -> Result<Vec<String>, ExtractError> {
        let file = messages.file(file)?;
        match game.generation() {
            Gen::Four => gen4_messages(file),
            Gen::Five => gen5_messages(file),
        }
    };
    Ok(Names {
        moves: text(files.moves)?,
        items: text(files.items)?,
        abilities: text(files.abilities)?,
        types: text(files.types)?,
        places: text(files.places)?,
    })
}

pub(super) fn read_moves(game: Game, rom: &NdsRom<'_>) -> Result<Vec<Move>, ExtractError> {
    let narc = Narc::parse(rom.file(move_data_path(game))?)?;
    narc.files()
        .map(|file| {
            let r = file?;
            Ok(match game.generation() {
                Gen::Four if r.len() >= GEN4_MOVE_LEN => Move {
                    category: r[2],
                    power: r[3],
                    type_id: r[4],
                    accuracy: r[5],
                    pp: r[6],
                },
                Gen::Five if r.len() >= GEN5_MOVE_LEN => Move {
                    type_id: r[0],
                    category: r[2],
                    power: r[3],
                    accuracy: r[4],
                    pp: r[5],
                },
                _ => Move {
                    type_id: 0,
                    category: 0,
                    power: 0,
                    accuracy: 0,
                    pp: 0,
                },
            })
        })
        .collect()
}
