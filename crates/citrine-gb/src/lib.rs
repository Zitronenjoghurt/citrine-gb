#[cfg(feature = "gba")]
mod achievement_map;
pub mod cheats;
pub mod console;
#[cfg(feature = "debug")]
pub mod debug;
pub mod error;
pub mod gb;
#[cfg(feature = "gba")]
pub mod gba;
pub mod instructions;
#[cfg(jit)]
pub mod jit;
#[cfg(feature = "jit")]
pub mod jit_cache;
pub mod machine;
#[cfg(feature = "nds")]
pub mod nds;
#[cfg(feature = "persistence")]
pub mod netplay;
#[cfg(feature = "gba")]
mod paged;
#[cfg(feature = "persistence")]
pub mod persistence;
#[cfg(any(feature = "gba", feature = "nds"))]
mod ppu2d;
#[cfg(feature = "recording")]
pub mod recording;
pub mod rom;
#[cfg(test)]
mod test_support;
pub mod traffic;
pub mod utils;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub trait ReadMemory {
    fn read_naive(&self, addr: u16) -> u8;
}

impl ReadMemory for &[u8] {
    fn read_naive(&self, addr: u16) -> u8 {
        self.get(addr as usize).copied().unwrap_or(0xFF)
    }
}

pub trait WriteMemory {
    fn write_naive(&mut self, addr: u16, value: u8);
}

impl WriteMemory for &mut [u8] {
    fn write_naive(&mut self, addr: u16, value: u8) {
        if let Some(byte) = self.get_mut(addr as usize) {
            *byte = value;
        }
    }
}
