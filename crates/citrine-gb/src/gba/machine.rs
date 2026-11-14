mod cart;
mod cheats;
mod console;
#[cfg(feature = "debug")]
mod debug;
mod idle;
#[cfg(feature = "persistence")]
mod link;
#[cfg(feature = "persistence")]
mod rollback;

use super::bios::{Bios, BiosKind, Boot};
use super::bus::Bus;
use super::cpu::{Cpu, Exception};
use super::power::Power;
use crate::console::Keys;
#[cfg(feature = "jit-lockstep")]
use crate::jit::lockstep::Track;
#[cfg(jit)]
use crate::jit::{Jit, JitStats};
#[cfg(feature = "jit")]
use crate::jit_cache::JitCache;
#[cfg(feature = "rom-id")]
use crate::rom::id::RomId;
use std::sync::Arc;

pub use idle::IdleStats;

pub const CYCLES_PER_FRAME: u64 = 280_896;
const HALT_STEP: u32 = 1232;
const WAKE_CYCLES: u32 = 1;
const FRESH_HALT_CYCLES: u32 = WAKE_CYCLES + 1;
const HELD_CYCLES: u32 = 2;
#[cfg(jit)]
const COMPILE_THREADS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Fault {
    Unsupported(u32),
}

pub struct Gba {
    cpu: Cpu,
    bus: Bus,
    bios: Bios,
    boot: Boot,
    keys: Keys,
    cycles: u64,
    frame_end: u64,
    fault: Option<Fault>,
    rumble: bool,
    idle: idle::IdleLoops,
    #[cfg(feature = "debug")]
    debugger: crate::debug::Debugger,
    #[cfg(feature = "debug")]
    code: super::disasm::CodeMap,
    #[cfg(feature = "debug")]
    stepping: super::cpu::step::Stepping,
    #[cfg(feature = "rom-id")]
    rom_id: RomId,
    #[cfg(jit)]
    jit: Option<Box<Jit>>,
    #[cfg(jit)]
    jit_cache: Option<Arc<JitCache>>,
    #[cfg(jit)]
    straight: bool,
    #[cfg(feature = "jit-lockstep")]
    lockstep: Track<false>,
}

impl Gba {
    pub fn new(rom: &[u8]) -> Self {
        Self::with_bios(rom.into(), Bios::built_in(), Boot::Skip)
    }

    pub fn with_bios(rom: Arc<[u8]>, bios: Bios, boot: Boot) -> Self {
        Self::from_rom(&crate::rom::Rom::from(rom), bios, boot)
    }

    pub fn from_rom(file: &crate::rom::Rom, bios: Bios, boot: Boot) -> Self {
        #[cfg(feature = "rom-id")]
        let rom_id = file.ids().0;
        let mut bus = Bus::with_rom(Arc::clone(&file.data));
        let cpu = bios.boot(&mut bus, boot);
        Self {
            cpu,
            bus,
            bios,
            boot,
            keys: Keys::empty(),
            cycles: boot.entry_cycles(),
            frame_end: 0,
            fault: None,
            rumble: false,
            idle: idle::IdleLoops::default(),
            #[cfg(feature = "debug")]
            debugger: crate::debug::Debugger::default(),
            #[cfg(feature = "debug")]
            code: super::disasm::CodeMap::default(),
            #[cfg(feature = "debug")]
            stepping: super::cpu::step::Stepping::default(),
            #[cfg(feature = "rom-id")]
            rom_id,
            #[cfg(jit)]
            jit: None,
            #[cfg(jit)]
            jit_cache: None,
            #[cfg(jit)]
            straight: false,
            #[cfg(feature = "jit-lockstep")]
            lockstep: Track::default(),
        }
    }

    pub fn bios(&self) -> BiosKind {
        self.bios.kind()
    }

    pub fn boot(&self) -> Boot {
        self.boot
    }

    pub fn power(&self) -> Power {
        self.bus.power().settled()
    }

    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    pub fn bus_mut(&mut self) -> &mut Bus {
        &mut self.bus
    }

    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    pub fn frame(&self) -> &[u8] {
        self.bus.ppu().frame()
    }

    pub fn fault(&self) -> Option<Fault> {
        self.fault
    }

    pub fn run_frame(&mut self) {
        if self.fault.is_some() {
            return;
        }
        self.bus.serial_mut().traffic_mut().next_frame();
        self.catch_up_frame_end();
        if self.runs_plain() {
            self.run_plain_frame();
        } else {
            while self.cycles < self.frame_end {
                #[cfg(feature = "debug")]
                let before = self.cpu.pc();
                if self.step_until(self.frame_end) == 0 {
                    break;
                }
                #[cfg(feature = "debug")]
                if self.breakpoint_after(before) {
                    self.bus.catch_up_devices();
                    return;
                }
            }
        }
        self.bus.catch_up_devices();
        #[cfg(feature = "debug")]
        self.drain_waves();
        self.rumble = self.bus.cart_mut().take_rumble();
    }

