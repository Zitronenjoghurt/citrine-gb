pub mod aux_spi;
pub mod boot;
pub mod bus;
pub mod card;
pub mod cp15;
pub mod dma;
pub mod download;
pub mod geometry;
pub mod input;
pub mod interrupt;
pub mod io_names;
pub mod ipc;
pub mod ir;
pub mod lcd;
pub mod machine;
pub mod math;
pub mod mic;
mod ram;
pub mod render;
pub mod rtc;
pub mod save;
pub mod sound;
pub mod spi;
pub mod tcm;
pub mod three_d;
pub mod timer;
pub mod video;
pub mod vram;
pub mod wifi;
pub mod wram;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Cpu {
    Arm9,
    Arm7,
}
