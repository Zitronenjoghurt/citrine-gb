#[cfg(target_os = "android")]
pub mod android;
#[cfg(any(target_os = "android", test))]
pub mod ime;

#[cfg(target_os = "android")]
pub fn haptic_tick() {
    android::haptic();
}

#[cfg(target_arch = "wasm32")]
pub fn haptic_tick() {
    if let Some(window) = web_sys::window() {
        let _ = window.navigator().vibrate_with_duration(15);
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn haptic_tick() {}

#[cfg(target_os = "android")]
pub fn immersive(on: bool) {
    android::lifecycle::immersive(on);
}

#[cfg(not(target_os = "android"))]
pub fn immersive(_on: bool) {}
