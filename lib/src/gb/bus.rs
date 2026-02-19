use crate::gb::cartridge::Cartridge;
use crate::gb::memory::Memory;
use crate::gb::ppu::Ppu;
use crate::gb::timer::Timer;
use crate::utils::{hi, lo};
use crate::{ReadMemory, WriteMemory};

/// Connecting the CPU to the other components of the Game Boy
pub struct Bus<'a> {
    pub cartridge: &'a mut Cartridge,
    pub memory: &'a mut Memory,
    pub ppu: &'a mut Ppu,
    pub timer: &'a mut Timer,
}

impl ReadMemory for Bus<'_> {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cartridge.read_naive(addr),
            0x8000..=0x9FFF => self.ppu.read_naive(addr),
            0xA000..=0xBFFF => self.cartridge.read_naive(addr),
            0xFE00..=0xFE9F => self.ppu.read_naive(addr),
            0xFF40..=0xFF4B | 0xFF4F | 0xFF51..=0xFF55 | 0xFF68..=0xFF6C => {
                self.ppu.read_naive(addr)
            }
            _ => self.memory.read_naive(addr),
        }
    }
}

impl WriteMemory for Bus<'_> {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x7FFF => self.cartridge.write_naive(addr, value),
            0x8000..=0x9FFF => self.ppu.write_naive(addr, value),
            0xA000..=0xBFFF => self.cartridge.write_naive(addr, value),
            0xFE00..=0xFE9F => self.ppu.write_naive(addr, value),
            0xFF40..=0xFF4B | 0xFF4F | 0xFF51..=0xFF55 | 0xFF68..=0xFF6C => {
                self.ppu.write_naive(addr, value)
            }
            _ => self.memory.write_naive(addr, value),
        }
    }
}

impl BusInterface for Bus<'_> {
    fn cycle(&mut self) {
        self.ppu.cycle();
    }

    fn read(&mut self, addr: u16) -> u8 {
        self.cycle();
        self.read_naive(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.cycle();
        self.write_naive(addr, value);
    }
}

pub trait BusInterface {
    fn cycle(&mut self);
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, value: u8);

    fn read_word(&mut self, addr: u16) -> u16 {
        u16::from_le_bytes([self.read(addr), self.read(addr + 1)])
    }

    fn write_word(&mut self, addr: u16, value: u16) {
        self.write(addr, lo(value));
        self.write(addr + 1, hi(value));
    }
}
