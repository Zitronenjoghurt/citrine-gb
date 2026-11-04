use crate::console::Keys;
#[cfg(feature = "debug")]
use crate::debug::step::Stop;
use crate::error::GbResult;
use crate::rom::Rom;
use crate::{ReadMemory, WriteMemory};
use ppu::types::framebuffer::Framebuffer;

const DMG_POST_BOOT_DIV: u16 = 0xABC8;
const CGB_POST_BOOT_DIV: u16 = 0x1EA0;
const CGB_DMG_MODE_POST_BOOT_DIV: u16 = 0x2674;

fn post_boot_div(model: GbModel, boot_rom_mounted: bool) -> u16 {
    if boot_rom_mounted {
        0
    } else if model.is_cgb() {
        CGB_POST_BOOT_DIV
    } else {
        DMG_POST_BOOT_DIV
    }
}

macro_rules! cpu_bus {
    ($gb:ident) => {
        bus::CpuBus {
            boot_rom: &mut $gb.boot_rom,
            cartridge: &mut $gb.cartridge,
            cgb: &mut $gb.cgb,
            #[cfg(feature = "debug")]
            debugger: &mut $gb.debugger,
            dma: &mut $gb.dma,
            hdma: &mut $gb.hdma,
            ic: &mut $gb.ic,
            joypad: &mut $gb.joypad,
            memory: &mut $gb.memory,
            ppu: &mut $gb.ppu,
            apu: &mut $gb.apu,
            timer: &mut $gb.timer,
            serial: &mut $gb.serial,
            model: $gb.model,
            cycles: &mut $gb.cycle_counter,
        }
    };
}

pub mod apu;
pub(crate) mod boot_rom;
pub mod bus;
pub mod cartridge;
pub mod cgb;
mod console;
pub mod cpu;
mod dma;
pub mod hdma;
pub mod ic;
pub mod joypad;
pub(crate) mod memory;
mod oam_bug;
pub mod ppu;
pub mod printer;
pub mod ram_init;
#[cfg(feature = "persistence")]
mod rollback;
pub mod serial;
pub mod timer;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GameBoy {
    #[cfg_attr(feature = "serde", serde(skip, default))]
    pub boot_rom: boot_rom::BootRom,
    pub cpu: cpu::Cpu,
    pub cartridge: cartridge::Cartridge,
    #[cfg_attr(feature = "serde", serde(default))]
    pub cgb: cgb::CgbControl,
    #[cfg(feature = "debug")]
    #[cfg_attr(feature = "serde", serde(skip, default))]
    pub debugger: crate::debug::Debugger,
    pub dma: dma::DmaController,
    #[cfg_attr(feature = "serde", serde(default))]
    pub hdma: hdma::Hdma,
    pub ic: ic::InterruptController,
    pub memory: memory::Memory,
    pub timer: timer::Timer,
    pub ppu: ppu::Ppu,
    pub apu: apu::Apu,
    pub joypad: joypad::Joypad,
    #[cfg_attr(feature = "serde", serde(default))]
    pub serial: serial::Serial,
    pub model: GbModel,
    pub cycle_counter: u32,
    pub ram_init: ram_init::RamInit,
}

impl GameBoy {
    pub fn new(model: GbModel, boot_rom: Option<Vec<u8>>, rom_header_checksum: u8) -> Self {
        Self::new_with_ram_init(
            model,
            boot_rom,
            rom_header_checksum,
            ram_init::RamInit::default(),
        )
    }

    pub fn new_with_ram_init(
        model: GbModel,
        boot_rom: Option<Vec<u8>>,
        rom_header_checksum: u8,
        ram_init: ram_init::RamInit,
    ) -> Self {
        let boot_rom = boot_rom::BootRom::new(boot_rom.unwrap_or_default(), model);
        let cpu = if boot_rom.mounted {
            cpu::Cpu::new_with_boot_rom(model)
        } else {
            match model {
                GbModel::Dmg => cpu::Cpu::new_dmg(rom_header_checksum),
                GbModel::Cgb => cpu::Cpu::new_cgb(),
            }
        };
        let mut timer = timer::Timer::new();
        timer.div = post_boot_div(model, boot_rom.mounted);
        let mut apu = apu::Apu::new(model);
        let mut joypad = joypad::Joypad::new();
        if !boot_rom.mounted {
            apu.apply_post_boot();
            if model.is_cgb() {
                joypad.write_naive(0xFF00, 0x30);
            }
        }

        let mut gb = Self {
            boot_rom,
            cpu,
            cartridge: cartridge::Cartridge::new(),
            cgb: cgb::CgbControl::new(model),
            #[cfg(feature = "debug")]
            debugger: crate::debug::Debugger::new(),
            dma: dma::DmaController::new(model),
            hdma: hdma::Hdma::new(),
            ic: ic::InterruptController::new(),
            memory: memory::Memory::new(model, ram_init),
            timer,
            ppu: ppu::Ppu::new(model),
            apu,
            joypad,
            serial: serial::Serial::new(),
            model,
            cycle_counter: 0,
            ram_init,
        };
        if gb.boot_rom.mounted {
            gb.power_on();
        }
        gb
    }

