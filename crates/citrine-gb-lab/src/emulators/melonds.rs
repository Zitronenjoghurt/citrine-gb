use crate::emulator::{Button, FrameEmulator};
use citrine_gb::console::{ConsoleKind, Screen, VideoSpec};
use melonds_sys as sys;
use std::ptr::null_mut;

const DS_SCREENS: [Screen; 2] = [
    Screen::whole(sys::SCREEN_WIDTH, sys::SCREEN_HEIGHT),
    Screen {
        x: 0,
        y: sys::SCREEN_HEIGHT,
        width: sys::SCREEN_WIDTH,
        height: sys::SCREEN_HEIGHT,
        touch: true,
    },
];

pub const DS_VIDEO: VideoSpec = VideoSpec::of(&DS_SCREENS);

pub const DS_CYCLES_PER_FRAME: u64 = 560_190;

pub const DS_SAMPLE_RATE: u32 = 48_000;

#[derive(Clone, Copy)]
pub struct DsBios<'a> {
    pub arm9: &'a [u8],
    pub arm7: &'a [u8],
}

impl DsBios<'static> {
    pub const FREE: Self = Self {
        arm9: include_bytes!("../../../citrine-gb/assets/nds-bios/freebios9.bin"),
        arm7: include_bytes!("../../../citrine-gb/assets/nds-bios/freebios7.bin"),
    };
}

pub struct MelonDsEmulator {
    emu: *mut sys::CitrineMelonds,
    keys: u32,
    sample_rate: u32,
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

fn rgba(px: u32) -> [u8; 4] {
    let [b, g, r, _] = px.to_le_bytes();
    [r, g, b, 0xFF]
}

impl MelonDsEmulator {
    pub fn new() -> Self {
        Self {
            emu: null_mut(),
            keys: 0,
            sample_rate: DS_SAMPLE_RATE,
        }
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.sample_rate = rate;
    }

    pub fn load_nds(&mut self, rom: &[u8], bios: DsBios) -> anyhow::Result<VideoSpec> {
        anyhow::ensure!(
            bios.arm9.len() == sys::ARM9_BIOS_SIZE && bios.arm7.len() == sys::ARM7_BIOS_SIZE,
            "a DS BIOS is {} bytes for the ARM9 and {} for the ARM7, not {} and {}",
            sys::ARM9_BIOS_SIZE,
            sys::ARM7_BIOS_SIZE,
            bios.arm9.len(),
            bios.arm7.len()
        );
        self.release();
        self.emu = unsafe {
            sys::citrine_melonds_new(
                rom.as_ptr().cast(),
                rom.len(),
                bios.arm9.as_ptr().cast(),
                bios.arm9.len(),
                bios.arm7.as_ptr().cast(),
                bios.arm7.len(),
                self.sample_rate,
            )
        };
        anyhow::ensure!(!self.emu.is_null(), "melonds could not load the ROM");
        self.keys = 0;
        Ok(DS_VIDEO)
    }

    pub fn set_keys(&mut self, keys: u32) {
        self.keys = keys;
        unsafe { sys::citrine_melonds_set_keys(self.emu, keys) };
    }

    pub fn touch(&mut self, point: Option<(u32, u32)>) {
        let (x, y) = point.unwrap_or_default();
        unsafe { sys::citrine_melonds_touch(self.emu, x, y, u32::from(point.is_some())) };
    }

    pub fn scene3d(&mut self, out: &mut crate::scene3d::Scene) {
        unsafe { sys::citrine_melonds_scene3d(self.emu, out.raw_mut()) };
    }

    pub fn drain_audio(&mut self, out: &mut Vec<i16>) {
        let mut chunk = [0i16; 2 * 1024];
        loop {
            let frames = unsafe {
                sys::citrine_melonds_read_audio(self.emu, chunk.as_mut_ptr(), chunk.len() / 2)
            };
            out.extend_from_slice(&chunk[..frames * 2]);
            if frames < chunk.len() / 2 {
                break;
            }
        }
    }

    fn release(&mut self) {
        if !self.emu.is_null() {
            unsafe { sys::citrine_melonds_free(self.emu) };
            self.emu = null_mut();
        }
    }
}

impl Default for MelonDsEmulator {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameEmulator for MelonDsEmulator {
    fn name(&self) -> &str {
        "melonds"
    }

    fn load(
        &mut self,
        _rom: &[u8],
        _boot_rom: Option<&[u8]>,
        console: ConsoleKind,
    ) -> anyhow::Result<VideoSpec> {
        anyhow::bail!("melonds runs only DS games in the lab, not {console:?}")
    }

    fn set_button(&mut self, button: Button, pressed: bool) {
        if pressed {
            self.keys |= key(button);
        } else {
            self.keys &= !key(button);
        }
        unsafe { sys::citrine_melonds_set_keys(self.emu, self.keys) };
    }

    fn total_cycles(&self) -> u64 {
        unsafe { sys::citrine_melonds_cycles(self.emu) }
    }

    fn step(&mut self) -> bool {
        unsafe { sys::citrine_melonds_run_frame(self.emu) };
        true
    }

    fn render_into(&self, out: &mut Vec<u8>) {
        let screen = sys::SCREEN_WIDTH * sys::SCREEN_HEIGHT;
        out.clear();
        out.reserve(screen * 2 * 4);
        for index in 0..2 {
            let pixels = unsafe {
                std::slice::from_raw_parts(sys::citrine_melonds_screen(self.emu, index), screen)
            };
            for &px in pixels {
                out.extend_from_slice(&rgba(px));
            }
        }
    }
}

unsafe impl Send for MelonDsEmulator {}

impl Drop for MelonDsEmulator {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_reads_melonds_xrgb_pixels() {
        assert_eq!(rgba(0xFF_FC_80_04), [0xFC, 0x80, 0x04, 0xFF]);
    }

    #[test]
    fn freebios_has_the_ds_bios_sizes() {
        assert_eq!(DsBios::FREE.arm9.len(), sys::ARM9_BIOS_SIZE);
        assert_eq!(DsBios::FREE.arm7.len(), sys::ARM7_BIOS_SIZE);
    }
}
