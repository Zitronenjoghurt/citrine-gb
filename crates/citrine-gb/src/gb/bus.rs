use crate::gb::GbModel;
use crate::gb::apu::Apu;
use crate::gb::boot_rom::BootRom;
use crate::gb::cartridge::{Cartridge, NO_ROM_WINDOW};
use crate::gb::cgb::CgbControl;
use crate::gb::dma::DmaController;
use crate::gb::hdma::Hdma;
use crate::gb::ic::{ICInterface, Interrupt, InterruptController};
use crate::gb::joypad::Joypad;
use crate::gb::memory::Memory;
use crate::gb::oam_bug::{self, OamAccess};
use crate::gb::ppu::Ppu;
use crate::gb::ppu::types::mode::PpuMode;
use crate::gb::serial::Serial;
use crate::gb::timer::Timer;
use crate::utils::bit::{add_word_signed_byte, hi, lo};
use crate::{ReadMemory, WriteMemory};

/// Connecting the CPU to the other components of the Game Boy
pub struct CpuBus<'a, const WINDOWS: bool = true> {
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
    pub acknowledge: Option<Interrupt>,
    pub(crate) quiet: u32,
    pub(crate) quiet_left: u32,
    pub(crate) dma_room: u32,
    pub(crate) p1_written: bool,
    pub(crate) ticked: bool,
    pub(crate) hdma_due: bool,
    pub(crate) parts_noisy: bool,
    pub(crate) rom_window: [usize; 4],
}

impl<const WINDOWS: bool> Drop for CpuBus<'_, WINDOWS> {
    fn drop(&mut self) {
        self.settle_quiet();
    }
}

pub(crate) fn mapped_rom_window<const WINDOWS: bool>(
    boot_rom: &BootRom,
    cartridge: &Cartridge,
) -> [usize; 4] {
    match cartridge.rom_window() {
        Some(window) if WINDOWS && !boot_rom.mounted => window,
        _ => NO_ROM_WINDOW,
    }
}

fn quiet_read(addr: u16) -> bool {
    addr < 0x8000 || quiet_write(addr)
}

fn quiet_pair(addr: u16) -> bool {
    (0xC000..0xFDFF).contains(&addr) || (0xFF80..0xFFFE).contains(&addr)
}

fn quiet_write(addr: u16) -> bool {
    (0xC000..0xFE00).contains(&addr) || (0xFF80..0xFFFF).contains(&addr)
}

