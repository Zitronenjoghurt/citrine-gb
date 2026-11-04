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
    use crate::fixtures::gen3_rom::{MOVED, PICTURE_BYTE, SPOTS, gen3_rom};

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
}
