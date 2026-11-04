mod autosaves;
mod backend;
mod battery;
pub mod cloud;
mod crash;
#[cfg(any(target_arch = "wasm32", test))]
mod mirror;
mod names;
mod snapshots;

pub(crate) use backend::Backend;
#[cfg(not(target_arch = "wasm32"))]
pub use backend::data_dir;
#[cfg(target_arch = "wasm32")]
pub use backend::open_saves;
use citrine_gb::console::{ConsoleKind, VideoSpec};
pub use crash::CrashSave;

pub fn thumb_size(console: Option<ConsoleKind>) -> [usize; 2] {
    let video = console.unwrap_or(ConsoleKind::Gb).video();
    [video.width / 2, video.height / 2]
}

pub fn thumb_size_of(pixels: &[u8]) -> Option<[usize; 2]> {
    [ConsoleKind::Gb, ConsoleKind::Gba]
        .map(|kind| thumb_size(Some(kind)))
        .into_iter()
        .find(|[width, height]| width * height * 4 == pixels.len())
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
    }
}

fn console_of_code(code: u8) -> Option<ConsoleKind> {
    match code {
        0 => Some(ConsoleKind::Gb),
        1 => Some(ConsoleKind::Gbc),
        2 => Some(ConsoleKind::Gba),
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

#[derive(Default, Clone)]
pub struct SaveStore {
    backend: Backend,
    touched: std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<String>>>,
}

impl SaveStore {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn at(root: std::path::PathBuf) -> Self {
        Self {
            backend: Backend::at(root),
            touched: Default::default(),
        }
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
    pub fn files(&self) -> Vec<(String, String)> {
        let mut files = Vec::new();
        for game in self.games() {
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
        for game in self.games() {
            self.backend.clear(&game)?;
        }
        Ok(())
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

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
