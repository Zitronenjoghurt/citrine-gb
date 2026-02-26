use crate::gb::ic::{ICInterface, Interrupt};
use crate::gb::ppu::fetcher::PixelFetcher;
use crate::gb::ppu::fifo::PixelFifo;
use crate::gb::ppu::framebuffer::Framebuffer;
use crate::gb::ppu::lcdc::LCDC;
use crate::gb::ppu::mode::PpuMode;
use crate::gb::ppu::scanner::OamScanner;
use crate::gb::ppu::stat::STAT;
use crate::gb::GbModel;
use crate::{ReadMemory, WriteMemory};

mod color;
mod fetcher;
mod fifo;
pub mod framebuffer;
mod lcdc;
mod mode;
mod scanner;
mod sprite;
mod stat;
mod tile;

pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;
const VRAM_BANK_SIZE: usize = 0x2000; // 8KiB
const OAM_SIZE: usize = 160; // Bytes

pub struct Ppu {
    frame: Framebuffer,
    model: GbModel,
    pub fetcher: PixelFetcher,
    pub fifo: PixelFifo,
    pub scanner: OamScanner,
    /// How many dots are left in the current H or V blank period
    pub blank_timeout: u16,
    /// Dots in the current scanline
    pub line_dot_counter: u16,
    // Memory
    /// Video RAM (2 banks on CGB)
    vram: [[u8; VRAM_BANK_SIZE]; 2],
    /// Sprite attribute table
    oam: [u8; OAM_SIZE],
    /// LCD control
    pub lcdc: LCDC,
    /// LCD status
    pub stat: STAT,
    /// BG scroll Y
    pub scy: u8,
    /// BG scroll X
    pub scx: u8,
    /// Current scanline
    pub ly: u8,
    /// Scanline compare
    pub lyc: u8,
    /// BG palette (DMG)
    bgp: u8,
    /// OBJ palette 0 (DMG)
    obp0: u8,
    /// OBJ palette 1 (DMG)
    obp1: u8,
    /// Window Y position
    pub wy: u8,
    /// Window X position (+7)
    pub wx: u8,
    /// VRAM bank select (CGB)
    vbk: u8,
    /// BG palette index (CGB)
    bcps: u8,
    /// Internal BG palette RAM, accessed via BCPS
    bg_palette_ram: [u8; 64],
    /// OBJ palette index (CGB)
    ocps: u8,
    /// Internal OBJ palette RAM, accessed via OCPS
    obj_palette_ram: [u8; 64],
    /// OBJ priority mode (CGB)
    opri: u8,
    /// VRAM DMA source high (CGB)
    hdma1: u8,
    /// VRAM DMA source low (CGB)
    hdma2: u8,
    /// VRAM DMA dest high (CGB)
    hdma3: u8,
    /// VRAM DMA dest low (CGB)
    hdma4: u8,
    /// VRAM DMA length/mode/start (CGB)
    hdma5: u8,
}

impl Ppu {
    pub fn new(model: GbModel) -> Self {
        Self {
            frame: Framebuffer::new(),
            model,
            fetcher: PixelFetcher::default(),
            fifo: PixelFifo::default(),
            scanner: OamScanner::default(),
            blank_timeout: 456,
            line_dot_counter: 0,
            vram: [[0x00; VRAM_BANK_SIZE]; 2],
            oam: [0x00; OAM_SIZE],
            lcdc: 0x91.into(),
            stat: 0x85.into(),
            scy: 0x00,
            scx: 0x00,
            ly: 0x91,
            lyc: 0x00,
            bgp: 0xFC,
            obp0: 0x00,
            obp1: 0x00,
            wy: 0x00,
            wx: 0x00,
            vbk: 0xFE,
            bcps: 0x00,
            bg_palette_ram: [0x00; 64],
            ocps: 0x00,
            obj_palette_ram: [0x00; 64],
            opri: 0x00,
            hdma1: 0xFF,
            hdma2: 0xFF,
            hdma3: 0xFF,
            hdma4: 0xFF,
            hdma5: 0xFF,
        }
    }

    pub fn cycle(&mut self, ic: &mut impl ICInterface, oam_dma: bool) {
        if self.model.is_cgb() {
            self.dot(ic, oam_dma);
            self.dot(ic, oam_dma);
        } else {
            self.dot(ic, oam_dma);
            self.dot(ic, oam_dma);
            self.dot(ic, oam_dma);
            self.dot(ic, oam_dma);
        }
    }

