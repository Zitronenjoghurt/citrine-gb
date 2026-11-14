mod autosaves;
mod backend;
mod battery;
pub mod cloud;
mod crash;
#[cfg(any(target_arch = "wasm32", test))]
mod mirror;
mod names;
mod snapshots;
mod writes;

pub(crate) use backend::Backend;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use backend::TestDirs;
#[cfg(target_arch = "wasm32")]
pub use backend::open_saves;
#[cfg(not(target_arch = "wasm32"))]
pub use backend::{cache_dir, data_dir};
use citrine_gb::console::{ConsoleKind, VideoSpec};
pub use crash::CrashSave;
pub use writes::{Outcome, Write, Writes, Written};

pub fn thumb_size(console: Option<ConsoleKind>) -> [usize; 2] {
    let video = console.unwrap_or(ConsoleKind::Gb).video();
    [video.width / 2, video.height / 2]
}

const BATTERY: &str = "battery.sav";
const BATTERY_META: &str = "battery.meta";
const BATTERY_NAME: &str = "battery.name";

fn snapshot_blob(slot: usize) -> String {
    format!("slot{slot}.snap")
}

fn snapshot_meta(slot: usize) -> String {
    format!("slot{slot}.meta")
}

fn snapshot_thumb(slot: usize) -> String {
    format!("slot{slot}.thumb")
}

fn snapshot_name(slot: usize) -> String {
    format!("slot{slot}.name")
}

fn autosave_blob(id: usize) -> String {
    format!("auto{id}.snap")
}

fn autosave_meta(id: usize) -> String {
    format!("auto{id}.meta")
}

fn autosave_thumb(id: usize) -> String {
    format!("auto{id}.thumb")
}

fn autosave_cloud(id: usize) -> String {
    format!("auto{id}.cloud")
}

pub const BORROWED: &str = "borrowed-";
#[cfg(not(target_arch = "wasm32"))]
pub const CODECS: &str = "codecs";
#[cfg(not(target_arch = "wasm32"))]
pub const MEDIA_TEMP: &str = "media-temp";
#[cfg(not(target_arch = "wasm32"))]
pub const EXPORT: &str = "export";
#[cfg(not(target_arch = "wasm32"))]
pub const UPDATES: &str = "updates";
#[cfg(not(target_arch = "wasm32"))]
pub const REPORTS: &str = "reports";

pub const MAX_SAVE_NAME: usize = 60;

pub struct SnapshotInfo {
    pub slot: usize,
    pub saved_at: u64,
    pub thumbnail: Vec<u8>,
    pub name: Option<String>,
    pub console: Option<ConsoleKind>,
}

impl SnapshotInfo {
    pub fn thumb_size(&self) -> [usize; 2] {
        thumb_size(self.console)
    }

    pub fn display_name(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => default_slot_name(self.slot),
        }
    }
}

fn console_code(console: ConsoleKind) -> u8 {
    match console {
        ConsoleKind::Gb => 0,
        ConsoleKind::Gbc => 1,
        ConsoleKind::Gba => 2,
        ConsoleKind::Nds => 3,
    }
}

fn console_of_code(code: u8) -> Option<ConsoleKind> {
    match code {
        0 => Some(ConsoleKind::Gb),
        1 => Some(ConsoleKind::Gbc),
        2 => Some(ConsoleKind::Gba),
        3 => Some(ConsoleKind::Nds),
        _ => None,
    }
}

fn encode_meta(saved_at: u64, console: ConsoleKind) -> Vec<u8> {
    let mut meta = saved_at.to_le_bytes().to_vec();
    meta.push(console_code(console));
    meta
}

fn decode_meta(meta: &[u8]) -> (Option<u64>, Option<ConsoleKind>) {
    let saved_at = meta
        .get(..8)
        .and_then(|b| b.try_into().ok())
        .map(u64::from_le_bytes);
    let console = meta.get(8).copied().and_then(console_of_code);
    (saved_at, console)
}

pub fn default_slot_name(slot: usize) -> String {
    format!("Slot {}", slot + 1)
}

const SETTINGS_NAMESPACE: &str = "settings";
const SETTINGS: &str = "settings.json";
#[cfg(feature = "gba")]
const GBA_BIOS: &str = "gba-bios.bin";
#[cfg(ds_wifi)]
const DS_WIFI: &str = "nds-wifi.bin";
#[cfg(feature = "gba")]
const GBA_BIOS_NAME: &str = "gba-bios-name.txt";

pub struct SettingsStore {
    backend: Backend,
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self {
            backend: Backend::settings(),
        }
    }
}

impl SettingsStore {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn at(root: std::path::PathBuf) -> Self {
        Self {
            backend: Backend::at(root),
        }
    }

