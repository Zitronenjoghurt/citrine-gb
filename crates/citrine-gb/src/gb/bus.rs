use crate::gb::GbModel;
use crate::gb::apu::Apu;
use crate::gb::boot_rom::BootRom;
use crate::gb::cartridge::Cartridge;
use crate::gb::cgb::CgbControl;
use crate::gb::dma::DmaController;
use crate::gb::hdma::Hdma;
use crate::gb::ic::{ICInterface, InterruptController};
use crate::gb::joypad::Joypad;
use crate::gb::memory::Memory;
use crate::gb::oam_bug::{self, OamAccess};
use crate::gb::ppu::Ppu;
use crate::gb::ppu::types::mode::PpuMode;
use crate::gb::serial::Serial;
use crate::gb::timer::Timer;
use crate::utils::bit::{hi, lo};
use crate::{ReadMemory, WriteMemory};

/// Connecting the CPU to the other components of the Game Boy
pub struct CpuBus<'a> {
    pub boot_rom: &'a mut BootRom,
    pub cartridge: &'a mut Cartridge,
    pub cgb: &'a mut CgbControl,
    #[cfg(feature = "debug")]
    pub debugger: &'a mut crate::debug::Debugger,
    pub dma: &'a mut DmaController,
    pub hdma: &'a mut Hdma,
    pub ic: &'a mut InterruptController,
    pub joypad: &'a mut Joypad,
    pub memory: &'a mut Memory,
    pub ppu: &'a mut Ppu,
    pub apu: &'a mut Apu,
    pub timer: &'a mut Timer,
    pub serial: &'a mut Serial,
    pub model: GbModel,
    pub cycles: &'a mut u32,
}

macro_rules! read_io {
    ($bus:expr, $addr:expr) => {{
        let addr: u16 = $addr;
        #[allow(clippy::match_overlapping_arm)]
        match addr {
            0xFF00 => $bus.joypad.read_naive(addr),
            0xFF01 => $bus.serial.sb,
            0xFF02 => $bus.serial.read_sc($bus.cgb.cgb_mode),
            0xFF04..=0xFF07 => $bus.timer.read_naive(addr),
            0xFF0F => $bus.ic.flag.into(),
            0xFF10..=0xFF14 | 0xFF16..=0xFF1E | 0xFF20..=0xFF26 | 0xFF30..=0xFF3F => {
                $bus.apu.read_naive(addr)
            }
            0xFF46 => $bus.dma.source,
            0xFF69 | 0xFF6B | 0xFF6C if !$bus.cgb.cgb_mode => 0xFF,
            0xFF40..=0xFF45 | 0xFF47..=0xFF4B | 0xFF4F | 0xFF68..=0xFF6C => {
                $bus.ppu.read_naive(addr)
            }
            0xFF4D | 0xFF56 | 0xFF72..=0xFF75 => $bus.cgb.read(addr),
            0xFF55 if $bus.cgb.cgb_mode => $bus.hdma.read_hdma5(),
            0xFF70 if $bus.cgb.cgb_mode => $bus.memory.svbk(),
            0xFF76 | 0xFF77 if $bus.model.is_cgb() => $bus.apu.read_naive(addr),
            0xFFFF => $bus.ic.enable,
            _ => 0xFF,
        }
    }};
}

#[cfg(feature = "debug")]
pub(crate) use read_io;

impl ReadMemory for CpuBus<'_> {
    fn read_naive(&self, addr: u16) -> u8 {
        if self.boot_rom.maps(addr) {
            return self.boot_rom.rom[addr as usize];
        }

        match addr {
            0x0000..=0x7FFF => self.cartridge.read_naive(addr),
            0x8000..=0x9FFF => self.ppu.read_naive(addr),
            0xA000..=0xBFFF => self.cartridge.read_naive(addr),
            0xFE00..=0xFE9F => self.ppu.read_naive(addr),
            0xFF00..=0xFF7F | 0xFFFF => read_io!(self, addr),
            _ => self.memory.read_naive(addr),
        }
    }
}

