use crate::emulator::{Button, FrameEmulator};
use citrine_gb::console::{ConsoleKind, VideoSpec};
use mgba_sys as sys;
use std::ptr::{null, null_mut};

pub struct MgbaEmulator {
    emu: *mut sys::CitrineMgba,
    keys: u32,
    frames: u32,
    video: VideoSpec,
}

fn key(button: Button) -> u32 {
    match button {
        Button::A => sys::KEY_A,
        Button::B => sys::KEY_B,
        Button::Select => sys::KEY_SELECT,
        Button::Start => sys::KEY_START,
        Button::Right => sys::KEY_RIGHT,
        Button::Left => sys::KEY_LEFT,
        Button::Up => sys::KEY_UP,
        Button::Down => sys::KEY_DOWN,
    }
}

impl MgbaEmulator {
    pub fn new() -> Self {
        Self {
            emu: null_mut(),
            keys: 0,
            frames: 0,
            video: ConsoleKind::Gba.video(),
        }
    }

    fn release(&mut self) {
        if !self.emu.is_null() {
            unsafe { sys::citrine_mgba_free(self.emu) };
            self.emu = null_mut();
        }
    }
}

impl Default for MgbaEmulator {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameEmulator for MgbaEmulator {
    fn name(&self) -> &str {
        "mgba"
    }

    fn load(
        &mut self,
        rom: &[u8],
        boot_rom: Option<&[u8]>,
        console: ConsoleKind,
    ) -> anyhow::Result<VideoSpec> {
        anyhow::ensure!(
            console == ConsoleKind::Gba,
            "mgba runs only GBA games in the lab, not {console:?}"
        );
        self.release();
        let (bios, bios_size) = boot_rom.map_or((null(), 0), |b| (b.as_ptr().cast(), b.len()));
        self.emu =
            unsafe { sys::citrine_mgba_new(rom.as_ptr().cast(), rom.len(), bios, bios_size) };
        anyhow::ensure!(!self.emu.is_null(), "mgba could not load the ROM");
        let video = unsafe {
            VideoSpec {
                width: sys::citrine_mgba_width(self.emu) as usize,
                height: sys::citrine_mgba_height(self.emu) as usize,
            }
        };
        anyhow::ensure!(
            video == console.video(),
            "mgba reports a {video:?} screen, the GBA has {:?}",
            console.video()
        );
        self.video = video;
        self.keys = 0;
        self.frames = unsafe { sys::citrine_mgba_frame_counter(self.emu) };
        Ok(video)
    }

    fn set_button(&mut self, button: Button, pressed: bool) {
        if pressed {
            self.keys |= key(button);
        } else {
            self.keys &= !key(button);
        }
        unsafe { sys::citrine_mgba_set_keys(self.emu, self.keys) };
    }

    fn total_cycles(&self) -> u64 {
        unsafe { sys::citrine_mgba_cycles(self.emu) }
    }

    fn step(&mut self) -> bool {
        let frames = unsafe { sys::citrine_mgba_run_loop(self.emu) };
        let completed = frames != self.frames;
        self.frames = frames;
        completed
    }

    fn render_into(&self, out: &mut Vec<u8>) {
        let VideoSpec { width, height } = self.video;
        let pixels = unsafe {
            std::slice::from_raw_parts(sys::citrine_mgba_pixels(self.emu), width * height)
        };
        out.clear();
        out.reserve(width * height * 4);
        for px in pixels {
            let [r, g, b, _] = px.to_le_bytes();
            out.extend_from_slice(&[r, g, b, 0xFF]);
        }
    }
}

unsafe impl Send for MgbaEmulator {}

impl Drop for MgbaEmulator {
    fn drop(&mut self) {
        self.release();
    }
}
