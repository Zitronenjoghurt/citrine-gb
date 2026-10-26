use crate::debug::e2e::E2ETest;
use crate::gb::GameBoy;
use crate::rom::Rom;
use crate::rom::header::RomHeader;
use crate::tests::skip;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

#[path = "../../tests/archives/mod.rs"]
mod archives;

#[derive(Deserialize)]
struct Sources {
    sources: BTreeMap<String, archives::Source>,
}

struct LedgerRoms {
    archives: Vec<(zip::ZipArchive<File>, HashMap<String, String>)>,
}

impl LedgerRoms {
    fn load() -> Result<Self, String> {
        let manifest: Sources = archives::read_manifest()?;
        let mut archives = Vec::new();
        for (name, source) in &manifest.sources {
            let mut archive = archives::fetch(name, source)?;
            let index = rom_index(&source.sha256, &mut archive)?;
            archives.push((archive, index));
        }
        Ok(Self { archives })
    }

    fn find(&mut self, sha: &str) -> Result<Option<Vec<u8>>, String> {
        for (archive, index) in &mut self.archives {
            if let Some(path) = index.get(sha) {
                return archives::read_entry(archive, path).map(Some);
            }
        }
        Ok(None)
    }
}

fn parse_index(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(sha, path)| (sha.to_uppercase(), path.to_string()))
        .collect()
}

fn rom_index(
    archive_sha: &str,
    archive: &mut zip::ZipArchive<File>,
) -> Result<HashMap<String, String>, String> {
    let dir = archives::cache_dir();
    let path = dir.join(format!("{archive_sha}.roms"));
    if let Ok(text) = fs::read_to_string(&path) {
        return Ok(parse_index(&text));
    }

    let mut text = String::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.to_string();
        if !(name.ends_with(".gb") || name.ends_with(".gbc")) {
            continue;
        }
        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .map_err(|e| format!("{name} in archive: {e}"))?;
        text.push_str(&format!("{} {name}\n", archives::sha256_hex(&data)));
    }

    let partial = dir.join(format!("{archive_sha}.{}.roms.part", std::process::id()));
    fs::write(&partial, &text).map_err(|e| format!("{}: {e}", partial.display()))?;
    fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(parse_index(&text))
}

fn map_available_roms(dir: &Path, rom_map: &mut HashMap<String, PathBuf>) {
    if !dir.is_dir() {
        return;
    }

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                map_available_roms(&path, rom_map);
            } else if let Some(ext) = path.extension()
                && (ext == "gb" || ext == "gbc")
                && let Ok(data) = fs::read(&path)
            {
                let sha_hex = RomHeader::calculate_sha256(&data)
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<String>();
                rom_map.insert(sha_hex, path);
            }
        }
    }
}

#[test]
pub fn run() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let roms_dir = Path::new(manifest_dir).join("../roms/test");
    let test_dir = Path::new(manifest_dir).join("../tests").join("e2e");

    let mut rom_map = HashMap::new();
    map_available_roms(&roms_dir, &mut rom_map);

    let entries = fs::read_dir(&test_dir).unwrap_or_else(|e| {
        panic!("Failed to read e2e test directory at {:?}: {}", test_dir, e);
    });

    let mut used_rom_shas: HashSet<String> = HashSet::new();
    let mut ledger: Option<Result<LedgerRoms, String>> = None;
    let mut skipped = Vec::new();
    let mut total = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let test = E2ETest::import(&path).unwrap_or_else(|e| {
            panic!("Failed to import test at {:?}: {}", path, e);
        });

        let target_sha = &test.meta.rom.sha256;
        used_rom_shas.insert(target_sha.clone());
        total += 1;

        let rom_data = match rom_map.get(target_sha) {
            Some(rom_path) => fs::read(rom_path).expect("Failed to read ROM file"),
            None => match ledger.get_or_insert_with(LedgerRoms::load) {
                Ok(roms) => match roms.find(target_sha) {
                    Ok(Some(data)) => data,
                    Ok(None) => {
                        skipped.push(test.meta.name.clone());
                        continue;
                    }
                    Err(e) => panic!("Failed to read ROM for test '{}': {e}", test.meta.name),
                },
                Err(_) => {
                    skipped.push(test.meta.name.clone());
                    continue;
                }
            },
        };
        let rom = Rom::new(&rom_data);

        let mut gb = GameBoy::new_empty(test.meta.model);
        gb.load_rom(&rom).expect("Failed to load ROM into GameBoy");

        gb.ppu.dmg_theme = test.meta.theme;

        let mut remaining_cycles = test.meta.cycles;
        while remaining_cycles > 0 {
            let chunk = std::cmp::min(remaining_cycles, u32::MAX as u128) as u32;
            gb.run_cycles(chunk);
            remaining_cycles -= chunk as u128;
        }

        let actual_png = gb.frame().render_png();
        assert_eq!(
            test.png, actual_png,
            "❌ E2E Test '{}' failed! The rendered frame did not match the expected PNG.",
            test.meta.name
        );

        println!("✅ E2E Test '{}' passed.", test.meta.name);
    }

    if !skipped.is_empty() {
        let reason = match ledger {
            Some(Err(e)) => format!(
                "could not read the ledger archives ({e}), put the ROMs in roms/test or run `make ledger-check` online once"
            ),
            _ => "their ROMs are not in roms/test or the ledger archives".to_string(),
        };
        let tests = match skipped.len() {
            1..=3 => skipped.join(", "),
            n => format!("{n} of {total} tests"),
        };
        skip(&format!("e2e {tests}"), &reason);
    }

    for (sha, path) in &rom_map {
        if !used_rom_shas.contains(sha.as_str()) {
            println!(
                "⚠️ WARNING: ROM not covered by any E2E test: {:?}",
                path.display()
            );
        }
    }
}
