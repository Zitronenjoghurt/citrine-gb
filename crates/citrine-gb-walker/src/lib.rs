mod battle;
mod clock;
mod dowsing;
mod eeprom;
mod home;
mod info;
mod lcd;
mod lz;
pub mod packet;
mod radar;
mod ram;
mod rng;
mod route;
mod rules;
mod screen;
mod session;
mod walk;

pub use battle::{Action, Battle, BattleState};
pub use dowsing::{Dowsing, DowsingState, Hint, Prize};
pub use lcd::{Image, LCD_HEIGHT, LCD_WIDTH, Lcd, Picture, Text};
pub use radar::{Radar, RadarState};
pub use ram::Ram;
pub use route::Found;
pub use rules::{Rules, Source};
pub use screen::{App, Button, HomeEvent, Keep, Message, Screen, Setting, Shelf};

pub use eeprom::{Block, EEPROM_SIZE, Eeprom, Repair};
pub use info::{
    FLAG_HAS_POKEMON, FLAG_INIT, FLAG_POKEMON_JOINED, Health, PokemonSummary, WalkerInfo,
};
pub use session::{
    ADVERTISE_GAP, ADVERTISEMENTS, CYCLES_PER_SECOND, End, Event, Gift, Link, REPLY_DELAY, Reset,
    SESSION_TIMEOUT, airtime,
};

use clock::Clock;
use rng::Rng;
use session::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    pub rng: u64,
    pub walk_steps: u32,
    pub home: Option<HomeEvent>,
    pub ram: Option<Ram>,
}

