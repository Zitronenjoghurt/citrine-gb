use crate::error::GbResult;
use crate::rom::header::RomCgbMode;
use crate::rom::Rom;

pub mod bus;
mod cartridge;
pub mod cpu;
mod dma;
pub mod ic;
mod memory;
mod ppu;
mod timer;

// ToDo: CGB specific registers like speed mode
pub struct GameBoy {
    pub cpu: cpu::Cpu,
    pub cartridge: cartridge::Cartridge,
    pub dma: dma::DmaController,
    pub ic: ic::InterruptController,
    pub memory: memory::Memory,
    pub timer: timer::Timer,
    pub ppu: ppu::Ppu,
    pub cgb: bool,
}

impl GameBoy {
    pub fn new_dmg(header_checksum: u8) -> Self {
        Self {
            cpu: cpu::Cpu::new_dmg(header_checksum),
            cartridge: cartridge::Cartridge::new(),
            dma: dma::DmaController::new(false),
            ic: ic::InterruptController::new(),
            memory: memory::Memory::new(),
            timer: timer::Timer::new(),
            ppu: ppu::Ppu::new(false),
            cgb: false,
        }
    }

    pub fn new_cgb() -> Self {
        Self {
            cpu: cpu::Cpu::new_cgb(),
            cartridge: cartridge::Cartridge::new(),
            dma: dma::DmaController::new(true),
            ic: ic::InterruptController::new(),
            memory: memory::Memory::new(),
            timer: timer::Timer::new(),
            ppu: ppu::Ppu::new(true),
            cgb: true,
        }
    }

    pub fn load_rom(&mut self, rom: &Rom) -> GbResult<()> {
        let use_cgb = rom.cgb_mode()? != RomCgbMode::None;
        *self = if use_cgb {
            Self::new_cgb()
        } else {
            Self::new_dmg(rom.provided_header_checksum()?)
        };
        self.cartridge.load_rom(rom)?;
        Ok(())
    }

    pub fn step(&mut self) {
        self.cpu.step(&mut bus::CpuBus {
            cartridge: &mut self.cartridge,
            dma: &mut self.dma,
            ic: &mut self.ic,
            memory: &mut self.memory,
            ppu: &mut self.ppu,
            timer: &mut self.timer,
        });
    }
}
