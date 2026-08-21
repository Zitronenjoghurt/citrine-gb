pub mod avg_timer;
pub mod file_channels;
pub mod file_loader;
pub mod file_saver;

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn(future: impl std::future::Future<Output = ()> + Send + 'static) {
    std::thread::spawn(|| pollster::block_on(future));
}

#[cfg(target_arch = "wasm32")]
pub fn spawn(future: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(future);
}
