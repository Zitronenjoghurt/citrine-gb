use crate::game::Language;
use crate::gen3::{Layout, text};

pub const FLASH: usize = 0x20000;
pub const SLOT: usize = 0xE000;
const SECTION: usize = 0x1000;
const DATA: usize = 0xF80;

const ORDERS: [&str; 24] = [
    "GAEM", "GAME", "GEAM", "GEMA", "GMAE", "GMEA", "AGEM", "AGME", "AEGM", "AEMG", "AMGE", "AMEG",
    "EGAM", "EGMA", "EAGM", "EAMG", "EMGA", "EMAG", "MGAE", "MGEA", "MAGE", "MAEG", "MEGA", "MEAG",
];

#[derive(Debug, Clone, Copy)]
pub enum Pocket {
    Pc,
    Items,
    KeyItems,
    Balls,
    TmsHms,
    Berries,
}

struct Spots {
    small_size: usize,
    rival_size: usize,
    key: Option<usize>,
    code: Option<u32>,
    filler: usize,
    team_size: usize,
    team: usize,
    money: usize,
    pockets: [(usize, usize); 6],
    flags: usize,
    story: [usize; 3],
}

fn spots(layout: Layout) -> Spots {
    match layout {
        Layout::RubySapphire => Spots {
            flags: 0x1220,
            story: [0x801, 0x807, 0x804],
            small_size: 0x890,
            rival_size: 0xC40,
            key: None,
            code: Some(0),
            filler: 0x880,
            team_size: 0x234,
            team: 0x238,
            money: 0x490,
            pockets: [
                (0x498, 50),
                (0x560, 20),
                (0x5B0, 20),
                (0x600, 16),
                (0x640, 64),
                (0x740, 46),
            ],
        },
        Layout::Emerald => Spots {
            flags: 0x1270,
            story: [0x861, 0x867, 0x864],
            small_size: 0xF2C,
            rival_size: 0xF08,
            key: Some(0xAC),
            code: None,
            filler: 0xF28,
            team_size: 0x234,
            team: 0x238,
            money: 0x490,
            pockets: [
                (0x498, 50),
                (0x560, 30),
                (0x5D8, 30),
                (0x650, 16),
                (0x690, 64),
                (0x790, 46),
            ],
        },
        Layout::FireRedLeafGreen => Spots {
            flags: 0xEE0,
            story: [0x829, 0x820, 0x82C],
            small_size: 0xF24,
            rival_size: 0xEE8,
            key: Some(0xF20),
            code: Some(1),
            filler: 0xF00,
            team_size: 0x34,
            team: 0x38,
            money: 0x290,
            pockets: [
                (0x298, 30),
                (0x310, 42),
                (0x3B8, 30),
                (0x430, 13),
                (0x464, 58),
                (0x54C, 43),
            ],
        },
    }
}

pub fn gen3_text(text: &str) -> Vec<u8> {
    text::encode(text, Language::English).expect("test text")
}

pub fn gen3_name(text: &[u8], len: usize) -> Vec<u8> {
    let mut bytes = text.to_vec();
    bytes.resize(len, 0xFF);
    bytes
}

fn put(buffer: &mut [u8], at: usize, bytes: &[u8]) {
    buffer[at..at + bytes.len()].copy_from_slice(bytes);
}

fn words_sum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    for word in data.as_chunks::<4>().0 {
        sum = sum.wrapping_add(u32::from_le_bytes(*word));
    }
    ((sum >> 16) as u16).wrapping_add(sum as u16)
}

#[derive(Debug, Clone)]
pub struct TestMon3 {
    pub personality: u32,
    pub ot_id: u32,
    pub species: u16,
    pub held_item: u16,
    pub exp: u32,
    pub friendship: u8,
    pub moves: [(u16, u8); 4],
    pub pp_ups: u8,
    pub ivs: [u8; 6],
    pub evs: [u8; 6],
    pub egg: bool,
    pub ability: bool,
    pub nickname: Vec<u8>,
    pub ot_name: Vec<u8>,
    pub language: u8,
    pub flags: u8,
    pub pokerus: u8,
    pub met_location: u8,
    pub origins: u16,
    pub level: u8,
    pub hp: u16,
    pub stats: [u16; 6],
    pub status: u32,
    pub break_checksum: bool,
}