impl WriteMemory for CpuBus<'_> {
    fn write_naive(&mut self, addr: u16, value: u8) {
        if self.boot_rom.mounted && addr == 0xFF50 {
            self.boot_rom.mounted = false;
            self.ppu.set_cgb_mode(self.cgb.cgb_mode);
            return;
        }

        match addr {
            0x0000..=0x7FFF => self.cartridge.write_naive(addr, value),
            0x8000..=0x9FFF => self.ppu.write_naive(addr, value),
            0xA000..=0xBFFF => self.cartridge.write_naive(addr, value),
            0xFE00..=0xFE9F => self.ppu.write_naive(addr, value),
            0xFF00 => self.joypad.write_naive(addr, value),
            0xFF01 => self.serial.sb = value,
            0xFF02 => self.serial.write_sc(value, self.cgb.cgb_mode, self.ic),
            0xFF04 => self.reset_div(),
            0xFF05..=0xFF07 => self.timer.write_naive(addr, value),
            0xFF0F => self.ic.flag = value.into(),
            0xFF10..=0xFF14 | 0xFF16..=0xFF1E | 0xFF20..=0xFF26 | 0xFF30..=0xFF3F => {
                self.apu.write_naive(addr, value)
            }
            0xFF46 => self.dma.start(value),
            0xFF40 => {
                let was_enabled = self.ppu.lcdc.lcd_enabled;
                let mode = self.ppu.stat.ppu_mode;
                self.ppu.write_naive(addr, value);
                if was_enabled && !self.ppu.lcdc.lcd_enabled && mode != PpuMode::HBlank {
                    self.hdma.lcd_disabled();
                }
            }
            0xFF69 | 0xFF6B | 0xFF6C if !self.cgb.cgb_mode => {}
            0xFF41..=0xFF45 | 0xFF47..=0xFF4B | 0xFF4F | 0xFF68..=0xFF6C => {
                self.ppu.write_naive(addr, value)
            }
            0xFF4C if self.boot_rom.mounted => self.cgb.write_key0(value),
            0xFF4D | 0xFF56 | 0xFF72..=0xFF75 => self.cgb.write(addr, value),
            0xFF51..=0xFF55 if self.cgb.cgb_mode => {
                let in_hblank = self.ppu.stat.ppu_mode == PpuMode::HBlank;
                self.hdma.write(addr, value, in_hblank)
            }
            0xFF70 if self.cgb.cgb_mode || (self.model.is_cgb() && self.boot_rom.mounted) => {
                self.memory.set_svbk(value)
            }
            0xFF4C..=0xFF7F => {}
            0xFFFF => self.ic.enable = value,
            _ => self.memory.write_naive(addr, value),
        }

        if addr == 0xFF40 || addr == 0xFF41 || addr == 0xFF45 {
            self.ppu.check_lyc();
            self.ppu.evaluate_stat_interrupts(self.ic);
        }
    }
}

impl CpuBus<'_> {
    fn tick(&mut self, halt_sample: Option<u8>) -> bool {
        let old_div = self.timer.div;
        self.timer.cycle(self.ic);
        self.serial
            .div_changed(old_div, self.timer.div, self.cgb.cgb_mode, self.ic);
        self.serial.tick(self.cgb.double_speed, self.ic);
        if self.cgb.div_reset_pending {
            self.cgb.div_reset_pending = false;
            let old = self.timer.div;
            self.timer.speed_switch_reset(self.cgb.div_reset_lookahead);
            self.serial
                .div_changed(old, self.timer.div, self.cgb.cgb_mode, self.ic);
            self.apu.speed_switched(old, self.cgb.double_speed);
        }

        let mode = self.ppu.stat.ppu_mode;
        let pending = match halt_sample {
            Some(dot) => {
                self.ppu
                    .cycle_sampled(self.ic, self.dma.active, self.cgb.double_speed, dot)
            }
            None => {
                self.ppu
                    .cycle(self.ic, self.dma.active, self.cgb.double_speed);
                false
            }
        };
        if mode != PpuMode::HBlank && self.ppu.stat.ppu_mode == PpuMode::HBlank && self.ppu.ly < 144
        {
            self.hdma.hblank_started(halt_sample.is_some());
        }

        self.apu.cycle(
            self.timer,
            self.cgb.double_speed,
            #[cfg(feature = "debug")]
            self.debugger,
        );

        if self.cgb.normal_speed_cycle() {
            self.cartridge.cycle();
        }

        if let Some((src, dst)) = self.dma.cycle() {
            let byte = self.read_naive(self.dma.source_address(src));
            self.dma.bus_value = byte;
            self.write_naive(dst, byte);
        }

        *self.cycles = self.cycles.wrapping_add(1);

        #[cfg(feature = "debug")]
        {
            self.debugger.total_cycles = self.debugger.total_cycles.wrapping_add(1);
        }
        pending
    }

    fn oam_bug(&mut self, addr: u16, access: OamAccess) {
        if addr & 0xFF00 == 0xFE00 && self.model.is_dmg() {
            oam_bug::corrupt(self.ppu, access);
        }
    }

    fn read_now(&self, addr: u16) -> u8 {
        if self.dma.cpu_conflicts(addr) {
            return if addr >= 0xFE00 {
                0xFF
            } else {
                self.dma.bus_value
            };
        }

        if self.ppu.cpu_read_blocked(addr) {
            return 0xFF;
        }

        self.read_naive(addr)
    }

    fn reset_div(&mut self) {
        let old = self.timer.div;
        self.timer.write_naive(0xFF04, 0);
        self.serial
            .div_changed(old, self.timer.div, self.cgb.cgb_mode, self.ic);
    }

    #[cfg(feature = "debug")]
    #[cold]
    fn watch(&mut self, addr: u16, seen: crate::debug::breakpoints::Access) {
        let address_of = |addr| {
            crate::debug::memory::address_of(
                self.boot_rom,
                self.cartridge,
                self.ppu,
                self.memory,
                addr,
            )
        };
        let Some(at) = address_of(addr) else {
            return;
        };
        let by = address_of(self.debugger.breakpoints.instruction);
        self.debugger.breakpoints.check(at, seen, by);
    }

    fn hdma_source_byte(&self, src: u16) -> u8 {
        if src < 0x8000 || (src & 0xE000) == 0xA000 || (src & 0xE000) == 0xC000 {
            self.read_naive(src)
        } else {
            0xFF
        }
    }
}

