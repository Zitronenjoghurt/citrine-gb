use crate::game::Game;

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
    pub item: u8,
    pub count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bag {
    pub items: Vec<ItemStack>,
    pub key_items: Vec<ItemStack>,
    pub balls: Vec<ItemStack>,
    pub tms_hms: Vec<ItemStack>,
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
    pub species: u8,
    pub nickname: String,
    pub ot_name: String,
    pub ot_id: u16,
    pub level: u8,
    pub exp: u32,
    pub moves: Vec<MoveSlot>,
    pub held_item: Option<u8>,
    pub dvs: Dvs,
    pub stat_exp: [u16; 5],
    pub hp: Option<u16>,
    pub stats: Option<Stats>,
    pub status: Option<u8>,
    pub friendship: Option<u8>,
    pub pokerus: Option<u8>,
    pub caught: Option<Caught>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveSlot {
    pub id: u8,
    pub pp: u8,
    pub pp_ups: u8,
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
        let byte = self.pokerus.filter(|&byte| byte != 0)?;
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

impl GameState {
    pub fn badge_count(&self) -> usize {
        self.trainer.badges.iter().filter(|b| **b).count()
    }
}
