pub mod avg_timer;
pub mod file_channels;
pub mod file_loader;
pub mod file_saver;
pub mod save_files;
pub mod screenshot;
pub mod zip_games;

#[cfg(target_arch = "wasm32")]
pub async fn next_tick() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback(&resolve);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

#[cfg(not(target_arch = "wasm32"))]
pub fn reduced_motion() -> bool {
    false
}

#[cfg(target_arch = "wasm32")]
pub fn reduced_motion() -> bool {
    web_sys::window()
        .and_then(|window| window.match_media("(prefers-reduced-motion: reduce)").ok())
        .flatten()
        .is_some_and(|query| query.matches())
}
#[cfg(target_arch = "wasm32")]
pub mod idb;
