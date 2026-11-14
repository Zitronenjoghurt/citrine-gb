use citrine_gb_net::GtsPokemon;
use citrine_gb_pkm::pkm::text;
use citrine_gb_pkm::pkm::{Date, Gender};
use citrine_gb_pkm::{Gen, Language, Pokemon};

use super::crypt::Generation;

const SPECIES: usize = 0xEC;
const GENDER: usize = 0xEE;
const LEVEL: usize = 0xEF;
const WANTED_SPECIES: usize = 0xF0;
const WANTED_GENDER: usize = 0xF2;
const WANTED_MIN: usize = 0xF3;
const WANTED_MAX: usize = 0xF4;
const DEPOSITED: usize = 0xF8;
const TRADED: usize = 0x100;
const PID: usize = 0x108;
const NAME: usize = 0x10C;
const GEN4_TRAINER_GENDER: usize = 0xF6;
const GEN4_TRAINER_ID: usize = 0x11C;
const GEN4_REGION: usize = 0x11F;
const GEN4_AVATAR: usize = 0x120;
const GEN4_LANGUAGE: usize = 0x123;
const GEN4_COUNTRY: usize = 0x11E;
const GEN4_EXCHANGED: usize = 0x121;
const GEN4_VERSION: usize = 0x122;
const GEN5_NAME: usize = 0x110;
const GEN5_COUNTRY: usize = 0x120;
const GEN5_EXCHANGED: usize = 0x123;
const GEN5_VERSION: usize = 0x124;
const NAME_UNITS: usize = 8;
const MAX_LEVEL: u8 = 100;
const MAX_EVS: u32 = 510;

pub const EITHER: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub generation: Generation,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Egg,
    Broken,
    Mismatch,
    Level,
    Evs,
    Species,
    Wanted,
    Capsule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trailer<'a> {
    pub name: &'a str,
    pub language: Language,
    pub female: bool,
    pub tid: u16,
    pub country: u8,
    pub region: u8,
    pub avatar: u8,
    pub version: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wanted {
    pub species: u16,
    pub gender: u8,
    pub min: u8,
    pub max: u8,
}

impl Record {
    pub fn new(generation: Generation, bytes: &[u8]) -> Option<Self> {
        (bytes.len() == generation.record_bytes()).then(|| Self {
            generation,
            bytes: bytes.to_vec(),
        })
    }

    pub fn compose(pokemon: &Pokemon, wanted: Wanted, trailer: &Trailer) -> Option<Self> {
        let party = pokemon.party.as_ref()?;
        let generation = Generation::Four;
        let mut bytes = pokemon.encode();
        if bytes.len() != Gen::Four.party_size() {
            return None;
        }
        bytes.resize(generation.record_bytes(), 0);
        bytes[SPECIES..SPECIES + 2].copy_from_slice(&pokemon.species.to_le_bytes());
        bytes[GENDER] = match pokemon.gender {
            Gender::Male => 1,
            Gender::Female => 2,
            Gender::Genderless => 3,
            Gender::Invalid => return None,
        };
        bytes[LEVEL] = party.level;
        bytes[WANTED_SPECIES..WANTED_SPECIES + 2].copy_from_slice(&wanted.species.to_le_bytes());
        bytes[WANTED_GENDER] = wanted.gender;
        bytes[WANTED_MIN] = wanted.min;
        bytes[WANTED_MAX] = wanted.max;
        bytes[GEN4_TRAINER_GENDER] = u8::from(trailer.female);
        let units: [u16; NAME_UNITS] =
            text::encode(Gen::Four, trailer.language, trailer.name).ok()?;
        for (i, unit) in units.iter().enumerate() {
            bytes[NAME + 2 * i..NAME + 2 * i + 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes[GEN4_TRAINER_ID..GEN4_TRAINER_ID + 2].copy_from_slice(&trailer.tid.to_le_bytes());
        bytes[GEN4_COUNTRY] = trailer.country;
        bytes[GEN4_REGION] = trailer.region;
        bytes[GEN4_AVATAR] = trailer.avatar;
        bytes[GEN4_VERSION] = trailer.version;
        bytes[GEN4_LANGUAGE] = trailer.language.to_byte();
        Some(Self { generation, bytes })
    }

    fn party_bytes(&self) -> usize {
        match self.generation {
            Generation::Four => Gen::Four.party_size(),
            Generation::Five => Gen::Five.party_size(),
        }
    }

    fn u16_at(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.bytes[at], self.bytes[at + 1]])
    }

    pub fn species(&self) -> u16 {
        self.u16_at(SPECIES)
    }

    pub fn gender(&self) -> u8 {
        self.bytes[GENDER]
    }

    pub fn level(&self) -> u8 {
        self.bytes[LEVEL]
    }

