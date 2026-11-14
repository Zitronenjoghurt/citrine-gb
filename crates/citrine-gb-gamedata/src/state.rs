use crate::battle::{Battle, Clock};
use crate::game::Game;
use crate::gen3::{Nature, Spread, Whereabouts};
pub use citrine_gb_pkm::legality::Reason;

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
    Ds(DsSpot),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DsSpot {
    pub map: u16,
    pub x: i32,
    pub z: i32,
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
    Ds(DsGenes),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DsGenes {
    pub genes: Gen3Genes,
    pub ability: u16,
    pub form: u8,
    pub legality: Legality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Legality {
    Passes,
    Breaks(Reason),
    NotChecked(Reason),
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
    pub location: u16,
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
            Genes::Gba(_) | Genes::Ds(_) => None,
        }
    }

    pub fn gen3(&self) -> Option<&Gen3Genes> {
        match &self.genes {
            Genes::Gb { .. } => None,
            Genes::Gba(genes) | Genes::Ds(DsGenes { genes, .. }) => Some(genes),
        }
    }

    pub fn ds(&self) -> Option<&DsGenes> {
        match &self.genes {
            Genes::Ds(genes) => Some(genes),
            Genes::Gb { .. } | Genes::Gba(_) => None,
        }
    }

    pub fn shiny(&self) -> bool {
        match &self.genes {
            Genes::Gb { dvs, .. } => dvs.shiny(),
            Genes::Gba(genes) | Genes::Ds(DsGenes { genes, .. }) => {
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
            Position::Gba(_) | Position::Ds(_) => None,
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

#[cfg(test)]
mod tests {
    use crate::fixtures::{crystal_save, game, put, seal};
    use crate::layout::CRYSTAL_INTERNATIONAL;
    use crate::test_support::{international_save, seal_international};
    use crate::{Language, Location, Milestone, Position, Step, Version, read_save};

    #[test]
    fn a_gen1_save_says_where_the_player_is_and_what_is_done() {
        let mut save = international_save();
        put(&mut save, 0x260A, &[0x0C, 0, 0, 5, 7]);
        save[0x29F3 + 4] |= 1 << 5;
        seal_international(&mut save);
        let state = read_save(game(Version::Red, Language::English), &save).expect("save");
        assert_eq!(
            state.location,
            Some(Position::Gb(Location {
                group: 0,
                map: 0x0C,
                x: 7,
                y: 5,
                outside: None
            }))
        );
        assert_eq!(state.hall_of_fame, Some(0));
        let done: Vec<bool> = state.journey.iter().map(|s| s.done).collect();
        assert_eq!(
            done,
            [
                true, true, false, true, false, false, false, false, false, false
            ]
        );
        assert_eq!(state.next_goal(), Some(Milestone::Badge(1)));

        save[0x284E] = 2;
        save[0x2602] = 0xFF;
        seal_international(&mut save);
        let state = read_save(game(Version::Red, Language::English), &save).expect("save");
        assert_eq!(state.hall_of_fame, Some(2));
        assert_eq!(state.next_goal(), None);
        assert_eq!(
            state.journey.last(),
            Some(&Step {
                milestone: Milestone::EliteFour,
                done: true
            })
        );
    }

    #[test]
    fn a_gen2_save_says_where_the_player_is_and_what_is_done() {
        let layout = &CRYSTAL_INTERNATIONAL;
        let mut save = crystal_save();
        put(&mut save, 0x2843, &[24, 2, 7, 9]);
        put(&mut save, 0x283B, &[24, 1]);
        save[0x23DA] |= 1;
        seal(layout, &mut save);
        let state = read_save(game(Version::Crystal, Language::English), &save).expect("save");
        assert_eq!(
            state.location,
            Some(Position::Gb(Location {
                group: 24,
                map: 2,
                x: 9,
                y: 7,
                outside: Some((24, 1))
            }))
        );
        let milestones: Vec<Milestone> = state.journey.iter().map(|s| s.milestone).collect();
        assert_eq!(milestones.len(), 10);
        assert_eq!(milestones[0], Milestone::Pokedex);
        assert_eq!(milestones[9], Milestone::EliteFour);
        assert_eq!(state.next_goal(), Some(Milestone::Badge(4)));

        save[layout.johto_badges] = 0xFF;
        save[0x2600 + 8] |= 1 << 4;
        save[layout.visited_spawns] |= 0x80;
        seal(layout, &mut save);
        let state = read_save(game(Version::Crystal, Language::English), &save).expect("save");
        assert_eq!(state.journey.len(), 18);
        assert_eq!(state.next_goal(), Some(Milestone::Badge(8)));
    }
}
