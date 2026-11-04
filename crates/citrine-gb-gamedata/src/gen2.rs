use crate::game::{Game, Version};
use crate::layout::Gen2Layout;
use crate::list::{self, Shape};
use crate::state::*;
use crate::{ReadError, text};

const PARTY_MON: usize = 48;
const BOX_MON: usize = 32;
const DEX: u16 = 251;
const EGG: u8 = 0xFD;
const CHECK_START: u8 = 0x63;
const CHECK_END: u8 = 0x7F;
const BEAT_ELITE_FOUR: usize = 68;
const KANTO_SPAWNS: std::ops::RangeInclusive<usize> = 2..=12;

pub(crate) fn read_save(game: Game, sram: &[u8]) -> Result<GameState, ReadError> {
    let layout = Gen2Layout::of(game);
    if sram.len() < layout.sram_size {
        return Err(ReadError::TooShort);
    }
    if main_valid(layout, sram)
        && let Some(state) = read(game, layout, sram, SaveCopy::Main)
    {
        return Ok(state);
    }
    let image = backup_image(layout, sram).ok_or(ReadError::NoValidCopy)?;
    read(game, layout, &image, SaveCopy::Backup).ok_or(ReadError::NoValidCopy)
}

pub(crate) fn read_live(game: Game, image: &[u8]) -> Result<GameState, ReadError> {
    let layout = Gen2Layout::of(game);
    read(game, layout, image, SaveCopy::Live).ok_or(ReadError::NotRunning)
}

pub(crate) fn checksum(bytes: &[u8]) -> u16 {
    bytes
        .iter()
        .fold(0u16, |sum, &b| sum.wrapping_add(u16::from(b)))
}

fn stored_checksum(sram: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([sram[at], sram[at + 1]])
}

fn main_valid(layout: &Gen2Layout, sram: &[u8]) -> bool {
    let (start, end) = layout.main;
    sram[layout.main_checks.0] == CHECK_START
        && sram[layout.main_checks.1] == CHECK_END
        && checksum(&sram[start..end]) == stored_checksum(sram, layout.main_checksum)
}

fn backup_image(layout: &Gen2Layout, sram: &[u8]) -> Option<Vec<u8>> {
    if layout.backup.is_empty()
        || sram[layout.backup_checks.0] != CHECK_START
        || sram[layout.backup_checks.1] != CHECK_END
    {
        return None;
    }
    let sum = layout.backup.iter().fold(0u16, |sum, &(_, backup, len)| {
        sum.wrapping_add(checksum(&sram[backup..backup + len]))
    });
    if sum != stored_checksum(sram, layout.backup_checksum) {
        return None;
    }
    let mut image = sram.to_vec();
    for &(main, backup, len) in layout.backup {
        image.copy_within(backup..backup + len, main);
    }
    Some(image)
}

pub(crate) fn entered_kanto(layout: &Gen2Layout, sram: &[u8]) -> bool {
    let spawns = &sram[layout.visited_spawns..][..4];
    let visited = |bit: usize| spawns[bit / 8] >> (bit % 8) & 1 == 1;
    sram[layout.kanto_badges] != 0
        || (sram[layout.johto_badges] == 0xFF && KANTO_SPAWNS.into_iter().any(visited))
}

fn read(game: Game, layout: &Gen2Layout, sram: &[u8], copy: SaveCopy) -> Option<GameState> {
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
    let current_box = usize::from(sram[layout.current_box] & 0x0F);
    if current_box >= layout.boxes.len() {
        return None;
    }
    let box_name_len = layout.box_name_len;
    let boxes = layout
        .boxes
        .iter()
        .enumerate()
        .map(|(number, &offset)| {
            let at = if number == current_box && copy == SaveCopy::Live {
                layout.box_data
            } else {
                offset
            };
            PcBox {
                number,
                name: Some(text::decode(
                    game,
                    &sram[layout.box_names + number * box_name_len..][..box_name_len],
                )),
                pokemon: pokemon(game, box_shape, &sram[at..]).unwrap_or_default(),
            }
        })
        .collect();
    let money = layout.money;
    let play = &sram[layout.play_time..];
    let johto = sram[layout.johto_badges];
    let kanto = sram[layout.kanto_badges];
    let badge_count = if entered_kanto(layout, sram) { 16 } else { 8 };
    let tms_hms = sram[layout.tms_hms..][..57]
        .iter()
        .enumerate()
        .filter(|(_, count)| **count != 0)
        .map(|(index, &count)| ItemStack {
            item: u16::from(tm_hm_item(index)),
            count: u16::from(count),
        })
        .collect();
    let gender = layout.gender.map(|at| {
        if sram[at] & 1 == 0 {
            Gender::Male
        } else {
            Gender::Female
        }
    });
    Some(GameState {
        game,
        trainer: Trainer {
            name: text::decode(game, name),
            id: u16::from_be_bytes([sram[layout.player_id], sram[layout.player_id + 1]]),
            gender,
            money: list::be(&sram[money..][..3]),
            mom_money: Some(list::be(&sram[money + 3..][..3])),
            coins: u16::from_be_bytes([sram[money + 7], sram[money + 8]]),
            play_time: PlayTime {
                hours: u16::from_be_bytes([play[0], play[1]]),
                minutes: play[2],
                seconds: play[3],
            },
            badges: (0..badge_count)
                .map(|bit| {
                    if bit < 8 {
                        johto >> bit & 1 == 1
                    } else {
                        kanto >> (bit - 8) & 1 == 1
                    }
                })
                .collect(),
        },
        party,
        boxes,
        current_box,
        bag: Bag {
            items: list::items(&sram[layout.items..], 20)?,
            key_items: list::key_items(&sram[layout.key_items..], 25)?,
            balls: list::items(&sram[layout.balls..], 12)?,
            tms_hms,
            berries: Vec::new(),
        },
        pc_items: list::items(&sram[layout.pc_items..], 50)?,
        pokedex: Pokedex {
            seen: list::flags(&sram[layout.pokedex_seen..][..32], DEX),
            caught: list::flags(&sram[layout.pokedex_caught..][..32], DEX),
        },
        copy,
        location: layout.location.map(|at| {
            Position::Gb(Location {
                group: sram[at],
                map: sram[at + 1],
                x: sram[at + 3],
                y: sram[at + 2],
                outside: layout.outside.map(|at| (sram[at], sram[at + 1])),
            })
        }),
        journey: journey(layout, sram, johto, kanto, badge_count),
        hall_of_fame: None,
        battle: None,
        clock: None,
    })
}