    pub fn load<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        let raw = self.backend.read(SETTINGS_NAMESPACE, SETTINGS)?;
        match serde_json::from_slice(&raw) {
            Ok(value) => Some(value),
            Err(err) => {
                log::error!("Ignoring unreadable settings: {err}");
                None
            }
        }
    }

    pub fn store<T: serde::Serialize>(&self, value: &T) -> std::io::Result<()> {
        let raw = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
        self.backend.write(SETTINGS_NAMESPACE, SETTINGS, &raw)
    }

    pub fn location(&self) -> String {
        self.backend.location()
    }

    #[cfg(feature = "nds")]
    pub fn ds_file(&self, file: crate::app::ds_firmware::DsFile) -> Option<(Vec<u8>, String)> {
        let (image_key, name_key) = file.store_keys();
        let image = self.backend.read(SETTINGS_NAMESPACE, image_key)?;
        let name = self
            .backend
            .read(SETTINGS_NAMESPACE, name_key)
            .and_then(|name| String::from_utf8(name).ok())
            .unwrap_or_else(|| image_key.to_owned());
        Some((image, name))
    }

    #[cfg(feature = "nds")]
    pub fn store_ds_file(
        &self,
        file: crate::app::ds_firmware::DsFile,
        value: Option<(&[u8], &str)>,
    ) -> std::io::Result<()> {
        let (image_key, name_key) = file.store_keys();
        match value {
            Some((image, name)) => {
                self.backend.write(SETTINGS_NAMESPACE, image_key, image)?;
                self.backend
                    .write(SETTINGS_NAMESPACE, name_key, name.as_bytes())
            }
            None => {
                self.backend.delete(SETTINGS_NAMESPACE, name_key)?;
                self.backend.delete(SETTINGS_NAMESPACE, image_key)
            }
        }
    }

    #[cfg(ds_wifi)]
    pub fn ds_wifi(&self) -> Option<crate::ds_wifi::Identity> {
        crate::ds_wifi::Identity::from_bytes(&self.backend.read(SETTINGS_NAMESPACE, DS_WIFI)?)
    }

    #[cfg(ds_wifi)]
    pub fn store_ds_wifi(&self, identity: &crate::ds_wifi::Identity) -> std::io::Result<()> {
        self.backend
            .write(SETTINGS_NAMESPACE, DS_WIFI, &identity.to_bytes())
    }

    #[cfg(feature = "gba")]
    pub fn gba_bios(&self) -> Option<(Vec<u8>, String)> {
        let image = self.backend.read(SETTINGS_NAMESPACE, GBA_BIOS)?;
        let name = self
            .backend
            .read(SETTINGS_NAMESPACE, GBA_BIOS_NAME)
            .and_then(|name| String::from_utf8(name).ok())
            .unwrap_or_else(|| GBA_BIOS.to_owned());
        Some((image, name))
    }

    #[cfg(feature = "gba")]
    pub fn store_gba_bios(&self, bios: Option<(&[u8], &str)>) -> std::io::Result<()> {
        match bios {
            Some((image, name)) => {
                self.backend.write(SETTINGS_NAMESPACE, GBA_BIOS, image)?;
                self.backend
                    .write(SETTINGS_NAMESPACE, GBA_BIOS_NAME, name.as_bytes())
            }
            None => {
                self.backend.delete(SETTINGS_NAMESPACE, GBA_BIOS_NAME)?;
                self.backend.delete(SETTINGS_NAMESPACE, GBA_BIOS)
            }
        }
    }
}

const SPECIES_NAMES: &str = "species-names";

#[derive(Clone)]
pub struct SaveStore {
    backend: Backend,
    cache: Backend,
    #[expect(
        clippy::disallowed_types,
        reason = "clones of a SaveStore on side jobs share one set of written keys"
    )]
    touched: std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<String>>>,
}

impl Default for SaveStore {
    fn default() -> Self {
        Self {
            backend: Backend::default(),
            cache: Backend::cache(),
            touched: Default::default(),
        }
    }
}

impl SaveStore {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn at(root: std::path::PathBuf) -> Self {
        Self::in_dir(root)
    }

    pub fn location(&self) -> String {
        self.backend.location()
    }

    fn write(&self, rom_key: &str, name: &str, data: &[u8]) -> std::io::Result<()> {
        self.touch(rom_key);
        self.backend.write(rom_key, name, data)
    }

    fn delete(&self, rom_key: &str, name: &str) -> std::io::Result<()> {
        self.touch(rom_key);
        self.backend.delete(rom_key, name)
    }

    fn touch(&self, rom_key: &str) {
        if cloud::is_game_key(rom_key) {
            self.touched
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(rom_key.to_string());
        }
    }
}

impl SaveStore {
    pub fn save_keys(&self) -> Vec<String> {
        self.backend
            .namespaces()
            .into_iter()
            .filter(|ns| is_save_key(ns))
            .collect()
    }

    pub fn files(&self) -> Vec<(String, String)> {
        let mut files = Vec::new();
        for game in self.save_keys() {
            for name in self.backend.list(&game) {
                if is_user_file(&name) {
                    files.push((game.clone(), name));
                }
            }
        }
        files.sort();
        files
    }

