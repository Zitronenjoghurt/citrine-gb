use super::Credited;
use super::sync::LastStored;
use crate::storage::Backend;
use citrine_gb_walker::{EEPROM_SIZE, Eeprom, Memory, Walker};

const MAGIC: &[u8; 4] = b"CWLK";
const VERSION: u8 = 4;
const MEMORY: usize = MAGIC.len() + 1 + Memory::SIZE;
const HEADER: usize = MEMORY + Credited::SIZE;
const V3_HEADER: usize = MEMORY;
const V2_HEADER: usize = MAGIC.len() + 1 + Memory::OLD_SIZE;
const V1_HEADER: usize = MAGIC.len() + 1 + 8;
pub(super) const NAMESPACE: &str = "walker";
const FILE: &str = "walker.bin";
const UNREADABLE: &str = "walker-unreadable.bin";
const LAST_STORED: &str = "cloud-stored.dat";

pub trait WalkerStore {
    fn load(&mut self) -> Option<(Walker, Credited)>;
    fn save(&mut self, bytes: &[u8]);
    fn keep(&mut self, bytes: &[u8]);
    fn files(&self) -> Vec<(String, Vec<u8>)>;
    fn put(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()>;
    fn clear(&mut self) -> std::io::Result<()>;
    fn last_stored(&self) -> Option<LastStored>;
    fn set_last_stored(&mut self, last_stored: Option<LastStored>);
}

pub const MAIN: &str = FILE;

pub fn is_walker_file(name: &str) -> bool {
    name.strip_prefix("walker")
        .and_then(|rest| rest.strip_suffix(".bin"))
        .is_some_and(|rest| rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
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
    if bytes[MAGIC.len()] == VERSION && bytes.len() == HEADER + EEPROM_SIZE {
        let walker = Walker::restore(
            Eeprom::from_bytes(&bytes[HEADER..])?,
            Memory::from_bytes(&bytes[MAGIC.len() + 1..MEMORY])?,
        );
        return Some((walker, Credited::from_bytes(&bytes[MEMORY..HEADER])?));
    }
    decode_old(bytes).map(|walker| (walker, Credited::default()))
}

fn decode_old(bytes: &[u8]) -> Option<Walker> {
    match (bytes[MAGIC.len()], bytes.len()) {
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

pub struct FileStore {
    backend: Backend,
}

impl Default for FileStore {
    fn default() -> Self {
        Self {
            backend: Backend::settings(),
        }
    }
}

impl FileStore {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn at(root: std::path::PathBuf) -> Self {
        Self {
            backend: Backend::at(root),
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

    fn keep(&mut self, bytes: &[u8]) {
        let name = format!("walker-kept-{}.bin", crate::storage::now_unix());
        match self.backend.write(NAMESPACE, &name, bytes) {
            Ok(()) => log::info!(
                "another device took the walker, this copy is kept in {} as {NAMESPACE}/{name}",
                self.backend.location()
            ),
            Err(err) => log::error!("failed to keep the walker: {err}"),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
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
        let names: Vec<String> = std::fs::read_dir(dir.join(NAMESPACE))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(names.iter().any(|name| name == FILE));
        let kept = names
            .iter()
            .find(|name| name.starts_with("walker-kept-"))
            .expect("kept");
        assert_eq!(
            std::fs::read(dir.join(NAMESPACE).join(kept)).unwrap(),
            b"CWLK theirs"
        );
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
            std::fs::read(dir.join(NAMESPACE).join(UNREADABLE)).unwrap(),
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
}
