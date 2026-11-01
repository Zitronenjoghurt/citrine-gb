mod build;
mod dex;
mod events;
mod moves;
mod protocol;
mod rng;
#[cfg(test)]
mod tests;

use build::Context;
use citrine_gb::netplay::LinkPeer;
use citrine_gb_gamedata::{Game, Generation, RomTables};
use dex::Dex;
use protocol::{Block, Machine, Speech};
use rng::Rng;

pub use build::Offered;
pub use events::{Dvs, EVENTS, Event, Genes, Trainer, TrainerId, event, events_for};
pub use protocol::{Happening, Stage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OfferError {
    #[error("the game's data tables weren't found in the cartridge")]
    Rom,
    #[error("this event isn't for this game")]
    Generation,
    #[error("this event wasn't given out in this game's language")]
    Language,
    #[error("the game has no data for this species")]
    Species,
    #[error("a name doesn't fit this game's characters")]
    Name,
    #[error("the game doesn't know one of the moves")]
    Moves,
}

const GEN1_INDEX: [u8; 151] = [
    0x99, 0x09, 0x9A, 0xB0, 0xB2, 0xB4, 0xB1, 0xB3, 0x1C, 0x7B, 0x7C, 0x7D, 0x70, 0x71, 0x72, 0x24,
    0x96, 0x97, 0xA5, 0xA6, 0x05, 0x23, 0x6C, 0x2D, 0x54, 0x55, 0x60, 0x61, 0x0F, 0xA8, 0x10, 0x03,
    0xA7, 0x07, 0x04, 0x8E, 0x52, 0x53, 0x64, 0x65, 0x6B, 0x82, 0xB9, 0xBA, 0xBB, 0x6D, 0x2E, 0x41,
    0x77, 0x3B, 0x76, 0x4D, 0x90, 0x2F, 0x80, 0x39, 0x75, 0x21, 0x14, 0x47, 0x6E, 0x6F, 0x94, 0x26,
    0x95, 0x6A, 0x29, 0x7E, 0xBC, 0xBD, 0xBE, 0x18, 0x9B, 0xA9, 0x27, 0x31, 0xA3, 0xA4, 0x25, 0x08,
    0xAD, 0x36, 0x40, 0x46, 0x74, 0x3A, 0x78, 0x0D, 0x88, 0x17, 0x8B, 0x19, 0x93, 0x0E, 0x22, 0x30,
    0x81, 0x4E, 0x8A, 0x06, 0x8D, 0x0C, 0x0A, 0x11, 0x91, 0x2B, 0x2C, 0x0B, 0x37, 0x8F, 0x12, 0x01,
    0x28, 0x1E, 0x02, 0x5C, 0x5D, 0x9D, 0x9E, 0x1B, 0x98, 0x2A, 0x1A, 0x48, 0x35, 0x33, 0x1D, 0x3C,
    0x85, 0x16, 0x13, 0x4C, 0x66, 0x69, 0x68, 0x67, 0xAA, 0x62, 0x63, 0x5A, 0x5B, 0xAB, 0x84, 0x4A,
    0x4B, 0x49, 0x58, 0x59, 0x42, 0x83, 0x15,
];

pub(crate) fn gen1_index(dex: u16) -> Option<u8> {
    GEN1_INDEX.get(usize::from(dex).checked_sub(1)?).copied()
}

pub struct Partner {
    ctx: Context,
    event: &'static Event,
    machine: Machine,
    rng: Rng,
    offered: Offered,
    blocks: Vec<Block>,
    happened: Vec<Happening>,
    traded: u32,
}

impl Partner {
    pub fn new(
        game: Game,
        rom: &[u8],
        event: &'static Event,
        seed: u64,
    ) -> Result<Self, OfferError> {
        let tables = RomTables::load(game, rom).map_err(|_| OfferError::Rom)?;
        let pp = moves::pp_table(game, rom).ok_or(OfferError::Rom)?;
        Self::with_dex(game, Dex::from_tables(&tables), pp, event, seed)
    }

    pub(crate) fn with_dex(
        game: Game,
        dex: Dex,
        pp: Vec<u8>,
        event: &'static Event,
        seed: u64,
    ) -> Result<Self, OfferError> {
        let speech = match (event.generation, game.generation()) {
            (Generation::Two, Generation::One) => return Err(OfferError::Generation),
            (Generation::One, Generation::Two) if event.egg => return Err(OfferError::Generation),
            (Generation::Two, Generation::Two) => Speech::Gen2,
            _ => Speech::Gen1,
        };
        let ctx = Context {
            game,
            speech,
            dex,
            pp,
        };
        let mut rng = Rng::new(seed);
        let offered = build::offer(&ctx, event, &mut rng)?;
        let blocks = build::blocks(&ctx, &offered, &mut rng);
        Ok(Self {
            ctx,
            event,
            machine: Machine::new(speech),
            rng,
            offered,
            blocks,
            happened: Vec::new(),
            traded: 0,
        })
    }

    pub fn event(&self) -> &'static Event {
        self.event
    }

    pub fn offered(&self) -> &Offered {
        &self.offered
    }

    pub fn stage(&self) -> Stage {
        self.machine.stage()
    }

    pub fn traded(&self) -> u32 {
        self.traded
    }

    pub fn through_time_capsule(&self) -> bool {
        self.ctx.speech == Speech::Gen1 && self.ctx.game.generation() == Generation::Two
    }

    pub fn take_happenings(&mut self) -> Vec<Happening> {
        std::mem::take(&mut self.happened)
    }

    pub fn received(&self) -> &[Vec<u8>] {
        self.machine.received()
    }

    fn offer_again(&mut self) {
        if let Ok(offered) = build::offer(&self.ctx, self.event, &mut self.rng) {
            self.blocks = build::blocks(&self.ctx, &offered, &mut self.rng);
            self.offered = offered;
        }
    }
}

impl LinkPeer for Partner {
    fn exchange(&mut self, sent: u8) -> u8 {
        let reply = self.machine.exchange(sent, &self.blocks);
        for happening in self.machine.take_happenings() {
            if let Happening::Traded { .. } = happening {
                self.traded += 1;
                self.offer_again();
            }
            self.happened.push(happening);
        }
        reply
    }
}
