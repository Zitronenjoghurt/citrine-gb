use super::base::{self, BaseStats};
use super::pics::{self, Picture};
use super::region_map::Warp;
use super::text::{self, TERMINATOR};
use super::wild::WildTables;
use super::{DEX, Mon, u16_at};
use crate::ReadError;
use crate::game::{Game, Language, Version};
use crate::moves::MoveData;
use crate::state::{Gender, Stats};
use crate::wild::WildList;

const CODE: usize = 0xAC;
const REVISION: usize = 0xBC;
const HEADER: usize = 0xC0;
const SPECIES: usize = 412;
const JOHTO_LAST: usize = 251;
const SPECIES_NAME: usize = 11;
const MOVES: usize = 355;
const ABILITIES: usize = 78;
const LONG_NAME: usize = 13;
const TYPES: usize = 18;
const TYPE_NAME: usize = 7;
const UNKNOWN_TYPE: usize = 9;
const ITEM: usize = 44;
const ITEM_NAME: usize = 14;
const ITEM_ID: usize = 14;
const FIRST_ITEMS: usize = 13;
const JOHTO: u16 = 251;
const MOVE: usize = 12;
const POUND: [u8; 5] = [0, 40, 0, 100, 35];
const KARATE_CHOP: [u8; 4] = [50, 1, 100, 25];
const MACHINES: usize = 58;
const FIRST_MACHINES: [u16; 4] = [264, 337, 352, 347];
const TM01: u16 = 289;
const ROM_BASE: u32 = 0x0800_0000;
const PIC_SIZE: u16 = 0x800;
const SHINY_TAG: u16 = 500;
const HM01: u16 = 339;
const QUESTION: u8 = 0xAC;
const DASH: u8 = 0xAE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomTables {
    pub game: Game,
    species_names: Vec<String>,
    move_names: Vec<String>,
    ability_names: Vec<String>,
    type_names: Vec<String>,
    item_names: Vec<String>,
    national: Vec<u16>,
    base_stats: Vec<Option<BaseStats>>,
    moves: Vec<MoveData>,
    machines: Vec<u16>,
    pictures: Option<PictureTables>,
    wild: WildTables,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PictureTables {
    front: usize,
    palettes: usize,
    shiny: usize,
}

pub fn identify(rom: &[u8]) -> Option<Game> {
    if rom.len() < HEADER {
        return None;
    }
    let code = &rom[CODE..CODE + 4];
    let version = match &code[..3] {
        b"AXV" => Version::Ruby,
        b"AXP" => Version::Sapphire,
        b"BPE" => Version::Emerald,
        b"BPR" => Version::FireRed,
        b"BPG" => Version::LeafGreen,
        _ => return None,
    };
    let language = match code[3] {
        b'J' => Language::Japanese,
        b'E' => Language::English,
        b'D' => Language::German,
        b'F' => Language::French,
        b'I' => Language::Italian,
        b'S' => Language::Spanish,
        _ => return None,
    };
    Some(Game {
        version,
        language,
        revision: rom[REVISION],
    })
}

impl RomTables {
    pub fn load(rom: &[u8]) -> Result<Self, ReadError> {
        let game = identify(rom).ok_or(ReadError::RomTables)?;
        let language = game.language;
        let names = |at: usize, size: usize, count: usize| -> Vec<String> {
            rom[at..at + size * count]
                .chunks_exact(size)
                .map(|record| text::decode(record, language))
                .collect()
        };
        let mut species_first = vec![QUESTION; SPECIES_NAME - 1];
        species_first.push(TERMINATOR);
        let species = find_names(rom, SPECIES_NAME, SPECIES, &species_first, 0);
        let moves = find_names(rom, LONG_NAME, MOVES, &[DASH, TERMINATOR], 0);
        let mut ability_first = vec![DASH; 7];
        ability_first.push(TERMINATOR);
        let abilities = find_names(rom, LONG_NAME, ABILITIES, &ability_first, 0);
        let types = find_names(
            rom,
            TYPE_NAME,
            TYPES,
            &[QUESTION, QUESTION, QUESTION, TERMINATOR, 0, 0, 0],
            UNKNOWN_TYPE,
        );
        let (Some(species), Some(moves), Some(abilities), Some(types)) =
            (species, moves, abilities, types)
        else {
            return Err(ReadError::RomTables);
        };
        let (items, item_count) = find_items(rom).ok_or(ReadError::RomTables)?;
        let national = find_national(rom).ok_or(ReadError::RomTables)?;
        let base_stats = find_base_stats(rom).ok_or(ReadError::RomTables)?;
        Ok(Self {
            game,
            species_names: names(species, SPECIES_NAME, SPECIES),
            move_names: names(moves, LONG_NAME, MOVES),
            ability_names: names(abilities, LONG_NAME, ABILITIES),
            type_names: names(types, TYPE_NAME, TYPES),
            item_names: (0..item_count)
                .map(|i| text::decode(&rom[items + i * ITEM..][..ITEM_NAME], language))
                .collect(),
            national,
            base_stats: rom[base_stats..base_stats + base::RECORD * SPECIES]
                .as_chunks::<{ base::RECORD }>()
                .0
                .iter()
                .map(|record| BaseStats::parse(record))
                .collect(),
            moves: find_moves(rom),
            machines: find_machines(rom),
            pictures: find_pictures(rom),
            wild: WildTables::load(rom),
        })
    }

    pub fn pokedex_size(&self) -> u16 {
        DEX
    }

    pub fn wild(&self, at: Warp) -> Vec<WildList> {
        self.wild.here(at)
    }

    pub fn wild_maps(&self) -> usize {
        self.wild.len()
    }

    pub fn national_number(&self, species: u16) -> Option<u16> {
        self.national
            .get(usize::from(species))
            .copied()
            .filter(|&n| n != 0)
    }

    pub fn species_for_national(&self, national: u16) -> Option<u16> {
        if national == 0 {
            return None;
        }
        let index = self.national.iter().position(|&n| n == national)?;
        u16::try_from(index).ok()
    }

    pub fn species_name(&self, species: u16) -> Option<&str> {
        self.national_number(species)?;
        self.species_names
            .get(usize::from(species))
            .map(String::as_str)
    }

    pub fn move_name(&self, id: u16) -> Option<&str> {
        named(&self.move_names, usize::from(id))
    }

    pub fn item_name(&self, id: u16) -> Option<&str> {
        named(&self.item_names, usize::from(id))
    }

    pub fn ability_name(&self, id: u8) -> Option<&str> {
        named(&self.ability_names, usize::from(id))
    }

    pub fn type_name(&self, id: u8) -> Option<&str> {
        self.type_names.get(usize::from(id)).map(String::as_str)
    }

    pub fn base_stats(&self, species: u16) -> Option<&BaseStats> {
        self.national_number(species)?;
        self.base_stats.get(usize::from(species))?.as_ref()
    }

    pub fn species_types(&self, species: u16) -> Vec<&str> {
        let Some(base) = self.base_stats(species) else {
            return Vec::new();
        };
        let mut types = vec![base.types[0]];
        if base.types[1] != base.types[0] {
            types.push(base.types[1]);
        }
        types
            .into_iter()
            .filter_map(|id| self.type_name(id))
            .collect()
    }

    pub fn move_data(&self, id: u16) -> Option<MoveData> {
        (id != 0).then(|| self.moves.get(usize::from(id)).copied())?
    }

    pub fn machine_move(&self, item: u16) -> Option<&str> {
        let index = item.checked_sub(TM01)?;
        self.move_name(*self.machines.get(usize::from(index))?)
    }

    pub fn is_hm(&self, item: u16) -> bool {
        (HM01..TM01 + MACHINES as u16).contains(&item)
    }

    pub fn front_picture(&self, rom: &[u8], species: u16, shiny: bool) -> Option<Picture> {
        self.base_stats(species)?;
        let at = self.pictures?;
        let entry = |table: usize| table + usize::from(species) * 8;
        let pixels = pics::tiles(&pics::lz77(pointed(rom, entry(at.front))?)?)?;
        let palettes = if shiny { at.shiny } else { at.palettes };
        let palette = pics::palette(&pics::lz77(pointed(rom, entry(palettes))?)?)?;
        Some(Picture { pixels, palette })
    }

    pub fn mon_level(&self, mon: &Mon) -> Option<u8> {
        if let Some(level) = mon.level {
            return Some(level);
        }
        Some(mon.level_with(self.base_stats(mon.species)?.growth))
    }

    pub fn mon_stats(&self, mon: &Mon) -> Option<Stats> {
        if let Some(party) = &mon.party {
            return Some(party.stats);
        }
        let base = self.base_stats(mon.species)?;
        let national = self.national_number(mon.species)?;
        Some(base.stats(mon, self.mon_level(mon)?, national))
    }

    pub fn mon_gender(&self, mon: &Mon) -> Option<Gender> {
        self.base_stats(mon.species)?.gender(mon.personality)
    }

    pub fn mon_ability(&self, mon: &Mon) -> Option<&str> {
        let base = self.base_stats(mon.species)?;
        self.ability_name(base.ability(mon.ability_slot))
    }

    pub fn exp_to_next_level(&self, mon: &Mon) -> Option<u32> {
        let growth = self.base_stats(mon.species)?.growth;
        let level = self.mon_level(mon)?;
        (level < 100).then(|| growth.exp(level + 1).saturating_sub(mon.exp))
    }
}

fn named(names: &[String], index: usize) -> Option<&str> {
    if index == 0 {
        return None;
    }
    names.get(index).map(String::as_str)
}

fn positions<'a>(rom: &'a [u8], needle: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    let first = needle[0];
    (0..rom.len().saturating_sub(needle.len()))
        .filter(move |&at| rom[at] == first && rom[at..at + needle.len()] == *needle)
}

