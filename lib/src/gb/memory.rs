use crate::{ReadMemory, WriteMemory};

const VRAM_BANK_SIZE: usize = 0x2000; // 8KiB
const WRAM_BANK_SIZE: usize = 0x1000; // 4KiB
const OAM_SIZE: usize = 160; // Bytes
const HRAM_SIZE: usize = 127; // Bytes
const IO_SIZE: usize = 128; // Bytes

pub struct Memory {
    // ToDo: Put in PPU
    vram: Vec<[u8; VRAM_BANK_SIZE]>,
    wram: Vec<[u8; WRAM_BANK_SIZE]>,
    oam: [u8; OAM_SIZE],
    hram: [u8; HRAM_SIZE],
    // ToDo: Put in IO components (e.g. Timer, Serial, Joypad)
    io: [u8; IO_SIZE],
}

impl Memory {
    pub fn new() -> Self {
        Self {
            vram: vec![[0; VRAM_BANK_SIZE]; 1],
            wram: vec![[0; WRAM_BANK_SIZE]; 2],
            oam: [0; OAM_SIZE],
            hram: [0; HRAM_SIZE],
            io: [0; IO_SIZE],
        }
    }
}

impl ReadMemory for Memory {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => self.vram[0][(addr - 0x8000) as usize],
            0xC000..=0xCFFF => self.wram[0][(addr - 0xC000) as usize],
            0xD000..=0xDFFF => self.wram[1][(addr - 0xD000) as usize],
            0xE000..=0xFDFF => self.read_naive(addr - 0x2000), // echo RAM
            0xFE00..=0xFE9F => self.oam[(addr - 0xFE00) as usize],
            0xFF00..=0xFF7F => self.io[(addr - 0xFF00) as usize],
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize],
            0xFFFF => self.io[0x7F],
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Memory {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => self.vram[0][(addr - 0x8000) as usize] = value,
            0xC000..=0xCFFF => self.wram[0][(addr - 0xC000) as usize] = value,
            0xD000..=0xDFFF => self.wram[1][(addr - 0xD000) as usize] = value,
            0xE000..=0xFDFF => self.write_naive(addr - 0x2000, value), // echo RAM
            0xFE00..=0xFE9F => self.oam[(addr - 0xFE00) as usize] = value,
            0xFF00..=0xFF7F => self.io[(addr - 0xFF00) as usize] = value,
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize] = value,
            0xFFFF => self.io[0x7F] = value,
            _ => {}
        }
    }
}
