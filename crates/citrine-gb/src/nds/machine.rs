use super::Cpu;
use super::aux_spi::AuxDevice;
use super::boot::{Boot, Header};
use super::bus::Buses;
use super::card::{Cartridge, KeyTable};
use super::download::Download;
use super::ir::{IrPort, NoLink};
use super::sound::output::Output;
use super::spi::Spi;
use super::spi::firmware::online::Online;
use super::spi::firmware::{Firmware, UserSettings};
use super::spi::power::PowerModel;
use super::three_d::Scene;
use super::video::color::rgba;
use super::video::{Composite, HEIGHT, WIDTH};
use super::wifi::Wifi;
use super::wifi::air::{NoPeer, WifiPeer};
use crate::console::{Keys, Touch};
#[cfg(feature = "debug")]
use crate::debug::Debugger;
use crate::error::{GbError, GbResult};
use crate::gba::cpu::{Arm9, Core, Cpu as Arm7, CpuBus, Exception};
#[cfg(jit)]
use crate::jit::{Jit, JitStats};
#[cfg(feature = "jit")]
pub use crate::jit_cache::{CacheError, JitCache};
#[cfg(feature = "rom-id")]
use crate::rom::id::RomId;
use crate::traffic::Ring;
use citrine_gb_threads::{Side, Split};
pub use idle::IdleStats;
use link::AirPort;
use std::sync::Arc;

mod cheats;
mod console;
#[cfg(feature = "debug")]
mod debug;
mod download;
mod idle;
mod link;
#[cfg(feature = "jit-lockstep")]
pub mod lockstep;
#[cfg(feature = "persistence")]
mod rollback;
#[cfg(feature = "persistence")]
mod state;
#[cfg(feature = "persistence")]
pub use state::StateCopy;
#[cfg(test)]
pub(crate) mod test_rom;

pub const FREE_ARM9_BIOS: &[u8] = include_bytes!("../../assets/nds-bios/freebios9.bin");
pub const FREE_ARM7_BIOS: &[u8] = include_bytes!("../../assets/nds-bios/freebios7.bin");
pub const SCREEN_PIXELS: usize = WIDTH * HEIGHT;
pub const FRAME_BYTES: usize = SCREEN_PIXELS * 2 * 4;
pub const JIT_AVAILABLE: bool = cfg!(jit);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JitCounts {
    pub blocks: u64,
    pub compiled: u64,
    pub stepped: u64,
    pub compile_time: std::time::Duration,
    pub threads: usize,
}

#[cfg(jit)]
impl JitCounts {
    fn of(jits: &Jits) -> Self {
        Self::over(&[&jits.arm9, &jits.arm7])
    }

    pub(crate) fn over(jits: &[&crate::jit::Jit]) -> Self {
        jits.iter().fold(Self::default(), |counts, jit| {
            let stats = jit.stats();
            Self {
                blocks: counts.blocks + stats.compiles,
                compiled: counts.compiled + stats.retired,
                stepped: counts.stepped + stats.stepped,
                compile_time: counts.compile_time
                    + std::time::Duration::from_nanos(stats.compile_nanos),
                threads: counts.threads + jit.compile_threads(),
            }
        })
    }
}

