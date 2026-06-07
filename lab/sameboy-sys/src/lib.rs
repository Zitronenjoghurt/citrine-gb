//! Minimal hand-written FFI bindings for the SameBoy core.
//!
//! Only the subset of the C API needed by the Citrine lab is declared here. Signatures and
//! constants are mirrored from `lab/SameBoy/Core/{gb,display,joypad,model}.h`. Keep them in sync
//! if the submodule is updated.
#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]

use std::ffi::{c_int, c_uint, c_void};

/// Opaque SameBoy instance. We only ever hold a pointer to one (via `GB_alloc`).
#[repr(C)]
pub struct GB_gameboy_t {
    _private: [u8; 0],
}

/// `GB_model_t` (see `model.h`). The enum's underlying type is `int`.
pub type GB_model_t = c_int;
pub const GB_MODEL_DMG_B: GB_model_t = 0x002;
pub const GB_MODEL_CGB_E: GB_model_t = 0x205;

/// `GB_key_t` (see `joypad.h`).
pub type GB_key_t = c_int;
pub const GB_KEY_RIGHT: GB_key_t = 0;
pub const GB_KEY_LEFT: GB_key_t = 1;
pub const GB_KEY_UP: GB_key_t = 2;
pub const GB_KEY_DOWN: GB_key_t = 3;
pub const GB_KEY_A: GB_key_t = 4;
pub const GB_KEY_B: GB_key_t = 5;
pub const GB_KEY_SELECT: GB_key_t = 6;
pub const GB_KEY_START: GB_key_t = 7;

/// `GB_vblank_type_t` (see `display.h`).
pub type GB_vblank_type_t = c_int;

/// `GB_color_correction_mode_t` (see `display.h`).
pub type GB_color_correction_mode_t = c_int;
pub const GB_COLOR_CORRECTION_DISABLED: GB_color_correction_mode_t = 0;

/// `GB_palette_t` (see `display.h`): 5 colors of `{r, g, b}` bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GB_color_s {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GB_palette_t {
    pub colors: [GB_color_s; 5],
}

/// `GB_registers_t` (see `gb.h`). The C type is a union; its 16-bit view is laid out exactly as
/// these fields, so a `#[repr(C)]` struct of six `u16`s aliases it correctly.
#[repr(C)]
pub struct GB_registers_t {
    pub af: u16,
    pub bc: u16,
    pub de: u16,
    pub hl: u16,
    pub sp: u16,
    pub pc: u16,
}

pub type GB_vblank_callback_t = extern "C" fn(gb: *mut GB_gameboy_t, ty: GB_vblank_type_t);
pub type GB_rgb_encode_callback_t =
    extern "C" fn(gb: *mut GB_gameboy_t, r: u8, g: u8, b: u8) -> u32;

unsafe extern "C" {
    pub fn GB_alloc() -> *mut GB_gameboy_t;
    pub fn GB_dealloc(gb: *mut GB_gameboy_t);
    pub fn GB_init(gb: *mut GB_gameboy_t, model: GB_model_t) -> *mut GB_gameboy_t;
    pub fn GB_reset(gb: *mut GB_gameboy_t);

    pub fn GB_load_rom_from_buffer(gb: *mut GB_gameboy_t, buffer: *const u8, size: usize);
    pub fn GB_load_boot_rom_from_buffer(gb: *mut GB_gameboy_t, buffer: *const u8, size: usize);

    pub fn GB_run(gb: *mut GB_gameboy_t) -> c_uint;

    /// Disable SameBoy's real-time clock sync (`GB_timing_sync`), which otherwise `nanosleep`s to
    /// pin the core to ~60 fps. `no_frame_skip = true` keeps every frame rendered. Essential for the
    /// lab: without it a 10k-frame run sleeps for ~167 s of wall time.
    pub fn GB_set_turbo_mode(gb: *mut GB_gameboy_t, on: bool, no_frame_skip: bool);

    pub fn GB_set_pixels_output(gb: *mut GB_gameboy_t, output: *mut u32);
    pub fn GB_set_rgb_encode_callback(gb: *mut GB_gameboy_t, callback: GB_rgb_encode_callback_t);
    pub fn GB_set_vblank_callback(gb: *mut GB_gameboy_t, callback: GB_vblank_callback_t);
    pub fn GB_get_screen_width(gb: *mut GB_gameboy_t) -> c_uint;
    pub fn GB_get_screen_height(gb: *mut GB_gameboy_t) -> c_uint;

    pub fn GB_set_key_state(gb: *mut GB_gameboy_t, index: GB_key_t, pressed: bool);

    pub fn GB_set_palette(gb: *mut GB_gameboy_t, palette: *const GB_palette_t);
    pub fn GB_set_color_correction_mode(gb: *mut GB_gameboy_t, mode: GB_color_correction_mode_t);
    pub static GB_PALETTE_GREY: GB_palette_t;

    pub fn GB_get_registers(gb: *mut GB_gameboy_t) -> *mut GB_registers_t;

    pub fn GB_set_user_data(gb: *mut GB_gameboy_t, data: *mut c_void);
    pub fn GB_get_user_data(gb: *mut GB_gameboy_t) -> *mut c_void;
}