    pub fn read_file(&self, rom_key: &str, name: &str) -> Option<Vec<u8>> {
        self.backend.read(rom_key, name)
    }

    pub fn file_size(&self, rom_key: &str, name: &str) -> u64 {
        self.backend.size(rom_key, name)
    }

    pub fn put_file(&self, rom_key: &str, name: &str, data: &[u8]) -> std::io::Result<()> {
        self.write(rom_key, name, data)
    }

    pub fn remove_file(&self, rom_key: &str, name: &str) -> std::io::Result<()> {
        self.delete(rom_key, name)
    }

    pub fn delete_every_game(&self) -> std::io::Result<()> {
        for game in self.save_keys() {
            self.backend.clear(&game)?;
        }
        Ok(())
    }

    pub fn clear_cache(&self) -> std::io::Result<()> {
        for namespace in self.cache.namespaces() {
            self.cache.clear(&namespace)?;
        }
        Ok(())
    }

    pub fn covers(&self) -> crate::covers::CoverStore {
        crate::covers::CoverStore::on(self.cache.clone())
    }

    pub fn game_facts(&self) -> crate::game_facts::InfoStore {
        crate::game_facts::InfoStore::on(self.cache.clone())
    }

    pub fn species_names(&self, generation: u8) -> Option<Vec<String>> {
        let raw = self
            .cache
            .read(SPECIES_NAMES, &format!("gen{generation}"))?;
        let text = String::from_utf8(raw).ok()?;
        Some(text.split('\n').map(str::to_owned).collect())
    }

    pub fn put_species_names(&self, generation: u8, names: &[String]) -> std::io::Result<()> {
        self.cache.write(
            SPECIES_NAMES,
            &format!("gen{generation}"),
            names.join("\n").as_bytes(),
        )
    }

    pub fn move_cache_out(&self) {
        for namespace in crate::covers::NAMESPACES {
            let names = self.backend.list(namespace);
            if names.is_empty() {
                continue;
            }
            for name in &names {
                if let Some(data) = self.backend.read(namespace, name)
                    && let Err(err) = self.cache.write(namespace, name, &data)
                {
                    log::warn!("couldn't move {namespace}/{name} to the cache: {err}");
                }
            }
            match self.backend.clear(namespace) {
                Ok(()) => log::info!(
                    "moved {} files of {namespace} out of the saves",
                    names.len()
                ),
                Err(err) => log::warn!("couldn't remove {namespace} from the saves: {err}"),
            }
        }
    }
}

