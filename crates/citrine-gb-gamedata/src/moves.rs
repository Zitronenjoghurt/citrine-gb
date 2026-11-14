use crate::game::{Game, Generation};

const GEN1_RECORD: usize = 6;
const GEN2_RECORD: usize = 7;
const GEN1_MOVES: usize = 165;
const GEN2_MOVES: usize = 251;
const MAX_PP_BONUS: u8 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveData {
    pub type_id: u8,
    pub power: u8,
    pub accuracy: u8,
    pub pp: u8,
}

impl MoveData {
    pub fn max_pp(self, pp_ups: u8) -> u8 {
        self.pp + (self.pp / 5).min(MAX_PP_BONUS) * pp_ups.min(3)
    }

    pub fn accuracy_percent(self) -> u8 {
        ((u16::from(self.accuracy) * 100 + 127) / 255) as u8
    }
}

pub(crate) fn load(game: Game, rom: &[u8]) -> Vec<MoveData> {
    let (record, count) = match game.generation() {
        Generation::One => (GEN1_RECORD, GEN1_MOVES),
        Generation::Two => (GEN2_RECORD, GEN2_MOVES),
        Generation::Three | Generation::Four | Generation::Five => return Vec::new(),
    };
    let span = record * count;
    let Some(at) = (0..rom.len().saturating_sub(span)).find(|&at| {
        (0..count).all(|i| usize::from(rom[at + i * record]) == i + 1)
            && (0..count).all(|i| rom[at + i * record + 5] <= 40)
    }) else {
        return Vec::new();
    };
    rom[at..at + span]
        .chunks_exact(record)
        .map(|r| MoveData {
            type_id: r[3],
            power: r[2],
            accuracy: r[4],
            pp: r[5],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::fixtures::crystal_rom;
    use crate::fixtures::{game, put};
    use crate::test_support::red_rom;
    use crate::{Dvs, Effect, Language, MoveData, MoveSlot, RomTables, Version};

    const FIRE: u8 = 0x14;

    const WATER: u8 = 0x15;

    const GRASS: u8 = 0x16;

    const GHOST: u8 = 0x08;

    const NORMAL: u8 = 0x00;

    const ROCK: u8 = 0x05;

    pub(crate) fn move_table(count: usize, record: usize) -> Vec<u8> {
        (1..=count)
            .flat_map(|id| {
                let mut r = vec![0u8; record];
                r[0] = id as u8;
                r[2] = if id == 2 { 0 } else { 40 + (id % 5) as u8 * 10 };
                r[3] = match id {
                    1 => WATER,
                    2 => NORMAL,
                    3 => NORMAL,
                    _ => FIRE,
                };
                r[4] = if id == 1 { 0xFF } else { 0xE6 };
                r[5] = match id {
                    1 => 40,
                    2 => 20,
                    _ => 15,
                };
                r
            })
            .collect()
    }

    pub(crate) fn type_chart(foresight: bool) -> Vec<u8> {
        let mut rules = vec![
            (WATER, FIRE, 20),
            (FIRE, WATER, 5),
            (WATER, ROCK, 20),
            (FIRE, GRASS, 20),
            (WATER, GRASS, 5),
        ];
        for attack in 0x16..0x1B {
            for defend in 0x01..0x0F {
                rules.push((attack, defend, if defend % 3 == 0 { 20 } else { 5 }));
            }
        }
        let mut bytes = vec![0xFF];
        for (a, d, m) in rules {
            bytes.extend([a, d, m]);
        }
        if foresight {
            bytes.push(0xFE);
        }
        bytes.extend([NORMAL, GHOST, 0, 0xFF]);
        bytes
    }

    fn red() -> RomTables {
        let mut rom = red_rom();
        put(&mut rom, 0x1E * 0x4000, &move_table(165, 6));
        put(&mut rom, 0x1E * 0x4000 + 0x2000, &type_chart(false));
        RomTables::load(game(Version::Red, Language::English), &rom).expect("tables")
    }

    fn crystal() -> RomTables {
        let mut rom = crystal_rom();
        put(&mut rom, 0x60 * 0x4000, &move_table(251, 7));
        put(&mut rom, 0x60 * 0x4000 + 0x3000, &type_chart(true));
        RomTables::load(game(Version::Crystal, Language::English), &rom).expect("tables")
    }

    #[test]
    fn move_type_power_and_pp_come_from_the_rom() {
        for tables in [red(), crystal()] {
            assert_eq!(
                tables.move_data(1),
                Some(MoveData {
                    type_id: WATER,
                    power: 50,
                    accuracy: 0xFF,
                    pp: 40
                })
            );
            assert_eq!(tables.move_data(1).unwrap().accuracy_percent(), 100);
            assert_eq!(tables.move_data(3).unwrap().accuracy_percent(), 90);
            assert_eq!(tables.move_data(0), None);
        }
        assert_eq!(red().move_data(166), None);
        assert!(crystal().move_data(251).is_some());
    }

    #[test]
    fn pp_ups_add_a_fifth_each_up_to_seven() {
        let tables = crystal();
        let slot = |id, pp_ups| MoveSlot { id, pp: 0, pp_ups };
        assert_eq!(tables.max_pp(slot(1, 0)), Some(40));
        assert_eq!(tables.max_pp(slot(1, 3)), Some(61));
        assert_eq!(tables.max_pp(slot(2, 1)), Some(24));
        assert_eq!(tables.max_pp(slot(3, 3)), Some(24));
        assert_eq!(tables.max_pp(slot(0, 0)), None);
    }

    #[test]
    fn moves_meet_the_type_chart_of_the_rom() {
        for tables in [red(), crystal()] {
            assert_eq!(tables.effect(1, [FIRE, FIRE]), Some(Effect::Double));
            assert_eq!(tables.effect(1, [FIRE, ROCK]), Some(Effect::Quadruple));
            assert_eq!(tables.effect(1, [GRASS, FIRE]), Some(Effect::Normal));
            assert_eq!(tables.effect(1, [GRASS, GRASS]), Some(Effect::Half));
            assert_eq!(tables.effect(4, [WATER, WATER]), Some(Effect::Half));
            assert_eq!(tables.effect(3, [GHOST, GHOST]), Some(Effect::NoEffect));
            assert_eq!(tables.effect(3, [NORMAL, NORMAL]), Some(Effect::Normal));
            assert_eq!(tables.effect(2, [FIRE, FIRE]), None);
        }
    }

    #[test]
    fn a_rom_without_a_chart_leaves_effects_unknown() {
        let tables =
            RomTables::load(game(Version::Red, Language::English), &red_rom()).expect("tables");
        assert_eq!(tables.effect(1, [FIRE, FIRE]), None);
        assert_eq!(tables.move_data(1), None);
    }

    #[test]
    fn hidden_power_follows_the_dvs() {
        let dvs = |attack, defense, speed, special| Dvs {
            attack,
            defense,
            speed,
            special,
        };
        assert_eq!(dvs(15, 15, 15, 15).hidden_power(), (0x1B, 70));
        assert_eq!(dvs(0, 0, 0, 0).hidden_power(), (0x01, 31));
        assert_eq!(dvs(12, 13, 0, 8).hidden_power(), (0x02, 63));
        assert_eq!(dvs(1, 1, 7, 7).hidden_power(), (0x07, 32));
        assert_eq!(dvs(2, 0, 7, 7).hidden_power(), (0x14, 32));
        assert_eq!(dvs(2, 2, 7, 7).hidden_power(), (0x16, 32));
    }
}
