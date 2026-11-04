mod app;
mod audio;
mod autosave;
mod controller_debug;
#[cfg(all(feature = "dev-route", target_arch = "wasm32"))]
mod dev_route;
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
#[cfg(all(test, not(target_arch = "wasm32")))]
mod pins;
mod platform;
pub mod play;
mod progress;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod real_roms;
mod recorder;
mod replay;
mod senses;
mod stats;
mod storage;
mod update;
mod utils;
mod your_data;

pub use app::Citrine;
pub use egui_phosphor::regular as icons;
#[cfg(target_arch = "wasm32")]
pub use storage::open_saves;