impl Memory {
    pub const SIZE: usize = Self::OLD_SIZE + Ram::SIZE;
    pub const OLD_SIZE: usize = 15;

    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[0..8].copy_from_slice(&self.rng.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.walk_steps.to_le_bytes());
        let (kind, value) = match self.home {
            None => (0, 0),
            Some(HomeEvent::Nothing) => (1, 0),
            Some(HomeEvent::Watts(watts)) => (2, watts),
            Some(HomeEvent::Item(item)) => (3, item),
            Some(HomeEvent::Joined) => (4, 0),
        };
        bytes[12] = kind;
        bytes[13..15].copy_from_slice(&value.to_le_bytes());
        if let Some(ram) = self.ram {
            bytes[Self::OLD_SIZE..].copy_from_slice(&ram.to_bytes());
        }
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let head: &[u8; Self::OLD_SIZE] = bytes.get(..Self::OLD_SIZE)?.try_into().ok()?;
        let ram = match bytes.len() {
            Self::OLD_SIZE => None,
            Self::SIZE => Some(Ram::from_bytes(&bytes[Self::OLD_SIZE..])?),
            _ => return None,
        };
        let value = u16::from_le_bytes([head[13], head[14]]);
        let home = match head[12] {
            0 => None,
            1 => Some(HomeEvent::Nothing),
            2 => Some(HomeEvent::Watts(value)),
            3 => Some(HomeEvent::Item(value)),
            4 => Some(HomeEvent::Joined),
            _ => return None,
        };
        Some(Self {
            rng: u64::from_le_bytes(head[0..8].try_into().ok()?),
            walk_steps: u32::from_le_bytes(head[8..12].try_into().ok()?),
            home,
            ram,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walker {
    eeprom: Eeprom,
    rng: Rng,
    session: Session,
    rules: Rules,
    screen: Screen,
    ram: Ram,
    clock: Clock,
    walk_steps: u32,
    home: Option<HomeEvent>,
    offer: u32,
    woke: bool,
}

impl Walker {
    pub fn new(seed: u64) -> Self {
        Self::from_eeprom(Eeprom::blank(), seed)
    }

    pub fn from_eeprom(mut eeprom: Eeprom, seed: u64) -> Self {
        eeprom.repair_all();
        let rules = Rules::ORIGINAL;
        let ram = ram::boot(&mut eeprom, &rules);
        Self {
            eeprom,
            rng: Rng::new(seed as u32),
            session: Session::new(),
            screen: Screen::Home,
            ram,
            clock: Clock::default(),
            walk_steps: 0,
            home: None,
            offer: 0,
            woke: false,
            rules,
        }
    }

    pub fn restore(eeprom: Eeprom, memory: Memory) -> Self {
        let mut walker = Self::from_eeprom(eeprom, memory.rng);
        walker.walk_steps = memory.walk_steps;
        walker.home = memory.home;
        walker.offer = u32::from(memory.home.is_some()) * walker.rules.home_offer_passes;
        if let Some(ram) = memory.ram {
            walker.ram = ram;
        }
        walker
    }

    pub fn memory(&self) -> Memory {
        Memory {
            rng: self.rng.state() as u64,
            walk_steps: self.walk_steps,
            home: self.home,
            ram: Some(self.ram),
        }
    }

    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    pub fn set_rules(&mut self, rules: Rules) {
        self.rules = rules;
    }

    pub fn eeprom(&self) -> &Eeprom {
        &self.eeprom
    }

    pub fn rng_state(&self) -> u64 {
        self.rng.state() as u64
    }

    pub fn set_rng_state(&mut self, state: u64) {
        self.rng = Rng::new(state as u32);
    }

    pub fn link(&self) -> Link {
        self.session.link()
    }

    pub fn take(&mut self, now: u64) -> Option<Vec<u8>> {
        self.expire_session(now);
        self.session.take(now, &mut self.rng)
    }

    pub fn events(&mut self) -> Vec<Event> {
        self.session.events()
    }

    pub fn info(&mut self) -> WalkerInfo {
        WalkerInfo::from_slice(&self.eeprom.read_reliable(Block::Info).unwrap_or_default())
    }

    pub fn health(&self) -> Health {
        Health::from_slice(&self.ram.health)
    }

    pub fn today_steps(&self) -> u32 {
        self.ram.today_steps
    }

    pub fn clock(&self) -> u32 {
        self.health().clock()
    }

    pub fn walking_pokemon(&mut self) -> Option<PokemonSummary> {
        if !self.info().has_pokemon() {
            return None;
        }
        let bytes = self.eeprom.read(eeprom::ROUTE, PokemonSummary::SIZE);
        Some(PokemonSummary::from_bytes(
            bytes.as_slice().try_into().ok()?,
        ))
    }

    pub(crate) fn stored_info(&self) -> WalkerInfo {
        WalkerInfo::from_slice(&self.eeprom.read(Block::Info.address(), Block::Info.size()))
    }

    pub(crate) fn set_health(&mut self, health: &Health) {
        self.ram.health.copy_from_slice(health.bytes());
    }

    pub(crate) fn save_health(&mut self) {
        let health = self.ram.health;
        self.eeprom.write_reliable(Block::Health, &health);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_round_trips_through_its_bytes() {
        let mut walker = Walker::new(0xABCD);
        walker.walk_steps = 1234;
        walker.home = Some(HomeEvent::Item(0x0155));
        walker.steps(77);
        let bytes = walker.memory().to_bytes();
        let memory = Memory::from_bytes(&bytes).unwrap();
        assert_eq!(memory, walker.memory());
        let mut restored = Walker::restore(walker.eeprom().clone(), memory);
        restored.offer = walker.offer;
        assert_eq!(restored, walker);
        assert_eq!(Memory::from_bytes(&bytes[..Memory::SIZE - 1]), None);
        let mut bad = bytes;
        bad[12] = 9;
        assert_eq!(Memory::from_bytes(&bad), None);
    }

    #[test]
    fn an_old_memory_record_boots_the_ram_from_the_eeprom() {
        let mut eeprom = Eeprom::blank();
        let mut info = vec![0; 0x68];
        info[0x5B] = FLAG_INIT | FLAG_HAS_POKEMON | (5 << 3);
        eeprom.write_reliable(Block::Info, &info);
        let mut health = [0; 0x18];
        health[0x0E..0x10].copy_from_slice(&321u16.to_be_bytes());
        eeprom.write_reliable(Block::Health, &health);
        let mut old = [0u8; Memory::OLD_SIZE];
        old[0] = 7;
        old[12] = 2;
        old[13] = 20;
        let memory = Memory::from_bytes(&old).unwrap();
        assert_eq!(
            (memory.home, memory.ram),
            (Some(HomeEvent::Watts(20)), None)
        );
        let walker = Walker::restore(eeprom, memory);
        assert_eq!(walker.health().watts(), 321);
        assert_eq!(walker.rng_state(), 7);
        assert!(walker.ram.walking);
        assert_eq!(walker.ram.rollover_hour, 5);
    }

    #[test]
    fn a_walker_loaded_with_a_bad_copy_repairs_it() {
        let mut eeprom = Eeprom::blank();
        eeprom.write_reliable(Block::Health, &[3; 0x18]);
        let mut bytes = eeprom.bytes().to_vec();
        bytes[0x0256] = 9;
        let walker = Walker::from_eeprom(Eeprom::from_bytes(&bytes).unwrap(), 1);
        assert_eq!(
            walker.eeprom().read(0x0256, 0x19),
            walker.eeprom().read(0x0156, 0x19)
        );
    }
}
