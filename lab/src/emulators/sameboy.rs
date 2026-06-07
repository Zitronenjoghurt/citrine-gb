use crate::emulator::{Button, FrameEmulator, SCREEN_HEIGHT, SCREEN_WIDTH};
use citrine_gb::gb::GbModel;
use std::ffi::c_void;
use sameboy_sys as sys;

/// State shared with the C callbacks via SameBoy's user-data pointer.
struct Ctx {
    frame_ready: bool,
}

/// Adapter driving SameBoy (the reference emulator) through the [`FrameEmulator`] trait.
///
/// SameBoy is a C library reached through `sameboy-sys`. This wrapper owns the instance, a pixel
/// buffer it renders into, and a [`Ctx`] the vblank callback flips when a frame completes.
pub struct SameBoyEmulator {
    gb: *mut sys::GB_gameboy_t,
    ctx: Box<Ctx>,
    pixels: Vec<u32>,
    width: usize,
    height: usize,
    total_ticks: u64,
    model: GbModel,
}

/// Pack r,g,b so the in-memory byte order of the u32 is `[r, g, b, a]` on little-endian targets,
/// letting us reinterpret pixels as RGBA directly.
extern "C" fn rgb_encode(_gb: *mut sys::GB_gameboy_t, r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | (0xFFu32 << 24)
}

extern "C" fn vblank(gb: *mut sys::GB_gameboy_t, _ty: sys::GB_vblank_type_t) {
    // SAFETY: user data always points at the owning `Box<Ctx>` for the lifetime of the instance.
    unsafe {
        let ctx = sys::GB_get_user_data(gb) as *mut Ctx;
        if !ctx.is_null() {
            (*ctx).frame_ready = true;
        }
    }
}

fn sys_model(model: GbModel) -> sys::GB_model_t {
    match model {
        GbModel::Dmg => sys::GB_MODEL_DMG_B,
        GbModel::Cgb => sys::GB_MODEL_CGB_E,
    }
}

fn map_button(button: Button) -> sys::GB_key_t {
    match button {
        Button::A => sys::GB_KEY_A,
        Button::B => sys::GB_KEY_B,
        Button::Select => sys::GB_KEY_SELECT,
        Button::Start => sys::GB_KEY_START,
        Button::Right => sys::GB_KEY_RIGHT,
        Button::Left => sys::GB_KEY_LEFT,
        Button::Up => sys::GB_KEY_UP,
        Button::Down => sys::GB_KEY_DOWN,
    }
}

impl SameBoyEmulator {
    pub fn new() -> Self {
        let model = GbModel::Dmg;
        let mut emu = unsafe {
            let gb = sys::GB_init(sys::GB_alloc(), sys_model(model));
            SameBoyEmulator {
                gb,
                ctx: Box::new(Ctx { frame_ready: false }),
                pixels: Vec::new(),
                width: 0,
                height: 0,
                total_ticks: 0,
                model,
            }
        };
        emu.wire_callbacks();
        emu
    }

    fn wire_callbacks(&mut self) {
        unsafe {
            sys::GB_set_user_data(self.gb, &mut *self.ctx as *mut Ctx as *mut c_void);
            sys::GB_set_rgb_encode_callback(self.gb, rgb_encode);
            sys::GB_set_vblank_callback(self.gb, vblank);
            // Force a clean, evenly-spaced greyscale so the DMG shades are palette-independent and
            // line up with Citrine's `DmgTheme::GreyScale` after canonical normalization.
            sys::GB_set_color_correction_mode(self.gb, sys::GB_COLOR_CORRECTION_DISABLED);
            sys::GB_set_palette(self.gb, &raw const sys::GB_PALETTE_GREY);
            // Run flat-out: disable SameBoy's real-time 60 fps sync (it `nanosleep`s otherwise),
            // but keep every frame (no frame skip) so the comparison sees a complete sequence.
            sys::GB_set_turbo_mode(self.gb, true, true);
            self.resize_pixel_buffer();
        }
    }

    /// (Re)allocate and rebind the pixel buffer to the current screen dimensions.
    unsafe fn resize_pixel_buffer(&mut self) {
        unsafe {
            self.width = sys::GB_get_screen_width(self.gb) as usize;
            self.height = sys::GB_get_screen_height(self.gb) as usize;
            self.pixels.clear();
            self.pixels.resize(self.width * self.height, 0xFF00_0000);
            sys::GB_set_pixels_output(self.gb, self.pixels.as_mut_ptr());
        }
    }

