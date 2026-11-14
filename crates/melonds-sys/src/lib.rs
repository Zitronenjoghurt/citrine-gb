use std::ffi::{c_uint, c_void};

#[repr(C)]
pub struct CitrineMelonds {
    _private: [u8; 0],
}

pub const SCREEN_WIDTH: usize = 256;
pub const SCREEN_HEIGHT: usize = 192;
pub const ARM9_BIOS_SIZE: usize = 0x1000;
pub const ARM7_BIOS_SIZE: usize = 0x4000;

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
pub const KEY_X: u32 = 1 << 10;
pub const KEY_Y: u32 = 1 << 11;

pub const SCENE_POLYGONS: usize = 2048;
pub const SCENE_VERTICES: usize = 6144 * 2;
pub const TEXTURE_SIZE: usize = 512 * 1024;
pub const PALETTE_SIZE: usize = 128 * 1024;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SceneVertex {
    pub position: [i32; 4],
    pub screen: [i32; 2],
    pub hires: [i32; 2],
    pub color: [i32; 3],
    pub texcoord: [i32; 2],
    pub clipped: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ScenePolygon {
    pub vertices: [u32; 10],
    pub count: u32,
    pub z: [i32; 10],
    pub w: [i32; 10],
    pub attributes: u32,
    pub texture: u32,
    pub palette: u32,
    pub w_buffer: u32,
    pub degenerate: u32,
    pub front: u32,
    pub translucent: u32,
    pub top: u32,
    pub bottom: u32,
}

#[repr(C)]
pub struct Scene3d {
    pub disp3dcnt: u32,
    pub alpha_reference: u32,
    pub clear_color: u32,
    pub clear_depth: u32,
    pub fog_color: u32,
    pub fog_offset: u32,
    pub toon: [u32; 32],
    pub edge: [u32; 8],
    pub fog_density: [u32; 32],
    pub polygon_count: u32,
    pub vertex_count: u32,
    pub polygons: [ScenePolygon; SCENE_POLYGONS],
    pub vertices: [SceneVertex; SCENE_VERTICES],
    pub texture: [u8; TEXTURE_SIZE],
    pub palette: [u8; PALETTE_SIZE],
    pub layer: [u32; SCREEN_WIDTH * SCREEN_HEIGHT],
    pub plain: [u32; SCREEN_WIDTH * SCREEN_HEIGHT],
    pub depth: [u32; SCREEN_WIDTH * SCREEN_HEIGHT],
    pub attributes: [u32; SCREEN_WIDTH * SCREEN_HEIGHT],
    pub under_color: [u32; SCREEN_WIDTH * SCREEN_HEIGHT],
    pub under_depth: [u32; SCREEN_WIDTH * SCREEN_HEIGHT],
    pub under_attributes: [u32; SCREEN_WIDTH * SCREEN_HEIGHT],
}

unsafe extern "C" {
    pub fn citrine_melonds_new(
        rom: *const c_void,
        rom_size: usize,
        bios9: *const c_void,
        bios9_size: usize,
        bios7: *const c_void,
        bios7_size: usize,
        sample_rate: c_uint,
    ) -> *mut CitrineMelonds;
    #[cfg(feature = "jit")]
    pub fn citrine_melonds_new_jit(
        rom: *const c_void,
        rom_size: usize,
        bios9: *const c_void,
        bios9_size: usize,
        bios7: *const c_void,
        bios7_size: usize,
        sample_rate: c_uint,
    ) -> *mut CitrineMelonds;
    pub fn citrine_melonds_free(emu: *mut CitrineMelonds);
    pub fn citrine_melonds_set_save(emu: *mut CitrineMelonds, data: *const c_void, size: usize);
    pub fn citrine_melonds_set_keys(emu: *mut CitrineMelonds, keys: u32);
    pub fn citrine_melonds_touch(emu: *mut CitrineMelonds, x: c_uint, y: c_uint, down: c_uint);
    pub fn citrine_melonds_run_frame(emu: *mut CitrineMelonds) -> u32;
    pub fn citrine_melonds_screen(emu: *const CitrineMelonds, index: c_uint) -> *const u32;
    pub fn citrine_melonds_cycles(emu: *const CitrineMelonds) -> u64;
    pub fn citrine_melonds_scene3d(emu: *mut CitrineMelonds, out: *mut Scene3d);
    pub fn citrine_melonds_read_audio(
        emu: *mut CitrineMelonds,
        out: *mut i16,
        max_frames: usize,
    ) -> usize;
}