const SLICE: u32 = 64;
const PAINTERS: usize = 2;
const EXTRA_RASTERS: usize = 2;
#[cfg(jit)]
const ARM_BRANCH_TO_SELF: u32 = 0xEAFF_FFFE;
#[cfg(jit)]
const THUMB_BRANCH_TO_SELF: u32 = 0xE7FE;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NdsFirmware {
    pub arm9_bios: Option<Vec<u8>>,
    pub arm7_bios: Option<Vec<u8>>,
    pub firmware: Option<Vec<u8>>,
    pub user_settings: Option<UserSettings>,
    pub online: Online,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Fault {
    pub cpu: Cpu,
    pub pc: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JitBlocks {
    pub arm: u64,
    pub thumb: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SideWork {
    pub lines_2d: u64,
    pub lines_3d: u64,
    pub geometry: u64,
}

pub struct Nds {
    idle_skips: u64,
    arm9: Arm9,
    arm7: Arm7,
    buses: Buses,
    rom: Arc<[u8]>,
    firmware: NdsFirmware,
    header: Header,
    arm9_cycles: u64,
    arm7_cycles: u64,
    now: u64,
    frames: u64,
    frame: Box<[u8]>,
    published: Vec<[u32; WIDTH]>,
    faults: [Option<Fault>; 2],
    audio: Output,
    idle: idle::IdleLoops,
    air: Option<AirPort>,
    download: Option<Box<Download>>,
    slice_limit: u32,
    blind: bool,
    torn: bool,
    #[cfg(feature = "rom-id")]
    rom_id: RomId,
    #[cfg(feature = "rom-id")]
    game_sha1: [u8; 20],
    #[cfg(feature = "debug")]
    debugger: Debugger,
    #[cfg(feature = "debug")]
    debug: debug::NdsDebug,
    #[cfg(jit)]
    jit: Option<Box<Jits>>,
    #[cfg(jit)]
    jit_cache: Option<Arc<JitCache>>,
    #[cfg(feature = "jit-lockstep")]
    lockstep: lockstep::Lockstep,
}

fn firmware_image(firmware: &NdsFirmware) -> Firmware {
    let image = match (&firmware.firmware, &firmware.user_settings) {
        (Some(image), Some(settings)) => Firmware::with_user_settings(image, settings),
        (Some(image), None) => Firmware::new(image),
        (None, settings) => {
            Firmware::built_in(settings.as_ref().unwrap_or(&UserSettings::default()))
        }
    };
    let mut bytes = image.bytes().to_vec();
    firmware.online.apply(&mut bytes);
    Firmware::new(&bytes)
}

impl Nds {
    pub fn new(rom: Arc<[u8]>, firmware: NdsFirmware) -> GbResult<Self> {
        Self::from_rom(&crate::rom::Rom::from(rom), firmware)
    }

    pub fn from_rom(file: &crate::rom::Rom, firmware: NdsFirmware) -> GbResult<Self> {
        Self::boot(file, firmware, None)
    }

    fn boot(
        file: &crate::rom::Rom,
        firmware: NdsFirmware,
        parent: Option<&[u8]>,
    ) -> GbResult<Self> {
        let rom = Arc::clone(&file.data);
        let header = Header::parse(&rom).ok_or(GbError::NotNds)?;
        let mut buses = Buses::default();
        buses.load_arm9_bios(firmware.arm9_bios.as_deref().unwrap_or(FREE_ARM9_BIOS));
        buses.load_arm7_bios(firmware.arm7_bios.as_deref().unwrap_or(FREE_ARM7_BIOS));
        let key_table = firmware
            .arm7_bios
            .as_deref()
            .and_then(KeyTable::from_arm7_bios);
        let image = firmware_image(&firmware);
        let firmware_bytes = image.bytes().to_vec();
        *buses.wifi_mut() = Wifi::new(&image);
        *buses.spi_mut() = Spi::new(image, PowerModel::Ds);
        let aux = AuxDevice::for_game(header.game_code.to_le_bytes());
        *buses.cart_mut() = Cartridge::with_rom(Arc::clone(&rom), key_table.as_ref(), aux);
        let (arm9, arm7) = Boot {
            rom: &rom,
            key_table: key_table.as_ref(),
            free_bios: firmware.arm9_bios.is_none(),
            firmware: &firmware_bytes,
            parent,
        }
        .run(&header, &mut buses);
        #[cfg(feature = "rom-id")]
        let (rom_id, game_sha1) = file.ids();
        let mut nds = Self {
            arm9,
            arm7,
            buses,
            #[cfg(feature = "rom-id")]
            game_sha1,
            #[cfg(feature = "rom-id")]
            rom_id,
            rom,
            firmware,
            header,
            arm9_cycles: 0,
            arm7_cycles: 0,
            now: 0,
            frames: 0,
            frame: vec![0; FRAME_BYTES].into_boxed_slice(),
            published: Vec::new(),
            faults: [None; 2],
            idle_skips: 0,
            audio: Output::default(),
            idle: idle::IdleLoops::default(),
            air: None,
            download: None,
            slice_limit: u32::MAX,
            blind: false,
            torn: false,
            #[cfg(feature = "debug")]
            debugger: Debugger::default(),
            #[cfg(feature = "debug")]
            debug: debug::NdsDebug::default(),
            #[cfg(jit)]
            jit: (!cfg!(windows)).then(Box::default),
            #[cfg(jit)]
            jit_cache: None,
            #[cfg(feature = "jit-lockstep")]
            lockstep: lockstep::Lockstep::default(),
        };
        nds.publish();
        Ok(nds)
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn arm9(&self) -> &Arm9 {
        &self.arm9
    }

    pub fn arm7(&self) -> &Arm7 {
        &self.arm7
    }

    pub fn buses(&self) -> &Buses {
        &self.buses
    }

    pub fn buses_mut(&mut self) -> &mut Buses {
        &mut self.buses
    }

    pub fn three_d_scene(&self) -> Scene<'_> {
        self.buses.three_d_scene()
    }

    pub fn record_composite(&mut self, on: bool) {
        if !on {
            self.finish_picture();
        }
        self.buses.record_composite(on);
    }

    pub fn defer_3d(&mut self, on: bool) {
        self.buses.defer_3d(on);
    }

    pub fn finish_picture(&mut self) -> bool {
        let filled = self.buses.fill_3d();
        if filled && !self.blind {
            self.publish();
        }
        filled
    }

    pub fn composite(&self) -> Option<&Composite> {
        self.buses.screens().composite()
    }

    pub fn faults(&self) -> [Option<Fault>; 2] {
        self.faults
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn cycles(&self) -> u64 {
        self.now
    }

    pub fn traffic(&self) -> Vec<Ring<'_>> {
        self.buses.traffic().to_vec()
    }

    pub fn traffic_clock(&self) -> u64 {
        self.buses.traffic_clock()
    }

    pub fn run_frame(&mut self) {
        self.run_frame_with_ir(&mut NoLink);
    }

    pub fn run_frame_with_ir(&mut self, port: &mut dyn IrPort) {
        self.run_frame_with(port, &mut NoPeer);
    }

    pub fn run_frame_on(
        &mut self,
        port: &mut dyn IrPort,
        peer: &mut dyn WifiPeer,
        split: &mut Split<'_>,
    ) {
        if !split.has_pool() {
            self.run_frame_with(port, peer);
            return;
        }
        let painters = split.lanes().saturating_sub(1).min(PAINTERS);
        let rasters = 1 + split
            .lanes()
            .saturating_sub(1 + painters)
            .min(EXTRA_RASTERS);
        let (rasters, ending) = self.buses.lend_3d(rasters);
        let (painters, lines_ending) = self.buses.lend_lines(painters);
        let mut sides: Vec<(&str, Side<'_>)> = Vec::new();
        for raster in rasters {
            sides.push(("3d", Box::new(move || raster.run())));
        }
        for painter in painters {
            sides.push(("2d lines", Box::new(move || painter.run())));
        }
        split.beside(sides, || {
            let ended = self.run_frame_slices(port, peer);
            drop(ending);
            drop(lines_ending);
            if ended {
                self.end_frame();
            }
        });
        self.buses.home_lines();
        self.buses.home_3d();
    }

    pub fn run_frame_with(&mut self, port: &mut dyn IrPort, peer: &mut dyn WifiPeer) {
        if self.run_frame_slices(port, peer) {
            self.end_frame();
        }
    }

    fn run_frame_slices(&mut self, port: &mut dyn IrPort, peer: &mut dyn WifiPeer) -> bool {
        let delivers = peer.delivers();
        loop {
            let frame = self.run_slice();
            self.buses.cart_mut().exchange_ir(port);
            let at = self.buses.traffic_clock();
            let chip = self.buses.wifi_mut();
            if chip.powered() {
                chip.traffic_mut().set_clock(at);
                if delivers || chip.sending() {
                    peer.exchange(chip);
                }
            }
            if frame {
                return true;
            }
            #[cfg(feature = "debug")]
            if self.debugger.hit_breakpoint {
                return false;
            }
        }
    }

    fn end_frame(&mut self) {
        let muted = self.buses.sound_muted();
        for sample in self.buses.take_sound_samples() {
            if muted {
                self.audio.hold(sample);
            } else {
                self.audio.push(sample);
            }
        }
        self.frames += 1;
        self.buses.next_traffic_frame();
        self.buses.finish_lines();
        self.buses.settle_3d();
        let torn = std::mem::take(&mut self.torn);
        if !self.blind && !torn {
            self.publish();
        }
    }

    pub fn frame(&self) -> &[u8] {
        &self.frame
    }

    pub fn keys(&self) -> Keys {
        self.buses.keys()
    }

    pub fn set_keys(&mut self, keys: Keys) {
        self.buses.set_keys(keys);
    }

    pub fn set_touch(&mut self, touch: Option<Touch>) {
        self.buses
            .set_pen(touch.map(|touch| (touch.x, touch.y.min(HEIGHT as u8 - 1))));
    }

    pub fn set_clock(&mut self, unix: i64) {
        self.buses.rtc_mut().set_unix(unix);
    }

    pub fn hold_clock(&mut self, secs: f64) -> f64 {
        self.buses.rtc_mut().hold(secs)
    }

    pub fn take_clock_set(&mut self) -> Option<i64> {
        let rtc = self.buses.rtc_mut();
        rtc.take_set_by_game().then(|| rtc.unix())
    }

    pub fn soft_reset(&mut self) {
        let keys = self.keys();
        if let Ok(mut fresh) = Self::new(Arc::clone(&self.rom), self.firmware.clone()) {
            let sample_rate = self.audio.sample_rate();
            fresh.buses.cart_mut().adopt_aux(self.buses.cart_mut());
            fresh.buses.adopt_traffic(&mut self.buses);
            #[cfg(jit)]
            {
                fresh.jit = self.jit.as_deref().map(|jit| Box::new(jit.fork()));
                fresh.jit_cache = self.jit_cache.clone();
            }
            #[cfg(feature = "debug")]
            let (debugger, debug) = (
                std::mem::take(&mut self.debugger),
                std::mem::take(&mut self.debug),
            );
            *self = fresh;
            #[cfg(feature = "debug")]
            {
                self.debugger = debugger;
                self.debugger.soft_reset();
                self.debug = debug.after_reset();
            }
            self.audio.set_sample_rate(sample_rate);
            self.set_keys(keys);
        }
    }

    pub fn rom_bytes(&self) -> Arc<[u8]> {
        Arc::clone(&self.rom)
    }

    pub fn title(&self) -> String {
        self.header.title.clone()
    }

    pub fn take_audio(&mut self, out: &mut Vec<f32>) {
        self.audio.take(out);
    }

    pub fn sample_rate(&self) -> u32 {
        self.audio.sample_rate()
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.audio.set_sample_rate(rate);
    }

    #[cfg(feature = "jit")]
    pub fn set_jit(&mut self, on: bool) {
        #[cfg(jit)]
        let on = on && JIT_AVAILABLE;
        #[cfg(jit)]
        if on != self.jit.is_some() {
            self.jit = on.then(|| self.cached_jit());
        }
        #[cfg(not(jit))]
        let _ = on;
    }

    #[cfg(feature = "jit")]
    pub fn compile_jit_on(&mut self, spawner: &citrine_gb_threads::Spawner) {
        #[cfg(jit)]
        if let Some(jit) = self.jit.as_deref_mut() {
            jit.compile_on(spawner);
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
    fn cached_jit(&self) -> Box<Jits> {
        let mut jits = Box::<Jits>::default();
        jits.set_cache(self.jit_cache.clone());
        jits
    }

    #[cfg(jit)]
    pub fn jit_stats(&self) -> Option<JitStats> {
        self.jit.as_deref().map(|jits| jits.arm9.stats())
    }

    #[cfg(jit)]
    pub fn arm7_jit_stats(&self) -> Option<JitStats> {
        self.jit.as_deref().map(|jits| jits.arm7.stats())
    }

    #[cfg(feature = "jit")]
    pub fn jit_counts(&self) -> Option<JitCounts> {
        #[cfg(jit)]
        return self.jit.as_deref().map(JitCounts::of);
        #[cfg(not(jit))]
        None
    }

    #[cfg(feature = "jit")]
    pub fn jit_blocks(&self) -> JitBlocks {
        #[cfg(jit)]
        return self
            .jit_stats()
            .map_or(JitBlocks::default(), |stats| JitBlocks {
                arm: stats.compiles - stats.thumb_compiles,
                thumb: stats.thumb_compiles,
            });
        #[cfg(not(jit))]
        JitBlocks::default()
    }

    pub fn idle_skips(&self) -> u64 {
        self.idle_skips
    }

    pub fn side_work(&self) -> SideWork {
        self.buses.side_work()
    }

    #[cfg(feature = "jit")]
    pub fn jit_on(&self) -> bool {
        #[cfg(jit)]
        return self.jit.is_some();
        #[cfg(not(jit))]
        false
    }

    #[cfg(feature = "jit-lockstep")]
    pub fn lockstep(&mut self) -> &mut lockstep::Lockstep {
        &mut self.lockstep
    }

    fn run_slice(&mut self) -> bool {
        if self.buses.sleeping() {
            return self.sleep_slice();
        }
        let running = [Cpu::Arm9, Cpu::Arm7].map(|cpu| self.awake(cpu));
        let mut until = self.buses.until_event().min(self.slice_limit);
        if running.iter().any(|&awake| awake) {
            until = until.min(SLICE);
        }
        let mut target = self.now + u64::from(until);
        for (cpu, awake) in [Cpu::Arm9, Cpu::Arm7].into_iter().zip(running) {
            if awake {
                if let Some(start) = self.run_cpu(cpu, scaled(cpu, target)) {
                    target = target.min(unscaled(cpu, *self.clock_mut(cpu)));
                    let start = start / scaled(cpu, 1);
                    self.buses
                        .slice_cut_since(target.saturating_sub(start) as u32);
                }
            } else {
                let clock = self.clock_mut(cpu);
                *clock = (*clock).max(scaled(cpu, target));
            }
        }
        let until = (target - self.now) as u32;
        let frame = self.buses.elapse(until);
        self.charge_stalls();
        self.now = target;
        frame
    }

    fn charge_stalls(&mut self) {
        for cpu in [Cpu::Arm9, Cpu::Arm7] {
            let stall = self.buses.take_stall(cpu);
            *self.clock_mut(cpu) += u64::from(stall);
        }
    }

    fn sleep_slice(&mut self) -> bool {
        let until = self.buses.until_sleep_frame().min(self.slice_limit);
        self.now += u64::from(until);
        for cpu in [Cpu::Arm9, Cpu::Arm7] {
            let target = scaled(cpu, self.now);
            let clock = self.clock_mut(cpu);
            *clock = (*clock).max(target);
        }
        self.buses.sleep(until)
    }

    fn awake(&mut self, cpu: Cpu) -> bool {
        self.faults[side(cpu)].is_none() && (!self.buses.halted(cpu) || self.buses.wake(cpu))
    }

    fn clock_mut(&mut self, cpu: Cpu) -> &mut u64 {
        match cpu {
            Cpu::Arm9 => &mut self.arm9_cycles,
            Cpu::Arm7 => &mut self.arm7_cycles,
        }
    }

    fn stops(&self, cpu: Cpu) -> bool {
        let ends = match cpu {
            Cpu::Arm9 => self.arm9.ends(),
            Cpu::Arm7 => self.arm7.ends(),
        };
        ends || self.buses.halted(cpu)
    }

    fn pc_of(&self, cpu: Cpu) -> u32 {
        match cpu {
            Cpu::Arm9 => self.arm9.pc(),
            Cpu::Arm7 => self.arm7.pc(),
        }
    }

    fn run_cpu(&mut self, cpu: Cpu, target: u64) -> Option<u64> {
        #[cfg(jit)]
        if self.jit.is_some() {
            return match cpu {
                Cpu::Arm9 => self.run_side::<true>(target),
                Cpu::Arm7 => self.run_side::<false>(target),
            };
        }
        match cpu {
            Cpu::Arm9 => self.interpret::<true>(target),
            Cpu::Arm7 => self.interpret::<false>(target),
        }
    }

    fn interpret<const ARM9: bool>(&mut self, target: u64) -> Option<u64> {
        let cpu = if ARM9 { Cpu::Arm9 } else { Cpu::Arm7 };
        let armed = self.buses.slice_cut();
        let mut stop = if ARM9 {
            let finish = self.buses.next_geometry_finish();
            target.min(self.geometry_at(finish))
        } else {
            target
        };
        if let Some(ended) = self.idle_slice::<ARM9>(stop, target, armed) {
            return ended;
        }
        #[cfg(feature = "debug")]
        let watched = self.watches_instructions(cpu);
        loop {
            if *self.clock_mut(cpu) >= stop && self.stops(cpu) {
                if !ARM9 || self.arm9_cycles >= target {
                    return None;
                }
                let offset = (self.arm9_cycles / 2 - self.now) as u32;
                let finish = self.buses.finish_geometry(offset);
                self.arm9_cycles += u64::from(self.buses.take_stall(Cpu::Arm9));
                stop = target.min(self.geometry_at(finish));
                if let Some(ended) = self.idle_slice::<ARM9>(stop, target, armed) {
                    return ended;
                }
                continue;
            }
            if self.buses.halted(cpu) && !self.buses.wake(cpu) {
                *self.clock_mut(cpu) = stop;
                continue;
            }
            #[cfg(feature = "debug")]
            if watched {
                let before = self.pc_of(cpu);
                let start = *self.clock_mut(cpu);
                if self.execute(cpu) == 0 {
                    *self.clock_mut(cpu) = target;
                    return None;
                }
                if !armed && self.buses.slice_cut() {
                    return Some(start);
                }
                if self.after_instruction(cpu, before) {
                    return None;
                }
                continue;
            }
            let irq = self.buses.irq_line(cpu);
            match self.burst::<ARM9>(stop, irq) {
                Burst::Fault => {
                    self.faults[side(cpu)] = Some(Fault {
                        cpu,
                        pc: self.pc_of(cpu),
                    });
                    *self.clock_mut(cpu) = target;
                    return None;
                }
                Burst::Attention(start) if !armed && self.buses.slice_cut() => {
                    return Some(start);
                }
                Burst::Attention(_) | Burst::Done => {}
            }
        }
    }

    #[inline(always)]
    fn burst<const ARM9: bool>(&mut self, target: u64, irq: bool) -> Burst {
        if ARM9 {
            let mut bus = self.buses.arm9();
            run_burst(
                &mut self.arm9,
                &mut bus,
                &mut self.arm9_cycles,
                target,
                irq,
                |bus| bus.take_events(),
                #[cfg(feature = "jit-lockstep")]
                &mut self.lockstep.arm9,
            )
        } else {
            let mut bus = self.buses.arm7();
            run_burst(
                &mut self.arm7,
                &mut bus,
                &mut self.arm7_cycles,
                target,
                irq,
                |bus| bus.take_events(),
                #[cfg(feature = "jit-lockstep")]
                &mut self.lockstep.arm7,
            )
        }
    }

    #[cfg(jit)]
    fn run_side<const ARM9: bool>(&mut self, target: u64) -> Option<u64> {
        let cpu = if ARM9 { Cpu::Arm9 } else { Cpu::Arm7 };
        let armed = self.buses.slice_cut();
        let mut stop = if ARM9 {
            let finish = self.buses.next_geometry_finish();
            target.min(self.geometry_at(finish))
        } else {
            target
        };
        if self.spins(cpu) {
            self.skip_spin(cpu, stop);
        }
        #[cfg(feature = "debug")]
        let watched = self.watches_instructions(cpu);
        #[cfg(jit)]
        let mut straight = false;
        loop {
            if *self.clock_mut(cpu) >= stop && self.stops(cpu) {
                if !ARM9 || self.arm9_cycles >= target {
                    return None;
                }
                let offset = (self.arm9_cycles / 2 - self.now) as u32;
                let finish = self.buses.finish_geometry(offset);
                self.arm9_cycles += u64::from(self.buses.take_stall(Cpu::Arm9));
                stop = target.min(self.geometry_at(finish));
                if self.spins(cpu) {
                    self.skip_spin(cpu, stop);
                }
                continue;
            }
            if self.buses.halted(cpu) && !self.buses.wake(cpu) {
                *self.clock_mut(cpu) = stop;
                continue;
            }
            #[cfg(jit)]
            if !straight {
                let start = *self.clock_mut(cpu);
                if let Some(last) = self.run_jit::<ARM9>(stop) {
                    if !armed && self.buses.slice_cut() {
                        return Some(start + u64::from(last));
                    }
                    continue;
                }
            }
            #[cfg(jit)]
            let from = self.pc_of(cpu);
            #[cfg(feature = "debug")]
            let before = if watched { self.pc_of(cpu) } else { 0 };
            let start = *self.clock_mut(cpu);
            if self.execute(cpu) == 0 {
                *self.clock_mut(cpu) = target;
                return None;
            }
            #[cfg(jit)]
            {
                let thumb = if ARM9 {
                    self.arm9.thumb()
                } else {
                    self.arm7.thumb()
                };
                let size = if thumb { 2 } else { 4 };
                straight = self.pc_of(cpu) == from.wrapping_add(size);
                if let Some(jits) = self.jit.as_deref_mut() {
                    jits.of::<ARM9>().stepped();
                }
            }
            if !armed && self.buses.slice_cut() {
                return Some(start);
            }
            #[cfg(feature = "debug")]
            if watched && self.after_instruction(cpu, before) {
                return None;
            }
        }
    }

    fn geometry_at(&self, finish: Option<u32>) -> u64 {
        finish.map_or(u64::MAX, |cycles| {
            scaled(Cpu::Arm9, self.now + u64::from(cycles))
        })
    }

    #[cfg(jit)]
    fn spins(&self, cpu: Cpu) -> bool {
        let (op, pc, thumb) = match cpu {
            Cpu::Arm9 => (self.arm9.pipeline()[0], self.arm9.pc(), self.arm9.thumb()),
            Cpu::Arm7 => (self.arm7.pipeline()[0], self.arm7.pc(), self.arm7.thumb()),
        };
        if (op as u16 | 0x1800) != 0xFFFE {
            return false;
        }
        let branch_to_self = if thumb {
            op & 0xFFFF == THUMB_BRANCH_TO_SELF
        } else {
            op == ARM_BRANCH_TO_SELF
        };
        branch_to_self
            && !self.buses.halted(cpu)
            && matches!(pc >> 24, 0x00..=0x03 | 0xFF)
            && !self.buses.irq_line(cpu)
            && self.no_analysis()
    }

    #[cfg(jit)]
    #[inline(never)]
    fn skip_spin(&mut self, cpu: Cpu, target: u64) {
        let mut cost = 0;
        for _ in 0..2 {
            let start = *self.clock_mut(cpu);
            if start >= target {
                return;
            }
            if self.execute(cpu) == 0 {
                *self.clock_mut(cpu) = target;
                return;
            }
            cost = *self.clock_mut(cpu) - start;
        }
        let clock = self.clock_mut(cpu);
        if *clock < target {
            *clock += cost * (target - *clock).div_ceil(cost);
            self.idle_skips += 1;
        }
    }

    #[cfg(jit)]
    fn run_jit<const ARM9: bool>(&mut self, target: u64) -> Option<u32> {
        let cpu = if ARM9 { Cpu::Arm9 } else { Cpu::Arm7 };
        if !self.no_debugger(cpu) {
            return None;
        }
        let masked = if ARM9 {
            self.arm9.irq_stays_masked()
        } else {
            self.arm7.irq_stays_masked()
        };
        if self.buses.irq_line(cpu) && !masked {
            return None;
        }
        #[cfg(feature = "jit-lockstep")]
        let start = self.pc_of(cpu);
        let clock = if ARM9 {
            self.arm9_cycles
        } else {
            self.arm7_cycles
        };
        let budget = target.saturating_sub(clock).min(u64::from(u32::MAX)) as u32;
        let jits = self.jit.as_deref_mut()?;
        let ran = if ARM9 {
            let ran = jits
                .arm9
                .run(&mut self.arm9, &mut self.buses.arm9(), budget)?;
            self.arm9_cycles += u64::from(ran.cycles);
            #[cfg(feature = "jit-lockstep")]
            self.lockstep
                .arm9
                .ran(start, ran.retired, &self.arm9, self.arm9_cycles);
            ran
        } else {
            let ran = jits
                .arm7
                .run(&mut self.arm7, &mut self.buses.arm7(), budget)?;
            self.arm7_cycles += u64::from(ran.cycles);
            #[cfg(feature = "jit-lockstep")]
            self.lockstep
                .arm7
                .ran(start, ran.retired, &self.arm7, self.arm7_cycles);
            ran
        };
        Some(ran.last)
    }

    #[cfg(all(jit, feature = "debug"))]
    fn no_debugger(&self, cpu: Cpu) -> bool {
        self.no_analysis() && self.debug.no_breakpoints(cpu)
    }

    #[cfg(all(jit, not(feature = "debug")))]
    fn no_debugger(&self, _cpu: Cpu) -> bool {
        true
    }

    #[cfg(all(jit, feature = "debug"))]
    fn no_analysis(&self) -> bool {
        !self.debugger.static_analysis_enabled
    }

    #[cfg(all(jit, not(feature = "debug")))]
    fn no_analysis(&self) -> bool {
        true
    }

    #[cfg(any(jit, feature = "debug"))]
    #[inline(always)]
    fn execute(&mut self, cpu: Cpu) -> u32 {
        let irq = self.buses.irq_line(cpu);
        self.execute_with(cpu, irq)
    }

    #[cfg(any(jit, feature = "debug"))]
    #[inline(always)]
    fn execute_with(&mut self, cpu: Cpu, irq: bool) -> u32 {
        #[cfg(feature = "jit-lockstep")]
        let interrupted;
        let (cycles, stall) = match cpu {
            Cpu::Arm9 => {
                let mut bus = self.buses.arm9();
                let entered = if irq {
                    self.arm9.interrupt(&mut bus, Exception::Irq)
                } else {
                    None
                };
                #[cfg(feature = "jit-lockstep")]
                {
                    interrupted = entered.is_some();
                }
                let cycles = entered.unwrap_or_else(|| self.arm9.step(&mut bus));
                (cycles, bus.take_stall())
            }
            Cpu::Arm7 => {
                let mut bus = self.buses.arm7();
                let entered = if irq {
                    self.arm7.interrupt(&mut bus, Exception::Irq)
                } else {
                    None
                };
                #[cfg(feature = "jit-lockstep")]
                {
                    interrupted = entered.is_some();
                }
                let cycles = entered.unwrap_or_else(|| self.arm7.step(&mut bus));
                (cycles, bus.take_stall())
            }
        };
        if cycles == 0 {
            self.faults[side(cpu)] = Some(Fault {
                cpu,
                pc: self.pc_of(cpu),
            });
        } else {
            *self.clock_mut(cpu) += u64::from(cycles + stall);
        }
        #[cfg(feature = "jit-lockstep")]
        if !interrupted {
            match cpu {
                Cpu::Arm9 => self.lockstep.arm9.stepped(&self.arm9, self.arm9_cycles),
                Cpu::Arm7 => self.lockstep.arm7.stepped(&self.arm7, self.arm7_cycles),
            }
        }
        cycles
    }

    fn publish(&mut self) {
        let screens = self.buses.screens();
        let known = self.published.len() == 2 * HEIGHT;
        self.published.resize(2 * HEIGHT, [0; WIDTH]);
        let mut published = self.published.iter_mut();
        let (top, bottom) = self
            .frame
            .as_chunks_mut::<4>()
            .0
            .split_at_mut(screens.top().len());
        for (pixels, colors) in [(top, screens.top()), (bottom, screens.bottom())] {
            let rows = pixels
                .as_chunks_mut::<WIDTH>()
                .0
                .iter_mut()
                .zip(colors.as_chunks::<WIDTH>().0);
            for ((pixels, colors), shown) in rows.zip(&mut published) {
                if known && shown == colors {
                    continue;
                }
                *shown = *colors;
                match uniform(colors) {
                    Some(color) => pixels.fill(rgba(color)),
                    None => {
                        for (pixel, &color) in pixels.iter_mut().zip(colors) {
                            *pixel = rgba(color);
                        }
                    }
                }
            }
        }
    }
}

fn uniform(row: &[u32; WIDTH]) -> Option<u32> {
    let first = row[0];
    let ends = first == row[row.len() / 2] && first == row[row.len() - 1];
    (ends && row.iter().fold(0, |diff, &color| diff | color ^ first) == 0).then_some(first)
}

#[cfg(jit)]
#[derive(Default)]
struct Jits {
    arm9: Jit,
    arm7: Jit,
}

#[cfg(jit)]
impl Jits {
    fn fork(&self) -> Self {
        Self {
            arm9: self.arm9.fork(),
            arm7: self.arm7.fork(),
        }
    }

    fn restart(&mut self) {
        self.arm9.restart();
        self.arm7.restart();
    }

    fn compile_on(&mut self, spawner: &citrine_gb_threads::Spawner) {
        self.arm9.compile_on(spawner, 2);
        self.arm7.compile_on(spawner, 1);
    }

    fn set_cache(&mut self, cache: Option<Arc<JitCache>>) {
        self.arm9.set_cache(cache.clone());
        self.arm7.set_cache(cache);
    }

    fn of<const ARM9: bool>(&mut self) -> &mut Jit {
        if ARM9 { &mut self.arm9 } else { &mut self.arm7 }
    }
}

fn scaled(cpu: Cpu, time: u64) -> u64 {
    match cpu {
        Cpu::Arm9 => time * 2,
        Cpu::Arm7 => time,
    }
}

fn unscaled(cpu: Cpu, time: u64) -> u64 {
    match cpu {
        Cpu::Arm9 => time.div_ceil(2),
        Cpu::Arm7 => time,
    }
}

fn side(cpu: Cpu) -> usize {
    match cpu {
        Cpu::Arm9 => 0,
        Cpu::Arm7 => 1,
    }
}

enum Burst {
    Done,
    Attention(u64),
    Fault,
}

#[inline(always)]
fn run_burst<const V5: bool, B: CpuBus>(
    core: &mut Core<V5>,
    bus: &mut B,
    clock: &mut u64,
    target: u64,
    irq: bool,
    settle: impl Fn(&mut B) -> (bool, u32),
    #[cfg(feature = "jit-lockstep")] track: &mut crate::jit::lockstep::Track<V5>,
) -> Burst {
    if irq {
        burst_steps::<V5, true, B>(
            core,
            bus,
            clock,
            target,
            settle,
            #[cfg(feature = "jit-lockstep")]
            track,
        )
    } else {
        burst_steps::<V5, false, B>(
            core,
            bus,
            clock,
            target,
            settle,
            #[cfg(feature = "jit-lockstep")]
            track,
        )
    }
}

#[inline(always)]
fn burst_steps<const V5: bool, const IRQ: bool, B: CpuBus>(
    core: &mut Core<V5>,
    bus: &mut B,
    clock: &mut u64,
    target: u64,
    settle: impl Fn(&mut B) -> (bool, u32),
    #[cfg(feature = "jit-lockstep")] track: &mut crate::jit::lockstep::Track<V5>,
) -> Burst {
    loop {
        if let Some(burst) = burst_step::<V5, IRQ, B>(
            core,
            bus,
            clock,
            &settle,
            #[cfg(feature = "jit-lockstep")]
            track,
        ) {
            return burst;
        }
        if *clock >= target {
            break;
        }
    }
    if core.ends() {
        return Burst::Done;
    }
    let mut over = core.overshoot();
    let burst = loop {
        if let Some(burst) = burst_step::<V5, IRQ, B>(
            core,
            bus,
            clock,
            &settle,
            #[cfg(feature = "jit-lockstep")]
            track,
        ) {
            break burst;
        }
        if core.overshot(&mut over) {
            break Burst::Done;
        }
    };
    core.end_overshoot(over);
    burst
}

#[inline(always)]
fn burst_step<const V5: bool, const IRQ: bool, B: CpuBus>(
    core: &mut Core<V5>,
    bus: &mut B,
    clock: &mut u64,
    settle: &impl Fn(&mut B) -> (bool, u32),
    #[cfg(feature = "jit-lockstep")] track: &mut crate::jit::lockstep::Track<V5>,
) -> Option<Burst> {
    let start = *clock;
    let entered = if IRQ {
        core.interrupt(bus, Exception::Irq)
    } else {
        None
    };
    #[cfg(feature = "jit-lockstep")]
    let interrupted = entered.is_some();
    let cycles = entered.unwrap_or_else(|| core.step_inlined(bus));
    let (attention, stall) = settle(bus);
    if (IRQ || !V5) && cycles == 0 {
        return Some(Burst::Fault);
    }
    debug_assert_ne!(cycles, 0);
    *clock += u64::from(cycles + stall);
    #[cfg(feature = "jit-lockstep")]
    if !interrupted {
        track.stepped(core, *clock);
    }
    attention.then_some(Burst::Attention(start))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::wait::Width;
    use crate::nds::lcd::FRAME_CYCLES;
    use crate::nds::machine::test_rom::SLEEP_ON_LID;
    use crate::nds::rtc::CYCLES_PER_SECOND;
    use crate::nds::sound::output::DEFAULT_SAMPLE_RATE;

    const ARM9_RAM: u32 = 0x0200_0000;
    const ARM7_RAM: u32 = 0x0380_0000;
    const BRANCH_TO_SELF: u32 = 0xEAFF_FFFE;
    const COPY_NICKNAME_FROM_RAM: [u32; 14] = [
        0xE59F_0018,
        0xE28F_1018,
        0xE3A0_2014,
        0xE4D0_3001,
        0xE4C1_3001,
        0xE252_2001,
        0x1AFF_FFFB,
        0xEAFF_FFFE,
        0x027F_FC86,
        0,
        0,
        0,
        0,
        0,
    ];
    const ARM9_NICKNAME: u32 = ARM9_RAM + 0x24;
    const READ_NICKNAME_OVER_SPI: [u32; 31] = [
        0xE59F_005C,
        0xE3A0_1C89,
        0xE1C0_10B0,
        0xE3A0_2003,
        0xEB00_000D,
        0xE3A0_2003,
        0xEB00_000B,
        0xE3A0_20FF,
        0xEB00_0009,
        0xE3A0_2006,
        0xEB00_0007,
        0xE28F_4034,
        0xE3A0_5014,
        0xE3A0_2000,
        0xEB00_0003,
        0xE4C4_3001,
        0xE255_5001,
        0x1AFF_FFFA,
        0xEAFF_FFFE,
        0xE1C0_20B2,
        0xE1D0_10B0,
        0xE311_0080,
        0x1AFF_FFFC,
        0xE5D0_3002,
        0xE12F_FF1E,
        0x0400_01C0,
        0,
        0,
        0,
        0,
        0,
    ];
    const ARM7_NICKNAME: u32 = ARM7_RAM + 0x68;

    pub(super) fn rom(arm9: &[u32], arm7: &[u32]) -> Arc<[u8]> {
        let mut rom = vec![0; 0x1000];
        rom[..4].copy_from_slice(b"TEST");
        let words =
            |code: &[u32]| -> Vec<u8> { code.iter().flat_map(|word| word.to_le_bytes()).collect() };
        let (arm9, arm7) = (words(arm9), words(arm7));
        for (at, offset, ram, code) in [
            (0x20, 0x200, ARM9_RAM, &arm9),
            (0x30, 0x800, ARM7_RAM, &arm7),
        ] {
            for (index, value) in [offset, ram, ram, code.len() as u32]
                .into_iter()
                .enumerate()
            {
                rom[at + 4 * index..at + 4 * index + 4].copy_from_slice(&value.to_le_bytes());
            }
            rom[offset as usize..offset as usize + code.len()].copy_from_slice(code);
        }
        rom.into()
    }

    pub(super) fn nds(arm9: &[u32], arm7: &[u32]) -> Nds {
        Nds::new(rom(arm9, arm7), NdsFirmware::default()).unwrap()
    }

    fn nicknames(firmware: NdsFirmware) -> [String; 2] {
        let mut nds = Nds::new(
            rom(&COPY_NICKNAME_FROM_RAM, &READ_NICKNAME_OVER_SPI),
            firmware,
        )
        .unwrap();
        nds.run_frame();
        assert_eq!(nds.faults(), [None, None]);
        let text = |units: Vec<u16>| {
            String::from_utf16(&units)
                .unwrap()
                .trim_end_matches('\0')
                .to_owned()
        };
        let arm9 = (0..10)
            .map(|index| {
                nds.buses
                    .arm9()
                    .read(ARM9_NICKNAME + 2 * index, Width::Half) as u16
            })
            .collect();
        let arm7 = (0..10)
            .map(|index| {
                nds.buses
                    .arm7()
                    .read(ARM7_NICKNAME + 2 * index, Width::Half) as u16
            })
            .collect();
        [text(arm9), text(arm7)]
    }

    fn zitrone() -> Option<UserSettings> {
        Some(UserSettings {
            nickname: "Zitrone".into(),
            ..UserSettings::default()
        })
    }

    #[cfg(feature = "persistence")]
    const CUBE: &[u8] =
        include_bytes!("../../../citrine-gb-app/fixtures/nds/blocksds-lighting_cube.nds");

    #[cfg(feature = "persistence")]
    fn cube() -> Nds {
        let mut nds = Nds::new(CUBE.into(), NdsFirmware::default()).unwrap();
        nds.record_composite(true);
        nds
    }

    #[cfg(feature = "persistence")]
    fn deferred_3d_finished_late_matches_3d_drawn_at_once(split: &mut Split<'_>) {
        let mut eager = cube();
        let mut deferred = cube();
        deferred.defer_3d(true);
        let (mut late, mut blank) = (0, 0);
        for frame in 0..120 {
            for nds in [&mut eager, &mut deferred] {
                nds.run_frame_on(&mut NoLink, &mut NoPeer, split);
                nds.take_audio(&mut Vec::new());
            }
            if frame % 3 != 0 {
                continue;
            }
            blank += usize::from(deferred.frame() != eager.frame());
            late += usize::from(deferred.finish_picture());
            assert!(deferred.frame() == eager.frame(), "frame {frame}");
            assert!(
                deferred.composite().map(Composite::pixels)
                    == eager.composite().map(Composite::pixels),
                "frame {frame}"
            );
            assert!(
                deferred.encode_state().unwrap() == eager.encode_state().unwrap(),
                "frame {frame}"
            );
        }
        assert!(
            late > 30 && blank > 30 && blank <= late,
            "{late} finished late, {blank} blank"
        );
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_deferred_3d_frame_finished_late_matches_one_drawn_at_once() {
        deferred_3d_finished_late_matches_3d_drawn_at_once(&mut Split::inline());
        citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads)
            .split(deferred_3d_finished_late_matches_3d_drawn_at_once);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_state_loaded_mid_frame_shows_its_last_whole_frame_until_the_next() {
        use crate::console::Rollback;
        let hash = |nds: &Nds| nds.state_hash(&mut nds.scratch());
        let mut nds = cube();
        for _ in 0..30 {
            nds.run_frame();
        }
        while nds.buses.lcd().line() != 100 {
            nds.step_on_air();
        }
        let shown = nds.frame().to_vec();
        let mut loaded = Nds::decode_state(&nds, &nds.encode_state().unwrap()).unwrap();
        assert_eq!(hash(&loaded), hash(&nds));
        nds.run_frame();
        loaded.run_frame();
        assert!(loaded.frame() == shown.as_slice());
        assert_eq!(hash(&loaded), hash(&nds));
        nds.run_frame();
        loaded.run_frame();
        assert!(loaded.frame() == nds.frame());
        assert_eq!(hash(&loaded), hash(&nds));
    }

    #[test]
    fn only_rows_of_one_colour_publish_as_a_fill() {
        let mut row = [0x1_2345; WIDTH];
        assert_eq!(uniform(&row), Some(0x1_2345));
        for at in [1, WIDTH / 2 + 1, WIDTH - 2, 0, WIDTH / 2, WIDTH - 1] {
            row[at] = 0;
            assert_eq!(uniform(&row), None, "{at}");
            row[at] = 0x1_2345;
        }
    }

    #[test]
    fn programs_read_the_nickname_from_the_settings() {
        for image in [None, Some(vec![0xFF; 0x4_0000])] {
            let firmware = NdsFirmware {
                firmware: image,
                user_settings: zitrone(),
                ..NdsFirmware::default()
            };
            assert_eq!(nicknames(firmware), ["Zitrone", "Zitrone"]);
        }
    }

    #[test]
    fn the_online_identity_reaches_the_firmware_and_the_wifi_chip() {
        let mac = [0x00, 0x09, 0xBF, 0xAB, 0xCD, 0xEF];
        let firmware = NdsFirmware {
            online: Online {
                mac: Some(mac),
                ..Online::default()
            },
            ..NdsFirmware::default()
        };
        let nds = Nds::new(
            rom(&COPY_NICKNAME_FROM_RAM, &READ_NICKNAME_OVER_SPI),
            firmware,
        )
        .unwrap();
        assert_eq!(nds.buses().spi().devices().firmware.mac(), mac);
        assert_eq!(
            nds.buses().wifi().channel(),
            Wifi::new(&nds.buses().spi().devices().firmware).channel()
        );
    }

    #[test]
    fn a_user_firmware_can_keep_its_own_nickname() {
        let own = UserSettings {
            nickname: "Dump".into(),
            ..UserSettings::default()
        };
        let image = Firmware::built_in(&own).bytes().to_vec();
        let firmware = NdsFirmware {
            firmware: Some(image.clone()),
            ..NdsFirmware::default()
        };
        assert_eq!(nicknames(firmware), ["Dump", "Dump"]);
        let firmware = NdsFirmware {
            firmware: Some(image),
            user_settings: zitrone(),
            ..NdsFirmware::default()
        };
        assert_eq!(nicknames(firmware), ["Zitrone", "Zitrone"]);
    }

    #[test]
    fn a_rom_without_a_ds_header_is_refused() {
        let result = Nds::new(vec![0; 0x100].into(), NdsFirmware::default());
        assert!(matches!(result, Err(GbError::NotNds)));
    }

    #[test]
    fn a_frame_is_560190_cycles_of_both_cpus() {
        let mut nds = nds(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]);
        nds.run_frame();
        let first = nds.cycles();
        nds.run_frame();
        assert_eq!(nds.cycles() - first, u64::from(FRAME_CYCLES));
        assert_eq!(nds.faults(), [None, None]);
        assert!(nds.arm7_cycles >= nds.now && nds.arm9_cycles >= 2 * nds.now);
    }

    #[test]
    fn both_cpus_run_their_own_code() {
        let mut nds = nds(
            &[0xE3A0_0011, 0xE58F_0000, BRANCH_TO_SELF, 0],
            &[0xE3A0_0077, 0xE58F_0000, BRANCH_TO_SELF, 0],
        );
        nds.run_frame();
        assert_eq!(nds.buses.arm9().read(ARM9_RAM + 12, Width::Word), 0x11);
        assert_eq!(nds.buses.arm7().read(ARM7_RAM + 12, Width::Word), 0x77);
    }

    #[cfg(feature = "debug")]
    #[test]
    fn a_skipped_branch_to_self_ends_on_the_cycle_it_would_have_run_to() {
        let program = [0xE59F_0000, 0xE58F_0000, BRANCH_TO_SELF, 0];
        let run = |analysis: bool| {
            let mut nds = nds(&program, &program);
            nds.debugger.static_analysis_enabled = analysis;
            for _ in 0..3 {
                nds.run_frame();
            }
            (
                nds.arm9_cycles,
                nds.arm7_cycles,
                nds.arm9.pc(),
                nds.arm7.pc(),
            )
        };
        assert_eq!(run(false), run(true));
    }

    fn burst_by_ends(nds: &mut Nds, arm9: bool, target: u64) -> u32 {
        loop {
            let (cycles, (attention, stall)) = if arm9 {
                let mut bus = nds.buses.arm9();
                let cycles = nds.arm9.step_inlined(&mut bus);
                (cycles, bus.take_events())
            } else {
                let mut bus = nds.buses.arm7();
                let cycles = nds.arm7.step_inlined(&mut bus);
                (cycles, bus.take_events())
            };
            let (clock, ends, pc) = if arm9 {
                (&mut nds.arm9_cycles, nds.arm9.ends(), nds.arm9.pc())
            } else {
                (&mut nds.arm7_cycles, nds.arm7.ends(), nds.arm7.pc())
            };
            *clock += u64::from(cycles + stall);
            if attention || *clock >= target && ends {
                return pc;
            }
        }
    }

    #[test]
    fn a_burst_past_its_target_stops_where_ends_holds() {
        let mut program = vec![0xE3A0_0000; 5];
        program.extend([0xE280_0001; 40]);
        program.push(0xEAFF_FFD6);
        let mut fold = nds(&program, &program);
        let mut each = nds(&program, &program);
        let mut landed = 0;
        for round in 0..600 {
            let target = fold.arm9_cycles + 1 + round % 97;
            fold.burst::<true>(target, false);
            let pc = burst_by_ends(&mut each, true, target);
            assert_eq!(
                (fold.arm9.pc(), fold.arm9_cycles),
                (pc, each.arm9_cycles),
                "arm9 round {round}"
            );
            landed += usize::from(pc & 0x7F == 0x14);
            let target = fold.arm7_cycles + 1 + round % 89;
            fold.burst::<false>(target, false);
            let pc = burst_by_ends(&mut each, false, target);
            assert_eq!(
                (fold.arm7.pc(), fold.arm7_cycles),
                (pc, each.arm7_cycles),
                "arm7 round {round}"
            );
            landed += usize::from(pc & 0x7F == 0x14);
        }
        assert!(landed > 0);
    }

    #[test]
    fn a_frame_shows_both_screens_top_then_bottom() {
        let mut nds = nds(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]);
        nds.run_frame();
        assert_eq!(nds.frame().len(), 256 * 384 * 4);
        assert!(nds.frame().chunks(4).all(|pixel| pixel[3] == 0xFF));
    }

    #[test]
    fn a_halted_arm7_wakes_on_its_vblank_irq() {
        let mut nds = nds(
            &[BRANCH_TO_SELF],
            &[
                0xE3A0_0301,
                0xE3A0_1008,
                0xE1C0_10B4,
                0xE3A0_1001,
                0xE580_1210,
                0xE3A0_1080,
                0xE5C0_1301,
                0xE3A0_2042,
                0xE58F_2000,
                BRANCH_TO_SELF,
                0,
            ],
        );
        nds.run_frame();
        assert!(nds.buses.halted(Cpu::Arm7));
        assert_eq!(nds.buses.arm7().read(ARM7_RAM + 40, Width::Word), 0);
        nds.run_frame();
        assert!(!nds.buses.halted(Cpu::Arm7));
        assert_eq!(nds.buses.arm7().read(ARM7_RAM + 40, Width::Word), 0x42);
    }

    #[test]
    fn sleep_mode_blanks_the_screens_until_the_lid_opens() {
        let mut nds = nds(&[BRANCH_TO_SELF], &SLEEP_ON_LID);
        nds.run_frame();
        assert!(nds.buses.sleeping());
        nds.run_frame();
        assert!(nds.frame().chunks(4).all(|pixel| pixel[..3] == [0, 0, 0]));
        nds.set_keys(Keys::LID);
        nds.run_frame();
        assert!(nds.buses.sleeping());
        assert_eq!(nds.buses.arm7().read(ARM7_RAM + 32, Width::Word), 0);
        nds.set_keys(Keys::empty());
        assert!(!nds.buses.sleeping());
        nds.run_frame();
        assert_eq!(nds.buses.arm7().read(ARM7_RAM + 32, Width::Word), 0x42);
    }

    #[test]
    fn a_console_rolled_back_to_another_picture_repaints_every_row_it_runs_into() {
        use crate::console::Rollback;
        let mut reference = nds(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]);
        reference.run_frame();
        reference.run_frame();
        let mut nds = nds(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]);
        nds.run_frame();
        let mut black = nds.clone_state();
        black.frame.fill(0);
        assert_ne!(black.frame(), reference.frame());
        nds.copy_state_from(&black);
        assert_eq!(nds.frame(), black.frame());
        nds.run_frame();
        assert_eq!(nds.frame(), reference.frame());
    }

    #[test]
    fn sleep_mode_stops_the_clocks_that_would_wake_a_halt() {
        let mut program = SLEEP_ON_LID;
        program[1] = 0xE3A0_1001;
        let mut nds = nds(&[BRANCH_TO_SELF], &program);
        nds.buses.arm7().write(0x0400_0004, Width::Half, 8);
        for _ in 0..3 {
            nds.run_frame();
        }
        assert!(nds.buses.sleeping());
        assert_eq!(nds.buses.arm7().read(ARM7_RAM + 32, Width::Word), 0);
    }

    #[test]
    fn a_silent_console_plays_silence_at_the_sample_rate() {
        let mut nds = nds(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]);
        nds.buses.arm7().write(0x0400_0504, Width::Half, 0x200);
        let mut out = Vec::new();
        nds.run_frame();
        nds.take_audio(&mut out);
        out.clear();
        for _ in 0..60 {
            nds.run_frame();
            nds.take_audio(&mut out);
        }
        let seconds = 60.0 * f64::from(FRAME_CYCLES) / f64::from(CYCLES_PER_SECOND);
        let expected = (seconds * f64::from(DEFAULT_SAMPLE_RATE)) as usize * 2;
        assert!(out.len().abs_diff(expected) <= 8);
        assert!(out.iter().all(|&sample| sample == 0.0));
    }

    const POLL_A_CARD_BLOCK: [u32; 23] = [
        0xE59F_0040,
        0xE59F_5040,
        0xE59F_1040,
        0xE3A0_7902,
        0xE140_70B4,
        0xE3A0_2000,
        0xE3A0_6000,
        0xE580_1000,
        0xE590_3000,
        0xE286_6001,
        0xE313_0880,
        0x1595_4000,
        0x1282_2001,
        0xE313_0102,
        0x1AFF_FFF8,
        0xE58F_2010,
        0xE58F_6010,
        0xEAFF_FFFE,
        0x0400_01A4,
        0x0410_0010,
        0x8100_0000,
        0,
        0,
    ];

    #[test]
    fn an_arm9_card_poll_sees_each_word_when_it_is_ready() {
        let mut nds = nds(&POLL_A_CARD_BLOCK, &[BRANCH_TO_SELF]);
        nds.run_frame();
        assert_eq!(nds.buses.arm9().read(ARM9_RAM + 84, Width::Word), 128);
        assert_eq!(nds.buses.arm9().read(ARM9_RAM + 88, Width::Word), 130);
    }

    const START_A_CARD_BLOCK: [u32; 8] = [
        0xE59F_0010,
        0xE59F_1010,
        0xE3A0_7902,
        0xE140_70B4,
        0xE580_1000,
        0xEAFF_FFFE,
        0x0400_01A4,
        0x8100_0000,
    ];

    #[test]
    fn a_card_transfer_counts_from_the_start_of_the_write() {
        let mut nds = nds(&START_A_CARD_BLOCK, &[BRANCH_TO_SELF]);
        while nds.arm9.pc() != ARM9_RAM + 16 {
            nds.execute(Cpu::Arm9);
        }
        let start = nds.arm9_cycles / 2;
        while !nds.buses.cart().dma_request() {
            nds.run_slice();
        }
        assert_eq!(nds.now, start + 12 * 5);
    }

    const POLL_AN_AUX_BYTE: [u32; 14] = [
        0xE59F_0024,
        0xE59F_1024,
        0xE1C0_10B0,
        0xE3A0_2000,
        0xE5C0_2002,
        0xE1D0_30B0,
        0xE282_2001,
        0xE313_0080,
        0x1AFF_FFFB,
        0xE58F_2008,
        0xEAFF_FFFE,
        0x0400_01A0,
        0x0000_A040,
        0,
    ];

    #[test]
    fn an_aux_byte_stays_busy_for_its_whole_time_after_the_write() {
        let mut nds = nds(&POLL_AN_AUX_BYTE, &[BRANCH_TO_SELF]);
        nds.run_frame();
        assert_eq!(nds.buses.arm9().read(ARM9_RAM + 52, Width::Word), 3);
    }

    const POLL_A_FIRMWARE_BYTE: [u32; 14] = [
        0xE59F_0024,
        0xE59F_1024,
        0xE1C0_10B0,
        0xE3A0_2000,
        0xE5C0_2002,
        0xE1D0_30B0,
        0xE282_2001,
        0xE313_0080,
        0x1AFF_FFFB,
        0xE58F_2008,
        0xEAFF_FFFE,
        0x0400_01C0,
        0x0000_8100,
        0,
    ];

    #[test]
    fn a_firmware_byte_stays_busy_for_its_whole_time_after_the_write() {
        let mut nds = nds(&[BRANCH_TO_SELF], &POLL_A_FIRMWARE_BYTE);
        nds.run_frame();
        assert_eq!(nds.buses.arm7().read(ARM7_RAM + 52, Width::Word), 10);
    }

    fn count_after_a_copy(words: u32) -> u32 {
        let program = [
            0xE59F_0020,
            0xE59F_1020,
            0xE580_1000,
            0xE59F_101C,
            0xE580_1004,
            0xE59F_1018,
            0xE580_1008,
            0xE282_2001,
            0xE58F_2010,
            0xEAFF_FFFC,
            0x0400_00D4,
            ARM9_RAM,
            ARM9_RAM + 0x10_0000,
            0x8400_0000 | words,
            0,
        ];
        let mut nds = nds(&program, &[BRANCH_TO_SELF]);
        nds.run_frame();
        let mut bus = nds.buses.arm9();
        assert_eq!(bus.read(ARM9_RAM + 0x10_0000, Width::Word), program[0]);
        bus.read(ARM9_RAM + 56, Width::Word)
    }

    #[test]
    fn a_dma_holds_the_cpu_that_started_it() {
        let short = count_after_a_copy(1);
        let long = count_after_a_copy(0x401);
        assert!(
            long < short,
            "{long} loops after the long copy, {short} after the short one"
        );
    }

    #[cfg(jit)]
    #[test]
    fn the_jit_compiles_and_runs_a_loop() {
        let mut nds = nds(&[0xE280_0001, 0xEAFF_FFFD], &[BRANCH_TO_SELF]);
        nds.set_jit(true);
        nds.run_frame();
        let stats = nds.jit_stats().expect("the JIT is on");
        assert!(
            stats.compiles > 0 && stats.refused == 0 && stats.runs > 0,
            "{stats:?}"
        );
    }

    #[cfg(jit)]
    #[test]
    fn a_reset_rebuilds_the_fast_table() {
        use crate::jit::FastBus;
        let mut nds = nds(&[0xE280_0001, 0xEAFF_FFFD], &[BRANCH_TO_SELF]);
        nds.set_jit(true);
        nds.run_frame();
        let kept = nds.buses.arm9().fast();
        unsafe { (*kept).windows[2].bytes = std::ptr::dangling_mut() };
        nds.soft_reset();
        let main = unsafe { (*nds.buses.arm9().fast()).windows[2].bytes };
        assert_eq!(main.cast_const(), nds.buses.main_ram().as_ptr());
        nds.run_frame();
        assert!(nds.jit_stats().expect("the JIT is on").runs > 0);
    }

    #[cfg(jit)]
    const COUNTER: u32 = ARM9_RAM + 0x1000;

    #[cfg(jit)]
    const COUNT_INTO_RAM: [u32; 7] = [
        0xE59F_1010,
        0xE1A0_0000,
        0xE1A0_0000,
        0xE280_0001,
        0xE581_0000,
        0xEAFF_FFFA,
        COUNTER,
    ];

    #[cfg(jit)]
    #[test]
    fn a_compiled_loop_runs_its_new_code_after_a_write_from_outside() {
        let mut nds = nds(&COUNT_INTO_RAM, &[BRANCH_TO_SELF]);
        nds.set_jit(true);
        nds.run_frame();
        let stats = nds.jit_stats().expect("the JIT is on");
        assert!(stats.runs > stats.lookups, "{stats:?}");
        nds.buses
            .arm9()
            .write(ARM9_RAM + 12, Width::Word, 0xE280_0000);
        nds.run_frame();
        let frozen = nds.buses.arm9().read(COUNTER, Width::Word);
        nds.run_frame();
        assert_eq!(nds.buses.arm9().read(COUNTER, Width::Word), frozen);
    }

    #[cfg(jit)]
    const STOP_COUNTING_AT_100: [u32; 12] = [
        0xE59F_1020,
        0xE59F_2020,
        0xE1A0_0000,
        0xE1A0_0000,
        0xE280_0001,
        0xE581_0000,
        0xE350_0064,
        0x050F_2014,
        0xEAFF_FFF8,
        0,
        COUNTER,
        0xE280_0000,
    ];

    #[cfg(jit)]
    #[test]
    fn a_compiled_loop_that_rewrites_itself_runs_the_new_code() {
        for jit in [false, true] {
            let mut nds = nds(&STOP_COUNTING_AT_100, &[BRANCH_TO_SELF]);
            nds.set_jit(jit);
            nds.run_frame();
            nds.run_frame();
            assert_eq!(nds.buses.arm9().read(COUNTER, Width::Word), 100);
        }
    }

    #[cfg(jit)]
    const STOP_COUNTING_AT_100_WITH_A_BLOCK_STORE: [u32; 14] = [
        0xE59F_1020,
        0xE59F_2020,
        0xE59F_3020,
        0xE59F_4020,
        0xE280_0001,
        0xE581_0000,
        0xE350_0064,
        0x0883_0014,
        0xEAFF_FFF8,
        0,
        COUNTER,
        0xE280_0000,
        ARM9_RAM + 16,
        0xE581_0000,
    ];

    #[cfg(jit)]
    #[test]
    fn a_compiled_loop_that_rewrites_itself_with_a_block_store_runs_the_new_code() {
        for jit in [false, true] {
            let mut nds = nds(&STOP_COUNTING_AT_100_WITH_A_BLOCK_STORE, &[BRANCH_TO_SELF]);
            nds.set_jit(jit);
            nds.run_frame();
            nds.run_frame();
            assert_eq!(nds.buses.arm9().read(COUNTER, Width::Word), 100);
        }
    }

    #[cfg(jit)]
    const WRITE_EVERY_PINNED_REGISTER_BEFORE_A_HOP: [u32; 27] = [
        0xE59F_605C,
        0xE59F_705C,
        0xE3A0_0001,
        0xE3A0_1002,
        0xE3A0_2003,
        0xE3A0_3004,
        0xE3A0_4005,
        0xE3A0_5006,
        0xE3A0_D007,
        0xE3A0_E008,
        0xE080_0001,
        0xE021_11E2,
        0xE082_2003,
        0xE084_4005,
        0xE085_5000,
        0xE08D_D000,
        0xE02E_E001,
        0xE053_3004,
        0x6AFF_FFFF,
        0xE0A2_2003,
        0x4280_0001,
        0xE257_7001,
        0x1AFF_FFF2,
        0xE886_603F,
        0xEAFF_FFFE,
        COUNTER,
        10_000,
    ];

    #[cfg(jit)]
    #[test]
    fn registers_written_before_a_refused_hop_reach_the_core() {
        let results = [false, true].map(|jit| {
            let mut nds = nds(&WRITE_EVERY_PINNED_REGISTER_BEFORE_A_HOP, &[BRANCH_TO_SELF]);
            nds.set_jit(jit);
            for _ in 0..10 {
                nds.run_frame();
            }
            if jit {
                let stats = nds.jit_stats().expect("the JIT is on");
                assert!(stats.runs > stats.lookups, "{stats:?}");
            }
            (0..8)
                .map(|word| nds.buses.arm9().read(COUNTER + 4 * word, Width::Word))
                .collect::<Vec<_>>()
        });
        assert_ne!(results[0], vec![0; 8]);
        assert_eq!(results[0], results[1]);
    }

    #[cfg(jit)]
    const READ_THE_CPSR_AFTER_EACH_FLAG_WRITE: [u32; 18] = [
        0xE59F_6038,
        0xE59F_7038,
        0xE3A0_5000,
        0xE257_0032,
        0xE10F_1000,
        0xE021_51E5,
        0xE095_2005,
        0xE10F_3000,
        0xE023_52E5,
        0xE128_F000,
        0xE10F_4000,
        0xE024_53E5,
        0xE257_7001,
        0x1AFF_FFF4,
        0xE586_5000,
        0xEAFF_FFFE,
        COUNTER,
        10_000,
    ];

    #[cfg(jit)]
    #[test]
    fn cpsr_reads_after_flag_writes_match_the_interpreter() {
        let results = [false, true].map(|jit| {
            let mut nds = nds(&READ_THE_CPSR_AFTER_EACH_FLAG_WRITE, &[BRANCH_TO_SELF]);
            nds.set_jit(jit);
            for _ in 0..10 {
                nds.run_frame();
            }
            if jit {
                let stats = nds.jit_stats().expect("the JIT is on");
                assert!(stats.retired > 10_000, "{stats:?}");
            }
            nds.buses.arm9().read(COUNTER, Width::Word)
        });
        assert_ne!(results[0], 0);
        assert_eq!(results[0], results[1]);
    }

    #[cfg(jit)]
    const SWITCH_MODES_AND_WRITE_EACH_BANK: [u32; 28] = [
        0xE59F_6060,
        0xE59F_7060,
        0xE10F_0000,
        0xE3C0_101F,
        0xE381_2011,
        0xE381_1012,
        0xE121_F001,
        0xE28D_D001,
        0xE28E_E003,
        0xE121_F002,
        0xE288_8007,
        0xE28D_D009,
        0xE121_F000,
        0xE288_8002,
        0xE28D_D005,
        0xE257_7001,
        0x1AFF_FFF4,
        0xE886_2100,
        0xE121_F001,
        0xE586_D008,
        0xE586_E00C,
        0xE121_F002,
        0xE586_8010,
        0xE586_D014,
        0xE121_F000,
        0xEAFF_FFFE,
        COUNTER,
        10_000,
    ];

    #[cfg(jit)]
    #[test]
    fn mode_switches_through_msr_bank_registers_like_the_interpreter() {
        let results = [false, true].map(|jit| {
            let mut nds = nds(&SWITCH_MODES_AND_WRITE_EACH_BANK, &[BRANCH_TO_SELF]);
            nds.set_jit(jit);
            for _ in 0..10 {
                nds.run_frame();
            }
            if jit {
                let stats = nds.jit_stats().expect("the JIT is on");
                assert!(stats.retired > 10_000, "{stats:?}");
            }
            (0..6)
                .map(|word| nds.buses.arm9().read(COUNTER + 4 * word, Width::Word))
                .collect::<Vec<_>>()
        });
        assert_ne!(results[0][0], results[0][4]);
        assert_ne!(results[0][1], results[0][2]);
        assert_eq!(results[0], results[1]);
    }

    const POLL_THE_GEOMETRY_FIFO_LEVEL: [u32; 18] = [
        0xE59F_0030,
        0xE59F_5030,
        0xE3A0_100A,
        0xE580_5000,
        0xE251_1001,
        0x1AFF_FFFC,
        0xE59F_3020,
        0xE3A0_2000,
        0xE593_4000,
        0xE282_2001,
        0xE314_061F,
        0x1AFF_FFFB,
        0xE58F_200C,
        0xEAFF_FFFE,
        0x0400_0400,
        0x1515_1515,
        0x0400_0600,
        0,
    ];

    #[test]
    fn gxstat_reads_see_the_fifo_level_drop_by_the_next_end_point() {
        let mut nds = nds(&POLL_THE_GEOMETRY_FIFO_LEVEL, &[BRANCH_TO_SELF]);
        nds.run_frame();
        let polls = nds.buses.arm9().read(ARM9_RAM + 68, Width::Word);
        assert_eq!(polls, 2);
    }

    #[cfg(jit)]
    const CACHE_ON: [u32; 4] = [0xEE11_0F10, 0xE380_0A01, 0xE380_0001, 0xEE01_0F10];

    #[cfg(jit)]
    const HOP_ACROSS_LINES_MODES_AND_ITCM: [(usize, u32); 20] = [
        (0x10, 0xE28F_7F5A),
        (0x14, 0xE28F_BF69),
        (0x18, 0xE3A0_3C01),
        (0x20, 0xE281_1001),
        (0x24, 0xEA00_000C),
        (0x5C, 0xE282_2001),
        (0x60, 0xEA00_0007),
        (0x84, 0xE284_4001),
        (0x88, 0xE28F_5071),
        (0x8C, 0xE12F_FF15),
        (0x100, 0xE00D_3601),
        (0x120, 0xE00C_3601),
        (0x13C, 0x3601_0000),
        (0x140, 0x0000_E007),
        (0x150, 0x3601_0000),
        (0x154, 0x0000_4738),
        (0x180, 0xE288_8001),
        (0x184, 0xE1A0_F003),
        (0x1C0, 0xE289_9001),
        (0x1C4, 0xEA00_038C),
    ];

    #[cfg(jit)]
    #[test]
    fn compiled_branches_refetch_with_the_cycles_and_state_of_the_interpreter() {
        use crate::nds::cp15::Fetch;
        for cached in [false, true] {
            let run = |jit| {
                let mut code = vec![0xE1A0_0000; 0x1C8 / 4];
                if cached {
                    code[..4].copy_from_slice(&CACHE_ON);
                }
                for (at, word) in HOP_ACROSS_LINES_MODES_AND_ITCM {
                    code[at / 4] = word;
                }
                let mut nds = nds(&code, &[BRANCH_TO_SELF]);
                {
                    let mut bus = nds.buses.arm9();
                    bus.cpu_write(0x100, Width::Word, 0xE28A_A001);
                    bus.cpu_write(0x104, Width::Word, 0xE1A0_F00B);
                    bus.write(ARM9_RAM + 0xFFC, Width::Word, 0xEAFF_FC07);
                }
                nds.set_jit(jit);
                for _ in 0..2 {
                    nds.run_frame();
                }
                nds
            };
            let (mut jitted, interpreted) = (run(true), run(false));
            let bus = jitted.buses.arm9();
            assert_eq!(bus.cp15().cached(ARM9_RAM, Fetch::Code), cached);
            let laps = [1, 2, 4, 6, 8, 9, 10].map(|r| jitted.arm9.reg(r));
            assert!(laps.iter().all(|&lap| lap > 100), "{laps:?}");
            let stats = jitted.jit_stats();
            assert!(stats.is_some_and(|jit| jit.runs > 0), "{stats:?}");
            assert_eq!(
                (jitted.arm9_cycles, jitted.arm9.clone()),
                (interpreted.arm9_cycles, interpreted.arm9.clone()),
                "cached {cached}"
            );
        }
    }

    #[cfg(jit)]
    const RETURN_THROUGH_LOADS_INTO_PC: [(usize, u32); 9] = [
        (0x10, 0xE289_9001),
        (0x14, 0xE3A0_3C02),
        (0x18, 0xE3A0_2004),
        (0x1C, 0xE28F_4080),
        (0x20, 0xE493_F004),
        (0x5C, 0xE493_F004),
        (0x80, 0xE593_F000),
        (0xA4, 0xE288_8001),
        (0xA8, 0xEAFF_FFD8),
    ];

    #[cfg(jit)]
    #[test]
    fn compiled_word_loads_into_pc_return_with_the_cycles_of_the_interpreter() {
        use crate::nds::cp15::Fetch;
        for cached in [false, true] {
            let run = |jit| {
                let mut code = vec![0xE1A0_0000; 0xAC / 4];
                if cached {
                    code[..4].copy_from_slice(&CACHE_ON);
                }
                for (at, word) in RETURN_THROUGH_LOADS_INTO_PC {
                    code[at / 4] = word;
                }
                let mut nds = nds(&code, &[BRANCH_TO_SELF]);
                {
                    let mut bus = nds.buses.arm9();
                    for (at, word) in [
                        (0x100, 0xE28A_A001),
                        (0x104, 0xE693_F002),
                        (0x140, 0x4720_3101),
                        (0x200, 0x100),
                        (0x204, ARM9_RAM + 0x5C),
                        (0x208, ARM9_RAM + 0x80),
                        (0x20C, 0x141),
                    ] {
                        bus.cpu_write(at, Width::Word, word);
                    }
                }
                nds.set_jit(jit);
                for _ in 0..2 {
                    nds.run_frame();
                }
                nds
            };
            let (mut jitted, interpreted) = (run(true), run(false));
            let bus = jitted.buses.arm9();
            assert_eq!(bus.cp15().cached(ARM9_RAM, Fetch::Code), cached);
            let laps = [1, 8, 9, 10].map(|r| jitted.arm9.reg(r));
            assert!(laps.iter().all(|&lap| lap > 100), "{laps:?}");
            let stats = jitted.jit_stats();
            assert!(stats.is_some_and(|jit| jit.runs > 0), "{stats:?}");
            assert_eq!(
                (jitted.arm9_cycles, jitted.arm9.clone()),
                (interpreted.arm9_cycles, interpreted.arm9.clone()),
                "cached {cached}"
            );
        }
    }

    #[cfg(jit)]
    const RETURN_FROM_EXCEPTION_MODES: [(usize, u32); 13] = [
        (0x10, 0xE321_F0D2),
        (0x14, 0xE361_F0DF),
        (0x18, 0xE28F_E040),
        (0x1C, 0xE289_9001),
        (0x20, 0xE1B0_F00E),
        (0x60, 0xE288_8001),
        (0x64, 0xE321_F0D2),
        (0x68, 0xE28F_E058),
        (0x6C, 0xE25E_F004),
        (0xC4, 0xE28A_A001),
        (0xC8, 0xEAFF_FFD0),
        (0x70, 0xEAFF_FFFE),
        (0x24, 0xEAFF_FFFE),
    ];

    #[cfg(jit)]
    #[test]
    fn compiled_exception_returns_restore_the_cpsr_with_the_cycles_of_the_interpreter() {
        use crate::nds::cp15::Fetch;
        for cached in [false, true] {
            let run = |jit| {
                let mut code = vec![0xE1A0_0000; 0xCC / 4];
                if cached {
                    code[..4].copy_from_slice(&CACHE_ON);
                }
                for (at, word) in RETURN_FROM_EXCEPTION_MODES {
                    code[at / 4] = word;
                }
                let mut nds = nds(&code, &[BRANCH_TO_SELF]);
                nds.set_jit(jit);
                for _ in 0..2 {
                    nds.run_frame();
                }
                nds
            };
            let (mut jitted, interpreted) = (run(true), run(false));
            let bus = jitted.buses.arm9();
            assert_eq!(bus.cp15().cached(ARM9_RAM, Fetch::Code), cached);
            let laps = [8, 9, 10].map(|r| jitted.arm9.reg(r));
            assert!(laps.iter().all(|&lap| lap > 100), "{laps:?}");
            let stats = jitted.jit_stats();
            assert!(stats.is_some_and(|jit| jit.runs > 0), "{stats:?}");
            assert_eq!(
                (jitted.arm9_cycles, jitted.arm9.clone()),
                (interpreted.arm9_cycles, interpreted.arm9.clone()),
                "cached {cached}"
            );
        }
    }

    #[cfg(jit)]
    const SHIFT_BY_REGISTER: [u32; 23] = [
        0xE3A0_0000,
        0xE3A0_110F,
        0xE1B0_2011,
        0xE0A4_4002,
        0xE1B0_3031,
        0xE0A5_5003,
        0xE1B0_2051,
        0xE0A6_6002,
        0xE1B0_3071,
        0xE0A7_7003,
        0xE088_8031,
        0xE111_0011,
        0xE2A9_9000,
        0x428A_A001,
        0x028B_B001,
        0xE051_C051,
        0xE0AA_A00C,
        0xE1F0_E071,
        0xE0A9_900E,
        0xE280_0001,
        0xE021_1800,
        0xEAFF_FFEB,
        BRANCH_TO_SELF,
    ];

    #[cfg(jit)]
    #[test]
    fn compiled_shifts_by_register_match_the_interpreter() {
        let run = |jit| {
            let mut nds = nds(&SHIFT_BY_REGISTER, &[BRANCH_TO_SELF]);
            nds.set_jit(jit);
            for _ in 0..2 {
                nds.run_frame();
            }
            nds
        };
        let (jitted, interpreted) = (run(true), run(false));
        assert!(jitted.arm9.reg(0) > 300, "{}", jitted.arm9.reg(0));
        let stats = jitted.jit_stats();
        assert!(stats.is_some_and(|jit| jit.runs > 0), "{stats:?}");
        assert_eq!(
            (jitted.arm9_cycles, jitted.arm9.clone()),
            (interpreted.arm9_cycles, interpreted.arm9.clone())
        );
    }

    #[cfg(jit)]
    const COUNT_LEADING_ZEROS: [u32; 14] = [
        0xE3A0_0000,
        0xE3A0_1001,
        0xE16F_2F11,
        0xE083_3002,
        0xE16F_4F10,
        0xE085_5004,
        0xE1A0_1FE1,
        0xE280_0001,
        0xE021_1800,
        0xE310_0001,
        0x116F_6F11,
        0xE087_7006,
        0xEAFF_FFF4,
        BRANCH_TO_SELF,
    ];

    #[cfg(jit)]
    #[test]
    fn compiled_count_leading_zeros_match_the_interpreter() {
        let run = |jit| {
            let mut nds = nds(&COUNT_LEADING_ZEROS, &[BRANCH_TO_SELF]);
            nds.set_jit(jit);
            for _ in 0..2 {
                nds.run_frame();
            }
            nds
        };
        let (jitted, interpreted) = (run(true), run(false));
        assert!(jitted.arm9.reg(0) > 300, "{}", jitted.arm9.reg(0));
        let stats = jitted.jit_stats();
        assert!(stats.is_some_and(|jit| jit.runs > 0), "{stats:?}");
        assert_eq!(
            (jitted.arm9_cycles, jitted.arm9.clone()),
            (interpreted.arm9_cycles, interpreted.arm9.clone())
        );
    }

    #[cfg(jit)]
    const SWAPS: [u32; 9] = [
        0xE3A0_0623,
        0xE281_1003,
        0xE100_2091,
        0xE140_3091,
        0xE280_0001,
        0xE100_4094,
        0xE287_7001,
        0xEAFF_FFF8,
        BRANCH_TO_SELF,
    ];

    #[cfg(jit)]
    #[test]
    fn compiled_swaps_match_the_interpreter() {
        let run = |jit| {
            let mut nds = nds(&SWAPS, &[BRANCH_TO_SELF]);
            nds.set_jit(jit);
            for _ in 0..2 {
                nds.run_frame();
            }
            nds
        };
        let (mut jitted, mut interpreted) = (run(true), run(false));
        assert!(jitted.arm9.reg(7) > 300, "{}", jitted.arm9.reg(7));
        let stats = jitted.jit_stats();
        assert!(stats.is_some_and(|jit| jit.runs > 0), "{stats:?}");
        assert_eq!(
            (jitted.arm9_cycles, jitted.arm9.clone()),
            (interpreted.arm9_cycles, interpreted.arm9.clone())
        );
        for addr in (0x0230_0000..0x0230_0100).step_by(4) {
            assert_eq!(
                jitted.buses.arm9().read(addr, Width::Word),
                interpreted.buses.arm9().read(addr, Width::Word),
                "{addr:08X}"
            );
        }
    }

    #[cfg(jit)]
    const USER_BANK_TRANSFERS: [u32; 19] = [
        0xE3A0_0623,
        0xE321_F0D2,
        0xE28D_D003,
        0xE28E_E005,
        0xE8C0_7F7E,
        0xE9D0_7F7E,
        0xE321_F0DF,
        0xE28D_D007,
        0xE081_100D,
        0xE9E0_7F7C,
        0xE321_F0D1,
        0xE288_8001,
        0xE28D_D009,
        0xE590_3000,
        0xE8C0_7F00,
        0xE9D0_7F00,
        0xE287_7001,
        0xEAFF_FFED,
        BRANCH_TO_SELF,
    ];

    #[cfg(jit)]
    const IRQ_USER_BANK_TRANSFERS: [u32; 10] = [
        0xE3A0_0623,
        0xE321_F0D2,
        0xE28D_D003,
        0xE28E_E005,
        0xE288_8001,
        0xE8C0_7F7E,
        0xE9D0_7F7E,
        0xE287_7001,
        0xEAFF_FFF8,
        BRANCH_TO_SELF,
    ];

    #[cfg(jit)]
    #[test]
    fn compiled_user_bank_transfers_match_the_interpreter() {
        for program in [&USER_BANK_TRANSFERS[..], &IRQ_USER_BANK_TRANSFERS[..]] {
            let run = |jit| {
                let mut nds = nds(program, &[BRANCH_TO_SELF]);
                nds.set_jit(jit);
                for _ in 0..2 {
                    nds.run_frame();
                }
                nds
            };
            let (jitted, interpreted) = (run(true), run(false));
            assert!(jitted.arm9.reg(7) > 300, "{}", jitted.arm9.reg(7));
            let stats = jitted.jit_stats();
            assert!(stats.is_some_and(|jit| jit.runs > 0), "{stats:?}");
            assert_eq!(
                (jitted.arm9_cycles, jitted.arm9.clone()),
                (interpreted.arm9_cycles, interpreted.arm9.clone())
            );
        }
    }
}
