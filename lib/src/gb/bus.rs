use crate::gb::memory::Memory;
use crate::gb::ppu::Ppu;
use crate::gb::timer::Timer;

/// Connecting the CPU to the other components of the Game Boy
pub struct Bus<'a> {
    pub memory: &'a mut Memory,
    pub ppu: &'a mut Ppu,
    pub timer: &'a mut Timer,
}

impl Bus<'_> {
    pub fn cycle(&mut self) {}

    pub fn read(&mut self, addr: u16) -> u8 {
        self.cycle();
        self.memory.read(addr)
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        self.cycle();
        self.memory.write(addr, value);
    }
}
