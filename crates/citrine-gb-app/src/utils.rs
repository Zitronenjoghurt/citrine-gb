pub mod avg_timer;
pub mod file_channels;
pub mod file_loader;
pub mod file_saver;
pub mod save_files;
pub mod screenshot;

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn spawn(future: impl std::future::Future<Output = ()> + Send + 'static) {
    std::thread::spawn(|| pollster::block_on(future));
}

#[cfg(target_arch = "wasm32")]
pub fn spawn(future: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(future);
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
