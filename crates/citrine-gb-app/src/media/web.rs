use super::{
    MediaId, MediaInfo, MediaItem, NewMedia, decode_png, encode_thumbnail, sort_newest_first,
    unique_file_name,
};
use crate::utils::idb::{finished, wait};
use std::cell::{Cell, RefCell};
use std::io;
use std::sync::mpsc::{Receiver, Sender, channel};
use web_sys::wasm_bindgen::{JsCast, JsValue};
use web_sys::{IdbDatabase, IdbTransaction, IdbTransactionMode};

const DB_NAME: &str = "citrine-media";
const DB_VERSION: u32 = 1;
const ITEMS: &str = "items";
const FILES: &str = "files";
const READ_BATCH: usize = 32;
const WRITE_BATCH: usize = 64;

thread_local! {
    static WAITING: RefCell<Vec<(String, JsValue, JsValue)>> = RefCell::default();
    static WRITING: Cell<bool> = Cell::default();
}

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
    pub fn name_games(&mut self, names: &crate::game_name::GameNames) {
        super::name_games(&mut self.items, names);
    }

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

    pub fn add_file(&mut self, new: NewMedia, bytes: &[u8]) -> io::Result<MediaItem> {
        self.add_bytes(new, bytes.to_vec())
    }

    pub fn read_bytes(&self, id: &MediaId, done: impl FnOnce(Option<Vec<u8>>) + 'static) {
        self.read(id, move |blob| match blob {
            None => done(None),
            Some(blob) => wasm_bindgen_futures::spawn_local(async move {
                let buffer = wasm_bindgen_futures::JsFuture::from(blob.array_buffer())
                    .await
                    .ok();
                done(buffer.map(|b| js_sys::Uint8Array::new(&b).to_vec()));
            }),
        });
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
        queue_put(id.as_str().to_owned(), record, file);
        self.items.push(item.clone());
        sort_newest_first(&mut self.items);
        Ok(item)
    }

    pub fn delete(&mut self, id: &MediaId) -> io::Result<()> {
        self.items.retain(|i| &i.id != id);
        let key = id.as_str().to_string();
        WAITING.with_borrow_mut(|waiting| waiting.retain(|(queued, _, _)| *queued != key));
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(err) = remove(&key).await {
                log::error!("Could not delete {key}: {err:?}");
            }
        });
        Ok(())
    }

    pub fn read_files(&self, mut each: impl FnMut(Option<(MediaId, Vec<u8>)>) + 'static) {
        let ids: Vec<MediaId> = self.items.iter().map(|item| item.id.clone()).collect();
        wasm_bindgen_futures::spawn_local(async move {
            while WRITING.get() {
                crate::utils::next_tick().await;
            }
            for chunk in ids.chunks(READ_BATCH) {
                let keys: Vec<&str> = chunk.iter().map(MediaId::as_str).collect();
                let Ok(blobs) = read_many(&keys).await else {
                    continue;
                };
                let found: Vec<(&MediaId, web_sys::Blob)> = chunk
                    .iter()
                    .zip(blobs)
                    .filter_map(|(id, blob)| Some((id, blob?)))
                    .collect();
                let buffers = found
                    .iter()
                    .map(|(_, blob)| JsValue::from(blob.array_buffer()))
                    .collect::<js_sys::Array>();
                let Ok(buffers) =
                    wasm_bindgen_futures::JsFuture::from(js_sys::Promise::all(&buffers)).await
                else {
                    continue;
                };
                let buffers: js_sys::Array = buffers.unchecked_into();
                for ((id, _), buffer) in found.iter().zip(buffers.iter()) {
                    each(Some((
                        (*id).clone(),
                        js_sys::Uint8Array::new(&buffer).to_vec(),
                    )));
                }
            }
            each(None);
        });
    }

    pub fn restore(
        &mut self,
        id: &MediaId,
        info: MediaInfo,
        thumb_png: Option<&[u8]>,
        bytes: &[u8],
    ) -> io::Result<()> {
        let kind = id
            .kind()
            .ok_or_else(|| io::Error::other("unknown media kind"))?;
        let info = MediaInfo {
            kind,
            bytes: bytes.len() as u64,
            ..info
        };
        let thumb_png = thumb_png.unwrap_or_default();
        let record = record(&info, thumb_png).ok_or_else(|| io::Error::other("bad record"))?;
        let file = js_sys::Uint8Array::from(bytes);
        queue_put(id.as_str().to_owned(), record, file.into());
        self.items.retain(|i| &i.id != id);
        self.items.push(MediaItem {
            id: id.clone(),
            info,
            thumbnail: decode_png(thumb_png),
        });
        sort_newest_first(&mut self.items);
        Ok(())
    }

    pub fn delete_all(&mut self) -> io::Result<()> {
        self.items.clear();
        self.loaded = true;
        WAITING.with_borrow_mut(Vec::clear);
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(err) = clear().await {
                log::error!("Could not clear the media library: {err:?}");
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

fn queue_put(key: String, record: JsValue, file: JsValue) {
    WAITING.with_borrow_mut(|waiting| waiting.push((key, record, file)));
    if WRITING.replace(true) {
        return;
    }
    wasm_bindgen_futures::spawn_local(async {
        loop {
            let batch: Vec<_> = WAITING.with_borrow_mut(|waiting| {
                let end = waiting.len().min(WRITE_BATCH);
                waiting.drain(..end).collect()
            });
            if batch.is_empty() {
                break;
            }
            if let Err(err) = put_all(&batch).await {
                log::error!("Could not store {} media files: {err:?}", batch.len());
            }
        }
        WRITING.set(false);
    });
}

async fn put_all(batch: &[(String, JsValue, JsValue)]) -> Result<(), JsValue> {
    let db = open().await?;
    let tx = readwrite(&db)?;
    let files = tx.object_store(FILES)?;
    let items = tx.object_store(ITEMS)?;
    for (key, record, file) in batch {
        let key = JsValue::from(key.as_str());
        files.put_with_key(file, &key)?;
        items.put_with_key(record, &key)?;
    }
    finished(&tx).await
}

async fn remove(key: &str) -> Result<(), JsValue> {
    let db = open().await?;
    let tx = readwrite(&db)?;
    tx.object_store(FILES)?.delete(&key.into())?;
    tx.object_store(ITEMS)?.delete(&key.into())?;
    finished(&tx).await
}

async fn clear() -> Result<(), JsValue> {
    let db = open().await?;
    let tx = readwrite(&db)?;
    tx.object_store(FILES)?.clear()?;
    tx.object_store(ITEMS)?.clear()?;
    finished(&tx).await
}

async fn read_file(key: &str) -> Result<Option<web_sys::Blob>, JsValue> {
    Ok(read_many(&[key]).await?.pop().flatten())
}

async fn read_many(keys: &[&str]) -> Result<Vec<Option<web_sys::Blob>>, JsValue> {
    let db = open().await?;
    let tx = db.transaction_with_str(FILES)?;
    let store = tx.object_store(FILES)?;
    let requests = keys
        .iter()
        .map(|key| store.get(&JsValue::from(*key)))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(last) = requests.last() {
        wait(last).await?;
    }
    requests
        .iter()
        .map(|request| request.result().map(as_blob))
        .collect()
}

fn as_blob(value: JsValue) -> Option<web_sys::Blob> {
    if value.is_instance_of::<web_sys::Blob>() {
        return Some(value.unchecked_into());
    }
    let bytes = value.dyn_into::<js_sys::Uint8Array>().ok()?;
    web_sys::Blob::new_with_u8_array_sequence(&js_sys::Array::of1(&bytes)).ok()
}

fn readwrite(db: &IdbDatabase) -> Result<IdbTransaction, JsValue> {
    let names = js_sys::Array::of2(&ITEMS.into(), &FILES.into());
    db.transaction_with_str_sequence_and_mode(&names, IdbTransactionMode::Readwrite)
}

async fn open() -> Result<IdbDatabase, JsValue> {
    crate::utils::idb::open(DB_NAME, DB_VERSION, &[ITEMS, FILES]).await
}
