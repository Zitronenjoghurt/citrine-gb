mod gen4;
mod gen5;
mod names;

use crate::pkm::{BOX_SIZE, Gen, Language, Pokemon, text};
use crate::tables::{ExtractError, Game, Tables};
use citrine_gb_nds_files::NdsRom;
pub use names::{Move, Names};

pub const MAIN_RAM_BASE: u32 = 0x0200_0000;
pub const MAIN_RAM_SIZE: usize = 0x40_0000;
const GAME_CODE: u32 = 0x027F_FE0C;
const PARTY_SLOTS: usize = 6;
const BOX_SLOTS: u32 = 30;
const BOX_NAME_UNITS: usize = 20;
const BOX_NAME_STRIDE: u32 = 40;
const NATIONAL_DEX: u16 = 493;
const GEN5_NATIONAL_DEX: u16 = 649;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Live {
    pub game: Game,
    pub language: Language,
    pub trainer: Trainer,
    pub party: Vec<Pokemon>,
    pub pokedex: Pokedex,
    pub spot: Option<Spot>,
    pub boxes: Vec<PcBox>,
    pub current_box: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcBox {
    pub name: String,
    pub pokemon: Vec<Pokemon>,
}

#[derive(Debug, Clone, Copy)]
struct Storage {
    count: usize,
    current: u32,
    names: u32,
    first: u32,
    stride: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trainer {
    pub name: String,
    pub tid: u16,
    pub sid: u16,
    pub female: bool,
    pub money: u32,
    pub badges: Vec<bool>,
    pub hours: u16,
    pub minutes: u8,
    pub seconds: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pokedex {
    pub seen: Vec<u16>,
    pub caught: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    pub map: u16,
    pub x: i32,
    pub z: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Book {
    pub tables: Tables,
    pub names: Names,
    pub moves: Vec<Move>,
}

impl Book {
    pub fn load(rom: &[u8]) -> Result<Self, ExtractError> {
        let tables = crate::tables::extract(rom)?;
        let parsed = NdsRom::parse(rom)?;
        Ok(Self {
            names: names::read(tables.game, &parsed)?,
            moves: names::read_moves(tables.game, &parsed)?,
            tables,
        })
    }
}

pub fn read(main_ram: &[u8]) -> Option<Live> {
    let ram = Ram(main_ram);
    let code = ram.u32(GAME_CODE)?.to_le_bytes();
    let (game, language) = Game::identify(code)?;
    match game.generation() {
        Gen::Four => gen4::read(ram, game, language),
        Gen::Five => gen5::read(ram, game, language),
    }
}

pub fn steps(main_ram: &[u8]) -> Option<u32> {
    let ram = Ram(main_ram);
    let code = ram.u32(GAME_CODE)?.to_le_bytes();
    let (game, language) = Game::identify(code)?;
    match game.generation() {
        Gen::Four => gen4::steps(ram, game, language),
        Gen::Five => None,
    }
}

impl Pokedex {
    pub fn size(game: Game) -> u16 {
        match game.generation() {
            Gen::Four => NATIONAL_DEX,
            Gen::Five => GEN5_NATIONAL_DEX,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Ram<'a>(&'a [u8]);

impl<'a> Ram<'a> {
    fn bytes(self, address: u32, len: usize) -> Option<&'a [u8]> {
        let start = address.checked_sub(MAIN_RAM_BASE)? as usize & (MAIN_RAM_SIZE - 1);
        self.0.get(start..start.checked_add(len)?)
    }

    fn u8(self, address: u32) -> Option<u8> {
        Some(self.bytes(address, 1)?[0])
    }

    fn u16(self, address: u32) -> Option<u16> {
        let b = self.bytes(address, 2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(self, address: u32) -> Option<u32> {
        let b = self.bytes(address, 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn find(self, from: u32, len: usize, needle: &[u8]) -> Option<u32> {
        let at = self
            .bytes(from, len)?
            .windows(needle.len())
            .position(|w| w == needle)?;
        Some(from + at as u32)
    }

    fn i32(self, address: u32) -> Option<i32> {
        Some(self.u32(address)? as i32)
    }

    fn text(self, generation: Gen, address: u32, units: usize) -> Option<String> {
        let b = self.bytes(address, units * 2)?;
        let units: Vec<u16> = b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        Some(text::decode(generation, &units))
    }

    fn party(self, generation: Gen, count: u32, slots: u32) -> Option<Vec<Pokemon>> {
        let count = self.u8(count)?;
        if usize::from(count) > PARTY_SLOTS {
            return None;
        }
        let size = generation.party_size();
        (0..u32::from(count))
            .map(|i| {
                let bytes = self.bytes(slots + i * size as u32, size)?;
                Pokemon::decode(generation, bytes).ok()
            })
            .collect()
    }

    fn boxes(self, generation: Gen, storage: Storage) -> Option<(Vec<PcBox>, usize)> {
        let current = usize::from(self.u8(storage.current)?);
        if current >= storage.count {
            return None;
        }
        let boxes = (0..storage.count as u32)
            .map(|number| {
                let name = self.text(
                    generation,
                    storage.names + number * BOX_NAME_STRIDE,
                    BOX_NAME_UNITS,
                )?;
                let first = storage.first + number * storage.stride;
                let pokemon = (0..BOX_SLOTS)
                    .filter_map(|slot| {
                        let bytes = self.bytes(first + slot * BOX_SIZE as u32, BOX_SIZE)?;
                        Pokemon::decode(generation, bytes)
                            .ok()
                            .filter(|mon| mon.species != 0)
                    })
                    .collect();
                Some(PcBox { name, pokemon })
            })
            .collect::<Option<Vec<PcBox>>>()?;
        Some((boxes, current))
    }

    fn flags(self, address: u32, len: usize, last: u16) -> Option<Vec<u16>> {
        let b = self.bytes(address, len)?;
        Some(
            (1..=last.min((len * 8) as u16))
                .filter(|&n| b[usize::from(n - 1) / 8] >> ((n - 1) % 8) & 1 == 1)
                .collect(),
        )
    }
}

fn badges(bits: u16, count: usize) -> Vec<bool> {
    (0..count).map(|i| bits >> i & 1 == 1).collect()
}

#[cfg(test)]
pub(crate) mod fixture {
    use super::{GAME_CODE, MAIN_RAM_BASE, MAIN_RAM_SIZE};
    use crate::pkm::{Gen, Language, Pokemon};

    pub(crate) struct RamImage(pub Vec<u8>);

    impl RamImage {
        pub(crate) fn new(code: &[u8; 4]) -> Self {
            let mut image = Self(vec![0; MAIN_RAM_SIZE]);
            image.put(GAME_CODE, code);
            image
        }

        pub(crate) fn put(&mut self, address: u32, bytes: &[u8]) {
            let start = (address - MAIN_RAM_BASE) as usize & (MAIN_RAM_SIZE - 1);
            self.0[start..start + bytes.len()].copy_from_slice(bytes);
        }

        pub(crate) fn put_u16(&mut self, address: u32, value: u16) {
            self.put(address, &value.to_le_bytes());
        }

        pub(crate) fn put_u32(&mut self, address: u32, value: u32) {
            self.put(address, &value.to_le_bytes());
        }

        pub(crate) fn put_text(&mut self, address: u32, generation: Gen, text: &str) {
            let units: [u16; 8] =
                crate::pkm::text::encode(generation, Language::English, text).expect("text");
            for (i, unit) in units.iter().enumerate() {
                self.put_u16(address + 2 * i as u32, *unit);
            }
        }

        pub(crate) fn put_box_name(&mut self, address: u32, name: &str) {
            self.put_text(address, Gen::Four, name);
        }

        pub(crate) fn put_boxed(&mut self, address: u32, mon: &Pokemon) {
            self.put(address, &mon.encode_box());
        }

        pub(crate) fn put_party(&mut self, count: u32, slots: u32, party: &[Pokemon]) {
            self.put(count, &[party.len() as u8]);
            for (i, mon) in party.iter().enumerate() {
                let bytes = mon.encode();
                self.put(slots + (i * bytes.len()) as u32, &bytes);
            }
        }
    }

    pub(crate) fn mon(generation: Gen, species: u16, level: u8) -> Pokemon {
        let mut mon = Pokemon::new(generation);
        mon.species = species;
        mon.pid = 0x1234_5678;
        mon.tid = 1;
        mon.set_nickname("PIKA").expect("nickname");
        mon.set_ot("ASH").expect("ot");
        mon.moves = [1, 0, 0, 0];
        mon.pp = [35, 0, 0, 0];
        mon.party = Some(crate::pkm::Party {
            level,
            hp: 20,
            ..Default::default()
        });
        mon
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::RamImage;
    use super::{Ram, read};

    #[test]
    fn unknown_games_read_nothing() {
        assert_eq!(read(&RamImage::new(b"AMCE").0), None);
        assert_eq!(read(&[0; 16]), None);
    }

    #[test]
    fn addresses_wrap_in_main_ram() {
        let mut image = RamImage::new(b"CPUE");
        image.put_u32(0x0200_0010, 0xAABB_CCDD);
        let ram = Ram(&image.0);
        assert_eq!(ram.u32(0x0200_0010), Some(0xAABB_CCDD));
        assert_eq!(ram.u32(0x0240_0010), Some(0xAABB_CCDD));
        assert_eq!(ram.u32(0x0100_0000), None);
    }

    #[test]
    fn flags_count_from_one() {
        let mut image = RamImage::new(b"CPUE");
        image.put(0x0200_0100, &[0b0000_0101, 0b1000_0000]);
        let ram = Ram(&image.0);
        assert_eq!(ram.flags(0x0200_0100, 2, 493), Some(vec![1, 3, 16]));
        assert_eq!(ram.flags(0x0200_0100, 2, 10), Some(vec![1, 3]));
    }
}
