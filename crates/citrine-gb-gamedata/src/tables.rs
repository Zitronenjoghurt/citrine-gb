use crate::game::{Game, Generation, identify};
use crate::gen3::{self, Growth, Layout, Memory, RegionMaps};
use crate::matchups::Effect;
use crate::moves::MoveData;
use crate::region_map::Marker;
use crate::rom::{Place, RomTables};
use crate::state::{Caught, GameState, Gender, Genes, Mon, MoveSlot, Position, Stats, TimeOfDay};
use crate::wild::WildList;
use crate::{ReadError, read_live, read_save};

const GEN3_TYPES: [u8; 18] = [
    0, 1, 2, 3, 4, 5, 7, 8, 9, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B,
];
const GEN3_GIFT_EGG: u8 = 0xFD;
const GEN3_TRADE: u8 = 0xFE;
const GEN3_EVENT: u8 = 0xFF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tables {
    Gb(Box<RomTables>),
    Gba {
        tables: Box<gen3::RomTables>,
        maps: Option<Box<RegionMaps>>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum Ram<'a> {
    Gb(&'a [u8]),
    Gba(Memory<'a>),
}

impl Tables {
    pub fn load(rom: &[u8]) -> Option<Self> {
        if let Some(game) = identify(rom) {
            return RomTables::load(game, rom)
                .ok()
                .map(|tables| Tables::Gb(Box::new(tables)));
        }
        let tables = gen3::RomTables::load(rom).ok()?;
        Some(Tables::Gba {
            tables: Box::new(tables),
            maps: RegionMaps::load(rom).map(Box::new),
        })
    }

    pub fn game(&self) -> Game {
        match self {
            Tables::Gb(tables) => tables.game,
            Tables::Gba { tables, .. } => tables.game,
        }
    }

    pub fn generation(&self) -> Generation {
        self.game().generation()
    }

    pub fn gb(&self) -> Option<&RomTables> {
        match self {
            Tables::Gb(tables) => Some(tables),
            Tables::Gba { .. } => None,
        }
    }

    pub fn read_save(&self, battery: &[u8]) -> Result<GameState, ReadError> {
        match self {
            Tables::Gb(tables) => read_save(tables.game, battery),
            Tables::Gba { tables, .. } => {
                let game = tables.game;
                let layout = Layout::of(game.version).ok_or(ReadError::RomTables)?;
                Ok(gen3::read_save(battery, layout, game.language)?.to_game_state(tables))
            }
        }
    }

    pub fn read_live(&self, ram: Ram, battery: &[u8]) -> Result<GameState, ReadError> {
        match (self, ram) {
            (Tables::Gb(tables), Ram::Gb(wram)) => read_live(tables, wram, battery),
            (Tables::Gba { tables, .. }, Ram::Gba(memory)) => {
                let game = tables.game;
                let layout = Layout::of(game.version).ok_or(ReadError::RomTables)?;
                Ok(gen3::read_live(memory, layout, game.language)?.to_game_state(tables))
            }
            (Tables::Gb(_), Ram::Gba(_)) | (Tables::Gba { .. }, Ram::Gb(_)) => {
                Err(ReadError::NotRunning)
            }
        }
    }

    pub fn species_name(&self, species: u16) -> Option<&str> {
        match self {
            Tables::Gb(tables) => tables.species_name(species),
            Tables::Gba { tables, .. } => tables.species_name(species),
        }
    }

    pub fn dex_number(&self, species: u16) -> Option<u16> {
        match self {
            Tables::Gb(tables) => tables.dex_number(species),
            Tables::Gba { tables, .. } => tables.national_number(species),
        }
    }

    pub fn species_for_dex(&self, dex: u16) -> Option<u16> {
        match self {
            Tables::Gb(tables) => tables.species_for_dex(dex),
            Tables::Gba { tables, .. } => tables.species_for_national(dex),
        }
    }

    pub fn pokedex_size(&self) -> u16 {
        match self {
            Tables::Gb(tables) => tables.pokedex_size(),
            Tables::Gba { tables, .. } => tables.pokedex_size(),
        }
    }

    pub fn move_name(&self, id: u16) -> Option<&str> {
        match self {
            Tables::Gb(tables) => tables.move_name(id),
            Tables::Gba { tables, .. } => tables.move_name(id),
        }
    }

    pub fn item_name(&self, id: u16) -> Option<String> {
        match self {
            Tables::Gb(tables) => tables.item_name(id),
            Tables::Gba { tables, .. } => tables.item_name(id).map(str::to_owned),
        }
    }

    pub fn type_name(&self, id: u8) -> Option<&str> {
        match self {
            Tables::Gb(tables) => tables.type_name(id),
            Tables::Gba { tables, .. } => {
                let own = GEN3_TYPES.iter().position(|&gb| gb == id)?;
                tables.type_name(own as u8)
            }
        }
    }

    pub fn species_type_ids(&self, species: u16) -> Vec<u8> {
        let mut ids = match self {
            Tables::Gb(tables) => tables
                .base_stats(species)
                .map(|base| base.types.to_vec())
                .unwrap_or_default(),
            Tables::Gba { tables, .. } => tables
                .base_stats(species)
                .map(|base| base.types.iter().filter_map(|&id| gb_type(id)).collect())
                .unwrap_or_default(),
        };
        ids.dedup();
        ids
    }

    pub fn mon_stats(&self, mon: &Mon) -> Option<Stats> {
        match self {
            Tables::Gb(tables) => mon.stats.or_else(|| tables.mon_stats(mon)),
            Tables::Gba { .. } => mon.stats,
        }
    }

    pub fn mon_gender(&self, mon: &Mon) -> Option<Gender> {
        match (self, &mon.genes) {
            (Tables::Gb(tables), _) => tables.mon_gender(mon),
            (Tables::Gba { tables, .. }, Genes::Gba(genes)) => {
                tables.base_stats(mon.species)?.gender(genes.personality)
            }
            (Tables::Gba { .. }, Genes::Gb { .. }) => None,
        }
    }

    pub fn mon_ability(&self, mon: &Mon) -> Option<&str> {
        let (Tables::Gba { tables, .. }, Genes::Gba(genes)) = (self, &mon.genes) else {
            return None;
        };
        let base = tables.base_stats(mon.species)?;
        tables.ability_name(base.ability(genes.ability_slot))
    }

    pub fn exp_span(&self, mon: &Mon) -> Option<(u32, u32)> {
        if mon.level >= 100 {
            return None;
        }
        match self {
            Tables::Gb(tables) => {
                let growth = tables.base_stats(mon.species)?.growth_rate;
                Some((growth.exp_at(mon.level), growth.exp_at(mon.level + 1)))
            }
            Tables::Gba { tables, .. } => {
                let growth: Growth = tables.base_stats(mon.species)?.growth;
                Some((growth.exp(mon.level), growth.exp(mon.level + 1)))
            }
        }
    }

    pub fn caught_place(&self, caught: &Caught) -> Option<Place<'_>> {
        match self {
            Tables::Gb(tables) => tables.caught_place(caught),
            Tables::Gba { maps, .. } => match caught.location {
                GEN3_GIFT_EGG | GEN3_TRADE => Some(Place::Gift),
                GEN3_EVENT => Some(Place::Event),
                section => maps.as_ref()?.section_name(section).map(Place::Landmark),
            },
        }
    }

    pub fn marker(&self, position: &Position) -> Option<Marker<'_>> {
        match (self, position) {
            (Tables::Gb(tables), Position::Gb(location)) => tables.marker(location),
            (Tables::Gba { maps, .. }, Position::Gba(whereabouts)) => {
                maps.as_ref()?.marker(whereabouts)
            }
            (Tables::Gb(_), Position::Gba(_)) | (Tables::Gba { .. }, Position::Gb(_)) => None,
        }
    }

    pub fn wild(&self, position: &Position, time: Option<TimeOfDay>) -> Vec<WildList> {
        match (self, position) {
            (Tables::Gb(tables), Position::Gb(location)) => tables.wild(location, time),
            (Tables::Gba { tables, .. }, Position::Gba(whereabouts)) => {
                tables.wild(whereabouts.location)
            }
            (Tables::Gb(_), Position::Gba(_)) | (Tables::Gba { .. }, Position::Gb(_)) => Vec::new(),
        }
    }

    pub fn move_data(&self, id: u16) -> Option<MoveData> {
        match self {
            Tables::Gb(tables) => tables.move_data(id),
            Tables::Gba { tables, .. } => {
                let data = tables.move_data(id)?;
                Some(MoveData {
                    type_id: gb_type(data.type_id)?,
                    accuracy: ((u16::from(data.accuracy) * 255 + 50) / 100) as u8,
                    ..data
                })
            }
        }
    }

    pub fn max_pp(&self, slot: MoveSlot) -> Option<u8> {
        match self {
            Tables::Gb(tables) => tables.max_pp(slot),
            Tables::Gba { tables, .. } => {
                let pp = u16::from(tables.move_data(slot.id)?.pp);
                Some((pp + pp * 20 * u16::from(slot.pp_ups.min(3)) / 100) as u8)
            }
        }
    }

    pub fn effect(&self, move_id: u16, defenders: [u8; 2]) -> Option<Effect> {
        self.gb()?.effect(move_id, defenders)
    }

    pub fn machine_move(&self, item: u16) -> Option<&str> {
        match self {
            Tables::Gb(tables) => tables.machine_move(item),
            Tables::Gba { tables, .. } => tables.machine_move(item),
        }
    }

    pub fn is_hm(&self, item: u16) -> bool {
        match self {
            Tables::Gb(tables) => tables.is_hm(item),
            Tables::Gba { tables, .. } => tables.is_hm(item),
        }
    }

    pub fn front_picture(&self, rom: &[u8], species: u16, shiny: bool) -> Option<gen3::Picture> {
        match self {
            Tables::Gb(_) => None,
            Tables::Gba { tables, .. } => tables.front_picture(rom, species, shiny),
        }
    }

    pub fn hidden_power(&self, mon: &Mon) -> Option<(u8, u8)> {
        match (self.generation(), &mon.genes) {
            (Generation::One, _) => None,
            (Generation::Two, Genes::Gb { dvs, .. }) => Some(dvs.hidden_power()),
            (Generation::Three, Genes::Gba(genes)) => {
                let iv = genes.ivs;
                let order = [
                    iv.hp,
                    iv.attack,
                    iv.defense,
                    iv.speed,
                    iv.special_attack,
                    iv.special_defense,
                ];
                let bits = |shift: u8| {
                    order
                        .iter()
                        .enumerate()
                        .map(|(i, &value)| u32::from(value >> shift & 1) << i)
                        .sum::<u32>()
                };
                let index = (bits(0) * 15 / 63) as u8;
                let own = if index < 8 { index + 1 } else { index + 2 };
                let power = (bits(1) * 40 / 63 + 30) as u8;
                Some((gb_type(own)?, power))
            }
            _ => None,
        }
    }
}

fn gb_type(own: u8) -> Option<u8> {
    GEN3_TYPES.get(usize::from(own)).copied()
}

#[cfg(test)]
mod tests {
    use crate::fixtures::gen3_game::{
        gen3_sample_save, gen3_state, gen3_tables, hatched, rom_code,
    };
    use crate::fixtures::gen3_rom::{SPOTS, gen3_rom};
    use crate::gen3::{Layout, Spread};
    use crate::{Generation, Genes, MoveData, MoveSlot};

    #[test]
    fn gen3_tables_name_species_moves_items_and_types() {
        let tables = gen3_tables(Layout::Emerald);
        assert_eq!(tables.generation(), Generation::Three);
        assert!(tables.gb().is_none());
        assert_eq!(tables.species_name(277), Some("TREECKO"));
        assert_eq!(tables.dex_number(277), Some(252));
        assert_eq!(tables.species_for_dex(252), Some(277));
        assert_eq!(tables.move_name(1), Some("POUND"));
        assert_eq!(tables.item_name(13).as_deref(), Some("ITEM13"));
        assert_eq!(tables.type_name(0), Some("NORMAL"));
        assert_eq!(tables.type_name(0x1B), Some("DARK"));
        assert_eq!(tables.species_type_ids(277), vec![0x08, 0x14]);
    }

    #[test]
    fn gen3_move_data_uses_gb_type_ids_and_accuracy() {
        let tables = gen3_tables(Layout::FireRedLeafGreen);
        assert_eq!(
            tables.move_data(1),
            Some(MoveData {
                type_id: 0,
                power: 40,
                accuracy: 255,
                pp: 35,
            })
        );
        let dragon = tables.move_data(52).expect("move 52");
        assert_eq!(dragon.type_id, 0x1A);
        assert_eq!(dragon.accuracy_percent(), 78);
        assert_eq!(tables.move_data(0), None);
        assert_eq!(tables.move_data(355), None);
    }

    #[test]
    fn gen3_pp_ups_add_a_fifth_each() {
        let tables = gen3_tables(Layout::Emerald);
        let slot = |pp_ups| MoveSlot {
            id: 1,
            pp: 0,
            pp_ups,
        };
        assert_eq!(tables.max_pp(slot(0)), Some(35));
        assert_eq!(tables.max_pp(slot(1)), Some(42));
        assert_eq!(tables.max_pp(slot(3)), Some(56));
    }

    #[test]
    fn gen3_machines_name_their_moves() {
        let tables = gen3_tables(Layout::RubySapphire);
        assert_eq!(tables.machine_move(289), Some("MOVE264"));
        assert_eq!(tables.machine_move(292), Some("MOVE347"));
        assert_eq!(tables.machine_move(288), None);
        assert!(!tables.is_hm(338));
        assert!(tables.is_hm(339));
        assert!(tables.is_hm(346));
        assert!(!tables.is_hm(347));
    }

    #[test]
    fn gen3_hidden_power_comes_from_the_iv_bits() {
        let tables = gen3_tables(Layout::Emerald);
        let state = gen3_state(Layout::Emerald, &gen3_sample_save(Layout::Emerald));
        assert_eq!(
            tables.hidden_power(hatched(&state.party[0])),
            Some((0x16, 38))
        );
        let mut perfect = hatched(&state.party[0]).clone();
        if let Genes::Gba(genes) = &mut perfect.genes {
            genes.ivs = Spread {
                hp: 31,
                attack: 31,
                defense: 31,
                speed: 31,
                special_attack: 31,
                special_defense: 31,
            };
        }
        assert_eq!(tables.hidden_power(&perfect), Some((0x1B, 70)));
    }

    #[test]
    fn gen3_front_pictures_come_from_the_rom() {
        let rom = gen3_rom(rom_code(Layout::Emerald), 0, SPOTS);
        let tables = gen3_tables(Layout::Emerald);
        let red = tables.front_picture(&rom, 277, false).expect("picture");
        let blue = tables.front_picture(&rom, 277, true).expect("shiny");
        assert_eq!(red.pixels.len(), 64 * 64);
        assert_eq!(red.palette[1], [0xFF, 0, 0]);
        assert_eq!(blue.palette[1], [0, 0, 0xFF]);
        assert_eq!(tables.front_picture(&rom, 0, false), None);
    }
}
