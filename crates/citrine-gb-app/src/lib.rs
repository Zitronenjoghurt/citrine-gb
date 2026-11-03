mod app;
mod audio;
mod autosave;
mod controller_debug;
mod discord;
mod emulator;
mod game_info;
pub mod game_name;
mod held;
mod homebrew;
mod local_link;
mod media;
pub mod online;
mod pad_input;
mod partner_link;
mod platform;
pub mod play;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod real_roms;
mod recorder;
mod replay;
mod stats;
mod storage;
mod update;
mod utils;
mod your_data;

pub use app::Citrine;
pub use egui_phosphor::regular as icons;
