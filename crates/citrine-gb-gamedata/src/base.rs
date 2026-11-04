use crate::game::{Game, Generation};
use crate::scan::{Pattern, banked, byte, word};
use crate::state::{Dvs, Gender, Stats};

const GEN1_RECORD: usize = 28;
const GEN2_RECORD: usize = 32;
const GEN1_DEX: usize = 151;
const GEN2_DEX: usize = 251;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaseStats {
    pub dex: u8,
    pub hp: u8,
    pub attack: u8,
    pub defense: u8,
    pub speed: u8,
    pub special_attack: u8,
    pub special_defense: u8,
    pub types: [u8; 2],
    pub catch_rate: u8,
    pub base_exp: u8,
    pub gender_ratio: Option<u8>,
    pub growth_rate: GrowthRate,
    pub egg_cycles: Option<u8>,
    pub pic_size: u8,
    pub(crate) front_pic: u16,
    pub(crate) back_pic: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrowthRate {
    MediumFast,
    SlightlyFast,
    SlightlySlow,
    MediumSlow,
    Fast,
    Slow,
}

impl GrowthRate {
    fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => Self::MediumFast,
            1 => Self::SlightlyFast,
            2 => Self::SlightlySlow,
            3 => Self::MediumSlow,
            4 => Self::Fast,
            5 => Self::Slow,
            _ => return None,
        })
    }

    pub fn exp_at(self, level: u8) -> u32 {
        let n = i64::from(level);
        let (num, den, square, linear, constant) = match self {
            Self::MediumFast => (1, 1, 0, 0, 0),
            Self::SlightlyFast => (3, 4, 10, 0, 30),
            Self::SlightlySlow => (3, 4, 20, 0, 70),
            Self::MediumSlow => (6, 5, -15, 100, 140),
            Self::Fast => (4, 5, 0, 0, 0),
            Self::Slow => (5, 4, 0, 0, 0),
        };
        let exp = num * n * n * n / den + square * n * n + linear * n - constant;
        exp.clamp(0, i64::from(u32::MAX)) as u32
    }
}

impl BaseStats {
    pub fn from_record(generation: Generation, record: &[u8]) -> Option<Self> {
        match generation {
            Generation::One => record.get(..GEN1_RECORD).and_then(parse_gen1),
            Generation::Two => record.get(..GEN2_RECORD).and_then(parse_gen2),
            Generation::Three => None,
        }
    }

    pub fn stats(&self, level: u8, dvs: Dvs, stat_exp: [u16; 5]) -> Stats {
        let stat = |base: u8, dv: u8, exp: u16, bonus: u16| {
            let root = (1..=255u32)
                .find(|r| r * r >= u32::from(exp))
                .unwrap_or(255);
            let core = (u32::from(base) + u32::from(dv)) * 2 + root / 4;
            let value = core * u32::from(level) / 100 + u32::from(bonus);
            value.min(999) as u16
        };
        let normal = |base, dv, exp| stat(base, dv, exp, 5);
        Stats {
            max_hp: stat(self.hp, dvs.hp(), stat_exp[0], u16::from(level) + 10),
            attack: normal(self.attack, dvs.attack, stat_exp[1]),
            defense: normal(self.defense, dvs.defense, stat_exp[2]),
            speed: normal(self.speed, dvs.speed, stat_exp[3]),
            special_attack: normal(self.special_attack, dvs.special, stat_exp[4]),
            special_defense: normal(self.special_defense, dvs.special, stat_exp[4]),
        }
    }

    pub fn gender(&self, dvs: Dvs) -> Option<Gender> {
        let ratio = self.gender_ratio?;
        let roll = (dvs.attack << 4) | dvs.speed;
        match ratio {
            0xFF => None,
            0 => Some(Gender::Male),
            0xFE => Some(Gender::Female),
            _ if ratio < roll => Some(Gender::Male),
            _ => Some(Gender::Female),
        }
    }

