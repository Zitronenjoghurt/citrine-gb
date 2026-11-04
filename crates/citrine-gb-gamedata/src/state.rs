use crate::battle::{Battle, Clock};
use crate::game::Game;
use crate::gen3::{Nature, Spread, Whereabouts};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameState {
    pub game: Game,
    pub trainer: Trainer,
    pub party: Vec<Pokemon>,
    pub boxes: Vec<PcBox>,
    pub current_box: usize,
    pub bag: Bag,
    pub pc_items: Vec<ItemStack>,
    pub pokedex: Pokedex,
    pub copy: SaveCopy,
    pub location: Option<Position>,
    pub journey: Vec<Step>,
    pub hall_of_fame: Option<u8>,
    pub battle: Option<Battle>,
    pub clock: Option<Clock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Gb(Location),
    Gba(Whereabouts),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub group: u8,
    pub map: u8,
    pub x: u8,
    pub y: u8,
    pub outside: Option<(u8, u8)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Milestone {
    Pokedex,
    Badge(usize),
    EliteFour,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub milestone: Milestone,
    pub done: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveCopy {
    Main,
    Backup,
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trainer {
    pub name: String,
    pub id: u16,
    pub gender: Option<Gender>,
    pub money: u32,
    pub mom_money: Option<u32>,
    pub coins: u16,
    pub play_time: PlayTime,
    pub badges: Vec<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    Male,
    Female,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayTime {
    pub hours: u16,
    pub minutes: u8,
    pub seconds: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemStack {
    pub item: u16,
    pub count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bag {
    pub items: Vec<ItemStack>,
    pub key_items: Vec<ItemStack>,
    pub balls: Vec<ItemStack>,
    pub tms_hms: Vec<ItemStack>,
    pub berries: Vec<ItemStack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pokedex {
    pub seen: Vec<u16>,
    pub caught: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcBox {
    pub number: usize,
    pub name: Option<String>,
    pub pokemon: Vec<Pokemon>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pokemon {
    Egg,
    Hatched(Box<Mon>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mon {
    pub species: u16,
    pub nickname: String,
    pub ot_name: String,
    pub ot_id: u16,
    pub level: u8,
    pub exp: u32,
    pub moves: Vec<MoveSlot>,
    pub held_item: Option<u16>,
    pub genes: Genes,
    pub hp: Option<u16>,
    pub stats: Option<Stats>,
    pub status: Option<u8>,
    pub friendship: Option<u8>,
    pub pokerus: Option<u8>,
    pub caught: Option<Caught>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveSlot {
    pub id: u16,
    pub pp: u8,
    pub pp_ups: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Genes {
    Gb { dvs: Dvs, stat_exp: [u16; 5] },
    Gba(Gen3Genes),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gen3Genes {
    pub personality: u32,
    pub ot_secret_id: u16,
    pub nature: Nature,
    pub ivs: Spread,
    pub evs: Spread,
    pub ability_slot: u8,
    pub ball: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dvs {
    pub attack: u8,
    pub defense: u8,
    pub speed: u8,
    pub special: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub max_hp: u16,
    pub attack: u16,
    pub defense: u16,
    pub speed: u16,
    pub special_attack: u16,
    pub special_defense: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caught {
    pub time_of_day: TimeOfDay,
    pub level: u8,
    pub ot_gender: Gender,
    pub location: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeOfDay {
    Morning,
    Day,
    Night,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pokerus {
    Infected { strain: u8, days: u8 },
    Cured { strain: u8 },
}

impl Mon {
    pub fn pokerus_status(&self) -> Option<Pokerus> {
        Pokerus::from_byte(self.pokerus?)
    }

    pub fn dvs(&self) -> Option<Dvs> {
        match self.genes {
            Genes::Gb { dvs, .. } => Some(dvs),
            Genes::Gba(_) => None,
        }
    }

    pub fn gen3(&self) -> Option<&Gen3Genes> {
        match &self.genes {
            Genes::Gb { .. } => None,
            Genes::Gba(genes) => Some(genes),
        }
    }

    pub fn shiny(&self) -> bool {
        match &self.genes {
            Genes::Gb { dvs, .. } => dvs.shiny(),
            Genes::Gba(genes) => {
                let p = genes.personality;
                (self.ot_id ^ genes.ot_secret_id ^ (p >> 16) as u16 ^ p as u16) < 8
            }
        }
    }
}

impl Pokerus {
    pub fn from_byte(byte: u8) -> Option<Self> {
        if byte == 0 {
            return None;
        }
        let (strain, days) = (byte >> 4, byte & 0x0F);
        Some(if days > 0 {
            Pokerus::Infected { strain, days }
        } else {
            Pokerus::Cured { strain }
        })
    }
}

impl Dvs {
    pub fn from_bytes(high: u8, low: u8) -> Self {
        Self {
            attack: high >> 4,
            defense: high & 0x0F,
            speed: low >> 4,
            special: low & 0x0F,
        }
    }

    pub fn hidden_power(self) -> (u8, u8) {
        let high = |v: u8| v >> 3 & 1;
        let x = (high(self.attack) << 3)
            | (high(self.defense) << 2)
            | (high(self.speed) << 1)
            | high(self.special);
        let power = (x * 5 + (self.special & 3)) / 2 + 31;
        let mut kind = ((self.attack & 3) << 2 | (self.defense & 3)) + 1;
        if kind >= 6 {
            kind += 1;
        }
        if kind >= 0x0A {
            kind += 0x0A;
        }
        (kind, power)
    }

    pub fn hp(self) -> u8 {
        ((self.attack & 1) << 3)
            | ((self.defense & 1) << 2)
            | ((self.speed & 1) << 1)
            | (self.special & 1)
    }

    pub fn shiny(self) -> bool {
        self.defense == 10
            && self.speed == 10
            && self.special == 10
            && matches!(self.attack, 2 | 3 | 6 | 7 | 10 | 11 | 14 | 15)
    }

    pub fn unown_letter(self) -> char {
        let bits = ((self.attack & 0x06) << 5)
            | ((self.defense & 0x06) << 3)
            | ((self.speed & 0x06) << 1)
            | ((self.special & 0x06) >> 1);
        (b'A' + bits / 10) as char
    }
}

impl Position {
    pub fn gb(&self) -> Option<&Location> {
        match self {
            Position::Gb(location) => Some(location),
            Position::Gba(_) => None,
        }
    }
}

impl GameState {
    pub fn badge_count(&self) -> usize {
        self.trainer.badges.iter().filter(|b| **b).count()
    }

    pub fn next_goal(&self) -> Option<Milestone> {
        self.journey
            .iter()
            .find(|step| !step.done)
            .map(|step| step.milestone)
    }
}
