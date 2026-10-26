use super::{
    MediaId, MediaInfo, MediaItem, NewMedia, decode_png, encode_thumbnail, sort_newest_first,
    unique_file_name,
};
use std::io;
use std::sync::mpsc::{Receiver, Sender, channel};
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::{JsCast, JsValue};
use web_sys::{IdbDatabase, IdbRequest, IdbTransaction, IdbTransactionMode};

const DB_NAME: &str = "citrine-media";
const DB_VERSION: u32 = 1;
const ITEMS: &str = "items";
const FILES: &str = "files";

pub struct MediaStore {
    items: Vec<MediaItem>,
    loaded_rx: Receiver<Vec<MediaItem>>,
    loaded: bool,
}

impl Default for MediaStore {
    fn default() -> Self {
        let (tx, loaded_rx) = channel();
        wasm_bindgen_futures::spawn_local(load_all(tx));
        Self {
            items: Vec::new(),
            loaded_rx,
            loaded: false,
        }
    }
}

impl MediaStore {
    pub fn items(&self) -> &[MediaItem] {
        &self.items
    }

    pub fn refresh(&mut self) {
        if self.loaded {
            return;
        }
        if let Ok(stored) = self.loaded_rx.try_recv() {
            self.loaded = true;
            for item in stored {
                if !self.items.iter().any(|i| i.id == item.id) {
                    self.items.push(item);
                }
            }
            sort_newest_first(&mut self.items);
        }
    }

    pub fn add_screenshot(&mut self, new: NewMedia, png: &[u8]) -> io::Result<MediaItem> {
        self.add_bytes(new, png.to_vec())
    }

    pub fn add_recording_blob(
        &mut self,
        blob: web_sys::Blob,
        new: NewMedia,
    ) -> io::Result<MediaItem> {
        let bytes = blob.size() as u64;
        self.add_value(new, blob.into(), bytes)
    }

    fn add_bytes(&mut self, new: NewMedia, bytes: Vec<u8>) -> io::Result<MediaItem> {
        let size = bytes.len() as u64;
        let file = js_sys::Uint8Array::from(bytes.as_slice());
        self.add_value(new, file.into(), size)
    }

    fn add_value(&mut self, new: NewMedia, file: JsValue, bytes: u64) -> io::Result<MediaItem> {
        let kind = new.kind;
        let stem = super::file_stem(&new.game, new.created_at);
        let file_name = unique_file_name(&stem, kind.extension(), |name| {
            let id = MediaId::new(kind, name);
            self.items.iter().any(|i| i.id == id)
        });
        let id = MediaId::new(kind, &file_name);
        let info = new.info(bytes);
        let thumb_png = encode_thumbnail(&new.frame).unwrap_or_default();
        let record = record(&info, &thumb_png).ok_or_else(|| io::Error::other("bad record"))?;
        let item = MediaItem {
            id: id.clone(),
            info,
            thumbnail: decode_png(&thumb_png),
        };
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(err) = put(id.as_str(), record, file).await {
                log::error!("Could not store {}: {err:?}", id.as_str());
            }
        });
        self.items.push(item.clone());
        sort_newest_first(&mut self.items);
        Ok(item)
    }

    pub fn delete(&mut self, id: &MediaId) -> io::Result<()> {
        self.items.retain(|i| &i.id != id);
        let key = id.as_str().to_string();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(err) = remove(&key).await {
                log::error!("Could not delete {key}: {err:?}");
            }
        });
        Ok(())
    }

    pub fn read(&self, id: &MediaId, done: impl FnOnce(Option<web_sys::Blob>) + 'static) {
        let key = id.as_str().to_string();
        wasm_bindgen_futures::spawn_local(async move {
            done(read_file(&key).await.ok().flatten());
        });
    }
}

fn record(info: &MediaInfo, thumb_png: &[u8]) -> Option<JsValue> {
    let object = js_sys::Object::new();
    let info = serde_json::to_string(info).ok()?;
    js_sys::Reflect::set(&object, &"info".into(), &info.into()).ok()?;
    let thumb = js_sys::Uint8Array::from(thumb_png);
    js_sys::Reflect::set(&object, &"thumb".into(), &thumb).ok()?;
    Some(object.into())
}

fn item_from_record(key: &JsValue, value: &JsValue) -> Option<MediaItem> {
    let key = key.as_string()?;
    let info = js_sys::Reflect::get(value, &"info".into())
        .ok()?
        .as_string()?;
    let info: MediaInfo = serde_json::from_str(&info).ok()?;
    let kind = MediaId(key.clone()).kind()?;
    let thumb = js_sys::Reflect::get(value, &"thumb".into())
        .ok()
        .and_then(|t| t.dyn_into::<js_sys::Uint8Array>().ok())
        .map(|t| t.to_vec());
    Some(MediaItem {
        id: MediaId(key),
        info: MediaInfo { kind, ..info },
        thumbnail: thumb.as_deref().and_then(decode_png),
    })
}