    pub fn exp_to_next_level(&self, level: u8, exp: u32) -> Option<u32> {
        (level < 100).then(|| self.growth_rate.exp_at(level + 1).saturating_sub(exp))
    }
}

pub(crate) fn load(game: Game, rom: &[u8]) -> Vec<BaseStats> {
    match game.generation() {
        Generation::One => load_gen1(rom),
        Generation::Two => find_table(rom, GEN2_RECORD, GEN2_DEX)
            .map(|at| records(rom, at, GEN2_RECORD, GEN2_DEX, parse_gen2))
            .unwrap_or_default(),
        Generation::Three => Vec::new(),
    }
}

fn load_gen1(rom: &[u8]) -> Vec<BaseStats> {
    if let Some(at) = find_table(rom, GEN1_RECORD, GEN1_DEX) {
        return records(rom, at, GEN1_RECORD, GEN1_DEX, parse_gen1);
    }
    let Some(at) = find_table(rom, GEN1_RECORD, GEN1_DEX - 1) else {
        return Vec::new();
    };
    let mut table = records(rom, at, GEN1_RECORD, GEN1_DEX - 1, parse_gen1);
    if let Some(mew) = find_mew(rom) {
        table.push(mew);
    }
    table
}

fn find_table(rom: &[u8], record: usize, count: usize) -> Option<usize> {
    let span = record * count;
    (0..rom.len().saturating_sub(span)).find(|&at| {
        (0..count).all(|i| usize::from(rom[at + i * record]) == i + 1)
            && (0..count).all(|i| {
                let growth = match record {
                    GEN1_RECORD => rom[at + i * record + 19],
                    _ => rom[at + i * record + 22],
                };
                GrowthRate::from_byte(growth).is_some()
            })
    })
}

fn records(
    rom: &[u8],
    at: usize,
    record: usize,
    count: usize,
    parse: fn(&[u8]) -> Option<BaseStats>,
) -> Vec<BaseStats> {
    rom[at..at + record * count]
        .chunks_exact(record)
        .filter_map(parse)
        .collect()
}

fn parse_gen1(r: &[u8]) -> Option<BaseStats> {
    Some(BaseStats {
        dex: r[0],
        hp: r[1],
        attack: r[2],
        defense: r[3],
        speed: r[4],
        special_attack: r[5],
        special_defense: r[5],
        types: [r[6], r[7]],
        catch_rate: r[8],
        base_exp: r[9],
        gender_ratio: None,
        growth_rate: GrowthRate::from_byte(r[19])?,
        egg_cycles: None,
        pic_size: r[10],
        front_pic: u16::from_le_bytes([r[11], r[12]]),
        back_pic: u16::from_le_bytes([r[13], r[14]]),
    })
}

fn parse_gen2(r: &[u8]) -> Option<BaseStats> {
    Some(BaseStats {
        dex: r[0],
        hp: r[1],
        attack: r[2],
        defense: r[3],
        speed: r[4],
        special_attack: r[5],
        special_defense: r[6],
        types: [r[7], r[8]],
        catch_rate: r[9],
        base_exp: r[10],
        gender_ratio: Some(r[13]),
        growth_rate: GrowthRate::from_byte(r[22])?,
        egg_cycles: Some(r[15]),
        pic_size: r[17],
        front_pic: 0,
        back_pic: 0,
    })
}

fn find_mew(rom: &[u8]) -> Option<BaseStats> {
    let pattern = Pattern::new("21 ?? ?? 11 ?? ?? 01 1C 00 3E ?? CD");
    let home = &rom[..rom.len().min(0x4000)];
    pattern.find_all(home).find_map(|at| {
        let bank = usize::from(byte(rom, at + 10)?);
        let record = banked(rom, bank, word(rom, at + 1)?)?;
        let bytes = rom.get(record..record + GEN1_RECORD)?;
        (usize::from(bytes[0]) == GEN1_DEX)
            .then(|| parse_gen1(bytes))
            .flatten()
    })
}
