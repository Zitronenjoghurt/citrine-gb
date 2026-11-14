use crate::console::Keys;
#[cfg(feature = "debug")]
use crate::debug::step::Stop;
use crate::error::GbResult;
use crate::rom::Rom;
use crate::{ReadMemory, WriteMemory};
use bus::CpuBusInterface;
use ppu::types::framebuffer::Framebuffer;

const POWER_ON_DIV: u16 = 4;
const DMG_POST_BOOT_DIV: u16 = 0xABC8;
const CGB_POST_BOOT_DIV: u16 = 0x1EA0;
const CGB_DMG_MODE_POST_BOOT_DIV: u16 = 0x2674;

fn post_boot_div(model: GbModel, boot_rom_mounted: bool) -> u16 {
    if boot_rom_mounted {
        POWER_ON_DIV
    } else if model.is_cgb() {
        CGB_POST_BOOT_DIV
    } else {
        DMG_POST_BOOT_DIV
    }
}

macro_rules! cpu_bus {
    ($gb:ident) => {
        cpu_bus!($gb, true)
    };
    ($gb:ident, $windows:expr) => {
        bus::CpuBus::<{ $windows }> {
            rom_window: bus::mapped_rom_window::<{ $windows }>(&$gb.boot_rom, &$gb.cartridge),
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
            acknowledge: None,
            quiet: 0,
            quiet_left: 0,
            dma_room: 0,
            p1_written: false,
            ticked: true,
            hdma_due: true,
            parts_noisy: true,
        }
    };
}

