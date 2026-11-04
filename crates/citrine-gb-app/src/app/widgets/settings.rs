pub mod cartridge;
pub mod content;
pub mod controls;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub mod discord;
pub mod form;
pub mod online;
pub mod palette;
pub mod your_data;