fn named_record(record: &[u8]) -> bool {
    record
        .iter()
        .position(|&b| b == TERMINATOR)
        .is_some_and(|end| end > 0)
}

fn find_names(
    rom: &[u8],
    size: usize,
    count: usize,
    known: &[u8],
    known_index: usize,
) -> Option<usize> {
    positions(rom, known)
        .filter_map(|at| at.checked_sub(known_index * size))
        .find(|&start| {
            rom.get(start..start + size * count)
                .is_some_and(|table| table.chunks_exact(size).all(named_record))
        })
}

fn find_items(rom: &[u8]) -> Option<(usize, usize)> {
    let id = |at: usize, i: usize| {
        rom.get(at + i * ITEM + ITEM_ID..at + i * ITEM + ITEM_ID + 2)
            .map(|bytes| usize::from(u16_at(bytes, 0)))
    };
    let start = (0..rom.len().saturating_sub(ITEM * FIRST_ITEMS))
        .step_by(4)
        .find(|&at| rom[at] == QUESTION && (0..FIRST_ITEMS).all(|i| id(at, i) == Some(i)))?;
    let count = (FIRST_ITEMS..).find(|&i| {
        !id(start, i).is_some_and(|value| value == i || value == 0)
            || !named_record(&rom[start + i * ITEM..][..ITEM_NAME])
    })?;
    Some((start, count))
}