async fn load_all(tx: Sender<Vec<MediaItem>>) {
    let items = match list().await {
        Ok(items) => items,
        Err(err) => {
            log::error!("Could not read the media library: {err:?}");
            Vec::new()
        }
    };
    let _ = tx.send(items);
}

async fn list() -> Result<Vec<MediaItem>, JsValue> {
    let db = open().await?;
    let tx = db.transaction_with_str(ITEMS)?;
    let store = tx.object_store(ITEMS)?;
    let keys = wait(&store.get_all_keys()?).await?;
    let values = wait(&store.get_all()?).await?;
    let keys = js_sys::Array::from(&keys);
    let values = js_sys::Array::from(&values);
    Ok((0..keys.length().min(values.length()))
        .filter_map(|i| item_from_record(&keys.get(i), &values.get(i)))
        .collect())
}

async fn put(key: &str, record: JsValue, file: JsValue) -> Result<(), JsValue> {
    let db = open().await?;
    let tx = readwrite(&db)?;
    tx.object_store(FILES)?.put_with_key(&file, &key.into())?;
    tx.object_store(ITEMS)?.put_with_key(&record, &key.into())?;
    finished(&tx).await
}

async fn remove(key: &str) -> Result<(), JsValue> {
    let db = open().await?;
    let tx = readwrite(&db)?;
    tx.object_store(FILES)?.delete(&key.into())?;
    tx.object_store(ITEMS)?.delete(&key.into())?;
    finished(&tx).await
}

async fn read_file(key: &str) -> Result<Option<web_sys::Blob>, JsValue> {
    let db = open().await?;
    let tx = db.transaction_with_str(FILES)?;
    let value = wait(&tx.object_store(FILES)?.get(&key.into())?).await?;
    if value.is_instance_of::<web_sys::Blob>() {
        return Ok(Some(value.unchecked_into()));
    }
    let Ok(bytes) = value.dyn_into::<js_sys::Uint8Array>() else {
        return Ok(None);
    };
    let parts = js_sys::Array::of1(&bytes);
    Ok(web_sys::Blob::new_with_u8_array_sequence(&parts).ok())
}

fn readwrite(db: &IdbDatabase) -> Result<IdbTransaction, JsValue> {
    let names = js_sys::Array::of2(&ITEMS.into(), &FILES.into());
    db.transaction_with_str_sequence_and_mode(&names, IdbTransactionMode::Readwrite)
}

async fn open() -> Result<IdbDatabase, JsValue> {
    let factory = web_sys::window()
        .ok_or("no window")?
        .indexed_db()?
        .ok_or("no IndexedDB")?;
    let request = factory.open_with_u32(DB_NAME, DB_VERSION)?;
    let target = request.clone();
    let upgrade = Closure::once_into_js(move || {
        if let Ok(db) = target.result() {
            let db: IdbDatabase = db.unchecked_into();
            let names = db.object_store_names();
            for name in [ITEMS, FILES] {
                if !names.contains(name) {
                    let _ = db.create_object_store(name);
                }
            }
        }
    });
    request.set_onupgradeneeded(Some(upgrade.unchecked_ref()));
    Ok(wait(&request).await?.unchecked_into())
}

async fn wait(request: &IdbRequest) -> Result<JsValue, JsValue> {
    let request = request.clone();
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let done = request.clone();
        let failed = request.clone();
        let on_success = Closure::once_into_js(move || {
            let _ = resolve.call1(&JsValue::NULL, &done.result().unwrap_or(JsValue::UNDEFINED));
        });
        let on_error = Closure::once_into_js(move || {
            let error = failed
                .error()
                .ok()
                .flatten()
                .map_or(JsValue::UNDEFINED, JsValue::from);
            let _ = reject.call1(&JsValue::NULL, &error);
        });
        request.set_onsuccess(Some(on_success.unchecked_ref()));
        request.set_onerror(Some(on_error.unchecked_ref()));
    });
    wasm_bindgen_futures::JsFuture::from(promise).await
}

async fn finished(tx: &IdbTransaction) -> Result<(), JsValue> {
    let tx = tx.clone();
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let on_complete = Closure::once_into_js(move || {
            let _ = resolve.call0(&JsValue::NULL);
        });
        let failed = tx.clone();
        let on_error = Closure::once_into_js(move || {
            let error = failed.error().map_or(JsValue::UNDEFINED, JsValue::from);
            let _ = reject.call1(&JsValue::NULL, &error);
        });
        tx.set_oncomplete(Some(on_complete.unchecked_ref()));
        tx.set_onabort(Some(on_error.unchecked_ref()));
    });
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map(drop)
}
