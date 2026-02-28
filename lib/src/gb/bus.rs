use crate::gb::boot_rom::BootRom;
use crate::gb::cartridge::Cartridge;
use crate::gb::dma::DmaController;
use crate::gb::ic::{ICInterface, InterruptController};
use crate::gb::joypad::Joypad;
use crate::gb::memory::Memory;
use crate::gb::ppu::Ppu;
use crate::gb::timer::Timer;
use crate::utils::bit::{hi, lo};
use crate::{ReadMemory, WriteMemory};

/// Connecting the CPU to the other components of the Game Boy
pub struct CpuBus<'a> {
    pub boot_rom: &'a mut BootRom,
    pub cartridge: &'a mut Cartridge,
    #[cfg(feature = "debug")]
    pub debugger: &'a mut crate::debug::Debugger,
    pub dma: &'a mut DmaController,
    pub ic: &'a mut InterruptController,
    pub joypad: &'a mut Joypad,
    pub memory: &'a mut Memory,
    pub ppu: &'a mut Ppu,
    pub timer: &'a mut Timer,
    pub cycles: &'a mut u32,
}

impl ReadMemory for CpuBus<'_> {
    fn read_naive(&self, addr: u16) -> u8 {
        if self.boot_rom.mounted && addr < self.boot_rom.boundary_address() {
            return self.boot_rom.rom[addr as usize];
        }

        match addr {
            0x0000..=0x7FFF => self.cartridge.read_naive(addr),
            0x8000..=0x9FFF => self.ppu.read_naive(addr),
            0xA000..=0xBFFF => self.cartridge.read_naive(addr),
            0xFE00..=0xFE9F => self.ppu.read_naive(addr),
            0xFF00 => self.joypad.read_naive(addr),
            0xFF04..=0xFF07 => self.timer.read_naive(addr),
            0xFF0F => self.ic.flag.into(),
            0xFF46 => self.dma.source,
            0xFF40..=0xFF45 | 0xFF47..=0xFF4B | 0xFF4F | 0xFF51..=0xFF55 | 0xFF68..=0xFF6C => {
                self.ppu.read_naive(addr)
            }
            0xFFFF => self.ic.enable.into(),
            _ => self.memory.read_naive(addr),
        }
    }
}

impl WriteMemory for CpuBus<'_> {
    fn write_naive(&mut self, addr: u16, value: u8) {
        if self.boot_rom.mounted && addr == 0xFF50 {
            self.boot_rom.mounted = false;
            return;
        }

        match addr {
            0x0000..=0x7FFF => self.cartridge.write_naive(addr, value),
            0x8000..=0x9FFF => self.ppu.write_naive(addr, value),
            0xA000..=0xBFFF => self.cartridge.write_naive(addr, value),
            0xFE00..=0xFE9F => self.ppu.write_naive(addr, value),
            0xFF00 => self.joypad.write_naive(addr, value),
            0xFF04..=0xFF07 => self.timer.write_naive(addr, value),
            0xFF0F => self.ic.flag = value.into(),
            0xFF46 => self.dma.start(value),
            0xFF40..=0xFF45 | 0xFF47..=0xFF4B | 0xFF4F | 0xFF51..=0xFF55 | 0xFF68..=0xFF6C => {
                self.ppu.write_naive(addr, value)
            }
            0xFFFF => self.ic.enable = value.into(),
            _ => self.memory.write_naive(addr, value),
        }

        if addr == 0xFF40 || addr == 0xFF41 || addr == 0xFF45 {
            self.ppu.check_lyc();
            self.ppu.evaluate_stat_interrupts(self.ic);
        }
    }
}

impl CpuBusInterface for CpuBus<'_> {
    fn cycle(&mut self) {
        self.timer.cycle(self.ic);
        self.ppu.cycle(self.ic, self.dma.active);

        if let Some((src, dst)) = self.dma.cycle() {
            self.write_naive(dst, self.read_naive(src));
        }

        *self.cycles = self.cycles.wrapping_add(1);

        #[cfg(feature = "debug")]
        {
            self.debugger.total_cycles = self.debugger.total_cycles.wrapping_add(1);
        }
    }

    fn read(&mut self, addr: u16) -> u8 {
        self.cycle();

        if self.dma.cpu_conflicts(addr) || self.ppu.cpu_conflicts(addr) {
            return 0xFF;
        }

        self.read_naive(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.cycle();

        if self.dma.cpu_conflicts(addr) || self.ppu.cpu_conflicts(addr) {
            return;
        }

        self.write_naive(addr, value);
    }
}

pub trait CpuBusInterface {
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

impl ICInterface for CpuBus<'_> {
    fn request_interrupt(&mut self, interrupt: crate::gb::ic::Interrupt) {
        self.ic.request_interrupt(interrupt);
    }

    fn take_interrupt(&mut self) -> Option<crate::gb::ic::Interrupt> {
        self.ic.take_interrupt()
    }

    fn has_pending_interrupt(&self) -> bool {
        self.ic.has_pending_interrupt()
    }
}

#[cfg(feature = "debug")]
impl crate::debug::DebuggerAccess for CpuBus<'_> {
    fn debugger(&self) -> &dyn crate::debug::DebuggerInterface {
        self.debugger
    }

    fn debugger_mut(&mut self) -> &mut dyn crate::debug::DebuggerInterface {
        self.debugger
    }
}
