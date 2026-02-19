use crate::gb::ppu::framebuffer::Framebuffer;
use crate::{ReadMemory, WriteMemory};

mod color;
mod framebuffer;

const VRAM_BANK_SIZE: usize = 0x2000; // 8KiB
const OAM_SIZE: usize = 160; // Bytes

pub struct Ppu {
    frame: Framebuffer,
    cgb: bool,
    // Memory
    /// Video RAM (2 banks on CGB)
    vram: [[u8; VRAM_BANK_SIZE]; 2],
    /// Sprite attribute table
    oam: [u8; OAM_SIZE],
    /// LCD control
    lcdc: u8,
    /// LCD status
    stat: u8,
    /// BG scroll Y
    scy: u8,
    /// BG scroll X
    scx: u8,
    /// Current scanline
    ly: u8,
    /// Scanline compare
    lyc: u8,
    /// OAM DMA source address
    dma: u8,
    /// BG palette (DMG)
    bgp: u8,
    /// OBJ palette 0 (DMG)
    obp0: u8,
    /// OBJ palette 1 (DMG)
    obp1: u8,
    /// Window Y position
    wy: u8,
    /// Window X position (+7)
    wx: u8,
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

impl Default for Ppu {
    fn default() -> Self {
        Self {
            frame: Framebuffer::new(),
            cgb: true,
            vram: [[0x00; VRAM_BANK_SIZE]; 2],
            oam: [0x00; OAM_SIZE],
            lcdc: 0x91,
            stat: 0x85,
            scy: 0x00,
            scx: 0x00,
            ly: 0x00,
            lyc: 0x00,
            dma: 0x00,
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
}

impl Ppu {
    pub fn new(cgb: bool) -> Self {
        if cgb {
            Self::default()
        } else {
            Self {
                cgb: false,
                dma: 0xFF,
                ..Default::default()
            }
        }
    }
}

impl ReadMemory for Ppu {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => {
                if self.cgb {
                    self.vram[(self.vbk & 1) as usize][(addr - 0x8000) as usize]
                } else {
                    self.vram[0][(addr - 0x8000) as usize]
                }
            }
            0xFE00..=0xFE9F => self.oam[(addr - 0xFE00) as usize],
            0xFF40 => self.lcdc,
            0xFF41 => self.stat,
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF46 => self.dma,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            0xFF4F => {
                if self.cgb {
                    self.vbk | 0xFE // only bit 0 readable
                } else {
                    0xFF
                }
            }
            0xFF51 => {
                if self.cgb {
                    self.hdma1
                } else {
                    0xFF
                }
            }
            0xFF52 => {
                if self.cgb {
                    self.hdma2
                } else {
                    0xFF
                }
            }
            0xFF53 => {
                if self.cgb {
                    self.hdma3
                } else {
                    0xFF
                }
            }
            0xFF54 => {
                if self.cgb {
                    self.hdma4
                } else {
                    0xFF
                }
            }
            0xFF55 => {
                if self.cgb {
                    self.hdma5
                } else {
                    0xFF
                }
            }
            0xFF68 => {
                if self.cgb {
                    self.bcps
                } else {
                    0xFF
                }
            }
            0xFF69 => {
                if self.cgb {
                    self.bg_palette_ram[(self.bcps & 0x3F) as usize]
                } else {
                    0xFF
                }
            }
            0xFF6A => {
                if self.cgb {
                    self.ocps
                } else {
                    0xFF
                }
            }
            0xFF6B => {
                if self.cgb {
                    self.obj_palette_ram[(self.ocps & 0x3F) as usize]
                } else {
                    0xFF
                }
            }
            0xFF6C => {
                if self.cgb {
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
                if self.cgb {
                    self.vram[(self.vbk & 1) as usize][(addr - 0x8000) as usize] = value
                } else {
                    self.vram[0][(addr - 0x8000) as usize] = value;
                }
            }
            0xFE00..=0xFE9F => self.oam[(addr - 0xFE00) as usize] = value,
            0xFF40 => self.lcdc = value,
            0xFF41 => self.stat = (self.stat & 0x87) | (value & 0x78), // bits 0-2 read-only, bit 7 unused
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {} // LY is read-only
            0xFF45 => self.lyc = value,
            0xFF46 => self.dma = value, // TODO: trigger OAM DMA
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
            0xFF4A => self.wy = value,
            0xFF4B => self.wx = value,
            0xFF4F => {
                if self.cgb {
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
