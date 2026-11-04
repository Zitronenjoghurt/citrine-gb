use super::{BATTERY, BATTERY_META, SaveStore, now_unix};
use citrine_gb::console::Console;
#[cfg(feature = "gba")]
use citrine_gb::gba::save::SaveType;
use citrine_gb::persistence::battery_save::BatterySave;

#[cfg(feature = "gba")]
const CHIPS: [SaveType; 7] = [
    SaveType::Sram32K,
    SaveType::Sram64K,
    SaveType::Flash64K,
    SaveType::Flash128K,
    SaveType::Eeprom,
    SaveType::Eeprom512,
    SaveType::Eeprom8K,
];

impl SaveStore {
    pub fn load_battery(&self, rom_key: &str) -> Option<Vec<u8>> {
        self.backend.read(rom_key, BATTERY)
    }

    pub fn store_battery(&self, rom_key: &str, data: &[u8]) -> std::io::Result<()> {
        self.write(rom_key, BATTERY, data)?;
        self.write(rom_key, BATTERY_META, &now_unix().to_le_bytes())
    }

    pub fn store_battery_save(&self, rom_key: &str, save: &BatterySave) -> std::io::Result<()> {
        self.write(rom_key, BATTERY, &save.to_file())?;
        self.write(rom_key, BATTERY_META, &battery_meta(save, now_unix()))
    }

    pub fn load_battery_save(&self, rom_key: &str, console: &dyn Console) -> Option<BatterySave> {
        let data = self.load_battery(rom_key)?;
        #[allow(unused_mut)]
        let mut save = console.battery_from_file(&data);
        #[cfg(feature = "gba")]
        {
            let tag = self
                .backend
                .read(rom_key, BATTERY_META)
                .and_then(|meta| meta.get(8).copied());
            save.chip = tag.and_then(|tag| CHIPS.get(usize::from(tag).checked_sub(1)?).copied());
        }
        Some(save)
    }

    pub fn battery_saved_at(&self, rom_key: &str) -> Option<u64> {
        self.backend
            .read(rom_key, BATTERY_META)
            .and_then(|b| b.get(..8)?.try_into().ok())
            .map(u64::from_le_bytes)
    }

    pub fn has_battery(&self, rom_key: &str) -> bool {
        self.backend.exists(rom_key, BATTERY)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn legacy_save_path(rom_path: &std::path::Path) -> std::path::PathBuf {
        rom_path.with_extension("sav")
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn has_legacy_save(rom_path: &std::path::Path) -> bool {
        Self::legacy_save_path(rom_path).is_file()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn import_legacy_save(&self, rom_key: &str, rom_path: &std::path::Path) -> bool {
        let Ok(data) = std::fs::read(Self::legacy_save_path(rom_path)) else {
            return false;
        };
        self.store_battery(rom_key, &data).is_ok()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn import_legacy_save_if_new(&self, rom_key: &str, rom_path: &std::path::Path) -> bool {
        if self.has_battery(rom_key) {
            return false;
        }
        self.import_legacy_save(rom_key, rom_path)
    }
}

pub(super) fn battery_meta(save: &BatterySave, saved_at: u64) -> Vec<u8> {
    let mut meta = saved_at.to_le_bytes().to_vec();
    meta.extend(chip_tag(save));
    meta
}

#[cfg(feature = "gba")]
fn chip_tag(save: &BatterySave) -> Option<u8> {
    let index = CHIPS.iter().position(|&chip| Some(chip) == save.chip)?;
    u8::try_from(index + 1).ok()
}

#[cfg(not(feature = "gba"))]
fn chip_tag(_save: &BatterySave) -> Option<u8> {
    None
}
