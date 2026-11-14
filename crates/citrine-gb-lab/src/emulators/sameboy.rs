#![expect(
    clippy::disallowed_types,
    reason = "the lab runs SameBoy outside the app, and its reset must not run on two threads at once"
)]

use crate::emulator::{Button, FrameEmulator};
use citrine_gb::console::{ConsoleKind, VideoSpec};
use citrine_gb::gb::GbModel;
use citrine_gb::gb::ppu::compat::CompatPalette;
use citrine_gb::gb::ppu::types::color::RGBA;
use sameboy_sys as sys;
use std::ffi::c_void;
use std::sync::Mutex;

struct Ctx {
    frame_ready: bool,
}

pub struct SameBoyEmulator {
    gb: *mut sys::GB_gameboy_t,
    ctx: Box<Ctx>,
    pixels: Vec<u32>,
    width: usize,
    height: usize,
    total_ticks: u64,
    model: GbModel,
    video: VideoSpec,
}

/// Packs to `[r, g, b, a]` in memory on little-endian targets, so pixels reinterpret as RGBA.
extern "C" fn rgb_encode(_gb: *mut sys::GB_gameboy_t, r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | (0xFFu32 << 24)
}

extern "C" fn vblank(gb: *mut sys::GB_gameboy_t, _ty: sys::GB_vblank_type_t) {
    // SAFETY: user data points at the owning `Box<Ctx>` for the instance's lifetime.
    unsafe {
        let ctx = sys::GB_get_user_data(gb) as *mut Ctx;
        if !ctx.is_null() {
            (*ctx).frame_ready = true;
        }
    }
}

const RAM_SEED: u64 = 0x0C17_A17E;
const IDLE_BOOT_ROM: [u8; 0x900] = [0; 0x900];
const IDLE_BOOT_PC_LIMIT: u16 = 0xF0;
const BOOT_LCD_ON_DMG: u64 = 267_576;
const BOOT_LCD_ON_CGB: u64 = 659_556;
const BOOT_HANDOFF_DMG: u64 = 5_881_108;
const BOOT_HANDOFF_CGB: u64 = 13_085_724;
const BOOT_HANDOFF_COMPAT: u64 = 13_156_368;
const BOOT_TRADEMARK: [u8; 8] = [0x3C, 0x42, 0xB9, 0xA5, 0xB9, 0xA5, 0x42, 0x3C];

