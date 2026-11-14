pub mod cartridge;
pub mod content;
pub mod controls;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub mod discord;
#[cfg(feature = "nds")]
pub mod ds;
pub mod ds_screens;
pub mod form;
pub mod online;
pub mod page;
pub mod palette;
pub mod your_data;
