use crate::ReadError;
use crate::base::{self, BaseStats};
use crate::game::{GLOBAL_CHECKSUM, Game, Generation};
use crate::layout::Layout;
use crate::names;
use crate::pic::{Sprite, SpriteError};
use crate::sprite::{Palette, Side, SpriteTables};
use crate::state::{Caught, Gender, Mon, Stats};
use crate::text::{self, TERMINATOR};

pub(crate) const BANK_SIZE: usize = 0x4000;
const MAX_NAME_BYTES: usize = 16;
const GEN1_SPECIES: usize = 190;
const GEN1_DEX: u16 = 151;
const GEN2_SPECIES: usize = 251;
const UNOWN: u16 = 201;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomTables {
    pub game: Game,
    species_names: Vec<String>,
    move_names: Vec<String>,
    item_names: Vec<String>,
    dex_order: Vec<u8>,
    pub(crate) live: Vec<LiveBlock>,
    pub sprites: SpriteTables,
    base_stats: Vec<BaseStats>,
    type_names: Vec<String>,
    landmarks: Vec<String>,
    machines: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place<'a> {
    Landmark(&'a str),
    Gift,
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiveBlock {
    pub wram: u16,
    pub sram: usize,
    pub len: usize,
}

struct NameSources {
    species: usize,
    moves: usize,
    items: usize,
}

impl RomTables {
    pub fn load(game: Game, rom: &[u8]) -> Result<Self, ReadError> {
        let sources = match game.generation() {
            Generation::One => gen1_sources(game, rom),
            Generation::Two => gen2_sources(game, rom),
        }
        .ok_or(ReadError::RomTables)?;
        let (species_count, move_count, item_count) = match game.generation() {
            Generation::One => (GEN1_SPECIES, 165, 83),
            Generation::Two => (GEN2_SPECIES, 251, 249),
        };
        let species_names =
            fixed_names(game, rom, sources.species, species_count).ok_or(ReadError::RomTables)?;
        if game.is_japanese()
            && (!global_checksum_holds(rom)
                || spelled_in_latin(game, rom, sources.species, species_count))
        {
            return Err(ReadError::RomTables);
        }
        let move_names =
            name_list(game, rom, sources.moves, move_count).ok_or(ReadError::RomTables)?;
        let item_names =
            name_list(game, rom, sources.items, item_count).ok_or(ReadError::RomTables)?;
        let dex_order = match game.generation() {
            Generation::One => find_dex_order(rom).ok_or(ReadError::RomTables)?,
            Generation::Two => Vec::new(),
        };
        let live = live_blocks(rom, Layout::of(game).live_starts);
        let base_stats = base::load(game, rom);
        let sprites = SpriteTables::load(game, rom, &base_stats, &dex_order);
        Ok(Self {
            game,
            species_names,
            move_names,
            item_names,
            dex_order,
            live,
            sprites,
            base_stats,
            type_names: names::type_names(game, rom),
            landmarks: names::landmark_names(game, rom),
            machines: find_machines(game, rom),
        })
    }

    pub fn base_stats(&self, species: u8) -> Option<&BaseStats> {
        let dex = self.dex_number(species)?;
        self.base_stats.get(usize::from(dex) - 1)
    }

    pub fn type_name(&self, id: u8) -> Option<&str> {
        self.type_names.get(usize::from(id)).map(String::as_str)
    }

    pub fn species_types(&self, species: u8) -> Vec<&str> {
        let Some(base) = self.base_stats(species) else {
            return Vec::new();
        };
        let mut types: Vec<&str> = base
            .types
            .iter()
            .filter_map(|&id| self.type_name(id))
            .filter(|name| !name.is_empty())
            .collect();
        types.dedup();
        types
    }

    pub fn sprite(
        &self,
        rom: &[u8],
        species: u8,
        unown_letter: Option<char>,
        side: Side,
    ) -> Result<Sprite, SpriteError> {
        let dex = self.dex_number(species).ok_or(SpriteError::NoSpecies)?;
        let unown = (self.game.generation() == Generation::Two && dex == UNOWN)
            .then_some(unown_letter.unwrap_or('A'))
            .and_then(|letter| {
                letter
                    .is_ascii_uppercase()
                    .then(|| letter as usize - 'A' as usize)
            });
        self.sprites.sprite(rom, dex, unown, side)
    }

    pub fn palette(&self, species: u8, shiny: bool) -> Option<Palette> {
        self.sprites.palette(self.dex_number(species)?, shiny)
    }

    pub fn mon_sprite(&self, rom: &[u8], mon: &Mon, side: Side) -> Result<Sprite, SpriteError> {
        self.sprite(rom, mon.species, Some(mon.dvs.unown_letter()), side)
    }

    pub fn mon_palette(&self, mon: &Mon) -> Option<Palette> {
        let shiny = self.game.generation() == Generation::Two && mon.dvs.shiny();
        self.palette(mon.species, shiny)
    }

    pub fn mon_stats(&self, mon: &Mon) -> Option<Stats> {
        let base = self.base_stats(mon.species)?;
        Some(base.stats(mon.level, mon.dvs, mon.stat_exp))
    }

    pub fn mon_gender(&self, mon: &Mon) -> Option<Gender> {
        self.base_stats(mon.species)?.gender(mon.dvs)
    }

    pub fn exp_to_next_level(&self, mon: &Mon) -> Option<u32> {
        self.base_stats(mon.species)?
            .exp_to_next_level(mon.level, mon.exp)
    }

    pub fn caught_place(&self, caught: &Caught) -> Option<Place<'_>> {
        match caught.location {
            0x7E => Some(Place::Gift),
            0x7F => Some(Place::Event),
            location => self
                .landmarks
                .get(usize::from(location))
                .map(|name| Place::Landmark(name)),
        }
    }

    pub fn species_name(&self, species: u8) -> Option<&str> {
        let index = usize::from(species).checked_sub(1)?;
        self.species_names.get(index).map(String::as_str)
    }

    pub fn dex_number(&self, species: u8) -> Option<u16> {
        match self.game.generation() {
            Generation::One => {
                let index = usize::from(species).checked_sub(1)?;
                let dex = *self.dex_order.get(index)?;
                (dex != 0).then_some(u16::from(dex))
            }
            Generation::Two => (1..=GEN2_SPECIES as u16)
                .contains(&u16::from(species))
                .then_some(u16::from(species)),
        }
    }

    pub fn species_for_dex(&self, dex: u16) -> Option<u8> {
        match self.game.generation() {
            Generation::One => self
                .dex_order
                .iter()
                .position(|&d| d != 0 && u16::from(d) == dex)
                .map(|index| index as u8 + 1),
            Generation::Two => (1..=GEN2_SPECIES as u16)
                .contains(&dex)
                .then_some(dex as u8),
        }
    }

    pub fn move_name(&self, id: u8) -> Option<&str> {
        let index = usize::from(id).checked_sub(1)?;
        self.move_names.get(index).map(String::as_str)
    }

    pub fn item_name(&self, id: u8) -> Option<String> {
        if self.game.generation() == Generation::One {
            match id {
                0xC4..=0xC8 => return Some(format!("HM{:02}", id - 0xC3)),
                0xC9..=0xFF => return Some(format!("TM{:02}", id - 0xC8)),
                _ => {}
            }
        }
        let index = usize::from(id).checked_sub(1)?;
        self.item_names.get(index).cloned()
    }

    pub fn machine_move(&self, item: u8) -> Option<&str> {
        let index = match (self.game.generation(), item) {
            (Generation::One, 0xC4..=0xC8) => item - 0xC4 + 50,
            (Generation::One, 0xC9..=0xFA) => item - 0xC9,
            (Generation::Two, 0xBF..=0xC2) => item - 0xBF,
            (Generation::Two, 0xC4..=0xDB) => item - 0xC0,
            (Generation::Two, 0xDD..=0xF9) => item - 0xC1,
            _ => return None,
        };
        self.move_name(*self.machines.get(usize::from(index))?)
    }

    pub fn is_hm(&self, item: u8) -> bool {
        match self.game.generation() {
            Generation::One => (0xC4..=0xC8).contains(&item),
            Generation::Two => (0xF3..=0xF9).contains(&item),
        }
    }

    pub fn live_map(&self) -> Vec<(u16, usize)> {
        self.live
            .iter()
            .map(|block| (block.wram, block.sram))
            .collect()
    }

    pub fn pokedex_size(&self) -> u16 {
        match self.game.generation() {
            Generation::One => GEN1_DEX,
            Generation::Two => GEN2_SPECIES as u16,
        }
    }
}

pub(crate) fn rom_offset(bank: usize, addr: u16) -> usize {
    if addr < 0x4000 {
        usize::from(addr)
    } else {
        bank * BANK_SIZE + usize::from(addr) - BANK_SIZE
    }
}

pub(crate) fn word(rom: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([rom[at], rom[at + 1]])
}

pub(crate) fn in_rom_bank(addr: u16) -> bool {
    (0x4000..0x8000).contains(&addr)
}

fn in_wram(addr: u16) -> bool {
    (0xC000..0xE000).contains(&addr)
}

fn gen2_sources(game: Game, rom: &[u8]) -> Option<NameSources> {
    let home = &rom[..BANK_SIZE.min(rom.len())];
    (0..home.len().saturating_sub(24)).find_map(|at| {
        let entry = |k: usize| (home[at + 3 * k], word(home, at + 3 * k + 1));
        let far = |k: usize| entry(k).0 != 0 && in_rom_bank(entry(k).1);
        let wram = |k: usize| entry(k).0 == 0 && in_wram(entry(k).1);
        if entry(2) != (0, 0) || !wram(4) || !wram(5) || ![0, 1, 3, 6].into_iter().all(far) {
            return None;
        }
        let at_far = |k: usize| rom_offset(usize::from(entry(k).0), entry(k).1);
        let sources = NameSources {
            species: at_far(0),
            moves: at_far(1),
            items: at_far(3),
        };
        fixed_names(game, rom, sources.species, GEN2_SPECIES)?;
        Some(sources)
    })
}

fn gen1_sources(game: Game, rom: &[u8]) -> Option<NameSources> {
    let home = &rom[..BANK_SIZE.min(rom.len())];
    let banks = rom.len() / BANK_SIZE;
    (0..home.len().saturating_sub(14)).find_map(|at| {
        let entry = |k: usize| word(home, at + 2 * k);
        if !in_wram(entry(4)) || !in_wram(entry(5)) {
            return None;
        }
        if ![0, 1, 2, 3, 6].into_iter().all(|k| in_rom_bank(entry(k))) {
            return None;
        }
        let find = |addr: u16, check: &dyn Fn(usize) -> bool| {
            (1..banks)
                .map(|bank| rom_offset(bank, addr))
                .find(|&offset| check(offset))
        };
        let species = find(entry(0), &|o| {
            fixed_names(game, rom, o, GEN1_SPECIES).is_some()
        })?;
        let moves = find(entry(1), &|o| name_list(game, rom, o, 165).is_some())?;
        let items = find(entry(3), &|o| name_list(game, rom, o, 83).is_some())?;
        Some(NameSources {
            species,
            moves,
            items,
        })
    })
}

fn fixed_names(game: Game, rom: &[u8], at: usize, count: usize) -> Option<Vec<String>> {
    let len = game.species_name_length();
    let table = rom.get(at..at + len * count)?;
    table
        .chunks(len)
        .map(|record| {
            let end = record.iter().position(|&b| b == TERMINATOR).unwrap_or(len);
            if !record[end..].iter().all(|&b| b == TERMINATOR) {
                return None;
            }
            text::decode_strict(game, &record[..end])
        })
        .collect()
}

fn global_checksum_holds(rom: &[u8]) -> bool {
    let Some(stored) = rom.get(GLOBAL_CHECKSUM..GLOBAL_CHECKSUM + 2) else {
        return false;
    };
    let sum = rom
        .iter()
        .fold(0u16, |sum, &b| sum.wrapping_add(u16::from(b)))
        .wrapping_sub(u16::from(stored[0]))
        .wrapping_sub(u16::from(stored[1]));
    sum == u16::from_be_bytes([stored[0], stored[1]])
}

fn spelled_in_latin(game: Game, rom: &[u8], at: usize, count: usize) -> bool {
    let len = game.species_name_length();
    let Some(table) = rom.get(at..at + len * count) else {
        return false;
    };
    let latin = |b: &u8| matches!(b, 0x80..=0x99 | 0x7F | 0xE3 | 0xE8 | 0xEF | 0xF5..=0xFF);
    let spelled = table
        .chunks(len)
        .filter(|record| {
            let end = record.iter().position(|&b| b == TERMINATOR).unwrap_or(len);
            record[..end].iter().all(latin)
        })
        .count();
    spelled * 10 >= count * 9
}

pub(crate) fn name_list(
    game: Game,
    rom: &[u8],
    mut at: usize,
    count: usize,
) -> Option<Vec<String>> {
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        let window = rom.get(at..(at + MAX_NAME_BYTES).min(rom.len()))?;
        let end = window.iter().position(|&b| b == TERMINATOR)?;
        names.push(text::decode_strict(game, &window[..end])?);
        at += end + 1;
    }
    Some(names)
}

