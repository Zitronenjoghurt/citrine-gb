mod base;
mod battle;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
mod game;
mod gen1;
mod gen2;
pub mod gen3;
mod hangul;
mod layout;
mod list;
mod matchups;
mod moves;
mod names;
mod pic;
mod places;
mod region_map;
mod rom;
mod scan;
mod sprite;
mod state;
mod tables;
mod text;
mod wild;

pub use base::{BaseStats, GrowthRate};
pub use battle::{Battle, BattleKind, Clock, Fighter};
pub use game::{Game, Generation, Language, Version, identify};
pub use matchups::Effect;
pub use moves::MoveData;
pub use pic::{Sprite, SpriteError};
pub use places::{Region, Spot};
pub use region_map::{Marker, RegionMap};
pub use rom::{Place, RomTables};
pub use sprite::{Palette, Side, SpriteTables};
pub use state::*;
pub use tables::{Ram, Tables};
pub use wild::{Encounter, Method, WildList};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("the save is too small for this game")]
    TooShort,
    #[error("the save holds no valid copy of the game")]
    NoValidCopy,
    #[error("the game's work RAM holds no game yet")]
    NotRunning,
    #[error("the ROM's name tables were not found")]
    RomTables,
    #[error("this game's save needs its ROM to read")]
    NeedsRom,
}

pub fn encode_name(game: Game, name: &str, length: usize) -> Option<Vec<u8>> {
    let mut bytes = text::encode(game, name)?;
    if bytes.is_empty() || bytes.len() >= length {
        return None;
    }
    bytes.resize(length, text::TERMINATOR);
    Some(bytes)
}

pub fn read_save(game: Game, sram: &[u8]) -> Result<GameState, ReadError> {
    match game.generation() {
        Generation::One => gen1::read_save(game, sram),
        Generation::Two => gen2::read_save(game, sram),
        Generation::Three => Err(ReadError::NeedsRom),
    }
}

pub fn read_live(tables: &RomTables, wram: &[u8], sram: &[u8]) -> Result<GameState, ReadError> {
    let game = tables.game;
    let layout = layout::Layout::of(game);
    if sram.len() < layout.sram_size
        || wram.len() < 0x2000
        || tables.live.len() < layout.live_starts.len()
    {
        return Err(ReadError::NotRunning);
    }
    let mut image = sram.to_vec();
    for block in &tables.live {
        let from = usize::from(block.wram - 0xC000);
        let source = wram
            .get(from..from + block.len)
            .ok_or(ReadError::NotRunning)?;
        image
            .get_mut(block.sram..block.sram + block.len)
            .ok_or(ReadError::NotRunning)?
            .copy_from_slice(source);
    }
    let mut state = match game.generation() {
        Generation::One => gen1::read_live(game, &image),
        Generation::Two => gen2::read_live(game, &image),
        Generation::Three => Err(ReadError::NeedsRom),
    }?;
    state.battle = battle::read_battle(tables, wram);
    state.clock = battle::read_clock(tables, wram);
    Ok(state)
}

#[cfg(test)]
mod tests;
