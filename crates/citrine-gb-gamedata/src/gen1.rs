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

#[cfg(test)]
mod tests {
    use crate::fixtures::{TestMon, game, name, put, put_list};
    use crate::gen1::checksum;
    use crate::test_support::{
        empty_banks, entry, international_save, mon, red_rom, seal_international,
    };
    use crate::{
        Genes, Language, Pokemon, ReadError, RomTables, SaveCopy, Version, read_live, read_save,
    };

    #[test]
    fn reads_an_international_save() {
        let game = game(Version::Red, Language::English);
        let state = read_save(game, &international_save()).expect("save");
        assert_eq!(state.copy, SaveCopy::Main);
        let trainer = &state.trainer;
        assert_eq!(trainer.name, "RED");
        assert_eq!(trainer.id, 12345);
        assert_eq!(trainer.money, 12345);
        assert_eq!(trainer.coins, 999);
        assert_eq!(
            (
                trainer.play_time.hours,
                trainer.play_time.minutes,
                trainer.play_time.seconds
            ),
            (12, 34, 56)
        );
        assert_eq!(
            trainer.badges,
            vec![true, false, true, false, false, false, false, false]
        );
        assert_eq!(state.badge_count(), 2);
        assert_eq!(state.pokedex.caught, vec![1, 25]);
        assert_eq!(state.pokedex.seen, vec![1, 4, 25, 151]);
        assert_eq!(state.bag.items.len(), 2);
        assert_eq!(
            (state.bag.items[1].item, state.bag.items[1].count),
            (0x14, 3)
        );
        assert_eq!(state.pc_items.len(), 1);

        assert_eq!(state.party.len(), 2);
        let Pokemon::Hatched(first) = &state.party[0] else {
            panic!("egg in gen 1");
        };
        assert_eq!(first.species, 0x99);
        assert_eq!(first.nickname, "BULB");
        assert_eq!(first.ot_name, "RED");
        assert_eq!(first.ot_id, 12345);
        assert_eq!(first.level, 5);
        assert_eq!(first.exp, 300);
        assert_eq!(first.hp, Some(35));
        let Genes::Gb { dvs, stat_exp } = first.genes else {
            panic!("gb genes");
        };
        assert_eq!(stat_exp[0], 500);
        assert_eq!(dvs.attack, 0xA);
        assert_eq!(dvs.defense, 0x5);
        assert_eq!(dvs.hp(), 0b0110);
        assert_eq!(first.moves.len(), 2);
        assert_eq!(
            (first.moves[0].id, first.moves[0].pp, first.moves[0].pp_ups),
            (0x21, 35, 0)
        );
        assert_eq!((first.moves[1].pp, first.moves[1].pp_ups), (5, 3));
        let stats = first.stats.expect("party stats");
        assert_eq!(
            (stats.max_hp, stats.special_attack, stats.special_defense),
            (40, 24, 24)
        );

        assert_eq!(state.current_box, 2);
        assert_eq!(state.boxes.len(), 12);
        let counts: Vec<usize> = state.boxes.iter().map(|b| b.pokemon.len()).collect();
        assert_eq!(counts, vec![1, 0, 1, 0, 0, 0, 0, 2, 0, 0, 0, 0]);
        let Pokemon::Hatched(bird) = &state.boxes[2].pokemon[0] else {
            panic!("egg");
        };
        assert_eq!(
            (bird.level, bird.stats, bird.nickname.as_str()),
            (3, None, "BIRD")
        );
    }

    #[test]
    fn a_wrong_checksum_is_refused() {
        let game = game(Version::Blue, Language::English);
        let mut save = international_save();
        save[0x25F3] = 0x99;
        assert_eq!(read_save(game, &save), Err(ReadError::NoValidCopy));
        assert_eq!(read_save(game, &save[..0x4000]), Err(ReadError::TooShort));
        assert_eq!(
            read_save(game, &[0xFF; 0x8000]),
            Err(ReadError::NoValidCopy)
        );
    }

