use crate::error::GbResult;
use crate::gb::ppu::framebuffer::Framebuffer;
use crate::rom::Rom;

mod boot_rom;
pub mod bus;
pub mod cartridge;
pub mod cpu;
mod dma;
pub mod ic;
mod memory;
pub mod ppu;
pub mod timer;

// ToDo: CGB specific registers like speed mode
pub struct GameBoy {
    pub boot_rom: boot_rom::BootRom,
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
    pub cycle_counter: u32,
}

impl GameBoy {
    pub fn new(model: GbModel, boot_rom: Option<Vec<u8>>, rom_header_checksum: u8) -> Self {
        let cpu = if boot_rom.is_some() {
            cpu::Cpu::new_with_boot_rom(model)
        } else {
            match model {
                GbModel::Dmg => cpu::Cpu::new_dmg(rom_header_checksum),
                GbModel::Cgb => cpu::Cpu::new_cgb(),
            }
        };

        let boot_rom = if let Some(boot_rom) = boot_rom {
            boot_rom::BootRom {
                rom: boot_rom,
                mounted: true,
            }
        } else {
            boot_rom::BootRom::default()
        };

        Self {
            boot_rom,
            cpu,
            cartridge: cartridge::Cartridge::new(),
            #[cfg(feature = "debug")]
            debugger: crate::debug::Debugger::new(),
            dma: dma::DmaController::new(model),
            ic: ic::InterruptController::new(),
            memory: memory::Memory::new(),
            timer: timer::Timer::new(),
            ppu: ppu::Ppu::new(model),
            model,
            cycle_counter: 0,
        }
    }

    pub fn new_empty(model: GbModel) -> Self {
        Self::new(model, None, 0x00)
    }

    pub fn load_rom(&mut self, rom: &Rom) -> GbResult<()> {
        let boot_rom = if !self.boot_rom.rom.is_empty() {
            Some(self.boot_rom.rom.clone())
        } else {
            None
        };

        *self = Self::new(self.model, boot_rom, rom.provided_header_checksum()?);
        self.cartridge.load_rom(rom)?;
        Ok(())
    }

    pub fn load_boot_rom(&mut self, rom: &[u8]) {
        *self = Self::new(self.model, Some(rom.to_vec()), 0x00);
    }

    pub fn step(&mut self) {
        self.cpu.step(&mut bus::CpuBus {
            boot_rom: &mut self.boot_rom,
            cartridge: &mut self.cartridge,
            #[cfg(feature = "debug")]
            debugger: &mut self.debugger,
            dma: &mut self.dma,
            ic: &mut self.ic,
            memory: &mut self.memory,
            ppu: &mut self.ppu,
            timer: &mut self.timer,
            cycles: &mut self.cycle_counter,
        });
    }

    // ToDo: Rely on PPU frame ready rather than frame cycles
    pub fn run_frame(&mut self) {
        while self.cycle_counter < self.model.frame_cycles() {
            self.step();
        }
        self.cycle_counter -= self.model.frame_cycles();
    }

    pub fn run_cycles(&mut self, cycles: u32) {
        self.cycle_counter = 0;
        while self.cycle_counter < cycles {
            self.step();
        }
    }

    pub fn frame(&self) -> &Framebuffer {
        self.ppu.frame()
    }

    pub fn soft_reset(&mut self) {
        self.boot_rom.soft_reset();
        self.cpu
            .soft_reset(self.cartridge.header.provided_header_checksum);
        self.cartridge.soft_reset();
        self.dma.soft_reset();
        self.ic.soft_reset();
        self.memory.soft_reset();
        self.timer.soft_reset();
        self.ppu.soft_reset();
        self.cycle_counter = 0;
        #[cfg(feature = "debug")]
        {
            self.debugger.soft_reset();
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum GbModel {
    #[default]
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