impl TestMon3 {
    pub fn new(species: u16, personality: u32, ot_id: u32) -> Self {
        Self {
            personality,
            ot_id,
            species,
            held_item: 0,
            exp: 1000,
            friendship: 70,
            moves: [(33, 35), (45, 40), (0, 0), (0, 0)],
            pp_ups: 0,
            ivs: [31, 0, 15, 7, 20, 1],
            evs: [4, 252, 0, 252, 0, 0],
            egg: false,
            ability: false,
            nickname: gen3_name(&gen3_text("TREECKO"), 10),
            ot_name: gen3_name(&gen3_text("MAY"), 7),
            language: 2,
            flags: 0x02,
            pokerus: 0,
            met_location: 16,
            origins: 5 | (2 << 7) | (4 << 11),
            level: 10,
            hp: 30,
            stats: [31, 20, 18, 22, 25, 19],
            status: 0,
            break_checksum: false,
        }
    }

    fn blocks(&self) -> [[u8; 12]; 4] {
        let mut growth = [0; 12];
        put(&mut growth, 0, &self.species.to_le_bytes());
        put(&mut growth, 2, &self.held_item.to_le_bytes());
        put(&mut growth, 4, &self.exp.to_le_bytes());
        growth[8] = self.pp_ups;
        growth[9] = self.friendship;
        let mut attacks = [0; 12];
        for (i, (id, pp)) in self.moves.iter().enumerate() {
            put(&mut attacks, 2 * i, &id.to_le_bytes());
            attacks[8 + i] = *pp;
        }
        let mut effort = [0; 12];
        put(&mut effort, 0, &self.evs);
        let mut misc = [0; 12];
        misc[0] = self.pokerus;
        misc[1] = self.met_location;
        put(&mut misc, 2, &self.origins.to_le_bytes());
        let mut genes = self
            .ivs
            .iter()
            .enumerate()
            .fold(0u32, |genes, (i, &iv)| genes | (u32::from(iv) << (5 * i)));
        genes |= u32::from(self.egg) << 30 | u32::from(self.ability) << 31;
        put(&mut misc, 4, &genes.to_le_bytes());
        [growth, attacks, effort, misc]
    }

    pub fn boxed(&self) -> Vec<u8> {
        let mut bytes = vec![0; 80];
        put(&mut bytes, 0, &self.personality.to_le_bytes());
        put(&mut bytes, 4, &self.ot_id.to_le_bytes());
        put(&mut bytes, 0x08, &self.nickname);
        bytes[0x12] = self.language;
        bytes[0x13] = self.flags;
        put(&mut bytes, 0x14, &self.ot_name);
        let blocks = self.blocks();
        let order = ORDERS[(self.personality % 24) as usize];
        let mut plain = Vec::with_capacity(48);
        for letter in order.chars() {
            let kind = "GAEM".find(letter).expect("letter");
            plain.extend_from_slice(&blocks[kind]);
        }
        let mut sum = 0u16;
        for half in plain.as_chunks::<2>().0 {
            sum = sum.wrapping_add(u16::from_le_bytes(*half));
        }
        if self.break_checksum {
            sum = sum.wrapping_add(1);
        }
        put(&mut bytes, 0x1C, &sum.to_le_bytes());
        let key = self.personality ^ self.ot_id;
        for (i, word) in plain.as_chunks::<4>().0.iter().enumerate() {
            let value = u32::from_le_bytes(*word) ^ key;
            put(&mut bytes, 0x20 + 4 * i, &value.to_le_bytes());
        }
        bytes
    }

    pub fn party(&self) -> Vec<u8> {
        let mut bytes = self.boxed();
        bytes.resize(100, 0);
        put(&mut bytes, 0x50, &self.status.to_le_bytes());
        bytes[0x54] = self.level;
        put(&mut bytes, 0x56, &self.hp.to_le_bytes());
        for (i, stat) in self.stats.iter().enumerate() {
            put(&mut bytes, 0x58 + 2 * i, &stat.to_le_bytes());
        }
        bytes
    }
}

#[derive(Debug, Clone)]
pub struct Gen3Save {
    pub layout: Layout,
    pub key: u32,
    pub small: Vec<u8>,
    pub large: Vec<u8>,
    pub pc: Vec<u8>,
}

impl Gen3Save {
    pub fn new(layout: Layout, key: u32) -> Self {
        let spots = spots(layout);
        let mut small = vec![0; DATA];
        small[spots.filler] = 0x5A;
        if let Some(code) = spots.code {
            put(&mut small, 0xAC, &code.to_le_bytes());
        }
        let key = spots.key.map_or(0, |at| {
            put(&mut small, at, &key.to_le_bytes());
            key
        });
        Self {
            layout,
            key,
            small,
            large: vec![0; 4 * DATA],
            pc: vec![0; 9 * DATA],
        }
    }

    pub fn fresh(mut self) -> Self {
        self.small[spots(self.layout).filler] = 0;
        self
    }

