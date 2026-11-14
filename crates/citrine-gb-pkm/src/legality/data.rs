use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::tables::{Encounter, EncounterMethod, Evolution, Game, Tables};

pub(super) const GREAT_MARSH: u16 = 52;
pub(super) const SAFARI_ZONE: u16 = 202;
pub(super) const NATIONAL_PARK: u16 = 207;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rod {
    Old,
    Good,
    Super,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Grass,
    Surf,
    Rod(Rod),
    RockSmash,
    SafariGrass,
    SafariSurf,
    SafariRod(Rod),
    BugContest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Slot {
    pub kind: Kind,
    pub block: bool,
    pub encounter: Encounter,
    pub tiles: u16,
}

impl Slot {
    pub fn allows_tile(&self, tile: u8) -> bool {
        tile < 16 && self.tiles >> tile & 1 == 1
    }

    pub fn tile(&self) -> Option<u8> {
        (self.tiles.count_ones() == 1).then(|| self.tiles.trailing_zeros() as u8)
    }
}

pub(crate) struct GameData {
    pub game: Game,
    pub tables: Tables,
    pub slots: HashMap<u16, Vec<Slot>>,
    pub hatch_places: HashSet<u16>,
    pub evolves_from: HashMap<u16, Vec<(u16, Evolution)>>,
}

macro_rules! game_files {
    ($dir:literal) => {
        game_files!($dir, $dir)
    };
    ($encounters:literal, $dir:literal) => {
        |name: &str| -> Option<&'static str> {
            Some(match name {
                "base_stats.tsv" => include_str!(concat!("../../data/", $dir, "/base_stats.tsv")),
                "learnsets.tsv" => include_str!(concat!("../../data/", $dir, "/learnsets.tsv")),
                "evolutions.tsv" => include_str!(concat!("../../data/", $dir, "/evolutions.tsv")),
                "encounters.tsv" => {
                    include_str!(concat!("../../data/", $encounters, "/encounters.tsv"))
                }
                "places.tsv" => include_str!(concat!("../../data/", $dir, "/places.tsv")),
                "growth.tsv" => include_str!(concat!("../../data/", $dir, "/growth.tsv")),
                "moves.tsv" => include_str!(concat!("../../data/", $dir, "/moves.tsv")),
                _ => return None,
            })
        }
    };
}

impl GameData {
    pub(crate) fn for_version(version: u8) -> Option<&'static Self> {
        static PLATINUM: OnceLock<GameData> = OnceLock::new();
        static HEART_GOLD: OnceLock<GameData> = OnceLock::new();
        static SOUL_SILVER: OnceLock<GameData> = OnceLock::new();
        match version {
            7 => Some(HEART_GOLD.get_or_init(|| {
                Self::new(Game::HeartGold, game_files!("heartgold", "soulsilver"))
            })),
            12 => Some(PLATINUM.get_or_init(|| Self::new(Game::Platinum, game_files!("platinum")))),
            8 => Some(
                SOUL_SILVER.get_or_init(|| Self::new(Game::SoulSilver, game_files!("soulsilver"))),
            ),
            _ => None,
        }
    }

    fn new(game: Game, files: impl Fn(&str) -> Option<&'static str>) -> Self {
        let tables = Tables::from_tsv(game, files).expect("the committed tables parse");
        let mut slots: HashMap<u16, Vec<Slot>> = HashMap::new();
        for e in &tables.encounters {
            let Some((kind, block)) = kind_of(game, e.method) else {
                continue;
            };
            let water = matches!(kind, Kind::SafariSurf | Kind::SafariRod(_));
            if water && SAFARI_DRY_AREAS.contains(&e.place) {
                continue;
            }
            let fixed = match kind {
                Kind::SafariGrass | Kind::SafariSurf | Kind::SafariRod(_) => Some(SAFARI_ZONE),
                Kind::BugContest => Some(NATIONAL_PARK),
                _ => None,
            };
            let homes: Vec<(u16, u8, u8)> = match fixed {
                Some(location) => vec![(location, 0, 0)],
                None => tables
                    .places
                    .iter()
                    .filter(|p| p.encounters == Some(e.place))
                    .filter(|p| !(is_hgss(game) && p.location == SAFARI_ZONE))
                    .filter(|p| {
                        !(kind == Kind::Surf && is_hgss(game) && p.location == HGSS_NO_SURF)
                    })
                    .map(|p| (p.location, p.area_icon, p.battle_background))
                    .collect(),
            };
            for (location, icon, background) in homes {
                let slot = Slot {
                    kind,
                    block,
                    encounter: *e,
                    tiles: tiles_of(game, kind, location, icon, background),
                };
                let list = slots.entry(location).or_default();
                if !list.contains(&slot) {
                    list.push(slot);
                }
            }
        }
        let hatch_range = if is_hgss(game) {
            FIRST_JOHTO_LOCATION..=LAST_JOHTO_LOCATION
        } else {
            1..=LAST_SINNOH_LOCATION
        };
        let hatch_places = tables
            .places
            .iter()
            .map(|p| p.location)
            .filter(|l| hatch_range.contains(l) && !(!is_hgss(game) && *l == HALL_OF_ORIGIN))
            .collect();
        let mut evolves_from: HashMap<u16, Vec<(u16, Evolution)>> = HashMap::new();
        for (species, evos) in tables.evolutions.iter().enumerate() {
            for e in evos {
                evolves_from
                    .entry(e.into)
                    .or_default()
                    .push((species as u16, *e));
            }
        }
        Self {
            game,
            tables,
            slots,
            hatch_places,
            evolves_from,
        }
    }
}

