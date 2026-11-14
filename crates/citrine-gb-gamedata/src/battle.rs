use crate::game::{Game, Language, Version};
use crate::rom::RomTables;
use crate::state::{MoveSlot, TimeOfDay};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleKind {
    Wild,
    Trainer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fighter {
    pub species: u16,
    pub level: u8,
    pub hp: u16,
    pub max_hp: u16,
    pub status: u8,
    pub types: [u8; 2],
    pub moves: Vec<MoveSlot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Battle {
    pub kind: BattleKind,
    pub enemy: Fighter,
    pub player: Fighter,
    pub trainer_party: Vec<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub time: TimeOfDay,
    pub weekday: u8,
}

struct Addresses {
    anchor: u16,
    mode: u16,
    enemy: u16,
    player: u16,
    party_count: u16,
    party: u16,
    party_mon: usize,
    party_hp: usize,
    clock: Option<(u16, u16)>,
}

const RED: Addresses = Addresses {
    anchor: 0xD158,
    mode: 0xD057,
    enemy: 0xCFE5,
    player: 0xD014,
    party_count: 0xD89C,
    party: 0xD8A4,
    party_mon: 44,
    party_hp: 1,
    clock: None,
};

const YELLOW: Addresses = Addresses {
    anchor: 0xD157,
    mode: 0xD056,
    enemy: 0xCFE4,
    player: 0xD013,
    party_count: 0xD89B,
    party: 0xD8A3,
    party_mon: 44,
    party_hp: 1,
    clock: None,
};

const GOLD_SILVER: Addresses = Addresses {
    anchor: 0xD1A1,
    mode: 0xD116,
    enemy: 0xD0EF,
    player: 0xCB0C,
    party_count: 0xDD55,
    party: 0xDD5D,
    party_mon: 48,
    party_hp: 34,
    clock: Some((0xD157, 0xD1F2)),
};

const CRYSTAL: Addresses = Addresses {
    anchor: 0xD47B,
    mode: 0xD22D,
    enemy: 0xD206,
    player: 0xC62C,
    party_count: 0xD280,
    party: 0xD288,
    party_mon: 48,
    party_hp: 34,
    clock: Some((0xD269, 0xD4CB)),
};

fn addresses(tables: &RomTables) -> Option<&'static Addresses> {
    let game: Game = tables.game;
    if matches!(game.language, Language::Japanese | Language::Korean) {
        return None;
    }
    let addresses = match game.version {
        Version::Red | Version::Blue => &RED,
        Version::Yellow => &YELLOW,
        Version::Gold | Version::Silver => &GOLD_SILVER,
        Version::Crystal => &CRYSTAL,
        Version::Green
        | Version::Ruby
        | Version::Sapphire
        | Version::Emerald
        | Version::FireRed
        | Version::LeafGreen
        | Version::Diamond
        | Version::Pearl
        | Version::Platinum
        | Version::HeartGold
        | Version::SoulSilver
        | Version::Black
        | Version::White
        | Version::Black2
        | Version::White2 => return None,
    };
    (tables.live.first()?.wram == addresses.anchor).then_some(addresses)
}

struct Wram<'a>(&'a [u8]);

impl Wram<'_> {
    fn byte(&self, addr: u16) -> Option<u8> {
        self.0.get(usize::from(addr.checked_sub(0xC000)?)).copied()
    }

    fn word(&self, addr: u16) -> Option<u16> {
        Some(u16::from_be_bytes([self.byte(addr)?, self.byte(addr + 1)?]))
    }
}

pub(crate) fn read_battle(tables: &RomTables, wram: &[u8]) -> Option<Battle> {
    let at = addresses(tables)?;
    let wram = Wram(wram);
    let kind = match wram.byte(at.mode)? {
        1 => BattleKind::Wild,
        2 => BattleKind::Trainer,
        _ => return None,
    };
    let gen1 = at.party_mon == 44;
    let fighter = |base: u16| -> Option<Fighter> {
        let f = if gen1 {
            Fighter {
                species: u16::from(wram.byte(base)?),
                hp: wram.word(base + 1)?,
                status: wram.byte(base + 4)?,
                types: [wram.byte(base + 5)?, wram.byte(base + 6)?],
                level: wram.byte(base + 14)?,
                max_hp: wram.word(base + 15)?,
                moves: moves(&wram, base + 8, base + 25)?,
            }
        } else {
            Fighter {
                species: u16::from(wram.byte(base)?),
                level: wram.byte(base + 13)?,
                status: wram.byte(base + 14)?,
                hp: wram.word(base + 16)?,
                max_hp: wram.word(base + 18)?,
                types: [wram.byte(base + 30)?, wram.byte(base + 31)?],
                moves: moves(&wram, base + 2, base + 8)?,
            }
        };
        (tables.species_name(f.species).is_some()
            && (1..=100).contains(&f.level)
            && f.max_hp > 0
            && f.hp <= f.max_hp)
            .then_some(f)
    };
    let trainer_party = match kind {
        BattleKind::Wild => Vec::new(),
        BattleKind::Trainer => {
            let count = wram.byte(at.party_count)?.min(6);
            (0..u16::from(count))
                .map(|i| {
                    let hp = at.party + i * at.party_mon as u16 + at.party_hp as u16;
                    Some(wram.word(hp)? > 0)
                })
                .collect::<Option<Vec<bool>>>()?
        }
    };
    Some(Battle {
        kind,
        enemy: fighter(at.enemy)?,
        player: fighter(at.player)?,
        trainer_party,
    })
}

fn moves(wram: &Wram, ids: u16, pp: u16) -> Option<Vec<MoveSlot>> {
    let mut slots = Vec::new();
    for i in 0..4 {
        let id = wram.byte(ids + i)?;
        if id == 0 {
            continue;
        }
        let pp = wram.byte(pp + i)?;
        slots.push(MoveSlot {
            id: u16::from(id),
            pp: pp & 0x3F,
            pp_ups: pp >> 6,
        });
    }
    Some(slots)
}

pub(crate) fn read_clock(tables: &RomTables, wram: &[u8]) -> Option<Clock> {
    let (time, day) = addresses(tables)?.clock?;
    let wram = Wram(wram);
    let time = match wram.byte(time)? {
        0 => TimeOfDay::Morning,
        1 => TimeOfDay::Day,
        2 => TimeOfDay::Night,
        _ => return None,
    };
    let weekday = wram.byte(day)?;
    (weekday < 7).then_some(Clock { time, weekday })
}

#[cfg(test)]
mod tests {
    use crate::battle::{read_battle, read_clock};
    use crate::fixtures::crystal_rom;
    use crate::fixtures::{game, put};
    use crate::test_support::red_rom;
    use crate::{
        Battle, BattleKind, Clock, Fighter, Language, MoveSlot, RomTables, TimeOfDay, Version,
    };

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
        let tables =
            RomTables::load(game(Version::Crystal, Language::Japanese), &crystal_rom()).ok();
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
}