fn find_national(rom: &[u8]) -> Option<Vec<u16>> {
    let johto: Vec<u8> = (1..=JOHTO).flat_map(u16::to_le_bytes).collect();
    positions(rom, &johto).find_map(|at| {
        let table = rom.get(at..at + 2 * (SPECIES - 1))?;
        let national: Vec<u16> = std::iter::once(0)
            .chain(
                table
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair)),
            )
            .map(|n| if n > DEX { 0 } else { n })
            .collect();
        let mut hoenn: Vec<u16> = national[usize::from(JOHTO) + 1..]
            .iter()
            .copied()
            .filter(|&n| n != 0)
            .collect();
        hoenn.sort_unstable();
        let complete = hoenn.iter().copied().eq(JOHTO + 1..=DEX);
        complete.then_some(national)
    })
}

fn find_base_stats(rom: &[u8]) -> Option<usize> {
    let span = base::RECORD * SPECIES;
    (0..rom.len().saturating_sub(span)).step_by(4).find(|&at| {
        let table = &rom[at..at + span];
        let records = table[base::RECORD..].as_chunks::<{ base::RECORD }>().0;
        table[..base::RECORD].iter().all(|&b| b == 0)
            && records[..JOHTO_LAST]
                .iter()
                .all(|record| record[..6].iter().all(|&stat| stat != 0))
            && records.iter().all(|record| base::plausible(record))
    })
}

