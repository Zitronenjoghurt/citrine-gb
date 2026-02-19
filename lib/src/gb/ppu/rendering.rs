use crate::gb::ppu::mode::PpuMode;
use crate::gb::ppu::Ppu;

impl Ppu {
    pub fn dot(&mut self) {
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
                        // ToDo: STAT interrupt
                    }
                }
            }
            PpuMode::HBlank => {
                if self.dot_counter == 456 {
                    self.dot_counter = 0;
                    self.ly += 1;
                    self.check_lyc();

                    if self.ly == 144 {
                        self.stat.ppu_mode = PpuMode::VBlank;
                        // ToDo: VBLANK interrupt
                        if self.stat.mode1_interrupt {
                            // ToDo: STAT interrupt
                        }
                    } else {
                        self.stat.ppu_mode = PpuMode::OamScan;
                        if self.stat.mode2_interrupt {
                            // ToDo: STAT interrupt
                        }
                    }

                    return;
                }
            }
            PpuMode::VBlank => {
                if self.dot_counter == 456 {
                    self.dot_counter = 0;
                    self.ly += 1;
                    self.check_lyc();

                    if self.ly == 154 {
                        self.ly = 0;
                        self.check_lyc();
                        self.stat.ppu_mode = PpuMode::OamScan;
                    }

                    return;
                }
            }
        }

        self.dot_counter += 1;
    }

    fn check_lyc(&mut self) {
        let prev = self.stat.lyc_equals_ly;
        self.stat.lyc_equals_ly = self.ly == self.lyc;

        if !prev && self.stat.lyc_equals_ly && self.stat.lyc_interrupt {
            // ToDo: STAT interrupt
        }
    }
}
