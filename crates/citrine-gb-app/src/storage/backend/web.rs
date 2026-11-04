use super::super::mirror::{self, Batch, Mirror, Op};
use crate::utils::idb::{finished, wait};
use base64::Engine;
use std::cell::{Cell, RefCell};
use web_sys::wasm_bindgen::{JsCast, JsValue};

const DB_NAME: &str = "citrine-saves";
const DB_VERSION: u32 = 1;
const FILES: &str = "files";
const CRASH_KEY: &str = "citrine-crash";

thread_local! {
    static SAVES: RefCell<Mirror> = RefCell::default();
    static OPENED: Cell<bool> = const { Cell::new(false) };
    static WITHOUT_DATABASE: Cell<bool> = const { Cell::new(false) };
}

#[derive(Default, Clone)]
pub struct Backend {
    settings: bool,
}

pub async fn open_saves() {
    SAVES.with_borrow_mut(|saves| saves.adopt_local(local_entries()));
    match load().await {
        Ok(stored) => SAVES.with_borrow_mut(|saves| saves.fill(stored)),
        Err(err) => {
            log::error!("couldn't open the saves database, saves stay in local storage: {err:?}");
            WITHOUT_DATABASE.set(true);
            SAVES.with_borrow_mut(Mirror::take_pending);
        }
    }
    take_crash_save();
    OPENED.set(true);
    flush();
}

fn take_crash_save() {
    let Some(storage) = storage() else {
        return;
    };
    let Some(raw) = storage.get_item(CRASH_KEY).ok().flatten() else {
        return;
    };
    super::super::recover_crash_save(&raw);
    if WITHOUT_DATABASE.get() {
        let _ = storage.remove_item(CRASH_KEY);
    } else {
        SAVES.with_borrow_mut(|saves| saves.remove_after_commit(CRASH_KEY.to_string()));
    }
}

pub fn keep_crash_save(raw: &str) {
    if WITHOUT_DATABASE.get() {
        return;
    }
    let Some(storage) = storage() else {
        return;
    };
    if storage.set_item(CRASH_KEY, raw).is_err() {
        log::error!("couldn't keep the autosave in local storage (quota?)");
        return;
    }
    SAVES.with_borrow_mut(|saves| saves.remove_after_commit(CRASH_KEY.to_string()));
    flush();
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn local_entries() -> Vec<(String, String)> {
    let Some(storage) = storage() else {
        return Vec::new();
    };
    let count = storage.length().unwrap_or(0);
    (0..count)
        .filter_map(|i| storage.key(i).ok().flatten())
        .filter_map(|key| {
            let value = storage.get_item(&key).ok()??;
            Some((key, value))
        })
        .collect()
}

fn local_read(namespace: &str, name: &str) -> Option<Vec<u8>> {
    let raw = storage()?
        .get_item(&mirror::local_key(namespace, name))
        .ok()??;
    base64::engine::general_purpose::STANDARD.decode(raw).ok()
}

fn local_write(namespace: &str, name: &str, data: &[u8]) -> std::io::Result<()> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(data);
    storage()
        .ok_or_else(|| std::io::Error::other("local storage unavailable"))?
        .set_item(&mirror::local_key(namespace, name), &encoded)
        .map_err(|_| std::io::Error::other("local storage write failed (quota?)"))
}

fn local_delete(namespace: &str, name: &str) {
    if let Some(storage) = storage() {
        let _ = storage.remove_item(&mirror::local_key(namespace, name));
    }
}

fn flush() {
    if !OPENED.get() || WITHOUT_DATABASE.get() {
        return;
    }
    let batch = SAVES.with_borrow_mut(Mirror::take_pending);
    if batch.is_empty() {
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        match store(&batch).await {
            Ok(()) => {
                if let Some(storage) = storage() {
                    for key in &batch.moved {
                        let _ = storage.remove_item(key);
                    }
                }
            }
            Err(err) => {
                log::error!("couldn't write saves to the database, trying again later: {err:?}");
                SAVES.with_borrow_mut(|saves| saves.retry(batch));
            }
        }
    });
}

async fn database() -> Result<web_sys::IdbDatabase, JsValue> {
    crate::utils::idb::open(DB_NAME, DB_VERSION, &[FILES]).await
}