    fn catch_up_frame_end(&mut self) {
        while self.cycles >= self.frame_end {
            self.frame_end += CYCLES_PER_FRAME;
        }
    }

    fn runs_plain(&self) -> bool {
        #[cfg(jit)]
        if self.jit.is_some() {
            return false;
        }
        !cfg!(feature = "jit-lockstep") && self.no_debugger()
    }

    fn run_plain_frame(&mut self) {
        while self.cycles < self.frame_end {
            let until = self.frame_end.min(self.idle_at());
            if self.bus.power().running() && self.cycles < until {
                if !self.run_plain(until) {
                    break;
                }
            } else if self.step_until(self.frame_end) == 0 {
                break;
            }
        }
    }

    fn run_plain(&mut self, until: u64) -> bool {
        let mut at = self.cycles;
        let ran = loop {
            self.bus.start_cpu_step();
            let cycles = if self.bus.interrupts().pending()
                && let Some(cycles) = self.cpu.interrupt(&mut self.bus, Exception::Irq)
            {
                cycles
            } else {
                self.cpu.step_inlined(&mut self.bus)
            };
            if cycles == 0 {
                at += u64::from(self.fault_step());
                break false;
            }
            let cycles = cycles + self.bus.dma_cycles();
            let settled = self.bus.finish_cpu_step(cycles);
            at += u64::from(cycles);
            if at >= until || settled && !self.bus.power().running() {
                break true;
            }
        };
        self.cycles = at;
        ran
    }

    #[cold]
    #[inline(never)]
    fn fault_step(&mut self) -> u32 {
        self.fault = Some(Fault::Unsupported(self.next_instruction()));
        self.bus.settle_this_step();
        let cycles = self.bus.dma_cycles();
        self.bus.finish_cpu_step(cycles);
        cycles
    }

    pub fn step(&mut self) -> u32 {
        let cycles = self.step_until(0);
        self.bus.catch_up_devices();
        cycles
    }

    #[inline(always)]
    fn step_until(&mut self, until: u64) -> u32 {
        if self.fault.is_some() {
            return 0;
        }
        let cycles = if self.bus.power().running() {
            self.run_cpu(until)
        } else {
            self.idle_step()
        };
        #[cfg(feature = "jit-lockstep")]
        self.lockstep.settle(self.bus.interrupts());
        self.cycles += u64::from(cycles);
        cycles
    }

    pub fn soft_reset(&mut self) {
        let bios = std::mem::take(&mut self.bios);
        let fresh = Self::with_bios(self.bus.shared_rom(), bios, self.boot);
        let mut old = std::mem::replace(self, fresh);
        #[cfg(jit)]
        {
            self.jit = old.jit.take();
            self.jit_cache = old.jit_cache.take();
            if let Some(jit) = self.jit.as_deref_mut() {
                jit.recheck();
            }
        }
        let traffic = self.bus.serial_mut().traffic_mut();
        traffic.adopt(old.bus.serial_mut().traffic_mut());
        traffic.push(crate::traffic::Entry::new(crate::traffic::Kind::SoftReset));
        *self.bus.backup_mut() = old.bus.backup().clone();
        *self.bus.cart_mut() = old.bus.cart().clone();
        self.bus.sound_mut().keep_settings(old.bus.sound());
        self.idle = old.idle.restarted();
        self.set_keys(old.keys);
    }

    pub fn set_keys(&mut self, keys: Keys) {
        let keys = Keys::from_bits_truncate(keys.bits() & super::keypad::KEY_MASK);
        self.keys = keys;
        self.bus.set_keys(keys);
    }

    #[cfg(feature = "jit")]
    pub fn set_jit(&mut self, on: bool) {
        #[cfg(jit)]
        if on != self.jit.is_some() {
            self.jit = on.then(|| self.cached_jit());
            self.straight = false;
        }
        #[cfg(not(jit))]
        let _ = on;
    }

    #[cfg(feature = "jit")]
    pub fn compile_jit_on(&mut self, spawner: &citrine_gb_threads::Spawner) {
        #[cfg(jit)]
        if let Some(jit) = self.jit.as_deref_mut() {
            jit.compile_on(spawner, COMPILE_THREADS);
        }
        #[cfg(not(jit))]
        let _ = spawner;
    }

    #[cfg(feature = "jit")]
    pub fn set_jit_cache(&mut self, cache: Option<Arc<JitCache>>) {
        #[cfg(jit)]
        {
            if let Some(jit) = self.jit.as_deref_mut() {
                jit.set_cache(cache.clone());
            }
            self.jit_cache = cache;
        }
        #[cfg(not(jit))]
        let _ = cache;
    }