    #[test]
    fn boxes_stay_empty_until_the_player_first_changes_box() {
        let game = game(Version::Yellow, Language::English);
        let mut save = international_save();
        save[0x284C] = 2;
        save[0x4000] = 0x37;
        seal_international(&mut save);
        let state = read_save(game, &save).expect("save");
        let counts: Vec<usize> = state.boxes.iter().map(|b| b.pokemon.len()).collect();
        assert_eq!(counts, vec![0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn reads_a_japanese_save() {
        let game = game(Version::Green, Language::Japanese);
        let mut s = vec![0u8; 0x8000];
        put(&mut s, 0x2598, &name(&[0xD7, 0xDE, 0xD8], 6));
        put(&mut s, 0x25C4, &[1, 0x04, 9, 0xFF]);
        put(&mut s, 0x25EE, &[0x00, 0x30, 0x00]);
        s[0x25F8] = 0xFF;
        put(&mut s, 0x25FB, &[0x00, 0x07]);
        put(&mut s, 0x27DC, &[0, 0xFF]);
        s[0x2842] = 0x80 | 5;
        put(&mut s, 0x2CA0, &[3, 0, 4, 5, 0]);
        let jp = |species: u8, nickname: &[u8]| TestMon {
            listed: species,
            mon: mon(species, 20, true),
            ot: vec![0x80, 0xD8],
            nickname: nickname.to_vec(),
        };
        put_list(&mut s, 0x2ED5, 6, 6, &[jp(0x54, &[0x41, 0xE5, 0x9B])]);
        empty_banks(&mut s, 4, 0x566);
        let boxed: Vec<TestMon> = (0..30)
            .map(|_| TestMon {
                listed: 0x99,
                mon: mon(0x99, 4, false),
                ot: vec![0x80],
                nickname: vec![0x85],
            })
            .collect();
        put_list(&mut s, 0x302D, 30, 6, &boxed);
        put_list(&mut s, 0x6000 + 2 * 0x566, 30, 6, &boxed[..1]);
        s[0x3594] = checksum(&s[0x2598..0x3594]);

        let state = read_save(game, &s).expect("save");
        assert_eq!(state.trainer.name, "らんり");
        assert_eq!(state.trainer.money, 3000);
        assert_eq!(state.trainer.id, 7);
        assert_eq!(state.badge_count(), 8);
        assert_eq!(state.boxes.len(), 8);
        assert_eq!(state.boxes[5].pokemon.len(), 30);
        assert_eq!(state.boxes[6].pokemon.len(), 1);
        let Pokemon::Hatched(pika) = &state.party[0] else {
            panic!("egg");
        };
        assert_eq!(pika.ot_name, "アリ");
        assert_eq!(pika.nickname, "ピ゛フ");
    }

    #[test]
    fn live_state_comes_from_work_ram_and_other_boxes_from_the_save() {
        let game = game(Version::Red, Language::English);
        let tables = RomTables::load(game, &red_rom()).expect("tables");
        assert_eq!(
            tables.live_map(),
            vec![
                (0xD158, 0x2598),
                (0xD2F7, 0x25A3),
                (0xD163, 0x2F2C),
                (0xDA80, 0x30C0)
            ]
        );
        let saved = international_save();
        let mut now = saved.clone();
        put(&mut now, 0x25F3, &[0x09, 0x87, 0x65]);
        put_list(
            &mut now,
            0x2F2C,
            6,
            11,
            &[
                entry(0x99, 6, true, "BULB"),
                entry(0x54, 7, true, "PIKA"),
                entry(0x24, 3, true, "NEW"),
            ],
        );
        seal_international(&mut now);
        let mut wram = vec![0u8; 0x2000];
        for (addr, at, len) in [
            (0xD158, 0x2598, 0x0B),
            (0xD2F7, 0x25A3, 0x789),
            (0xD163, 0x2F2C, 0x194),
            (0xDA80, 0x30C0, 0x462),
        ] {
            put(&mut wram, addr - 0xC000, &now[at..at + len]);
        }
        let live = read_live(&tables, &wram, &saved).expect("live");
        let mut expected = read_save(game, &now).expect("save");
        expected.copy = SaveCopy::Live;
        assert_eq!(live, expected);
        assert_eq!(live.trainer.money, 98765);
        assert_eq!(live.party.len(), 3);

        assert_eq!(
            read_live(&tables, &[0u8; 0x2000], &saved),
            Err(ReadError::NotRunning)
        );
    }
}