fn quiet_io_write(addr: u16) -> bool {
    (0xA000..0xC000).contains(&addr) || (addr >= 0xFF00 && !(0xFF40..=0xFF4B).contains(&addr))
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

fn reads_settled(addr: u16) -> bool {
    matches!(addr, 0xFF04..=0xFF07 | 0xFF10..=0xFF3F | 0xFF76 | 0xFF77)
}

impl<const WINDOWS: bool> ReadMemory for CpuBus<'_, WINDOWS> {
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

impl<const WINDOWS: bool> WriteMemory for CpuBus<'_, WINDOWS> {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x7FFF | 0xA000..=0xBFFF => {
                self.cartridge.write_naive(addr, value);
                self.refresh_rom_window();
            }
            0x8000..=0x9FFF => self.ppu.write_naive(addr, value),
            0xFE00..=0xFE9F => self.ppu.write_naive(addr, value),
            0xFF00 => {
                self.joypad.write_naive(addr, value);
                self.p1_written = true;
            }
            0xFF01 => self.serial.sb = value,
            0xFF02 => {
                self.settle_clock();
                self.serial.write_sc(value, self.cgb.cgb_mode, self.ic);
                self.timer.set_lazy(0);
                self.parts_noisy = true;
            }
            0xFF04 => self.reset_div(),
            0xFF05..=0xFF07 => {
                self.settle_clock();
                self.timer.write_naive(addr, value);
                self.timer.set_lazy(0);
            }
            0xFF0F => self.ic.flag = value.into(),
            0xFF10..=0xFF14 | 0xFF16..=0xFF1E | 0xFF20..=0xFF26 | 0xFF30..=0xFF3F => {
                self.settle_apu();
                self.apu.write_naive(addr, value)
            }
            0xFF46 => {
                self.dma.start(value);
                self.parts_noisy = true;
            }
            0xFF40 => {
                let was_enabled = self.ppu.lcdc.lcd_enabled;
                let mode = self.ppu.stat.ppu_mode;
                self.ppu.write_naive(addr, value);
                if was_enabled && !self.ppu.lcdc.lcd_enabled && mode != PpuMode::HBlank {
                    self.hdma.lcd_disabled();
                    self.hdma_due = true;
                }
            }
            0xFF69 | 0xFF6B | 0xFF6C if !self.cgb.cgb_mode => {}
            0xFF41..=0xFF45 | 0xFF47..=0xFF4B | 0xFF4F | 0xFF68..=0xFF6C => {
                self.ppu.write_naive(addr, value)
            }
            0xFF4C if self.boot_rom.mounted => {
                self.settle_clock();
                self.cgb.write_key0(value);
                self.timer.set_lazy(0);
            }
            0xFF50 if self.boot_rom.mounted => {
                self.boot_rom.mounted = false;
                self.refresh_rom_window();
                self.ppu.set_cgb_mode(self.cgb.cgb_mode);
                return;
            }
            0xFF4D | 0xFF56 | 0xFF72..=0xFF75 => self.cgb.write(addr, value),
            0xFF51..=0xFF55 if self.cgb.cgb_mode => {
                let in_hblank = self.ppu.stat.ppu_mode == PpuMode::HBlank;
                self.hdma.write(addr, value, in_hblank);
                self.hdma_due = true;
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

impl<const WINDOWS: bool> CpuBus<'_, WINDOWS> {
    #[inline(never)]
    fn tick(&mut self, halt_sample: Option<u8>) -> bool {
        self.tick_inline(halt_sample)
    }

    #[inline(always)]
    fn tick_inline(&mut self, halt_sample: Option<u8>) -> bool {
        self.tick_parts::<false>(halt_sample)
    }

    #[inline(always)]
    fn tick_parts<const PARTS_QUIET: bool>(&mut self, halt_sample: Option<u8>) -> bool {
        self.tick_parts_at::<PARTS_QUIET>(halt_sample, self.cgb.double_speed)
    }

    #[inline(always)]
    fn tick_parts_at<const PARTS_QUIET: bool>(
        &mut self,
        halt_sample: Option<u8>,
        double_speed: bool,
    ) -> bool {
        if !self.timer.owe_cycle() {
            self.clock_cycle();
        }
        if !PARTS_QUIET {
            if WINDOWS {
                self.serial.tick(double_speed, self.ic);
            } else {
                self.serial.tick_plugged(double_speed, self.ic);
            }
            if self.cgb.div_reset_pending {
                self.reset_div_after_speed_switch();
            }
        } else if !WINDOWS {
            self.serial.advance_idle(1, double_speed);
        }

        let acknowledge = if PARTS_QUIET || self.acknowledge.is_none() {
            None
        } else {
            self.acknowledge.take()
        };
        let (pending, hblank) = match (halt_sample, acknowledge) {
            (_, Some(interrupt)) => {
                let dot = if double_speed { 1 } else { 2 };
                let ((), hblank) =
                    self.ppu
                        .cycle_split(self.ic, self.dma.active, double_speed, dot, |ic| {
                            ic.acknowledge_interrupt(interrupt)
                        });
                (false, hblank)
            }
            (Some(dot), None) => {
                self.ppu
                    .cycle_split(self.ic, self.dma.active, double_speed, dot, |ic| {
                        ic.has_pending_interrupt()
                    })
            }
            (None, None) => (
                false,
                self.ppu.cycle(self.ic, self.dma.active, double_speed),
            ),
        };
        if hblank {
            self.hdma.hblank_started(halt_sample.is_some());
            self.hdma_due = true;
        }

        self.apu.owe_cycle();

        if self.cgb.normal_speed_cycle() {
            self.cartridge.cycle();
        }

        if !PARTS_QUIET && let Some((src, dst)) = self.dma.cycle() {
            self.dma_transfer(src, dst);
        }

        *self.cycles = self.cycles.wrapping_add(1);
        if WINDOWS {
            self.ticked = true;
        }

        #[cfg(feature = "debug")]
        {
            self.debugger.total_cycles = self.debugger.total_cycles.wrapping_add(1);
        }
        pending
    }

    #[inline(always)]
    fn take_quiet_cycle(&mut self) -> bool {
        if !WINDOWS {
            return false;
        }
        match self.quiet_left.checked_sub(1) {
            Some(left) => {
                self.quiet_left = left;
                true
            }
            None => false,
        }
    }

    #[inline(always)]
    fn take_quiet_pair(&mut self) -> bool {
        if !WINDOWS {
            return false;
        }
        match self.quiet_left.checked_sub(2) {
            Some(left) => {
                self.quiet_left = left;
                true
            }
            None => false,
        }
    }

    #[inline(always)]
    fn quiet_cycle(&mut self) {
        if !WINDOWS {
            self.tick(None);
        } else if !self.take_quiet_cycle() {
            self.tick_open();
        }
    }

    #[inline(always)]
    fn settled_cycle(&mut self) {
        if !WINDOWS {
            self.tick(None);
        } else if self.take_quiet_cycle() {
            self.apply_quiet();
        } else {
            self.tick_open();
        }
    }

    #[inline(always)]
    fn settle_quiet(&mut self) {
        if WINDOWS && self.quiet != self.quiet_left {
            self.apply_quiet();
        }
    }

    fn close_quiet(&mut self) {
        if WINDOWS {
            self.settle_quiet();
            self.open_window(0);
            self.dma_room = 0;
        }
    }

    #[inline(always)]
    fn cycle_for_read(&mut self, addr: u16) {
        if quiet_read(addr) {
            self.quiet_cycle();
        } else if addr & 0xFF00 != 0xFE00 {
            self.settled_cycle();
        } else {
            self.close_quiet();
            self.tick(None);
        }
    }

    #[inline(never)]
    fn apply_quiet(&mut self) {
        let cycles = self.quiet - self.quiet_left;
        self.quiet = self.quiet_left;
        if self.dma.active {
            self.advance_dma(cycles);
        } else {
            self.advance_quiet(cycles, self.cgb.double_speed);
        }
    }

    #[inline(never)]
    fn tick_open(&mut self) {
        self.tick_open_inline();
    }

    #[inline(always)]
    fn tick_open_inline(&mut self) -> bool {
        self.settle_quiet();
        if !self.parts_noisy {
            let double_speed = self.cgb.double_speed;
            self.tick_parts_at::<true>(None, double_speed);
            let window = self.clock_window_at(double_speed);
            self.open_window(window);
            return true;
        }
        self.tick_inline(None);
        if !self.check_parts_quiet() {
            self.open_window(0);
            self.dma_room = self.dma_window();
            return false;
        }
        let window = self.clock_window();
        self.open_window(window);
        true
    }

    #[inline(always)]
    fn open_window(&mut self, cycles: u32) {
        self.quiet = cycles;
        self.quiet_left = cycles;
    }

    #[inline(never)]
    fn dma_window(&self) -> u32 {
        if !WINDOWS
            || !self.dma.active
            || self.dma.delay > 0
            || self.ppu.line() < 144
            || !matches!(self.dma.source, 0x00..=0x7F | 0xC0..=0xDF)
            || self.cgb.div_reset_pending
            || self.acknowledge.is_some()
            || !self.serial.quiet()
            || !self.cartridge.advances_in_bulk()
        {
            return 0;
        }
        self.clock_window()
            .min(160u32.saturating_sub(u32::from(self.dma.progress)))
    }

    #[inline(always)]
    fn take_dma_cycle(&mut self) -> bool {
        if WINDOWS && self.dma_room > 0 {
            self.dma_room -= 1;
            self.quiet += 1;
            true
        } else {
            false
        }
    }

    #[inline(never)]
    fn cycle_open(&mut self) {
        if !self.take_dma_cycle() {
            self.tick_open();
        }
    }

    fn quiet_window(&mut self) -> u32 {
        if !self.check_parts_quiet() {
            return 0;
        }
        self.clock_window()
    }

    #[inline(always)]
    fn check_parts_quiet(&mut self) -> bool {
        if self.parts_noisy {
            if !self.parts_quiet() {
                return false;
            }
            self.parts_noisy = false;
        }
        true
    }

    #[inline(always)]
    fn parts_quiet(&self) -> bool {
        !self.dma.active
            && self.dma.delay == 0
            && !self.cgb.div_reset_pending
            && self.acknowledge.is_none()
            && (self.serial.quiet() || (!WINDOWS && self.serial.link_silent()))
            && self.cartridge.advances_in_bulk()
    }

    #[inline(always)]
    fn clock_window(&self) -> u32 {
        self.clock_window_at(self.cgb.double_speed)
    }

    #[inline(always)]
    fn clock_window_at(&self, double_speed: bool) -> u32 {
        let dots = if double_speed { 2 } else { 4 };
        (u32::from(self.ppu.quiet_dots()) / dots).min(self.timer.lazy_cycles())
    }

    fn advance_quiet(&mut self, cycles: u32, double_speed: bool) {
        let dots = if double_speed { 2 } else { 4 };
        self.ppu.skip_quiet((cycles * dots) as u16);
        if !WINDOWS {
            self.serial.advance_idle(cycles, double_speed);
        }
        self.advance_lazy(cycles);
    }

    #[inline(never)]
    fn advance_dma(&mut self, cycles: u32) {
        let dots = if self.cgb.double_speed { 2 } else { 4 };
        if let Some(page) = self.dma.wram_burst(cycles) {
            let start = self.dma.progress;
            let bytes = &self.memory.wram_page(page)[usize::from(start)..][..cycles as usize];
            for (offset, &byte) in (u16::from(start)..).zip(bytes) {
                self.ppu.skip_quiet(dots);
                self.ppu.write_naive(0xFE00 | offset, byte);
            }
            if let Some(&last) = bytes.last() {
                self.dma.bus_value = last;
            }
            self.dma.progress = start + cycles as u8;
        } else {
            for _ in 0..cycles {
                self.ppu.skip_quiet(dots);
                if let Some((src, dst)) = self.dma.cycle() {
                    self.dma_transfer(src, dst);
                }
            }
        }
        self.advance_lazy(cycles);
    }

    #[inline(always)]
    fn advance_lazy(&mut self, cycles: u32) {
        self.timer.owe_cycles(cycles);
        self.apu.owe_cycles(cycles);
        let normal = if self.cgb.double_speed {
            (0..cycles)
                .filter(|_| self.cgb.normal_speed_cycle())
                .count() as u32
        } else {
            cycles
        };
        self.cartridge.advance(normal);
        *self.cycles = self.cycles.wrapping_add(cycles);
        #[cfg(feature = "debug")]
        {
            self.debugger.total_cycles = self.debugger.total_cycles.wrapping_add(cycles.into());
        }
    }

    #[inline(never)]
    fn run_hdma(&mut self) {
        let block_cycles = if self.cgb.double_speed { 16 } else { 8 };
        self.close_quiet();
        self.tick(None);
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

    #[cold]
    #[inline(never)]
    fn reset_div_after_speed_switch(&mut self) {
        self.settle_clock();
        self.timer.set_lazy(0);
        self.cgb.div_reset_pending = false;
        let old = self.timer.div;
        self.timer.speed_switch_reset(self.cgb.div_reset_lookahead);
        self.serial
            .div_changed(old, self.timer.div, self.cgb.cgb_mode, self.ic);
        self.apu.speed_switched(old, self.cgb.double_speed);
    }

    #[cold]
    #[inline(never)]
    fn dma_transfer(&mut self, src: u16, dst: u16) {
        let src = self.dma.source_address(src);
        let byte = match src {
            0xC000..=0xDFFF => self.memory.read_quiet(src),
            0xFF00.. => {
                self.settle_apu();
                self.read_naive(src)
            }
            _ => self.read_naive(src),
        };
        self.dma.bus_value = byte;
        self.ppu.write_naive(dst, byte);
    }

    fn oam_bug(&mut self, addr: u16, access: OamAccess) {
        if addr & 0xFF00 == 0xFE00 && self.model.is_dmg() {
            oam_bug::corrupt(self.ppu, access);
        }
    }

    #[inline(never)]
    fn cycle_with_oam_address(&mut self, addr: u16) {
        self.close_quiet();
        self.tick(None);
        self.oam_bug(addr, OamAccess::Write);
    }

    fn high_read(&mut self, addr: u16, access: OamAccess) {
        if reads_settled(addr) {
            self.settle_for_read(addr);
        }
        self.oam_bug(addr, access);
    }

    #[inline(always)]
    fn try_read_quiet(&mut self, addr: u16) -> Option<u8> {
        if addr < 0x8000 {
            if !self.take_quiet_cycle() {
                return None;
            }
            #[cfg(feature = "debug")]
            if self.debugger.breakpoints.watching() {
                self.watch(addr, crate::debug::breakpoints::Access::Read);
            }
            let value = self.cartridge.rom_window_byte(&self.rom_window, addr);
            Some(value.unwrap_or_else(|| self.read_naive(addr)))
        } else if (0xC000..0xFE00).contains(&addr) {
            if !self.take_quiet_cycle() {
                return None;
            }
            #[cfg(feature = "debug")]
            if self.debugger.breakpoints.watching() {
                self.watch(addr, crate::debug::breakpoints::Access::Read);
            }
            Some(self.memory.read_wram(addr))
        } else if (0xFF80..0xFFFF).contains(&addr) && self.take_quiet_cycle() {
            #[cfg(feature = "debug")]
            if self.debugger.breakpoints.watching() {
                self.watch(addr, crate::debug::breakpoints::Access::Read);
            }
            Some(self.memory.read_hram(addr))
        } else {
            None
        }
    }

    #[inline(always)]
    fn quiet_fetch_byte(&self, addr: u16) -> Option<u8> {
        if let Some(value) = self.cartridge.rom_window_byte(&self.rom_window, addr) {
            return Some(value);
        }
        if (0xC000..0xFE00).contains(&addr) {
            Some(self.memory.read_wram(addr))
        } else if (0xFF80..0xFFFF).contains(&addr) {
            Some(self.memory.read_hram(addr))
        } else {
            None
        }
    }

    #[inline(always)]
    fn pop_quiet_with_fetch(
        &mut self,
        sp: u16,
        cycles: u32,
        at: Option<u16>,
    ) -> Option<(u16, Option<u8>)> {
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            return None;
        }
        if !WINDOWS || self.quiet_left <= cycles {
            return None;
        }
        let value = if (0xC000..0xFDFF).contains(&sp) {
            self.memory.read_wram_pair(sp)
        } else if (0xFF80..0xFFFE).contains(&sp) {
            self.memory.read_hram_pair(sp)
        } else {
            return None;
        };
        let Some(ir) = self.quiet_fetch_byte(at.unwrap_or(value)) else {
            self.quiet_left -= cycles;
            return Some((value, None));
        };
        self.quiet_left -= cycles + 1;
        Some((value, Some(ir)))
    }

    fn refresh_rom_window(&mut self) {
        if WINDOWS {
            self.rom_window = mapped_rom_window::<WINDOWS>(self.boot_rom, self.cartridge);
        }
    }

    #[inline(never)]
    fn read_rom_open(&mut self, addr: u16) -> u8 {
        let parts_quiet = self.tick_open_inline();
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            self.watch(addr, crate::debug::breakpoints::Access::Read);
        }
        if (parts_quiet || !self.dma.active)
            && let Some(value) = self.cartridge.rom_window_byte(&self.rom_window, addr)
        {
            return value;
        }
        self.read_now(addr)
    }

    #[inline(never)]
    fn read_open(&mut self, addr: u16, access: OamAccess) -> u8 {
        if !WINDOWS {
            return self.read_linked(addr, access);
        }
        if self.dma_room > 0 && (0xFF80..0xFFFF).contains(&addr) && self.take_dma_cycle() {
            #[cfg(feature = "debug")]
            if self.debugger.breakpoints.watching() {
                self.watch(addr, crate::debug::breakpoints::Access::Read);
            }
            return self.memory.read_quiet(addr);
        }
        self.cycle_for_read(addr);
        if addr >= 0xFE00 {
            self.high_read(addr, access);
        }
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            self.watch(addr, crate::debug::breakpoints::Access::Read);
        }
        self.read_now(addr)
    }

    #[inline(always)]
    fn read_linked(&mut self, addr: u16, access: OamAccess) -> u8 {
        self.tick_inline(None);
        if addr < 0x8000 {
            #[cfg(feature = "debug")]
            if self.debugger.breakpoints.watching() {
                self.watch(addr, crate::debug::breakpoints::Access::Read);
            }
            if !self.boot_rom.mounted && !self.dma.active {
                return self.cartridge.read_rom(addr);
            }
            return self.read_now(addr);
        }
        if quiet_write(addr) && !self.dma.cpu_conflicts(addr) {
            #[cfg(feature = "debug")]
            if self.debugger.breakpoints.watching() {
                self.watch(addr, crate::debug::breakpoints::Access::Read);
            }
            return self.memory.read_quiet(addr);
        }
        if addr >= 0xFE00 {
            self.high_read(addr, access);
        }
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            self.watch(addr, crate::debug::breakpoints::Access::Read);
        }
        self.read_now(addr)
    }

    #[inline(never)]
    fn read_pair_linked(&mut self, addr: u16) -> (u8, u8) {
        let low = self.read_linked(addr, OamAccess::Read);
        let high = self.read_linked(addr.wrapping_add(1), OamAccess::Read);
        (low, high)
    }

    #[inline(never)]
    fn pop_pair_linked(&mut self, sp: u16) -> (u8, u8) {
        let low = self.read_linked(sp, OamAccess::ReadIncrement);
        let high = self.read_linked(sp.wrapping_add(1), OamAccess::Read);
        (low, high)
    }

    #[inline(never)]
    fn push_pair_linked(&mut self, sp: u16, value: u16) {
        self.write_linked(sp.wrapping_sub(1), hi(value));
        self.write_linked(sp.wrapping_sub(2), lo(value));
    }

    #[cfg(feature = "debug")]
    #[inline(always)]
    fn watch_write(&mut self, addr: u16, value: u8) {
        if self.debugger.breakpoints.watching() {
            use crate::debug::breakpoints::Access;
            self.close_quiet();
            if reads_settled(addr) {
                self.settle_for_read(addr);
            }
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
    }

    #[inline(always)]
    fn write_linked(&mut self, addr: u16, value: u8) {
        #[cfg(feature = "debug")]
        self.watch_write(addr, value);
        let ram = quiet_write(addr);
        let scheduled =
            !ram && !self.dma.cpu_conflicts(addr) && self.ppu.schedule_write(addr, value);
        self.tick_inline(None);
        if ram {
            if !self.dma.cpu_conflicts(addr) {
                self.memory.write_quiet(addr, value);
            }
        } else if !scheduled {
            self.oam_bug(addr, OamAccess::Write);
            if !self.dma.cpu_conflicts(addr) && !self.ppu.cpu_write_blocked(addr) {
                self.write_naive(addr, value);
            }
        }
    }

    #[inline(never)]
    fn write_open(&mut self, addr: u16, value: u8) {
        if !WINDOWS {
            self.write_linked(addr, value);
            return;
        }
        #[cfg(feature = "debug")]
        self.watch_write(addr, value);
        if addr < 0x8000 && self.take_quiet_cycle() {
            if self.cartridge.clocked() {
                self.apply_quiet();
            }
            self.cartridge.write_naive(addr, value);
            self.refresh_rom_window();
            return;
        }
        if addr & 0xE000 == 0x8000 && self.take_quiet_cycle() {
            self.close_quiet();
            if !self.ppu.cpu_write_blocked(addr) {
                self.ppu.write_naive(addr, value);
            }
            let window = self.clock_window();
            self.open_window(window);
            return;
        }
        if quiet_io_write(addr) && self.take_quiet_cycle() {
            self.close_quiet();
            self.write_naive(addr, value);
            self.ticked = true;
            self.reopen_quiet();
            return;
        }
        self.close_quiet();
        if !self.dma.cpu_conflicts(addr) && self.ppu.schedule_write(addr, value) {
            self.tick(None);
            return;
        }
        self.tick(None);
        self.oam_bug(addr, OamAccess::Write);
        if !self.dma.cpu_conflicts(addr) && !self.ppu.cpu_write_blocked(addr) {
            self.write_naive(addr, value);
        }
        self.reopen_quiet();
    }

    #[inline(always)]
    fn reopen_quiet(&mut self) {
        if WINDOWS {
            let window = if self.parts_noisy {
                self.quiet_window()
            } else {
                self.clock_window()
            };
            self.open_window(window);
        }
    }

    fn read_now(&self, addr: u16) -> u8 {
        if addr < 0x8000 && !self.boot_rom.mounted && !self.dma.active {
            return self.cartridge.read_rom(addr);
        }

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

    fn clock_cycle(&mut self) {
        self.settle_clock();
        let old_div = self.timer.div;
        self.timer.cycle(self.ic);
        self.serial
            .div_changed(old_div, self.timer.div, self.cgb.cgb_mode, self.ic);
        let lazy = self.timer.quiet_cycles().min(
            self.serial
                .edge_free_cycles(self.timer.div, self.cgb.cgb_mode),
        );
        self.timer.set_lazy(lazy);
    }

    fn settle_clock(&mut self) {
        self.timer.settle(self.serial, self.cgb.cgb_mode);
    }

    fn settle_for_read(&mut self, addr: u16) {
        if addr <= 0xFF07 {
            self.settle_clock();
        } else {
            self.settle_apu();
        }
    }

    fn settle_apu(&mut self) {
        self.settle_clock();
        self.apu.settle(
            self.timer.div,
            self.cgb.double_speed,
            #[cfg(feature = "debug")]
            self.debugger,
        );
    }

    fn reset_div(&mut self) {
        self.settle_apu();
        let old = self.timer.div;
        self.timer.write_naive(0xFF04, 0);
        self.serial
            .div_changed(old, self.timer.div, self.cgb.cgb_mode, self.ic);
        self.timer.set_lazy(0);
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

impl<const WINDOWS: bool> CpuBusInterface for CpuBus<'_, WINDOWS> {
    const LINKED: bool = !WINDOWS;

    fn cycle(&mut self) {
        if !WINDOWS {
            self.tick(None);
        } else if !self.take_quiet_cycle() {
            self.cycle_open();
        }
    }

    fn cycle_halted(&mut self, sample_dot: u8) -> Option<bool> {
        self.close_quiet();
        Some(self.tick_inline(Some(sample_dot)))
    }

    fn acknowledge_in_next_cycle(&mut self, interrupt: Interrupt) {
        if WINDOWS && self.quiet_left > 0 {
            self.ic.acknowledge_interrupt(interrupt);
            return;
        }
        self.close_quiet();
        self.acknowledge = Some(interrupt);
        self.parts_noisy = true;
    }

    fn halt_ended(&mut self) {
        self.close_quiet();
        self.hdma
            .left_halt(self.ppu.stat.ppu_mode == PpuMode::HBlank);
        self.hdma_due = true;
        if WINDOWS {
            let window = if self.parts_noisy {
                self.quiet_window()
            } else {
                self.clock_window()
            };
            self.open_window(window);
        }
    }

    fn keep_halting(&self) -> bool {
        !self.ppu.frame_ready && !self.serial.awaiting()
    }

    fn skip_halted(&mut self, budget: u32) -> u32 {
        self.close_quiet();
        if self.ic.has_pending_interrupt() {
            return 0;
        }
        let cycles = self.quiet_window().min(budget);
        if cycles < 2 {
            return 0;
        }
        self.advance_quiet(cycles, self.cgb.double_speed);
        cycles
    }

    #[inline(always)]
    fn run_halted<const BOUNDED: bool>(
        &mut self,
        left: &mut u32,
        sample_dot: u8,
        cgb: bool,
    ) -> HaltedRun {
        self.close_quiet();
        if !self.check_parts_quiet() {
            return if !WINDOWS && self.serial.plugged() {
                HaltedRun::Plugged
            } else {
                HaltedRun::Noisy
            };
        }
        let double_speed = self.cgb.double_speed;
        loop {
            let pending_before = self.ic.has_pending_interrupt();
            if (!BOUNDED || *left > 1) && !pending_before {
                let window = self.clock_window_at(double_speed);
                let cycles = if BOUNDED {
                    window.min(*left - 1)
                } else {
                    window
                };
                if cycles >= 2 {
                    self.advance_quiet(cycles, double_speed);
                    if BOUNDED {
                        *left -= cycles;
                    }
                }
            }
            let pending = self.tick_parts_at::<true>(Some(sample_dot), double_speed);
            let pending = if cgb { pending_before } else { pending };
            if BOUNDED {
                *left = left.saturating_sub(1);
            }
            if pending {
                return HaltedRun::Woke;
            }
            if (BOUNDED && *left == 0) || self.ppu.frame_ready {
                return HaltedRun::Stopped;
            }
        }
    }

    #[inline(always)]
    fn service_hdma(&mut self) {
        if self.hdma_due {
            self.hdma_due = false;
            if self.hdma.is_pending() {
                self.run_hdma();
            }
        }
    }

    fn speed_switch(&mut self, ime: bool) -> bool {
        if !self.cgb.speed_switch_armed() {
            return false;
        }
        self.close_quiet();
        self.ticked = true;
        self.settle_apu();
        self.cgb.switch_speed(!ime);
        self.parts_noisy = true;
        self.ppu.speed_switched(self.cgb.double_speed, self.ic);
        true
    }

    #[inline(always)]
    fn cycle_with_address(&mut self, addr: u16) {
        if addr & 0xFF00 != 0xFE00 {
            self.quiet_cycle();
            return;
        }
        self.cycle_with_oam_address(addr);
    }

    #[inline(always)]
    fn read_increment(&mut self, addr: u16) -> u8 {
        if let Some(value) = self.try_read_quiet(addr) {
            return value;
        }
        if WINDOWS && addr < 0x8000 {
            return self.read_rom_open(addr);
        }
        self.read_open(addr, OamAccess::ReadIncrement)
    }

    #[inline(always)]
    fn read(&mut self, addr: u16) -> u8 {
        if let Some(value) = self.try_read_quiet(addr) {
            return value;
        }
        if WINDOWS && addr < 0x8000 {
            return self.read_rom_open(addr);
        }
        self.read_open(addr, OamAccess::Read)
    }

    #[inline(always)]
    fn read_program_byte_and_cycle(&mut self, addr: u16) -> Option<u8> {
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            return None;
        }
        if !WINDOWS || self.quiet_left < 2 {
            return None;
        }
        let value = self.cartridge.rom_window_byte(&self.rom_window, addr)?;
        self.quiet_left -= 2;
        Some(value)
    }

    #[inline(always)]
    fn jump_relative_quiet(&mut self, pc: u16) -> Option<(u16, u8)> {
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            return None;
        }
        if !WINDOWS || self.quiet_left < 3 {
            return None;
        }
        let offset = self.cartridge.rom_window_byte(&self.rom_window, pc)?;
        let (target, _, _) = add_word_signed_byte(pc.wrapping_add(1), offset as i8);
        let ir = self.cartridge.rom_window_byte(&self.rom_window, target)?;
        self.quiet_left -= 3;
        Some((target, ir))
    }

    #[inline(always)]
    fn write(&mut self, addr: u16, value: u8) {
        #[cfg(feature = "debug")]
        let watching = self.debugger.breakpoints.watching();
        #[cfg(not(feature = "debug"))]
        let watching = false;
        if !watching {
            if (0xC000..0xFE00).contains(&addr) {
                if self.take_quiet_cycle() {
                    self.memory.write_wram(addr, value);
                    return;
                }
            } else if (0xFF80..0xFFFF).contains(&addr) && self.take_quiet_cycle() {
                self.memory.write_hram(addr, value);
                return;
            }
        }
        self.write_open(addr, value);
    }

    #[inline(always)]
    fn pop_quiet(&mut self, sp: u16) -> Option<u16> {
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            return None;
        }
        if (0xC000..0xFDFF).contains(&sp) {
            if !self.take_quiet_pair() {
                return None;
            }
            return Some(self.memory.read_wram_pair(sp));
        }
        if !(0xFF80..0xFFFE).contains(&sp) || !self.take_quiet_pair() {
            return None;
        }
        Some(self.memory.read_hram_pair(sp))
    }

    #[inline(always)]
    fn pop_and_fetch_quiet(&mut self, sp: u16, pc: u16) -> Option<(u16, Option<u8>)> {
        self.pop_quiet_with_fetch(sp, 2, Some(pc))
    }

    #[inline(always)]
    fn return_and_fetch_quiet(&mut self, sp: u16) -> Option<(u16, Option<u8>)> {
        self.pop_quiet_with_fetch(sp, 3, None)
    }

    #[inline(always)]
    fn read_pair_quiet(&mut self, addr: u16) -> Option<u16> {
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            return None;
        }
        if !WINDOWS || self.quiet_left < 2 {
            return None;
        }
        let value = if addr < 0x8000 {
            self.cartridge.rom_window_pair(&self.rom_window, addr)?
        } else if quiet_pair(addr) {
            self.memory.read_quiet_pair(addr)
        } else {
            return None;
        };
        self.quiet_left -= 2;
        Some(value)
    }

    #[inline(always)]
    fn read_pair_open(&mut self, addr: u16) -> (u8, u8) {
        if !WINDOWS {
            return self.read_pair_linked(addr);
        }
        let low = self.read(addr);
        (low, self.read(addr.wrapping_add(1)))
    }

    #[inline(always)]
    fn pop_pair_open(&mut self, sp: u16) -> (u8, u8) {
        if !WINDOWS {
            return self.pop_pair_linked(sp);
        }
        let low = self.read_increment(sp);
        (low, self.read(sp.wrapping_add(1)))
    }

    #[inline(always)]
    fn push_pair_open(&mut self, sp: u16, value: u16) {
        if !WINDOWS {
            self.push_pair_linked(sp, value);
            return;
        }
        self.write(sp.wrapping_sub(1), hi(value));
        self.write(sp.wrapping_sub(2), lo(value));
    }

    #[inline(always)]
    fn push_quiet(&mut self, sp: u16, value: u16) -> bool {
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            return false;
        }
        let low = sp.wrapping_sub(2);
        if (0xC000..0xFDFF).contains(&low) {
            if !self.take_quiet_pair() {
                return false;
            }
            self.memory.write_wram_pair(low, value);
            return true;
        }
        if !(0xFF80..0xFFFE).contains(&low) || !self.take_quiet_pair() {
            return false;
        }
        self.memory.write_hram_pair(low, value);
        true
    }

    #[inline(always)]
    fn dispatch_quiet(&mut self, sp: u16, value: u16) -> Option<(u16, u8)> {
        #[cfg(feature = "debug")]
        if self.debugger.breakpoints.watching() {
            return None;
        }
        let low = sp.wrapping_sub(2);
        if !WINDOWS || self.quiet_left < 5 || sp & 0xFF00 == 0xFE00 || !quiet_pair(low) {
            return None;
        }
        let interrupt = self.ic.next_interrupt(self.ic.enabled_interrupts());
        let vector = interrupt.map_or(0x0000, |interrupt| interrupt.vector());
        let ir = self.cartridge.rom_window_byte(&self.rom_window, vector)?;
        self.quiet_left -= 5;
        if low >= 0xFF80 {
            self.memory.write_hram_pair(low, value);
        } else {
            self.memory.write_wram_pair(low, value);
        }
        if let Some(interrupt) = interrupt {
            self.ic.acknowledge_interrupt(interrupt);
        }
        Some((vector, ir))
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
        self.settle_quiet();
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaltedRun {
    Unhandled,
    Noisy,
    Plugged,
    Woke,
    Stopped,
}

pub trait CpuBusInterface {
    const LINKED: bool = false;

    fn cycle(&mut self);

    fn cycle_halted(&mut self, _sample_dot: u8) -> Option<bool> {
        self.cycle();
        None
    }

    fn halt_ended(&mut self) {}

    fn keep_halting(&self) -> bool {
        false
    }

    fn skip_halted(&mut self, _budget: u32) -> u32 {
        0
    }

    fn run_halted<const BOUNDED: bool>(
        &mut self,
        _left: &mut u32,
        _sample_dot: u8,
        _cgb: bool,
    ) -> HaltedRun {
        HaltedRun::Unhandled
    }

    fn acknowledge_in_next_cycle(&mut self, _interrupt: Interrupt) {}

    fn service_hdma(&mut self) {}

    fn speed_switch(&mut self, _ime: bool) -> bool {
        false
    }

    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, value: u8);

    fn read_program_byte_and_cycle(&mut self, _addr: u16) -> Option<u8> {
        None
    }

    fn jump_relative_quiet(&mut self, _pc: u16) -> Option<(u16, u8)> {
        None
    }

    fn cycle_with_address(&mut self, _addr: u16) {
        self.cycle();
    }

    fn read_increment(&mut self, addr: u16) -> u8 {
        self.read(addr)
    }

    fn pop_quiet(&mut self, _sp: u16) -> Option<u16> {
        None
    }

    fn pop_and_fetch_quiet(&mut self, _sp: u16, _pc: u16) -> Option<(u16, Option<u8>)> {
        None
    }

    fn return_and_fetch_quiet(&mut self, _sp: u16) -> Option<(u16, Option<u8>)> {
        None
    }

    fn push_quiet(&mut self, _sp: u16, _value: u16) -> bool {
        false
    }

    fn dispatch_quiet(&mut self, _sp: u16, _value: u16) -> Option<(u16, u8)> {
        None
    }

    fn read_pair_quiet(&mut self, _addr: u16) -> Option<u16> {
        None
    }

    fn read_pair_open(&mut self, addr: u16) -> (u8, u8) {
        let low = self.read(addr);
        (low, self.read(addr.wrapping_add(1)))
    }

    fn pop_pair_open(&mut self, sp: u16) -> (u8, u8) {
        let low = self.read_increment(sp);
        (low, self.read(sp.wrapping_add(1)))
    }

    fn push_pair_open(&mut self, sp: u16, value: u16) {
        self.write(sp.wrapping_sub(1), hi(value));
        self.write(sp.wrapping_sub(2), lo(value));
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

impl<const WINDOWS: bool> ICInterface for CpuBus<'_, WINDOWS> {
    fn request_interrupt(&mut self, interrupt: crate::gb::ic::Interrupt) {
        self.ic.request_interrupt(interrupt);
    }

    fn enabled_interrupts(&self) -> u8 {
        self.ic.enabled_interrupts()
    }

    fn next_interrupt(&self, enabled: u8) -> Option<crate::gb::ic::Interrupt> {
        self.ic.next_interrupt(enabled)
    }

    fn acknowledge_interrupt(&mut self, interrupt: Interrupt) {
        self.ic.acknowledge_interrupt(interrupt);
    }

    fn has_pending_interrupt(&self) -> bool {
        self.ic.has_pending_interrupt()
    }
}

#[cfg(feature = "debug")]
impl<const WINDOWS: bool> crate::debug::DebuggerAccess for CpuBus<'_, WINDOWS> {
    fn debugger(&self) -> &dyn crate::debug::DebuggerInterface {
        self.debugger
    }

    fn debugger_mut(&mut self) -> &mut dyn crate::debug::DebuggerInterface {
        self.debugger
    }
}

