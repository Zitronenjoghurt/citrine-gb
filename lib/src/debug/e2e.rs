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
            crc_32: format!("{:08X}", header.crc32),
            sha1: header
                .sha1
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<String>(),
            sha256: header
                .sha256
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<String>(),
        }
    }
}
