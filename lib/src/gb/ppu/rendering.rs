use crate::gb::ic::{ICInterface, Interrupt};
use crate::gb::ppu::mode::PpuMode;
use crate::gb::ppu::Ppu;

impl Ppu {
    pub fn dot(&mut self, ic: &mut impl ICInterface) {
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

    fn check_lyc(&mut self, ic: &mut impl ICInterface) {
        let prev = self.stat.lyc_equals_ly;
        self.stat.lyc_equals_ly = self.ly == self.lyc;

        if !prev && self.stat.lyc_equals_ly && self.stat.lyc_interrupt {
            ic.request_interrupt(Interrupt::Lcd)
        }
    }
}
