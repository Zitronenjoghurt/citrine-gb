mod events;
mod offer;
mod side;
mod trade;

use crate::Partner;
use crate::dex::{Dex, Entry};
use crate::events::{Event, Genes, Trainer};
use citrine_gb_gamedata::{BaseStats, Game, Generation, Language, Version};

pub(super) fn game(version: Version, language: Language) -> Game {
    Game {
        version,
        language,
        revision: 0,
    }
}

fn gen1_record(dex: u8, types: [u8; 2], catch_rate: u8) -> Vec<u8> {
    let mut record = vec![0u8; 28];
    record[0] = dex;
    record[1..6].copy_from_slice(&[100, 100, 100, 100, 100]);
    record[6..8].copy_from_slice(&types);
    record[8] = catch_rate;
    record[19] = 3;
    record
}

fn gen2_record(dex: u8, types: [u8; 2], egg_cycles: u8) -> Vec<u8> {
    let mut record = vec![0u8; 32];
    record[0] = dex;
    record[1..7].copy_from_slice(&[100, 100, 100, 100, 100, 100]);
    record[7..9].copy_from_slice(&types);
    record[9] = 45;
    record[15] = egg_cycles;
    record[22] = 3;
    record
}

pub(super) const PSYCHIC: u8 = 0x18;
pub(super) const EGG_CYCLES: u8 = 10;

pub(super) fn dex(game: Game) -> Dex {
    let mut dex = Dex::default();
    let generation = game.generation();
    let count = match generation {
        Generation::One => 151,
        Generation::Two => 251,
        Generation::Three => 0,
    };
    for number in 1..=count {
        let (index, record) = match generation {
            Generation::One => (
                crate::gen1_index(number).expect("an index"),
                gen1_record(number as u8, [PSYCHIC, PSYCHIC], 45),
            ),
            Generation::Two => (
                number as u8,
                gen2_record(number as u8, [PSYCHIC, PSYCHIC], EGG_CYCLES),
            ),
            Generation::Three => break,
        };
        let name = match game.language {
            Language::Japanese => "ミュウ",
            _ => "MEW",
        };
        dex.insert(
            number,
            Entry {
                index,
                name: name.to_string(),
                base: BaseStats::from_record(generation, &record).expect("record"),
            },
        );
    }
    dex
}

pub(super) fn pp() -> Vec<u8> {
    let mut pp = vec![20; 252];
    pp[0] = 0;
    pp[1] = 35;
    pp
}

pub(super) fn partner(game: Game, event: &'static Event, seed: u64) -> Partner {
    Partner::with_dex(game, dex(game), pp(), event, seed).expect("an offer")
}

pub(super) fn custom(event: Event) -> &'static Event {
    Box::leak(Box::new(event))
}

pub(super) fn mew(trainers: &'static [Trainer]) -> Event {
    Event {
        key: "test-mew",
        place: "test",
        year: 1999,
        generation: Generation::One,
        dex: 151,
        level: 5,
        moves: &[1],
        trainers,
        genes: Genes::Random,
        item: None,
        egg: false,
        source: "test",
    }
}
