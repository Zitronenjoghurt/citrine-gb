use crate::eeprom::{self, Block, Eeprom};
use crate::info::{PokemonSummary, WalkerInfo};

pub(crate) const HAPPINESS: usize = eeprom::ROUTE + 0x26;
pub(crate) const NICKNAME: usize = eeprom::ROUTE + 0x10;
const ROUTE_IMAGE: usize = eeprom::ROUTE + 0x27;
pub(crate) const ROUTE_NAME: usize = eeprom::ROUTE + 0x28;
const ROUTE_POKEMON: usize = eeprom::ROUTE + 0x52;
const ROUTE_POKEMON_STEPS: usize = eeprom::ROUTE + 0x82;
const ROUTE_POKEMON_PERCENT: usize = eeprom::ROUTE + 0x88;
const ROUTE_ITEMS: usize = eeprom::ROUTE + 0x8C;
const ROUTE_ITEM_STEPS: usize = eeprom::ROUTE + 0xA0;
const ROUTE_ITEM_PERCENT: usize = eeprom::ROUTE + 0xB4;
pub(crate) const ROUTE_ITEM_COUNT: usize = 10;

const SPECIAL_ROUTE_IMAGE: usize = 0xBF06;
const EVENT_POKEMON: usize = 0xBF08;
const EVENT_POKEMON_STEPS: usize = 0xBF44;
const EVENT_POKEMON_PERCENT: usize = 0xBF46;
const EVENT_ITEM: usize = 0xBF48;
const EVENT_ITEM_STEPS: usize = 0xBF4A;
const EVENT_ITEM_PERCENT: usize = 0xBF4C;
const SPECIAL_ROUTE_NAME: usize = 0xBF50;
const EVENT_POKEMON_NUMBER: usize = 0xBF7A;
const EVENT_ITEM_NUMBER: usize = 0xBF7B;
const POKEMON_RECEIVED: u8 = 5;
const ITEM_RECEIVED: u8 = 6;
const CAUGHT_EVENT_POKEMON: usize = 0xBA44;
const RECEIVED_EVENT_ITEM: usize = 0xBD40;

