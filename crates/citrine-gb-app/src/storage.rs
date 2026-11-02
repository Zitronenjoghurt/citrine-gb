mod autosaves;
mod backend;
mod battery;
pub mod cloud;
mod names;
mod snapshots;

pub(crate) use backend::Backend;
#[cfg(not(target_arch = "wasm32"))]
pub use backend::data_dir;
use citrine_gb::gb::GbModel;

pub const THUMB_WIDTH: usize = 80;
pub const THUMB_HEIGHT: usize = 72;

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
    pub console: Option<GbModel>,
}

impl SnapshotInfo {
    pub fn display_name(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => default_slot_name(self.slot),
        }
    }
}

fn encode_meta(saved_at: u64, model: GbModel) -> Vec<u8> {
    let mut meta = saved_at.to_le_bytes().to_vec();
    meta.push(model as u8);
    meta
}

fn decode_meta(meta: &[u8]) -> (Option<u64>, Option<GbModel>) {
    let saved_at = meta
        .get(..8)
        .and_then(|b| b.try_into().ok())
        .map(u64::from_le_bytes);
    let console = match meta.get(8) {
        Some(0) => Some(GbModel::Dmg),
        Some(1) => Some(GbModel::Cgb),
        _ => None,
    };
    (saved_at, console)
}

fn state_console(state: &[u8]) -> Option<GbModel> {
    citrine_gb::persistence::full_dump::FullDump::from_brotli(state)
        .ok()
        .map(|dump| dump.gb.model)
}

pub fn default_slot_name(slot: usize) -> String {
    format!("Slot {}", slot + 1)
}

const SETTINGS_NAMESPACE: &str = "settings";
const SETTINGS: &str = "settings.json";

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

fn downscale(frame: &[u8]) -> Vec<u8> {
    const SRC_WIDTH: usize = THUMB_WIDTH * 2;
    let mut out = vec![0u8; THUMB_WIDTH * THUMB_HEIGHT * 4];
    if frame.len() < SRC_WIDTH * THUMB_HEIGHT * 2 * 4 {
        return out;
    }
    for y in 0..THUMB_HEIGHT {
        for x in 0..THUMB_WIDTH {
            for channel in 0..4 {
                let at = |dy: usize, dx: usize| -> u32 {
                    frame[(((y * 2 + dy) * SRC_WIDTH) + x * 2 + dx) * 4 + channel] as u32
                };
                let sum = at(0, 0) + at(0, 1) + at(1, 0) + at(1, 1);
                out[(y * THUMB_WIDTH + x) * 4 + channel] = (sum / 4) as u8;
            }
        }
    }
    out
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
