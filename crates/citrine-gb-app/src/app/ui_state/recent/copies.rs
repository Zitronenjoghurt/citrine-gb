use crate::storage::kinds::Folder;
use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
pub use native::RomCopies;
#[cfg(target_arch = "wasm32")]
pub use web::RomCopies;

pub fn copy_name(key: &str, name: &str) -> String {
    let extension = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| matches!(e.as_str(), "gbc" | "gba" | "nds"))
        .unwrap_or_else(|| "gb".to_owned());
    format!("{key}.{extension}")
}

fn key_of(path: &Path) -> Option<&str> {
    path.file_stem().and_then(|stem| stem.to_str())
}

#[cfg(any(test, target_arch = "wasm32"))]
pub struct CopySizes {
    folders: crate::storage::Folders,
    folder: Folder,
    sizes: std::collections::BTreeMap<String, u64>,
}

#[cfg(any(test, target_arch = "wasm32"))]
impl CopySizes {
    const FILE: &str = "sizes.txt";

    pub fn load(folders: crate::storage::Folders, folder: Folder) -> Self {
        let text = folders
            .read(folder, Self::FILE)
            .and_then(|data| String::from_utf8(data).ok())
            .unwrap_or_default();
        let sizes = text
            .lines()
            .filter_map(|line| {
                let (key, bytes) = line.split_once(' ')?;
                Some((key.to_owned(), bytes.parse().ok()?))
            })
            .collect();
        Self {
            folders,
            folder,
            sizes,
        }
    }

    pub fn has(&self, key: &str) -> bool {
        self.sizes.contains_key(key)
    }

    pub fn record(&mut self, key: &str, bytes: u64) {
        if self.sizes.insert(key.to_owned(), bytes) != Some(bytes) {
            self.save();
        }
    }

    pub fn forget(&mut self, key: &str) {
        if self.sizes.remove(key).is_some() {
            self.save();
        }
    }

    pub fn clear(&mut self) {
        self.sizes.clear();
        let _ = self.folders.delete(self.folder, Self::FILE);
    }