fn find_moves(rom: &[u8]) -> Vec<MoveData> {
    let span = MOVE * MOVES;
    let found =
        (0..rom.len().saturating_sub(span)).step_by(4).find(|&at| {
            let table = &rom[at..at + span];
            table[..MOVE].iter().all(|&b| b == 0)
                && table[MOVE..MOVE + POUND.len()] == POUND
                && table[2 * MOVE + 1..2 * MOVE + 1 + KARATE_CHOP.len()] == KARATE_CHOP
                && table.as_chunks::<MOVE>().0.iter().all(|r| {
                    usize::from(r[2]) < TYPES && r[3] <= 100 && r[4] <= 40 && r[9..] == [0; 3]
                })
        });
    let Some(at) = found else {
        return Vec::new();
    };
    rom[at..at + span]
        .as_chunks::<MOVE>()
        .0
        .iter()
        .map(|r| MoveData {
            type_id: r[2],
            power: r[1],
            accuracy: r[3],
            pp: r[4],
        })
        .collect()
}

fn find_machines(rom: &[u8]) -> Vec<u16> {
    let first: Vec<u8> = FIRST_MACHINES
        .iter()
        .copied()
        .flat_map(u16::to_le_bytes)
        .collect();
    positions(rom, &first)
        .find_map(|at| {
            let table = rom.get(at..at + 2 * MACHINES)?;
            let moves: Vec<u16> = table
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            moves
                .iter()
                .all(|&id| (1..MOVES as u16).contains(&id))
                .then_some(moves)
        })
        .unwrap_or_default()
}

fn pointed(rom: &[u8], at: usize) -> Option<&[u8]> {
    let word = u32::from_le_bytes(rom.get(at..at + 4)?.try_into().ok()?);
    let offset = usize::try_from(word.checked_sub(ROM_BASE)?).ok()?;
    rom.get(offset..)
}

fn tagged(
    rom: &[u8],
    at: usize,
    tag_at: usize,
    tag: impl Fn(u16) -> u16,
    size: Option<u16>,
) -> bool {
    (0..SPECIES as u16).all(|i| {
        let entry = at + usize::from(i) * 8;
        rom.get(entry..entry + 8).is_some_and(|e| {
            let pointer = u32::from_le_bytes([e[0], e[1], e[2], e[3]]);
            pointer >> 24 == ROM_BASE >> 24
                && u16_at(e, tag_at) == tag(i)
                && size.is_none_or(|size| u16_at(e, 4) == size)
        })
    })
}

fn find_pictures(rom: &[u8]) -> Option<PictureTables> {
    let starts = || (0..rom.len().saturating_sub(SPECIES * 8)).step_by(4);
    let front = starts().find(|&at| tagged(rom, at, 6, |i| i, Some(PIC_SIZE)))?;
    let palettes = starts().find(|&at| tagged(rom, at, 4, |i| i, None))?;
    let shiny = starts().find(|&at| tagged(rom, at, 4, |i| i + SHINY_TAG, None))?;
    Some(PictureTables {
        front,
        palettes,
        shiny,
    })
}