    /// Program the canonical post-boot CPU registers, mirroring Citrine's `Cpu::new_dmg` /
    /// `Cpu::new_cgb` so both emulators start byte-identical at the CPU level (no boot ROM).
    fn set_post_boot_registers(&mut self, rom: &[u8]) {
        let header_checksum = rom.get(0x014D).copied().unwrap_or(0);
        let regs = unsafe { &mut *sys::GB_get_registers(self.gb) };
        match self.model {
            GbModel::Dmg => {
                // F = 0xB0 (Z,H,C) when the stored header checksum byte is 0, else 0x80 (Z).
                let f: u16 = if header_checksum == 0 { 0xB0 } else { 0x80 };
                regs.af = 0x0100 | f;
                regs.bc = 0x0013;
                regs.de = 0x00D8;
                regs.hl = 0x014D;
            }
            GbModel::Cgb => {
                regs.af = 0x1180;
                regs.bc = 0x0000;
                regs.de = 0xFF56;
                regs.hl = 0x000D;
            }
        }
        regs.sp = 0xFFFE;
        regs.pc = 0x0100;
    }
}

impl Default for SameBoyEmulator {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameEmulator for SameBoyEmulator {
    fn name(&self) -> &str {
        "sameboy"
    }

    fn load(&mut self, rom: &[u8], boot_rom: Option<&[u8]>, model: GbModel) -> anyhow::Result<()> {
        if model != self.model {
            self.model = model;
            unsafe {
                sys::GB_init(self.gb, sys_model(model));
            }
            // Re-init clears callbacks/user-data; wire them again.
            self.wire_callbacks();
        }

        unsafe {
            if let Some(boot) = boot_rom {
                sys::GB_load_boot_rom_from_buffer(self.gb, boot.as_ptr(), boot.len());
            }
            sys::GB_load_rom_from_buffer(self.gb, rom.as_ptr(), rom.len());
            sys::GB_reset(self.gb);
            self.resize_pixel_buffer();
        }
        // Without a boot ROM, jump straight to the cartridge entry point with post-boot registers.
        // (SameBoy needs a boot ROM to run on its own, so prefer providing one.)
        if boot_rom.is_none() {
            self.set_post_boot_registers(rom);
        }

        self.ctx.frame_ready = false;
        self.total_ticks = 0;
        Ok(())
    }

    fn set_button(&mut self, button: Button, pressed: bool) {
        unsafe {
            sys::GB_set_key_state(self.gb, map_button(button), pressed);
        }
    }

    fn total_cycles(&self) -> u64 {
        // SameBoy counts 8 MiHz ticks; the harness unit is T-cycles (~4.19 MHz), so halve.
        self.total_ticks / 2
    }

    fn step(&mut self) -> bool {
        self.ctx.frame_ready = false;
        let ticks = unsafe { sys::GB_run(self.gb) } as u64;
        self.total_ticks = self.total_ticks.wrapping_add(ticks);
        self.ctx.frame_ready
    }

    fn render_into(&self, out: &mut Vec<u8>) {
        // Reinterpret the u32 pixels (encoded as [r,g,b,a] bytes) as a flat RGBA buffer, cropped/
        // padded to the Game Boy screen if SameBoy ever reports different dimensions.
        out.clear();
        out.reserve(SCREEN_WIDTH * SCREEN_HEIGHT * 4);
        // Fast path: SameBoy already reports the exact screen size, so the pixel buffer is a
        // contiguous RGBA frame we can copy wholesale (its bytes are little-endian [r,g,b,a]).
        if self.width == SCREEN_WIDTH && self.height == SCREEN_HEIGHT {
            let bytes = bytemuck_cast(&self.pixels);
            out.extend_from_slice(bytes);
            return;
        }
        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                let px = if x < self.width && y < self.height {
                    self.pixels[y * self.width + x]
                } else {
                    0xFF00_0000
                };
                out.extend_from_slice(&px.to_le_bytes());
            }
        }
    }
}

/// Reinterpret a `&[u32]` as the underlying `&[u8]` (little-endian byte order) without copying.
fn bytemuck_cast(pixels: &[u32]) -> &[u8] {
    // SAFETY: `u32` has no padding and any bit pattern is valid `u8`; the resulting slice covers
    // exactly the same bytes and borrows `pixels` for its lifetime.
    unsafe { std::slice::from_raw_parts(pixels.as_ptr() as *const u8, std::mem::size_of_val(pixels)) }
}

// SAFETY: `SameBoyEmulator` owns its `GB_gameboy_t` exclusively (one `GB_alloc`, freed in `Drop`).
// The raw pointer, the boxed `Ctx`, and the pixel buffer are only ever touched through `&mut self`,
// so a value is only used by whichever single thread currently owns it. The streaming runner moves
// the whole value to a worker thread and never aliases it; SameBoy itself is not thread-safe, but
// confining one instance to one thread at a time is sound.
unsafe impl Send for SameBoyEmulator {}

impl Drop for SameBoyEmulator {
    fn drop(&mut self) {
        unsafe {
            sys::GB_dealloc(self.gb);
        }
    }
}
