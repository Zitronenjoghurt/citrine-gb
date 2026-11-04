use crate::game::Game;
use crate::layout::Gen1Layout;
use crate::list::{self, Shape};
use crate::state::*;
use crate::{ReadError, text};

const SRAM_SIZE: usize = 0x8000;
const PARTY_MON: usize = 44;
const BOX_MON: usize = 33;
const DEX: u16 = 151;
const GOT_POKEDEX: usize = 37;

pub(crate) fn read_save(game: Game, sram: &[u8]) -> Result<GameState, ReadError> {
    let layout = Gen1Layout::of(game);
    if sram.len() < SRAM_SIZE {
        return Err(ReadError::TooShort);
    }
    if checksum(&sram[layout.player_name..layout.checksum]) != sram[layout.checksum] {
        return Err(ReadError::NoValidCopy);
    }
    read(game, layout, sram, SaveCopy::Main).ok_or(ReadError::NoValidCopy)
}

pub(crate) fn read_live(game: Game, image: &[u8]) -> Result<GameState, ReadError> {
    let layout = Gen1Layout::of(game);
    read(game, layout, image, SaveCopy::Live).ok_or(ReadError::NotRunning)
}

pub(crate) fn checksum(bytes: &[u8]) -> u8 {
    !bytes.iter().fold(0u8, |sum, &b| sum.wrapping_add(b))
}

fn read(game: Game, layout: &Gen1Layout, sram: &[u8], copy: SaveCopy) -> Option<GameState> {
    let name_len = game.name_length();
    let name = &sram[layout.player_name..][..name_len];
    if !text::is_name(game, name) {
        return None;
    }
    let party_shape = Shape {
        capacity: 6,
        mon_size: PARTY_MON,
        name_len,
    };
    let box_shape = Shape {
        capacity: layout.box_slots,
        mon_size: BOX_MON,
        name_len,
    };
    let party = pokemon(game, party_shape, &sram[layout.party..])?;
    let box_byte = sram[layout.current_box];
    let current_box = usize::from(box_byte & 0x7F);
    let box_count = layout.boxes_per_bank * 2;
    if current_box >= box_count {
        return None;
    }
    let boxes = (0..box_count)
        .map(|number| {
            let bytes = if number == current_box {
                Some(&sram[layout.box_data..])
            } else if box_byte & 0x80 != 0 {
                let bank = 0x4000 + (number / layout.boxes_per_bank) * 0x2000;
                Some(&sram[bank + (number % layout.boxes_per_bank) * box_shape.size()..])
            } else {
                None
            };
            PcBox {
                number,
                name: None,
                pokemon: bytes
                    .and_then(|bytes| pokemon(game, box_shape, bytes))
                    .unwrap_or_default(),
            }
        })
        .collect();
    let play = &sram[layout.play_time..];
    let badges = sram[layout.badges];
    Some(GameState {
        game,
        trainer: Trainer {
            name: text::decode(game, name),
            id: u16::from_be_bytes([sram[layout.player_id], sram[layout.player_id + 1]]),
            gender: None,
            money: list::bcd(&sram[layout.money..][..3]),
            mom_money: None,
            coins: list::bcd(&sram[layout.coins..][..2]) as u16,
            play_time: PlayTime {
                hours: u16::from(play[0]),
                minutes: play[2],
                seconds: play[3],
            },
            badges: (0..8).map(|bit| badges >> bit & 1 == 1).collect(),
        },
        party,
        boxes,
        current_box,
        bag: Bag {
            items: list::items(&sram[layout.bag..], 20)?,
            ..Bag::default()
        },
        pc_items: list::items(&sram[layout.pc_items..], 50)?,
        pokedex: Pokedex {
            seen: list::flags(&sram[layout.pokedex_seen..][..19], DEX),
            caught: list::flags(&sram[layout.pokedex_owned..][..19], DEX),
        },
        copy,
        location: layout.location.map(|at| {
            Position::Gb(Location {
                group: 0,
                map: sram[at],
                x: sram[at + 4],
                y: sram[at + 3],
                outside: None,
            })
        }),
        journey: journey(layout, sram, badges),
        hall_of_fame: layout.hall_of_fame.map(|at| sram[at]),
        battle: None,
        clock: None,
    })
}

fn journey(layout: &Gen1Layout, sram: &[u8], badges: u8) -> Vec<Step> {
    let (Some(events), Some(hall_of_fame)) = (layout.events, layout.hall_of_fame) else {
        return Vec::new();
    };
    let mut steps = vec![Step {
        milestone: Milestone::Pokedex,
        done: list::flag(&sram[events..], GOT_POKEDEX),
    }];
    steps.extend((0..8).map(|bit| Step {
        milestone: Milestone::Badge(bit),
        done: badges >> bit & 1 == 1,
    }));
    steps.push(Step {
        milestone: Milestone::EliteFour,
        done: sram[hall_of_fame] > 0,
    });
    steps
}

fn pokemon(game: Game, shape: Shape, bytes: &[u8]) -> Option<Vec<Pokemon>> {
    Some(
        shape
            .entries(bytes)?
            .into_iter()
            .map(|entry| {
                let m = entry.mon;
                let party = m.len() == PARTY_MON;
                let word = |at: usize| u16::from_be_bytes([m[at], m[at + 1]]);
                Pokemon::Hatched(Box::new(Mon {
                    species: u16::from(m[0]),
                    nickname: text::decode(game, entry.nickname),
                    ot_name: text::decode(game, entry.ot_name),
                    ot_id: word(12),
                    level: if party { m[33] } else { m[3] },
                    exp: list::be(&m[14..17]),
                    moves: (0..4)
                        .filter(|&i| m[8 + i] != 0)
                        .map(|i| MoveSlot {
                            id: u16::from(m[8 + i]),
                            pp: m[29 + i] & 0x3F,
                            pp_ups: m[29 + i] >> 6,
                        })
                        .collect(),
                    held_item: None,
                    genes: Genes::Gb {
                        dvs: Dvs::from_bytes(m[27], m[28]),
                        stat_exp: [word(17), word(19), word(21), word(23), word(25)],
                    },
                    hp: Some(word(1)),
                    stats: party.then(|| Stats {
                        max_hp: word(34),
                        attack: word(36),
                        defense: word(38),
                        speed: word(40),
                        special_attack: word(42),
                        special_defense: word(42),
                    }),
                    status: Some(m[4]),
                    friendship: None,
                    pokerus: None,
                    caught: None,
                }))
            })
            .collect(),
    )
}