#[cfg(test)]
mod tests {
    use super::RomTables;
    use crate::fixtures::gen3_rom::{ITEMS, SHEDINJA, SPECIES, Spots, national_for, species_name};
    use crate::fixtures::gen3_rom::{MOVED, PICTURE_BYTE, SPOTS, gen3_rom};
    use crate::gen3::{Met, Mon, PartyStats, Spread, identify};
    use crate::{Game, Gender, Language, ReadError, Stats, Version};

    #[test]
    fn front_pictures_come_with_their_palettes() {
        for spots in [SPOTS, MOVED] {
            let rom = gen3_rom(b"BPEE", 0, spots);
            let tables = RomTables::load(&rom).expect("tables");
            let plain = tables.front_picture(&rom, 277, false).expect("picture");
            assert_eq!(plain.pixels.len(), 64 * 64);
            assert_eq!(&plain.pixels[..2], &[PICTURE_BYTE & 0xF, PICTURE_BYTE >> 4]);
            assert_eq!(plain.palette[1], [0xFF, 0, 0]);
            let shiny = tables.front_picture(&rom, 277, true).expect("shiny");
            assert_eq!(shiny.pixels, plain.pixels);
            assert_eq!(shiny.palette[1], [0, 0, 0xFF]);
        }
    }

    #[test]
    fn placeholder_species_have_no_picture() {
        let rom = gen3_rom(b"BPRE", 0, SPOTS);
        let tables = RomTables::load(&rom).expect("tables");
        assert_eq!(tables.front_picture(&rom, 0, false), None);
        assert_eq!(tables.front_picture(&rom, 260, false), None);
        assert_eq!(tables.front_picture(&rom, 999, false), None);
    }

    #[test]
    fn a_rom_without_picture_tables_still_loads() {
        let mut rom = gen3_rom(b"AXVE", 0, SPOTS);
        rom[SPOTS.pictures + 6] ^= 0xFF;
        let tables = RomTables::load(&rom).expect("tables");
        assert_eq!(tables.front_picture(&rom, 277, false), None);
        assert_eq!(tables.species_name(277), Some("TREECKO"));
    }

    fn tables(at: Spots) -> RomTables {
        RomTables::load(&gen3_rom(b"BPEE", 0, at)).expect("tables")
    }

    fn mon(species: u16, personality: u32, exp: u32) -> Mon {
        Mon {
            species,
            personality,
            ot_id: 1,
            ot_secret_id: 2,
            nickname: String::new(),
            ot_name: String::new(),
            language: Some(Language::English),
            held_item: None,
            exp,
            level: None,
            friendship: 70,
            moves: Vec::new(),
            ivs: Spread {
                hp: 31,
                attack: 0,
                defense: 15,
                speed: 7,
                special_attack: 20,
                special_defense: 1,
            },
            evs: Spread {
                hp: 4,
                attack: 252,
                defense: 0,
                speed: 252,
                special_attack: 0,
                special_defense: 0,
            },
            ability_slot: 0,
            pokerus: 0,
            met: Met {
                location: 0,
                level: 5,
                game: 3,
                ball: 4,
                ot_gender: Gender::Male,
            },
            party: None,
        }
    }

    #[test]
    fn identifies_each_game_from_its_header() {
        let cases = [
            (b"AXVE", Version::Ruby, Language::English),
            (b"AXPD", Version::Sapphire, Language::German),
            (b"BPEJ", Version::Emerald, Language::Japanese),
            (b"BPRF", Version::FireRed, Language::French),
            (b"BPGS", Version::LeafGreen, Language::Spanish),
            (b"BPEI", Version::Emerald, Language::Italian),
        ];
        for (code, version, language) in cases {
            let rom = gen3_rom(code, 2, SPOTS);
            assert_eq!(
                identify(&rom),
                Some(Game {
                    version,
                    language,
                    revision: 2,
                })
            );
        }
    }

    #[test]
    fn other_roms_are_not_identified() {
        assert_eq!(identify(&gen3_rom(b"BPEX", 0, SPOTS)), None);
        assert_eq!(identify(&gen3_rom(b"AGBE", 0, SPOTS)), None);
        assert_eq!(identify(&[0; 0xBF]), None);
        let mut gb = vec![0; 0x8000];
        gb[0x134..0x13F].copy_from_slice(b"POKEMON RED");
        assert_eq!(identify(&gb), None);
    }