    #[cfg(jit)]
    fn cached_jit(&self) -> Box<Jit> {
        let mut jit = Box::<Jit>::default();
        jit.set_cache(self.jit_cache.clone());
        jit
    }

    #[cfg(feature = "jit")]
    pub fn jit_on(&self) -> bool {
        #[cfg(jit)]
        return self.jit.is_some();
        #[cfg(not(jit))]
        false
    }

    #[cfg(feature = "jit")]
    pub fn jit_counts(&self) -> Option<crate::nds::machine::JitCounts> {
        #[cfg(jit)]
        return self
            .jit
            .as_deref()
            .map(|jit| crate::nds::machine::JitCounts::over(&[jit]));
        #[cfg(not(jit))]
        None
    }

    #[cfg(jit)]
    pub fn jit_stats(&self) -> Option<JitStats> {
        self.jit.as_deref().map(Jit::stats)
    }

    #[cfg(feature = "jit-lockstep")]
    pub fn lockstep(&mut self) -> &mut Track<false> {
        &mut self.lockstep
    }

    #[cold]
    #[inline(never)]
    fn idle_step(&mut self) -> u32 {
        self.bus.catch_up_devices();
        let stopped = self.bus.power() == Power::Stopped;
        let cycles = if let Some(cycles) = self.enter_halt() {
            cycles
        } else if self.bus.dma_active() {
            self.bus.start_cpu_step();
            let cycles = self.bus.run_dma();
            self.resume_after_dma().unwrap_or(cycles)
        } else {
            self.sleep()
        };
        if stopped {
            self.bus.elapse_interrupts(cycles);
        } else {
            self.bus.step_devices(cycles);
        }
        cycles
    }

    #[inline(always)]
    fn run_cpu(&mut self, until: u64) -> u32 {
        if self.idle_due()
            && let Some(cycles) = self.idle_turn(until)
        {
            return cycles;
        }
        #[cfg(jit)]
        if !self.straight
            && let Some(cycles) = self.run_jit(until)
        {
            self.bus.finish_cpu_step(cycles);
            return cycles;
        }
        self.bus.start_cpu_step();
        if self.bus.interrupts().pending()
            && let Some(cycles) = self.cpu.interrupt(&mut self.bus, Exception::Irq)
        {
            #[cfg(jit)]
            {
                self.straight = false;
            }
            let cycles = cycles + self.bus.dma_cycles();
            self.bus.finish_cpu_step(cycles);
            return cycles;
        }
        let from = self.cpu.pc();
        #[cfg(feature = "debug")]
        self.on_fetch();
        let cycles = self.cpu.step(&mut self.bus);
        self.finish_step(from, cycles)
    }

    #[inline(always)]
    fn finish_step(&mut self, from: u32, cycles: u32) -> u32 {
        if cycles == 0 {
            self.fault = Some(Fault::Unsupported(self.next_instruction()));
            self.bus.settle_this_step();
        }
        let cycles = cycles + self.bus.dma_cycles();
        #[cfg(jit)]
        self.stepped(from, cycles);
        #[cfg(not(jit))]
        let _ = from;
        self.bus.finish_cpu_step(cycles);
        cycles
    }

    #[cfg(jit)]
    fn run_jit(&mut self, until: u64) -> Option<u32> {
        if self.jit.is_none() || !self.no_debugger() || self.bus.serial().plugged() {
            return None;
        }
        if self.bus.interrupts().pending() && !self.cpu.irq_stays_masked() {
            return None;
        }
        let left = until.saturating_sub(self.cycles).min(u64::from(u32::MAX)) as u32;
        let budget = left.min(self.bus.jit_budget());
        if budget == 0 {
            return None;
        }
        let jit = self.jit.as_deref_mut()?;
        #[cfg(feature = "jit-lockstep")]
        let start = self.cpu.pc();
        self.bus.start_cpu_step();
        let ran = jit.run(&mut self.cpu, &mut self.bus, budget)?;
        let cycles = ran.cycles + self.bus.dma_cycles();
        #[cfg(feature = "jit-lockstep")]
        self.lockstep.ran(
            start,
            ran.retired,
            &self.cpu,
            self.cycles + u64::from(cycles),
        );
        Some(cycles)
    }

    #[cfg(jit)]
    fn stepped(&mut self, from: u32, cycles: u32) {
        let size = if self.cpu.thumb() { 2 } else { 4 };
        self.straight = self.cpu.pc() == from.wrapping_add(size);
        if let Some(jit) = self.jit.as_deref_mut() {
            jit.stepped();
        }
        #[cfg(feature = "jit-lockstep")]
        self.lockstep
            .stepped(&self.cpu, self.cycles + u64::from(cycles));
        #[cfg(not(feature = "jit-lockstep"))]
        let _ = cycles;
    }

