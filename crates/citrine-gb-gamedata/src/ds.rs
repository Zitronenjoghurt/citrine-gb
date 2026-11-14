mod pics;

use crate::ReadError;
use crate::game::{Game, Language, Version};
use crate::gen3::{Growth, Nature, Spread};
use crate::moves::MoveData;
use crate::rom::Place;
use crate::state::{
    Bag, Caught, DsGenes, DsSpot, GameState, Gen3Genes, Gender, Genes, Legality, Milestone, Mon,
    MoveSlot, PcBox, PlayTime, Pokedex, Pokemon, Position, SaveCopy, Stats, Step, TimeOfDay,
    Trainer,
};
use crate::wild::{Encounter, Method, WildList};
use citrine_gb_pkm::Gen;
use citrine_gb_pkm::info::{self, Book, Live};
use citrine_gb_pkm::legality::{self, Reason};
use citrine_gb_pkm::pkm::{self, Extra};
use citrine_gb_pkm::tables::Game as DsGame;
use citrine_gb_pkm::tables::{Encounter as DsEncounter, EncounterMethod};
use std::collections::HashMap;

const GEN4_DEX: u16 = 493;
const VERDICTS_KEPT: usize = 1024;
const GEN5_DEX: u16 = 649;
const GEN5_FIRE: u8 = 9;
const GEN4_ODD_TYPE: u8 = 9;
const GRASS_PERCENT: [u8; 12] = [20, 20, 10, 10, 10, 10, 5, 5, 4, 4, 1, 1];
const WATER_PERCENT: [u8; 5] = [60, 30, 5, 4, 1];
const ROD_PERCENT: [u8; 5] = [40, 40, 15, 4, 1];
const HGSS_ROCK_PERCENT: [u8; 2] = [90, 10];
const PLATINUM_ORDER: [usize; 8] = [0, 1, 4, 2, 3, 5, 6, 7];
const GEN4_GIFTS: u16 = 2000;
const GEN4_EVENTS: u16 = 3000;
const GEN5_GIFTS: u16 = 30000;
const GEN5_EVENTS: u16 = 40000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DsTables {
    pub game: Game,
    book: Book,
    verdicts: Verdicts,
}

#[derive(Default)]
struct Verdicts(HashMap<pkm::Pokemon, Legality>);

impl Verdicts {
    fn judge(&mut self, pkm: &pkm::Pokemon) -> Legality {
        if let Some(legality) = self.0.get(pkm) {
            return *legality;
        }
        let legality = judge(pkm);
        if self.0.len() >= VERDICTS_KEPT {
            self.0.clear();
        }
        self.0.insert(pkm.clone(), legality);
        legality
    }
}

impl Clone for Verdicts {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for Verdicts {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Verdicts {}

impl std::fmt::Debug for Verdicts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Verdicts")
    }
}

impl DsTables {
    pub fn load(rom: &[u8]) -> Option<Self> {
        let book = Book::load(rom).ok()?;
        let game = Game {
            version: version(book.tables.game),
            language: language(book.tables.language),
            revision: 0,
        };
        Some(Self {
            game,
            book,
            verdicts: Verdicts::default(),
        })
    }

    fn generation(&self) -> Gen {
        self.book.tables.game.generation()
    }

    pub fn read_live(&mut self, main_ram: &[u8]) -> Result<GameState, ReadError> {
        let live = info::read(main_ram).ok_or(ReadError::NotRunning)?;
        if live.game != self.book.tables.game {
            return Err(ReadError::NotRunning);
        }
        let legality: Vec<Legality> = live
            .party
            .iter()
            .chain(live.boxes.iter().flat_map(|pc| &pc.pokemon))
            .map(|p| self.verdicts.judge(p))
            .collect();
        Ok(self.fold(&live, &legality))
    }