    #[test]
    fn finds_the_name_tables_wherever_they_sit() {
        for at in [SPOTS, MOVED] {
            let tables = tables(at);
            assert_eq!(tables.species_name(1), Some("BULBASAUR"));
            assert_eq!(tables.species_name(277), Some("TREECKO"));
            assert_eq!(tables.species_name(411), Some("CHIMECHO"));
            assert_eq!(tables.move_name(1), Some("POUND"));
            assert_eq!(tables.move_name(354), Some("MOVE354"));
            assert_eq!(tables.move_name(0), None);
            assert_eq!(tables.move_name(355), None);
            assert_eq!(tables.ability_name(1), Some("STENCH"));
            assert_eq!(tables.ability_name(77), Some("SKILL77"));
            assert_eq!(tables.type_name(0), Some("NORMAL"));
            assert_eq!(tables.type_name(9), Some("???"));
            assert_eq!(tables.type_name(17), Some("DARK"));
            assert_eq!(tables.item_name(1), Some("MASTER BALL"));
            assert_eq!(tables.item_name(ITEMS as u16 - 1), Some("ITEM376"));
            assert_eq!(tables.item_name(ITEMS as u16), None);
        }
    }

    #[test]
    fn maps_internal_species_to_national_numbers() {
        let tables = tables(SPOTS);
        for species in 0..SPECIES as u16 {
            let national = national_for(species);
            let expected = (1..=386).contains(&national).then_some(national);
            assert_eq!(tables.national_number(species), expected, "{species}");
            if let Some(national) = expected {
                assert_eq!(tables.species_for_national(national), Some(species));
            }
        }
        assert_eq!(tables.national_number(277), Some(252));
        assert_eq!(tables.national_number(411), Some(358));
        assert_eq!(tables.species_for_national(0), None);
        assert_eq!(tables.species_for_national(387), None);
        assert_eq!(tables.pokedex_size(), 386);
    }

    #[test]
    fn placeholder_species_have_no_name_or_stats() {
        let tables = tables(SPOTS);
        for species in [0, 252, 260, 276, 412] {
            assert_eq!(tables.species_name(species), None, "{species}");
            assert_eq!(tables.base_stats(species), None, "{species}");
        }
        assert_eq!(species_name(260), "MON260");
    }

    #[test]
    fn reads_base_stats_by_species() {
        let tables = tables(MOVED);
        let base = tables.base_stats(6).expect("base");
        assert_eq!((base.hp, base.attack, base.special_defense), (51, 49, 65));
        assert_eq!(base.types, [6, 9]);
        assert_eq!(
            (base.catch_rate, base.exp_yield, base.gender_ratio),
            (45, 64, 31)
        );
        assert_eq!(base.growth, crate::gen3::Growth::MediumFast);
        assert_eq!(base.abilities, [7, 0]);
        assert_eq!(tables.species_types(6), vec!["TYPE6", "???"]);
        assert_eq!(tables.species_types(15), vec!["TYPE15", "NORMAL"]);
    }

    #[test]
    fn box_mons_get_their_level_from_the_growth_rate() {
        let tables = tables(SPOTS);
        let mut boxed = mon(6, 3, 125_000);
        assert_eq!(tables.mon_level(&boxed), Some(50));
        assert_eq!(tables.exp_to_next_level(&boxed), Some(132_651 - 125_000));
        boxed.exp = 124_999;
        assert_eq!(tables.mon_level(&boxed), Some(49));
        boxed.level = Some(77);
        assert_eq!(tables.mon_level(&boxed), Some(77));
        let erratic = mon(7, 3, 600_000);
        assert_eq!(tables.mon_level(&erratic), Some(100));
        assert_eq!(tables.exp_to_next_level(&erratic), None);
        assert_eq!(tables.mon_level(&mon(260, 3, 1000)), None);
    }

