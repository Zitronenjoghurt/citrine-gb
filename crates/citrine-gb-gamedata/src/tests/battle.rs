use super::gen1::red_rom;
use super::gen2::crystal_rom;
use super::*;
use crate::battle::{read_battle, read_clock};
use crate::{Battle, BattleKind, Clock, Fighter, MoveSlot, RomTables, TimeOfDay};

fn wram() -> Vec<u8> {
    vec![0u8; 0x2000]
}

fn poke(wram: &mut [u8], addr: u16, bytes: &[u8]) {
    put(wram, usize::from(addr - 0xC000), bytes);
}

fn crystal() -> RomTables {
    RomTables::load(game(Version::Crystal, Language::English), &crystal_rom()).expect("tables")
}

fn red() -> RomTables {
    RomTables::load(game(Version::Red, Language::English), &red_rom()).expect("tables")
}

fn gen2_fighter(wram: &mut [u8], base: u16, species: u8, level: u8, hp: u16, max_hp: u16) {
    poke(wram, base, &[species]);
    poke(wram, base + 2, &[33, 45, 0, 0]);
    poke(wram, base + 8, &[0b0100_0011, 40, 0, 0]);
    poke(wram, base + 13, &[level, 0]);
    poke(wram, base + 16, &hp.to_be_bytes());
    poke(wram, base + 18, &max_hp.to_be_bytes());
    poke(wram, base + 30, &[0x14, 0x02]);
}

#[test]
fn reads_a_crystal_trainer_battle() {
    let tables = crystal();
    let mut wram = wram();
    poke(&mut wram, 0xD22D, &[2]);
    gen2_fighter(&mut wram, 0xD206, 152, 12, 30, 41);
    gen2_fighter(&mut wram, 0xC62C, 155, 14, 44, 44);
    poke(&mut wram, 0xD280, &[3]);
    poke(&mut wram, 0xD288 + 34, &[0, 30]);
    poke(&mut wram, 0xD288 + 48 + 34, &[0, 0]);
    poke(&mut wram, 0xD288 + 96 + 34, &[0, 12]);

    let battle = read_battle(&tables, &wram).expect("battle");
    let fighter = |species, level, hp, max_hp| Fighter {
        species,
        level,
        hp,
        max_hp,
        status: 0,
        types: [0x14, 0x02],
        moves: vec![
            MoveSlot {
                id: 33,
                pp: 3,
                pp_ups: 1,
            },
            MoveSlot {
                id: 45,
                pp: 40,
                pp_ups: 0,
            },
        ],
    };
    assert_eq!(
        battle,
        Battle {
            kind: BattleKind::Trainer,
            enemy: fighter(152, 12, 30, 41),
            player: fighter(155, 14, 44, 44),
            trainer_party: vec![true, false, true],
        }
    );
}

#[test]
fn no_battle_outside_one_or_with_implausible_values() {
    let tables = crystal();
    let mut wram = wram();
    gen2_fighter(&mut wram, 0xD206, 152, 12, 30, 41);
    gen2_fighter(&mut wram, 0xC62C, 155, 14, 44, 44);
    assert_eq!(read_battle(&tables, &wram), None);

    poke(&mut wram, 0xD22D, &[1]);
    assert_eq!(
        read_battle(&tables, &wram).map(|battle| battle.kind),
        Some(BattleKind::Wild)
    );
    assert!(
        read_battle(&tables, &wram)
            .unwrap()
            .trainer_party
            .is_empty()
    );

    gen2_fighter(&mut wram, 0xD206, 152, 0, 30, 41);
    assert_eq!(read_battle(&tables, &wram), None);
    gen2_fighter(&mut wram, 0xD206, 152, 12, 50, 41);
    assert_eq!(read_battle(&tables, &wram), None);
    gen2_fighter(&mut wram, 0xD206, 0, 12, 30, 41);
    assert_eq!(read_battle(&tables, &wram), None);
}

#[test]
fn reads_a_red_wild_battle() {
    let tables = red();
    let species = (1..=255u8)
        .find(|s| tables.species_name(u16::from(*s)).is_some())
        .expect("a species");
    let mut wram = wram();
    poke(&mut wram, 0xD057, &[1]);
    for base in [0xCFE5u16, 0xD014] {
        poke(&mut wram, base, &[species, 0, 20]);
        poke(&mut wram, base + 4, &[0x08, 0x15, 0x16]);
        poke(&mut wram, base + 8, &[10, 0, 0, 0]);
        poke(&mut wram, base + 14, &[7, 0, 25]);
        poke(&mut wram, base + 25, &[35, 0, 0, 0]);
    }
    let battle = read_battle(&tables, &wram).expect("battle");
    assert_eq!(battle.kind, BattleKind::Wild);
    assert_eq!(
        battle.enemy,
        Fighter {
            species: u16::from(species),
            level: 7,
            hp: 20,
            max_hp: 25,
            status: 0x08,
            types: [0x15, 0x16],
            moves: vec![MoveSlot {
                id: 10,
                pp: 35,
                pp_ups: 0,
            }],
        }
    );
    assert_eq!(read_clock(&tables, &wram), None);
}

#[test]
fn japanese_games_read_no_battle() {
    let tables = RomTables::load(game(Version::Crystal, Language::Japanese), &crystal_rom()).ok();
    let mut wram = wram();
    poke(&mut wram, 0xD22D, &[1]);
    gen2_fighter(&mut wram, 0xD206, 152, 12, 30, 41);
    gen2_fighter(&mut wram, 0xC62C, 155, 14, 44, 44);
    if let Some(tables) = tables {
        assert_eq!(read_battle(&tables, &wram), None);
        assert_eq!(read_clock(&tables, &wram), None);
    }
}

#[test]
fn reads_the_crystal_clock() {
    let tables = crystal();
    let mut wram = wram();
    poke(&mut wram, 0xD269, &[2]);
    poke(&mut wram, 0xD4CB, &[3]);
    assert_eq!(
        read_clock(&tables, &wram),
        Some(Clock {
            time: TimeOfDay::Night,
            weekday: 3,
        })
    );
    poke(&mut wram, 0xD4CB, &[7]);
    assert_eq!(read_clock(&tables, &wram), None);
    poke(&mut wram, 0xD4CB, &[0]);
    poke(&mut wram, 0xD269, &[4]);
    assert_eq!(read_clock(&tables, &wram), None);
}