    fn fold(&self, live: &Live, legality: &[Legality]) -> GameState {
        let mut legality = legality.iter().copied();
        let mut pokemon = |list: &[pkm::Pokemon]| -> Vec<Pokemon> {
            list.iter()
                .map(|p| self.pokemon(p, legality.next().unwrap_or_else(|| judge(p))))
                .collect()
        };
        let trainer = &live.trainer;
        let journey = journey(self.game.version, &trainer.badges);
        GameState {
            game: self.game,
            trainer: Trainer {
                name: trainer.name.clone(),
                id: trainer.tid,
                gender: Some(if trainer.female {
                    Gender::Female
                } else {
                    Gender::Male
                }),
                money: trainer.money,
                mom_money: None,
                coins: 0,
                play_time: PlayTime {
                    hours: trainer.hours,
                    minutes: trainer.minutes,
                    seconds: trainer.seconds,
                },
                badges: trainer.badges.clone(),
            },
            party: pokemon(&live.party),
            boxes: live
                .boxes
                .iter()
                .enumerate()
                .map(|(i, pc)| PcBox {
                    number: i + 1,
                    name: Some(pc.name.clone()).filter(|name| !name.trim().is_empty()),
                    pokemon: pokemon(&pc.pokemon),
                })
                .collect(),
            current_box: live.current_box,
            bag: Bag::default(),
            pc_items: Vec::new(),
            pokedex: Pokedex {
                seen: live.pokedex.seen.clone(),
                caught: live.pokedex.caught.clone(),
            },
            copy: SaveCopy::Live,
            location: live.spot.map(|spot| {
                Position::Ds(DsSpot {
                    map: spot.map,
                    x: spot.x,
                    z: spot.z,
                })
            }),
            journey,
            hall_of_fame: None,
            battle: None,
            clock: None,
        }
    }

    fn pokemon(&self, pkm: &pkm::Pokemon, legality: Legality) -> Pokemon {
        if pkm.is_egg {
            return Pokemon::Egg;
        }
        Pokemon::Hatched(Box::new(self.mon(pkm, legality)))
    }

    fn mon(&self, pkm: &pkm::Pokemon, legality: Legality) -> Mon {
        let spread = |s: citrine_gb_pkm::Stats<u8>| Spread {
            hp: s.hp,
            attack: s.atk,
            defense: s.def,
            speed: s.spe,
            special_attack: s.spa,
            special_defense: s.spd,
        };
        let nature = match pkm.extra {
            Extra::Gen5 { nature, .. } => Nature::from_personality(u32::from(nature)),
            Extra::Gen4 { .. } => Nature::from_personality(pkm.pid),
        };
        let ball = match pkm.extra {
            Extra::Gen4 { hgss_ball, .. } if hgss_ball != 0 => hgss_ball,
            _ => pkm.ball,
        };
        let level = pkm
            .party
            .map(|party| party.level)
            .or_else(|| self.level_for(pkm.species, pkm.exp))
            .unwrap_or(1);
        Mon {
            species: pkm.species,
            nickname: pkm.nickname_text(),
            ot_name: pkm.ot_text(),
            ot_id: pkm.tid,
            level,
            exp: pkm.exp,
            moves: (0..4)
                .filter(|&i| pkm.moves[i] != 0)
                .map(|i| MoveSlot {
                    id: pkm.moves[i],
                    pp: pkm.pp[i],
                    pp_ups: pkm.pp_ups[i],
                })
                .collect(),
            held_item: (pkm.held_item != 0).then_some(pkm.held_item),
            genes: Genes::Ds(DsGenes {
                genes: Gen3Genes {
                    personality: pkm.pid,
                    ot_secret_id: pkm.sid,
                    nature,
                    ivs: spread(pkm.ivs),
                    evs: spread(pkm.evs),
                    ability_slot: (pkm.pid & 1) as u8,
                    ball,
                },
                ability: u16::from(pkm.ability),
                form: pkm.form,
                legality,
            }),
            hp: pkm.party.map(|party| party.hp),
            stats: pkm.party.map(|party| Stats {
                max_hp: party.stats.hp,
                attack: party.stats.atk,
                defense: party.stats.def,
                speed: party.stats.spe,
                special_attack: party.stats.spa,
                special_defense: party.stats.spd,
            }),
            status: pkm.party.map(|party| party.status as u8),
            friendship: Some(pkm.friendship),
            pokerus: Some(pkm.pokerus),
            caught: Some(Caught {
                time_of_day: TimeOfDay::Unknown,
                level: pkm.met_level,
                ot_gender: if pkm.ot_female {
                    Gender::Female
                } else {
                    Gender::Male
                },
                location: match pkm.extra {
                    Extra::Gen4 {
                        met_location_ext, ..
                    } if met_location_ext != 0 => met_location_ext,
                    _ => pkm.met_location,
                },
            }),
        }
    }