pub(crate) fn is_hgss(game: Game) -> bool {
    matches!(game, Game::HeartGold | Game::SoulSilver)
}

fn kind_of(game: Game, method: EncounterMethod) -> Option<(Kind, bool)> {
    use EncounterMethod::*;
    Some(match method {
        Grass | GrassMorning | GrassDay | GrassNight | Radar | DualSlot(_) | HoennSound
        | SinnohSound | GreatMarsh | GreatMarshNational | TrophyGarden => (Kind::Grass, false),
        Swarm if !is_hgss(game) => (Kind::Grass, false),
        Surf => (Kind::Surf, false),
        OldRod => (Kind::Rod(Rod::Old), false),
        GoodRod | NightGoodRod => (Kind::Rod(Rod::Good), false),
        SuperRod | NightSuperRod => (Kind::Rod(Rod::Super), false),
        RockSmash => (Kind::RockSmash, false),
        SafariGrass => (Kind::SafariGrass, false),
        SafariSurf => (Kind::SafariSurf, false),
        SafariOldRod => (Kind::SafariRod(Rod::Old), false),
        SafariGoodRod => (Kind::SafariRod(Rod::Good), false),
        SafariSuperRod => (Kind::SafariRod(Rod::Super), false),
        SafariGrassBlock => (Kind::SafariGrass, true),
        SafariSurfBlock => (Kind::SafariSurf, true),
        SafariOldRodBlock => (Kind::SafariRod(Rod::Old), true),
        SafariGoodRodBlock => (Kind::SafariRod(Rod::Good), true),
        SafariSuperRodBlock => (Kind::SafariRod(Rod::Super), true),
        BugContest => (Kind::BugContest, false),
        _ => return None,
    })
}

pub(crate) mod tile {
    pub const NONE: u8 = 0;
    pub const GRASS: u8 = 2;
    pub const ROCK: u8 = 4;
    pub const CAVE: u8 = 5;
    pub const WATER: u8 = 7;
    pub const BUILDING: u8 = 9;
    pub const MARSH: u8 = 10;
}

const SAFARI_DRY_AREAS: [u16; 7] = [0, 2, 3, 6, 9, 10, 11];
const HGSS_NO_SURF: u16 = 193;
const HALL_OF_ORIGIN: u16 = 86;
const LAST_SINNOH_LOCATION: u16 = 125;
const FIRST_JOHTO_LOCATION: u16 = 126;
const LAST_JOHTO_LOCATION: u16 = 234;
const PLATINUM_BUILDING_BACKGROUND: u8 = 8;
const MIXED_BACKGROUND: u8 = 3;
const MIXED_ICON: u8 = 5;

fn bit(tile: u8) -> u16 {
    1 << tile
}

fn tiles_of(game: Game, kind: Kind, location: u16, icon: u8, background: u8) -> u16 {
    match kind {
        Kind::Surf | Kind::Rod(_) | Kind::SafariSurf | Kind::SafariRod(_) => bit(tile::WATER),
        Kind::SafariGrass | Kind::BugContest => bit(tile::GRASS),
        Kind::RockSmash => {
            bit(tile::NONE) | bit(tile::ROCK) | bit(tile::CAVE) | bit(tile::BUILDING)
        }
        Kind::Grass if is_hgss(game) => match background {
            7..=11 => bit(tile::CAVE),
            MIXED_BACKGROUND if icon == MIXED_ICON => bit(tile::GRASS) | bit(tile::CAVE),
            _ => bit(tile::GRASS),
        },
        Kind::Grass if location == GREAT_MARSH => bit(tile::MARSH),
        Kind::Grass => match background {
            PLATINUM_BUILDING_BACKGROUND => bit(tile::BUILDING),
            9..=11 => bit(tile::CAVE),
            _ => bit(tile::GRASS),
        },
    }
}