impl CpuBusInterface for CpuBus<'_> {
    fn cycle(&mut self) {
        self.tick(None);
    }

    fn cycle_halted(&mut self, sample_dot: u8) -> Option<bool> {
        Some(self.tick(Some(sample_dot)))
    }

    fn halt_ended(&mut self) {
        self.hdma
            .left_halt(self.ppu.stat.ppu_mode == PpuMode::HBlank);
    }

    fn service_hdma(&mut self) {
        let block_cycles = if self.cgb.double_speed { 17 } else { 9 };
        while self.hdma.is_pending() {
            for _ in 0..16 {
                let (src, dst) = self.hdma.next_transfer();
                let byte = self.hdma_source_byte(src);
                self.ppu.write_naive(dst, byte);
            }
            for _ in 0..block_cycles {
                self.tick(None);
            }
            if self.hdma.block_done() {
                break;
            }
        }
    }

    fn speed_switch(&mut self, ime: bool) -> bool {
        if !self.cgb.speed_switch_armed() {
            return false;
        }
        self.cgb.switch_speed(!ime);
        self.ppu.speed_switched(self.cgb.double_speed, self.ic);
        true
    }

    fn cycle_with_address(&mut self, addr: u16) {
        self.tick(None);
        self.oam_bug(addr, OamAccess::Write);
    }

    fn read_increment(&mut self, addr: u16) -> u8 {
        self.tick(None);
        self.oam_bug(addr, OamAccess::ReadIncrement);
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            self.watch(addr, crate::debug::breakpoints::Access::Read);
        }
        self.read_now(addr)
    }

    fn read(&mut self, addr: u16) -> u8 {
        self.tick(None);
        self.oam_bug(addr, OamAccess::Read);
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            self.watch(addr, crate::debug::breakpoints::Access::Read);
        }
        self.read_now(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            use crate::debug::breakpoints::Access;
            let changed = self.read_naive(addr) != value;
            self.watch(
                addr,
                if changed {
                    Access::Change
                } else {
                    Access::Write
                },
            );
        }
        if !self.dma.cpu_conflicts(addr) && self.ppu.schedule_write(addr, value) {
            self.tick(None);
            return;
        }
        self.tick(None);
        self.oam_bug(addr, OamAccess::Write);

        if self.dma.cpu_conflicts(addr) || self.ppu.cpu_write_blocked(addr) {
            return;
        }

        self.write_naive(addr, value);
    }

    #[cfg(feature = "debug")]
    fn on_fetch(&mut self, addr: u16) {
        if self.debugger.static_analysis_enabled {
            match addr {
                0x0000..=0x7FFF => self.debugger.disassembly.on_fetch(self.cartridge, addr),
                _ => {
                    if let Some(at) = self.memory.code_address(addr) {
                        self.debugger.disassembly.on_ram_fetch(at);
                    }
                }
            }
        }
    }

    #[cfg(feature = "debug")]
    fn on_instruction(&mut self, cpu: &crate::gb::cpu::Cpu) {
        use crate::debug::disassembly::DisassemblySource;
        use crate::debug::trace::TraceEntry;
        let pc = cpu.pc.wrapping_sub(1);
        if self.debugger.breakpoints.watching() {
            self.debugger.breakpoints.instruction = pc;
        }
        if !self.debugger.trace.is_on() {
            return;
        }
        let at = match pc {
            0x0000..=0x7FFF if !self.boot_rom.maps(pc) => Some(self.cartridge.rom_address(pc)),
            0x0000..=0x7FFF => None,
            _ => self.memory.code_address(pc),
        };
        let bytes = [
            cpu.ir,
            self.read_naive(cpu.pc),
            self.read_naive(cpu.pc.wrapping_add(1)),
        ];
        self.debugger.trace.push(TraceEntry {
            pc,
            at,
            bytes,
            registers: cpu.trace_registers(),
        });
    }
}

pub trait CpuBusInterface {
    fn cycle(&mut self);

    fn cycle_halted(&mut self, _sample_dot: u8) -> Option<bool> {
        self.cycle();
        None
    }

    fn halt_ended(&mut self) {}

    fn service_hdma(&mut self) {}

    fn speed_switch(&mut self, _ime: bool) -> bool {
        false
    }

    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, value: u8);

    fn cycle_with_address(&mut self, _addr: u16) {
        self.cycle();
    }

    fn read_increment(&mut self, addr: u16) -> u8 {
        self.read(addr)
    }

    fn read_word(&mut self, addr: u16) -> u16 {
        u16::from_le_bytes([self.read(addr), self.read(addr + 1)])
    }

    fn write_word(&mut self, addr: u16, value: u16) {
        self.write(addr, lo(value));
        self.write(addr + 1, hi(value));
    }

    #[cfg(feature = "debug")]
    fn on_fetch(&mut self, _addr: u16) {}

    #[cfg(feature = "debug")]
    fn on_instruction(&mut self, _cpu: &crate::gb::cpu::Cpu) {}
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
