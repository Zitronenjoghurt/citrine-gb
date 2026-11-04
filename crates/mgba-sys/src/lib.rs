use std::ffi::{c_uint, c_void};

#[repr(C)]
pub struct CitrineMgba {
    _private: [u8; 0],
}

pub const KEY_A: u32 = 1 << 0;
pub const KEY_B: u32 = 1 << 1;
pub const KEY_SELECT: u32 = 1 << 2;
pub const KEY_START: u32 = 1 << 3;
pub const KEY_RIGHT: u32 = 1 << 4;
pub const KEY_LEFT: u32 = 1 << 5;
pub const KEY_UP: u32 = 1 << 6;
pub const KEY_DOWN: u32 = 1 << 7;
pub const KEY_R: u32 = 1 << 8;
pub const KEY_L: u32 = 1 << 9;

unsafe extern "C" {
    pub fn citrine_mgba_new(
        rom: *const c_void,
        rom_size: usize,
        bios: *const c_void,
        bios_size: usize,
    ) -> *mut CitrineMgba;
    pub fn citrine_mgba_free(emu: *mut CitrineMgba);
    pub fn citrine_mgba_width(emu: *const CitrineMgba) -> c_uint;
    pub fn citrine_mgba_height(emu: *const CitrineMgba) -> c_uint;
    pub fn citrine_mgba_pixels(emu: *const CitrineMgba) -> *const u32;
    pub fn citrine_mgba_set_keys(emu: *mut CitrineMgba, keys: u32);
    pub fn citrine_mgba_run_loop(emu: *mut CitrineMgba) -> u32;
    pub fn citrine_mgba_frame_counter(emu: *const CitrineMgba) -> u32;
    pub fn citrine_mgba_cycles(emu: *const CitrineMgba) -> u64;
}
