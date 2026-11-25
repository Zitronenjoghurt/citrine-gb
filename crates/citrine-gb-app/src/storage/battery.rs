use super::{BATTERY, BATTERY_META, SaveStore, now_unix};
use citrine_gb::console::Console;
#[cfg(feature = "gba")]
use citrine_gb::gba::save::SaveType;
use citrine_gb::persistence::battery_save::BatterySave;

#[cfg(not(target_arch = "wasm32"))]
pub(super) const MAX_LEGACY_SAVE_BYTES: u64 = 32 << 20;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatteryFiles {
    pub sav: Vec<u8>,
    pub meta: Option<Vec<u8>>,
}

impl SaveStore {
    pub fn battery_files(&self, rom_key: &str) -> Option<BatteryFiles> {
        Some(BatteryFiles {
            sav: self.load_battery(rom_key)?,
            meta: self.backend.read(rom_key, BATTERY_META),
        })
    }

    pub fn put_battery_files(&self, rom_key: &str, files: &BatteryFiles) -> std::io::Result<()> {
        self.write(rom_key, BATTERY, &files.sav)?;
        match &files.meta {
            Some(meta) => self.write(rom_key, BATTERY_META, meta),
            None => self.delete(rom_key, BATTERY_META),
        }
    }

    pub fn remove_battery(&self, rom_key: &str) -> std::io::Result<()> {
        self.delete(rom_key, BATTERY_META)?;
        self.delete(rom_key, BATTERY)
    }

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
        use crate::utils::zip_games;
        let zipped = zip_games::split_entry(rom_path).filter(|_| !rom_path.is_file());
        let (Some((zip, _)), Some(name)) = (zipped, rom_path.file_name()) else {
            return rom_path.with_extension("sav");
        };
        let game = zip.with_file_name(name).with_extension("sav");
        let pack = zip.with_extension("sav");
        let alone = || zip_games::entries(&zip).is_ok_and(|games| games.len() == 1);
        if !Self::is_legacy_save_file(&game) && Self::is_legacy_save_file(&pack) && alone() {
            return pack;
        }
        game
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn has_legacy_save(rom_path: &std::path::Path) -> bool {
        Self::is_legacy_save_file(&Self::legacy_save_path(rom_path))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn is_legacy_save_file(path: &std::path::Path) -> bool {
        std::fs::symlink_metadata(path)
            .is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_LEGACY_SAVE_BYTES)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn import_legacy_save(&self, rom_key: &str, rom_path: &std::path::Path) -> bool {
        let Some(data) = read_legacy_save(&Self::legacy_save_path(rom_path)) else {
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

#[cfg(not(target_arch = "wasm32"))]
fn read_legacy_save(path: &std::path::Path) -> Option<Vec<u8>> {
    use std::io::Read;
    if !SaveStore::is_legacy_save_file(path) {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_LEGACY_SAVE_BYTES {
        return None;
    }
    let mut data = Vec::new();
    file.take(MAX_LEGACY_SAVE_BYTES + 1)
        .read_to_end(&mut data)
        .ok()?;
    (data.len() as u64 <= MAX_LEGACY_SAVE_BYTES).then_some(data)
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