    #[cfg(feature = "debug")]
    fn no_debugger(&self) -> bool {
        !self.debugger.static_analysis_enabled && self.debugger.breakpoints.is_empty()
    }

    #[cfg(not(feature = "debug"))]
    fn no_debugger(&self) -> bool {
        true
    }

    fn enter_halt(&mut self) -> Option<u32> {
        if !self.bus.settle_halt() || self.bus.dma_active() || !self.bus.interrupts().wakes() {
            return None;
        }
        self.bus.start_cpu_step();
        let entry = if self.bus.interrupts().pending() {
            self.cpu.interrupt(&mut self.bus, Exception::Irq)
        } else {
            None
        };
        self.bus.wake();
        match entry {
            Some(cycles) => Some(cycles + self.bus.dma_cycles() + FRESH_HALT_CYCLES),
            None => {
                self.cpu.hold_irq();
                Some(FRESH_HALT_CYCLES)
            }
        }
    }

    fn resume_after_dma(&mut self) -> Option<u32> {
        if self.bus.power() != Power::Halted
            || self.bus.dma_active()
            || !self.bus.woke_before(HELD_CYCLES)
        {
            return None;
        }
        self.bus.wake();
        self.bus.resume_held(WAKE_CYCLES, HELD_CYCLES);
        #[cfg(jit)]
        let from = self.cpu.pc();
        #[cfg(feature = "debug")]
        self.on_fetch();
        let cycles = self.cpu.step(&mut self.bus);
        let unpaid = self.bus.end_held();
        if cycles == 0 && unpaid == HELD_CYCLES {
            self.fault = Some(Fault::Unsupported(self.next_instruction()));
        }
        let cycles = self.bus.step_cycles();
        #[cfg(jit)]
        self.stepped(from, cycles);
        Some(cycles)
    }

    fn sleep(&mut self) -> u32 {
        if !self.bus.interrupts().wakes() {
            return self.bus.until_event().clamp(1, HALT_STEP);
        }
        self.bus.wake();
        self.cpu.hold_irq();
        WAKE_CYCLES
    }