#[cfg(test)]
mod tests {
    use super::{CpuBus, CpuBusInterface};
    use crate::gb::ic::{ICInterface, Interrupt, InterruptController};
    use crate::gb::serial::Serial;
    use crate::gb::timer::Timer;
    use crate::gb::{GameBoy, GbModel};
    use crate::{ReadMemory, WriteMemory};

    struct Reference {
        timer: Timer,
        serial: Serial,
        ic: InterruptController,
        cgb_mode: bool,
    }

    impl Reference {
        fn cycle(&mut self) {
            let old = self.timer.div;
            self.timer.cycle(&mut self.ic);
            self.serial
                .div_changed(old, self.timer.div, self.cgb_mode, &mut self.ic);
            self.serial.tick(false, &mut self.ic);
        }

        fn read(&mut self, addr: u16) -> u8 {
            self.cycle();
            match addr {
                0xFF01 => self.serial.sb,
                0xFF02 => self.serial.read_sc(self.cgb_mode),
                _ => self.timer.read_naive(addr),
            }
        }

        fn write(&mut self, addr: u16, value: u8) {
            self.cycle();
            match addr {
                0xFF01 => self.serial.sb = value,
                0xFF02 => self.serial.write_sc(value, self.cgb_mode, &mut self.ic),
                0xFF04 => {
                    let old = self.timer.div;
                    self.timer.write_naive(addr, value);
                    self.serial
                        .div_changed(old, self.timer.div, self.cgb_mode, &mut self.ic);
                }
                _ => self.timer.write_naive(addr, value),
            }
        }
    }

