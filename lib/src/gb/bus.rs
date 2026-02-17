use crate::gb::memory::Memory;
use crate::gb::ppu::Ppu;
use crate::gb::timer::Timer;
use crate::utils::{hi, lo};

/// Connecting the CPU to the other components of the Game Boy
pub struct Bus<'a> {
    pub memory: &'a mut Memory,
    pub ppu: &'a mut Ppu,
    pub timer: &'a mut Timer,
}

impl BusInterface for Bus<'_> {
    fn cycle(&mut self) {}

    fn read(&mut self, addr: u16) -> u8 {
        self.cycle();
        self.memory.read(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.cycle();
        self.memory.write(addr, value);
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