    pub fn wanted(&self) -> Wanted {
        Wanted {
            species: self.u16_at(WANTED_SPECIES),
            gender: self.bytes[WANTED_GENDER],
            min: self.bytes[WANTED_MIN],
            max: self.bytes[WANTED_MAX],
        }
    }

    #[cfg(test)]
    pub fn pid(&self) -> i32 {
        i32::from_le_bytes(self.bytes[PID..PID + 4].try_into().unwrap_or_default())
    }

    pub fn country(&self) -> u8 {
        match self.generation {
            Generation::Four => self.bytes[GEN4_COUNTRY],
            Generation::Five => self.bytes[GEN5_COUNTRY],
        }
    }

    pub fn version(&self) -> u8 {
        match self.generation {
            Generation::Four => self.bytes[GEN4_VERSION],
            Generation::Five => self.bytes[GEN5_VERSION],
        }
    }

    pub fn trainer(&self) -> String {
        let at = match self.generation {
            Generation::Four => NAME,
            Generation::Five => GEN5_NAME,
        };
        let units: Vec<u16> = self.bytes[at..at + NAME_UNITS * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        text::decode(self.pkm_generation(), &units)
    }

    pub fn listed(&self) -> Option<GtsPokemon> {
        let pokemon = self.pokemon()?;
        Some(GtsPokemon {
            species: self.species(),
            level: self.level(),
            gender: self.gender(),
            shiny: pokemon.is_shiny(),
            nickname: text::decode(self.pkm_generation(), &pokemon.nickname),
            language: pokemon.language.to_byte(),
        })
    }

    fn pkm_generation(&self) -> Gen {
        match self.generation {
            Generation::Four => Gen::Four,
            Generation::Five => Gen::Five,
        }
    }

    pub fn pokemon(&self) -> Option<Pokemon> {
        Pokemon::decode(self.pkm_generation(), &self.bytes[..self.party_bytes()]).ok()
    }

    pub fn stamp(&mut self, pid: i32, deposited: u64, traded: Option<u64>) {
        self.bytes[PID..PID + 4].copy_from_slice(&pid.to_le_bytes());
        self.bytes[DEPOSITED..DEPOSITED + 8].copy_from_slice(&packed_time(deposited).to_le_bytes());
        let exchanged = match self.generation {
            Generation::Four => GEN4_EXCHANGED,
            Generation::Five => GEN5_EXCHANGED,
        };
        self.bytes[TRADED..TRADED + 8]
            .copy_from_slice(&traded.map_or(0, packed_time).to_le_bytes());
        self.bytes[exchanged] = u8::from(traded.is_some());
    }

    pub fn check(&self) -> Result<Pokemon, Refusal> {
        let pokemon = self.pokemon().ok_or(Refusal::Broken)?;
        let most = match self.generation {
            Generation::Four => 493,
            Generation::Five => 649,
        };
        let party = pokemon.party.as_ref().ok_or(Refusal::Broken)?;
        if pokemon.is_egg {
            return Err(Refusal::Egg);
        }
        if pokemon.species == 0 || pokemon.species > most {
            return Err(Refusal::Species);
        }
        if pokemon.species != self.species() || party.level != self.level() {
            return Err(Refusal::Mismatch);
        }
        if party.level == 0 || party.level > MAX_LEVEL {
            return Err(Refusal::Level);
        }
        if pokemon.evs.total() > MAX_EVS {
            return Err(Refusal::Evs);
        }
        if self.generation == Generation::Four && party.capsule != 0 {
            return Err(Refusal::Capsule);
        }
        let wanted = self.wanted();
        if wanted.species == 0
            || wanted.species > most
            || wanted.min > wanted.max
            || wanted.max > MAX_LEVEL
            || wanted.gender > 3
        {
            return Err(Refusal::Wanted);
        }
        Ok(pokemon)
    }

    pub fn fits(&self, wanted: Wanted) -> bool {
        self.species() == wanted.species
            && (wanted.gender == EITHER || wanted.gender == self.gender())
            && (wanted.min == 0 || self.level() >= wanted.min)
            && (wanted.max == 0 || self.level() <= wanted.max)
    }
}

pub fn date_of(unix: u64) -> Date {
    let (year, month, day) = civil(unix);
    Date {
        year: u8::try_from(year.saturating_sub(2000)).unwrap_or(99),
        month: month as u8,
        day: day as u8,
    }
}

fn civil(unix: u64) -> (u64, u64, u64) {
    let days = i64::try_from(unix / 86_400).unwrap_or_default();
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let of_era = shifted.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let day_of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u64;
    let month = (if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    }) as u64;
    let year = (year_of_era + era * 400 + i64::from(month <= 2)) as u64;
    (year, month, day)
}

