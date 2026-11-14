use crate::error::GbResult;
use crate::gb::ppu::types::theme::DmgTheme;
use crate::gb::{GameBoy, GbModel};
use crate::rom::header::RomHeader;
use std::path::Path;

#[derive(Debug)]
pub struct E2ETest {
    pub meta: E2EMeta,
    pub png: Vec<u8>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct E2EMeta {
    pub model: GbModel,
    pub theme: DmgTheme,
    pub name: String,
    pub description: String,
    pub rom: E2EMetaRom,
    pub cycles: u128,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct E2EMetaRom {
    pub title: String,
    pub licensee: String,
    pub cartridge_type: String,
    pub rom_banks: usize,
    pub ram_banks: usize,
    pub crc_32: String,
    pub sha1: String,
    pub sha256: String,
}

impl E2ETest {
    pub fn create(gb: &GameBoy, name: String, description: String) -> Self {
        Self {
            meta: E2EMeta::create(gb, name, description),
            png: gb.ppu.frame().render_png(),
        }
    }

    pub fn export(&self, tests_dir: &Path) -> GbResult<()> {
        let test_dir = tests_dir.join(&self.meta.name);
        std::fs::create_dir_all(&test_dir)?;
        std::fs::write(
            test_dir.join("meta.json"),
            serde_json::to_string_pretty(&self.meta)?,
        )?;
        std::fs::write(test_dir.join("output.png"), &self.png)?;
        Ok(())
    }

    pub fn import(test_dir: &Path) -> GbResult<Self> {
        let meta = std::fs::read_to_string(test_dir.join("meta.json"))?;
        let meta: E2EMeta = serde_json::from_str(&meta)?;
        let png = std::fs::read(test_dir.join("output.png"))?;
        Ok(Self { meta, png })
    }
}

impl E2EMeta {
    pub fn create(gb: &GameBoy, name: String, description: String) -> Self {
        Self {
            model: gb.model,
            theme: gb.ppu.dmg_theme,
            name,
            description,
            rom: E2EMetaRom::from(&gb.cartridge.header),
            cycles: gb.debugger.total_cycles,
        }
    }
}

impl From<&RomHeader> for E2EMetaRom {
    fn from(header: &RomHeader) -> Self {
        Self {
            title: header.title.to_string(),
            licensee: header.licensee.to_string(),
            cartridge_type: header.cartridge_type.unwrap().to_string(),
            rom_banks: header.rom_banks,
            ram_banks: header.ram_banks,
            crc_32: header.crc32_hex_string(),
            sha1: header.sha1_hex_string(),
            sha256: header.sha256_hex_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::debug::e2e::E2ETest;
    use crate::gb::GameBoy;
    use crate::rom::Rom;
    use crate::rom::id::RomId;
    use crate::test_support::LedgerRoms;
    use std::collections::{HashMap, HashSet};
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
                    let sha_hex = RomId::new(&data).sha256_hex_string();
                    rom_map.insert(sha_hex, path);
                }
            }
        }
    }

    #[test]
    pub fn run() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let roms_dir = Path::new(manifest_dir).join("../../roms/test");
        let test_dir = Path::new(manifest_dir).join("../../tests").join("e2e");

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
                    "the ledger archives ({e}); put the ROMs in roms/test or run `make ledger-check` online once"
                ),
                _ => "their ROMs in roms/test or the ledger archives".to_string(),
            };
            let tests = match skipped.len() {
                1..=3 => skipped.join(", "),
                n => format!("{n} of {total} tests"),
            };
            citrine_gb_test_roms::missing(&format!("e2e {tests}"), reason);
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
}
