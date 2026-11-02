use super::{MediaId, MediaKind, MediaStore};
use crate::icons;
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    Copy,
    Share,
}

impl Way {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Copy => icons::COPY,
            Self::Share => icons::SHARE_NETWORK,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::Share => "Share",
        }
    }

    pub fn hint(self, kind: MediaKind) -> &'static str {
        match (self, kind) {
            (Self::Copy, MediaKind::Screenshot) => "Copy picture",
            (Self::Copy, MediaKind::Recording) => "Copy file",
            (Self::Share, _) => "Share",
        }
    }
}

pub fn pick(kind: MediaKind, shares: bool, copies_images: bool) -> Option<Way> {
    match kind {
        _ if shares => Some(Way::Share),
        MediaKind::Screenshot if copies_images => Some(Way::Copy),
        MediaKind::Screenshot => None,
        MediaKind::Recording if cfg!(target_arch = "wasm32") => None,
        MediaKind::Recording => Some(Way::Copy),
    }
}

pub fn way(kind: MediaKind) -> Option<Way> {
    pick(kind, platform::shares(), platform::copies_images())
}

pub fn copied(kind: MediaKind) -> String {
    format!("{} copied", kind.label())
}

pub fn run(way: Way, store: &MediaStore, id: &MediaId, done: Sender<Result<String, String>>) {
    let kind = id.kind().unwrap_or(MediaKind::Screenshot);
    match way {
        Way::Copy => platform::copy(store, id, kind, done),
        Way::Share => platform::share(store, id, kind, done),
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use super::{MediaId, MediaKind, MediaStore, Sender, copied};
    use std::sync::{Mutex, OnceLock};

    static CLIPBOARD: OnceLock<Mutex<Option<arboard::Clipboard>>> = OnceLock::new();

    pub fn shares() -> bool {
        false
    }

    pub fn copies_images() -> bool {
        true
    }

    fn with_clipboard(
        put: impl FnOnce(&mut arboard::Clipboard) -> Result<(), arboard::Error>,
    ) -> Result<(), String> {
        let mut slot = CLIPBOARD
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "the clipboard is busy".to_string())?;
        if slot.is_none() {
            *slot = Some(arboard::Clipboard::new().map_err(|err| err.to_string())?);
        }
        let clipboard = slot.as_mut().expect("the clipboard was just opened");
        put(clipboard).map_err(|err| err.to_string())
    }

    pub fn copy(
        store: &MediaStore,
        id: &MediaId,
        kind: MediaKind,
        done: Sender<Result<String, String>>,
    ) {
        let path = store.path(id);
        let result = match kind {
            MediaKind::Screenshot => std::fs::read(&path)
                .map_err(|err| err.to_string())
                .and_then(|bytes| {
                    crate::media::decode_png(&bytes).ok_or_else(|| "unreadable picture".into())
                })
                .and_then(|frame| {
                    with_clipboard(|clipboard| {
                        clipboard.set_image(arboard::ImageData {
                            width: frame.width as usize,
                            height: frame.height as usize,
                            bytes: frame.rgba.into(),
                        })
                    })
                }),
            MediaKind::Recording => {
                with_clipboard(|clipboard| clipboard.set().file_list(&[path.as_path()]))
            }
        };
        let _ = done.send(result.map(|()| copied(kind)));
    }

    pub fn share(
        _store: &MediaStore,
        _id: &MediaId,
        _kind: MediaKind,
        done: Sender<Result<String, String>>,
    ) {
        let _ = done.send(Err("sharing works only in the browser".into()));
    }
}

#[cfg(target_arch = "wasm32")]
mod platform {
    use super::{MediaId, MediaKind, MediaStore, Sender, copied};
    use js_sys::{Array, Function, Object, Promise, Reflect};
    use wasm_bindgen::{JsCast, JsValue};

    fn global(name: &str) -> Option<JsValue> {
        Reflect::get(&js_sys::global(), &name.into())
            .ok()
            .filter(|value| !value.is_undefined())
    }

    fn navigator() -> Option<JsValue> {
        global("navigator")
    }

    fn method(target: &JsValue, name: &str) -> Option<Function> {
        Reflect::get(target, &name.into()).ok()?.dyn_into().ok()
    }