async fn load() -> Result<Vec<(String, Vec<u8>)>, JsValue> {
    let db = database().await?;
    let tx = db.transaction_with_str(FILES)?;
    let store = tx.object_store(FILES)?;
    let keys: js_sys::Array = wait(&store.get_all_keys()?).await?.unchecked_into();
    let values: js_sys::Array = wait(&store.get_all()?).await?.unchecked_into();
    Ok(keys
        .iter()
        .zip(values.iter())
        .filter_map(|(key, value)| {
            let key = key.as_string()?;
            let data = value.dyn_into::<js_sys::Uint8Array>().ok()?.to_vec();
            Some((key, data))
        })
        .collect())
}

async fn store(batch: &Batch) -> Result<(), JsValue> {
    let db = database().await?;
    let tx = db.transaction_with_str_and_mode(FILES, web_sys::IdbTransactionMode::Readwrite)?;
    let store = tx.object_store(FILES)?;
    for op in &batch.ops {
        match op {
            Op::Put(key, data) => {
                store.put_with_key(&js_sys::Uint8Array::from(data.as_slice()), &key.into())?;
            }
            Op::Delete(key) => {
                store.delete(&key.into())?;
            }
        }
    }
    finished(&tx).await
}

impl Backend {
    pub fn settings() -> Self {
        Self { settings: true }
    }

    fn local(&self) -> bool {
        self.settings || WITHOUT_DATABASE.get()
    }

    pub fn read(&self, namespace: &str, name: &str) -> Option<Vec<u8>> {
        if self.local() {
            return local_read(namespace, name);
        }
        SAVES.with_borrow(|saves| {
            saves
                .read(&mirror::key(namespace, name))
                .map(<[u8]>::to_vec)
        })
    }

    pub fn write(&self, namespace: &str, name: &str, data: &[u8]) -> std::io::Result<()> {
        if self.local() {
            return local_write(namespace, name, data);
        }
        SAVES.with_borrow_mut(|saves| saves.write(mirror::key(namespace, name), data));
        flush();
        Ok(())
    }

    pub fn delete(&self, namespace: &str, name: &str) -> std::io::Result<()> {
        if self.local() {
            local_delete(namespace, name);
            return Ok(());
        }
        SAVES.with_borrow_mut(|saves| saves.delete(mirror::key(namespace, name)));
        flush();
        Ok(())
    }

    pub fn exists(&self, namespace: &str, name: &str) -> bool {
        if self.local() {
            return local_read(namespace, name).is_some();
        }
        SAVES.with_borrow(|saves| saves.read(&mirror::key(namespace, name)).is_some())
    }

    pub fn size(&self, namespace: &str, name: &str) -> u64 {
        if self.local() {
            return local_read(namespace, name).map_or(0, |data| data.len() as u64);
        }
        SAVES.with_borrow(|saves| {
            saves
                .read(&mirror::key(namespace, name))
                .map_or(0, |data| data.len() as u64)
        })
    }

    pub fn clear(&self, namespace: &str) -> std::io::Result<()> {
        for name in self.list(namespace) {
            self.delete(namespace, &name)?;
        }
        Ok(())
    }

    pub fn list(&self, namespace: &str) -> Vec<String> {
        if self.local() {
            let prefix = mirror::local_key(namespace, "");
            return local_entries()
                .into_iter()
                .filter_map(|(key, _)| key.strip_prefix(&prefix).map(str::to_owned))
                .collect();
        }
        SAVES.with_borrow(|saves| saves.list(namespace))
    }

    pub fn namespaces(&self) -> Vec<String> {
        if self.local() {
            let mut found: Vec<String> = local_entries()
                .into_iter()
                .filter_map(|(key, _)| {
                    let rest = key.strip_prefix(mirror::LOCAL_PREFIX)?;
                    rest.split_once('/').map(|(ns, _)| ns.to_owned())
                })
                .collect();
            found.sort();
            found.dedup();
            return found;
        }
        SAVES.with_borrow(Mirror::namespaces)
    }

    pub fn location(&self) -> String {
        if self.local() {
            "browser local storage".to_string()
        } else {
            "browser storage".to_string()
        }
    }
}