    fn next_instruction(&self) -> u32 {
        let width = if self.cpu.thumb() { 2 } else { 4 };
        self.cpu.reg(15).wrapping_sub(2 * width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::{Console, ConsoleKind};

    const LOOP: u32 = 0xEAFF_FFFE;

    fn rom(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    fn timer_irq_program() -> Vec<u8> {
        rom(&[
            0xE59F_0030,
            0xE59F_1030,
            0xE580_1000,
            0xE59F_002C,
            0xE59F_102C,
            0xE580_1100,
            0xE280_2C02,
            0xE3A0_1008,
            0xE582_1000,
            0xE3A0_1001,
            0xE582_1008,
            LOOP,
            0,
            0,
            0x0300_7FFC,
            0x0800_0048,
            0x0400_0000,
            0x00C0_0000,
            0xE285_5001,
            0xE3A0_0301,
            0xE280_0C02,
            0xE3A0_1008,
            0xE1C0_10B2,
            0xE12F_FF1E,
        ])
    }

    #[test]
    fn boots_at_the_cartridge_entry_in_system_mode() {
        let gba = Gba::new(&rom(&[LOOP, 0xE3A0_0001]));
        let state = gba.cpu().state();
        assert_eq!(gba.cpu().cpsr(), 0x1F);
        assert_eq!(state.r[13], 0x0300_7F00);
        assert_eq!(state.irq[0], 0x0300_7FA0);
        assert_eq!(state.svc[0], 0x0300_7FE0);
        assert_eq!(state.r[15], 0x0800_0008);
        assert_eq!(state.pipeline, [LOOP, 0xE3A0_0001]);
    }

    #[test]
    fn a_frame_is_280896_cycles_and_overshoot_carries_into_the_next() {
        let mut gba = Gba::new(&rom(&[LOOP]));
        let boundary = gba.cycles().next_multiple_of(CYCLES_PER_FRAME);
        gba.run_frame();
        let first = gba.cycles();
        assert!((boundary..boundary + 32).contains(&first), "{first}");
        gba.run_frame();
        let next = boundary + CYCLES_PER_FRAME;
        assert!((next..next + 32).contains(&gba.cycles()));
    }

    #[test]
    fn a_frame_after_stepping_past_several_frames_runs_a_whole_frame() {
        let mut gba = Gba::new(&rom(&[LOOP]));
        let base = gba.cycles().next_multiple_of(CYCLES_PER_FRAME);
        while gba.cycles() < base + 2 * CYCLES_PER_FRAME + 100 {
            gba.step();
        }
        gba.run_frame();
        let end = gba.cycles();
        let third = base + 3 * CYCLES_PER_FRAME;
        assert!((third..third + 32).contains(&end), "{end}");
        gba.run_frame();
        let fourth = third + CYCLES_PER_FRAME;
        assert!((fourth..fourth + 32).contains(&gba.cycles()));
    }

    #[test]
    fn an_irq_calls_the_handler_at_03007ffc_and_returns_to_the_program() {
        let mut gba = Gba::new(&timer_irq_program());
        let end = gba.cycles() + CYCLES_PER_FRAME;
        while gba.cycles() < end {
            gba.step();
        }
        assert_eq!(gba.fault(), None);
        assert_eq!(gba.cpu().reg(5), 4);
        assert_eq!(gba.cpu().cpsr() & 0x1F, 0x1F);
        assert_eq!(gba.cpu().reg(15) - 8, 0x0800_002C);
    }

    #[test]
    fn an_irq_enters_through_the_bios_handler_at_0128() {
        let mut gba = Gba::new(&timer_irq_program());
        let mut seen = Vec::new();
        let limit = gba.cycles() + CYCLES_PER_FRAME;
        while gba.cycles() < limit && seen.len() < 3 {
            gba.step();
            let pc = gba.cpu().reg(15) - 8;
            if pc < 0x4000 && seen.last() != Some(&pc) {
                seen.push(pc);
            }
        }
        assert_eq!(seen, [0x18, 0x128, 0x12C]);
    }

    #[test]
    fn an_swi_runs_the_bios_call_and_returns() {
        let mut gba = Gba::new(&rom(&[0xE3A0_0E19, 0xEF08_0000, LOOP]));
        gba.run_frame();
        assert_eq!(gba.fault(), None);
        assert_eq!(gba.cpu().reg(0), 20);
        assert_eq!(gba.cpu().cpsr() & 0x1F, 0x1F);
        assert_eq!(gba.cpu().reg(15) - 8, 0x0800_0008);
    }

    #[test]
    fn haltcnt_sleeps_until_ie_and_if_meet_with_ime_off() {
        let mut gba = Gba::new(&rom(&[
            0xE3A0_0301,
            0xE3A0_1008,
            0xE1C0_10B4,
            0xE3A0_1001,
            0xE280_2C02,
            0xE1C2_10B0,
            0xE3A0_1000,
            0xE5C0_1301,
            0xE1D0_50B6,
            LOOP,
        ]));
        gba.step();
        let limit = gba.cycles() + CYCLES_PER_FRAME;
        while gba.power() == Power::Running && gba.cycles() < limit {
            gba.step();
        }
        assert_eq!(gba.power(), Power::Halted);
        gba.run_frame();
        assert_eq!(gba.power(), Power::Running);
        assert_eq!(gba.cpu().reg(5), 160);
        assert_eq!(gba.cpu().cpsr() & 0x1F, 0x1F);
    }

    #[test]
    fn stop_freezes_video_and_timers_until_a_key_irq() {
        let mut gba = Gba::new(&rom(&[0xE3A0_0301, 0xE3A0_1080, 0xE5C0_1301, LOOP]));
        gba.bus_mut().write16(0x0400_0132, 0x4001);
        gba.bus_mut().write16(0x0400_0200, 0x1000);
        while gba.power() != Power::Stopped {
            gba.step();
        }
        let vcount = gba.bus().read16(0x0400_0006);
        gba.run_frame();
        assert_eq!(gba.power(), Power::Stopped);
        assert_eq!(gba.bus().read16(0x0400_0006), vcount);
        gba.set_keys(Keys::A);
        assert_eq!(gba.step(), 2);
        assert_eq!(gba.power(), Power::Stopped);
        gba.step();
        assert_eq!(gba.power(), Power::Running);
    }

    #[test]
    fn soft_reset_boots_the_same_cartridge_again() {
        let mut gba = Gba::new(&rom(&[0xEF05_0000]));
        gba.run_frame();
        gba.soft_reset();
        assert_eq!(gba.fault(), None);
        assert_eq!(gba.cycles(), Boot::Skip.entry_cycles());
        assert_eq!(gba.cpu().state().pipeline[0], 0xEF05_0000);
    }

    #[test]
    fn soft_reset_keeps_the_sample_rate_and_muted_channels() {
        let mut gba = Gba::new(&rom(&[LOOP]));
        gba.set_sample_rate(32_000);
        gba.set_channel_enabled(4, false);
        gba.soft_reset();
        assert_eq!(Console::sample_rate(&gba), 32_000);
        assert!(!gba.bus().sound().channel_enabled(4));
        assert!(gba.bus().sound().channel_enabled(0));
    }

    #[test]
    fn a_frame_gives_stereo_audio_at_the_sample_rate() {
        let mut gba = Gba::new(&rom(&[LOOP]));
        gba.set_sample_rate(48_000);
        let mut out = Vec::new();
        gba.run_frame();
        gba.take_audio(&mut out);
        out.clear();
        gba.run_frame();
        gba.take_audio(&mut out);
        assert_eq!(out.len() % 2, 0);
        assert!((1600..=1612).contains(&out.len()), "{}", out.len());
    }

    #[test]
    fn the_console_names_the_six_sound_channels() {
        let gba = Gba::new(&rom(&[LOOP]));
        assert_eq!(gba.audio_channels().len(), 6);
        assert_eq!(gba.audio_channels()[4], "Direct Sound A");
    }

    #[test]
    fn the_console_is_a_240_by_160_gba() {
        let gba = Gba::new(&rom(&[LOOP]));
        assert_eq!(gba.kind(), ConsoleKind::Gba);
        assert_eq!(Console::video(&gba).width, 240);
        assert_eq!(Console::video(&gba).height, 160);
        assert_eq!(Console::frame(&gba).len(), 240 * 160 * 4);
        assert_eq!(gba.timing().cycles_per_frame, 280_896);
    }

    #[test]
    fn the_title_is_the_header_name_up_to_its_first_zero() {
        let mut data = rom(&[LOOP]);
        data.resize(0xC0, 0);
        data[0xA0..0xA7].copy_from_slice(b"TOBU GB");
        let gba = Gba::new(&data);
        assert_eq!(gba.title(), "TOBU GB");
        assert_eq!(*gba.rom_bytes(), *data);
        assert!(gba.has_battery());
        assert!(!gba.has_clock());
    }

    #[cfg(feature = "persistence")]
    fn sram_game() -> Gba {
        let mut data = rom(&[LOOP]);
        data.extend_from_slice(b"SRAM_V113");
        data.resize(0x200, 0);
        Gba::new(&data)
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_write_to_the_backup_is_saved_once() {
        let mut gba = sram_game();
        assert!(gba.battery(false, 0).is_none());
        gba.bus.write8(0x0E00_0010, 0x42);
        let save = gba.battery(false, 0).expect("written");
        assert_eq!(save.ram.len(), 0x8000);
        assert_eq!(save.ram[0x10], 0x42);
        assert_eq!(save.clock, None);
        assert!(gba.battery(false, 0).is_none());
        assert_eq!(gba.battery(true, 0).expect("forced").ram, save.ram);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_forced_read_leaves_the_write_for_the_next_save() {
        let mut gba = sram_game();
        gba.bus.write8(0x0E00_0010, 0x42);
        assert_eq!(gba.battery(true, 0).expect("forced").ram[0x10], 0x42);
        let save = gba.battery(false, 0).expect("still unsaved");
        assert_eq!(save.ram[0x10], 0x42);
        assert!(gba.battery(false, 0).is_none());
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_put_battery_comes_back_from_the_backup() {
        let mut gba = sram_game();
        let mut ram = vec![0xFF; 0x8000];
        ram[3] = 7;
        gba.put_battery(gba.battery_from_file(&ram), 0);
        assert_eq!(gba.bus.read8(0x0E00_0003), 7);
        assert_eq!(gba.battery(true, 0).expect("forced").ram, ram);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_game_that_never_saved_has_nothing_to_store() {
        let mut gba = Gba::new(&rom(&[LOOP]));
        assert!(gba.battery(true, 0).is_none());
    }

    #[test]
    fn a_short_rom_has_no_title() {
        assert_eq!(Gba::new(&rom(&[LOOP])).title(), "");
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_gba_state_carries_the_magic_and_resumes_where_it_was_saved() {
        use crate::persistence::state::{GBA_STATE_MAGIC, GBA_STATE_VERSION};
        let program = timer_irq_program();
        let mut gba = Gba::new(&program);
        gba.bus_mut().write8(0x0E00_0010, 0x5A);
        for _ in 0..3 {
            gba.run_frame();
        }
        let state = gba.save_state(0).expect("state");
        assert_eq!(state[..4], GBA_STATE_MAGIC);
        assert_eq!(state[4], GBA_STATE_VERSION);

        let mut restored = Gba::new(&program).restore(&state).expect("restore");
        for _ in 0..5 {
            gba.run_frame();
            restored.run_frame();
        }
        assert_eq!(restored.cycles(), gba.cycles());
        assert_eq!(restored.frame(), gba.frame());
        assert_eq!(restored.bus().backup().bytes(), gba.bus().backup().bytes());
        assert_eq!(restored.bus().read8(0x0E00_0010), 0x5A);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_gba_state_from_a_newer_build_is_refused() {
        let mut gba = Gba::new(&rom(&[LOOP]));
        let mut state = gba.save_state(0).expect("state");
        state[4] += 1;
        assert!(matches!(
            gba.restore(&state),
            Err(crate::error::GbError::NewerState)
        ));
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_gba_state_from_an_older_build_is_refused() {
        let mut gba = Gba::new(&rom(&[LOOP]));
        let mut state = gba.save_state(0).expect("state");
        state[4] -= 1;
        assert!(matches!(
            gba.restore(&state),
            Err(crate::error::GbError::OlderState)
        ));
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_gba_state_loads_as_a_gba_machine() {
        use crate::machine::{Firmware, Machine};
        use crate::rom::Rom;
        let program = timer_irq_program();
        let mut machine = Machine::Gba(Box::new(Gba::new(&program)));
        machine.run_frame();
        let state = machine.save_state(0).expect("state");
        let firmware = Firmware::default();
        let loaded =
            Machine::from_state(&state, &Rom::new(&program), 0, &firmware).expect("from state");
        assert_eq!(loaded.kind(), ConsoleKind::Gba);
        let mut restored = machine.restore(&state, 0, &firmware).expect("restore");
        restored.run_frame();
        machine.run_frame();
        assert_eq!(restored.frame(), machine.frame());
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_gba_state_loads_with_the_user_bios() {
        use crate::gba::bios::{Bios, BiosKind, Boot, CULT_OF_GBA};
        use crate::machine::{Firmware, Machine};
        use crate::rom::Rom;
        let program = timer_irq_program();
        let state = Machine::Gba(Box::new(Gba::new(&program)))
            .save_state(0)
            .expect("state");
        let firmware = Firmware {
            gba_bios: Some(Bios::user(CULT_OF_GBA).expect("16 KB")),
            gba_boot: Boot::Skip,
            ..Firmware::default()
        };
        let Machine::Gba(loaded) =
            Machine::from_state(&state, &Rom::new(&program), 0, &firmware).expect("from state")
        else {
            panic!("not a gba");
        };
        assert_eq!(loaded.bios(), BiosKind::User);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_battery_save_names_its_chip_and_settles_an_undetected_cartridge() {
        use crate::gba::save::SaveType;
        let program = rom(&[LOOP]);
        let mut gba = Gba::new(&program);
        gba.bus_mut().write8(0x0E00_0001, 0x42);
        let save = gba.battery(false, 0).expect("save");
        assert_eq!(save.chip, Some(SaveType::Sram32K));

        let mut fresh = Gba::new(&program);
        let mut flash = save.clone();
        flash.chip = Some(SaveType::Flash64K);
        flash.ram = vec![0x11; 0x1_0000];
        fresh.put_battery(flash, 0);
        assert_eq!(fresh.bus().backup().kind(), SaveType::Flash64K);
        assert_eq!(fresh.bus().read8(0x0E00_0123), 0x11);
    }

    #[test]
    fn the_screen_starts_in_forced_blank() {
        let mut gba = Gba::new(&rom(&[LOOP]));
        gba.run_frame();
        assert!(gba.frame().iter().all(|&byte| byte == 0xFF));
    }

    #[test]
    fn a_program_that_draws_in_mode_3_shows_its_pixel() {
        let mut gba = Gba::new(&rom(&[
            0xE3A0_0301,
            0xE3A0_1B01,
            0xE381_1003,
            0xE1C0_10B0,
            0xE3A0_0406,
            0xE3A0_101F,
            0xE1C0_10B0,
            LOOP,
        ]));
        gba.run_frame();
        gba.run_frame();
        assert_eq!(gba.frame()[..8], [0xFF, 0, 0, 0xFF, 0, 0, 0, 0xFF]);
    }

    #[test]
    fn a_timer_read_counts_the_cycles_of_its_own_instruction() {
        let mut gba = Gba::new(&rom(&[
            0xE3A0_3301,
            0xE3A0_0502,
            0xE583_0100,
            0xE593_1100,
            LOOP,
        ]));
        for _ in 0..4 {
            gba.step();
        }
        assert_eq!(gba.cpu().reg(1), 0x0080_0007);
    }

    #[cfg(all(feature = "persistence", feature = "brotli"))]
    #[test]
    fn a_gba_state_saves_in_a_small_part_of_the_slowest_brotli() {
        use crate::persistence::state::SAVE_QUALITY;
        let mut gba = Gba::new(&rom(&[LOOP]));
        let mut x = 0x1234_5678u32;
        for offset in 0..0x4_0000u32 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let value = if offset % 64 < 40 { (x % 12) as u8 } else { 0 };
            gba.bus_mut().write8(0x0200_0000 + offset, value);
        }
        let started = std::time::Instant::now();
        let state = gba.save_state(0).expect("state");
        let saved = started.elapsed();
        let started = std::time::Instant::now();
        gba.state_body(0)
            .expect("body")
            .pack(SAVE_QUALITY)
            .expect("pack");
        let slowest = started.elapsed();
        assert!(
            saved * 10 < slowest,
            "saved in {saved:?}, the slowest brotli took {slowest:?}"
        );
        assert!(Gba::new(&rom(&[LOOP])).restore(&state).is_ok());
    }

    #[test]
    fn a_halt_that_a_dma_ends_runs_the_held_instruction_as_the_dma_lets_go() {
        let mut gba = Gba::new(&rom(&[
            0xE3A0_0301,
            0xE3A0_1010,
            0xE1C0_10B4,
            0xE280_2C02,
            0xE3A0_1002,
            0xE1C2_10B0,
            0xE280_30D4,
            0xE3A0_1302,
            0xE583_1000,
            0xE3A0_1402,
            0xE583_1004,
            0xE3A0_1C02,
            0xE1C3_10B8,
            0xE3A0_1CA4,
            0xE1C3_10BA,
            0xE5C0_1301,
            0xE3A0_5001,
            LOOP,
        ]));
        let limit = gba.cycles() + CYCLES_PER_FRAME;
        while gba.power() == Power::Running && gba.cycles() < limit {
            gba.step();
        }
        while !gba.bus().dma_active() && gba.cycles() < limit {
            gba.step();
        }
        assert_eq!(gba.power(), Power::Halted);
        gba.step();
        assert_eq!(gba.power(), Power::Running);
        assert_eq!(gba.cpu().reg(5), 1);
    }

    fn busy_program() -> Vec<u8> {
        rom(&[
            0xE3A0_0301,
            0xE59F_1094,
            0xE59F_2094,
            0xE582_1000,
            0xE3A0_1C64,
            0xE381_1038,
            0xE1C0_10B4,
            0xE3A0_1080,
            0xE1C0_18B4,
            0xE3A0_1CFF,
            0xE381_1077,
            0xE1C0_18B0,
            0xE3A0_1002,
            0xE1C0_18B2,
            0xE3A0_1CF0,
            0xE381_1080,
            0xE1C0_16B2,
            0xE3A0_1B21,
            0xE1C0_16B4,
            0xE3A0_1408,
            0xE580_10D4,
            0xE3A0_1405,
            0xE580_10D8,
            0xE3A0_1002,
            0xE1C0_1DBC,
            0xE3A0_18C0,
            0xE381_1CF0,
            0xE580_1100,
            0xE280_2C02,
            0xE3A0_100F,
            0xE1C2_10B0,
            0xE3A0_1001,
            0xE1C2_10B8,
            0xE3A0_8C02,
            0xE258_8001,
            0x1AFF_FFFD,
            0xE3A0_1000,
            0xE5C0_1301,
            0xE3A0_8C02,
            0xEAFF_FFF9,
            0x0800_00A8,
            0x0300_7FFC,
            0xE285_5001,
            0xE3A0_0301,
            0xE3A0_1902,
            0xE1C0_1DBE,
            0xE1D0_30B6,
            0xE086_6003,
            0xE280_2C01,
            0xE1D2_30B0,
            0xE087_7003,
            0xE280_2C02,
            0xE1D2_10B2,
            0xE1C2_10B2,
            0xE12F_FF1E,
        ])
    }

    #[cfg(feature = "persistence")]
    fn run_frame_settling_every_step(gba: &mut Gba) {
        gba.bus.serial_mut().traffic_mut().next_frame();
        gba.catch_up_frame_end();
        while gba.cycles < gba.frame_end && gba.step() > 0 {}
        gba.rumble = gba.bus.cart_mut().take_rumble();
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_frame_run_in_quiet_spans_matches_settling_every_step() {
        for program in [timer_irq_program(), busy_program()] {
            let [mut lazy, mut settled] = [(); 2].map(|()| {
                let mut gba = Gba::new(&program);
                #[cfg(feature = "jit")]
                gba.set_jit(false);
                gba.set_sample_rate(48_000);
                gba
            });
            let [mut lazy_audio, mut settled_audio] = [Vec::new(), Vec::new()];
            for frame in 0..40 {
                lazy.run_frame();
                run_frame_settling_every_step(&mut settled);
                lazy.take_audio(&mut lazy_audio);
                settled.take_audio(&mut settled_audio);
                let state = |gba: &Gba| rmp_serde::to_vec(&(&gba.cpu, &gba.bus)).expect("state");
                assert_eq!(lazy.cycles(), settled.cycles(), "frame {frame}");
                assert!(state(&lazy) == state(&settled), "frame {frame}");
                assert!(lazy_audio == settled_audio, "frame {frame}");
            }
            assert_ne!(lazy.cpu().reg(5), 0);
        }
    }
}