pub fn is_save_key(name: &str) -> bool {
    cloud::is_game_key(name)
        || name
            .strip_prefix(BORROWED)
            .is_some_and(|hash| hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn move_out_of_data(data: &std::path::Path, cache: &std::path::Path) {
    let codecs = data.join(CODECS);
    if codecs.is_dir() {
        let moved = std::fs::create_dir_all(cache)
            .and_then(|()| std::fs::rename(&codecs, cache.join(CODECS)));
        if let Err(err) = moved {
            log::info!("the video codec downloads again, it couldn't move to the cache: {err}");
        }
    }
    for old in [
        codecs,
        data.join("media").join(".library").join("temp"),
        data.join(EXPORT),
        data.join(UPDATES),
        data.join(REPORTS),
    ] {
        match std::fs::remove_dir_all(&old) {
            Ok(()) => log::info!("removed {} from the data folder", old.display()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => log::warn!("couldn't remove {}: {err}", old.display()),
        }
    }
}

pub fn is_user_file(name: &str) -> bool {
    let Some((stem, role)) = name.rsplit_once('.') else {
        return false;
    };
    let safe = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    };
    safe(stem) && matches!(role, "sav" | "meta" | "name" | "snap" | "thumb")
}

pub(crate) fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut writer = brotli::CompressorWriter::new(&mut out, 4096, 6, 22);
    if std::io::Write::write_all(&mut writer, data).is_err() {
        drop(writer);
        return data.to_vec();
    }
    drop(writer);
    out
}

pub(crate) fn decompress(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut reader = brotli::Decompressor::new(data, 4096);
    std::io::Read::read_to_end(&mut reader, &mut out).ok()?;
    Some(out)
}

pub fn now_unix() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn downscale(frame: &[u8], video: VideoSpec) -> Vec<u8> {
    let [width, height] = [video.width / 2, video.height / 2];
    let source_width = width * 2;
    let mut out = vec![0u8; width * height * 4];
    if frame.len() < source_width * height * 2 * 4 {
        return out;
    }
    for y in 0..height {
        for x in 0..width {
            for channel in 0..4 {
                let at = |dy: usize, dx: usize| -> u32 {
                    frame[(((y * 2 + dy) * source_width) + x * 2 + dx) * 4 + channel] as u32
                };
                let sum = at(0, 0) + at(0, 1) + at(1, 0) + at(1, 1);
                out[(y * width + x) * 4 + channel] = (sum / 4) as u8;
            }
        }
    }
    out
}

#[cfg(target_arch = "wasm32")]
pub fn keep_crash_save(crash: &CrashSave) {
    backend::keep_crash_save(&crash.encode());
}

#[cfg(target_arch = "wasm32")]
fn recover_crash_save(raw: &str) {
    let Some(crash) = CrashSave::decode(raw) else {
        log::error!("the autosave kept in local storage is damaged, dropping it");
        return;
    };
    match SaveStore::default().recover_crash_save(&crash) {
        Ok(true) => log::info!("took the autosave kept in local storage, it was newer"),
        Ok(false) => {}
        Err(err) => log::error!("couldn't take the autosave kept in local storage: {err}"),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use citrine_gb::console::ConsoleKind;

    const KEY: &str = "ABC123";

    fn store(name: &str) -> (SaveStore, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("citrine-store-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(root.with_file_name(format!(
            "citrine-store-test-{}-{name}-cache",
            std::process::id()
        )));
        (SaveStore::at(root.clone()), root)
    }

    fn settings_store(name: &str) -> SettingsStore {
        let root = std::env::temp_dir().join(format!(
            "citrine-settings-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        SettingsStore::at(root)
    }

    fn frame() -> Vec<u8> {
        (0..160 * 144 * 4).map(|i| (i % 256) as u8).collect()
    }

    #[test]
    fn settings_round_trip() {
        let store = settings_store("round-trip");
        assert!(store.load::<Vec<String>>().is_none(), "nothing stored yet");

        let value = vec!["a".to_string(), "b".to_string()];
        store.store(&value).expect("store");
        assert_eq!(store.load::<Vec<String>>().as_ref(), Some(&value));
    }

    #[test]
    fn settings_that_cannot_be_read_fall_back_to_none() {
        let store = settings_store("unreadable");
        store.store(&"a string").expect("store");

        assert!(store.load::<Vec<String>>().is_none());
    }

    #[test]
    fn battery_round_trips() {
        let (store, root) = store("battery");
        assert!(store.load_battery(KEY).is_none());
        assert!(!store.has_battery(KEY));

        store.store_battery(KEY, &[1, 2, 3]).expect("store");
        assert!(store.has_battery(KEY));
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&[1, 2, 3][..]));

        store.store_battery(KEY, &[4, 5]).expect("overwrite");
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&[4, 5][..]));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn writes_leave_no_temp_file_behind() {
        let (store, root) = store("atomic");
        store.store_battery(KEY, &[7; 64]).expect("store");
        let leftovers: Vec<_> = std::fs::read_dir(root.join(KEY))
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "stray temp files: {leftovers:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_game_boy_battery_save_is_stored_as_before() {
        use citrine_gb::persistence::battery_save::BatterySave;
        let (store, root) = store("battery-gb");
        let save = BatterySave::raw(vec![3; 0x2000]);
        store.store_battery_save(KEY, &save).expect("store");
        assert_eq!(store.load_battery(KEY), Some(save.to_file()));
        let meta = store.read_file(KEY, "battery.meta").expect("meta");
        assert_eq!(meta.len(), 8);
        assert!(store.battery_saved_at(KEY).is_some_and(|t| t > 0));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_gba_battery_save_keeps_its_chip_next_to_the_raw_file() {
        use citrine_gb::gba::machine::Gba;
        use citrine_gb::gba::save::SaveType;
        let (store, root) = store("battery-gba");
        let gba = Gba::new(&[0; 0x200]);
        for chip in [
            SaveType::Sram32K,
            SaveType::Sram64K,
            SaveType::Flash64K,
            SaveType::Flash128K,
            SaveType::Eeprom,
            SaveType::Eeprom512,
            SaveType::Eeprom8K,
        ] {
            let mut save = citrine_gb::persistence::battery_save::BatterySave::raw(vec![1, 2, 3]);
            save.chip = Some(chip);
            store.store_battery_save(KEY, &save).expect("store");
            assert_eq!(store.load_battery(KEY).as_deref(), Some(&[1, 2, 3][..]));
            let loaded = store.load_battery_save(KEY, &gba).expect("load");
            assert_eq!(loaded.chip, Some(chip));
            assert!(store.battery_saved_at(KEY).is_some_and(|t| t > 0));
        }
        store.store_battery(KEY, &[4]).expect("raw");
        assert_eq!(store.load_battery_save(KEY, &gba).expect("load").chip, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn battery_records_when_it_was_written() {
        let (store, root) = store("battery-meta");
        assert!(store.battery_saved_at(KEY).is_none());
        store.store_battery(KEY, &[1]).expect("store");
        assert!(store.battery_saved_at(KEY).is_some_and(|t| t > 0));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_reimport_overwrites_a_stored_save() {
        let (store, root) = store("reimport");
        let rom_dir = root.join("roms");
        std::fs::create_dir_all(&rom_dir).expect("rom dir");
        let rom = rom_dir.join("game.gb");
        std::fs::write(&rom, b"rom").expect("rom");
        std::fs::write(rom_dir.join("game.sav"), b"from-file").expect("sav");

        assert!(SaveStore::has_legacy_save(&rom));
        store.store_battery(KEY, b"stored").expect("stored");

        assert!(!store.import_legacy_save_if_new(KEY, &rom));
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"stored"[..]));

        assert!(store.import_legacy_save(KEY, &rom));
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"from-file"[..]));
        assert!(rom_dir.join("game.sav").exists(), "original is kept");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_legacy_file_means_no_import() {
        let (store, root) = store("no-legacy");
        let rom_dir = root.join("roms");
        std::fs::create_dir_all(&rom_dir).expect("rom dir");
        let rom = rom_dir.join("game.gb");
        std::fs::write(&rom, b"rom").expect("rom");

        assert!(!SaveStore::has_legacy_save(&rom));
        assert!(!store.import_legacy_save_if_new(KEY, &rom));
        assert!(store.load_battery(KEY).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_save_is_adopted_once_and_original_kept() {
        let (store, root) = store("legacy");
        let rom_dir = root.join("roms");
        std::fs::create_dir_all(&rom_dir).expect("rom dir");
        let rom = rom_dir.join("game.gb");
        std::fs::write(&rom, b"rom").expect("rom");
        std::fs::write(rom_dir.join("game.sav"), b"old-save").expect("sav");

        assert!(
            store.import_legacy_save_if_new(KEY, &rom),
            "first import runs"
        );
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"old-save"[..]));
        assert!(rom_dir.join("game.sav").exists(), "original is kept");

        store.store_battery(KEY, b"newer").expect("newer");
        assert!(
            !store.import_legacy_save_if_new(KEY, &rom),
            "does not re-import"
        );
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"newer"[..]));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn snapshot_round_trips_with_metadata() {
        let (store, root) = store("snapshot");
        assert!(store.snapshot_info(KEY, 0).is_none());

        store
            .store_snapshot(KEY, 3, b"state-bytes", &frame(), ConsoleKind::Gb)
            .expect("store");
        assert_eq!(
            store.load_snapshot(KEY, 3).as_deref(),
            Some(&b"state-bytes"[..])
        );

        let info = store.snapshot_info(KEY, 3).expect("filled");
        assert_eq!(info.slot, 3);
        assert!(info.saved_at > 0, "timestamp recorded");
        assert_eq!(info.thumbnail.len(), 80 * 72 * 4);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn slots_are_unbounded_and_listed_in_order() {
        let (store, root) = store("unbounded");
        for slot in [0, 5, 99, 1000] {
            store
                .store_snapshot(KEY, slot, b"state", &frame(), ConsoleKind::Gb)
                .expect("store");
        }
        assert_eq!(store.used_slots(KEY), vec![0, 5, 99, 1000]);
        assert_eq!(store.snapshot_infos(KEY).len(), 4);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn next_free_slot_fills_gaps_before_extending() {
        let (store, root) = store("nextfree");
        assert_eq!(store.next_free_slot(KEY), 0);

        for slot in [0, 1, 2] {
            store
                .store_snapshot(KEY, slot, b"state", &frame(), ConsoleKind::Gb)
                .expect("store");
        }
        assert_eq!(store.next_free_slot(KEY), 3);

        store.delete_snapshot(KEY, 1).expect("delete");
        assert_eq!(store.next_free_slot(KEY), 1, "reuses the gap");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn thumbnails_are_stored_compressed() {
        let (store, root) = store("compress");
        store
            .store_snapshot(KEY, 0, b"state", &frame(), ConsoleKind::Gb)
            .expect("store");

        let raw = 80 * 72 * 4;
        let on_disk = std::fs::metadata(root.join(KEY).join("slot0.thumb"))
            .expect("thumb")
            .len() as usize;
        assert!(
            on_disk < raw / 2,
            "expected compression, got {on_disk} of {raw} bytes"
        );

        let info = store.snapshot_info(KEY, 0).expect("filled");
        assert_eq!(info.thumbnail.len(), raw);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn snapshots_and_battery_share_one_namespace() {
        let (store, root) = store("namespace");
        store.store_battery(KEY, b"sram").expect("battery");
        store
            .store_snapshot(KEY, 0, b"state", &frame(), ConsoleKind::Gb)
            .expect("snapshot");
        assert!(root.join(KEY).join("battery.sav").exists());
        assert!(root.join(KEY).join("slot0.snap").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_slot_with_no_name_counts_from_one() {
        let (store, root) = store("unnamed");
        store
            .store_snapshot(KEY, 2, b"state", &frame(), ConsoleKind::Gb)
            .expect("store");

        let info = store.snapshot_info(KEY, 2).expect("filled");
        assert_eq!(info.name, None, "nothing was written for a fresh slot");
        assert_eq!(
            info.display_name(),
            "Slot 3",
            "the third slot reads as Slot 3"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_save_from_before_one_based_names_still_loads() {
        let (store, root) = store("old-layout");
        let dir = root.join(KEY);
        std::fs::create_dir_all(&dir).expect("dir");
        store
            .store_snapshot(KEY, 0, b"old state", &frame(), ConsoleKind::Gb)
            .expect("store");
        std::fs::write(dir.join("slot1.name"), "Slot 0").expect("old name");
        store
            .store_snapshot(KEY, 1, b"named", &frame(), ConsoleKind::Gb)
            .expect("store");

        assert_eq!(store.used_slots(KEY), vec![0, 1]);
        assert_eq!(
            store.load_snapshot(KEY, 0).as_deref(),
            Some(&b"old state"[..])
        );
        let infos = store.snapshot_infos(KEY);
        assert_eq!(infos[0].display_name(), "Slot 1");
        assert_eq!(
            infos[1].display_name(),
            "Slot 0",
            "a stored name is kept as written"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn naming_a_slot_sticks() {
        let (store, root) = store("named");
        store
            .store_snapshot(KEY, 0, b"state", &frame(), ConsoleKind::Gb)
            .expect("store");

        store
            .set_snapshot_name(KEY, 0, "  Before the boss  ")
            .expect("name");

        let info = store.snapshot_info(KEY, 0).expect("filled");
        assert_eq!(info.name.as_deref(), Some("Before the boss"), "trimmed");
        assert_eq!(info.display_name(), "Before the boss");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn overwriting_a_slot_keeps_its_name() {
        let (store, root) = store("named-overwrite");
        store
            .store_snapshot(KEY, 1, b"first", &frame(), ConsoleKind::Gb)
            .expect("store");
        store.set_snapshot_name(KEY, 1, "Checkpoint").expect("name");

        store
            .store_snapshot(KEY, 1, b"second", &frame(), ConsoleKind::Gb)
            .expect("overwrite");

        let info = store.snapshot_info(KEY, 1).expect("filled");
        assert_eq!(info.display_name(), "Checkpoint");
        assert_eq!(store.load_snapshot(KEY, 1).as_deref(), Some(&b"second"[..]));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_blank_name_goes_back_to_the_default() {
        let (store, root) = store("name-cleared");
        store
            .store_snapshot(KEY, 0, b"state", &frame(), ConsoleKind::Gb)
            .expect("store");
        store.set_snapshot_name(KEY, 0, "Temporary").expect("name");

        store.set_snapshot_name(KEY, 0, "   ").expect("clear");

        let info = store.snapshot_info(KEY, 0).expect("filled");
        assert_eq!(info.name, None);
        assert_eq!(info.display_name(), "Slot 1");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_name_matching_the_default_is_not_stored() {
        let (store, root) = store("name-default");
        store
            .store_snapshot(KEY, 4, b"state", &frame(), ConsoleKind::Gb)
            .expect("store");

        store.set_snapshot_name(KEY, 4, "Slot 5").expect("name");

        assert_eq!(store.snapshot_name(KEY, 4), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn names_are_capped() {
        let (store, root) = store("name-length");
        store
            .store_snapshot(KEY, 0, b"state", &frame(), ConsoleKind::Gb)
            .expect("store");

        let long = "x".repeat(MAX_SAVE_NAME * 3);
        store.set_snapshot_name(KEY, 0, &long).expect("name");

        let stored = store.snapshot_name(KEY, 0).expect("named");
        assert_eq!(stored.chars().count(), MAX_SAVE_NAME);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn deleting_a_slot_takes_its_name_with_it() {
        let (store, root) = store("name-delete");
        store
            .store_snapshot(KEY, 0, b"state", &frame(), ConsoleKind::Gb)
            .expect("store");
        store.set_snapshot_name(KEY, 0, "Gone soon").expect("name");

        store.delete_snapshot(KEY, 0).expect("delete");

        assert_eq!(store.snapshot_name(KEY, 0), None, "no orphaned name file");
        assert!(store.snapshot_info(KEY, 0).is_none());

        store
            .store_snapshot(KEY, 0, b"fresh", &frame(), ConsoleKind::Gb)
            .expect("store");
        assert_eq!(
            store.snapshot_info(KEY, 0).expect("filled").display_name(),
            "Slot 1"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn names_do_not_look_like_snapshot_slots() {
        let (store, root) = store("name-listing");
        store
            .store_snapshot(KEY, 7, b"state", &frame(), ConsoleKind::Gb)
            .expect("store");
        store.set_snapshot_name(KEY, 7, "Named").expect("name");

        assert_eq!(
            store.used_slots(KEY),
            vec![7],
            "the name file must not register as another slot"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_battery_save_has_no_name_until_one_is_given() {
        let (store, root) = store("battery-unnamed");
        store.store_battery(KEY, b"sram").expect("battery");
        assert_eq!(store.battery_name(KEY), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn renaming_the_battery_save_sticks_across_writes() {
        let (store, root) = store("battery-named");
        store.store_battery(KEY, b"first").expect("battery");

        store
            .set_battery_name(KEY, "  Main run  ", "POKEMON_SLV")
            .expect("name");
        store.store_battery(KEY, b"second").expect("overwrite");

        assert_eq!(store.battery_name(KEY).as_deref(), Some("Main run"));
        assert_eq!(store.load_battery(KEY).as_deref(), Some(&b"second"[..]));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_blank_or_default_battery_name_is_not_stored() {
        let (store, root) = store("battery-name-default");
        store
            .set_battery_name(KEY, "Nuzlocke", "TETRIS")
            .expect("name");

        store.set_battery_name(KEY, "   ", "TETRIS").expect("clear");
        assert_eq!(store.battery_name(KEY), None);

        store
            .set_battery_name(KEY, "TETRIS", "TETRIS")
            .expect("default");
        assert_eq!(store.battery_name(KEY), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn battery_names_are_capped_and_do_not_look_like_slots() {
        let (store, root) = store("battery-name-cap");
        let long = "y".repeat(MAX_SAVE_NAME * 2);
        store.set_battery_name(KEY, &long, "TETRIS").expect("name");

        assert_eq!(
            store.battery_name(KEY).map(|n| n.chars().count()),
            Some(MAX_SAVE_NAME)
        );
        assert!(store.used_slots(KEY).is_empty());
        assert!(!store.has_battery(KEY), "a name alone is not a save");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn thumbnail_averages_each_2x2_block() {
        let mut frame = vec![0u8; 160 * 144 * 4];
        for (i, dx) in [(0usize, 0usize), (1, 4), (160, 8), (161, 12)] {
            frame[i * 4] = dx as u8;
        }
        let thumb = downscale(&frame, ConsoleKind::Gb.video());
        assert_eq!(thumb.len(), 80 * 72 * 4);
        assert_eq!(thumb[0], 6);
    }

    #[test]
    fn a_thumbnail_is_half_the_console_screen() {
        assert_eq!(thumb_size(Some(ConsoleKind::Gb)), [80, 72]);
        assert_eq!(thumb_size(Some(ConsoleKind::Gbc)), [80, 72]);
        assert_eq!(thumb_size(Some(ConsoleKind::Gba)), [120, 80]);
        assert_eq!(thumb_size(None), [80, 72]);
    }

    #[test]
    fn a_gba_snapshot_keeps_its_console_and_wide_thumbnail() {
        let (store, root) = store("gba-thumb");
        let frame = vec![200u8; 240 * 160 * 4];
        store
            .store_snapshot(KEY, 0, b"state", &frame, ConsoleKind::Gba)
            .unwrap();
        let info = store.snapshot_info(KEY, 0).unwrap();
        assert_eq!(info.console, Some(ConsoleKind::Gba));
        assert_eq!(info.thumb_size(), [120, 80]);
        assert_eq!(info.thumbnail, vec![200u8; 120 * 80 * 4]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn autosaves_keep_only_the_newest() {
        let (store, root) = store("auto-keep");
        for n in 0..5u8 {
            store
                .store_autosave(KEY, &[n], &frame(), ConsoleKind::Gb, 3)
                .expect("autosave");
        }

        assert_eq!(store.autosave_ids(KEY), vec![2, 3, 4]);
        let newest_first: Vec<usize> = store.autosave_infos(KEY).iter().map(|i| i.slot).collect();
        assert_eq!(newest_first, vec![4, 3, 2]);
        assert_eq!(store.load_autosave(KEY, 4).as_deref(), Some(&[4u8][..]));
        assert_eq!(store.load_autosave(KEY, 1), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_lower_limit_drops_the_oldest_at_once() {
        let (store, root) = store("auto-shrink");
        for _ in 0..4 {
            store
                .store_autosave(KEY, b"state", &frame(), ConsoleKind::Gb, 10)
                .expect("autosave");
        }
        store.prune_autosaves(KEY, 1).expect("prune");
        assert_eq!(store.autosave_ids(KEY), vec![3]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn autosaves_never_touch_manual_snapshots() {
        let (store, root) = store("auto-apart");
        store
            .store_snapshot(KEY, 0, b"mine", &frame(), ConsoleKind::Gb)
            .expect("snapshot");
        store.set_snapshot_name(KEY, 0, "Boss fight").expect("name");

        for _ in 0..3 {
            store
                .store_autosave(KEY, b"auto", &frame(), ConsoleKind::Gb, 1)
                .expect("autosave");
        }

        assert_eq!(store.used_slots(KEY), vec![0]);
        assert_eq!(store.next_free_slot(KEY), 1);
        assert_eq!(store.load_snapshot(KEY, 0).as_deref(), Some(&b"mine"[..]));
        assert_eq!(store.snapshot_name(KEY, 0).as_deref(), Some("Boss fight"));
        assert_eq!(store.autosave_ids(KEY).len(), 1);

        store.delete_snapshot(KEY, 0).expect("delete");
        assert_eq!(store.autosave_ids(KEY).len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_eight_megabyte_state_round_trips_through_every_native_store() {
        let (store, root) = store("big");
        let big: Vec<u8> = (0..8u32 << 20).map(|i| (i % 251) as u8).collect();
        store
            .store_snapshot(KEY, 0, &big, &frame(), ConsoleKind::Gb)
            .expect("snapshot");
        assert_eq!(store.load_snapshot(KEY, 0), Some(big.clone()));
        let id = store
            .store_autosave(KEY, &big, &frame(), ConsoleKind::Gb, 3)
            .expect("autosave");
        assert_eq!(store.load_autosave(KEY, id), Some(big.clone()));
        store.store_battery(KEY, &big).expect("battery");
        assert_eq!(store.load_battery(KEY), Some(big));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn covers_are_written_outside_the_saves() {
        let (store, root) = store("covers-outside");
        store.covers().put_art(KEY, b"art").unwrap();
        assert_eq!(store.covers().art(KEY).as_deref(), Some(&b"art"[..]));
        assert!(!root.join("covers").exists());
        assert!(!root.join("cover-names").exists());
    }

    #[test]
    fn the_move_takes_covers_and_their_names_out_of_the_saves() {
        let (store, root) = store("covers-move");
        let saves = Backend::at(root.clone());
        saves
            .write("covers", &format!("{KEY}.png"), b"art")
            .unwrap();
        saves.write("cover-names", "GB", b"names").unwrap();
        store.move_cache_out();
        assert!(!root.join("covers").exists());
        assert!(!root.join("cover-names").exists());
        assert_eq!(store.covers().art(KEY).as_deref(), Some(&b"art"[..]));
        assert_eq!(
            store.cache.read("cover-names", "GB").as_deref(),
            Some(&b"names"[..])
        );
        store.move_cache_out();
        assert_eq!(store.covers().art(KEY).as_deref(), Some(&b"art"[..]));
    }

    #[test]
    fn borrowed_game_saves_are_listed_and_deleted_with_the_rest() {
        let (store, root) = store("borrowed");
        let game = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let borrowed = "borrowed-00112233445566778899aabbccddeeff00112233";
        store.put_file(game, "battery.sav", b"mine").unwrap();
        store.put_file(borrowed, "battery.sav", b"theirs").unwrap();
        store.put_file("cloud", "ledger.bin", b"ledger").unwrap();
        assert_eq!(
            store.files(),
            [
                (game.to_owned(), "battery.sav".to_owned()),
                (borrowed.to_owned(), "battery.sav".to_owned()),
            ]
        );
        store.delete_every_game().unwrap();
        assert!(store.files().is_empty());
        assert!(!root.join(borrowed).exists());
        assert!(
            root.join("cloud").exists(),
            "the cloud list stays with the account"
        );
        assert!(!is_save_key("borrowed-zz"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn clearing_the_cache_empties_every_cache_folder() {
        let (store, root) = store("clear-cache");
        store.covers().put_art(KEY, b"art").unwrap();
        store
            .cache
            .write(CODECS, "libopenh264.so", b"codec")
            .unwrap();
        store
            .cache
            .write(MEDIA_TEMP, "recording.mp4.part", b"half")
            .unwrap();
        store.clear_cache().unwrap();
        assert!(store.cache.namespaces().is_empty());
        assert_eq!(store.covers().art(KEY), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn old_cache_folders_leave_the_data_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (data, cache) = (dir.path().join("data"), dir.path().join("cache"));
        let temp = data.join("media").join(".library").join("temp");
        for folder in [
            data.join(CODECS),
            temp.clone(),
            data.join(EXPORT).join("3"),
            data.join(UPDATES),
            data.join(REPORTS),
        ] {
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("file"), b"x").unwrap();
        }
        std::fs::create_dir_all(data.join("saves")).unwrap();
        move_out_of_data(&data, &cache);
        assert_eq!(
            std::fs::read(cache.join(CODECS).join("file")).unwrap(),
            b"x"
        );
        for gone in [CODECS, EXPORT, UPDATES, REPORTS] {
            assert!(!data.join(gone).exists(), "{gone}");
        }
        assert!(!temp.exists());
        assert!(data.join("media").join(".library").exists());
        assert!(data.join("saves").exists());
        move_out_of_data(&data, &cache);
        assert!(cache.join(CODECS).join("file").exists());
    }

    #[test]
    fn species_names_live_in_the_cache() {
        let (store, root) = store("species-names");
        assert_eq!(store.species_names(4), None);
        let names = vec![String::new(), "BULBASAUR".to_owned(), "IVYSAUR".to_owned()];
        store.put_species_names(4, &names).unwrap();
        assert_eq!(store.species_names(4), Some(names));
        assert_eq!(store.species_names(5), None);
        assert!(store.games().is_empty());
        assert!(!root.join("species-names").exists());
    }
}
