use super::Bus;
use crate::gba::dma::Timing;
use crate::gba::ppu::Memory;

const VRAM_WINDOW: u32 = 0x1_FFFF;
const VRAM_HOLE: std::ops::Range<u32> = 0x1_8000..0x1_C000;

impl Bus {
    pub(super) fn run_video(&mut self, start: u32, cycles: u32) {
        if self.ppu.skip(cycles) {
            return;
        }
        let memory = Memory {
            vram: &self.vram,
            palette: &self.palette,
            oam: &self.oam,
        };
        let signals = self.ppu.step(cycles, start, memory, &mut self.interrupts);
        let mut scheduled = false;
        if let Some(at) = signals.vblank {
            scheduled |= self.dma.request(Timing::VBlank, at);
        }
        if let Some(at) = signals.hblank {
            scheduled |= self.dma.request(Timing::HBlank, at);
        }
        if let Some(at) = signals.capture {
            scheduled |= self.dma.request_capture(at);
        }
        if signals.capture_end {
            self.dma.stop_capture();
        }
        if signals.capture_arm {
            scheduled |= self.dma.arm_capture();
        }
        if scheduled {
            self.dma_check = 0;
        }
    }

    pub(super) fn in_vram_hole(&self, addr: u32) -> bool {
        self.ppu.bitmap_mode() && VRAM_HOLE.contains(&(addr & VRAM_WINDOW))
    }
}