    // ToDo: Conflicts on oam dma? Check relevance and implementation
    pub fn dot(&mut self, ic: &mut impl ICInterface, oam_dma: bool) {
        self.check_window_condition();

        match self.stat.ppu_mode {
            PpuMode::OamScan => {
                self.line_dot_counter += 1;
                let done = self.dot_oam_scan();
                if done {
                    self.fifo.start_scanline(self.scx);
                    self.stat.ppu_mode = PpuMode::Drawing;
                }
            }
            PpuMode::Drawing => {
                self.line_dot_counter += 1;
                self.dot_fetcher();
                let fifo_done = self.dot_fifo();
                if fifo_done {
                    if self.fetcher.window_mode {
                        self.fetcher.wl += 1;
                    }
                    self.fetcher.reset_scanline();
                    self.blank_timeout = 456 - self.line_dot_counter;
                    self.stat.ppu_mode = PpuMode::HBlank;
                    if self.stat.mode0_interrupt {
                        ic.request_interrupt(Interrupt::Lcd);
                    }
                }
            }
            PpuMode::HBlank => {
                self.blank_timeout -= 1;
                if self.blank_timeout == 0 {
                    self.ly += 1;
                    self.line_dot_counter = 0;
                    self.check_lyc(ic);
                    if self.ly == 144 {
                        self.blank_timeout = 456;
                        self.stat.ppu_mode = PpuMode::VBlank;
                        ic.request_interrupt(Interrupt::VBlank);
                        if self.stat.mode1_interrupt {
                            ic.request_interrupt(Interrupt::Lcd);
                        }
                    } else {
                        self.stat.ppu_mode = PpuMode::OamScan;
                        if self.stat.mode2_interrupt {
                            ic.request_interrupt(Interrupt::Lcd);
                        }
                    }
                }
            }
            PpuMode::VBlank => {
                self.blank_timeout -= 1;
                if self.blank_timeout == 0 {
                    self.ly += 1;
                    self.check_lyc(ic);
                    if self.ly == 154 {
                        self.ly = 0;
                        self.check_lyc(ic);
                        self.fetcher.reset_frame();
                        self.stat.ppu_mode = PpuMode::OamScan;
                        if self.stat.mode2_interrupt {
                            ic.request_interrupt(Interrupt::Lcd);
                        }
                    } else {
                        self.blank_timeout = 456;
                    }
                }
            }
        }
    }

    pub fn check_lyc(&mut self, ic: &mut impl ICInterface) {
        let prev = self.stat.lyc_equals_ly;
        self.stat.lyc_equals_ly = self.ly == self.lyc;

        if !prev && self.stat.lyc_equals_ly && self.stat.lyc_interrupt {
            ic.request_interrupt(Interrupt::Lcd)
        }
    }

    pub fn check_window_condition(&mut self) {
        if self.wy == self.ly {
            self.fetcher.wy_triggered = true;
        }
    }

    pub fn cpu_conflicts(&self, addr: u16) -> bool {
        false

        // ToDo: Improve accuracy, test viability?? => currently causing conflicts where it shouldn't
        // => run cpu_instrs test => part of the text is missing
        //if !self.lcdc.lcd_enabled
        //    || self.stat.ppu_mode == PpuMode::HBlank
        //    || self.stat.ppu_mode == PpuMode::VBlank
        //{
        //    return false;
        //}

        //match addr {
        //    // VRAM blocked during mode 3
        //    0x8000..=0x9FFF => self.stat.ppu_mode == PpuMode::Drawing,
        //    // OAM blocked during mode 2 and 3
        //    0xFE00..=0xFE9F => matches!(self.stat.ppu_mode, PpuMode::OamScan | PpuMode::Drawing),
        //    // CGB palettes blocked during mode 3
        //    0xFF69 | 0xFF6B => self.model.is_cgb() && self.stat.ppu_mode == PpuMode::Drawing,
        //    _ => false,
        //}
    }

    pub fn frame(&self) -> &Framebuffer {
        &self.frame
    }

    pub fn soft_reset(&mut self) {
        *self = Self::new(self.model);
    }

    pub fn clear_frame(&mut self) {
        self.frame.clear_with_test_pattern();
    }
}