fn find_machines(game: Game, rom: &[u8]) -> Vec<u8> {
    let (start, count): (&[u8], usize) = match game.generation() {
        Generation::One => (&[5, 13, 14, 18, 25, 92], 55),
        Generation::Two => (&[223, 29, 174, 205, 46, 92], 57),
    };
    rom.windows(start.len())
        .position(|window| window == start)
        .and_then(|at| rom.get(at..at + count))
        .map_or_else(Vec::new, <[u8]>::to_vec)
}

fn find_dex_order(rom: &[u8]) -> Option<Vec<u8>> {
    let mut window = DexWindow {
        counts: [0; 256],
        singles: 0,
    };
    let table = rom.get(..GEN1_SPECIES)?;
    table.iter().for_each(|&b| window.add(b));
    for start in 0.. {
        if window.is_dex_order() {
            return Some(rom[start..start + GEN1_SPECIES].to_vec());
        }
        let next = *rom.get(start + GEN1_SPECIES)?;
        window.remove(rom[start]);
        window.add(next);
    }
    None
}

struct DexWindow {
    counts: [u16; 256],
    singles: u16,
}

impl DexWindow {
    fn add(&mut self, value: u8) {
        self.change(value, 1);
    }

    fn remove(&mut self, value: u8) {
        self.change(value, -1);
    }

