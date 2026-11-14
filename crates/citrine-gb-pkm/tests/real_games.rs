use citrine_gb_test_roms as test_roms;
use std::path::{Path, PathBuf};

const SOURCES: [(&[u8; 4], &str); 2] = [(b"CPUE", "platinum"), (b"IPGE", "soulsilver")];
const COMMITTED: [&str; 7] = [
    "base_stats.tsv",
    "learnsets.tsv",
    "evolutions.tsv",
    "encounters.tsv",
    "places.tsv",
    "growth.tsv",
    "moves.tsv",
];

fn games(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("nds")))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

#[test]
#[ignore]
fn the_extractor_reproduces_the_committed_tables_from_the_games() {
    let test = "the_extractor_reproduces_the_committed_tables_from_the_games";
    let Some(dir) = test_roms::env_path(test, "CITRINE_DS_GAMES") else {
        return;
    };
    let Some(dir) = test_roms::dir(test, dir) else {
        return;
    };
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    let mut checked = Vec::new();
    for path in games(&dir) {
        let rom = std::fs::read(&path).unwrap();
        let Some(&(_, name)) = SOURCES
            .iter()
            .find(|(code, _)| rom.get(0x0C..0x10) == Some(&code[..]))
        else {
            continue;
        };
        let tables = citrine_gb_pkm::tables::extract(&rom).unwrap();
        for (file, text) in tables.to_tsv() {
            if COMMITTED.contains(&file) {
                let committed = std::fs::read_to_string(data.join(name).join(file)).unwrap();
                assert!(
                    committed == text,
                    "{name}/{file} differs from {}",
                    path.display()
                );
            }
        }
        checked.push(name);
    }
    if checked.is_empty() {
        test_roms::missing(
            test,
            format_args!("a Gen 4 Pokémon game in {}", dir.display()),
        );
    }
    eprintln!("checked {checked:?}");
}

#[test]
#[ignore]
fn the_name_book_reads_moves_items_types_and_places_from_every_game() {
    let test = "the_name_book_reads_moves_items_types_and_places_from_every_game";
    let Some(dir) = test_roms::env_path(test, "CITRINE_DS_GAMES") else {
        return;
    };
    let Some(dir) = test_roms::dir(test, dir) else {
        return;
    };
    let mut checked = Vec::new();
    for path in games(&dir) {
        let rom = std::fs::read(&path).unwrap();
        let code: [u8; 4] = rom[0x0C..0x10].try_into().unwrap();
        if citrine_gb_pkm::tables::Game::identify(code).is_none() {
            continue;
        }
        let shown = path.display();
        let book =
            citrine_gb_pkm::info::Book::load(&rom).unwrap_or_else(|e| panic!("{shown}: {e}"));
        let names = &book.names;
        assert!(
            ["Pound", "Pfund"].contains(&names.moves[1].as_str()),
            "{shown}: move 1 is {:?}",
            names.moves[1]
        );
        assert!(
            ["Master Ball", "Meisterball"].contains(&names.items[1].as_str()),
            "{shown}: item 1 is {:?}",
            names.items[1]
        );
        assert!(
            names.types.len() >= 17,
            "{shown}: {} types",
            names.types.len()
        );
        assert!(book.moves[1].power == 40, "{shown}: move 1 power");
        checked.push(format!("{:?} {:?}", book.tables.game, book.tables.language));
    }
    if checked.is_empty() {
        test_roms::missing(
            test,
            format_args!("a Gen 4 Pokémon game in {}", dir.display()),
        );
    }
    eprintln!("checked {checked:?}");
}