impl ReadMemory for Ppu {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => {
                if self.model.is_cgb() {
                    self.vram[(self.vbk & 1) as usize][(addr - 0x8000) as usize]
                } else {
                    self.vram[0][(addr - 0x8000) as usize]
                }
            }
            0xFE00..=0xFE9F => self.oam[(addr - 0xFE00) as usize],
            0xFF40 => self.lcdc.into(),
            0xFF41 => self.stat.into(),
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            0xFF4F => {
                if self.model.is_cgb() {
                    self.vbk | 0xFE // only bit 0 readable
                } else {
                    0xFF
                }
            }
            0xFF51 => {
                if self.model.is_cgb() {
                    self.hdma1
                } else {
                    0xFF
                }
            }
            0xFF52 => {
                if self.model.is_cgb() {
                    self.hdma2
                } else {
                    0xFF
                }
            }
            0xFF53 => {
                if self.model.is_cgb() {
                    self.hdma3
                } else {
                    0xFF
                }
            }
            0xFF54 => {
                if self.model.is_cgb() {
                    self.hdma4
                } else {
                    0xFF
                }
            }
            0xFF55 => {
                if self.model.is_cgb() {
                    self.hdma5
                } else {
                    0xFF
                }
            }
            0xFF68 => {
                if self.model.is_cgb() {
                    self.bcps
                } else {
                    0xFF
                }
            }
            0xFF69 => {
                if self.model.is_cgb() {
                    self.bg_palette_ram[(self.bcps & 0x3F) as usize]
                } else {
                    0xFF
                }
            }
            0xFF6A => {
                if self.model.is_cgb() {
                    self.ocps
                } else {
                    0xFF
                }
            }
            0xFF6B => {
                if self.model.is_cgb() {
                    self.obj_palette_ram[(self.ocps & 0x3F) as usize]
                } else {
                    0xFF
                }
            }
            0xFF6C => {
                if self.model.is_cgb() {
                    self.opri
                } else {
                    0xFF
                }
            }
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Ppu {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => {
                if self.model.is_cgb() {
                    self.vram[(self.vbk & 1) as usize][(addr - 0x8000) as usize] = value
                } else {
                    self.vram[0][(addr - 0x8000) as usize] = value;
                }
            }
            0xFE00..=0xFE9F => self.oam[(addr - 0xFE00) as usize] = value,
            0xFF40 => {
                // ToDo: Check accuracy of ppu reset when lcd is turned off => currently causing lines to never be drawn
                // => run dmg-acid2 test => first line is missing
                //let lcd_on = self.lcdc.lcd_enabled;
                self.lcdc = value.into();
                //if lcd_on && !self.lcdc.lcd_enabled {
                //    self.ly = 0;
                //    self.wl = 0;
                //    self.dot_counter = 0;
                //    self.stat.ppu_mode = PpuMode::HBlank;
                //}
            }
            0xFF41 => self.stat = ((u8::from(self.stat) & 0x87) | (value & 0x78)).into(), // bits 0-2 read-only, bit 7 unused
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {} // LY is read-only
            0xFF45 => self.lyc = value,
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
            0xFF4A => self.wy = value,
            0xFF4B => self.wx = value,
            0xFF4F => {
                if self.model.is_cgb() {
                    self.vbk = value & 0x01
                } else {
                    self.vbk = value
                }
            }
            0xFF51 => self.hdma1 = value,
            0xFF52 => self.hdma2 = value,
            0xFF53 => self.hdma3 = value,
            0xFF54 => self.hdma4 = value,
            0xFF55 => self.hdma5 = value, // TODO: trigger HDMA
            0xFF68 => self.bcps = value,
            0xFF69 => {
                let index = (self.bcps & 0x3F) as usize;
                self.bg_palette_ram[index] = value;
                if self.bcps & 0x80 != 0 {
                    self.bcps = 0x80 | ((self.bcps.wrapping_add(1)) & 0x3F);
                }
            }
            0xFF6A => self.ocps = value,
            0xFF6B => {
                let index = (self.ocps & 0x3F) as usize;
                self.obj_palette_ram[index] = value;
                if self.ocps & 0x80 != 0 {
                    self.ocps = 0x80 | ((self.ocps.wrapping_add(1)) & 0x3F);
                }
            }
            0xFF6C => self.opri = value & 0x01,
            _ => {}
        }
    }
}