    fn level_for(&self, species: u16, exp: u32) -> Option<u8> {
        let growth = self.growth(species)?;
        (1..=100u8).rev().find(|&level| growth.exp(level) <= exp)
    }

    fn growth(&self, species: u16) -> Option<Growth> {
        let base = self.book.tables.base_stats.get(usize::from(species))?;
        Growth::from_byte(base.growth)
    }

    pub fn species_name(&self, species: u16) -> Option<&str> {
        self.book
            .tables
            .species_names
            .get(usize::from(species))
            .filter(|_| (1..=self.pokedex_size()).contains(&species))
            .map(String::as_str)
    }

    pub fn front_picture(
        &self,
        rom: &[u8],
        species: u16,
        female: bool,
        shiny: bool,
    ) -> Option<citrine_gb_nds_files::Picture> {
        self.species_name(species)?;
        pics::front(rom, self.book.tables.game, species, female, shiny)
    }

    pub fn pokedex_size(&self) -> u16 {
        match self.generation() {
            Gen::Four => GEN4_DEX,
            Gen::Five => GEN5_DEX,
        }
    }

    pub fn move_name(&self, id: u16) -> Option<&str> {
        named(&self.book.names.moves, id)
    }

    pub fn item_name(&self, id: u16) -> Option<&str> {
        named(&self.book.names.items, id)
    }

    pub fn ability_name(&self, id: u16) -> Option<&str> {
        named(&self.book.names.abilities, id)
    }

    pub fn place_name(&self, location: u16) -> Option<&str> {
        named(&self.book.names.places, location)
    }