static RESET_LOCK: Mutex<()> = Mutex::new(());

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
                video: ConsoleKind::from(model).video(),
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
            // Matches Citrine's `DmgTheme::GreyScale` after normalization.
            sys::GB_set_color_correction_mode(self.gb, sys::GB_COLOR_CORRECTION_DISABLED);
            sys::GB_set_palette(self.gb, &raw const sys::GB_PALETTE_GREY);
            // Without turbo the core `nanosleep`s to real-time 60 fps.
            sys::GB_set_turbo_mode(self.gb, true, true);
            self.resize_pixel_buffer();
        }
    }

    unsafe fn resize_pixel_buffer(&mut self) {
        unsafe {
            self.width = sys::GB_get_screen_width(self.gb) as usize;
            self.height = sys::GB_get_screen_height(self.gb) as usize;
            self.pixels.clear();
            self.pixels.resize(self.width * self.height, 0xFF00_0000);
            sys::GB_set_pixels_output(self.gb, self.pixels.as_mut_ptr());
        }
    }

    fn skip_boot(&mut self, rom: &[u8]) {
        let byte = |addr: usize| rom.get(addr).copied().unwrap_or(0);
        let cgb_game = byte(0x143) & 0x80 != 0;
        let compat = self.model == GbModel::Cgb && !cgb_game;
        self.write_boot_logo(rom, self.model == GbModel::Dmg);
        if self.model == GbModel::Cgb {
            self.poke(0xFF00, 0x30);
            for addr in (0xFF31..0xFF40).step_by(2) {
                self.poke(addr, 0xFF);
            }
            self.poke(0xFF70, 0x00);
            if compat {
                self.write_compat_palettes(rom);
            } else {
                self.poke(0xFF68, 0x80);
                for _ in 0..8 {
                    for byte in [0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF, 0x7F] {
                        self.poke(0xFF69, byte);
                    }
                }
            }
        }
        let (lcd_on, handoff) = match self.model {
            GbModel::Dmg => (BOOT_LCD_ON_DMG, BOOT_HANDOFF_DMG),
            GbModel::Cgb if compat => (BOOT_LCD_ON_CGB, BOOT_HANDOFF_COMPAT),
            GbModel::Cgb => (BOOT_LCD_ON_CGB, BOOT_HANDOFF_CGB),
        };
        self.idle_until(lcd_on);
        self.poke(0xFF40, 0x91);
        self.idle_until(handoff);
        for (addr, value) in [
            (0xFF26, 0x80),
            (0xFF11, 0x80),
            (0xFF12, 0x08),
            (0xFF13, 0xC1),
            (0xFF14, 0x87),
            (0xFF12, 0xF3),
            (0xFF24, 0x77),
            (0xFF25, 0xF3),
            (0xFF47, 0xFC),
        ] {
            self.poke(addr, value);
        }

        let regs = unsafe { &mut *sys::GB_get_registers(self.gb) };
        match self.model {
            GbModel::Dmg => {
                let f: u16 = if byte(0x14D) == 0 { 0x80 } else { 0xB0 };
                regs.af = 0x0100 | f;
                regs.bc = 0x0013;
                regs.de = 0x00D8;
                regs.hl = 0x014D;
            }
            GbModel::Cgb if compat => {
                let nintendo = byte(0x14B) == 0x01
                    || (byte(0x14B) == 0x33 && byte(0x144) == b'0' && byte(0x145) == b'1');
                let b = if nintendo {
                    (0x134..=0x143).fold(0u8, |sum, addr| sum.wrapping_add(byte(addr)))
                } else {
                    0x00
                };
                regs.af = 0x1180;
                regs.bc = u16::from(b) << 8;
                regs.de = 0x0008;
                regs.hl = if b == 0x43 || b == 0x58 {
                    0x991A
                } else {
                    0x007C
                };
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
        if compat {
            self.poke(0xFF4C, 0x04);
            self.poke(0xFF6C, 0x01);
        }
        self.poke(0xFF50, 0x01);
    }

    fn idle_until(&mut self, cycle: u64) {
        while self.total_cycles() < cycle {
            let regs = unsafe { &mut *sys::GB_get_registers(self.gb) };
            if regs.pc >= IDLE_BOOT_PC_LIMIT {
                regs.pc = 0;
            }
            let ticks = unsafe { sys::GB_run(self.gb) } as u64;
            self.total_ticks += ticks;
        }
    }

    fn write_boot_logo(&mut self, rom: &[u8], tilemap: bool) {
        let doubled = |nibble: u8| {
            (0..4).fold(0u8, |acc, bit| {
                let set = (nibble >> (3 - bit)) & 1;
                acc << 2 | set << 1 | set
            })
        };
        let rows: Vec<u8> = (0x104..0x134)
            .map(|addr| rom.get(addr).copied().unwrap_or(0))
            .flat_map(|byte| {
                let high = doubled(byte >> 4);
                let low = doubled(byte & 0x0F);
                [high, high, low, low]
            })
            .chain(BOOT_TRADEMARK)
            .collect();
        for (i, row) in rows.into_iter().enumerate() {
            self.poke(0x8010 + i as u16 * 2, row);
        }
        if tilemap {
            self.poke(0x9910, 0x19);
            for tile in 1..=12u8 {
                self.poke(0x9903 + u16::from(tile), tile);
                self.poke(0x9923 + u16::from(tile), tile + 12);
            }
        }
    }

    fn write_compat_palettes(&mut self, rom: &[u8]) {
        let [bg, obj0, obj1] = CompatPalette::Auto.colors(rom);
        let rgb555 = |c: RGBA| {
            u16::from(c.r() >> 3) | u16::from(c.g() >> 3) << 5 | u16::from(c.b() >> 3) << 10
        };
        for (index, data, palettes) in [
            (0xFF68, 0xFF69, vec![bg]),
            (0xFF6A, 0xFF6B, vec![obj0, obj1]),
        ] {
            self.poke(index, 0x80);
            for color in palettes.into_iter().flatten() {
                for byte in rgb555(color).to_le_bytes() {
                    self.poke(data, byte);
                }
            }
        }
    }

    fn poke(&mut self, addr: u16, value: u8) {
        unsafe { sys::GB_write_memory(self.gb, addr, value) };
    }
}

impl SameBoyEmulator {
    pub fn peek(&self, addr: u16) -> u8 {
        unsafe { sys::GB_safe_read_memory(self.gb, addr) }
    }

    pub fn registers(&self) -> sys::GB_registers_t {
        unsafe { *sys::GB_get_registers(self.gb) }
    }

    pub fn save_battery(&mut self) -> Vec<u8> {
        unsafe {
            let size = sys::GB_save_battery_size(self.gb).max(0) as usize;
            let mut buffer = vec![0u8; size];
            sys::GB_save_battery_to_buffer(self.gb, buffer.as_mut_ptr(), buffer.len());
            buffer
        }
    }

    pub fn load_battery(&mut self, data: &[u8]) {
        unsafe {
            sys::GB_load_battery_from_buffer(self.gb, data.as_ptr(), data.len());
        }
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

    fn load(
        &mut self,
        rom: &[u8],
        boot_rom: Option<&[u8]>,
        console: ConsoleKind,
    ) -> anyhow::Result<VideoSpec> {
        let model = GbModel::try_from(console)
            .map_err(|e| anyhow::anyhow!("sameboy cannot run {console:?}: {e:?}"))?;
        self.video = console.video();
        if model != self.model {
            self.model = model;
            unsafe {
                sys::GB_init(self.gb, sys_model(model));
            }
            // Re-init clears callbacks and user data.
            self.wire_callbacks();
        }

        {
            let _guard = RESET_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            unsafe {
                let boot = boot_rom.unwrap_or(&IDLE_BOOT_ROM);
                sys::GB_load_boot_rom_from_buffer(self.gb, boot.as_ptr(), boot.len());
                sys::GB_load_rom_from_buffer(self.gb, rom.as_ptr(), rom.len());
                sys::GB_random_seed(RAM_SEED);
                sys::GB_reset(self.gb);
                self.resize_pixel_buffer();
            }
        }
        self.total_ticks = 0;
        if boot_rom.is_none() {
            self.skip_boot(rom);
        }

        self.ctx.frame_ready = false;
        self.total_ticks = 0;
        Ok(self.video)
    }

    fn set_button(&mut self, button: Button, pressed: bool) {
        unsafe {
            sys::GB_set_key_state(self.gb, map_button(button), pressed);
        }
    }

    fn total_cycles(&self) -> u64 {
        // SameBoy counts 8 MiHz ticks; the harness unit is T-cycles.
        self.total_ticks / 2
    }

    fn step(&mut self) -> bool {
        self.ctx.frame_ready = false;
        let ticks = unsafe { sys::GB_run(self.gb) } as u64;
        self.total_ticks = self.total_ticks.wrapping_add(ticks);
        self.ctx.frame_ready
    }

    fn render_into(&self, out: &mut Vec<u8>) {
        out.clear();
        let VideoSpec { width, height, .. } = self.video;
        out.reserve(width * height * 4);
        if self.width == width && self.height == height {
            let bytes = bytemuck_cast(&self.pixels);
            out.extend_from_slice(bytes);
            return;
        }
        for y in 0..height {
            for x in 0..width {
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

fn bytemuck_cast(pixels: &[u32]) -> &[u8] {
    // SAFETY: any bit pattern of a padding-free `u32` is a valid `u8`, and the slice borrows
    // `pixels` for its lifetime.
    unsafe {
        std::slice::from_raw_parts(pixels.as_ptr() as *const u8, std::mem::size_of_val(pixels))
    }
}

// SAFETY: everything owned here is only reached through `&mut self`, so the value is only ever
// used by the one thread holding it. SameBoy is not thread-safe, but confining an instance to a
// single thread at a time is sound.
unsafe impl Send for SameBoyEmulator {}

impl Drop for SameBoyEmulator {
    fn drop(&mut self) {
        unsafe {
            sys::GB_dealloc(self.gb);
        }
    }
}
