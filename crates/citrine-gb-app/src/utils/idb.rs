use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::{JsCast, JsValue};
use web_sys::{IdbDatabase, IdbRequest, IdbTransaction};

pub async fn open(
    name: &str,
    version: u32,
    stores: &[&'static str],
) -> Result<IdbDatabase, JsValue> {
    let factory = web_sys::window()
        .ok_or("no window")?
        .indexed_db()?
        .ok_or("no IndexedDB")?;
    let request = factory.open_with_u32(name, version)?;
    let target = request.clone();
    let stores = stores.to_vec();
    let upgrade = Closure::once_into_js(move || {
        if let Ok(db) = target.result() {
            let db: IdbDatabase = db.unchecked_into();
            let names = db.object_store_names();
            for name in stores {
                if !names.contains(name) {
                    let _ = db.create_object_store(name);
                }
            }
        }
    });
    request.set_onupgradeneeded(Some(upgrade.unchecked_ref()));
    Ok(wait(&request).await?.unchecked_into())
}

pub async fn wait(request: &IdbRequest) -> Result<JsValue, JsValue> {
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

pub async fn finished(tx: &IdbTransaction) -> Result<(), JsValue> {
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