mod achievements;
pub mod apu;
pub(crate) mod boot_rom;
pub mod bus;
pub mod cartridge;
pub mod cgb;
mod cheats;
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
    #[cfg_attr(feature = "serde", serde(serialize_with = "ppu::serialize_settled"))]
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
        self.step_unsettled(1);
        self.settle_apu();
    }

    fn settle_apu(&mut self) {
        self.timer.settle(&mut self.serial, self.cgb.cgb_mode);
        self.cartridge.settle();
        self.apu.settle(
            self.timer.div,
            self.cgb.double_speed,
            #[cfg(feature = "debug")]
            &mut self.debugger,
        );
    }

    #[inline(always)]
    fn step_unsettled(&mut self, halt_budget: u32) {
        if self.stalled() {
            return;
        }
        if self.serial.plugged() {
            self.cpu
                .step_halting(&mut cpu_bus!(self, false), halt_budget);
        } else {
            self.cpu.step_halting(&mut cpu_bus!(self), halt_budget);
        }
    }

    fn stalled(&mut self) -> bool {
        self.joypad.cycle(&mut self.ic);
        let frame_cycles = self.frame_cycles();
        if self.serial.stall(1, frame_cycles, &mut self.ic) {
            self.cycle_counter = self.cycle_counter.wrapping_add(1);
            return true;
        }
        false
    }

    fn run_burst(&mut self) {
        if self.stalled() {
            return;
        }
        self.cpu.run_until(&mut cpu_bus!(self), |bus, fetched| {
            if !bus.ticked {
                return false;
            }
            if fetched {
                bus.service_hdma();
            }
            bus.ticked = bus.hdma_due;
            if bus.ppu.frame_ready || bus.serial.awaiting() {
                return true;
            }
            if bus.p1_written {
                bus.p1_written = false;
                bus.joypad.cycle(bus.ic);
            }
            false
        });
    }

    #[cfg(feature = "debug")]
    fn frame_halt_budget(&self) -> u32 {
        if self.debugger.breakpoints.armed() {
            return 1;
        }
        u32::MAX
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
        self.serial.next_traffic_frame();

        while !self.ppu.frame_ready {
            let frame_cycles = self.frame_cycles();
            if self.serial.stall(frame_cycles, frame_cycles, &mut self.ic) {
                break;
            }

            #[cfg(feature = "debug")]
            if self.debugger.breakpoints.armed() || self.debugger.breakpoints.watching() {
                self.step_unsettled(self.frame_halt_budget());
                if self.at_breakpoint() {
                    self.debugger.hit_breakpoint = true;
                    self.settle_apu();
                    return;
                }
                continue;
            }

            self.run_burst();
        }

        if self.cycle_counter >= self.frame_cycles() {
            self.cycle_counter -= self.frame_cycles();
        }

        self.settle_apu();
        self.apu.flush_audio();
    }

    pub fn run_cycles(&mut self, cycles: u32) {
        self.cycle_counter = 0;
        while self.cycle_counter < cycles {
            self.step_unsettled(cycles - self.cycle_counter);
        }
        self.settle_apu();
    }

    #[cfg(feature = "debug")]
    pub fn run_for(&mut self, cycles: u32) -> Stop {
        let start = self.cycle_counter;
        let mut stop = Stop::Finished;
        while self.cycle_counter.wrapping_sub(start) < cycles {
            let left = cycles - self.cycle_counter.wrapping_sub(start);
            self.step_unsettled(left.min(self.frame_halt_budget()));
            if let Some(hit) = self.breakpoint_hit() {
                stop = hit;
                break;
            }
        }
        let frame_cycles = self.frame_cycles();
        while self.cycle_counter >= frame_cycles {
            self.cycle_counter -= frame_cycles;
        }
        self.settle_apu();
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

    #[inline]
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

#[cfg(test)]
mod tests {
    use super::*;

    const SPIN: [u8; 2] = [0x18, 0xFE];

    fn boot(model: GbModel, parts: &[(usize, &[u8])]) -> GameBoy {
        let mut rom = vec![0u8; if model.is_cgb() { 0x900 } else { 0x100 }];
        for (at, bytes) in parts {
            rom[*at..*at + bytes.len()].copy_from_slice(bytes);
        }
        GameBoy::new(model, Some(rom), 0)
    }

    fn hram(gb: &mut GameBoy) -> u8 {
        gb.with_bus(|bus| bus.read_naive(0xFF80))
    }

    fn div_after_nops(model: GbModel, nops: usize) -> u8 {
        let read = [0xF0, 0x04, 0xE0, 0x80, 0x18, 0xFE];
        let mut gb = boot(model, &[(nops, &read)]);
        gb.with_bus(|bus| bus.write_naive(0xFF80, 0xAA));
        for _ in 0..nops + 10 {
            gb.step();
        }
        hram(&mut gb)
    }

    fn first_div_tick(model: GbModel) -> usize {
        (0..0x80)
            .find(|&nops| div_after_nops(model, nops) == 1)
            .expect("div ticks")
    }

    #[test]
    fn div_ticks_like_sameboy_after_power_on() {
        assert_eq!(
            (first_div_tick(GbModel::Dmg), first_div_tick(GbModel::Cgb)),
            (60, 60)
        );
    }

    fn cycles_to_frame_end(gb: &mut GameBoy) -> u32 {
        gb.ppu.frame_ready = false;
        let start = gb.cycle_counter;
        while !gb.ppu.frame_ready {
            gb.step();
            assert!(gb.cycle_counter - start < 3 * 17556);
        }
        gb.cycle_counter - start
    }

    #[test]
    fn a_frame_with_the_lcd_off_lasts_70224_dots() {
        let mut gb = boot(GbModel::Dmg, &[(0, &SPIN)]);
        let first = cycles_to_frame_end(&mut gb);
        assert!((17556..17559).contains(&first), "{first}");
        let second = cycles_to_frame_end(&mut gb);
        assert!((17554..17559).contains(&second), "{second}");
    }

    fn lcd_on_after_delay(loops: u16) -> u32 {
        let [lo, hi] = loops.to_le_bytes();
        let program = [
            0x01, lo, hi, 0x0B, 0x78, 0xB1, 0x20, 0xFB, 0x3E, 0x91, 0xE0, 0x40, 0x18, 0xFE,
        ];
        let mut gb = boot(GbModel::Dmg, &[(0, &program)]);
        cycles_to_frame_end(&mut gb)
    }

    #[test]
    fn turning_the_lcd_on_after_ten_lines_ends_the_frame() {
        let frame = lcd_on_after_delay(200);
        assert!((1400..1420).contains(&frame), "{frame}");
    }

    #[test]
    fn turning_the_lcd_on_early_keeps_the_frame() {
        let frame = lcd_on_after_delay(100);
        assert!(frame > 16000, "{frame}");
    }

    #[test]
    fn the_vector_is_picked_after_the_low_push() {
        let setup = [
            0x31, 0x11, 0xFF, 0x3E, 0x05, 0xE0, 0xFF, 0x3E, 0x04, 0xE0, 0x0F, 0xFB, 0x00,
        ];
        let mut gb = boot(
            GbModel::Dmg,
            &[
                (0x00, &setup),
                (0x40, &[0x3E, 0x40, 0xE0, 0x80, 0x18, 0xFE]),
                (0x50, &[0x3E, 0x50, 0xE0, 0x80, 0x18, 0xFE]),
            ],
        );
        for _ in 0..30 {
            gb.step();
        }
        assert_eq!(hram(&mut gb), 0x40);
    }

    #[test]
    fn a_selected_key_raises_the_joypad_interrupt() {
        let mut gb = boot(GbModel::Dmg, &[(0, &SPIN)]);
        gb.with_bus(|bus| bus.write_naive(0xFF00, 0x20));
        gb.ic.flag.set(ic::Interrupt::Joypad, false);
        gb.joypad.press(joypad::JoypadState::DOWN);
        gb.step();
        assert!(gb.ic.flag.is_enabled(ic::Interrupt::Joypad));
    }

    fn acknowledge_rom(cgb: bool, scx: u8) -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
        if cgb {
            data[0x0143] = 0x80;
        }
        let mut program = vec![
            0xF3, 0x31, 0xFE, 0xDF, 0x3E, scx, 0xE0, 0x43, 0x3E, 0x91, 0xE0, 0x40, 0x1E, 0x00,
        ];
        let lp = 0x150 + program.len() as u16;
        program.extend([
            0xF3, 0x3E, 0x20, 0xE0, 0x41, 0x3E, 0x02, 0xE0, 0xFF, 0xAF, 0xE0, 0x0F, 0x76, 0x00,
            0x3E, 0x08, 0xE0, 0x41, 0x3E, 0x02, 0xE0, 0x0F,
        ]);
        let sled_end = 0x150 + program.len() as u16 + 11 + 120;
        let [end_lo, end_hi] = sled_end.to_le_bytes();
        program.extend([
            0x21, end_lo, end_hi, 0x7D, 0x93, 0x6F, 0x7C, 0xDE, 0x00, 0x67, 0xE9,
        ]);
        program.extend([0x00; 120]);
        program.extend([0xFB, 0x00, 0x18, 0xFE]);
        data[0x0150..0x0150 + program.len()].copy_from_slice(&program);
        let [lp_lo, lp_hi] = lp.to_le_bytes();
        let handler = [
            0xF0, 0x0F, 0x21, 0x00, 0xC0, 0x16, 0x00, 0x19, 0x77, 0x31, 0xFE, 0xDF, 0x1C, 0x7B,
            0xFE, 100, 0x28, 0x03, 0xC3, lp_lo, lp_hi, 0x3E, 0xAA, 0xEA, 0xFF, 0xC0, 0x18, 0xFE,
        ];
        data[0x48..0x48 + handler.len()].copy_from_slice(&handler);
        Rom::new(&data)
    }

    fn stat_still_pending(model: GbModel, scx: u8) -> Vec<u8> {
        let mut gb = boot(
            model,
            &[
                (0x00, &[0x3E, 0x11, 0xC3, 0xFE, 0x00]),
                (0xFE, &[0xE0, 0x50]),
            ],
        );
        gb.load_rom(&acknowledge_rom(model.is_cgb(), scx))
            .expect("load");
        let wram = |gb: &mut GameBoy, addr: u16| gb.with_bus(|bus| bus.read_naive(addr));
        while wram(&mut gb, 0xC0FF) != 0xAA {
            gb.step();
            assert!(gb.cycle_counter < 40 * 17556);
        }
        (0..100)
            .filter(|&delay| wram(&mut gb, 0xC000 + delay as u16) & 0x02 != 0)
            .collect()
    }

    #[test]
    fn a_stat_rise_right_after_the_vector_pick_is_acknowledged_like_sameboy() {
        let sameboy: [&[u8]; 8] = [
            &[32, 33],
            &[32, 33, 34],
            &[32, 33, 34],
            &[33, 34],
            &[33, 34],
            &[33, 34, 35],
            &[33, 34, 35],
            &[34, 35],
        ];
        for model in [GbModel::Dmg, GbModel::Cgb] {
            for (scx, expected) in sameboy.iter().enumerate() {
                assert_eq!(
                    stat_still_pending(model, scx as u8),
                    *expected,
                    "{model:?} scx {scx}"
                );
            }
        }
    }

    fn run_one_frame(gb: &mut GameBoy, batched: bool) {
        if batched {
            gb.run_frame();
            return;
        }
        gb.ppu.frame_ready = false;
        while !gb.ppu.frame_ready {
            gb.step();
        }
        if gb.cycle_counter >= gb.frame_cycles() {
            gb.cycle_counter -= gb.frame_cycles();
        }
    }

    fn halting_frames(
        model: GbModel,
        program: &[u8],
        batched: bool,
    ) -> Vec<(u32, u16, u16, u8, u8, u8)> {
        let mut gb = boot(model, &[(0, program), (0x40, &[0xD9]), (0x50, &[0xD9])]);
        (0..6)
            .map(|_| {
                run_one_frame(&mut gb, batched);
                (
                    gb.cycle_counter,
                    gb.cpu.pc,
                    gb.timer.div,
                    gb.timer.tima,
                    gb.ppu.ly,
                    gb.ic.flag.into(),
                )
            })
            .collect()
    }

    #[test]
    fn halted_frames_run_in_batches_like_single_steps() {
        let slow_timer = [
            0x3E, 0x91, 0xE0, 0x40, 0x3E, 0x05, 0xE0, 0xFF, 0x3E, 0x04, 0xE0, 0x07, 0xFB, 0x76,
            0x18, 0xFD,
        ];
        let fast_timer_and_serial = [
            0x3E, 0x91, 0xE0, 0x40, 0x3E, 0x05, 0xE0, 0xFF, 0x3E, 0x05, 0xE0, 0x07, 0x3E, 0x80,
            0xE0, 0x02, 0xFB, 0x76, 0x18, 0xFD,
        ];
        let lcd_off = [
            0x3E, 0x11, 0xE0, 0x40, 0x3E, 0x04, 0xE0, 0xFF, 0x3E, 0x06, 0xE0, 0x07, 0xFB, 0x76,
            0x18, 0xFD,
        ];
        for program in [&slow_timer[..], &fast_timer_and_serial, &lcd_off] {
            for model in [GbModel::Dmg, GbModel::Cgb] {
                let batched = halting_frames(model, program, true);
                assert_eq!(
                    batched,
                    halting_frames(model, program, false),
                    "{model:?} {program:02X?}"
                );
                assert!(batched.iter().any(|frame| frame.3 > 0), "{model:?}");
            }
        }
    }

    fn busy_frames(model: GbModel, batched: bool) -> Vec<(u32, u16, u8, u8, u8)> {
        let program = [
            0x3E, 0x91, 0xE0, 0x40, 0x3E, 0x10, 0xE0, 0xFF, 0xFB, 0x3E, 0x20, 0xE0, 0x00, 0x3E,
            0x10, 0xE0, 0x00, 0x18, 0xF6,
        ];
        let mut gb = boot(model, &[(0, &program), (0x60, &[0x04, 0xD9])]);
        gb.joypad.press(joypad::JoypadState::A);
        (0..4)
            .map(|_| {
                run_one_frame(&mut gb, batched);
                (
                    gb.cycle_counter,
                    gb.cpu.pc,
                    gb.cpu.b,
                    gb.ppu.ly,
                    gb.ic.flag.into(),
                )
            })
            .collect()
    }

    #[test]
    fn busy_frames_run_in_bursts_like_single_steps() {
        for model in [GbModel::Dmg, GbModel::Cgb] {
            let burst = busy_frames(model, true);
            assert_eq!(burst, busy_frames(model, false), "{model:?}");
            assert!(burst.iter().any(|frame| frame.2 > 0), "{model:?}");
        }
    }

    fn cartridge_frames(model: GbModel, batched: bool) -> Vec<(u32, u16, u8, u8, u8, u8)> {
        let mut data = vec![0u8; 0x8000];
        data[0x0040..0x004A]
            .copy_from_slice(&[0xF5, 0xFA, 0x00, 0xC2, 0x3C, 0xEA, 0x00, 0xC2, 0xF1, 0xD9]);
        data[0x0060] = 0xD9;
        data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
        let program = [
            0x31, 0xFE, 0xDF, 0x3E, 0x91, 0xE0, 0x40, 0x3E, 0x11, 0xE0, 0xFF, 0xFB, 0x21, 0x00,
            0xC0, 0x06, 0x20, 0x2A, 0x3C, 0xEA, 0x00, 0xC1, 0xFA, 0x00, 0xC1, 0xC5, 0xCD, 0x80,
            0x01, 0xC1, 0x3E, 0x20, 0xE0, 0x00, 0x3E, 0x10, 0xE0, 0x00, 0x05, 0x20, 0xE8, 0x7D,
            0xE6, 0x3F, 0x6F, 0x18, 0xE0,
        ];
        data[0x0150..0x0150 + program.len()].copy_from_slice(&program);
        data[0x0180..0x0183].copy_from_slice(&[0x0C, 0xC0, 0xC9]);
        let mut gb = GameBoy::new_empty(model);
        gb.load_rom_with_model(&Rom::new(&data), model)
            .expect("load");
        gb.joypad.press(joypad::JoypadState::A);
        (0..6)
            .map(|_| {
                run_one_frame(&mut gb, batched);
                let vblanks = gb.with_bus(|bus| bus.read_naive(0xC200));
                (
                    gb.cycle_counter,
                    gb.cpu.pc,
                    gb.cpu.c,
                    gb.ppu.ly,
                    gb.ic.flag.into(),
                    vblanks,
                )
            })
            .collect()
    }

    #[test]
    fn cartridge_frames_run_in_bursts_like_single_steps() {
        for model in [GbModel::Dmg, GbModel::Cgb] {
            let burst = cartridge_frames(model, true);
            assert_eq!(burst, cartridge_frames(model, false), "{model:?}");
            assert!(burst.iter().any(|frame| frame.5 > 0), "{model:?}");
        }
    }
}
