mod crypt;
pub mod text;

use crate::stats::Stats;
pub use text::TextError;

pub const BOX_SIZE: usize = 136;
pub const GEN4_PARTY_SIZE: usize = 236;
pub const GEN5_PARTY_SIZE: usize = 220;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gen {
    Four,
    Five,
}

impl Gen {
    pub fn party_size(self) -> usize {
        match self {
            Self::Four => GEN4_PARTY_SIZE,
            Self::Five => GEN5_PARTY_SIZE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PkmError {
    #[error("{0} bytes is neither a box nor a party Pokémon")]
    WrongSize(usize),
    #[error("checksum {stored:#06x} doesn't match the data's {computed:#06x}")]
    BadChecksum { stored: u16, computed: u16 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Gender {
    #[default]
    Male,
    Female,
    Genderless,
    Invalid,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Language {
    Japanese,
    #[default]
    English,
    French,
    Italian,
    German,
    Spanish,
    Korean,
    Other(u8),
}

impl Language {
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            1 => Self::Japanese,
            2 => Self::English,
            3 => Self::French,
            4 => Self::Italian,
            5 => Self::German,
            7 => Self::Spanish,
            8 => Self::Korean,
            other => Self::Other(other),
        }
    }

    pub fn to_byte(self) -> u8 {
        match self {
            Self::Japanese => 1,
            Self::English => 2,
            Self::French => 3,
            Self::Italian => 4,
            Self::German => 5,
            Self::Spanish => 7,
            Self::Korean => 8,
            Self::Other(byte) => byte,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Date {
    pub year: u8,
    pub month: u8,
    pub day: u8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Ribbons {
    pub sinnoh: u32,
    pub hoenn: u32,
    pub sinnoh_contest: u32,
}

impl Ribbons {
    pub fn count(self) -> u32 {
        (self.sinnoh & 0x0FFF_FFFF).count_ones()
            + self.hoenn.count_ones()
            + (self.sinnoh_contest & 0x000F_FFFF).count_ones()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Extra {
    Gen4 {
        shiny_leaves: u8,
        egg_location_ext: u16,
        met_location_ext: u16,
        hgss_ball: u8,
        walking_mood: i8,
    },
    Gen5 {
        nature: u8,
        hidden_ability: bool,
        n_sparkle: bool,
        pokestar_fame: u8,
    },
}

impl Extra {
    pub fn empty(generation: Gen) -> Self {
        match generation {
            Gen::Four => Self::Gen4 {
                shiny_leaves: 0,
                egg_location_ext: 0,
                met_location_ext: 0,
                hgss_ball: 0,
                walking_mood: 0,
            },
            Gen::Five => Self::Gen5 {
                nature: 0,
                hidden_ability: false,
                n_sparkle: false,
                pokestar_fame: 0,
            },
        }
    }

    pub fn generation(self) -> Gen {
        match self {
            Self::Gen4 { .. } => Gen::Four,
            Self::Gen5 { .. } => Gen::Five,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Party {
    pub status: u32,
    pub level: u8,
    pub capsule: u8,
    pub hp: u16,
    pub stats: Stats<u16>,
    pub mail: [u8; 56],
}

impl Default for Party {
    fn default() -> Self {
        Self {
            status: 0,
            level: 1,
            capsule: 0,
            hp: 0,
            stats: Stats::default(),
            mail: [0; 56],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Pokemon {
    pub pid: u32,
    pub species: u16,
    pub held_item: u16,
    pub tid: u16,
    pub sid: u16,
    pub exp: u32,
    pub friendship: u8,
    pub ability: u8,
    pub markings: u8,
    pub language: Language,
    pub evs: Stats<u8>,
    pub contest: [u8; 6],
    pub ribbons: Ribbons,
    pub moves: [u16; 4],
    pub pp: [u8; 4],
    pub pp_ups: [u8; 4],
    pub ivs: Stats<u8>,
    pub is_egg: bool,
    pub is_nicknamed: bool,
    pub fateful_encounter: bool,
    pub gender: Gender,
    pub form: u8,
    pub nickname: [u16; 11],
    pub version: u8,
    pub ot_name: [u16; 8],
    pub ot_female: bool,
    pub egg_date: Date,
    pub met_date: Date,
    pub egg_location: u16,
    pub met_location: u16,
    pub pokerus: u8,
    pub ball: u8,
    pub met_level: u8,
    pub ground_tile: u8,
    pub extra: Extra,
    pub party: Option<Party>,
    rest: Rest,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Rest([u8; GEN4_PARTY_SIZE]);

impl Pokemon {
    pub fn new(generation: Gen) -> Self {
        Self {
            pid: 0,
            species: 0,
            held_item: 0,
            tid: 0,
            sid: 0,
            exp: 0,
            friendship: 0,
            ability: 0,
            markings: 0,
            language: Language::Other(0),
            evs: Stats::default(),
            contest: [0; 6],
            ribbons: Ribbons::default(),
            moves: [0; 4],
            pp: [0; 4],
            pp_ups: [0; 4],
            ivs: Stats::default(),
            is_egg: false,
            is_nicknamed: false,
            fateful_encounter: false,
            gender: Gender::Male,
            form: 0,
            nickname: [0; 11],
            version: 0,
            ot_name: [0; 8],
            ot_female: false,
            egg_date: Date::default(),
            met_date: Date::default(),
            egg_location: 0,
            met_location: 0,
            pokerus: 0,
            ball: 0,
            met_level: 0,
            ground_tile: 0,
            extra: Extra::empty(generation),
            party: None,
            rest: Rest([0; GEN4_PARTY_SIZE]),
        }
    }

    pub fn generation(&self) -> Gen {
        self.extra.generation()
    }

    pub fn decode(generation: Gen, encrypted: &[u8]) -> Result<Self, PkmError> {
        if encrypted.len() != BOX_SIZE && encrypted.len() != generation.party_size() {
            return Err(PkmError::WrongSize(encrypted.len()));
        }
        let mut plain = encrypted.to_vec();
        crypt::decrypt(&mut plain);
        let stored = u16::from_le_bytes([plain[6], plain[7]]);
        let computed = crypt::checksum(&plain);
        if stored != computed {
            return Err(PkmError::BadChecksum { stored, computed });
        }
        Ok(Self::read(generation, &plain))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut plain = self.write();
        let check = crypt::checksum(&plain);
        plain[6..8].copy_from_slice(&check.to_le_bytes());
        crypt::encrypt(&mut plain);
        plain
    }

    pub fn encode_box(&self) -> Vec<u8> {
        let mut boxed = self.clone();
        boxed.party = None;
        boxed.encode()
    }

    pub fn nature(&self) -> u8 {
        match self.extra {
            Extra::Gen4 { .. } => nature_of(self.pid),
            Extra::Gen5 { nature, .. } => nature,
        }
    }

    pub fn is_shiny(&self) -> bool {
        is_shiny(self.pid, self.tid, self.sid)
    }

    pub fn nickname_text(&self) -> String {
        text::decode(self.generation(), &self.nickname)
    }

    pub fn ot_text(&self) -> String {
        text::decode(self.generation(), &self.ot_name)
    }

    pub fn set_nickname(&mut self, name: &str) -> Result<(), TextError> {
        self.nickname = text::encode(self.generation(), self.language, name)?;
        Ok(())
    }

    pub fn set_ot(&mut self, name: &str) -> Result<(), TextError> {
        self.ot_name = text::encode(self.generation(), self.language, name)?;
        Ok(())
    }

    fn read(generation: Gen, plain: &[u8]) -> Self {
        let b = Bytes(plain);
        let mask = owned_mask(generation);
        let mut rest = [0u8; GEN4_PARTY_SIZE];
        for (i, &byte) in plain.iter().enumerate() {
            rest[i] = byte & !mask[i];
        }
        let iv_word = b.u32(0x38);
        let flags40 = b.u8(0x40);
        let met84 = b.u8(0x84);
        let extra = match generation {
            Gen::Four => Extra::Gen4 {
                shiny_leaves: b.u8(0x41),
                egg_location_ext: b.u16(0x44),
                met_location_ext: b.u16(0x46),
                hgss_ball: b.u8(0x86),
                walking_mood: b.u8(0x87) as i8,
            },
            Gen::Five => Extra::Gen5 {
                nature: b.u8(0x41),
                hidden_ability: b.u8(0x42) & 1 != 0,
                n_sparkle: b.u8(0x42) & 2 != 0,
                pokestar_fame: b.u8(0x87),
            },
        };
        let party = (plain.len() > BOX_SIZE).then(|| Party {
            status: b.u32(0x88),
            level: b.u8(0x8C),
            capsule: b.u8(0x8D),
            hp: b.u16(0x8E),
            stats: Stats::from_array(std::array::from_fn(|i| b.u16(0x90 + 2 * i))),
            mail: std::array::from_fn(|i| b.u8(0x9C + i)),
        });
        Self {
            pid: b.u32(0x00),
            species: b.u16(0x08),
            held_item: b.u16(0x0A),
            tid: b.u16(0x0C),
            sid: b.u16(0x0E),
            exp: b.u32(0x10),
            friendship: b.u8(0x14),
            ability: b.u8(0x15),
            markings: b.u8(0x16),
            language: Language::from_byte(b.u8(0x17)),
            evs: Stats::from_array(std::array::from_fn(|i| b.u8(0x18 + i))),
            contest: std::array::from_fn(|i| b.u8(0x1E + i)),
            ribbons: Ribbons {
                sinnoh: b.u32(0x24),
                hoenn: b.u32(0x3C),
                sinnoh_contest: b.u32(0x60),
            },
            moves: std::array::from_fn(|i| b.u16(0x28 + 2 * i)),
            pp: std::array::from_fn(|i| b.u8(0x30 + i)),
            pp_ups: std::array::from_fn(|i| b.u8(0x34 + i)),
            ivs: Stats::unpack_ivs(iv_word),
            is_egg: iv_word & (1 << 30) != 0,
            is_nicknamed: iv_word & (1 << 31) != 0,
            fateful_encounter: flags40 & 1 != 0,
            gender: match (flags40 >> 1) & 3 {
                0 => Gender::Male,
                1 => Gender::Female,
                2 => Gender::Genderless,
                _ => Gender::Invalid,
            },
            form: flags40 >> 3,
            nickname: std::array::from_fn(|i| b.u16(0x48 + 2 * i)),
            version: b.u8(0x5F),
            ot_name: std::array::from_fn(|i| b.u16(0x68 + 2 * i)),
            egg_date: b.date(0x78),
            met_date: b.date(0x7B),
            egg_location: b.u16(0x7E),
            met_location: b.u16(0x80),
            pokerus: b.u8(0x82),
            ball: b.u8(0x83),
            met_level: met84 & 0x7F,
            ot_female: met84 & 0x80 != 0,
            ground_tile: b.u8(0x85),
            extra,
            party,
            rest: Rest(rest),
        }
    }

    fn write(&self) -> Vec<u8> {
        let size = match self.party {
            Some(_) => self.generation().party_size(),
            None => BOX_SIZE,
        };
        let mut plain = self.rest.0[..size].to_vec();
        let mut b = BytesMut(&mut plain);
        b.u32(0x00, self.pid);
        b.u16(0x08, self.species);
        b.u16(0x0A, self.held_item);
        b.u16(0x0C, self.tid);
        b.u16(0x0E, self.sid);
        b.u32(0x10, self.exp);
        b.u8(0x14, self.friendship);
        b.u8(0x15, self.ability);
        b.u8(0x16, self.markings);
        b.u8(0x17, self.language.to_byte());
        for (i, ev) in self.evs.to_array().into_iter().enumerate() {
            b.u8(0x18 + i, ev);
        }
        for (i, &value) in self.contest.iter().enumerate() {
            b.u8(0x1E + i, value);
        }
        b.u32(0x24, self.ribbons.sinnoh);
        for (i, &mv) in self.moves.iter().enumerate() {
            b.u16(0x28 + 2 * i, mv);
        }
        for i in 0..4 {
            b.u8(0x30 + i, self.pp[i]);
            b.u8(0x34 + i, self.pp_ups[i]);
        }
        let iv_word =
            self.ivs.pack_ivs() | u32::from(self.is_egg) << 30 | u32::from(self.is_nicknamed) << 31;
        b.u32(0x38, iv_word);
        b.u32(0x3C, self.ribbons.hoenn);
        let gender = match self.gender {
            Gender::Male => 0,
            Gender::Female => 1,
            Gender::Genderless => 2,
            Gender::Invalid => 3,
        };
        b.u8(
            0x40,
            u8::from(self.fateful_encounter) | gender << 1 | self.form << 3,
        );
        for (i, &unit) in self.nickname.iter().enumerate() {
            b.u16(0x48 + 2 * i, unit);
        }
        b.u8(0x5F, self.version);
        b.u32(0x60, self.ribbons.sinnoh_contest);
        for (i, &unit) in self.ot_name.iter().enumerate() {
            b.u16(0x68 + 2 * i, unit);
        }
        b.date(0x78, self.egg_date);
        b.date(0x7B, self.met_date);
        b.u16(0x7E, self.egg_location);
        b.u16(0x80, self.met_location);
        b.u8(0x82, self.pokerus);
        b.u8(0x83, self.ball);
        b.u8(0x84, self.met_level & 0x7F | u8::from(self.ot_female) << 7);
        b.u8(0x85, self.ground_tile);
        match self.extra {
            Extra::Gen4 {
                shiny_leaves,
                egg_location_ext,
                met_location_ext,
                hgss_ball,
                walking_mood,
            } => {
                b.u8(0x41, shiny_leaves);
                b.u16(0x44, egg_location_ext);
                b.u16(0x46, met_location_ext);
                b.u8(0x86, hgss_ball);
                b.u8(0x87, walking_mood as u8);
            }
            Extra::Gen5 {
                nature,
                hidden_ability,
                n_sparkle,
                pokestar_fame,
            } => {
                b.u8(0x41, nature);
                let flags =
                    self.rest.0[0x42] & !3 | u8::from(hidden_ability) | u8::from(n_sparkle) << 1;
                b.u8(0x42, flags);
                b.u8(0x87, pokestar_fame);
            }
        }
        if let Some(party) = &self.party {
            b.u32(0x88, party.status);
            b.u8(0x8C, party.level);
            b.u8(0x8D, party.capsule);
            b.u16(0x8E, party.hp);
            for (i, stat) in party.stats.to_array().into_iter().enumerate() {
                b.u16(0x90 + 2 * i, stat);
            }
            for (i, &byte) in party.mail.iter().enumerate() {
                b.u8(0x9C + i, byte);
            }
        }
        plain
    }
}

pub fn nature_of(pid: u32) -> u8 {
    (pid % 25) as u8
}

pub fn is_shiny(pid: u32, tid: u16, sid: u16) -> bool {
    shiny_value(pid, tid, sid) < 8
}

pub fn shiny_value(pid: u32, tid: u16, sid: u16) -> u16 {
    (pid >> 16) as u16 ^ pid as u16 ^ tid ^ sid
}

fn owned_mask(generation: Gen) -> [u8; GEN4_PARTY_SIZE] {
    let full = Stats::splat(u8::MAX);
    let mut all = Pokemon::new(generation);
    all.pid = u32::MAX;
    all.species = u16::MAX;
    all.held_item = u16::MAX;
    all.tid = u16::MAX;
    all.sid = u16::MAX;
    all.exp = u32::MAX;
    all.friendship = u8::MAX;
    all.ability = u8::MAX;
    all.markings = u8::MAX;
    all.language = Language::Other(u8::MAX);
    all.evs = full;
    all.contest = [u8::MAX; 6];
    all.ribbons = Ribbons {
        sinnoh: u32::MAX,
        hoenn: u32::MAX,
        sinnoh_contest: u32::MAX,
    };
    all.moves = [u16::MAX; 4];
    all.pp = [u8::MAX; 4];
    all.pp_ups = [u8::MAX; 4];
    all.ivs = full;
    all.is_egg = true;
    all.is_nicknamed = true;
    all.fateful_encounter = true;
    all.gender = Gender::Invalid;
    all.form = 0x1F;
    all.nickname = [u16::MAX; 11];
    all.version = u8::MAX;
    all.ot_name = [u16::MAX; 8];
    all.ot_female = true;
    all.egg_date = Date {
        year: u8::MAX,
        month: u8::MAX,
        day: u8::MAX,
    };
    all.met_date = all.egg_date;
    all.egg_location = u16::MAX;
    all.met_location = u16::MAX;
    all.pokerus = u8::MAX;
    all.ball = u8::MAX;
    all.met_level = u8::MAX;
    all.ground_tile = u8::MAX;
    all.extra = match generation {
        Gen::Four => Extra::Gen4 {
            shiny_leaves: u8::MAX,
            egg_location_ext: u16::MAX,
            met_location_ext: u16::MAX,
            hgss_ball: u8::MAX,
            walking_mood: -1,
        },
        Gen::Five => Extra::Gen5 {
            nature: u8::MAX,
            hidden_ability: true,
            n_sparkle: true,
            pokestar_fame: u8::MAX,
        },
    };
    all.party = Some(Party {
        status: u32::MAX,
        level: u8::MAX,
        capsule: u8::MAX,
        hp: u16::MAX,
        stats: Stats::splat(u16::MAX),
        mail: [u8::MAX; 56],
    });
    let mut mask = [0u8; GEN4_PARTY_SIZE];
    let written = all.write();
    mask[..written.len()].copy_from_slice(&written);
    mask[6] = u8::MAX;
    mask[7] = u8::MAX;
    mask
}

struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn u8(&self, at: usize) -> u8 {
        self.0[at]
    }

    fn u16(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.0[at], self.0[at + 1]])
    }

    fn u32(&self, at: usize) -> u32 {
        u32::from_le_bytes([self.0[at], self.0[at + 1], self.0[at + 2], self.0[at + 3]])
    }

    fn date(&self, at: usize) -> Date {
        Date {
            year: self.0[at],
            month: self.0[at + 1],
            day: self.0[at + 2],
        }
    }
}

struct BytesMut<'a>(&'a mut [u8]);

impl BytesMut<'_> {
    fn u8(&mut self, at: usize, value: u8) {
        self.0[at] = value;
    }

    fn u16(&mut self, at: usize, value: u16) {
        self.0[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, at: usize, value: u32) {
        self.0[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn date(&mut self, at: usize, date: Date) {
        self.0[at..at + 3].copy_from_slice(&[date.year, date.month, date.day]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turtwig() -> Pokemon {
        let mut mon = Pokemon::new(Gen::Four);
        mon.pid = 0x7B06_01DB;
        mon.species = 387;
        mon.tid = 12345;
        mon.sid = 54321;
        mon.exp = 135;
        mon.friendship = 70;
        mon.ability = 65;
        mon.language = Language::English;
        mon.evs = Stats::from_array([4, 0, 0, 252, 0, 252]);
        mon.moves = [33, 0, 0, 0];
        mon.pp = [35, 0, 0, 0];
        mon.ivs = Stats::from_array([19, 17, 20, 16, 3, 25]);
        mon.ribbons.sinnoh = 1 << 26;
        mon.set_nickname("TURTWIG").unwrap();
        mon.set_ot("Lucas").unwrap();
        mon.version = 10;
        mon.met_date = Date {
            year: 8,
            month: 4,
            day: 22,
        };
        mon.met_location = 16;
        mon.ball = 4;
        mon.met_level = 5;
        mon
    }

    fn sealed(generation: Gen, len: usize, mut fill: impl FnMut() -> u8) -> Vec<u8> {
        let mut plain: Vec<u8> = (0..len).map(|_| fill()).collect();
        let check = crypt::checksum(&plain);
        plain[6..8].copy_from_slice(&check.to_le_bytes());
        let _ = generation;
        crypt::encrypt(&mut plain);
        plain
    }

    #[test]
    fn a_hand_made_pokemon_round_trips() {
        let mon = turtwig();
        let bytes = mon.encode();
        assert_eq!(bytes.len(), BOX_SIZE);
        let back = Pokemon::decode(Gen::Four, &bytes).unwrap();
        assert_eq!(back, mon);
        assert_eq!(back.nickname_text(), "TURTWIG");
        assert_eq!(back.ot_text(), "Lucas");
        assert_eq!(back.nature(), 9);
    }

    #[test]
    fn the_stored_bytes_are_encrypted_and_shuffled() {
        let bytes = turtwig().encode();
        assert_ne!(u16::from_le_bytes([bytes[8], bytes[9]]), 387);
        assert_eq!(&bytes[..4], &0x7B06_01DBu32.to_le_bytes());
    }

    #[test]
    fn a_party_pokemon_round_trips_with_its_stats() {
        for generation in [Gen::Four, Gen::Five] {
            let mut mon = turtwig();
            mon.extra = Extra::empty(generation);
            if generation == Gen::Five {
                mon.extra = Extra::Gen5 {
                    nature: 3,
                    hidden_ability: true,
                    n_sparkle: false,
                    pokestar_fame: 0,
                };
                mon.set_nickname("Tsutarja").unwrap();
            }
            mon.party = Some(Party {
                level: 5,
                hp: 20,
                stats: Stats::from_array([20, 11, 12, 9, 10, 10]),
                ..Party::default()
            });
            let bytes = mon.encode();
            assert_eq!(bytes.len(), generation.party_size());
            assert_eq!(Pokemon::decode(generation, &bytes).unwrap(), mon);
            assert_eq!(mon.encode_box().len(), BOX_SIZE);
        }
    }

    #[test]
    fn gen5_keeps_its_nature_apart_from_the_pid() {
        let mut mon = Pokemon::new(Gen::Five);
        mon.pid = 0;
        mon.extra = Extra::Gen5 {
            nature: 17,
            hidden_ability: false,
            n_sparkle: false,
            pokestar_fame: 0,
        };
        assert_eq!(mon.nature(), 17);
        assert_eq!(
            Pokemon::decode(Gen::Five, &mon.encode()).unwrap().nature(),
            17
        );
    }

    #[test]
    fn every_byte_of_random_files_survives_a_round_trip() {
        let mut state = 0x2545_F491u32;
        let mut fill = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        };
        for generation in [Gen::Four, Gen::Five] {
            for len in [BOX_SIZE, generation.party_size()] {
                for _ in 0..200 {
                    let file = sealed(generation, len, &mut fill);
                    let mon = Pokemon::decode(generation, &file).unwrap();
                    assert_eq!(mon.encode(), file);
                }
            }
        }
    }

    #[test]
    fn a_changed_byte_fails_the_checksum() {
        let mut bytes = turtwig().encode();
        bytes[0x40] ^= 1;
        assert!(matches!(
            Pokemon::decode(Gen::Four, &bytes),
            Err(PkmError::BadChecksum { .. })
        ));
    }

    #[test]
    fn other_sizes_are_refused() {
        assert_eq!(
            Pokemon::decode(Gen::Five, &[0; GEN4_PARTY_SIZE]),
            Err(PkmError::WrongSize(GEN4_PARTY_SIZE))
        );
    }

    #[test]
    fn japanese_and_korean_names_round_trip_in_both_generations() {
        for generation in [Gen::Four, Gen::Five] {
            for (language, nickname, ot) in [
                (Language::Japanese, "ナエトル", "ヒカリ"),
                (Language::Korean, "모부기", "루비"),
            ] {
                let mut mon = turtwig();
                mon.extra = Extra::empty(generation);
                mon.language = language;
                mon.set_nickname(nickname).unwrap();
                mon.set_ot(ot).unwrap();
                let back = Pokemon::decode(generation, &mon.encode()).unwrap();
                assert_eq!(back, mon);
                assert_eq!(back.language, language);
                assert_eq!(back.nickname_text(), nickname);
                assert_eq!(back.ot_text(), ot);
            }
        }
    }

    #[test]
    fn language_bytes_follow_the_games_numbering() {
        assert_eq!(Language::from_byte(1), Language::Japanese);
        assert_eq!(Language::from_byte(8), Language::Korean);
        assert_eq!(Language::from_byte(6), Language::Other(6));
        assert_eq!(Language::Spanish.to_byte(), 7);
    }

    #[test]
    fn smogons_shiny_example_is_shiny() {
        assert!(is_shiny(0xB58F_0B2A, 43288, 6075));
        assert!(!is_shiny(0xB58F_0B2A, 43289, 6075 ^ 0x10));
    }

    #[test]
    fn ribbons_count_only_real_bits() {
        let ribbons = Ribbons {
            sinnoh: 0xF000_0001,
            hoenn: 0x3,
            sinnoh_contest: 0xFFF0_0000,
        };
        assert_eq!(ribbons.count(), 3);
    }
}