    fn power_on(&mut self) {
        self.ppu.power_on();
        self.ic.flag = 0xE0.into();
    }

    pub fn new_empty(model: GbModel) -> Self {
        Self::new(model, None, 0x00)
    }

    pub fn new_empty_with_ram_init(model: GbModel, ram_init: ram_init::RamInit) -> Self {
        Self::new_with_ram_init(model, None, 0x00, ram_init)
    }

    pub fn load_rom(&mut self, rom: &Rom) -> GbResult<()> {
        let sample_rate = self.apu.output_sample_rate;
        let muted_channels = self.apu.muted_channels;
        let mut serial = std::mem::take(&mut self.serial);
        let boot_rom = if !self.boot_rom.rom.is_empty() {
            Some(self.boot_rom.rom.clone())
        } else {
            None
        };

        *self = Self::new_with_ram_init(
            self.model,
            boot_rom,
            rom.provided_header_checksum()?,
            self.ram_init,
        );
        serial.reset_keeping_link();
        self.serial = serial;
        self.cartridge.load_rom(rom)?;
        self.apply_post_boot_mode();
        if !self.boot_rom.mounted {
            let logo: Vec<u8> = (0x104..0x134)
                .map(|addr| self.cartridge.read_naive(addr))
                .collect();
            let dmg_tilemap = self.model.is_dmg() || !self.cartridge.header.cgb_mode.is_cgb();
            self.ppu.load_boot_vram(&logo, dmg_tilemap);
        }
        self.apu.set_sample_rate(sample_rate);
        self.apu.muted_channels = muted_channels;
        Ok(())
    }

    pub fn load_rom_with_model(&mut self, rom: &Rom, model: GbModel) -> GbResult<()> {
        self.model = model;
        self.load_rom(rom)
    }

    pub fn frame_cycles(&self) -> u32 {
        if self.cgb.double_speed {
            self.model.frame_cycles() * 2
        } else {
            self.model.frame_cycles()
        }
    }

    fn apply_post_boot_mode(&mut self) {
        if self.boot_rom.mounted || self.model.is_dmg() || self.cartridge.header.cgb_mode.is_cgb() {
            return;
        }

        let header: Vec<u8> = (0..0x150)
            .map(|addr| self.cartridge.read_naive(addr))
            .collect();
        self.cgb.set_dmg_compatibility();
        self.ppu.set_cgb_mode(false);
        self.ppu.load_compat_palettes(&header);
        self.cpu.enter_dmg_compatibility(&header);
        self.timer.div = CGB_DMG_MODE_POST_BOOT_DIV;
        self.ppu.write_naive(0xFF68, 0x88);
        self.ppu.write_naive(0xFF6A, 0x90);
    }

    pub fn load_boot_rom(&mut self, rom: &[u8]) {
        let sample_rate = self.apu.output_sample_rate;
        let muted_channels = self.apu.muted_channels;
        *self = Self::new_with_ram_init(self.model, Some(rom.to_vec()), 0x00, self.ram_init);
        self.apu.set_sample_rate(sample_rate);
        self.apu.muted_channels = muted_channels;
    }

    pub fn step(&mut self) {
        let frame_cycles = self.frame_cycles();
        if self.serial.stall(1, frame_cycles, &mut self.ic) {
            self.cycle_counter = self.cycle_counter.wrapping_add(1);
            return;
        }
        self.cpu.step(&mut cpu_bus!(self));
    }

    #[cfg(feature = "debug")]
    pub(crate) fn write_io(&mut self, addr: u16, value: u8) {
        cpu_bus!(self).write_naive(addr, value);
    }

    #[cfg(test)]
    pub(crate) fn with_bus<R>(&mut self, f: impl FnOnce(&mut bus::CpuBus) -> R) -> R {
        f(&mut cpu_bus!(self))
    }

