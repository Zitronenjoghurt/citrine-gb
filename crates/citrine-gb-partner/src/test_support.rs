use crate::Partner;
use crate::dex::{Dex, Entry};
use crate::events::{Event, Genes, Trainer};
use citrine_gb::netplay::LinkPeer;
use citrine_gb_gamedata::{BaseStats, Game, Generation, Language, Version};

pub(crate) fn game(version: Version, language: Language) -> Game {
    Game {
        version,
        language,
        revision: 0,
    }
}

fn gen1_record(dex: u8, types: [u8; 2], catch_rate: u8) -> Vec<u8> {
    let mut record = vec![0u8; 28];
    record[0] = dex;
    record[1..6].copy_from_slice(&[100, 100, 100, 100, 100]);
    record[6..8].copy_from_slice(&types);
    record[8] = catch_rate;
    record[19] = 3;
    record
}

fn gen2_record(dex: u8, types: [u8; 2], egg_cycles: u8) -> Vec<u8> {
    let mut record = vec![0u8; 32];
    record[0] = dex;
    record[1..7].copy_from_slice(&[100, 100, 100, 100, 100, 100]);
    record[7..9].copy_from_slice(&types);
    record[9] = 45;
    record[15] = egg_cycles;
    record[22] = 3;
    record
}

pub(crate) const PSYCHIC: u8 = 0x18;

pub(crate) const EGG_CYCLES: u8 = 10;

pub(crate) fn dex(game: Game) -> Dex {
    let mut dex = Dex::default();
    let generation = game.generation();
    let count = match generation {
        Generation::One => 151,
        Generation::Two => 251,
        Generation::Three | Generation::Four | Generation::Five => 0,
    };
    for number in 1..=count {
        let (index, record) = match generation {
            Generation::One => (
                crate::gen1_index(number).expect("an index"),
                gen1_record(number as u8, [PSYCHIC, PSYCHIC], 45),
            ),
            Generation::Two => (
                number as u8,
                gen2_record(number as u8, [PSYCHIC, PSYCHIC], EGG_CYCLES),
            ),
            Generation::Three | Generation::Four | Generation::Five => break,
        };
        let name = match game.language {
            Language::Japanese => "ミュウ",
            _ => "MEW",
        };
        dex.insert(
            number,
            Entry {
                index,
                name: name.to_string(),
                base: BaseStats::from_record(generation, &record).expect("record"),
            },
        );
    }
    dex
}

pub(crate) fn pp() -> Vec<u8> {
    let mut pp = vec![20; 252];
    pp[0] = 0;
    pp[1] = 35;
    pp
}

pub(crate) fn partner(game: Game, event: &'static Event, seed: u64) -> Partner {
    Partner::with_dex(game, dex(game), pp(), event, seed).expect("an offer")
}

pub(crate) fn custom(event: Event) -> &'static Event {
    Box::leak(Box::new(event))
}

pub(crate) fn mew(trainers: &'static [Trainer]) -> Event {
    Event {
        key: "test-mew",
        place: "test",
        year: 1999,
        generation: Generation::One,
        dex: 151,
        level: 5,
        moves: &[1],
        trainers,
        genes: Genes::Random,
        item: None,
        egg: false,
        source: "test",
    }
}

pub(crate) const PREAMBLE: u8 = 0xFD;

const NO_DATA: u8 = 0xFE;

const PATCH_END: u8 = 0xFF;

const PATCH_PART: usize = 252;

const ROUNDS: usize = 10;

const PATIENCE: usize = 600;

pub(crate) struct Side<'a> {
    pub peer: &'a mut dyn LinkPeer,
}

impl Side<'_> {
    pub fn byte(&mut self, sent: u8) -> u8 {
        self.peer.exchange(sent)
    }

    pub fn handshake(&mut self) {
        assert_eq!(self.byte(0x01), 0x02);
        self.byte(0x00);
        self.byte(0x00);
    }

    pub fn nybble(&mut self, high: u8, value: u8) -> u8 {
        let sent = high | value;
        let mut got = (0..PATIENCE)
            .find_map(|_| {
                let reply = self.byte(sent);
                (reply & 0xF0 == high).then_some(reply & 0x0F)
            })
            .expect("the partner never answered the nybble");
        for _ in 0..ROUNDS {
            let reply = self.byte(sent);
            if reply & 0xF0 == high {
                got = reply & 0x0F;
            }
        }
        for _ in 0..ROUNDS {
            self.byte(0x00);
        }
        got
    }

    pub fn sync_bytes(&mut self, sent: u8) -> u8 {
        for _ in 0..PATIENCE {
            self.byte(sent);
            let first = self.byte(sent);
            let second = self.byte(sent);
            if let Some(found) = [first, second].into_iter().find(|b| b & 0xF0 == 0xD0) {
                return found;
            }
        }
        panic!("the partner never sent a sync byte")
    }

    pub fn exchange_bytes(&mut self, data: &[u8]) -> Vec<u8> {
        let mut received = Vec::with_capacity(data.len());
        let mut at = 0;
        let mut ignoring = true;
        let mut tries = 0;
        while received.len() < data.len() {
            let reply = self.byte(data[at]);
            if ignoring {
                tries += 1;
                assert!(tries < PATIENCE, "the partner never sent a preamble");
                ignoring = reply != PREAMBLE;
                continue;
            }
            received.push(reply);
            at += 1;
        }
        received
    }

    pub fn exchange_mail(&mut self, data: &[u8]) -> Vec<u8> {
        self.byte(data[0]);
        data.iter().map(|&b| self.byte(b)).collect()
    }
}

pub(crate) struct Party {
    pub name: Vec<u8>,
    pub count: u8,
    pub species: Vec<u8>,
    pub patched: Vec<u8>,
    pub ots: Vec<u8>,
    pub nicknames: Vec<u8>,
}

pub(crate) fn read_party(received: &[u8], patches: &[u8], name: usize, patched: usize) -> Party {
    let start = received
        .iter()
        .position(|&b| !matches!(b, 0x00 | PREAMBLE | NO_DATA))
        .expect("a name");
    let data: Vec<u8> = received[start..]
        .iter()
        .copied()
        .filter(|&b| b != NO_DATA)
        .collect();
    let (head, rest) = data.split_at(name);
    let count = rest[0];
    let species = rest[1..8].to_vec();
    let mut patched_data = rest[8..8 + patched].to_vec();
    let tail = &rest[8 + patched..];
    let mut base = 0;
    let mut parts = 2;
    for &entry in patches {
        if parts == 0 {
            break;
        }
        match entry {
            0x00 | PREAMBLE | NO_DATA => {}
            PATCH_END => {
                base = PATCH_PART;
                parts -= 1;
            }
            offset => patched_data[base + usize::from(offset) - 1] = NO_DATA,
        }
    }
    Party {
        name: head.to_vec(),
        count,
        species,
        patched: patched_data,
        ots: tail[..name * 6].to_vec(),
        nicknames: tail[name * 6..name * 12].to_vec(),
    }
}

pub(crate) fn game_block(len: usize, preamble: usize, fill: u8) -> Vec<u8> {
    let mut block = vec![PREAMBLE; preamble];
    block.resize(len, fill);
    block
}
