use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
pub use native::RomCopies;
#[cfg(target_arch = "wasm32")]
pub use web::RomCopies;

pub fn copy_name(key: &str, name: &str) -> String {
    let extension = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| e.eq_ignore_ascii_case("gbc"))
        .map_or("gb", |_| "gbc");
    format!("{key}.{extension}")
}

fn key_of(path: &Path) -> Option<&str> {
    path.file_stem().and_then(|stem| stem.to_str())
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;

    pub struct RomCopies {
        dir: PathBuf,
    }

    impl Default for RomCopies {
        fn default() -> Self {
            #[cfg(test)]
            let root = std::env::temp_dir().join(format!("citrine-test-{}", std::process::id()));
            #[cfg(not(test))]
            let root = crate::storage::data_dir();
            Self::at(root.join("roms"))
        }
    }

    impl RomCopies {
        pub fn at(dir: PathBuf) -> Self {
            Self { dir }
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
            !path.is_file()
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

    const DB_NAME: &str = "citrine-roms";
    const DB_VERSION: u32 = 1;
    const ROMS: &str = "roms";

    pub struct RomCopies {
        keys: Rc<RefCell<Option<HashSet<String>>>>,
    }

    impl Default for RomCopies {
        fn default() -> Self {
            let keys = Rc::new(RefCell::new(None));
            let loading = keys.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match all_keys().await {
                    Ok(found) => *loading.borrow_mut() = Some(found),
                    Err(err) => log::warn!("couldn't list the kept ROMs: {err:?}"),
                }
            });
            Self { keys }
        }
    }

    impl RomCopies {
        pub fn keep(&self, key: &str, name: &str, data: &[u8]) -> std::io::Result<PathBuf> {
            let path = PathBuf::from(copy_name(key, name));
            let known = self
                .keys
                .borrow()
                .as_ref()
                .is_some_and(|keys| keys.contains(key));
            if !known {
                if let Some(keys) = self.keys.borrow_mut().as_mut() {
                    keys.insert(key.to_owned());
                }
                let key = key.to_owned();
                let bytes = js_sys::Uint8Array::from(data);
                wasm_bindgen_futures::spawn_local(async move {
                    if let Err(err) = put(&key, bytes.into()).await {
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
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = remove(&key).await;
                });
            }
        }

        pub fn clear(&self) {
            if let Some(keys) = self.keys.borrow_mut().as_mut() {
                keys.clear();
            }
            wasm_bindgen_futures::spawn_local(async move {
                let _ = clear().await;
            });
        }

        pub fn read(&self, path: &Path, name: String, tx: Sender<PickedFile>) {
            let Some(key) = key_of(path).map(str::to_owned) else {
                return;
            };
            wasm_bindgen_futures::spawn_local(async move {
                match get(&key).await {
                    Ok(Some(data)) => {
                        let _ = tx.send(PickedFile { name, data });
                    }
                    Ok(None) => log::warn!("the kept ROM {key} is gone"),
                    Err(err) => log::warn!("couldn't read a kept ROM: {err:?}"),
                }
            });
        }
    }

    async fn open() -> Result<web_sys::IdbDatabase, JsValue> {
        crate::utils::idb::open(DB_NAME, DB_VERSION, &[ROMS]).await
    }

    async fn all_keys() -> Result<HashSet<String>, JsValue> {
        let db = open().await?;
        let tx = db.transaction_with_str(ROMS)?;
        let keys = wait(&tx.object_store(ROMS)?.get_all_keys()?).await?;
        Ok(js_sys::Array::from(&keys)
            .iter()
            .filter_map(|key| key.as_string())
            .collect())
    }

    async fn put(key: &str, bytes: JsValue) -> Result<(), JsValue> {
        let db = open().await?;
        let tx = db.transaction_with_str_and_mode(ROMS, IdbTransactionMode::Readwrite)?;
        tx.object_store(ROMS)?.put_with_key(&bytes, &key.into())?;
        finished(&tx).await
    }

    async fn remove(key: &str) -> Result<(), JsValue> {
        let db = open().await?;
        let tx = db.transaction_with_str_and_mode(ROMS, IdbTransactionMode::Readwrite)?;
        tx.object_store(ROMS)?.delete(&key.into())?;
        finished(&tx).await
    }

    async fn clear() -> Result<(), JsValue> {
        let db = open().await?;
        let tx = db.transaction_with_str_and_mode(ROMS, IdbTransactionMode::Readwrite)?;
        tx.object_store(ROMS)?.clear()?;
        finished(&tx).await
    }

    async fn get(key: &str) -> Result<Option<Vec<u8>>, JsValue> {
        let db = open().await?;
        let tx = db.transaction_with_str(ROMS)?;
        let value = wait(&tx.object_store(ROMS)?.get(&key.into())?).await?;
        Ok(value
            .dyn_into::<js_sys::Uint8Array>()
            .ok()
            .map(|bytes| bytes.to_vec()))
    }
}