pub fn packed_time(unix: u64) -> u64 {
    let (year, month, day) = civil(unix);
    let rest = unix % 86_400;
    (year << 48)
        | (month << 40)
        | (day << 32)
        | ((rest / 3600) << 24)
        | ((rest / 60 % 60) << 16)
        | ((rest % 60) << 8)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use citrine_gb_pkm::pkm::Party;

    pub(crate) fn pokemon_record(generation: Generation, species: u16, level: u8) -> Record {
        let pkm_generation = match generation {
            Generation::Four => Gen::Four,
            Generation::Five => Gen::Five,
        };
        let mut pokemon = Pokemon::new(pkm_generation);
        pokemon.pid = 0x1234_5678;
        pokemon.species = species;
        pokemon.party = Some(Party {
            level,
            ..Party::default()
        });
        let mut bytes = pokemon.encode();
        bytes.resize(generation.record_bytes(), 0);
        bytes[SPECIES..SPECIES + 2].copy_from_slice(&species.to_le_bytes());
        bytes[LEVEL] = level;
        bytes[GENDER] = 1;
        bytes[WANTED_GENDER] = EITHER;
        bytes[WANTED_SPECIES..WANTED_SPECIES + 2].copy_from_slice(&25u16.to_le_bytes());
        bytes[WANTED_MIN] = 1;
        bytes[WANTED_MAX] = 100;
        Record { generation, bytes }
    }

    #[test]
    fn the_known_date_packs_like_the_games() {
        assert_eq!(packed_time(1_268_656_496), 0x07DA_030F_0C22_3800);
        assert_eq!(
            date_of(1_268_656_496),
            Date {
                year: 10,
                month: 3,
                day: 15
            }
        );
    }

    #[test]
    fn a_composed_record_reads_back_its_pokemon_and_trainer() {
        let mut pokemon = Pokemon::new(Gen::Four);
        pokemon.pid = 0x0102_0304;
        pokemon.species = 399;
        pokemon.gender = Gender::Female;
        pokemon.language = Language::German;
        pokemon.party = Some(Party {
            level: 7,
            ..Party::default()
        });
        let wanted = Wanted {
            species: 396,
            gender: EITHER,
            min: 0,
            max: 10,
        };
        let trailer = Trailer {
            name: "Jonas",
            language: Language::German,
            female: false,
            tid: 4321,
            country: 9,
            region: 2,
            avatar: 3,
            version: 12,
        };
        let record = Record::compose(&pokemon, wanted, &trailer).unwrap();
        assert_eq!(record.check().map(|p| p.species), Ok(399));
        assert_eq!(record.wanted(), wanted);
        assert_eq!(record.trainer(), "Jonas");
        assert_eq!(
            (record.country(), record.version(), record.gender()),
            (9, 12, 2)
        );
        assert_eq!(record.bytes[GEN4_LANGUAGE], Language::German.to_byte());
    }

    #[test]
    fn a_sound_pokemon_passes_and_a_mismatched_or_egg_one_is_refused() {
        let record = pokemon_record(Generation::Four, 399, 5);
        assert_eq!(record.check().map(|pokemon| pokemon.species), Ok(399));
        let mut lying = record.clone();
        lying.bytes[LEVEL] = 50;
        assert_eq!(lying.check(), Err(Refusal::Mismatch));
        let mut egg = record.pokemon().unwrap();
        egg.is_egg = true;
        let mut bytes = egg.encode();
        bytes.extend_from_slice(&record.bytes[bytes.len()..]);
        assert_eq!(
            Record::new(Generation::Four, &bytes).unwrap().check(),
            Err(Refusal::Egg)
        );
        let mut greedy = record.clone();
        greedy.bytes[WANTED_SPECIES..WANTED_SPECIES + 2].copy_from_slice(&900u16.to_le_bytes());
        assert_eq!(greedy.check(), Err(Refusal::Wanted));
    }

    #[test]
    fn a_stamp_writes_the_pid_and_marks_a_trade() {
        let mut record = pokemon_record(Generation::Four, 399, 5);
        record.stamp(42, 1_268_656_496, Some(1_268_656_496));
        assert_eq!(record.pid(), 42);
        assert_eq!(record.bytes[GEN4_EXCHANGED], 1);
        assert_eq!(
            &record.bytes[DEPOSITED..DEPOSITED + 8],
            &0x07DA_030F_0C22_3800u64.to_le_bytes()
        );
    }

    #[test]
    fn a_request_fits_by_species_gender_and_level() {
        let record = pokemon_record(Generation::Four, 399, 5);
        let wanted = Wanted {
            species: 399,
            gender: EITHER,
            min: 1,
            max: 10,
        };
        assert!(record.fits(wanted));
        assert!(record.fits(Wanted {
            gender: 3,
            ..wanted
        }));
        assert!(!record.fits(Wanted {
            gender: 0,
            ..wanted
        }));
        assert!(!record.fits(Wanted { min: 6, ..wanted }));
        assert!(!record.fits(Wanted {
            species: 1,
            ..wanted
        }));
        assert!(!record.fits(Wanted {
            gender: 2,
            ..wanted
        }));
    }
}