    #[test]
    fn computes_box_stats_with_the_nature() {
        let tables = tables(SPOTS);
        let adamant = mon(6, 3, 125_000);
        assert_eq!(adamant.nature(), crate::gen3::Nature::Adamant);
        assert_eq!(
            tables.mon_stats(&adamant),
            Some(Stats {
                max_hp: 127,
                attack: 93,
                defense: 61,
                speed: 85,
                special_attack: 72,
                special_defense: 70,
            })
        );
        let hardy = mon(6, 0, 125_000);
        let stats = tables.mon_stats(&hardy).expect("stats");
        assert_eq!((stats.attack, stats.special_attack), (85, 80));
        let shedinja = mon(SHEDINJA, 3, 1_000_000);
        assert_eq!(tables.national_number(SHEDINJA), Some(292));
        assert_eq!(tables.mon_stats(&shedinja).map(|s| s.max_hp), Some(1));
    }

    #[test]
    fn party_mons_keep_their_stored_stats() {
        let tables = tables(SPOTS);
        let stats = Stats {
            max_hp: 1,
            attack: 2,
            defense: 3,
            speed: 4,
            special_attack: 5,
            special_defense: 6,
        };
        let mut party = mon(6, 3, 125_000);
        party.level = Some(12);
        party.party = Some(PartyStats {
            status: 0,
            hp: 1,
            stats,
        });
        assert_eq!(tables.mon_stats(&party), Some(stats));
        assert_eq!(tables.mon_level(&party), Some(12));
    }

    #[test]
    fn gender_follows_the_ratio_and_personality() {
        let tables = tables(SPOTS);
        assert_eq!(tables.mon_gender(&mon(6, 30, 0)), Some(Gender::Female));
        assert_eq!(tables.mon_gender(&mon(6, 31, 0)), Some(Gender::Male));
        assert_eq!(tables.mon_gender(&mon(6, 0x100, 0)), Some(Gender::Female));
        assert_eq!(tables.mon_gender(&mon(5, 0, 0)), Some(Gender::Male));
        assert_eq!(tables.mon_gender(&mon(8, 0xFF, 0)), Some(Gender::Female));
        assert_eq!(tables.mon_gender(&mon(9, 0, 0)), None);
    }

    #[test]
    fn the_ability_slot_picks_the_ability() {
        let tables = tables(SPOTS);
        let mut first = mon(7, 0, 0);
        assert_eq!(tables.mon_ability(&first), Some("SKILL8"));
        first.ability_slot = 1;
        assert_eq!(tables.mon_ability(&first), Some("SKILL2"));
        let mut single = mon(6, 0, 0);
        single.ability_slot = 1;
        assert_eq!(tables.mon_ability(&single), Some("SKILL7"));
    }

    #[test]
    fn a_missing_table_fails_the_load() {
        let base = gen3_rom(b"AXVE", 0, SPOTS);
        let cuts = [
            (SPOTS.species, 11),
            (SPOTS.moves + 13, 13),
            (SPOTS.abilities, 13),
            (SPOTS.types + 9 * 7, 7),
            (SPOTS.items + 44 + 14, 2),
            (SPOTS.national, 2),
            (SPOTS.base + 28 + 26, 2),
        ];
        for (cut, len) in cuts {
            let mut rom = base.clone();
            rom[cut..cut + len].fill(0x11);
            assert_eq!(RomTables::load(&rom), Err(ReadError::RomTables), "{cut:#x}");
        }
        assert_eq!(
            RomTables::load(&gen3_rom(b"ABCD", 0, SPOTS)),
            Err(ReadError::RomTables)
        );
    }

    #[test]
    fn truncated_roms_never_panic() {
        let rom = gen3_rom(b"BPRE", 1, MOVED);
        for len in [0, 0xC0, 0x1000, 0x2_1000, 0x3_1100, 0x3_3D00] {
            assert!(RomTables::load(&rom[..len]).is_err(), "{len:#x}");
        }
    }
}