pub(crate) const ITEMS: usize = 0xCEBC;
pub(crate) const HISTORY: usize = 0xCEF0;
const ENTRY_SIZE: usize = 0x88;
const PEER_SIZE: usize = 548;
const PEER_IDENTITY: usize = 8;
const PEER_IDENTITY_SIZE: usize = 0x28;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Found {
    Route(u8),
    Event,
    GroupC,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Chance {
    pub(crate) steps: u32,
    pub(crate) percent: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum LogKind {
    ItemDowsed = 0x0B,
    SpecialItemDowsed = 0x0C,
    PokemonCaught = 0x0D,
    SpecialPokemonCaught = 0x0E,
    PokemonRan = 0x0F,
    PokemonLost = 0x10,
    FoundItem = 0x11,
    FoundBigWatts = 0x12,
    FoundMidWatts = 0x13,
    FoundSmallWatts = 0x14,
    FoundNothing = 0x15,
    PokemonJoined = 0x17,
    WalkStart = 0x19,
    Hour = 0x1B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LogEntry {
    pub(crate) kind: LogKind,
    pub(crate) time: u32,
    pub(crate) other_species: u16,
    pub(crate) extra: u16,
    pub(crate) other: Option<PokemonSummary>,
    pub(crate) hour_steps: u16,
    pub(crate) today_steps: u32,
    pub(crate) special: bool,
}

fn le16(eeprom: &Eeprom, at: usize) -> u16 {
    let bytes = eeprom.read(at, 2);
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn byte(eeprom: &Eeprom, at: usize) -> u8 {
    eeprom.read(at, 1)[0]
}

fn summary(eeprom: &Eeprom, at: usize) -> PokemonSummary {
    let bytes: [u8; PokemonSummary::SIZE] = eeprom
        .read(at, PokemonSummary::SIZE)
        .try_into()
        .unwrap_or([0; PokemonSummary::SIZE]);
    PokemonSummary::from_bytes(&bytes)
}

fn empty_species(species: u16) -> bool {
    species == 0 || species == 0xFFFF
}

pub(crate) fn walking_species(eeprom: &Eeprom) -> u16 {
    le16(eeprom, eeprom::ROUTE)
}

pub(crate) fn has_walking_pokemon(eeprom: &Eeprom) -> bool {
    !empty_species(walking_species(eeprom))
}

pub(crate) fn happiness(eeprom: &Eeprom) -> u8 {
    byte(eeprom, HAPPINESS)
}

pub(crate) fn route_pokemon(eeprom: &Eeprom, slot: usize) -> PokemonSummary {
    summary(eeprom, ROUTE_POKEMON + slot * PokemonSummary::SIZE)
}

pub(crate) fn route_pokemon_chance(eeprom: &Eeprom, slot: usize) -> Chance {
    Chance {
        steps: le16(eeprom, ROUTE_POKEMON_STEPS + 2 * slot) as u32,
        percent: byte(eeprom, ROUTE_POKEMON_PERCENT + slot),
    }
}

pub(crate) fn route_item(eeprom: &Eeprom, slot: usize) -> u16 {
    le16(eeprom, ROUTE_ITEMS + 2 * slot)
}

pub(crate) fn route_item_chance(eeprom: &Eeprom, slot: usize) -> Chance {
    Chance {
        steps: le16(eeprom, ROUTE_ITEM_STEPS + 2 * slot) as u32,
        percent: byte(eeprom, ROUTE_ITEM_PERCENT + slot),
    }
}

fn received(eeprom: &Eeprom, bit: u8) -> bool {
    byte(eeprom, eeprom::RECEIVED) & (1 << bit) != 0
}

fn set_received(eeprom: &mut Eeprom, bit: u8) {
    let value = byte(eeprom, eeprom::RECEIVED) | (1 << bit);
    eeprom.write(eeprom::RECEIVED, &[value]);
}

fn info(eeprom: &Eeprom) -> WalkerInfo {
    WalkerInfo::from_slice(&eeprom.read(Block::Info.address(), Block::Info.size()))
}

fn mark_event(eeprom: &mut Eeprom, number: u8) {
    let mut info = info(eeprom);
    info.mark_event(number);
    eeprom.write_reliable(Block::Info, info.bytes());
}

pub(crate) fn event_pokemon(eeprom: &Eeprom, special: bool) -> Option<(PokemonSummary, Chance)> {
    let number = byte(eeprom, EVENT_POKEMON_NUMBER);
    (special && !received(eeprom, POKEMON_RECEIVED) && !info(eeprom).has_event(number)).then(|| {
        (
            summary(eeprom, EVENT_POKEMON),
            Chance {
                steps: le16(eeprom, EVENT_POKEMON_STEPS) as u32,
                percent: byte(eeprom, EVENT_POKEMON_PERCENT),
            },
        )
    })
}

pub(crate) fn event_item(eeprom: &Eeprom, special: bool) -> Option<(u16, Chance)> {
    let number = byte(eeprom, EVENT_ITEM_NUMBER);
    (special && le16(eeprom, RECEIVED_EVENT_ITEM + 6) == 0 && !info(eeprom).has_event(number)).then(
        || {
            (
                le16(eeprom, EVENT_ITEM),
                Chance {
                    steps: le16(eeprom, EVENT_ITEM_STEPS) as u32,
                    percent: byte(eeprom, EVENT_ITEM_PERCENT),
                },
            )
        },
    )
}

pub(crate) fn found_pokemon(eeprom: &Eeprom, found: Found) -> PokemonSummary {
    match found {
        Found::Route(slot) => route_pokemon(eeprom, slot as usize),
        Found::GroupC => route_pokemon(eeprom, 2),
        Found::Event => summary(eeprom, EVENT_POKEMON),
    }
}

pub(crate) fn caught(eeprom: &Eeprom, max: usize) -> Vec<PokemonSummary> {
    (0..max)
        .map(|slot| summary(eeprom, eeprom::CAUGHT + slot * PokemonSummary::SIZE))
        .filter(|pokemon| !empty_species(pokemon.species))
        .collect()
}

pub(crate) fn caught_slot(eeprom: &Eeprom, slot: usize) -> PokemonSummary {
    summary(eeprom, eeprom::CAUGHT + slot * PokemonSummary::SIZE)
}

pub(crate) fn free_caught_slot(eeprom: &Eeprom, max: usize) -> Option<usize> {
    (0..max).find(|slot| empty_species(caught_slot(eeprom, *slot).species))
}

pub(crate) fn put_caught(eeprom: &mut Eeprom, slot: usize, pokemon: PokemonSummary) {
    eeprom.write(
        eeprom::CAUGHT + slot * PokemonSummary::SIZE,
        &pokemon.to_bytes(),
    );
}

pub(crate) fn put_event_pokemon(eeprom: &mut Eeprom) {
    for (from, to, len) in [
        (EVENT_POKEMON, CAUGHT_EVENT_POKEMON, PokemonSummary::SIZE),
        (EVENT_POKEMON + 0x10, CAUGHT_EVENT_POKEMON + 0x10, 0x2C),
        (0xBF7C, 0xBA80, 0x170),
        (0xC6FC, 0xBC00, 0x140),
    ] {
        eeprom.copy_within(from, to, len);
    }
    set_received(eeprom, POKEMON_RECEIVED);
    let number = byte(eeprom, EVENT_POKEMON_NUMBER);
    mark_event(eeprom, number);
}

pub(crate) fn has_caught_event_pokemon(eeprom: &Eeprom) -> bool {
    received(eeprom, POKEMON_RECEIVED)
}

pub(crate) fn has_event_item(eeprom: &Eeprom) -> bool {
    received(eeprom, ITEM_RECEIVED)
}

pub(crate) fn put_event_item(eeprom: &mut Eeprom, item: u16) {
    eeprom.copy_within(eeprom::SPECIAL_ROUTE, RECEIVED_EVENT_ITEM, 6);
    eeprom.write(RECEIVED_EVENT_ITEM + 6, &item.to_le_bytes());
    eeprom.copy_within(0xCA3C, RECEIVED_EVENT_ITEM + 8, 0x180);
    set_received(eeprom, ITEM_RECEIVED);
    let number = byte(eeprom, EVENT_ITEM_NUMBER);
    mark_event(eeprom, number);
}

pub(crate) fn item_slot(eeprom: &Eeprom, slot: usize) -> u16 {
    le16(eeprom, ITEMS + 4 * slot)
}

pub(crate) fn items(eeprom: &Eeprom, max: usize) -> Vec<u16> {
    (0..max)
        .map(|slot| item_slot(eeprom, slot))
        .filter(|item| *item != 0)
        .collect()
}

pub(crate) fn free_item_slot(eeprom: &Eeprom, max: usize) -> Option<usize> {
    (0..max).find(|slot| item_slot(eeprom, *slot) == 0)
}

pub(crate) fn put_item(eeprom: &mut Eeprom, slot: usize, item: u16) {
    let mut record = [0; 4];
    record[..2].copy_from_slice(&item.to_le_bytes());
    eeprom.write(ITEMS + 4 * slot, &record);
}

pub(crate) fn history(eeprom: &Eeprom, days: usize) -> Vec<u32> {
    eeprom
        .read(HISTORY, 4 * days)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|day| u32::from_be_bytes(*day))
        .collect()
}

pub(crate) fn push_history(eeprom: &mut Eeprom, days: usize, today: u32) {
    let mut history = history(eeprom, days);
    history.rotate_right(1);
    history[0] = today;
    let bytes: Vec<u8> = history.iter().flat_map(|day| day.to_be_bytes()).collect();
    eeprom.write(HISTORY, &bytes);
}

pub(crate) fn forget_met_peers(eeprom: &mut Eeprom, peers: usize) {
    for peer in 0..peers {
        eeprom.fill(
            eeprom::PEERS + peer * PEER_SIZE + PEER_IDENTITY,
            PEER_IDENTITY_SIZE,
            0xFF,
        );
    }
}

pub(crate) fn join(eeprom: &mut Eeprom, happiness: u8) {
    for (from, to, len) in [
        (0x9D7E, 0x91BE, 0x180),
        (0x9EFE, 0x933E, 0x600),
        (0xA77E, 0x993E, 0x140),
    ] {
        eeprom.copy_within(from, to, len);
    }
    let mut pokemon = eeprom.read(
        ROUTE_POKEMON + 2 * PokemonSummary::SIZE,
        PokemonSummary::SIZE,
    );
    pokemon[0x0D] &= 0x7F;
    eeprom.write(eeprom::ROUTE, &pokemon);
    eeprom.fill(NICKNAME, 0x16, 0);
    eeprom.write(HAPPINESS, &[happiness]);
}

pub(crate) fn clear_log_kinds(eeprom: &mut Eeprom) {
    for index in 0..24 {
        eeprom.write(eeprom::EVENT_LOG + index * ENTRY_SIZE + 0x84, &[0]);
    }
}

pub(crate) fn log_kind_at(eeprom: &Eeprom, index: u8) -> u8 {
    byte(
        eeprom,
        eeprom::EVENT_LOG + index as usize * ENTRY_SIZE + 0x84,
    )
}

fn packed(pokemon: &PokemonSummary) -> u8 {
    let [variant, more] = pokemon.flags;
    (variant & 0x7F) | ((more & 0x02) << 6)
}

pub(crate) fn write_log(eeprom: &mut Eeprom, index: u8, entry: LogEntry) {
    let walking = summary(eeprom, eeprom::ROUTE);
    let mut bytes = [0u8; ENTRY_SIZE];
    bytes[0x00..0x04].copy_from_slice(&entry.time.to_be_bytes());
    bytes[0x0A..0x0C].copy_from_slice(&walking.species.to_le_bytes());
    bytes[0x0C..0x0E].copy_from_slice(&entry.other_species.to_le_bytes());
    bytes[0x0E..0x10].copy_from_slice(&entry.extra.to_le_bytes());
    bytes[0x20..0x36].copy_from_slice(&eeprom.read(NICKNAME, 0x16));
    if entry.special {
        bytes[0x4C..0x76].copy_from_slice(&eeprom.read(SPECIAL_ROUTE_NAME, 0x2A));
        bytes[0x76] = byte(eeprom, SPECIAL_ROUTE_IMAGE);
    } else {
        bytes[0x4C..0x76].copy_from_slice(&eeprom.read(ROUTE_NAME, 0x2A));
        bytes[0x76] = byte(eeprom, ROUTE_IMAGE);
    }
    bytes[0x77] = happiness(eeprom);
    bytes[0x78..0x7A].copy_from_slice(&entry.hour_steps.to_be_bytes());
    bytes[0x7C..0x80].copy_from_slice(&entry.today_steps.to_be_bytes());
    bytes[0x84] = entry.kind as u8;
    bytes[0x85] = packed(&walking);
    bytes[0x86] = entry.other.as_ref().map_or(0, packed);
    eeprom.write(eeprom::EVENT_LOG + index as usize * ENTRY_SIZE, &bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_history_shifts_with_the_newest_day_first_in_big_endian() {
        let mut eeprom = Eeprom::blank();
        for day in 1..=8 {
            push_history(&mut eeprom, 7, day * 1000);
        }
        assert_eq!(
            history(&eeprom, 7),
            vec![8000, 7000, 6000, 5000, 4000, 3000, 2000]
        );
        assert_eq!(eeprom.read(0xCEF0, 4), 8000u32.to_be_bytes());
    }

    #[test]
    fn items_take_the_first_free_four_byte_record() {
        let mut eeprom = Eeprom::blank();
        put_item(&mut eeprom, 0, 0x0102);
        assert_eq!(free_item_slot(&eeprom, 3), Some(1));
        put_item(&mut eeprom, 1, 0x0304);
        put_item(&mut eeprom, 2, 0x0506);
        assert_eq!(free_item_slot(&eeprom, 3), None);
        assert_eq!(
            eeprom.read(0xCEBC, 12),
            vec![2, 1, 0, 0, 4, 3, 0, 0, 6, 5, 0, 0]
        );
    }

    #[test]
    fn met_peers_lose_their_identity_but_keep_the_rest() {
        let mut eeprom = Eeprom::blank();
        forget_met_peers(&mut eeprom, 10);
        assert_eq!(eeprom.read(0xDE24, 8), vec![0; 8]);
        assert_eq!(eeprom.read(0xDE2C, 0x28), vec![0xFF; 0x28]);
        assert_eq!(eeprom.read(0xDE54, 1), vec![0]);
        assert_eq!(eeprom.read(0xDE24 + 9 * 548 + 8, 0x28), vec![0xFF; 0x28]);
    }

    #[test]
    fn an_event_pokemon_counts_on_a_special_route_until_it_is_caught() {
        let mut eeprom = Eeprom::blank();
        eeprom.write(0xBF08, &[25, 0]);
        eeprom.write(0xBF44, &[0x10, 0x27, 30]);
        eeprom.write(0xBF7A, &[9]);
        assert_eq!(event_pokemon(&eeprom, false), None);
        let (pokemon, chance) = event_pokemon(&eeprom, true).unwrap();
        assert_eq!(
            (pokemon.species, chance.steps, chance.percent),
            (25, 10_000, 30)
        );
        put_event_pokemon(&mut eeprom);
        assert_eq!(event_pokemon(&eeprom, true), None);
        assert_eq!(eeprom.read(0xBA44, 2), vec![25, 0]);
        assert_eq!(eeprom.read(0xB800, 1), vec![0x20]);
        assert_eq!(eeprom.read(0x00ED + 0x39, 1), vec![0x02]);
        eeprom.fill(0xB800, 1, 0);
        assert_eq!(event_pokemon(&eeprom, true), None);
    }
}
