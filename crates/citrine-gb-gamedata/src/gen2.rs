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
        location: u16::from(gender_location & 0x7F),
    }
}

#[cfg(test)]
mod tests {
    use crate::fixtures::{crystal_rom, en, game, put, put_list, save, seal};
    use crate::layout::{
        CRYSTAL_INTERNATIONAL, CRYSTAL_JAPANESE, GS_INTERNATIONAL, GS_JAPANESE, GS_KOREAN,
    };
    use crate::{
        Gender, Language, Pokemon, ReadError, RomTables, SaveCopy, TimeOfDay, Version, read_live,
        read_save,
    };

    #[test]
    fn reads_a_gold_and_silver_save() {
        let game = game(Version::Gold, Language::English);
        let state = read_save(game, &save(&GS_INTERNATIONAL, &en("GOLD"), 11)).expect("save");
        assert_eq!(state.copy, SaveCopy::Main);
        let trainer = &state.trainer;
        assert_eq!(trainer.name, "GOLD");
        assert_eq!(trainer.id, 258);
        assert_eq!(trainer.gender, None);
        assert_eq!(trainer.money, 100_000);
        assert_eq!(trainer.mom_money, Some(1000));
        assert_eq!(trainer.coins, 500);
        assert_eq!(
            (
                trainer.play_time.hours,
                trainer.play_time.minutes,
                trainer.play_time.seconds
            ),
            (300, 30, 15)
        );
        assert_eq!(trainer.badges.len(), 8);
        assert_eq!(state.badge_count(), 4);
        let items: Vec<(u16, u16)> = state
            .bag
            .tms_hms
            .iter()
            .map(|s| (s.item, s.count))
            .collect();
        assert_eq!(items, vec![(0xBF, 1), (0xC4, 2), (0xF3, 1)]);
        assert_eq!(state.bag.items.len(), 1);
        assert_eq!(state.bag.key_items.len(), 2);
        assert_eq!(state.bag.balls[0].count, 10);
        assert!(state.pc_items.is_empty());
        assert_eq!(state.pokedex.caught, vec![152]);
        assert_eq!(state.pokedex.seen, vec![152, 155]);

        assert_eq!(state.party.len(), 2);
        assert_eq!(state.party[1], Pokemon::Egg);
        let Pokemon::Hatched(first) = &state.party[0] else {
            panic!("egg first");
        };
        assert_eq!(first.species, 0x9B);
        assert_eq!(first.held_item, Some(0x12));
        assert_eq!(first.ot_id, 54321);
        assert_eq!(first.exp, 4096);
        assert_eq!(first.level, 12);
        assert_eq!(first.hp, Some(30));
        assert_eq!(first.friendship, Some(70));
        assert_eq!(first.pokerus, Some(0x21));
        assert_eq!(first.caught, None);
        assert!(first.shiny());
        assert_eq!(
            (first.moves.len(), first.moves[0].pp, first.moves[0].pp_ups),
            (1, 30, 1)
        );
        let stats = first.stats.expect("stats");
        assert_eq!(
            (stats.max_hp, stats.special_attack, stats.special_defense),
            (31, 15, 16)
        );

        assert_eq!(state.boxes.len(), 14);
        assert_eq!(state.current_box, 1);
        assert_eq!(state.boxes[13].name.as_deref(), Some("BOX14"));
        let counts: Vec<usize> = state.boxes.iter().map(|b| b.pokemon.len()).collect();
        assert_eq!(counts, vec![1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let Pokemon::Hatched(boxed) = &state.boxes[0].pokemon[0] else {
            panic!("egg");
        };
        assert_eq!((boxed.hp, boxed.stats, boxed.status), (None, None, None));
    }

    #[test]
    fn a_broken_main_copy_falls_back_to_the_backup() {
        let game = game(Version::Silver, Language::English);
        let mut s = save(&GS_INTERNATIONAL, &en("SILVER"), 11);
        s[0x23DB] = 0x00;
        let state = read_save(game, &s).expect("backup");
        assert_eq!(state.copy, SaveCopy::Backup);
        assert_eq!(state.trainer.money, 100_000);
        assert_eq!(state.trainer.name, "SILVER");
        assert_eq!(state.party.len(), 2);

        s[0x15C7 + 2] ^= 0xFF;
        assert_eq!(read_save(game, &s), Err(ReadError::NoValidCopy));

        let mut s = save(&GS_INTERNATIONAL, &en("SILVER"), 11);
        s[0x2D6B] = 0;
        assert_eq!(read_save(game, &s).expect("backup").copy, SaveCopy::Backup);
    }

    fn badges_shown(johto: u8, kanto: u8, spawns: [u8; 4]) -> (usize, usize) {
        let game = game(Version::Gold, Language::English);
        let mut s = save(&GS_INTERNATIONAL, &en("GOLD"), 11);
        s[0x23E4] = johto;
        s[0x23E5] = kanto;
        put(&mut s, 0x2856, &spawns);
        seal(&GS_INTERNATIONAL, &mut s);
        let state = read_save(game, &s).expect("save");
        (state.trainer.badges.len(), state.badge_count())
    }

    #[test]
    fn kanto_badges_stay_hidden_until_the_player_reaches_kanto() {
        let johto_towns = [0x00, 0xC0, 0xFF, 0x03];
        assert_eq!(badges_shown(0xFF, 0x00, johto_towns), (8, 8));
        assert_eq!(badges_shown(0xFF, 0x00, [0x00, 0xE0, 0xFF, 0x03]), (8, 8));
        assert_eq!(badges_shown(0xFF, 0x00, [0x80, 0xC0, 0xFF, 0x03]), (16, 8));
        assert_eq!(badges_shown(0x7F, 0x00, [0x80, 0xC0, 0xFF, 0x03]), (8, 7));
        assert_eq!(badges_shown(0xFF, 0x05, johto_towns), (16, 10));
    }

    #[test]
    fn reads_a_crystal_save_with_gender_and_caught_data() {
        let game = game(Version::Crystal, Language::English);
        let mut s = save(&CRYSTAL_INTERNATIONAL, &en("KRIS"), 11);
        let first = 0x2865 + 8;
        s[first + 29] = 0b1000_0101;
        s[first + 30] = 0x80 | 0x12;
        seal(&CRYSTAL_INTERNATIONAL, &mut s);
        let state = read_save(game, &s).expect("save");
        assert_eq!(state.trainer.name, "KRIS");
        assert_eq!(state.trainer.gender, Some(Gender::Female));
        let Pokemon::Hatched(mon) = &state.party[0] else {
            panic!("egg");
        };
        let caught = mon.caught.expect("caught");
        assert_eq!(caught.time_of_day, TimeOfDay::Day);
        assert_eq!(caught.level, 5);
        assert_eq!(caught.ot_gender, Gender::Female);
        assert_eq!(caught.location, 0x12);

        s[first + 29] = 0;
        s[first + 30] = 0x7E;
        seal(&CRYSTAL_INTERNATIONAL, &mut s);
        let state = read_save(game, &s).expect("gift");
        let Pokemon::Hatched(mon) = &state.party[0] else {
            panic!("egg");
        };
        let caught = mon.caught.expect("caught");
        assert_eq!((caught.time_of_day, caught.level), (TimeOfDay::Unknown, 0));
        assert_eq!(caught.location, 0x7E);

        s[0x2009] ^= 0xFF;
        let state = read_save(game, &s).expect("backup");
        assert_eq!((state.copy, state.trainer.id), (SaveCopy::Backup, 258));
    }

    #[test]
    fn crystal_tables_and_live_state() {
        let game = game(Version::Crystal, Language::English);
        let tables = RomTables::load(game, &crystal_rom()).expect("tables");
        assert_eq!(tables.species_name(1), Some("PAA"));
        assert_eq!(tables.species_name(251), Some("PJQ"));
        assert_eq!(tables.species_name(0), None);
        assert_eq!(tables.dex_number(25), Some(25));
        assert_eq!(tables.species_for_dex(251), Some(251));
        assert_eq!(tables.move_name(251), Some("MOVEJQ"));
        assert_eq!(tables.item_name(0xBF).as_deref(), Some("THINGHI"));
        assert_eq!(
            tables.live_map(),
            vec![
                (0xD47B, 0x2009),
                (0xDCA5, 0x2833),
                (0xDCD7, 0x2865),
                (0xD472, 0x3E3D)
            ]
        );

        let saved = save(&CRYSTAL_INTERNATIONAL, &en("KRIS"), 11);
        let mut now = saved.clone();
        put(&mut now, 0x23DC, &[0x00, 0x00, 0x2A]);
        put(
            &mut now,
            CRYSTAL_INTERNATIONAL.play_time,
            &[0x00, 0x3C, 0x29, 0x07],
        );
        now[CRYSTAL_INTERNATIONAL.pokedex_caught] |= 0b0000_0110;
        now[CRYSTAL_INTERNATIONAL.pokedex_seen] |= 0b0000_1110;
        now[0x3E3D] = 0;
        let mut wram = vec![0u8; 0x2000];
        for (addr, at, len) in [
            (0xD47B, 0x2009, 0x82A),
            (0xDCA5, 0x2833, 0x32),
            (0xDCD7, 0x2865, 0x31E),
            (0xD472, 0x3E3D, 7),
        ] {
            put(&mut wram, addr - 0xC000, &now[at..at + len]);
        }
        let mut live_sram = saved.clone();
        put_list(&mut live_sram, 0x2D10, 20, 11, &[]);
        let live = read_live(&tables, &wram, &live_sram).expect("live");
        assert_eq!(live.copy, SaveCopy::Live);
        assert_eq!(live.trainer.money, 42);
        assert_eq!(
            live.trainer.play_time,
            crate::PlayTime {
                hours: 60,
                minutes: 41,
                seconds: 7
            }
        );
        assert_eq!(live.pokedex.caught, vec![2, 3, 152]);
        assert_eq!(live.pokedex.seen, vec![2, 3, 4, 152, 155]);
        assert_eq!(live.trainer.gender, Some(Gender::Male));
        assert_eq!(live.boxes[1].pokemon.len(), 0);
        assert_eq!(live.boxes[0].pokemon.len(), 1);
        assert_eq!(
            read_save(game, &live_sram).expect("save").boxes[1]
                .pokemon
                .len(),
            2
        );
        assert_eq!(
            read_live(&tables, &[0u8; 0x2000], &live_sram),
            Err(ReadError::NotRunning)
        );
    }

    #[test]
    fn reads_japanese_gold_and_crystal_saves() {
        let hiragana = [0xB1, 0xB2, 0xB3];
        for (version, layout) in [
            (Version::Gold, &GS_JAPANESE),
            (Version::Crystal, &CRYSTAL_JAPANESE),
        ] {
            let game = game(version, Language::Japanese);
            let state = read_save(game, &save(layout, &hiragana, 6)).expect("save");
            assert_eq!(state.trainer.name, "あいう");
            assert_eq!(state.boxes.len(), 9);
            assert_eq!(state.trainer.money, 100_000);
            assert_eq!(state.party.len(), 2);
            assert_eq!(state.boxes[0].pokemon.len(), 1);
            assert_eq!(state.pokedex.seen, vec![152, 155]);
            assert_eq!(
                state.trainer.gender,
                (version == Version::Crystal).then_some(Gender::Female)
            );
        }
    }

    #[test]
    fn reads_a_korean_save() {
        let game = game(Version::Silver, Language::Korean);
        let hangul = [0x01, 0x01, 0x0A, 0xFE, 0x85];
        let state = read_save(game, &save(&GS_KOREAN, &hangul, 11)).expect("save");
        assert_eq!(state.trainer.name, "가힝F");
        assert_eq!(state.boxes.len(), 14);
        assert_eq!(state.boxes[0].name.as_deref(), Some("BOX1"));
        assert_eq!(state.pokedex.caught, vec![152]);

        let mut s = save(&GS_KOREAN, &hangul, 11);
        s[0x2009] ^= 1;
        assert_eq!(read_save(game, &s), Err(ReadError::NoValidCopy));
    }
}
