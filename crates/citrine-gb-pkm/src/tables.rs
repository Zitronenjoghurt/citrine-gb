mod encounters;
mod game;
mod growth;
mod learnsets;
mod messages;
mod personal;
mod places;
mod tsv;

pub use encounters::{Encounter, EncounterMethod};
pub use game::Game;
pub use learnsets::{Evolution, LevelMove};
pub use messages::{gen4_messages, gen5_messages};
pub use personal::BaseStats;
pub use places::Place;
pub use tsv::TsvError;

use crate::pkm::{Gen, Language};
use citrine_gb_nds_files::{Narc, NdsRom, RomError};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExtractError {
    #[error("game code {0:?} is not a DS Pokémon game")]
    UnknownGame(String),
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error("{what} record {index} is {len} bytes, too short")]
    ShortRecord {
        what: &'static str,
        index: usize,
        len: usize,
    },
    #[error("message file is broken at {0:#x}")]
    BadMessages(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tables {
    pub game: Game,
    pub language: Language,
    pub base_stats: Vec<BaseStats>,
    pub learnsets: Vec<Vec<LevelMove>>,
    pub evolutions: Vec<Vec<Evolution>>,
    pub encounters: Vec<Encounter>,
    pub species_names: Vec<String>,
    pub places: Vec<Place>,
    pub growth: Vec<Vec<u32>>,
    pub pp: Vec<u8>,
}

pub fn extract(rom: &[u8]) -> Result<Tables, ExtractError> {
    let rom = NdsRom::parse(rom)?;
    let code = rom.game_code();
    let (game, language) = Game::identify(code)
        .ok_or_else(|| ExtractError::UnknownGame(String::from_utf8_lossy(&code).into_owned()))?;
    let paths = game.paths();
    let narc = |path: &str| -> Result<Narc<'_>, ExtractError> { Ok(Narc::parse(rom.file(path)?)?) };
    let generation = game.generation();
    let base_stats = personal::read(generation, &narc(paths.personal)?)?;
    let learnsets = learnsets::read_learnsets(generation, &narc(paths.learnsets)?)?;
    let evolutions = learnsets::read_evolutions(&narc(paths.evolutions)?)?;
    let places = match (paths.map_names, places::header_table(code), paths.zones) {
        (Some(names), Some(table), _) => places::read(game, &rom.arm9()?, table, rom.file(names)?)?,
        (_, _, Some(zones)) => places::read_zones(narc(zones)?.file(0)?),
        _ => Vec::new(),
    };
    let mut encounters = encounters::read(game, &narc(paths.encounters)?)?;
    if let Some(path) = paths.safari {
        encounters.extend(encounters::read_safari(&narc(path)?)?);
    }
    if let Some(path) = paths.bug_contest {
        encounters.extend(encounters::read_bug_contest(rom.file(path)?));
    }
    if let Some(path) = paths.swaps {
        let files: Vec<(u16, u16)> = places
            .iter()
            .filter_map(|p| Some((p.encounters?, p.location)))
            .collect();
        let swaps = encounters::daily_swaps(&encounters, &files, &narc(path)?)?;
        encounters.extend(swaps);
    }
    let growth = match paths.growth {
        Some(path) => growth::read_growth(&narc(path)?)?,
        None => Vec::new(),
    };
    let pp = match paths.moves {
        Some(path) => growth::read_pp(&narc(path)?)?,
        None => Vec::new(),
    };
    let messages = narc(paths.messages)?;
    let names_file = messages.file(paths.species_names)?;
    let species_names = match generation {
        Gen::Four => gen4_messages(names_file)?,
        Gen::Five => gen5_messages(names_file)?,
    };
    Ok(Tables {
        game,
        language,
        base_stats,
        learnsets,
        evolutions,
        encounters,
        species_names,
        places,
        growth,
        pp,
    })
}

impl Tables {
    pub fn to_tsv(&self) -> Vec<(&'static str, String)> {
        tsv::write(self)
    }

    pub fn from_tsv<'a>(
        game: Game,
        file: impl Fn(&str) -> Option<&'a str>,
    ) -> Result<Self, TsvError> {
        tsv::read(game, file)
    }
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn record<'a>(
    what: &'static str,
    index: usize,
    bytes: &'a [u8],
    len: usize,
) -> Result<&'a [u8], ExtractError> {
    bytes.get(..len).ok_or(ExtractError::ShortRecord {
        what,
        index,
        len: bytes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_nds_files::fixtures::{narc, nds};

    fn synthetic_rom(code: &[u8; 4]) -> Vec<u8> {
        let (game, _) = Game::identify(*code).unwrap();
        let generation = game.generation();
        let paths = game.paths();
        let turtwig = personal::turtwig();
        let personal_record = match generation {
            Gen::Four => personal::gen4_record(&turtwig),
            Gen::Five => personal::gen5_record(&turtwig),
        };
        let grotle = Evolution {
            method: 4,
            param: 18,
            into: 388,
        };
        let mut encounter = vec![0u8; if generation == Gen::Four { 0x1A8 } else { 0xE8 }];
        match game {
            Game::HeartGold | Game::SoulSilver => {
                encounter[0] = 30;
                encounter[0x08] = 5;
                encounter[0x2C..0x2E].copy_from_slice(&387u16.to_le_bytes());
            }
            Game::Diamond | Game::Pearl | Game::Platinum => {
                encounter[0] = 30;
                encounter[4] = 5;
                encounter[8..10].copy_from_slice(&387u16.to_le_bytes());
                encounter[52] = 20;
                encounter[56..58].copy_from_slice(&399u16.to_le_bytes());
            }
            _ => {
                encounter[0] = 30;
                encounter[8..10].copy_from_slice(&387u16.to_le_bytes());
                encounter[10] = 5;
                encounter[11] = 5;
            }
        }
        let names = match generation {
            Gen::Four => messages::gen4_file(&["-----", "Turtwig"], 0x55AA),
            Gen::Five => messages::gen5_file(&["-----", "Turtwig"]),
        };
        let mut message_files = vec![vec![0u8; 4]; paths.species_names];
        message_files.push(names);
        let mut files = vec![
            (
                paths.personal,
                narc::build(&[vec![0; personal_record.len()], personal_record]),
            ),
            (
                paths.learnsets,
                narc::build(&[learnsets::learnset_record(
                    generation,
                    &learnsets::turtwig_moves(),
                )]),
            ),
            (
                paths.evolutions,
                narc::build(&[learnsets::evolution_record(&[grotle])]),
            ),
            (paths.encounters, narc::build(&[encounter])),
            (paths.messages, narc::build(&message_files)),
        ];
        if let Some(path) = paths.growth {
            files.push((path, narc::build(&[growth::growth_record(|l| l * l * l)])));
        }
        if let Some(path) = paths.moves {
            files.push((
                path,
                narc::build(&[growth::move_record(0), growth::move_record(35)]),
            ));
        }
        if let Some(path) = paths.map_names {
            files.push((path, vec![0; 32]));
        }
        if let Some(path) = paths.zones {
            let zones = [places::zone_record(4), places::zone_record(81)].concat();
            files.push((path, narc::build(&[zones])));
        }
        if let Some(path) = paths.safari {
            let mut area = vec![0u8; 0x390];
            area[0x08..0x0A].copy_from_slice(&19u16.to_le_bytes());
            area[0x0A] = 15;
            files.push((path, narc::build(&[area])));
        }
        if let Some(path) = paths.bug_contest {
            let mut set = vec![0u8; 80];
            set[0..4].copy_from_slice(&[10, 0, 7, 18]);
            files.push((path, set));
        }
        if let Some(path) = paths.swaps {
            let mut lists = vec![vec![0u8; 4]; 11];
            lists[8] = 446u32.to_le_bytes().to_vec();
            files.push((path, narc::build(&lists)));
        }
        let mut arm9 = Vec::new();
        if let Some(table) = places::header_table(*code) {
            arm9 = nds::arm9_with_params(&[], 0);
            arm9.resize(table, 0);
            for place in synthetic_places() {
                arm9.extend(places::header_record(game, &place));
            }
        }
        nds::build_with_arm9(code, &arm9, &files)
    }

    fn synthetic_places() -> [Place; 2] {
        [
            Place {
                header: 0,
                encounters: None,
                location: 0,
                area_icon: 3,
                battle_background: 0,
            },
            Place {
                header: 1,
                encounters: Some(0),
                location: 68,
                area_icon: 3,
                battle_background: 8,
            },
        ]
    }

    #[test]
    fn extracts_every_table_from_each_game() {
        for code in [
            b"ADAE", b"APAE", b"CPUJ", b"IPKE", b"IPGK", b"IRBE", b"IRAE", b"IREE", b"IRDE",
        ] {
            let tables = extract(&synthetic_rom(code)).unwrap();
            assert_eq!(tables.base_stats[1], personal::turtwig(), "{code:?}");
            assert_eq!(tables.learnsets[0], learnsets::turtwig_moves());
            assert_eq!(tables.evolutions[0][0].into, 388);
            let first = tables.encounters[0];
            assert_eq!(
                (first.species, first.min_level, first.max_level),
                (387, 5, 5)
            );
            assert_eq!(tables.species_names, ["-----", "Turtwig"]);
        }
    }

    #[test]
    fn reads_places_growth_and_pp_where_the_game_has_them() {
        for code in [b"CPUE", b"IPGE", b"IPKE"] {
            let tables = extract(&synthetic_rom(code)).unwrap();
            assert_eq!(tables.places, synthetic_places(), "{code:?}");
            assert_eq!(tables.growth[0][100], 1_000_000);
            assert_eq!(tables.pp, [0, 35]);
        }
        let japanese = extract(&synthetic_rom(b"CPUJ")).unwrap();
        assert!(japanese.places.is_empty());
        let black = extract(&synthetic_rom(b"IRBD")).unwrap();
        let locations: Vec<u16> = black.places.iter().map(|p| p.location).collect();
        assert_eq!(locations, [4, 81]);
    }

    #[test]
    fn adds_the_safari_zone_and_bug_contest_in_hgss() {
        let tables = extract(&synthetic_rom(b"IPGE")).unwrap();
        let find = |m| tables.encounters.iter().find(|e| e.method == m).copied();
        let safari = find(EncounterMethod::SafariGrass).unwrap();
        assert_eq!(
            (safari.place, safari.slot, safari.species, safari.min_level),
            (0, 0, 19, 15)
        );
        let bug = find(EncounterMethod::BugContest).unwrap();
        assert_eq!((bug.species, bug.min_level, bug.max_level), (10, 7, 18));
    }

    #[test]
    fn adds_trophy_garden_swaps_in_platinum() {
        let tables = extract(&synthetic_rom(b"CPUE")).unwrap();
        let swaps: Vec<_> = tables
            .encounters
            .iter()
            .filter(|e| e.method == EncounterMethod::TrophyGarden)
            .map(|e| (e.place, e.slot, e.species, e.min_level))
            .collect();
        assert_eq!(swaps, [(0, 6, 446, 20)]);
    }

    #[test]
    fn writes_small_tab_separated_tables() {
        let tables = extract(&synthetic_rom(b"CPUE")).unwrap();
        let files = tables.to_tsv();
        let get = |name: &str| &files.iter().find(|(n, _)| *n == name).unwrap().1;
        assert!(get("encounters.tsv").contains("\n0\t-\tGrass\t0\t387\t0\t5\t5\n"));
        assert!(get("learnsets.tsv").contains("\n0\t37\t446\n"));
        assert!(get("species_names.tsv").contains("English\t1\tTurtwig\n"));
        assert!(
            get("base_stats.tsv")
                .lines()
                .nth(2)
                .unwrap()
                .starts_with("1\t55\t68\t64\t31")
        );
        assert!(get("evolutions.tsv").contains("\n0\t4\t18\t388\n"));
    }

    #[test]
    fn tables_read_back_from_their_files_write_the_same_files() {
        for code in [b"CPUE", b"IPGE", b"IRBE"] {
            let tables = extract(&synthetic_rom(code)).unwrap();
            let files = tables.to_tsv();
            let read = Tables::from_tsv(tables.game, |name| {
                files
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, t)| t.as_str())
            })
            .unwrap();
            assert_eq!(read.to_tsv(), files, "{code:?}");
        }
    }

    #[test]
    fn a_broken_row_names_its_file_and_line() {
        let broken = Tables::from_tsv(Game::Platinum, |name| {
            (name == "moves.tsv").then_some("move\tpp\n0\t0\n1\tlots\n")
        });
        assert_eq!(
            broken.unwrap_err(),
            TsvError::Broken {
                file: "moves.tsv",
                line: 3
            }
        );
    }

    #[test]
    fn other_carts_are_refused() {
        let rom = nds::build(b"AMCE", &[]);
        assert_eq!(extract(&rom), Err(ExtractError::UnknownGame("AMCE".into())));
    }
}