    fn change(&mut self, value: u8, delta: i16) {
        let count = &mut self.counts[usize::from(value)];
        let was_single = *count == 1;
        *count = count.wrapping_add_signed(delta);
        let is_single = *count == 1;
        if (1..=GEN1_DEX).contains(&u16::from(value)) && was_single != is_single {
            self.singles = if is_single {
                self.singles + 1
            } else {
                self.singles - 1
            };
        }
    }

    fn is_dex_order(&self) -> bool {
        self.singles == GEN1_DEX
            && usize::from(self.counts[0]) == GEN1_SPECIES - usize::from(GEN1_DEX)
    }
}

fn live_blocks(rom: &[u8], starts: &[usize]) -> Vec<LiveBlock> {
    let mut found: Vec<LiveBlock> = Vec::new();
    for at in 0..rom.len().saturating_sub(10) {
        if rom[at] != 0x21 || rom[at + 3] != 0x11 || rom[at + 6] != 0x01 || rom[at + 9] != 0xCD {
            continue;
        }
        let source = word(rom, at + 1);
        let dest = word(rom, at + 4);
        let len = usize::from(word(rom, at + 7));
        if !in_wram(source) || !(0xA000..0xC000).contains(&dest) {
            continue;
        }
        let sram = 0x2000 + usize::from(dest - 0xA000);
        if !starts.contains(&sram) {
            continue;
        }
        match found.iter_mut().find(|block| block.sram == sram) {
            Some(block) if block.len >= len => {}
            Some(block) => {
                *block = LiveBlock {
                    wram: source,
                    sram,
                    len,
                }
            }
            None => found.push(LiveBlock {
                wram: source,
                sram,
                len,
            }),
        }
    }
    found.sort_by_key(|block| block.sram);
    found
}
