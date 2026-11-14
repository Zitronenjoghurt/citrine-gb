use super::{ExtractError, u16_at};
use crate::pkm::Gen;
use citrine_gb_nds_files::Narc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LevelMove {
    pub level: u8,
    pub move_id: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Evolution {
    pub method: u16,
    pub param: u16,
    pub into: u16,
}

pub(super) fn read_learnsets(
    generation: Gen,
    narc: &Narc<'_>,
) -> Result<Vec<Vec<LevelMove>>, ExtractError> {
    narc.files()
        .map(|file| {
            let file = file?;
            Ok(match generation {
                Gen::Four => gen4_learnset(file),
                Gen::Five => gen5_learnset(file),
            })
        })
        .collect()
}

fn gen4_learnset(file: &[u8]) -> Vec<LevelMove> {
    file.as_chunks::<2>()
        .0
        .iter()
        .map(|w| u16::from_le_bytes([w[0], w[1]]))
        .take_while(|&packed| packed != 0xFFFF)
        .map(|packed| LevelMove {
            level: (packed >> 9) as u8,
            move_id: packed & 0x1FF,
        })
        .collect()
}

fn gen5_learnset(file: &[u8]) -> Vec<LevelMove> {
    file.as_chunks::<4>()
        .0
        .iter()
        .take_while(|pair| u16_at(*pair, 0) != 0xFFFF)
        .map(|pair| LevelMove {
            level: u16_at(pair, 2) as u8,
            move_id: u16_at(pair, 0),
        })
        .collect()
}

pub(super) fn read_evolutions(narc: &Narc<'_>) -> Result<Vec<Vec<Evolution>>, ExtractError> {
    narc.files()
        .map(|file| {
            Ok(file?
                .as_chunks::<6>()
                .0
                .iter()
                .take(7)
                .map(|e| Evolution {
                    method: u16_at(e, 0),
                    param: u16_at(e, 2),
                    into: u16_at(e, 4),
                })
                .filter(|e| e.method != 0)
                .collect())
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn learnset_record(generation: Gen, moves: &[LevelMove]) -> Vec<u8> {
    let mut r = Vec::new();
    for m in moves {
        match generation {
            Gen::Four => r.extend_from_slice(&(m.move_id | u16::from(m.level) << 9).to_le_bytes()),
            Gen::Five => {
                r.extend_from_slice(&m.move_id.to_le_bytes());
                r.extend_from_slice(&u16::from(m.level).to_le_bytes());
            }
        }
    }
    match generation {
        Gen::Four => r.extend_from_slice(&[0xFF, 0xFF, 0, 0]),
        Gen::Five => r.extend_from_slice(&[0xFF; 4]),
    }
    r
}

#[cfg(test)]
pub(crate) fn evolution_record(evolutions: &[Evolution]) -> Vec<u8> {
    let mut r = vec![0u8; 44];
    for (i, e) in evolutions.iter().enumerate() {
        r[i * 6..i * 6 + 2].copy_from_slice(&e.method.to_le_bytes());
        r[i * 6 + 2..i * 6 + 4].copy_from_slice(&e.param.to_le_bytes());
        r[i * 6 + 4..i * 6 + 6].copy_from_slice(&e.into.to_le_bytes());
    }
    r
}

#[cfg(test)]
pub(crate) fn turtwig_moves() -> Vec<LevelMove> {
    [
        (1, 33),
        (5, 71),
        (9, 74),
        (13, 75),
        (17, 44),
        (21, 76),
        (37, 446),
    ]
    .map(|(level, move_id)| LevelMove { level, move_id })
    .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_nds_files::fixtures::narc;

    #[test]
    fn reads_packed_gen4_learnsets() {
        let bytes = narc::build(&[
            learnset_record(Gen::Four, &[]),
            learnset_record(Gen::Four, &turtwig_moves()),
        ]);
        let table = read_learnsets(Gen::Four, &Narc::parse(&bytes).unwrap()).unwrap();
        assert!(table[0].is_empty());
        assert_eq!(table[1], turtwig_moves());
    }

    #[test]
    fn gen4_packing_keeps_moves_over_255_and_level_100() {
        let moves = [LevelMove {
            level: 100,
            move_id: 467,
        }];
        let bytes = narc::build(&[learnset_record(Gen::Four, &moves)]);
        let table = read_learnsets(Gen::Four, &Narc::parse(&bytes).unwrap()).unwrap();
        assert_eq!(table[0], moves);
    }

    #[test]
    fn reads_gen5_learnset_pairs() {
        let bytes = narc::build(&[learnset_record(Gen::Five, &turtwig_moves())]);
        let table = read_learnsets(Gen::Five, &Narc::parse(&bytes).unwrap()).unwrap();
        assert_eq!(table[0], turtwig_moves());
    }

    #[test]
    fn reads_evolutions_and_skips_empty_slots() {
        let grotle = Evolution {
            method: 4,
            param: 18,
            into: 388,
        };
        let bytes = narc::build(&[evolution_record(&[]), evolution_record(&[grotle])]);
        let table = read_evolutions(&Narc::parse(&bytes).unwrap()).unwrap();
        assert!(table[0].is_empty());
        assert_eq!(table[1], [grotle]);
    }
}