    pub fn run_frame(&mut self) {
        self.ppu.frame_ready = false;

        while !self.ppu.frame_ready {
            let frame_cycles = self.frame_cycles();
            if self.serial.stall(frame_cycles, frame_cycles, &mut self.ic) {
                break;
            }
            self.step();

            #[cfg(feature = "debug")]
            if self.at_breakpoint() {
                self.debugger.hit_breakpoint = true;
                return;
            }

            if !self.ppu.lcdc.lcd_enabled && self.cycle_counter >= self.frame_cycles() {
                break;
            }
        }

        if self.cycle_counter >= self.frame_cycles() {
            self.cycle_counter -= self.frame_cycles();
        }

        self.apu.flush_audio();
    }

    pub fn run_cycles(&mut self, cycles: u32) {
        self.cycle_counter = 0;
        while self.cycle_counter < cycles {
            self.step();
        }
    }

    #[cfg(feature = "debug")]
    pub fn run_for(&mut self, cycles: u32) -> Stop {
        let start = self.cycle_counter;
        let mut stop = Stop::Finished;
        while self.cycle_counter.wrapping_sub(start) < cycles {
            self.step();
            if let Some(hit) = self.breakpoint_hit() {
                stop = hit;
                break;
            }
        }
        let frame_cycles = self.frame_cycles();
        while self.cycle_counter >= frame_cycles {
            self.cycle_counter -= frame_cycles;
        }
        self.apu.flush_audio();
        stop
    }

    #[cfg(feature = "debug")]
    #[inline]
    pub(crate) fn at_breakpoint(&mut self) -> bool {
        self.debugger.breakpoints.armed() && self.breakpoint_hit().is_some()
    }

    #[cfg(feature = "debug")]
    pub(crate) fn breakpoint_hit(&mut self) -> Option<Stop> {
        if !self.debugger.breakpoints.armed() {
            return None;
        }
        if let Some(stop) = self.debugger.breakpoints.take_pending() {
            return Some(stop);
        }
        let at = self
            .pc_address()
            .filter(|at| self.debugger.breakpoints.contains(at))?;
        let stop = Stop::Breakpoint(at);
        self.debugger.breakpoints.record(stop);
        Some(stop)
    }