    #[test]
    fn lazy_timer_and_serial_clock_match_cycling_one_by_one() {
        const REGS: [u16; 6] = [0xFF01, 0xFF02, 0xFF04, 0xFF05, 0xFF06, 0xFF07];
        for (model, cgb_mode) in [(GbModel::Dmg, false), (GbModel::Cgb, true)] {
            let mut gb = GameBoy::new(model, None, 0);
            gb.cgb.cgb_mode = cgb_mode;
            let mut reference = Reference {
                timer: gb.timer.clone(),
                serial: gb.serial.clone(),
                ic: InterruptController::new(),
                cgb_mode,
            };
            let mut seed = 0x2545_F491u32;
            let mut next = |below: u32| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed % below
            };
            let mut timer_irqs = 0;
            gb.with_bus(|bus| {
                for step in 0..20_000 {
                    let at = format!("{model:?} step {step}");
                    match next(8) {
                        0..=3 => {
                            let addr = REGS[next(6) as usize];
                            assert_eq!(
                                bus.read(addr),
                                reference.read(addr),
                                "{at} read {addr:04X}"
                            );
                        }
                        4 | 5 => {
                            let addr = REGS[next(6) as usize];
                            let value = match addr {
                                0xFF02 => [0x00, 0x01, 0x80, 0x81, 0x83][next(5) as usize],
                                0xFF05 | 0xFF06 => 0xF0 | next(16) as u8,
                                _ => next(256) as u8,
                            };
                            bus.write(addr, value);
                            reference.write(addr, value);
                        }
                        6 => {
                            for _ in 0..next(400) {
                                bus.cycle();
                                reference.cycle();
                            }
                        }
                        _ => {
                            bus.ic.flag = 0.into();
                            reference.ic.flag = 0.into();
                        }
                    }
                    let flags = |flag: u8| flag & 0x0C;
                    let seen = flags(bus.ic.flag.into());
                    assert_eq!(seen, flags(reference.ic.flag.into()), "{at} flags");
                    timer_irqs += usize::from(seen & 0x04 != 0);
                }
            });
            assert!(timer_irqs > 0, "{model:?}");
        }
    }

    struct PerTick<'a, 'b>(&'a mut CpuBus<'b>);

    impl<'b> PerTick<'_, 'b> {
        fn ticked<R>(&mut self, f: impl FnOnce(&mut CpuBus<'b>) -> R) -> R {
            let out = f(&mut *self.0);
            self.0.close_quiet();
            out
        }
    }

    impl CpuBusInterface for PerTick<'_, '_> {
        fn cycle(&mut self) {
            self.ticked(|bus| bus.cycle())
        }

        fn cycle_halted(&mut self, sample_dot: u8) -> Option<bool> {
            self.ticked(|bus| bus.cycle_halted(sample_dot))
        }

        fn halt_ended(&mut self) {
            self.ticked(|bus| bus.halt_ended())
        }

        fn keep_halting(&self) -> bool {
            self.0.keep_halting()
        }

        fn skip_halted(&mut self, budget: u32) -> u32 {
            self.ticked(|bus| bus.skip_halted(budget))
        }

        fn acknowledge_in_next_cycle(&mut self, interrupt: Interrupt) {
            self.ticked(|bus| bus.acknowledge_in_next_cycle(interrupt))
        }

        fn service_hdma(&mut self) {
            self.ticked(|bus| bus.service_hdma())
        }

        fn speed_switch(&mut self, ime: bool) -> bool {
            self.ticked(|bus| bus.speed_switch(ime))
        }

        fn read(&mut self, addr: u16) -> u8 {
            self.ticked(|bus| bus.read(addr))
        }

        fn write(&mut self, addr: u16, value: u8) {
            self.ticked(|bus| bus.write(addr, value))
        }

        fn cycle_with_address(&mut self, addr: u16) {
            self.ticked(|bus| bus.cycle_with_address(addr))
        }

        fn read_increment(&mut self, addr: u16) -> u8 {
            self.ticked(|bus| bus.read_increment(addr))
        }

        #[cfg(feature = "debug")]
        fn on_fetch(&mut self, addr: u16) {
            self.0.on_fetch(addr)
        }

        #[cfg(feature = "debug")]
        fn on_instruction(&mut self, cpu: &crate::gb::cpu::Cpu) {
            self.0.on_instruction(cpu)
        }
    }

    impl ICInterface for PerTick<'_, '_> {
        fn request_interrupt(&mut self, interrupt: Interrupt) {
            self.0.request_interrupt(interrupt)
        }

        fn enabled_interrupts(&self) -> u8 {
            self.0.enabled_interrupts()
        }

        fn next_interrupt(&self, enabled: u8) -> Option<Interrupt> {
            self.0.next_interrupt(enabled)
        }

        fn acknowledge_interrupt(&mut self, interrupt: Interrupt) {
            self.0.acknowledge_interrupt(interrupt)
        }

        fn has_pending_interrupt(&self) -> bool {
            self.0.has_pending_interrupt()
        }
    }

    #[cfg(feature = "debug")]
    impl crate::debug::DebuggerInterface for PerTick<'_, '_> {}

    #[cfg(feature = "serde")]
    #[test]
    fn quiet_windows_match_ticking_one_by_one() {
        use crate::console::Rollback;
        use crate::rom::Rom;
        let games: [(&[u8], GbModel); 3] = [
            (
                include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb"),
                GbModel::Dmg,
            ),
            (
                include_bytes!("../../../citrine-gb-app/homebrew/shock_lobster.gb"),
                GbModel::Dmg,
            ),
            (
                include_bytes!("../../../citrine-gb-app/homebrew/ucity.gbc"),
                GbModel::Cgb,
            ),
        ];
        for (data, model) in games {
            let load = || {
                let mut gb = GameBoy::new_empty(model);
                gb.load_rom_with_model(&Rom::new(data), model)
                    .expect("load");
                gb
            };
            let mut windowed = load();
            let mut ticked = load();
            let mut scratch = windowed.clone_state();
            for burst in 0..60 {
                for (gb, per_tick) in [(&mut windowed, false), (&mut ticked, true)] {
                    gb.ppu.frame_ready = false;
                    let mut cpu = gb.cpu.clone();
                    gb.with_bus(|bus| {
                        for _ in 0..10_000 {
                            if per_tick {
                                cpu.step_halting(&mut PerTick(bus), u32::MAX);
                            } else {
                                cpu.step_halting(bus, u32::MAX);
                            }
                        }
                        bus.close_quiet();
                        bus.settle_apu();
                    });
                    gb.cpu = cpu;
                }
                let at = format!("{model:?} burst {burst}");
                assert_eq!(windowed.cycle_counter, ticked.cycle_counter, "{at}");
                assert_eq!(
                    windowed.state_hash(&mut scratch),
                    ticked.state_hash(&mut scratch),
                    "{at}"
                );
            }
        }
    }

    #[test]
    fn a_19_block_gdma_costs_the_write_155_cycles() {
        let program = [
            0x3E, 0xC0, 0xE0, 0x51, 0xAF, 0xE0, 0x52, 0x3E, 0x80, 0xE0, 0x53, 0xAF, 0xE0, 0x54,
            0x0E, 0x55, 0x3E, 0x12, 0xE2, 0x18, 0xFE,
        ];
        let mut rom = vec![0u8; 0x900];
        rom[..program.len()].copy_from_slice(&program);
        let mut gb = GameBoy::new(GbModel::Cgb, Some(rom), 0);
        let mut longest = 0;
        for _ in 0..20 {
            let start = gb.cycle_counter;
            gb.step();
            longest = longest.max(gb.cycle_counter - start);
        }
        assert_eq!(longest, 155);
    }

    #[test]
    fn jumps_direct_loads_and_stack_words_match_ticking_one_by_one() {
        use crate::rom::Rom;
        let mut data = vec![0u8; 0x8000];
        data[0x0040..0x004A]
            .copy_from_slice(&[0xF5, 0xFA, 0x00, 0xC2, 0x3C, 0xEA, 0x00, 0xC2, 0xF1, 0xD9]);
        data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
        let program = [
            0x31, 0xFE, 0xDF, 0x3E, 0x91, 0xE0, 0x40, 0x3E, 0x01, 0xE0, 0xFF, 0xFB, 0x21, 0x00,
            0xC0, 0x06, 0x20, 0x2A, 0x3C, 0xEA, 0x00, 0xC1, 0xFA, 0x00, 0xC1, 0xC5, 0xCD, 0x80,
            0x01, 0xC1, 0x05, 0x20, 0xF0, 0x7D, 0xE6, 0x3F, 0x6F, 0x18, 0xE8,
        ];
        data[0x0150..0x0150 + program.len()].copy_from_slice(&program);
        data[0x0180..0x0183].copy_from_slice(&[0x0C, 0xC0, 0xC9]);
        let rom = Rom::new(&data);
        for model in [GbModel::Dmg, GbModel::Cgb] {
            let load = || {
                let mut gb = GameBoy::new_empty(model);
                gb.load_rom_with_model(&rom, model).expect("load");
                gb
            };
            let mut windowed = load();
            let mut ticked = load();
            for burst in 0..40 {
                for (gb, per_tick) in [(&mut windowed, false), (&mut ticked, true)] {
                    let mut cpu = gb.cpu.clone();
                    gb.with_bus(|bus| {
                        for _ in 0..2_000 {
                            if per_tick {
                                cpu.step_halting(&mut PerTick(bus), u32::MAX);
                            } else {
                                cpu.step_halting(bus, u32::MAX);
                            }
                        }
                        bus.close_quiet();
                        bus.settle_apu();
                    });
                    gb.cpu = cpu;
                }
                let at = format!("{model:?} burst {burst}");
                assert_eq!(windowed.cpu, ticked.cpu, "{at}");
                assert_eq!(windowed.cycle_counter, ticked.cycle_counter, "{at}");
                assert_eq!(windowed.timer.div, ticked.timer.div, "{at}");
                assert_eq!(windowed.ppu.ly, ticked.ppu.ly, "{at}");
                assert_eq!(windowed.ic.flag, ticked.ic.flag, "{at}");
                for addr in [0xC100, 0xC200, 0xDFF8, 0xDFF9, 0xDFFA, 0xDFFB] {
                    assert_eq!(
                        windowed.with_bus(|bus| bus.read_naive(addr)),
                        ticked.with_bus(|bus| bus.read_naive(addr)),
                        "{at} {addr:04X}"
                    );
                }
            }
            assert!(
                windowed.with_bus(|bus| bus.read_naive(0xC200)) > 0,
                "{model:?}"
            );
        }
    }

    #[test]
    fn stack_words_in_ram_code_match_ticking_one_by_one() {
        use crate::rom::Rom;
        let mut data = vec![0u8; 0x8000];
        data[0x0040..0x004A]
            .copy_from_slice(&[0xF5, 0xFA, 0x00, 0xC2, 0x3C, 0xEA, 0x00, 0xC2, 0xF1, 0xD9]);
        data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
        let program = [
            0x31, 0xFE, 0xDF, 0x3E, 0x91, 0xE0, 0x40, 0x3E, 0x01, 0xE0, 0xFF, 0x21, 0x90, 0x01,
            0x11, 0x00, 0xC3, 0x06, 0x08, 0x2A, 0x12, 0x13, 0x05, 0x20, 0xFA, 0x21, 0xA0, 0x01,
            0x0E, 0x90, 0x06, 0x06, 0x2A, 0xE2, 0x0C, 0x05, 0x20, 0xFA, 0xFB, 0xCD, 0x00, 0xC3,
            0xCD, 0x90, 0xFF, 0x18, 0xF8,
        ];
        data[0x0150..0x0150 + program.len()].copy_from_slice(&program);
        data[0x0180..0x0183].copy_from_slice(&[0x0C, 0xC0, 0xC9]);
        data[0x0190..0x0198].copy_from_slice(&[0xC5, 0xC1, 0xCD, 0x80, 0x01, 0x0C, 0xC0, 0xC9]);
        data[0x01A0..0x01A6].copy_from_slice(&[0xF5, 0xF1, 0xCD, 0x80, 0x01, 0xC9]);
        for model in [GbModel::Dmg, GbModel::Cgb] {
            for stack_page in [0xDF, 0xFF] {
                data[0x0152] = stack_page;
                let rom = Rom::new(&data);
                let load = || {
                    let mut gb = GameBoy::new_empty(model);
                    gb.load_rom_with_model(&rom, model).expect("load");
                    gb
                };
                let mut windowed = load();
                let mut ticked = load();
                for burst in 0..40 {
                    for (gb, per_tick) in [(&mut windowed, false), (&mut ticked, true)] {
                        let mut cpu = gb.cpu.clone();
                        gb.with_bus(|bus| {
                            for _ in 0..2_000 {
                                if per_tick {
                                    cpu.step_halting(&mut PerTick(bus), u32::MAX);
                                } else {
                                    cpu.step_halting(bus, u32::MAX);
                                }
                            }
                            bus.close_quiet();
                            bus.settle_apu();
                        });
                        gb.cpu = cpu;
                    }
                    let at = format!("{model:?} stack {stack_page:02X} burst {burst}");
                    assert_eq!(windowed.cpu, ticked.cpu, "{at}");
                    assert_eq!(windowed.cycle_counter, ticked.cycle_counter, "{at}");
                    assert_eq!(windowed.timer.div, ticked.timer.div, "{at}");
                    assert_eq!(windowed.ppu.ly, ticked.ppu.ly, "{at}");
                    assert_eq!(windowed.ic.flag, ticked.ic.flag, "{at}");
                    for addr in [
                        0xC200, 0xDFF8, 0xDFF9, 0xDFFA, 0xDFFB, 0xFFF8, 0xFFF9, 0xFFFA,
                    ] {
                        assert_eq!(
                            windowed.with_bus(|bus| bus.read_naive(addr)),
                            ticked.with_bus(|bus| bus.read_naive(addr)),
                            "{at} {addr:04X}"
                        );
                    }
                }
                assert!(
                    windowed.with_bus(|bus| bus.read_naive(0xC200)) > 0,
                    "{model:?} stack {stack_page:02X}"
                );
            }
        }
    }
}
