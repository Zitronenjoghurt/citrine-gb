mod base;
mod flash;
mod fold;
mod layout;
mod live;
mod mon;
mod pics;
mod region_map;
mod rom;
pub mod text;
mod wild;

use crate::game::{Language, Version};
use crate::state::{Gender, PlayTime, Pokedex, Pokerus, SaveCopy, Stats};
use crate::{ReadError, list};
use flash::Buffers;
use layout::Offsets;

pub use base::BaseStats;
pub use live::Memory;
pub use mon::Growth;
pub use pics::{Picture, SIDE as PICTURE_SIDE};
pub use region_map::{RegionMaps, Warp, Whereabouts};
pub use rom::{RomTables, identify};

pub const FLASH_SIZE: usize = 0x20000;
const DEX: u16 = 386;
const BOXES: usize = 14;
const BOX_SLOTS: usize = 30;
const PARTY_SLOTS: usize = 6;
const BADGES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layout {
    RubySapphire,
    Emerald,
    FireRedLeafGreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Certainty {
    Sure,
    Likely,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guess {
    pub layout: Layout,
    pub certainty: Certainty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveState {
    pub layout: Layout,
    pub language: Language,
    pub trainer: Trainer,
    pub party: Vec<Pokemon>,
    pub boxes: Vec<PcBox>,
    pub current_box: usize,
    pub bag: Bag,
    pub pc_items: Vec<ItemStack>,
    pub pokedex: Pokedex,
    pub whereabouts: Whereabouts,
    pub badges: Vec<bool>,
    pub pokedex_received: bool,
    pub champion: bool,
    pub copy: SaveCopy,
    pub save_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trainer {
    pub name: String,
    pub gender: Gender,
    pub id: u16,
    pub secret_id: u16,
    pub money: u32,
    pub coins: u16,
    pub play_time: PlayTime,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcBox {
    pub number: usize,
    pub name: String,
    pub slots: Vec<Option<Pokemon>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pokemon {
    BadEgg,
    Egg(Box<Mon>),
    Hatched(Box<Mon>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mon {
    pub species: u16,
    pub personality: u32,
    pub ot_id: u16,
    pub ot_secret_id: u16,
    pub nickname: String,
    pub ot_name: String,
    pub language: Option<Language>,
    pub held_item: Option<u16>,
    pub exp: u32,
    pub level: Option<u8>,
    pub friendship: u8,
    pub moves: Vec<MoveSlot>,
    pub ivs: Spread,
    pub evs: Spread,
    pub ability_slot: u8,
    pub pokerus: u8,
    pub met: Met,
    pub party: Option<PartyStats>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveSlot {
    pub id: u16,
    pub pp: u8,
    pub pp_ups: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Spread {
    pub hp: u8,
    pub attack: u8,
    pub defense: u8,
    pub speed: u8,
    pub special_attack: u8,
    pub special_defense: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Met {
    pub location: u8,
    pub level: u8,
    pub game: u8,
    pub ball: u8,
    pub ot_gender: Gender,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartyStats {
    pub status: u32,
    pub hp: u16,
    pub stats: Stats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nature {
    Hardy,
    Lonely,
    Brave,
    Adamant,
    Naughty,
    Bold,
    Docile,
    Relaxed,
    Impish,
    Lax,
    Timid,
    Hasty,
    Serious,
    Jolly,
    Naive,
    Modest,
    Mild,
    Quiet,
    Bashful,
    Rash,
    Calm,
    Gentle,
    Sassy,
    Careful,
    Quirky,
}

impl Layout {
    pub fn of(version: Version) -> Option<Layout> {
        match version {
            Version::Ruby | Version::Sapphire => Some(Layout::RubySapphire),
            Version::Emerald => Some(Layout::Emerald),
            Version::FireRed | Version::LeafGreen => Some(Layout::FireRedLeafGreen),
            Version::Red
            | Version::Green
            | Version::Blue
            | Version::Yellow
            | Version::Gold
            | Version::Silver
            | Version::Crystal => None,
        }
    }
}

impl Mon {
    pub fn nature(&self) -> Nature {
        Nature::from_personality(self.personality)
    }

    pub fn shiny(&self) -> bool {
        let p = self.personality;
        (self.ot_id ^ self.ot_secret_id ^ (p >> 16) as u16 ^ p as u16) < 8
    }

    pub fn pokerus_status(&self) -> Option<Pokerus> {
        Pokerus::from_byte(self.pokerus)
    }

    pub fn level_with(&self, growth: Growth) -> u8 {
        self.level.unwrap_or_else(|| growth.level(self.exp))
    }
}

impl Nature {
    const ALL: [Nature; 25] = [
        Nature::Hardy,
        Nature::Lonely,
        Nature::Brave,
        Nature::Adamant,
        Nature::Naughty,
        Nature::Bold,
        Nature::Docile,
        Nature::Relaxed,
        Nature::Impish,
        Nature::Lax,
        Nature::Timid,
        Nature::Hasty,
        Nature::Serious,
        Nature::Jolly,
        Nature::Naive,
        Nature::Modest,
        Nature::Mild,
        Nature::Quiet,
        Nature::Bashful,
        Nature::Rash,
        Nature::Calm,
        Nature::Gentle,
        Nature::Sassy,
        Nature::Careful,
        Nature::Quirky,
    ];

    const NAMES: [&str; 25] = [
        "Hardy", "Lonely", "Brave", "Adamant", "Naughty", "Bold", "Docile", "Relaxed", "Impish",
        "Lax", "Timid", "Hasty", "Serious", "Jolly", "Naive", "Modest", "Mild", "Quiet", "Bashful",
        "Rash", "Calm", "Gentle", "Sassy", "Careful", "Quirky",
    ];

    pub fn from_personality(personality: u32) -> Self {
        Self::ALL[(personality % 25) as usize]
    }

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }

    pub fn raised_and_lowered(self) -> Option<(usize, usize)> {
        let index = self as usize;
        let (up, down) = (index / 5, index % 5);
        (up != down).then_some((up, down))
    }
}

pub fn guess_layout(flash: &[u8]) -> Option<Guess> {
    flash::guess(flash)
}

pub fn read_save(flash: &[u8], layout: Layout, language: Language) -> Result<SaveState, ReadError> {
    let (buffers, copy) = flash::read(flash, layout)?;
    Ok(state(&buffers, copy, layout, language))
}

pub fn read_live(
    memory: Memory,
    layout: Layout,
    language: Language,
) -> Result<SaveState, ReadError> {
    let buffers = live::read(memory, layout, language)?;
    Ok(state(&buffers, SaveCopy::Live, layout, language))
}

fn state(buffers: &Buffers, copy: SaveCopy, layout: Layout, language: Language) -> SaveState {
    let offsets = Offsets::of(layout);
    let small = &buffers.small;
    let large = &buffers.large;
    let key = offsets.key.map_or(0, |at| u32_at(small, at));
    let short_key = key as u16;
    let party_count = usize::from(large[offsets.team_size]).min(PARTY_SLOTS);
    let party = (0..party_count)
        .filter_map(|i| mon::read(&large[offsets.team + i * mon::PARTY_SIZE..][..mon::PARTY_SIZE]))
        .collect();
    let pocket = |(at, slots): (usize, usize)| items(&large[at..][..slots * 4], short_key);
    let flag = |index: usize| list::flag(&large[offsets.flags..], index);
    SaveState {
        layout,
        language,
        trainer: Trainer {
            name: text::decode(&small[..layout::NAME_LEN], language),
            gender: if small[0x08] == 1 {
                Gender::Female
            } else {
                Gender::Male
            },
            id: u16_at(small, 0x0A),
            secret_id: u16_at(small, 0x0C),
            money: u32_at(large, offsets.money) ^ key,
            coins: u16_at(large, offsets.money + 4) ^ short_key,
            play_time: PlayTime {
                hours: u16_at(small, 0x0E),
                minutes: small[0x10],
                seconds: small[0x11],
            },
        },
        party,
        boxes: boxes(&buffers.pc, language),
        current_box: usize::from(buffers.pc[0]).min(BOXES - 1),
        bag: Bag {
            items: pocket(offsets.items),
            key_items: pocket(offsets.key_items),
            balls: pocket(offsets.balls),
            tms_hms: pocket(offsets.tms_hms),
            berries: pocket(offsets.berries),
        },
        pc_items: items(&large[offsets.pc_items.0..][..offsets.pc_items.1 * 4], 0),
        pokedex: Pokedex {
            seen: list::flags(&small[layout::DEX_SEEN..][..layout::DEX_LEN], DEX),
            caught: list::flags(&small[layout::DEX_OWNED..][..layout::DEX_LEN], DEX),
        },
        whereabouts: Whereabouts::read(large),
        badges: (0..BADGES).map(|i| flag(offsets.badge_flag + i)).collect(),
        pokedex_received: flag(offsets.pokedex_flag),
        champion: flag(offsets.champion_flag),
        copy,
        save_index: buffers.index,
    }
}

fn items(bytes: &[u8], key: u16) -> Vec<ItemStack> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|slot| ItemStack {
            item: u16_at(slot, 0),
            count: u16_at(slot, 2) ^ key,
        })
        .filter(|stack| stack.item != 0)
        .collect()
}

fn boxes(pc: &[u8], language: Language) -> Vec<PcBox> {
    (0..BOXES)
        .map(|number| {
            let at = layout::BOX_DATA + number * BOX_SLOTS * mon::BOX_SIZE;
            PcBox {
                number: number + 1,
                name: text::decode(&pc[layout::BOX_NAMES + number * 9..][..9], language),
                slots: (0..BOX_SLOTS)
                    .map(|slot| mon::read(&pc[at + slot * mon::BOX_SIZE..][..mon::BOX_SIZE]))
                    .collect(),
            }
        })
        .collect()
}

pub(crate) fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

pub(crate) fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::Nature;

    #[test]
    fn natures_have_names_and_stat_changes() {
        assert_eq!(Nature::from_personality(0x1234_5678), Nature::Gentle);
        assert_eq!(Nature::Gentle.name(), "Gentle");
        assert_eq!(Nature::Gentle.raised_and_lowered(), Some((4, 1)));
        assert_eq!(Nature::Hardy.raised_and_lowered(), None);
    }
}