    pub fn caught_place(&self, caught: &Caught) -> Option<Place<'_>> {
        let (gifts, events) = match self.generation() {
            Gen::Four => (GEN4_GIFTS, GEN4_EVENTS),
            Gen::Five => (GEN5_GIFTS, GEN5_EVENTS),
        };
        match caught.location {
            location if location >= events => Some(Place::Event),
            location if location >= gifts => Some(Place::Gift),
            location => self.place_name(location).map(Place::Landmark),
        }
    }

    pub fn spot_name(&self, spot: &DsSpot) -> Option<&str> {
        let place = self.book.tables.places.get(usize::from(spot.map))?;
        self.place_name(place.location)
    }

    pub fn wild(&self, spot: &DsSpot, time: Option<TimeOfDay>) -> Vec<WildList> {
        if self.generation() != Gen::Four {
            return Vec::new();
        }
        let Some(file) = self
            .book
            .tables
            .places
            .get(usize::from(spot.map))
            .and_then(|place| place.encounters)
        else {
            return Vec::new();
        };
        let here: Vec<&DsEncounter> = self
            .book
            .tables
            .encounters
            .iter()
            .filter(|encounter| encounter.place == file)
            .collect();
        let johto = matches!(
            self.book.tables.game,
            DsGame::HeartGold | DsGame::SoulSilver
        );
        let grass = match (johto, time) {
            (true, Some(TimeOfDay::Morning)) => vec![EncounterMethod::GrassMorning],
            (true, Some(TimeOfDay::Night)) => vec![EncounterMethod::GrassNight],
            (true, _) => vec![EncounterMethod::GrassDay],
            (false, Some(TimeOfDay::Night)) => {
                vec![EncounterMethod::Grass, EncounterMethod::GrassNight]
            }
            (false, Some(TimeOfDay::Day | TimeOfDay::Morning)) => {
                vec![EncounterMethod::Grass, EncounterMethod::GrassDay]
            }
            (false, _) => vec![EncounterMethod::Grass],
        };
        let rock: &[u8] = if johto {
            &HGSS_ROCK_PERCENT
        } else {
            &WATER_PERCENT
        };
        let old_rod: &[u8] = if johto { &ROD_PERCENT } else { &WATER_PERCENT };
        let lists: [(Method, Vec<EncounterMethod>, &[u8]); 6] = [
            (Method::Grass, grass, &GRASS_PERCENT),
            (Method::Water, vec![EncounterMethod::Surf], &WATER_PERCENT),
            (Method::RockSmash, vec![EncounterMethod::RockSmash], rock),
            (Method::OldRod, vec![EncounterMethod::OldRod], old_rod),
            (
                Method::GoodRod,
                vec![EncounterMethod::GoodRod],
                &ROD_PERCENT,
            ),
            (
                Method::SuperRod,
                vec![EncounterMethod::SuperRod],
                &ROD_PERCENT,
            ),
        ];
        lists
            .into_iter()
            .map(|(method, methods, percents)| {
                let mut slots: Vec<Option<&DsEncounter>> = vec![None; percents.len()];
                for &method in &methods {
                    for encounter in here.iter().filter(|e| e.method == method) {
                        if let Some(slot) = slots.get_mut(usize::from(encounter.slot)) {
                            *slot = Some(*encounter);
                        }
                    }
                }
                let mut encounters: Vec<Encounter> = Vec::new();
                for (slot, &percent) in slots.iter().zip(percents) {
                    let Some(slot) = slot.filter(|slot| slot.species != 0) else {
                        continue;
                    };
                    match encounters.iter_mut().find(|e| e.species == slot.species) {
                        Some(known) => {
                            known.min_level = known.min_level.min(slot.min_level);
                            known.max_level = known.max_level.max(slot.max_level);
                            known.percent += percent;
                        }
                        None => encounters.push(Encounter {
                            species: slot.species,
                            min_level: slot.min_level,
                            max_level: slot.max_level,
                            percent,
                        }),
                    }
                }
                encounters.sort_by_key(|encounter| std::cmp::Reverse(encounter.percent));
                WildList { method, encounters }
            })
            .collect()
    }

    pub fn has_wild_lists(&self) -> bool {
        self.generation() == Gen::Four && !self.book.tables.places.is_empty()
    }

    pub fn type_name(&self, gb: u8) -> Option<&str> {
        let own = self.own_type(gb)?;
        self.book
            .names
            .types
            .get(usize::from(own))
            .map(String::as_str)
            .filter(|name| !name.is_empty())
    }

    pub fn species_type_ids(&self, species: u16) -> Vec<u8> {
        self.book
            .tables
            .base_stats
            .get(usize::from(species))
            .map(|base| {
                base.types
                    .iter()
                    .filter_map(|&id| self.gb_type(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn exp_span(&self, mon: &Mon) -> Option<(u32, u32)> {
        let growth = self.growth(mon.species)?;
        Some((growth.exp(mon.level), growth.exp(mon.level + 1)))
    }

    pub fn move_data(&self, id: u16) -> Option<MoveData> {
        let data = self.book.moves.get(usize::from(id)).filter(|_| id != 0)?;
        Some(MoveData {
            type_id: self.gb_type(data.type_id)?,
            power: data.power,
            accuracy: ((u16::from(data.accuracy) * 255 + 50) / 100) as u8,
            pp: data.pp,
        })
    }

    pub fn max_pp(&self, slot: MoveSlot) -> Option<u8> {
        let pp = u16::from(self.book.moves.get(usize::from(slot.id))?.pp);
        Some((pp + pp * 20 * u16::from(slot.pp_ups.min(3)) / 100) as u8)
    }

    pub fn gender(&self, mon: &Mon) -> Option<Gender> {
        let genes = mon.ds()?;
        let ratio = self
            .book
            .tables
            .base_stats
            .get(usize::from(mon.species))?
            .gender_ratio;
        match ratio {
            0xFF => None,
            0xFE => Some(Gender::Female),
            0 => Some(Gender::Male),
            ratio if (genes.genes.personality & 0xFF) as u8 >= ratio => Some(Gender::Male),
            _ => Some(Gender::Female),
        }
    }

    pub(crate) fn gb_type(&self, own: u8) -> Option<u8> {
        let own = match self.generation() {
            Gen::Five if own >= GEN5_FIRE => own + 1,
            Gen::Five | Gen::Four => own,
        };
        crate::tables::gb_type(own)
    }

    fn own_type(&self, gb: u8) -> Option<u8> {
        let own = crate::tables::own_type(gb)?;
        match self.generation() {
            Gen::Five if own == GEN4_ODD_TYPE => None,
            Gen::Five if own > GEN4_ODD_TYPE => Some(own - 1),
            Gen::Five | Gen::Four => Some(own),
        }
    }
}

fn journey(version: Version, badges: &[bool]) -> Vec<Step> {
    let badge = |bit: usize| Step {
        milestone: Milestone::Badge(bit),
        done: badges.get(bit).copied().unwrap_or(false),
    };
    let order: &[usize] = match version {
        Version::Platinum => &PLATINUM_ORDER,
        _ => &[0, 1, 2, 3, 4, 5, 6, 7],
    };
    let mut steps: Vec<Step> = order.iter().map(|&bit| badge(bit)).collect();
    let kanto: Vec<Step> = (8..badges.len()).map(badge).collect();
    steps.push(Step {
        milestone: Milestone::EliteFour,
        done: kanto.iter().any(|step| step.done),
    });
    steps.extend(kanto);
    steps
}

fn named(list: &[String], id: u16) -> Option<&str> {
    list.get(usize::from(id))
        .filter(|name| id != 0 && !name.is_empty())
        .map(String::as_str)
}

fn judge(pkm: &pkm::Pokemon) -> Legality {
    match legality::check(pkm) {
        Ok(()) => Legality::Passes,
        Err(
            reason @ (Reason::Generation
            | Reason::Version(_)
            | Reason::Egg
            | Reason::Unchecked(_)
            | Reason::NoEncounter(_)
            | Reason::Move(_)),
        ) => Legality::NotChecked(reason),
        Err(reason) => Legality::Breaks(reason),
    }
}

fn version(game: DsGame) -> Version {
    match game {
        DsGame::Diamond => Version::Diamond,
        DsGame::Pearl => Version::Pearl,
        DsGame::Platinum => Version::Platinum,
        DsGame::HeartGold => Version::HeartGold,
        DsGame::SoulSilver => Version::SoulSilver,
        DsGame::Black => Version::Black,
        DsGame::White => Version::White,
        DsGame::Black2 => Version::Black2,
        DsGame::White2 => Version::White2,
    }
}

fn language(language: pkm::Language) -> Language {
    match language {
        pkm::Language::Japanese => Language::Japanese,
        pkm::Language::French => Language::French,
        pkm::Language::Italian => Language::Italian,
        pkm::Language::German => Language::German,
        pkm::Language::Spanish => Language::Spanish,
        pkm::Language::Korean => Language::Korean,
        pkm::Language::English | pkm::Language::Other(_) => Language::English,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_pkm::info::{
        Names, PcBox as LiveBox, Pokedex as LivePokedex, Spot, Trainer as LiveTrainer,
    };
    use citrine_gb_pkm::tables::{BaseStats, Place as DsPlace, Tables as PkmTables};

    fn tables(game: DsGame) -> DsTables {
        let mut base = BaseStats {
            types: [10, 2],
            growth: 0,
            gender_ratio: 31,
            abilities: [66, 0, 0],
            ..BaseStats::default()
        };
        base.stats.hp = 39;
        let names = |prefix: &str, count: usize| -> Vec<String> {
            (0..count).map(|i| format!("{prefix}{i}")).collect()
        };
        let encounter = |method, slot, species| DsEncounter {
            place: 7,
            season: None,
            method,
            slot,
            species,
            form: 0,
            min_level: 3,
            max_level: 3,
        };
        let book = Book {
            tables: PkmTables {
                game,
                language: pkm::Language::English,
                base_stats: vec![base; 500],
                learnsets: Vec::new(),
                evolutions: Vec::new(),
                encounters: vec![
                    encounter(EncounterMethod::Grass, 0, 396),
                    encounter(EncounterMethod::Grass, 1, 396),
                    encounter(EncounterMethod::Grass, 2, 399),
                    encounter(EncounterMethod::Grass, 3, 399),
                    encounter(EncounterMethod::GrassNight, 2, 163),
                    encounter(EncounterMethod::GrassNight, 3, 163),
                    encounter(EncounterMethod::Surf, 0, 129),
                ],
                species_names: names("MON", 500),
                places: vec![
                    DsPlace {
                        header: 0,
                        encounters: Some(7),
                        location: 4,
                        area_icon: 0,
                        battle_background: 0,
                    };
                    2
                ],
                growth: Vec::new(),
                pp: Vec::new(),
            },
            names: Names {
                moves: names("MOVE", 470),
                items: names("ITEM", 540),
                abilities: names("ABILITY", 124),
                types: names("TYPE", 18),
                places: names("PLACE", 130),
            },
            moves: Vec::new(),
        };
        DsTables {
            game: Game {
                version: version(game),
                language: Language::English,
                revision: 0,
            },
            book,
            verdicts: Verdicts::default(),
        }
    }

    fn live(game: DsGame, badges: Vec<bool>) -> Live {
        let mut mon = pkm::Pokemon::new(Gen::Four);
        mon.species = 390;
        mon.pid = 0x0000_0019;
        mon.exp = 125;
        mon.ability = 66;
        mon.moves = [10, 43, 0, 0];
        mon.pp = [35, 30, 0, 0];
        let mut egg = mon.clone();
        egg.is_egg = true;
        Live {
            game,
            language: pkm::Language::English,
            trainer: LiveTrainer {
                name: "LUCAS".to_owned(),
                tid: 12345,
                sid: 54321,
                female: false,
                money: 3000,
                badges,
                hours: 1,
                minutes: 2,
                seconds: 3,
            },
            party: vec![mon.clone()],
            boxes: vec![
                LiveBox {
                    name: "BOX 1".to_owned(),
                    pokemon: vec![mon.clone(), egg],
                },
                LiveBox {
                    name: String::new(),
                    pokemon: Vec::new(),
                },
            ],
            current_box: 1,
            pokedex: LivePokedex {
                seen: vec![387, 390, 396],
                caught: vec![390],
            },
            spot: Some(Spot {
                map: 1,
                x: 10,
                z: 20,
            }),
        }
    }

    #[test]
    fn a_live_ds_game_folds_into_game_state() {
        let tables = tables(DsGame::Platinum);
        let live = live(DsGame::Platinum, vec![true; 2]);
        let legality: Vec<Legality> = live.party.iter().map(judge).collect();
        let state = tables.fold(&live, &legality);
        assert_eq!(state.game.version, Version::Platinum);
        assert_eq!(state.trainer.name, "LUCAS");
        assert_eq!(state.trainer.badges, vec![true, true]);
        assert_eq!(state.pokedex.caught, vec![390]);
        let Pokemon::Hatched(mon) = &state.party[0] else {
            panic!("egg");
        };
        assert_eq!(mon.level, 5);
        assert_eq!(mon.moves.len(), 2);
        let genes = mon.ds().expect("ds genes");
        assert_eq!(genes.genes.nature, Nature::from_personality(0x19));
        assert_eq!(tables.ability_name(genes.ability), Some("ABILITY66"));
        assert!(matches!(
            genes.legality,
            Legality::NotChecked(Reason::Version(0))
        ));
        let Some(Position::Ds(spot)) = state.location else {
            panic!("no spot");
        };
        assert_eq!(tables.spot_name(&spot), Some("PLACE4"));
    }

    #[test]
    fn boxed_ds_pokemon_fold_with_their_level_and_legality() {
        let mut tables = tables(DsGame::HeartGold);
        let live = live(DsGame::HeartGold, vec![false; 16]);
        let legality: Vec<Legality> = live
            .party
            .iter()
            .chain(live.boxes.iter().flat_map(|pc| &pc.pokemon))
            .map(|p| tables.verdicts.judge(p))
            .collect();
        let state = tables.fold(&live, &legality);
        assert_eq!(state.boxes.len(), 2);
        assert_eq!(state.current_box, 1);
        assert_eq!(state.boxes[0].number, 1);
        assert_eq!(state.boxes[0].name.as_deref(), Some("BOX 1"));
        assert_eq!(state.boxes[1].name, None);
        assert!(state.boxes[1].pokemon.is_empty());
        let Pokemon::Hatched(boxed) = &state.boxes[0].pokemon[0] else {
            panic!("egg");
        };
        assert_eq!(boxed.level, 5);
        assert_eq!(boxed.hp, None);
        assert_eq!(state.boxes[0].pokemon[1], Pokemon::Egg);
        let Pokemon::Hatched(lead) = &state.party[0] else {
            panic!("egg");
        };
        assert_eq!(
            boxed.ds().map(|g| g.legality),
            lead.ds().map(|g| g.legality)
        );
    }

    #[test]
    fn platinum_badges_follow_its_gym_order() {
        let steps = journey(Version::Platinum, &[true; 8]);
        let bits: Vec<Milestone> = steps.iter().map(|step| step.milestone).collect();
        assert_eq!(bits[2], Milestone::Badge(4));
        assert_eq!(bits[3], Milestone::Badge(2));
        assert_eq!(bits[8], Milestone::EliteFour);
        let johto = journey(Version::SoulSilver, &[[true; 8], [false; 8]].concat());
        assert_eq!(johto[8].milestone, Milestone::EliteFour);
        assert!(!johto[8].done);
        assert_eq!(johto.len(), 17);
    }

    #[test]
    fn wild_lists_take_the_time_of_day_slots() {
        let tables = tables(DsGame::Platinum);
        let spot = DsSpot { map: 1, x: 0, z: 0 };
        let night = tables.wild(&spot, Some(TimeOfDay::Night));
        let grass = &night[0];
        assert_eq!(grass.method, Method::Grass);
        assert_eq!(grass.encounters[0].species, 396);
        assert_eq!(grass.encounters[0].percent, 40);
        assert_eq!(grass.encounters[1].species, 163);
        assert_eq!(grass.encounters[1].percent, 20);
        let unknown = tables.wild(&spot, None);
        assert_eq!(unknown[0].encounters[1].species, 399);
        assert_eq!(unknown[1].encounters[0].species, 129);
        assert_eq!(unknown[1].encounters[0].percent, 60);
    }

    #[test]
    fn gen5_types_skip_the_gen4_mystery_type() {
        let four = tables(DsGame::Platinum);
        let five = tables(DsGame::Black);
        assert_eq!(four.gb_type(10), five.gb_type(9));
        assert_eq!(five.type_name(five.gb_type(9).unwrap()), Some("TYPE9"));
        assert_eq!(four.type_name(four.gb_type(10).unwrap()), Some("TYPE10"));
    }
}
