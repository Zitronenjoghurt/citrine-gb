use super::Credited;
use super::sync::LastStored;
use crate::storage::Folders;
use crate::storage::kinds::Folder;
use citrine_gb_walker::{EEPROM_SIZE, Eeprom, Memory, Walker};
use serde::{Deserialize, Serialize};

const MAGIC: &[u8; 4] = b"CWLK";
const VERSION: u8 = 4;
const MEMORY: usize = MAGIC.len() + 1 + Memory::SIZE;
const HEADER: usize = MEMORY + Credited::SIZE;
const V3_HEADER: usize = MEMORY;
const V2_HEADER: usize = MAGIC.len() + 1 + Memory::OLD_SIZE;
const V1_HEADER: usize = MAGIC.len() + 1 + 8;
pub(super) const NAMESPACE: Folder = Folder::Walker;
const FILE: &str = "walker.bin";
const UNREADABLE: &str = "walker-unreadable.bin";
const LAST_STORED: &str = "cloud-stored.dat";
const TICKED: &str = "walker-clock.dat";
const COPIED: &str = "walker-copy.dat";
const LAN: &str = "walker-lan.dat";
const KEPT: &str = "walker-kept-";
const NOTE: &str = ".why";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeptWhy {
    Replaced { by: String },
    ChangedHere { holder: Option<String> },
    Imported,
    SwappedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptNote {
    pub why: KeptWhy,
    pub steps: u32,
    #[serde(default)]
    pub closed: bool,
}

pub trait WalkerStore {
    fn load(&mut self) -> Option<(Walker, Credited)>;
    fn save(&mut self, bytes: &[u8]);
    fn keep(&mut self, bytes: &[u8]) -> Option<String>;
    fn notes(&self) -> Vec<(String, KeptNote)>;
    fn set_note(&mut self, name: &str, note: &KeptNote);
    fn drop_kept(&mut self, name: &str);
    fn files(&self) -> Vec<(String, Vec<u8>)>;
    fn put(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()>;
    fn clear(&mut self) -> std::io::Result<()>;
    fn last_stored(&self) -> Option<LastStored>;
    fn set_last_stored(&mut self, last_stored: Option<LastStored>);
    fn ticked_at(&self) -> Option<u64>;
    fn set_ticked_at(&mut self, ms: Option<u64>);
    fn copied(&self) -> bool;
    fn set_copied(&mut self, copied: bool);
    fn lan(&self) -> Option<Vec<u8>>;
    fn set_lan(&mut self, bytes: Option<&[u8]>);
}

pub const MAIN: &str = FILE;

pub fn is_walker_file(name: &str) -> bool {
    name.strip_prefix("walker")
        .and_then(|rest| rest.strip_suffix(".bin"))
        .is_some_and(|rest| rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}

fn kept_number(name: &str) -> Option<u64> {
    let rest = name.strip_prefix(KEPT)?;
    let number = rest
        .strip_suffix(".bin")
        .or_else(|| rest.strip_suffix(NOTE))?;
    number.parse().ok()
}

fn note_name(name: &str) -> Option<String> {
    Some(format!("{KEPT}{}{NOTE}", kept_number(name)?))
}

pub fn encode(walker: &Walker, credited: Credited) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER + EEPROM_SIZE);
    bytes.extend_from_slice(MAGIC);
    bytes.push(VERSION);
    bytes.extend_from_slice(&walker.memory().to_bytes());
    bytes.extend_from_slice(&credited.to_bytes());
    bytes.extend_from_slice(walker.eeprom().bytes());
    bytes
}

pub fn decode(bytes: &[u8]) -> Option<(Walker, Credited)> {
    if bytes.get(..MAGIC.len())? != MAGIC {
        return None;
    }
    let version = *bytes.get(MAGIC.len())?;
    if version == VERSION && bytes.len() == HEADER + EEPROM_SIZE {
        let walker = Walker::restore(
            Eeprom::from_bytes(&bytes[HEADER..])?,
            Memory::from_bytes(&bytes[MAGIC.len() + 1..MEMORY])?,
        );
        return Some((walker, Credited::from_bytes(&bytes[MEMORY..HEADER])?));
    }
    decode_old(version, bytes).map(|walker| (walker, Credited::default()))
}

fn decode_old(version: u8, bytes: &[u8]) -> Option<Walker> {
    match (version, bytes.len()) {
        (3, len) if len == V3_HEADER + EEPROM_SIZE => Some(Walker::restore(
            Eeprom::from_bytes(&bytes[V3_HEADER..])?,
            Memory::from_bytes(&bytes[MAGIC.len() + 1..V3_HEADER])?,
        )),
        (2, len) if len == V2_HEADER + EEPROM_SIZE => Some(Walker::restore(
            Eeprom::from_bytes(&bytes[V2_HEADER..])?,
            Memory::from_bytes(&bytes[MAGIC.len() + 1..V2_HEADER])?,
        )),
        (1, len) if len == V1_HEADER + EEPROM_SIZE => Some(Walker::from_eeprom(
            Eeprom::from_bytes(&bytes[V1_HEADER..])?,
            u64::from_le_bytes(bytes[MAGIC.len() + 1..V1_HEADER].try_into().ok()?),
        )),
        _ => None,
    }
}

#[derive(Default)]
pub struct FileStore {
    backend: Folders,
}

impl FileStore {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn at(root: std::path::PathBuf) -> Self {
        Self {
            backend: Folders::at(root),
        }
    }
}

impl WalkerStore for FileStore {
    fn load(&mut self) -> Option<(Walker, Credited)> {
        let bytes = self.backend.read(NAMESPACE, FILE)?;
        let walker = decode(&bytes);
        if walker.is_none() {
            log::error!("The walker file is unreadable, kept as {UNREADABLE}");
            if let Err(err) = self.backend.write(NAMESPACE, UNREADABLE, &bytes) {
                log::error!("failed to keep the unreadable walker: {err}");
            }
        }
        walker
    }

    fn save(&mut self, bytes: &[u8]) {
        if let Err(err) = self.backend.write(NAMESPACE, FILE, bytes) {
            log::error!("failed to save the walker: {err}");
        }
    }

    fn keep(&mut self, bytes: &[u8]) -> Option<String> {
        let newest = self
            .backend
            .list(NAMESPACE)
            .iter()
            .filter_map(|name| kept_number(name))
            .max();
        let number = newest.map_or(0, |n| n + 1).max(crate::storage::now_unix());
        let name = format!("{KEPT}{number}.bin");
        match self.backend.write(NAMESPACE, &name, bytes) {
            Ok(()) => {
                log::info!(
                    "a walker copy is kept in {} as {}/{name}",
                    self.backend.location(),
                    NAMESPACE.name()
                );
                Some(name)
            }
            Err(err) => {
                log::error!("failed to keep the walker: {err}");
                None
            }
        }
    }

    fn notes(&self) -> Vec<(String, KeptNote)> {
        let mut notes: Vec<(u64, String, KeptNote)> = self
            .backend
            .list(NAMESPACE)
            .into_iter()
            .filter(|name| name.ends_with(NOTE))
            .filter_map(|name| {
                let number = kept_number(&name)?;
                let note = serde_json::from_slice(&self.backend.read(NAMESPACE, &name)?).ok()?;
                Some((number, format!("{KEPT}{number}.bin"), note))
            })
            .collect();
        notes.sort_by_key(|(number, _, _)| *number);
        notes
            .into_iter()
            .map(|(_, name, note)| (name, note))
            .collect()
    }

    fn set_note(&mut self, name: &str, note: &KeptNote) {
        let Some(file) = note_name(name) else {
            return;
        };
        let bytes = serde_json::to_vec(note).unwrap_or_default();
        if let Err(err) = self.backend.write(NAMESPACE, &file, &bytes) {
            log::error!("failed to note why the walker copy was kept: {err}");
        }
    }

    fn drop_kept(&mut self, name: &str) {
        let note = note_name(name);
        for file in [Some(name.to_owned()), note].into_iter().flatten() {
            if let Err(err) = self.backend.delete(NAMESPACE, &file) {
                log::error!("failed to remove the walker copy {file}: {err}");
            }
        }
    }

    fn files(&self) -> Vec<(String, Vec<u8>)> {
        let mut names = self.backend.list(NAMESPACE);
        names.retain(|name| is_walker_file(name));
        names.sort();
        names
            .into_iter()
            .filter_map(|name| Some((name.clone(), self.backend.read(NAMESPACE, &name)?)))
            .collect()
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.backend.write(NAMESPACE, name, bytes)
    }

    fn clear(&mut self) -> std::io::Result<()> {
        self.backend.clear(NAMESPACE)
    }

    fn last_stored(&self) -> Option<LastStored> {
        LastStored::from_bytes(&self.backend.read(NAMESPACE, LAST_STORED)?)
    }

    fn set_last_stored(&mut self, last_stored: Option<LastStored>) {
        let done = match last_stored {
            Some(last_stored) => {
                self.backend
                    .write(NAMESPACE, LAST_STORED, &last_stored.to_bytes())
            }
            None => self.backend.delete(NAMESPACE, LAST_STORED),
        };
        if let Err(err) = done {
            log::error!("failed to note the walker's cloud copy: {err}");
        }
    }

    fn ticked_at(&self) -> Option<u64> {
        let bytes = self.backend.read(NAMESPACE, TICKED)?;
        Some(u64::from_le_bytes(bytes.as_slice().try_into().ok()?))
    }

    fn set_ticked_at(&mut self, ms: Option<u64>) {
        let done = match ms {
            Some(ms) => self.backend.write(NAMESPACE, TICKED, &ms.to_le_bytes()),
            None => self.backend.delete(NAMESPACE, TICKED),
        };
        if let Err(err) = done {
            log::error!("failed to note the walker's clock: {err}");
        }
    }

    fn copied(&self) -> bool {
        self.backend.read(NAMESPACE, COPIED).is_some()
    }

    fn set_copied(&mut self, copied: bool) {
        let done = if copied {
            self.backend.write(NAMESPACE, COPIED, &[])
        } else {
            self.backend.delete(NAMESPACE, COPIED)
        };
        if let Err(err) = done {
            log::error!("failed to note where the walker came from: {err}");
        }
    }

    fn lan(&self) -> Option<Vec<u8>> {
        self.backend.read(NAMESPACE, LAN)
    }

    fn set_lan(&mut self, bytes: Option<&[u8]>) {
        let done = match bytes {
            Some(bytes) => self.backend.write(NAMESPACE, LAN, bytes),
            None => self.backend.delete(NAMESPACE, LAN),
        };
        if let Err(err) = done {
            log::error!("failed to note who keeps the walker nearby: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> std::path::PathBuf {
        let root = crate::storage::scratch_dir().join(format!(
            "citrine-walker-store-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn a_saved_walker_loads_with_its_eeprom_rng_and_credit() {
        let mut walker = Walker::new(42);
        walker.steps(1_234);
        let credited = Credited {
            counter: super::super::Counter::HealthConnect,
            until: Some(1_790_000_000),
        };
        let dir = root("round");
        FileStore::at(dir.clone()).save(&encode(&walker, credited));
        let (loaded, record) = FileStore::at(dir).load().expect("loads");
        assert_eq!(loaded, walker);
        assert_eq!(loaded.memory(), walker.memory());
        assert_eq!(record, credited);
    }

    #[test]
    fn the_cloud_record_stays_on_the_device_and_goes_with_forget() {
        let dir = root("record");
        let mut store = FileStore::at(dir.clone());
        store.save(&encode(&Walker::new(42), Credited::default()));
        let record = LastStored {
            version: 7,
            hash: [9; 32],
        };
        store.set_last_stored(Some(record));
        assert_eq!(FileStore::at(dir.clone()).last_stored(), Some(record));
        assert_eq!(
            store
                .files()
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            [MAIN]
        );
        store.set_last_stored(None);
        assert_eq!(store.last_stored(), None);
        store.set_last_stored(Some(record));
        store.clear().unwrap();
        assert_eq!(store.last_stored(), None);
    }

    #[test]
    fn a_version_three_file_loads_unchanged_with_nothing_credited() {
        let mut walker = Walker::new(5);
        walker.steps(777);
        let mut bytes = MAGIC.to_vec();
        bytes.push(3);
        bytes.extend_from_slice(&walker.memory().to_bytes());
        bytes.extend_from_slice(walker.eeprom().bytes());
        let (loaded, credited) = decode(&bytes).expect("loads");
        assert_eq!(loaded, walker);
        assert_eq!(loaded.memory(), walker.memory());
        assert_eq!(credited, Credited::default());
    }

    #[test]
    fn a_kept_copy_sits_beside_the_walker() {
        let dir = root("kept");
        let mut store = FileStore::at(dir.clone());
        store.save(b"CWLK mine");
        store.keep(b"CWLK theirs");
        let names: Vec<String> = std::fs::read_dir(dir.join(NAMESPACE.name()))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(names.iter().any(|name| name == FILE));
        let kept = names
            .iter()
            .find(|name| name.starts_with("walker-kept-"))
            .expect("kept");
        assert_eq!(
            std::fs::read(dir.join(NAMESPACE.name()).join(kept)).unwrap(),
            b"CWLK theirs"
        );
    }

    #[test]
    fn a_kept_note_survives_a_restart_and_goes_with_its_copy() {
        let dir = root("note");
        let mut store = FileStore::at(dir.clone());
        let first = store.keep(b"CWLK first").unwrap();
        let second = store.keep(b"CWLK second").unwrap();
        assert_ne!(first, second);
        let note = KeptNote {
            why: KeptWhy::Replaced {
                by: "Pixel 8".into(),
            },
            steps: 1_032,
            closed: false,
        };
        store.set_note(&second, &note);
        let mut again = FileStore::at(dir);
        assert_eq!(again.notes(), vec![(second.clone(), note)]);
        assert_eq!(again.files().len(), 2);
        again.drop_kept(&second);
        assert!(again.notes().is_empty());
        assert_eq!(again.files(), vec![(first, b"CWLK first".to_vec())]);
    }

    #[test]
    fn a_version_one_file_loads_with_its_rng() {
        let walker = Walker::new(7);
        let mut bytes = MAGIC.to_vec();
        bytes.push(1);
        bytes.extend_from_slice(&7u64.to_le_bytes());
        bytes.extend_from_slice(walker.eeprom().bytes());
        let (loaded, credited) = decode(&bytes).expect("loads");
        assert_eq!(credited, Credited::default());
        assert_eq!(loaded.rng_state(), 7);
        assert_eq!(loaded.eeprom(), walker.eeprom());
    }

    #[test]
    fn a_version_two_file_loads_its_memory_and_boots_the_rest() {
        let walker = Walker::new(9);
        let mut bytes = MAGIC.to_vec();
        bytes.push(2);
        let mut memory = [0u8; Memory::OLD_SIZE];
        memory[..8].copy_from_slice(&9u64.to_le_bytes());
        memory[8..12].copy_from_slice(&321u32.to_le_bytes());
        bytes.extend_from_slice(&memory);
        bytes.extend_from_slice(walker.eeprom().bytes());
        let (loaded, _) = decode(&bytes).expect("loads");
        assert_eq!(loaded.rng_state(), 9);
        assert_eq!(loaded.walk_steps(), 321);
        assert_eq!(loaded.eeprom(), walker.eeprom());
        assert_eq!(loaded.health(), walker.health());
    }

    #[test]
    fn nothing_saved_loads_nothing() {
        assert!(FileStore::at(root("empty")).load().is_none());
    }

    #[test]
    fn an_unreadable_file_is_kept_aside_and_loads_nothing() {
        let dir = root("bad");
        let mut store = FileStore::at(dir.clone());
        store.backend.write(NAMESPACE, FILE, b"CWLK junk").unwrap();
        assert!(store.load().is_none());
        assert_eq!(
            std::fs::read(dir.join(NAMESPACE.name()).join(UNREADABLE)).unwrap(),
            b"CWLK junk"
        );
    }

    #[test]
    fn decode_refuses_another_version_or_size() {
        let mut bytes = encode(&Walker::new(1), Credited::default());
        assert!(decode(&bytes[..bytes.len() - 1]).is_none());
        bytes[4] = VERSION + 1;
        assert!(decode(&bytes).is_none());
    }

    #[test]
    fn a_short_file_decodes_to_nothing() {
        let bytes = encode(&Walker::new(1), Credited::default());
        for len in 0..=8 {
            assert!(decode(&bytes[..len]).is_none(), "{len} bytes");
        }
        let dir = root("short");
        let mut store = FileStore::at(dir);
        store.backend.write(NAMESPACE, FILE, MAGIC).unwrap();
        assert!(store.load().is_none());
    }
}