    pub fn trainer(&mut self, name: &str, female: bool, ids: (u16, u16), time: (u16, u8, u8)) {
        put(&mut self.small, 0, &gen3_name(&gen3_text(name), 8));
        self.small[0x08] = u8::from(female);
        put(&mut self.small, 0x0A, &ids.0.to_le_bytes());
        put(&mut self.small, 0x0C, &ids.1.to_le_bytes());
        put(&mut self.small, 0x0E, &time.0.to_le_bytes());
        self.small[0x10] = time.1;
        self.small[0x11] = time.2;
    }

    pub fn money(&mut self, money: u32, coins: u16) {
        let at = spots(self.layout).money;
        put(&mut self.large, at, &(money ^ self.key).to_le_bytes());
        put(
            &mut self.large,
            at + 4,
            &(coins ^ self.key as u16).to_le_bytes(),
        );
    }

    pub fn party(&mut self, mons: &[Vec<u8>]) {
        let spots = spots(self.layout);
        self.large[spots.team_size] = mons.len() as u8;
        for (i, mon) in mons.iter().enumerate() {
            put(&mut self.large, spots.team + 100 * i, mon);
        }
    }

    pub fn pocket(&mut self, pocket: Pocket, items: &[(u16, u16)]) {
        let (at, slots) = spots(self.layout).pockets[pocket as usize];
        assert!(items.len() <= slots);
        let key = match pocket {
            Pocket::Pc => 0,
            _ => self.key as u16,
        };
        for (i, (item, count)) in items.iter().enumerate() {
            put(&mut self.large, at + 4 * i, &item.to_le_bytes());
            put(
                &mut self.large,
                at + 4 * i + 2,
                &(count ^ key).to_le_bytes(),
            );
        }
    }

    pub fn put_box(&mut self, number: usize, slot: usize, mon: &[u8]) {
        put(&mut self.pc, 4 + (number * 30 + slot) * 80, mon);
    }

    pub fn box_name(&mut self, number: usize, name: &str) {
        put(
            &mut self.pc,
            0x8344 + 9 * number,
            &gen3_name(&gen3_text(name), 9),
        );
    }

    pub fn story(&mut self, pokedex: bool, badges: usize, champion: bool) {
        let spots = spots(self.layout);
        let [dex_flag, badge_flag, champion_flag] = spots.story;
        let flags: Vec<usize> = (0..badges)
            .map(|i| badge_flag + i)
            .chain(pokedex.then_some(dex_flag))
            .chain(champion.then_some(champion_flag))
            .collect();
        for flag in flags {
            self.large[spots.flags + flag / 8] |= 1 << (flag % 8);
        }
    }

    pub fn current_box(&mut self, number: u8) {
        self.pc[0] = number;
    }

    pub fn dex(&mut self, owned: &[u16], seen: &[u16]) {
        for (at, list) in [(0x28, owned), (0x5C, seen)] {
            for &n in list {
                let bit = usize::from(n - 1);
                self.small[at + bit / 8] |= 1 << (bit % 8);
            }
        }
    }

    pub fn slot(&self, index: u32, rotation: usize) -> Vec<u8> {
        let spots = spots(self.layout);
        let mut slot = vec![0; SLOT];
        for id in 0..14usize {
            let data = match id {
                0 => &self.small[..],
                1..=4 => &self.large[(id - 1) * DATA..][..DATA],
                _ => &self.pc[(id - 5) * DATA..][..DATA],
            };
            let size = match id {
                0 => spots.small_size,
                4 => spots.rival_size,
                13 => 0x7D0,
                _ => DATA,
            };
            let at = ((id + rotation) % 14) * SECTION;
            put(&mut slot, at, data);
            put(&mut slot, at + 0xFF4, &(id as u16).to_le_bytes());
            put(
                &mut slot,
                at + 0xFF6,
                &words_sum(&data[..size]).to_le_bytes(),
            );
            put(&mut slot, at + 0xFF8, &0x0801_2025u32.to_le_bytes());
            put(&mut slot, at + 0xFFC, &index.to_le_bytes());
        }
        slot
    }
}

pub fn break_section(slot: &mut [u8], id: u16) {
    let section = slot
        .as_chunks_mut::<SECTION>()
        .0
        .iter_mut()
        .find(|s| u16::from_le_bytes([s[0xFF4], s[0xFF5]]) == id)
        .expect("section");
    section[0x10] ^= 0xFF;
}

pub fn gen3_flash(a: Option<&[u8]>, b: Option<&[u8]>) -> Vec<u8> {
    let mut flash = vec![0xFF; FLASH];
    if let Some(a) = a {
        put(&mut flash, 0, a);
    }
    if let Some(b) = b {
        put(&mut flash, SLOT, b);
    }
    flash
}
