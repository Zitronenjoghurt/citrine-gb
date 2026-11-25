mod achievements;
mod app;
mod audio;
mod autosave;
mod cheats;
mod combo;
mod controller_debug;
mod covers;
#[cfg(all(feature = "dev-route", target_arch = "wasm32"))]
mod dev_route;
mod discord;
#[cfg(feature = "nds")]
mod ds_banner;
#[cfg(ds_wifi)]
mod ds_wifi;
mod emulator;
mod frame_log;
mod game_facts;
mod game_info;
pub use homebrew::names as game_name;
pub mod game_report;
mod held;
mod homebrew;
pub mod icons;
#[cfg(not(target_arch = "wasm32"))]
pub mod lan;
mod local_link;
mod media;
pub mod online;
mod pad_input;
mod partner_link;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod pins;
mod platform;
pub mod play;
mod profile;
mod progress;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod real_roms;
mod recorder;
mod replay;
mod rom_ids;
mod second_screens;
mod senses;
mod stats;
mod storage;
#[cfg(all(test, target_os = "linux", target_env = "gnu"))]
mod test_heap;
mod update;
mod utils;
#[cfg(feature = "nds")]
pub mod walker;
mod your_data;

pub use app::Citrine;
#[cfg(not(target_arch = "wasm32"))]
pub use platform::wgpu_options;
#[cfg(not(target_arch = "wasm32"))]
pub use storage::app_state_file;
#[cfg(target_arch = "wasm32")]
pub use storage::open_saves;
