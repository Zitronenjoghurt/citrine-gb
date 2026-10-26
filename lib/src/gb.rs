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
mod boot_rom;
pub mod bus;
pub mod cartridge;
pub mod cgb;
pub mod cpu;
mod dma;
pub mod hdma;
pub mod ic;
pub mod joypad;
mod memory;
mod oam_bug;
pub mod ppu;
pub mod ram_init;
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
        *self = Self::new_with_ram_init(self.model, Some(rom.to_vec()), 0x00, self.ram_init);
        self.apu.set_sample_rate(sample_rate);
    }

    pub fn step(&mut self) {
        self.cpu.step(&mut cpu_bus!(self));
    }

    #[cfg(test)]
    pub(crate) fn with_bus<R>(&mut self, f: impl FnOnce(&mut bus::CpuBus) -> R) -> R {
        f(&mut cpu_bus!(self))
    }

    pub fn run_frame(&mut self) {
        self.ppu.frame_ready = false;

        while !self.ppu.frame_ready {
            self.step();

            #[cfg(feature = "debug")]
            if !self.debugger.breakpoints.is_empty() {
                use crate::disassembly::DisassemblySource;
                let loc = self
                    .cartridge
                    .probe_rom_location(self.cpu.pc.wrapping_sub(1));
                if self.debugger.breakpoints.contains(&loc) {
                    self.debugger.hit_breakpoint = true;
                    return;
                }
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
        self.apu = apu::Apu::new(self.model);
        if !self.boot_rom.mounted {
            self.apu.apply_post_boot();
        }
        self.apu.set_sample_rate(sample_rate);
        self.serial = serial::Serial::new();
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

    pub fn press_button(&mut self, button: joypad::JoypadState) {
        self.joypad.press(button);
    }

    pub fn release_button(&mut self, button: joypad::JoypadState) {
        self.joypad.release(button);
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
