use crate::gb::ic::{ICInterface, Interrupt};
use crate::gb::ppu::color::RGBA;
use crate::gb::ppu::mode::PpuMode;
use crate::gb::ppu::Ppu;

/// Using the Game Boy Pocket color scheme
/// https://en.wikipedia.org/wiki/List_of_video_game_console_palettes
const COLOR_SCHEME: [[u8; 4]; 4] = [
    [0xC5, 0xCA, 0xA4, 0xFF],
    [0x8C, 0x92, 0x6B, 0xFF],
    [0x4A, 0x51, 0x38, 0xFF],
    [0x18, 0x18, 0x18, 0xFF],
];

impl Ppu {
    pub fn dot(&mut self, ic: &mut impl ICInterface, oam_dma: bool) {
        if !self.lcdc.lcd_enabled {
            return;
        }

        match self.stat.ppu_mode {
            PpuMode::OamScan => {
                if self.dot_counter == 80 {
                    self.stat.ppu_mode = PpuMode::Drawing;
                }
            }
            PpuMode::Drawing => {
                // ToDo: Account for Mode 3 penalties => https://gbdev.io/pandocs/Rendering.html#mode-3-length
                if self.dot_counter == 80 + 172 {
                    if self.lcdc.bg_window_enable {
                        for x in 0..160_u8 {
                            let color_index = self.get_current_bg_color_index(x);
                            let shade = self.apply_bg_palette(color_index);
                            let fb_index = self.ly as usize * 160 + x as usize;
                            self.frame
                                .set(fb_index, RGBA::from(COLOR_SCHEME[shade as usize]));
                        }
                    }
                    self.stat.ppu_mode = PpuMode::HBlank;
                    if self.stat.mode0_interrupt {
                        ic.request_interrupt(Interrupt::Lcd);
                    }
                }
            }
            PpuMode::HBlank => {
                if self.dot_counter == 456 {
                    self.dot_counter = 0;
                    self.ly += 1;
                    self.check_lyc(ic);

                    if self.ly == 144 {
                        self.stat.ppu_mode = PpuMode::VBlank;
                        ic.request_interrupt(Interrupt::Vblank);
                        if self.stat.mode1_interrupt {
                            ic.request_interrupt(Interrupt::Lcd);
                        }
                    } else {
                        self.stat.ppu_mode = PpuMode::OamScan;
                        if self.stat.mode2_interrupt {
                            ic.request_interrupt(Interrupt::Lcd);
                        }
                    }

                    return;
                }
            }
            PpuMode::VBlank => {
                if self.dot_counter == 456 {
                    self.dot_counter = 0;
                    self.ly += 1;
                    self.check_lyc(ic);

                    if self.ly == 154 {
                        self.ly = 0;
                        self.check_lyc(ic);
                        self.stat.ppu_mode = PpuMode::OamScan;
                    }

                    return;
                }
            }
        }

        self.dot_counter += 1;
    }

    pub fn check_lyc(&mut self, ic: &mut impl ICInterface) {
        let prev = self.stat.lyc_equals_ly;
        self.stat.lyc_equals_ly = self.ly == self.lyc;

        if !prev && self.stat.lyc_equals_ly && self.stat.lyc_interrupt {
            ic.request_interrupt(Interrupt::Lcd)
        }
    }
}
