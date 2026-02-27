use crate::debug::e2e::E2ETest;
use crate::gb::GameBoy;
use crate::rom::header::RomHeader;
use crate::rom::Rom;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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
    let roms_dir = Path::new(manifest_dir).join("../roms");
    let test_dir = Path::new(manifest_dir).join("../tests").join("e2e");

    let mut rom_map = HashMap::new();
    map_available_roms(&roms_dir, &mut rom_map);

    let entries = fs::read_dir(&test_dir).unwrap_or_else(|e| {
        panic!("Failed to read e2e test directory at {:?}: {}", test_dir, e);
    });

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let test = E2ETest::import(&path).unwrap_or_else(|e| {
            panic!("Failed to import test at {:?}: {}", path, e);
        });

        let target_sha = &test.meta.rom.sha256;
        let rom_path = rom_map.get(target_sha).unwrap_or_else(|| {
            panic!(
                "Missing ROM for test '{}'. Expected SHA256: {}",
                test.meta.name, target_sha
            );
        });

        let rom_data = fs::read(rom_path).expect("Failed to read ROM file");
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
}
