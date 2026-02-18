use crate::gb::memory::Memory;
use crate::gb::ppu::Ppu;
use crate::gb::timer::Timer;
use crate::utils::{hi, lo};
use crate::{ReadMemory, WriteMemory};

/// Connecting the CPU to the other components of the Game Boy
pub struct Bus<'a> {
    pub memory: &'a mut Memory,
    pub ppu: &'a mut Ppu,
    pub timer: &'a mut Timer,
}

impl ReadMemory for Bus<'_> {
    fn read_naive(&self, addr: u16) -> u8 {
        self.memory.read(addr)
    }
}

impl WriteMemory for Bus<'_> {
    fn write_naive(&mut self, addr: u16, value: u8) {
        self.memory.write(addr, value);
    }
}

impl BusInterface for Bus<'_> {
    fn cycle(&mut self) {}

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
