use crate::error::GbResult;
use crate::gb::ppu::framebuffer::Framebuffer;
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
    #[cfg(feature = "debug")]
    pub debugger: crate::debug::Debugger,
    pub dma: dma::DmaController,
    pub ic: ic::InterruptController,
    pub memory: memory::Memory,
    pub timer: timer::Timer,
    pub ppu: ppu::Ppu,
    pub model: GbModel,
    pub frame_cycles: u32,
}

impl GameBoy {
    pub fn new_dmg(header_checksum: u8) -> Self {
        Self {
            cpu: cpu::Cpu::new_dmg(header_checksum),
            cartridge: cartridge::Cartridge::new(),
            #[cfg(feature = "debug")]
            debugger: crate::debug::Debugger::new(),
            dma: dma::DmaController::new(GbModel::Dmg),
            ic: ic::InterruptController::new(),
            memory: memory::Memory::new(),
            timer: timer::Timer::new(),
            ppu: ppu::Ppu::new(GbModel::Dmg),
            model: GbModel::Dmg,
            frame_cycles: 0,
        }
    }

    pub fn new_cgb() -> Self {
        Self {
            cpu: cpu::Cpu::new_cgb(),
            cartridge: cartridge::Cartridge::new(),
            #[cfg(feature = "debug")]
            debugger: crate::debug::Debugger::new(),
            dma: dma::DmaController::new(GbModel::Cgb),
            ic: ic::InterruptController::new(),
            memory: memory::Memory::new(),
            timer: timer::Timer::new(),
            ppu: ppu::Ppu::new(GbModel::Cgb),
            model: GbModel::Cgb,
            frame_cycles: 0,
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
            #[cfg(feature = "debug")]
            debugger: &mut self.debugger,
            dma: &mut self.dma,
            ic: &mut self.ic,
            memory: &mut self.memory,
            ppu: &mut self.ppu,
            timer: &mut self.timer,
            cycles: &mut self.frame_cycles,
        });
    }

    // ToDo: Rely on PPU frame ready rather than frame cycles
    pub fn run_frame(&mut self) {
        while self.frame_cycles < self.model.frame_cycles() {
            self.step();
        }
        self.frame_cycles -= self.model.frame_cycles();
    }

    pub fn frame(&self) -> &Framebuffer {
        self.ppu.frame()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GbModel {
    Dmg,
    Cgb,
}

impl GbModel {
    pub fn frame_cycles(&self) -> u32 {
        match self {
            GbModel::Dmg => 17556,
            GbModel::Cgb => 35112,
        }
    }

    pub fn is_dmg(&self) -> bool {
        matches!(self, GbModel::Dmg)
    }

    pub fn is_cgb(&self) -> bool {
        matches!(self, GbModel::Cgb)
    }
}