    pub fn audio_channels(&self) -> &'static [&'static str] {
        &apu::CHANNEL_NAMES
    }

    pub fn wram_bank(&self, bank: usize) -> Option<&[u8; 0x1000]> {
        self.memory.wram_bank(bank)
    }

    pub fn frame(&self) -> &Framebuffer {
        self.ppu.frame()
    }

    pub fn soft_reset(&mut self) {
        self.boot_rom.soft_reset(self.model);
        if self.boot_rom.mounted {
            self.cpu = cpu::Cpu::new_with_boot_rom(self.model);
        } else {
            self.cpu
                .soft_reset(self.cartridge.header.provided_header_checksum);
        }
        self.cartridge.soft_reset();
        self.cgb = cgb::CgbControl::new(self.model);
        self.dma.soft_reset();
        self.hdma = hdma::Hdma::new();
        self.ic.soft_reset();
        self.memory.soft_reset(self.model, self.ram_init);
        self.timer.soft_reset();
        self.timer.div = post_boot_div(self.model, self.boot_rom.mounted);
        self.ppu.soft_reset();
        let sample_rate = self.apu.output_sample_rate;
        let muted_channels = self.apu.muted_channels;
        self.apu = apu::Apu::new(self.model);
        if !self.boot_rom.mounted {
            self.apu.apply_post_boot();
        }
        self.apu.set_sample_rate(sample_rate);
        self.apu.muted_channels = muted_channels;
        self.serial.reset_keeping_link();
        self.cycle_counter = 0;
        if self.boot_rom.mounted {
            self.power_on();
        }
        self.apply_post_boot_mode();
        #[cfg(feature = "debug")]
        {
            self.debugger.soft_reset();
        }
    }

    pub fn plug_link(&mut self) {
        self.serial.plug();
    }

    pub fn unplug_link(&mut self) {
        self.serial.unplug(&mut self.ic);
    }

    pub fn link_plugged(&self) -> bool {
        self.serial.plugged()
    }

    pub fn link_waiting(&self) -> bool {
        self.serial.waiting()
    }

    pub fn link_listening_frames(&self) -> u32 {
        self.serial.listening_frames()
    }

    pub fn set_link_timeout_frames(&mut self, frames: u32) {
        self.serial.set_timeout_frames(frames);
    }

    pub fn take_link_messages(&mut self) -> Vec<serial::LinkMessage> {
        self.serial.take_messages()
    }

    pub fn receive_link_message(&mut self, message: serial::LinkMessage) {
        self.serial.receive(message, &mut self.ic);
    }

    pub fn has_ir(&self) -> bool {
        self.cgb.cgb_mode || self.cartridge.has_ir()
    }

    pub fn ir_led(&self) -> bool {
        self.cgb.ir_led() || self.cartridge.ir_led()
    }

    pub fn set_ir_light(&mut self, light: bool) {
        self.cgb.ir_light = light;
        self.cartridge.set_ir_light(light);
    }

    pub fn attach_printer(&mut self, attached: bool) {
        self.serial.attach_printer(attached);
    }

    pub fn printer_attached(&self) -> bool {
        self.serial.printer_attached()
    }

    pub fn take_prints(&mut self) -> Vec<printer::PrintedImage> {
        self.serial.take_prints()
    }

    pub fn has_camera(&self) -> bool {
        self.cartridge.has_camera()
    }

    pub fn set_camera_frame(&mut self, frame: &[u8]) {
        self.cartridge.set_camera_frame(frame);
    }

    pub fn has_tilt(&self) -> bool {
        self.cartridge.has_tilt()
    }

    pub fn set_tilt(&mut self, x: f32, y: f32) {
        self.cartridge.set_tilt(x, y);
    }

    pub fn has_rumble(&self) -> bool {
        self.cartridge.has_rumble()
    }

    pub fn take_rumble(&mut self) -> f32 {
        self.cartridge.take_rumble()
    }

    pub fn press_button(&mut self, button: joypad::JoypadState) {
        self.joypad.press(button);
    }

    pub fn release_button(&mut self, button: joypad::JoypadState) {
        self.joypad.release(button);
    }

    pub fn set_keys(&mut self, keys: Keys) {
        self.joypad.set(keys.into());
    }

    pub fn keys(&self) -> Keys {
        self.joypad.held().into()
    }

    #[cfg(feature = "persistence")]
    pub fn poll_sram_dump(
        &mut self,
        force: bool,
        now: u64,
    ) -> Option<super::persistence::sram_dump::SramDump> {
        self.cartridge.poll_sram_dump(force, now)
    }

    #[cfg(feature = "persistence")]
    pub fn put_sram_dump(&mut self, dump: super::persistence::sram_dump::SramDump, now: u64) {
        self.cartridge.put_sram_dump(dump, now);
    }

    #[cfg(feature = "persistence")]
    pub fn dump_full(&mut self, now: u64) -> GbResult<Vec<u8>> {
        let sram = self.poll_sram_dump(true, now);
        let dump_ref = super::persistence::full_dump::FullDumpRef { gb: self, sram };
        #[cfg(feature = "brotli")]
        {
            dump_ref.to_brotli()
        }
        #[cfg(not(feature = "brotli"))]
        {
            dump_ref.to_rmp()
        }
    }

    #[cfg(all(feature = "persistence", feature = "serde_json"))]
    pub fn dump_json(&mut self, now: u64) -> GbResult<String> {
        let sram = self.poll_sram_dump(true, now);
        let dump_ref = super::persistence::full_dump::FullDumpRef { gb: self, sram };
        dump_ref.to_json_pretty()
    }

    #[cfg(feature = "persistence")]
    pub fn from_dump(data: &[u8], rom: &Rom, now: u64) -> GbResult<Self> {
        let dump = {
            #[cfg(feature = "brotli")]
            {
                super::persistence::full_dump::FullDump::from_brotli(data)?
            }
            #[cfg(not(feature = "brotli"))]
            {
                super::persistence::full_dump::FullDump::from_rmp(data)?
            }
        };

        let mut gb = dump.gb;
        gb.cartridge.restore_rom(rom)?;
        gb.memory.ensure_wram_banks(gb.model);
        gb.cgb.ensure_model(gb.model);
        gb.apu.restore(gb.model);
        gb.serial.forget_link();
        if let Some(sram) = dump.sram {
            gb.put_sram_dump(sram, now);
        }

        Ok(gb)
    }

    #[cfg(feature = "debug")]
    pub fn create_e2e_test(
        &self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> crate::debug::e2e::E2ETest {
        crate::debug::e2e::E2ETest::create(self, name.into(), description.into())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum GbModel {
    #[default]
    Dmg = 0,
    Cgb = 1,
}

impl GbModel {
    pub fn for_rom(rom: &Rom) -> GbResult<Self> {
        Ok(if rom.cgb_mode()?.is_cgb() {
            GbModel::Cgb
        } else {
            GbModel::Dmg
        })
    }

    pub fn frame_cycles(&self) -> u32 {
        17556
    }

    pub fn is_dmg(&self) -> bool {
        matches!(self, GbModel::Dmg)
    }

    pub fn is_cgb(&self) -> bool {
        matches!(self, GbModel::Cgb)
    }
}
