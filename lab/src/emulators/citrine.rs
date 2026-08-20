use crate::emulator::{Button, FrameEmulator};
use citrine_gb::gb::ppu::types::theme::DmgTheme;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::rom::Rom;

pub struct CitrineEmulator {
    gb: GameBoy,
    model: GbModel,
}

impl CitrineEmulator {
    pub fn new() -> Self {
        Self {
            gb: GameBoy::new_empty(GbModel::Dmg),
            model: GbModel::Dmg,
        }
    }
}

impl Default for CitrineEmulator {
    fn default() -> Self {
        Self::new()
    }
}

impl CitrineEmulator {
    /// Mirrors `GameBoy::run_frame`. Without the flush `blip_buf`'s clock overflows after a few
    /// hundred frames, and the unread samples grow unbounded.
    fn finish_frame(&mut self) {
        self.gb.apu.flush_audio();
        self.gb.apu.audio_buffer.clear();
    }
}

impl FrameEmulator for CitrineEmulator {
    fn name(&self) -> &str {
        "citrine"
    }

    fn load(&mut self, rom: &[u8], boot_rom: Option<&[u8]>, model: GbModel) -> anyhow::Result<()> {
        self.model = model;
        self.gb = GameBoy::new_empty(model);
        if let Some(boot) = boot_rom {
            self.gb.load_boot_rom(boot);
        }
        let rom = Rom::new(rom);
        self.gb
            .load_rom(&rom)
            .map_err(|e| anyhow::anyhow!("citrine failed to load rom: {e:?}"))?;
        // Evenly-spaced greys, matching SameBoy's forced grey palette after normalization.
        self.gb.ppu.dmg_theme = DmgTheme::GreyScale;
        Ok(())
    }

    fn set_button(&mut self, button: Button, pressed: bool) {
        let state = button.to_joypad_state();
        if pressed {
            self.gb.press_button(state);
        } else {
            self.gb.release_button(state);
        }
    }

    fn total_cycles(&self) -> u64 {
        // M-cycles -> T-cycles.
        self.gb.debugger.total_cycles as u64 * 4
    }

    fn step(&mut self) -> bool {
        self.gb.step();
        let frame_cycles = self.model.frame_cycles();

        if self.gb.ppu.frame_ready {
            self.gb.ppu.frame_ready = false;
            if self.gb.cycle_counter >= frame_cycles {
                self.gb.cycle_counter -= frame_cycles;
            }
            self.finish_frame();
            return true;
        }

        // Emit an artificial frame while the LCD is off, matching SameBoy's LCD-off vblanks.
        if !self.gb.ppu.lcdc.lcd_enabled && self.gb.cycle_counter >= frame_cycles {
            self.gb.cycle_counter -= frame_cycles;
            self.finish_frame();
            return true;
        }

        false
    }

    fn render_into(&self, out: &mut Vec<u8>) {
        let src = self.gb.frame().as_slice();
        out.clear();
        out.extend_from_slice(src);
    }
}