fn journey(layout: &Gen2Layout, sram: &[u8], johto: u8, kanto: u8, badges: usize) -> Vec<Step> {
    let (Some(events), Some(status)) = (layout.events, layout.status_flags) else {
        return Vec::new();
    };
    let badge = |bit: usize| Step {
        milestone: Milestone::Badge(bit),
        done: if bit < 8 {
            johto >> bit & 1 == 1
        } else {
            kanto >> (bit - 8) & 1 == 1
        },
    };
    let mut steps = vec![Step {
        milestone: Milestone::Pokedex,
        done: sram[status] & 1 == 1,
    }];
    steps.extend((0..8).map(badge));
    steps.push(Step {
        milestone: Milestone::EliteFour,
        done: list::flag(&sram[events..], BEAT_ELITE_FOUR),
    });
    steps.extend((8..badges).map(badge));
    steps
}

fn tm_hm_item(index: usize) -> u8 {
    let index = index as u8;
    match index {
        0..=3 => 0xBF + index,
        4..=27 => 0xC4 + (index - 4),
        28..=49 => 0xDD + (index - 28),
        _ => 0xF3 + (index - 50),
    }
}

fn pokemon(game: Game, shape: Shape, bytes: &[u8]) -> Option<Vec<Pokemon>> {
    let crystal = game.version == Version::Crystal;
    Some(
        shape
            .entries(bytes)?
            .into_iter()
            .map(|entry| {
                if entry.listed == EGG {
                    return Pokemon::Egg;
                }
                let m = entry.mon;
                let party = m.len() == PARTY_MON;
                let word = |at: usize| u16::from_be_bytes([m[at], m[at + 1]]);
                Pokemon::Hatched(Box::new(Mon {
                    species: u16::from(m[0]),
                    nickname: text::decode(game, entry.nickname),
                    ot_name: text::decode(game, entry.ot_name),
                    ot_id: word(6),
                    level: m[31],
                    exp: list::be(&m[8..11]),
                    moves: (0..4)
                        .filter(|&i| m[2 + i] != 0)
                        .map(|i| MoveSlot {
                            id: u16::from(m[2 + i]),
                            pp: m[23 + i] & 0x3F,
                            pp_ups: m[23 + i] >> 6,
                        })
                        .collect(),
                    held_item: Some(u16::from(m[1])).filter(|&item| item != 0),
                    genes: Genes::Gb {
                        dvs: Dvs::from_bytes(m[21], m[22]),
                        stat_exp: [word(11), word(13), word(15), word(17), word(19)],
                    },
                    hp: party.then(|| word(34)),
                    stats: party.then(|| Stats {
                        max_hp: word(36),
                        attack: word(38),
                        defense: word(40),
                        speed: word(42),
                        special_attack: word(44),
                        special_defense: word(46),
                    }),
                    status: party.then(|| m[32]),
                    friendship: Some(m[27]),
                    pokerus: Some(m[28]),
                    caught: (crystal && (m[29] != 0 || m[30] != 0)).then(|| caught(m[29], m[30])),
                }))
            })
            .collect(),
    )
}

fn caught(time_level: u8, gender_location: u8) -> Caught {
    Caught {
        time_of_day: match time_level >> 6 {
            1 => TimeOfDay::Morning,
            2 => TimeOfDay::Day,
            3 => TimeOfDay::Night,
            _ => TimeOfDay::Unknown,
        },
        level: time_level & 0x3F,
        ot_gender: if gender_location & 0x80 == 0 {
            Gender::Male
        } else {
            Gender::Female
        },
        location: gender_location & 0x7F,
    }
}
