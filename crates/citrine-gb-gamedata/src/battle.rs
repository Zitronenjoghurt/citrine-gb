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
        | Version::LeafGreen => return None,
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
