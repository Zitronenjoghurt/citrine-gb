pub mod bus;
pub mod cpu;
mod memory;
mod ppu;
mod timer;

pub struct GameBoy {
    cpu: cpu::Cpu,
    memory: memory::Memory,
    timer: timer::Timer,
    ppu: ppu::Ppu,
    cgb: bool,
}

impl GameBoy {
    pub fn new_dmg() -> Self {
        Self {
            // ToDo: Pass actual header checksum
            cpu: cpu::Cpu::new_dmg(0x00),
            memory: memory::Memory::new(),
            timer: timer::Timer,
            ppu: ppu::Ppu,
            cgb: false,
        }
    }

    pub fn new_cgb() -> Self {
        Self {
            cpu: cpu::Cpu::new_cgb(),
            memory: memory::Memory::new(),
            timer: timer::Timer,
            ppu: ppu::Ppu,
            cgb: true,
        }
    }

    pub fn cycle(&mut self) {
        let mut bus = bus::Bus {
            memory: &mut self.memory,
            ppu: &mut self.ppu,
            timer: &mut self.timer,
        };
        self.cpu.cycle(&mut bus);
    }
}