    fn mime(kind: MediaKind) -> &'static str {
        match kind {
            MediaKind::Screenshot => "image/png",
            MediaKind::Recording => "video/mp4",
        }
    }

    fn file(parts: &JsValue, name: &str, kind: MediaKind) -> Option<JsValue> {
        let constructor: Function = global("File")?.dyn_into().ok()?;
        let options = Object::new();
        Reflect::set(&options, &"type".into(), &mime(kind).into()).ok()?;
        let args = Array::of3(&Array::of1(parts), &name.into(), &options);
        Reflect::construct(&constructor, &args).ok()
    }

    fn share_data(file: &JsValue) -> Option<Object> {
        let data = Object::new();
        Reflect::set(&data, &"files".into(), &Array::of1(file)).ok()?;
        Some(data)
    }

    pub fn shares() -> bool {
        thread_local! {
            static SHARES: bool = detect_share();
        }
        SHARES.with(|shares| *shares)
    }

    fn detect_share() -> bool {
        let coarse = web_sys::window()
            .and_then(|window| window.match_media("(pointer: coarse)").ok().flatten())
            .is_some_and(|query| query.matches());
        if !coarse {
            return false;
        }
        let Some(navigator) = navigator() else {
            return false;
        };
        let (Some(can_share), Some(_)) =
            (method(&navigator, "canShare"), method(&navigator, "share"))
        else {
            return false;
        };
        let Some(probe) = file(
            &js_sys::Uint8Array::new_with_length(0),
            "a.mp4",
            MediaKind::Recording,
        ) else {
            return false;
        };
        let Some(data) = share_data(&probe) else {
            return false;
        };
        can_share
            .call1(&navigator, &data)
            .ok()
            .is_some_and(|answer| answer.is_truthy())
    }

    pub fn copies_images() -> bool {
        let clipboard = navigator().and_then(|n| Reflect::get(&n, &"clipboard".into()).ok());
        global("ClipboardItem").is_some()
            && clipboard.is_some_and(|clipboard| method(&clipboard, "write").is_some())
    }

    fn reason(err: JsValue) -> String {
        Reflect::get(&err, &"message".into())
            .ok()
            .and_then(|message| message.as_string())
            .unwrap_or_else(|| "the browser said no".into())
    }

    fn settle(promise: Promise, done: Sender<Result<String, String>>, ok: Option<String>) {
        wasm_bindgen_futures::spawn_local(async move {
            let result = match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(_) => ok.ok_or_else(String::new),
                Err(err) => {
                    let name = Reflect::get(&err, &"name".into())
                        .ok()
                        .and_then(|name| name.as_string());
                    if name.as_deref() == Some("AbortError") {
                        Err(String::new())
                    } else {
                        Err(reason(err))
                    }
                }
            };
            let _ = done.send(result);
        });
    }

    pub fn copy(
        store: &MediaStore,
        id: &MediaId,
        kind: MediaKind,
        done: Sender<Result<String, String>>,
    ) {
        let attempt = || -> Option<Promise> {
            let pending = {
                let id = id.clone();
                let mut resolve_later = None;
                let promise = Promise::new(&mut |resolve, reject| {
                    resolve_later = Some((resolve, reject));
                });
                let (resolve, reject) = resolve_later?;
                store.read(&id, move |blob| match blob {
                    Some(blob) => {
                        let options = web_sys::BlobPropertyBag::new();
                        options.set_type(mime(kind));
                        let typed = web_sys::Blob::new_with_blob_sequence_and_options(
                            &Array::of1(&blob),
                            &options,
                        );
                        match typed {
                            Ok(typed) => {
                                let _ = resolve.call1(&JsValue::NULL, &typed);
                            }
                            Err(err) => {
                                let _ = reject.call1(&JsValue::NULL, &err);
                            }
                        }
                    }
                    None => {
                        let _ = reject.call1(
                            &JsValue::NULL,
                            &js_sys::Error::new("the file is missing").into(),
                        );
                    }
                });
                promise
            };
            let items = Object::new();
            Reflect::set(&items, &mime(kind).into(), &pending).ok()?;
            let constructor: Function = global("ClipboardItem")?.dyn_into().ok()?;
            let item = Reflect::construct(&constructor, &Array::of1(&items)).ok()?;
            let clipboard = Reflect::get(&navigator()?, &"clipboard".into()).ok()?;
            let write = method(&clipboard, "write")?;
            write
                .call1(&clipboard, &Array::of1(&item))
                .ok()?
                .dyn_into()
                .ok()
        };
        match attempt() {
            Some(promise) => settle(promise, done, Some(copied(kind))),
            None => {
                let _ = done.send(Err("this browser can't copy pictures".into()));
            }
        }
    }

    pub fn share(
        store: &MediaStore,
        id: &MediaId,
        kind: MediaKind,
        done: Sender<Result<String, String>>,
    ) {
        let name = id.file_name().to_string();
        store.read(id, move |blob| {
            let shared = blob.and_then(|blob| {
                let file = file(&blob, &name, kind)?;
                let data: JsValue = share_data(&file)?.into();
                let navigator = navigator()?;
                let share = method(&navigator, "share")?;
                share
                    .call1(&navigator, &data)
                    .ok()?
                    .dyn_into::<Promise>()
                    .ok()
            });
            match shared {
                Some(promise) => settle(promise, done, None),
                None => {
                    let _ = done.send(Err("the file could not be shared".into()));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_shares_both_kinds() {
        for kind in MediaKind::ALL {
            assert_eq!(pick(kind, true, true), Some(Way::Share));
        }
    }

    #[test]
    fn a_desktop_copies_pictures_and_native_recordings() {
        assert_eq!(pick(MediaKind::Screenshot, false, true), Some(Way::Copy));
        assert_eq!(pick(MediaKind::Screenshot, false, false), None);
        assert_eq!(pick(MediaKind::Recording, false, true), Some(Way::Copy));
    }
}