    pub fn missing<'a>(&self, keys: impl IntoIterator<Item = &'a String>) -> Vec<String> {
        keys.into_iter()
            .filter(|key| !self.has(key))
            .cloned()
            .collect()
    }

    pub fn total<'a>(&self, keys: impl IntoIterator<Item = &'a String>) -> Option<u64> {
        keys.into_iter()
            .map(|key| self.sizes.get(key).copied())
            .sum()
    }

    fn save(&self) {
        let text: String = self
            .sizes
            .iter()
            .map(|(key, bytes)| format!("{key} {bytes}\n"))
            .collect();
        if let Err(err) = self.folders.write(self.folder, Self::FILE, text.as_bytes()) {
            log::warn!("couldn't keep the game copy sizes: {err}");
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;

    #[derive(Clone)]
    pub struct RomCopies {
        dir: PathBuf,
    }

    impl Default for RomCopies {
        fn default() -> Self {
            Self::at(crate::storage::folder(Folder::RomCopies))
        }
    }

    impl RomCopies {
        pub fn at(dir: PathBuf) -> Self {
            Self { dir }
        }

        pub fn archive() -> Self {
            Self::at(crate::storage::folder(Folder::GameArchive))
        }

        pub fn path_for(&self, key: &str, name: &str) -> PathBuf {
            self.dir.join(copy_name(key, name))
        }

        pub fn holds(&self, path: &Path) -> Option<bool> {
            Some(path.parent() == Some(self.dir.as_path()) && path.is_file())
        }

        pub fn keep(&self, key: &str, name: &str, data: &[u8]) -> std::io::Result<PathBuf> {
            let path = self.dir.join(copy_name(key, name));
            if path.is_file() {
                return Ok(path);
            }
            std::fs::create_dir_all(&self.dir)?;
            let temp = path.with_extension("tmp");
            std::fs::write(&temp, data)?;
            std::fs::rename(&temp, &path)?;
            Ok(path)
        }

        pub fn is_gone(&self, path: &Path) -> bool {
            !crate::utils::zip_games::on_disk(path)
        }

        pub fn count(&self) -> usize {
            std::fs::read_dir(&self.dir).map_or(0, |entries| {
                entries
                    .flatten()
                    .filter(|entry| {
                        let path = entry.path();
                        path.is_file()
                            && key_of(&path).is_some()
                            && path.extension().is_some_and(|ext| ext != "tmp")
                    })
                    .count()
            })
        }

        pub fn drop_copies<'a>(&self, paths: impl IntoIterator<Item = &'a Path>) {
            for path in paths {
                if path.parent() == Some(self.dir.as_path()) && key_of(path).is_some() {
                    let _ = std::fs::remove_file(path);
                }
            }
        }

        pub fn clear(&self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    use crate::utils::file_loader::PickedFile;
    use crate::utils::idb::{finished, wait};
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::rc::Rc;
    use std::sync::mpsc::Sender;
    use web_sys::IdbTransactionMode;
    use web_sys::wasm_bindgen::{JsCast, JsValue};

    const DB_VERSION: u32 = 1;
    const ROMS: &str = "roms";

    pub struct RomCopies {
        keys: Rc<RefCell<Option<HashSet<String>>>>,
        db: &'static str,
        base: &'static str,
        sizes: Rc<RefCell<CopySizes>>,
    }

    impl Default for RomCopies {
        fn default() -> Self {
            Self::in_db("citrine-roms", "", Folder::RomCopies)
        }
    }

    impl RomCopies {
        pub fn archive() -> Self {
            Self::in_db("citrine-archive", "archive", Folder::GameArchive)
        }

        fn in_db(db: &'static str, base: &'static str, folder: Folder) -> Self {
            let keys = Rc::new(RefCell::new(None));
            let sizes = Rc::new(RefCell::new(CopySizes::load(
                crate::storage::Folders::default(),
                folder,
            )));
            let loading = keys.clone();
            let sizing = sizes.clone();
            citrine_gb_threads::spawn_local(async move {
                let found = match all_keys(db).await {
                    Ok(found) => found,
                    Err(err) => {
                        log::warn!("couldn't list the kept ROMs: {err:?}");
                        return;
                    }
                };
                let missing = sizing.borrow().missing(&found);
                *loading.borrow_mut() = Some(found);
                for key in missing {
                    match get(db, &key).await {
                        Ok(Some(data)) => sizing.borrow_mut().record(&key, data.len() as u64),
                        Ok(None) => {
                            if let Some(keys) = loading.borrow_mut().as_mut() {
                                keys.remove(&key);
                            }
                        }
                        Err(err) => log::warn!("couldn't read the size of a kept ROM: {err:?}"),
                    }
                }
            });
            Self {
                keys,
                db,
                base,
                sizes,
            }
        }

        pub fn path_for(&self, key: &str, name: &str) -> PathBuf {
            Path::new(self.base).join(copy_name(key, name))
        }

        pub fn holds(&self, path: &Path) -> Option<bool> {
            let key = key_of(path)?;
            self.keys.borrow().as_ref().map(|keys| keys.contains(key))
        }

        pub fn keep(&self, key: &str, name: &str, data: &[u8]) -> std::io::Result<PathBuf> {
            let path = self.path_for(key, name);
            let known = self
                .keys
                .borrow()
                .as_ref()
                .is_some_and(|keys| keys.contains(key));
            if !known || !self.sizes.borrow().has(key) {
                self.sizes.borrow_mut().record(key, data.len() as u64);
            }
            if !known {
                if let Some(keys) = self.keys.borrow_mut().as_mut() {
                    keys.insert(key.to_owned());
                }
                let key = key.to_owned();
                let bytes = js_sys::Uint8Array::from(data);
                let db = self.db;
                citrine_gb_threads::spawn_local(async move {
                    if let Err(err) = put(db, &key, bytes.into()).await {
                        log::warn!("couldn't keep a ROM copy: {err:?}");
                    }
                });
            }
            Ok(path)
        }

        pub fn is_gone(&self, path: &Path) -> bool {
            let Some(key) = key_of(path) else {
                return true;
            };
            self.keys
                .borrow()
                .as_ref()
                .is_some_and(|keys| !keys.contains(key))
        }

        pub fn drop_copies<'a>(&self, paths: impl IntoIterator<Item = &'a Path>) {
            for path in paths {
                let Some(key) = key_of(path).map(str::to_owned) else {
                    continue;
                };
                if let Some(keys) = self.keys.borrow_mut().as_mut() {
                    keys.remove(&key);
                }
                self.sizes.borrow_mut().forget(&key);
                let db = self.db;
                citrine_gb_threads::spawn_local(async move {
                    let _ = remove(db, &key).await;
                });
            }
        }

        pub fn count(&self) -> usize {
            self.keys.borrow().as_ref().map_or(0, |keys| keys.len())
        }

        pub fn bytes(&self) -> Option<u64> {
            let keys = self.keys.borrow();
            self.sizes.borrow().total(keys.as_ref()?)
        }

        pub fn clear(&self) {
            if let Some(keys) = self.keys.borrow_mut().as_mut() {
                keys.clear();
            }
            self.sizes.borrow_mut().clear();
            let db = self.db;
            citrine_gb_threads::spawn_local(async move {
                let _ = clear(db).await;
            });
        }

        pub fn read(&self, path: &Path, name: String, tx: Sender<PickedFile>) {
            let Some(key) = key_of(path).map(str::to_owned) else {
                return;
            };
            let path = path.to_path_buf();
            let db = self.db;
            citrine_gb_threads::spawn_local(async move {
                match get(db, &key).await {
                    Ok(Some(data)) => {
                        let seen = crate::rom_ids::Seen::kept(&path, data.len());
                        let _ = tx.send(PickedFile { name, data, seen });
                    }
                    Ok(None) => log::warn!("the kept ROM {key} is gone"),
                    Err(err) => log::warn!("couldn't read a kept ROM: {err:?}"),
                }
            });
        }
    }

    async fn open(db: &str) -> Result<web_sys::IdbDatabase, JsValue> {
        crate::utils::idb::open(db, DB_VERSION, &[ROMS]).await
    }

    async fn all_keys(db: &str) -> Result<HashSet<String>, JsValue> {
        let db = open(db).await?;
        let tx = db.transaction_with_str(ROMS)?;
        let keys = wait(&tx.object_store(ROMS)?.get_all_keys()?).await?;
        Ok(js_sys::Array::from(&keys)
            .iter()
            .filter_map(|key| key.as_string())
            .collect())
    }

    async fn put(db: &str, key: &str, bytes: JsValue) -> Result<(), JsValue> {
        let db = open(db).await?;
        let tx = db.transaction_with_str_and_mode(ROMS, IdbTransactionMode::Readwrite)?;
        tx.object_store(ROMS)?.put_with_key(&bytes, &key.into())?;
        finished(&tx).await
    }

    async fn remove(db: &str, key: &str) -> Result<(), JsValue> {
        let db = open(db).await?;
        let tx = db.transaction_with_str_and_mode(ROMS, IdbTransactionMode::Readwrite)?;
        tx.object_store(ROMS)?.delete(&key.into())?;
        finished(&tx).await
    }

    async fn clear(db: &str) -> Result<(), JsValue> {
        let db = open(db).await?;
        let tx = db.transaction_with_str_and_mode(ROMS, IdbTransactionMode::Readwrite)?;
        tx.object_store(ROMS)?.clear()?;
        finished(&tx).await
    }

    async fn get(db: &str, key: &str) -> Result<Option<std::sync::Arc<[u8]>>, JsValue> {
        let db = open(db).await?;
        let tx = db.transaction_with_str(ROMS)?;
        let value = wait(&tx.object_store(ROMS)?.get(&key.into())?).await?;
        Ok(value
            .dyn_into::<js_sys::Uint8Array>()
            .ok()
            .map(|bytes| crate::utils::file_loader::shared_bytes(&bytes)))
    }
}

#[cfg(all(test, feature = "gba"))]
mod tests {
    use super::copy_name;

    #[test]
    fn a_gba_copy_keeps_its_extension() {
        assert_eq!(copy_name("KEY", "Game.GBA"), "KEY.gba");
        assert_eq!(copy_name("KEY", "Game.gbc"), "KEY.gbc");
        assert_eq!(copy_name("KEY", "Game.gb"), "KEY.gb");
        assert_eq!(copy_name("KEY", "Game.nds"), "KEY.nds");
        assert_eq!(copy_name("KEY", "Game"), "KEY.gb");
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod size_tests {
    use super::CopySizes;
    use crate::storage::Folders;
    use crate::storage::kinds::Folder;

    fn folders(name: &str) -> Folders {
        let root = crate::storage::scratch_dir()
            .join(format!("citrine-copy-sizes-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Folders::at(root)
    }

    #[test]
    fn copies_count_once_every_size_is_known_and_the_sizes_are_kept() {
        let folders = folders("kept");
        let keys = ["AAA".to_owned(), "BBB".to_owned()];
        let mut sizes = CopySizes::load(folders.clone(), Folder::RomCopies);
        sizes.record("AAA", 1 << 20);
        assert_eq!(sizes.total(&keys), None, "BBB has no size yet");
        assert_eq!(sizes.missing(&keys), ["BBB"]);
        sizes.record("BBB", 512);
        assert_eq!(sizes.total(&keys), Some((1 << 20) + 512));

        let mut again = CopySizes::load(folders.clone(), Folder::RomCopies);
        assert_eq!(again.total(&keys), Some((1 << 20) + 512));
        assert!(again.missing(&keys).is_empty());
        again.forget("AAA");
        assert_eq!(
            CopySizes::load(folders.clone(), Folder::RomCopies).total(&keys[1..]),
            Some(512)
        );
        again.clear();
        assert_eq!(
            CopySizes::load(folders, Folder::RomCopies).missing(&keys),
            keys
        );
    }
}
