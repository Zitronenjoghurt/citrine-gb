mod arm7;
mod arm9;
mod lines;

pub use arm7::Arm7Bus;
pub use arm9::Arm9Bus;

use super::Cpu;
use super::card::Cartridge;
use super::cp15::{CacheMap, Cp15};
use super::dma::{Area, Dma, Start, Timing};
use super::input::Input;
use super::interrupt::{Interrupts, Irq};
use super::ipc::{self, Ipc};
use super::lcd::{FRAME_CYCLES, Lcd, LcdEvent, VISIBLE_LINES};
use super::machine::SideWork;
use super::math::Math;
use super::ram::Ram;
use super::rtc::Rtc;
use super::sound::mixer::Mixer;
use super::spi::Spi;
use super::spi::firmware::Firmware;
use super::tcm::Tcm;
use super::three_d::{Scene, ThreeD, raster};
use super::timer::Timers;
use super::video::{
    BLANK_3D, Composite, Engine, EngineKind, Fallback, HEIGHT, Memory, WIDTH, color,
};
use super::vram::{EngineView, Vram};
use super::wifi::Wifi;
use super::wram::SharedWram;
use crate::console::Keys;
use crate::gba::open_bus::lane;
use crate::gba::wait::{Access, Width};
#[cfg(jit)]
use crate::jit::Kept;
use crate::traffic::{Entry, Kind, Ring, Traffic};

pub const MAIN_RAM_SIZE: usize = 0x40_0000;
const IPC_TRAFFIC_CAPACITY: usize = 4096;
pub const ARM7_WRAM_SIZE: usize = 0x1_0000;
pub const ARM9_BIOS_WINDOW: usize = 0x8000;
pub const ARM7_BIOS_WINDOW: usize = 0x4000;
const PALETTE_OAM_SIZE: usize = 0x800;
const ENGINE_BYTES: usize = 0x400;
const EXMEM_SHARED: u16 = 0xFF80;
const EXMEM_OWN: u16 = 0x007F;
const EXMEM_WRITABLE: u16 = 0xE8FF;
const GBA_SLOT_ARM7: u16 = 1 << 7;
const CART_ARM7: u16 = 1 << 11;
const GBA_ROM_END: u32 = 0x0A00_0000;
const GBA_SLOT_FIRST_WAITS: [u32; 4] = [10, 8, 6, 18];
const DMA_SLOT_ROM: u8 = 0x10;
const DMA_SLOT_RAM: u8 = 0x11;
const NO_CART_RAM: u32 = 0xFF;
const ENGINE_B_ON: u16 = 1 << 9;
const SWAP_SCREENS: u16 = 1 << 15;
const RENDERING_ON: u16 = 1 << 2;
const GEOMETRY_ON: u16 = 1 << 3;
const RENDER_LINE: u16 = 215;
const RCNT_RTC_MASK: u16 = 0xC100;
const RCNT_RTC_IRQ: u16 = 0x8100;

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Shared {
    main_ram: Ram,
    wram: SharedWram,
    vram: Vram,
    exmem: u16,
    arm7_exmem: u16,
    irq: [Interrupts; 2],
    ipc: Ipc,
    cart: Cartridge,
    lcd: Lcd,
    input: Input,
    #[cfg_attr(feature = "serde", serde(skip, default = "ipc_traffic"))]
    ipc_traffic: Traffic,
}

fn ipc_traffic() -> Traffic {
    Traffic::with_capacity(IPC_TRAFFIC_CAPACITY)
}

#[cfg(feature = "serde")]
fn pending_after_load() -> bool {
    true
}

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Arm9Memory {
    #[cfg_attr(feature = "serde", serde(skip))]
    bios: Box<[u8]>,
    tcm: Tcm,
    palette: Ram,
    oam: Ram,
    cp15: Cp15,
    #[cfg_attr(feature = "serde", serde(skip))]
    caches: CacheMap,
    timers: Timers,
    dma: Dma,
    math: Math,
    engines: [Engine; 2],
    three_d: ThreeD,
    stall: u32,
    power: u16,
    postflg: u8,
    halted: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    attention: bool,
    #[cfg_attr(feature = "serde", serde(skip, default = "pending_after_load"))]
    pending: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    ahead: u32,
    #[cfg(jit)]
    #[cfg_attr(feature = "serde", serde(skip))]
    fast: Kept,
}

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Arm7Memory {
    #[cfg_attr(feature = "serde", serde(skip))]
    bios: Box<[u8]>,
    wram: Ram,
    #[cfg_attr(feature = "serde", serde(skip))]
    code: arm7::CodePages,
    timers: Timers,
    dma: Dma,
    spi: Spi,
    rtc: Rtc,
    sound: Mixer,
    rtc_interrupt: bool,
    power: u16,
    postflg: u8,
    halted: bool,
    biosprot: u16,
    rcnt: u16,
    wifi_waitcnt: u16,
    wifi: Wifi,
    #[cfg_attr(feature = "serde", serde(default))]
    sleeping: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    stall: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    attention: bool,
    #[cfg_attr(feature = "serde", serde(skip, default = "pending_after_load"))]
    pending: bool,
    #[cfg(jit)]
    #[cfg_attr(feature = "serde", serde(skip))]
    fast: Kept,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Waits {
    half: [u32; 2],
    word: [u32; 2],
}

impl Waits {
    const fn new(n16: u32, s16: u32, n32: u32, s32: u32) -> Self {
        Self {
            half: [n16, s16],
            word: [n32, s32],
        }
    }

    fn cycles(self, width: Width, access: Access) -> u32 {
        let index = match access {
            Access::NonSequential => 0,
            Access::Sequential => 1,
        };
        match width {
            Width::Word => self.word[index],
            Width::Byte | Width::Half => self.half[index],
        }
    }
}

#[derive(Clone)]
pub struct Screens {
    top: Box<[u32]>,
    bottom: Box<[u32]>,
    composite: Option<Box<Composite>>,
    missing_3d: Vec<(usize, bool)>,
}

impl Default for Screens {
    fn default() -> Self {
        Self {
            top: vec![color::WHITE; WIDTH * HEIGHT].into_boxed_slice(),
            bottom: vec![color::WHITE; WIDTH * HEIGHT].into_boxed_slice(),
            composite: None,
            missing_3d: Vec::new(),
        }
    }
}

impl Screens {
    pub fn top(&self) -> &[u32] {
        &self.top
    }

    pub fn bottom(&self) -> &[u32] {
        &self.bottom
    }

    pub fn composite(&self) -> Option<&Composite> {
        self.composite.as_deref()
    }

    fn put_row(&mut self, y: usize, top: bool, row: &[u32; WIDTH]) {
        let screen = if top { &mut self.top } else { &mut self.bottom };
        screen[y * WIDTH..(y + 1) * WIDTH].copy_from_slice(row);
    }
}

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Buses {
    shared: Shared,
    arm9: Arm9Memory,
    arm7: Arm7Memory,
    #[cfg_attr(feature = "serde", serde(skip))]
    screens: Screens,
    #[cfg_attr(feature = "serde", serde(skip))]
    sleep_cycles: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    lines: lines::Lines,
    #[cfg_attr(feature = "serde", serde(skip))]
    sound_muted: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    defer_3d: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    skip_draw: bool,
}

impl Default for Buses {
    fn default() -> Self {
        Self {
            shared: Shared {
                main_ram: Ram::new(MAIN_RAM_SIZE),
                wram: SharedWram::default(),
                vram: Vram::default(),
                exmem: 0,
                arm7_exmem: 0,
                irq: [Interrupts::new(Cpu::Arm9), Interrupts::new(Cpu::Arm7)],
                ipc: Ipc::default(),
                cart: Cartridge::default(),
                lcd: Lcd::default(),
                input: Input::default(),
                ipc_traffic: ipc_traffic(),
            },
            arm9: Arm9Memory {
                bios: Box::default(),
                tcm: Tcm::default(),
                palette: Ram::new(PALETTE_OAM_SIZE),
                oam: Ram::new(PALETTE_OAM_SIZE),
                cp15: Cp15::default(),
                caches: CacheMap::default(),
                timers: Timers::new(Cpu::Arm9),
                dma: Dma::new(Cpu::Arm9),
                math: Math::default(),
                engines: [Engine::new(EngineKind::A), Engine::new(EngineKind::B)],
                three_d: ThreeD::default(),
                stall: 0,
                power: 0,
                postflg: 0,
                halted: false,
                attention: false,
                pending: false,
                ahead: 0,
                #[cfg(jit)]
                fast: Kept::default(),
            },
            arm7: Arm7Memory {
                bios: Box::default(),
                wram: Ram::new(ARM7_WRAM_SIZE),
                code: arm7::CodePages::default(),
                timers: Timers::new(Cpu::Arm7),
                dma: Dma::new(Cpu::Arm7),
                spi: Spi::default(),
                rtc: Rtc::default(),
                sound: Mixer::new(),
                rtc_interrupt: false,
                power: 0,
                postflg: 0,
                halted: false,
                biosprot: 0,
                rcnt: 0,
                wifi_waitcnt: 0,
                wifi: Wifi::new(&Firmware::default()),
                sleeping: false,
                stall: 0,
                attention: false,
                pending: false,
                #[cfg(jit)]
                fast: Kept::default(),
            },
            screens: Screens::default(),
            sleep_cycles: 0,
            lines: lines::Lines::default(),
            sound_muted: false,
            defer_3d: false,
            skip_draw: false,
        }
    }
}

impl Buses {
    pub fn copy_state_from(&mut self, source: &Buses) {
        let Self {
            shared,
            arm9,
            arm7,
            screens,
            sleep_cycles,
            lines,
            sound_muted: _,
            defer_3d: _,
            skip_draw: _,
        } = self;
        shared.copy_state_from(&source.shared);
        arm9.copy_state_from(&source.arm9);
        arm7.copy_state_from(&source.arm7);
        screens.clone_from(&source.screens);
        *sleep_cycles = source.sleep_cycles;
        lines.clone_from(&source.lines);
    }

    pub fn share_pages_into(&mut self, slot: &mut Buses) {
        for (slot, live) in slot.rams_mut().zip(self.rams_mut()) {
            slot.snapshot_from(live);
        }
    }

    pub fn empty_rams(&mut self) {
        for ram in self.rams_mut() {
            *ram = Ram::empty();
        }
    }

    fn rams_mut(&mut self) -> impl Iterator<Item = &mut Ram> {
        let Self {
            shared, arm9, arm7, ..
        } = self;
        [
            &mut shared.main_ram,
            shared.wram.ram_mut(),
            &mut arm9.palette,
            &mut arm9.oam,
            &mut arm7.wram,
        ]
        .into_iter()
        .chain(arm9.tcm.rams_mut())
        .chain(shared.vram.rams_mut())
    }

    pub fn arm9(&mut self) -> Arm9Bus<'_> {
        Arm9Bus::new(&mut self.shared, &mut self.arm9)
    }

    pub fn arm7(&mut self) -> Arm7Bus<'_> {
        Arm7Bus::new(&mut self.shared, &mut self.arm7)
    }

    pub fn adopt_fixed(&mut self, from: &Buses) {
        self.arm9.bios.clone_from(&from.arm9.bios);
        self.arm9.refresh_caches();
        self.arm7.bios.clone_from(&from.arm7.bios);
        self.shared.cart.adopt_rom(&from.shared.cart);
    }

    #[cfg(feature = "persistence")]
    pub(crate) fn copy_saved_from(&mut self, source: &Buses) {
        let Self {
            shared,
            arm9,
            arm7,
            screens,
            sleep_cycles: _,
            lines: _,
            sound_muted: _,
            defer_3d: _,
            skip_draw: _,
        } = self;
        screens.missing_3d.clear();
        shared.copy_saved_from(&source.shared);
        arm9.copy_saved_from(&source.arm9);
        arm7.copy_saved_from(&source.arm7);
    }

    pub fn next_traffic_frame(&mut self) {
        self.shared.cart.next_traffic_frame();
        self.shared.ipc_traffic.next_frame();
        self.arm7.spi.traffic_mut().next_frame();
        self.arm7.wifi.traffic_mut().next_frame();
    }

    pub(crate) fn adopt_beam(&mut self, old: &Buses) {
        self.shared.lcd.adopt_beam(&old.shared.lcd);
    }

    pub fn adopt_traffic(&mut self, old: &mut Buses) {
        let at = self.shared.cart.clock();
        self.shared.ipc_traffic.adopt(&mut old.shared.ipc_traffic);
        self.shared
            .ipc_traffic
            .push(Entry::new(Kind::SoftReset).at(at));
        self.arm7
            .spi
            .traffic_mut()
            .adopt(old.arm7.spi.traffic_mut());
        self.arm7
            .spi
            .traffic_mut()
            .push(Entry::new(Kind::SoftReset).at(at));
        self.arm7
            .wifi
            .traffic_mut()
            .adopt(old.arm7.wifi.traffic_mut());
        self.arm7
            .wifi
            .traffic_mut()
            .push(Entry::new(Kind::SoftReset).at(at));
    }

    pub fn traffic(&self) -> [Ring<'_>; 5] {
        [
            Ring {
                name: "auxspi",
                traffic: self.shared.cart.aux_traffic(),
            },
            Ring {
                name: "card",
                traffic: self.shared.cart.card_traffic(),
            },
            Ring {
                name: "ipc",
                traffic: &self.shared.ipc_traffic,
            },
            Ring {
                name: "spi",
                traffic: self.arm7.spi.traffic(),
            },
            Ring {
                name: "wifi",
                traffic: self.arm7.wifi.traffic(),
            },
        ]
    }

    pub fn traffic_clock(&self) -> u64 {
        self.shared.cart.clock()
    }

    pub fn load_arm9_bios(&mut self, image: &[u8]) {
        self.arm9.bios = bios_image(image, ARM9_BIOS_WINDOW);
    }

    pub fn load_arm7_bios(&mut self, image: &[u8]) {
        self.arm7.bios = bios_image(image, ARM7_BIOS_WINDOW);
    }

    pub fn patch_arm9_bios(&mut self, offset: usize, bytes: &[u8]) {
        if let Some(target) = self.arm9.bios.get_mut(offset..offset + bytes.len()) {
            target.copy_from_slice(bytes);
        }
    }

    pub fn peek_io(&self, cpu: Cpu, addr: u32) -> u32 {
        match cpu {
            Cpu::Arm9 => arm9::peek_io(&self.shared, &self.arm9, addr),
            Cpu::Arm7 => arm7::peek_io(&self.shared, &self.arm7, addr),
        }
    }

    pub fn main_ram(&self) -> &[u8] {
        self.shared.main_ram.bytes()
    }

    pub fn shared_wram(&self, cpu: Cpu) -> &[u8] {
        self.shared.wram.bytes(cpu)
    }

    pub fn arm7_wram(&self) -> &[u8] {
        self.arm7.wram.bytes()
    }

    pub fn bios(&self, cpu: Cpu) -> &[u8] {
        match cpu {
            Cpu::Arm9 => &self.arm9.bios,
            Cpu::Arm7 => &self.arm7.bios,
        }
    }

    pub fn tcm(&self) -> &Tcm {
        &self.arm9.tcm
    }

    pub fn tcm_mut(&mut self) -> &mut Tcm {
        #[cfg(jit)]
        self.arm9.fast.moved();
        &mut self.arm9.tcm
    }

    pub fn cp15(&self) -> &Cp15 {
        &self.arm9.cp15
    }

    pub fn boot_cp15(&mut self) {
        self.arm9.cp15.direct_boot();
        self.arm9.place_tcm();
        self.arm9.refresh_caches();
    }

    pub fn vram(&self) -> &Vram {
        &self.shared.vram
    }

    pub fn palette(&self) -> &[u8] {
        self.arm9.palette.bytes()
    }

    pub fn oam(&self) -> &[u8] {
        self.arm9.oam.bytes()
    }

    pub fn cart(&self) -> &Cartridge {
        &self.shared.cart
    }

    pub fn cart_mut(&mut self) -> &mut Cartridge {
        &mut self.shared.cart
    }

    pub fn spi(&self) -> &Spi {
        &self.arm7.spi
    }

    pub fn spi_mut(&mut self) -> &mut Spi {
        &mut self.arm7.spi
    }

    pub fn wifi(&self) -> &Wifi {
        &self.arm7.wifi
    }

    pub fn wifi_mut(&mut self) -> &mut Wifi {
        &mut self.arm7.wifi
    }

    pub fn rtc_mut(&mut self) -> &mut Rtc {
        &mut self.arm7.rtc
    }

    pub fn three_d(&self) -> &ThreeD {
        &self.arm9.three_d
    }

    pub fn three_d_scene(&self) -> Scene<'_> {
        self.arm9.three_d.scene()
    }

    pub fn lend_3d(&mut self, tasks: usize) -> (Vec<raster::Task>, raster::Ending) {
        self.arm9.three_d.lend(tasks)
    }

    pub fn home_3d(&mut self) {
        self.arm9.three_d.come_home();
    }

    pub fn lend_lines(&mut self, painters: usize) -> (Vec<lines::Task>, Option<lines::Ending>) {
        self.lines.lend(painters)
    }

    pub fn finish_lines(&mut self) {
        self.lines.finish(&mut self.screens);
    }

    pub fn home_lines(&mut self) {
        self.lines.come_home(&mut self.screens);
    }

    pub fn side_work(&self) -> SideWork {
        let [lines_3d, geometry] = self.arm9.three_d.side_work();
        SideWork {
            lines_2d: self.lines.painted(),
            lines_3d,
            geometry,
        }
    }

    pub fn lcd(&self) -> &Lcd {
        &self.shared.lcd
    }

    pub fn screens(&self) -> &Screens {
        &self.screens
    }

    pub fn defer_3d(&mut self, on: bool) {
        self.defer_3d = on;
        if !on {
            self.arm9.three_d.draw_now();
        }
    }

    pub fn settle_3d(&mut self) {
        if self.screens.missing_3d.is_empty() {
            return;
        }
        let upscaled = self
            .screens
            .composite
            .as_ref()
            .is_some_and(|composite| composite.upscalable());
        if !upscaled || !self.arm9.three_d.later() {
            self.fill_3d();
        }
    }

    pub fn fill_3d(&mut self) -> bool {
        let mut missing = std::mem::take(&mut self.screens.missing_3d);
        let mut filled = false;
        if let Some(composite) = self.screens.composite.as_mut() {
            for &(y, top) in &missing {
                if !composite.needs_3d(y) {
                    continue;
                }
                let line = self.arm9.three_d.line(y);
                let screen = if top {
                    &mut self.screens.top
                } else {
                    &mut self.screens.bottom
                };
                composite.fill_3d(y, line, &mut screen[y * WIDTH..(y + 1) * WIDTH]);
                filled = true;
            }
        }
        missing.clear();
        self.screens.missing_3d = missing;
        filled
    }

    pub fn record_composite(&mut self, on: bool) {
        match (on, self.screens.composite.is_some()) {
            (true, false) => self.screens.composite = Some(Box::default()),
            (false, true) => self.screens.composite = None,
            _ => {}
        }
    }

    pub fn interrupts(&self, cpu: Cpu) -> &Interrupts {
        &self.shared.irq[side(cpu)]
    }

    pub fn keys(&self) -> Keys {
        self.shared.input.keys()
    }

    pub fn set_keys(&mut self, keys: Keys) {
        let [arm9, arm7] = self.shared.input.set_keys(keys);
        if arm9 {
            self.shared.irq[0].request(Irq::KEYPAD);
        }
        if arm7 {
            self.request_arm7(Irq::KEYPAD);
        }
        let power = &mut self.arm7.spi.devices_mut().power;
        power.set_lid_closed(keys.contains(Keys::LID));
        if power.take_lid_interrupt() {
            self.request_arm7(Irq::LID);
        }
    }

    pub fn lid_closed(&self) -> bool {
        self.shared.input.keys().contains(Keys::LID)
    }

    pub fn sleeping(&self) -> bool {
        self.arm7.sleeping
    }

    pub fn sleep(&mut self, cycles: u32) -> bool {
        self.step_rtc(cycles);
        self.sleep_cycles += cycles;
        if self.sleep_cycles < FRAME_CYCLES {
            return false;
        }
        self.sleep_cycles -= FRAME_CYCLES;
        self.lines.finish(&mut self.screens);
        self.screens.top.fill(0);
        self.screens.bottom.fill(0);
        self.screens.missing_3d.clear();
        if let Some(composite) = self.screens.composite.as_mut() {
            **composite = Composite::default();
        }
        true
    }

    pub fn until_sleep_frame(&self) -> u32 {
        FRAME_CYCLES - self.sleep_cycles
    }

    fn request_arm7(&mut self, irq: Irq) {
        self.shared.irq[1].request(irq);
        if self.arm7.sleeping && self.shared.irq[1].enables(irq) {
            self.arm7.sleeping = false;
            self.sleep_cycles = 0;
        }
    }

    pub fn set_pen(&mut self, pen: Option<(u8, u8)>) {
        self.shared.input.set_pen(pen.is_some());
        self.arm7.spi.devices_mut().touch.set_pen(pen);
    }

    pub fn halted(&self, cpu: Cpu) -> bool {
        match cpu {
            Cpu::Arm9 => self.arm9.halted,
            Cpu::Arm7 => self.arm7.halted,
        }
    }

    pub fn wake(&mut self, cpu: Cpu) -> bool {
        if !self.shared.irq[side(cpu)].wake() {
            return false;
        }
        match cpu {
            Cpu::Arm9 => self.arm9.halted = false,
            Cpu::Arm7 => self.arm7.halted = false,
        }
        true
    }

    pub fn irq_line(&self, cpu: Cpu) -> bool {
        self.shared.irq[side(cpu)].line()
    }

    #[inline]
    pub fn until_event(&self) -> u32 {
        let mut until = self.shared.lcd.until_event();
        if let Some(cycles) = self.arm9.timers.until_event() {
            until = until.min(cycles.div_ceil(2));
        }
        if let Some(cycles) = self.arm7.timers.until_event() {
            until = until.min(cycles);
        }
        if let Some(cycles) = self.shared.cart.until_event() {
            until = until.min(cycles);
        }
        if let Some(cycles) = self.arm7.spi.until_event() {
            until = until.min(cycles);
        }
        let dma = &self.arm9.dma;
        if let Some(cycles) = self
            .arm9
            .three_d
            .until_event(|| dma.waiting(Start::GeometryFifo))
        {
            until = until.min(cycles);
        }
        if self.arm7.wifi.powered()
            && let Some(cycles) = self.arm7.wifi.until_event()
        {
            until = until.min(cycles);
        }
        until.max(1)
    }

    pub fn elapse(&mut self, mut cycles: u32) -> bool {
        let mut frame = false;
        while cycles > 0 {
            let step = cycles.min(self.shared.lcd.until_event());
            self.step_devices(step);
            if let Some(event) = self.shared.lcd.advance(step) {
                frame |= self.lcd_event(event);
            }
            cycles -= step;
        }
        self.shared.cart.settle();
        self.arm7.spi.settle();
        frame
    }

    pub fn slice_cut(&self) -> bool {
        self.shared.cart.armed() || self.arm7.spi.armed()
    }

    pub fn slice_cut_since(&mut self, cycles: u32) {
        self.shared.cart.armed_since(cycles);
        self.arm7.spi.armed_since(cycles);
    }

    fn step_devices(&mut self, cycles: u32) {
        let irq = self.arm9.timers.step(cycles * 2);
        self.shared.irq[0].request(irq);
        let irq = self.arm7.timers.step(cycles);
        self.shared.irq[1].request(irq);
        self.arm9.math.step(cycles);
        self.step_geometry(cycles);
        self.arm7.spi.step_slice(cycles);
        if self.arm7.spi.take_interrupt() {
            self.shared.irq[1].request(Irq::SPI);
        }
        self.step_rtc(cycles);
        self.step_sound(cycles);
        self.step_cart(cycles);
        if self.arm7.wifi.powered() {
            let irq = self.arm7.wifi.step(cycles);
            self.shared.irq[1].request(irq);
        }
    }

    fn step_rtc(&mut self, cycles: u32) {
        self.arm7.rtc.tick(cycles);
        let interrupt = self.arm7.rtc.interrupt();
        if interrupt && !self.arm7.rtc_interrupt && self.arm7.rcnt & RCNT_RTC_MASK == RCNT_RTC_IRQ {
            self.request_arm7(Irq::SERIAL);
        }
        self.arm7.rtc_interrupt = interrupt;
    }

    fn step_sound(&mut self, cycles: u32) {
        let mixes = self.arm7.sound.elapse(cycles);
        if mixes != 0 {
            self.mix_sound(mixes);
        }
    }

    #[inline(never)]
    fn mix_sound(&mut self, mixes: u32) {
        let muted = self.sound_muted;
        let mut sound = std::mem::take(&mut self.arm7.sound);
        let mut bus = self.arm7();
        for _ in 0..mixes {
            sound.mix(&mut bus, muted);
        }
        self.arm7.sound = sound;
    }

    pub fn sound_muted(&self) -> bool {
        self.sound_muted
    }

    pub fn set_sound_muted(&mut self, muted: bool) {
        self.sound_muted = muted;
    }

    pub fn set_skip_draw(&mut self, on: bool) {
        self.skip_draw = on;
    }

    pub fn take_sound_samples(&mut self) -> std::vec::Drain<'_, [i16; 2]> {
        self.arm7.sound.take_samples()
    }

    pub fn sound(&self) -> &Mixer {
        &self.arm7.sound
    }

    pub fn rtc(&self) -> &Rtc {
        &self.arm7.rtc
    }

    pub fn next_geometry_finish(&self) -> Option<u32> {
        self.arm9.three_d.next_finish()
    }

    pub fn finish_geometry(&mut self, cycles: u32) -> Option<u32> {
        loop {
            let finish = self.arm9.three_d.next_finish()?;
            if finish > cycles {
                return Some(finish);
            }
            self.arm9.three_d.finish_command();
            self.geometry_signals();
        }
    }

    fn step_geometry(&mut self, cycles: u32) {
        self.arm9.three_d.step(cycles);
        self.geometry_signals();
    }

    fn geometry_signals(&mut self) {
        if self.arm9.three_d.dma_request() {
            self.trigger_dma(Cpu::Arm9, Start::GeometryFifo);
        }
        self.shared.irq[0].hold(Irq::GEOMETRY_FIFO, self.arm9.three_d.irq());
    }

    fn step_cart(&mut self, cycles: u32) {
        self.shared.cart.step_slice(cycles);
        let owner = self.shared.cart_owner();
        if self.shared.cart.take_interrupt() {
            self.shared.irq[side(owner)].request(Irq::CARD_DONE);
        }
        if self.shared.cart.dma_request() {
            let cycles = self.trigger_dma(owner, Start::Card);
            if !self.shared.cart.dma_request() {
                self.shared.cart.delay(cycles);
            }
        }
    }

    fn trigger_dma(&mut self, cpu: Cpu, start: Start) -> u32 {
        let pending = match cpu {
            Cpu::Arm9 => self.arm9.dma.trigger(start),
            Cpu::Arm7 => self.arm7.dma.trigger(start),
        };
        if pending { self.run_dma(cpu) } else { 0 }
    }

    #[inline(never)]
    fn run_dma(&mut self, cpu: Cpu) -> u32 {
        match cpu {
            Cpu::Arm9 => self.arm9().run_dma(),
            Cpu::Arm7 => self.arm7().run_dma(),
        }
    }

    pub fn take_stall(&mut self, cpu: Cpu) -> u32 {
        match cpu {
            Cpu::Arm9 => std::mem::take(&mut self.arm9.stall),
            Cpu::Arm7 => std::mem::take(&mut self.arm7.stall),
        }
    }

    #[inline(never)]
    fn lcd_event(&mut self, event: LcdEvent) -> bool {
        for cpu in [Cpu::Arm9, Cpu::Arm7] {
            let irq = self.shared.lcd.irqs(event, cpu);
            self.shared.irq[side(cpu)].request(irq);
        }
        match event {
            LcdEvent::HBlank(line) if line < VISIBLE_LINES => {
                self.draw_line(usize::from(line));
                self.trigger_dma(Cpu::Arm9, Start::HBlank);
                false
            }
            LcdEvent::HBlank(_) => false,
            LcdEvent::Line(line) => {
                for engine in &mut self.arm9.engines {
                    engine.start_line(line);
                }
                if line == RENDER_LINE && self.arm9.power & RENDERING_ON != 0 {
                    let defer = self.skip_draw || self.defer_3d && self.screens.composite.is_some();
                    if self.arm9.three_d.render(&self.shared.vram, defer) {
                        self.screens.missing_3d.clear();
                    }
                }
                if line != VISIBLE_LINES {
                    return false;
                }
                if self.arm9.power & GEOMETRY_ON != 0 {
                    self.arm9
                        .three_d
                        .vblank(self.arm9.power & RENDERING_ON != 0);
                }
                self.trigger_dma(Cpu::Arm9, Start::VBlank);
                self.trigger_dma(Cpu::Arm7, Start::VBlank);
                true
            }
        }
    }

    fn draw_line(&mut self, y: usize) {
        self.lines.sync(&mut self.shared.vram, &mut self.screens);
        let swapped = self.arm9.power & SWAP_SCREENS != 0;
        if y == 0 {
            self.screens.missing_3d.clear();
        }
        if self.skip_draw && !self.arm9.engines[0].feeds_the_game(y) {
            return;
        }
        let with_3d = !self.arm9.three_d.later()
            || self.screens.composite.is_none()
            || self.arm9.engines[0].reads_3d(y);
        if !with_3d {
            self.screens.missing_3d.push((y, swapped));
        }
        if !self.ship_line(EngineKind::A, y, swapped, with_3d) {
            self.draw_a(y, swapped, with_3d);
        }
        if !self.ship_line(EngineKind::B, y, !swapped, false) {
            self.draw_b(y, !swapped);
        }
    }

    fn ship_line(&mut self, kind: EngineKind, y: usize, top: bool, with_3d: bool) -> bool {
        let engine = &self.arm9.engines[kind as usize];
        let shown = kind == EngineKind::A || self.arm9.power & ENGINE_B_ON != 0;
        let layered = kind == EngineKind::A && self.screens.composite.is_some();
        if !shown || !engine.paints_alone(y) || !engine.draws_layers(y, layered) {
            return false;
        }
        let Some(mut job) = self.lines.job(y, kind, engine.look(), top) else {
            return false;
        };
        let start = kind as usize * ENGINE_BYTES;
        let bytes = start..start + ENGINE_BYTES;
        job.palette
            .copy_from_slice(&self.arm9.palette.bytes()[bytes.clone()]);
        job.oam.copy_from_slice(&self.arm9.oam.bytes()[bytes]);
        job.layered = false;
        if let Some(composite) = self.screens.composite.as_mut()
            && kind == EngineKind::A
        {
            composite.start_line(y, top);
            job.layered = true;
        }
        job.with_3d = kind == EngineKind::A;
        if job.with_3d {
            let line = if with_3d {
                self.arm9.three_d.line(y)
            } else {
                &BLANK_3D
            };
            job.three_d.color = line.color;
            job.three_d.alpha = line.alpha;
        }
        self.lines.ship(job);
        true
    }

    fn draw_a(&mut self, y: usize, top: bool, with_3d: bool) {
        let mut a = [0; WIDTH];
        let palette = self.arm9.palette.bytes();
        let oam = self.arm9.oam.bytes();
        let vram = &self.shared.vram;
        let layered = self.screens.composite.is_some();
        let engine = &mut self.arm9.engines[0];
        let memory = Memory {
            vram: &engine_view(vram, engine, y, layered),
            palette: &palette[..ENGINE_BYTES],
            oam: &oam[..ENGINE_BYTES],
            three_d: Some(if with_3d {
                self.arm9.three_d.line(y)
            } else {
                &BLANK_3D
            }),
        };
        let capture = engine.display_line(y, &memory, vram, layered, &mut a);
        if let Some(composite) = self.screens.composite.as_mut() {
            let line = composite.start_line(y, top);
            let scroll = match capture {
                Some(_) => Err(Fallback::Capture),
                None => Ok(engine.layered_line(memory.palette, &a, line)),
            };
            composite.end_line(y, scroll, engine.master_shade());
        }
        if let Some(capture) = capture {
            capture.write(&mut self.shared.vram);
        }
        self.screens.put_row(y, top, &a);
    }

    fn draw_b(&mut self, y: usize, top: bool) {
        let mut b = [0; WIDTH];
        if self.arm9.power & ENGINE_B_ON != 0 {
            let palette = self.arm9.palette.bytes();
            let oam = self.arm9.oam.bytes();
            let vram = &self.shared.vram;
            let engine = &mut self.arm9.engines[1];
            let memory = Memory {
                vram: &engine_view(vram, engine, y, false),
                palette: &palette[ENGINE_BYTES..],
                oam: &oam[ENGINE_BYTES..],
                three_d: None,
            };
            engine.display_line(y, &memory, vram, false, &mut b);
        } else {
            b.fill(color::WHITE);
        }
        self.screens.put_row(y, top, &b);
    }
}

impl Arm9Memory {
    fn copy_state_from(&mut self, source: &Arm9Memory) {
        let Self {
            bios,
            tcm,
            palette,
            oam,
            cp15,
            caches,
            timers,
            dma,
            math,
            engines,
            three_d,
            stall,
            power,
            postflg,
            halted,
            attention,
            pending,
            ahead,
            #[cfg(jit)]
            fast,
        } = self;
        bios.clone_from(&source.bios);
        tcm.clone_from(&source.tcm);
        palette.clone_from(&source.palette);
        oam.clone_from(&source.oam);
        cp15.clone_from(&source.cp15);
        caches.clone_from(&source.caches);
        timers.clone_from(&source.timers);
        dma.clone_from(&source.dma);
        math.clone_from(&source.math);
        engines.clone_from(&source.engines);
        three_d.clone_from(&source.three_d);
        *stall = source.stall;
        *power = source.power;
        *postflg = source.postflg;
        *halted = source.halted;
        *attention = source.attention;
        *pending = source.pending;
        *ahead = source.ahead;
        #[cfg(jit)]
        fast.clone_from(&source.fast);
    }

    #[cfg(feature = "persistence")]
    fn copy_saved_from(&mut self, source: &Arm9Memory) {
        let Self {
            bios: _,
            tcm,
            palette,
            oam,
            cp15,
            caches: _,
            timers,
            dma,
            math,
            engines,
            three_d,
            stall,
            power,
            postflg,
            halted,
            attention: _,
            pending: _,
            ahead,
            #[cfg(jit)]
                fast: _,
        } = self;
        tcm.clone_from(&source.tcm);
        palette.clone_from(&source.palette);
        oam.clone_from(&source.oam);
        cp15.clone_from(&source.cp15);
        timers.clone_from(&source.timers);
        dma.clone_from(&source.dma);
        math.clone_from(&source.math);
        engines.clone_from(&source.engines);
        three_d.copy_saved_from(&source.three_d);
        *stall = source.stall;
        *power = source.power;
        *postflg = source.postflg;
        *halted = source.halted;
        *ahead = source.ahead;
    }

    fn place_tcm(&mut self) {
        self.tcm.place_itcm(self.cp15.itcm());
        self.tcm.place_dtcm(self.cp15.dtcm());
        #[cfg(jit)]
        self.fast.moved();
    }

    fn refresh_caches(&mut self) {
        self.caches.refresh(&self.cp15);
    }
}

impl Arm7Memory {
    fn copy_state_from(&mut self, source: &Arm7Memory) {
        let Self {
            bios,
            wram,
            code,
            timers,
            dma,
            spi,
            rtc,
            sound,
            rtc_interrupt,
            power,
            postflg,
            halted,
            biosprot,
            rcnt,
            wifi_waitcnt,
            wifi,
            sleeping,
            stall,
            attention,
            pending,
            #[cfg(jit)]
            fast,
        } = self;
        bios.clone_from(&source.bios);
        wram.clone_from(&source.wram);
        code.clone_from(&source.code);
        timers.clone_from(&source.timers);
        dma.clone_from(&source.dma);
        spi.clone_from(&source.spi);
        rtc.clone_from(&source.rtc);
        sound.clone_from(&source.sound);
        *rtc_interrupt = source.rtc_interrupt;
        *power = source.power;
        *postflg = source.postflg;
        *halted = source.halted;
        *biosprot = source.biosprot;
        *rcnt = source.rcnt;
        *wifi_waitcnt = source.wifi_waitcnt;
        wifi.clone_from(&source.wifi);
        *sleeping = source.sleeping;
        *stall = source.stall;
        *attention = source.attention;
        *pending = source.pending;
        #[cfg(jit)]
        fast.clone_from(&source.fast);
    }
}

#[cfg(feature = "persistence")]
impl Arm7Memory {
    fn copy_saved_from(&mut self, source: &Arm7Memory) {
        let Self {
            bios: _,
            wram,
            code: _,
            timers,
            dma,
            spi,
            rtc,
            sound,
            rtc_interrupt,
            power,
            postflg,
            halted,
            biosprot,
            rcnt,
            wifi_waitcnt,
            wifi,
            sleeping,
            stall: _,
            attention: _,
            pending: _,
            #[cfg(jit)]
                fast: _,
        } = self;
        wram.clone_from(&source.wram);
        timers.clone_from(&source.timers);
        dma.clone_from(&source.dma);
        spi.clone_from(&source.spi);
        rtc.clone_from(&source.rtc);
        sound.clone_from(&source.sound);
        *rtc_interrupt = source.rtc_interrupt;
        *power = source.power;
        *postflg = source.postflg;
        *halted = source.halted;
        *biosprot = source.biosprot;
        *rcnt = source.rcnt;
        *wifi_waitcnt = source.wifi_waitcnt;
        wifi.clone_from(&source.wifi);
        *sleeping = source.sleeping;
    }
}

impl Shared {
    fn copy_state_from(&mut self, source: &Shared) {
        let Self {
            main_ram,
            wram,
            vram,
            exmem,
            arm7_exmem,
            irq,
            ipc,
            cart,
            lcd,
            input,
            ipc_traffic,
        } = self;
        main_ram.clone_from(&source.main_ram);
        wram.clone_from(&source.wram);
        vram.clone_from(&source.vram);
        *exmem = source.exmem;
        *arm7_exmem = source.arm7_exmem;
        irq.clone_from(&source.irq);
        ipc.clone_from(&source.ipc);
        cart.clone_from(&source.cart);
        lcd.clone_from(&source.lcd);
        input.clone_from(&source.input);
        ipc_traffic.clone_from(&source.ipc_traffic);
    }

    #[cfg(feature = "persistence")]
    fn copy_saved_from(&mut self, source: &Shared) {
        let Self {
            main_ram,
            wram,
            vram,
            exmem,
            arm7_exmem,
            irq,
            ipc,
            cart,
            lcd,
            input,
            ipc_traffic: _,
        } = self;
        main_ram.clone_from(&source.main_ram);
        wram.clone_from(&source.wram);
        vram.clone_from(&source.vram);
        *exmem = source.exmem;
        *arm7_exmem = source.arm7_exmem;
        irq.clone_from(&source.irq);
        ipc.clone_from(&source.ipc);
        cart.clone_from(&source.cart);
        lcd.clone_from(&source.lcd);
        input.clone_from(&source.input);
    }

    fn arm9_exmem(&self) -> u16 {
        self.exmem
    }

    fn arm7_exmem(&self) -> u16 {
        self.exmem & EXMEM_SHARED | self.arm7_exmem
    }

    fn write_arm9_exmem(&mut self, value: u16, mask: u16) {
        let mask = mask & EXMEM_WRITABLE;
        self.exmem = self.exmem & !mask | value & mask;
    }

    fn write_arm7_exmem(&mut self, value: u16, mask: u16) {
        let mask = mask & EXMEM_OWN;
        self.arm7_exmem = self.arm7_exmem & !mask | value & mask;
    }

    fn gba_slot_owner_is_arm7(&self) -> bool {
        self.exmem & GBA_SLOT_ARM7 != 0
    }

    fn gba_slot_dma_timing(&self, arm7: bool, addr: u32) -> Timing {
        if arm7 != self.gba_slot_owner_is_arm7() {
            return Timing::UNMAPPED;
        }
        let control = if arm7 { self.arm7_exmem } else { self.exmem };
        if addr < GBA_ROM_END {
            let n = GBA_SLOT_FIRST_WAITS[usize::from(control >> 2 & 3)];
            let s = if control & 1 << 4 != 0 { 4 } else { 6 };
            Timing::new(Area::Slow(DMA_SLOT_ROM), 16, n, s)
        } else {
            let n = GBA_SLOT_FIRST_WAITS[usize::from(control & 3)];
            Timing::new(Area::Plain(DMA_SLOT_RAM), 8, n, n)
        }
    }

    fn cart_owner(&self) -> Cpu {
        if self.exmem & CART_ARM7 != 0 {
            Cpu::Arm7
        } else {
            Cpu::Arm9
        }
    }

    fn read_gba_slot(&self, arm7: bool, addr: u32, width: Width) -> u32 {
        if arm7 != self.gba_slot_owner_is_arm7() {
            return 0;
        }
        if addr >= GBA_ROM_END {
            return NO_CART_RAM * splat(width);
        }
        let word = addr & !3;
        let low = word >> 1 & 0xFFFF;
        let high = (word + 2) >> 1 & 0xFFFF;
        lane(high << 16 | low, addr, width)
    }

    fn read_ipc(&self, cpu: Cpu, offset: usize) -> u32 {
        self.ipc.read(cpu, offset)
    }

    fn write_ipc(&mut self, cpu: Cpu, offset: usize, value: u32, mask: u32) {
        let kind = match offset & !3 {
            ipc::SYNC => Some(Kind::IpcSync),
            ipc::CONTROL => Some(Kind::IpcControl),
            ipc::SEND => Some(Kind::IpcSend),
            _ => None,
        };
        if let Some(kind) = kind {
            self.ipc_traffic.push(
                Entry::new(kind)
                    .tag(side(cpu) as u8)
                    .at(self.cart.clock())
                    .word(value & mask),
            );
        }
        self.ipc.write(cpu, offset, value, mask);
        self.take_ipc_requests();
    }

    fn peek_ipc(&self, cpu: Cpu) -> u32 {
        self.ipc.peek(cpu)
    }

    fn receive_ipc(&mut self, cpu: Cpu) -> u32 {
        let word = self.ipc.receive(cpu);
        self.take_ipc_requests();
        word
    }

    fn take_ipc_requests(&mut self) {
        for cpu in [Cpu::Arm9, Cpu::Arm7] {
            let requests = self.ipc.take_requests(cpu);
            self.irq[side(cpu)].request(requests);
        }
    }

    fn read_cart(&self, cpu: Cpu, offset: usize) -> u32 {
        if self.cart_owner() == cpu {
            self.cart.read(offset)
        } else {
            0
        }
    }

    fn write_cart(&mut self, cpu: Cpu, offset: usize, value: u32, mask: u32) {
        if self.cart_owner() == cpu {
            self.cart.write(offset, value, mask);
        }
    }

    fn peek_cart_data(&self, cpu: Cpu) -> u32 {
        if self.cart_owner() == cpu {
            self.cart.peek_data()
        } else {
            0
        }
    }

    fn read_cart_data(&mut self, cpu: Cpu) -> u32 {
        if self.cart_owner() == cpu {
            self.cart.read_data()
        } else {
            0
        }
    }
}

fn side(cpu: Cpu) -> usize {
    match cpu {
        Cpu::Arm9 => 0,
        Cpu::Arm7 => 1,
    }
}

fn bios_image(image: &[u8], window: usize) -> Box<[u8]> {
    image[..image.len().min(window)].into()
}

fn read_bios(bios: &[u8], offset: usize, width: Width) -> u32 {
    let start = offset & !(width.bytes() - 1);
    bios.get(start..start + width.bytes())
        .map_or(0, |bytes| match width {
            Width::Byte => u32::from(bytes[0]),
            Width::Half => u32::from(u16::from_le_bytes([bytes[0], bytes[1]])),
            Width::Word => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        })
}

fn splat(width: Width) -> u32 {
    match width {
        Width::Byte => 0x01,
        Width::Half => 0x0101,
        Width::Word => 0x0101_0101,
    }
}

fn width_mask(width: Width) -> u32 {
    match width {
        Width::Byte => 0xFF,
        Width::Half => 0xFFFF,
        Width::Word => 0xFFFF_FFFF,
    }
}

fn lanes(addr: u32, width: Width, value: u32) -> (u32, u32) {
    let shift = 8 * (addr & 3 & !(width.bytes() as u32 - 1));
    let mask = width_mask(width);
    ((value & mask) << shift, mask << shift)
}

fn engine_view<'a>(vram: &'a Vram, engine: &Engine, y: usize, layered: bool) -> EngineView<'a> {
    match engine.draws_layers(y, layered) {
        true => EngineView::new(vram, engine.kind()),
        false => EngineView::unpaged(vram, engine.kind()),
    }
}

fn read_engine(engine: &Engine, offset: usize) -> u32 {
    u32::from(engine.read(offset)) | u32::from(engine.read(offset + 2)) << 16
}

fn write_engine(engine: &mut Engine, offset: usize, value: u32, mask: u32) {
    for half in 0..2 {
        let shift = 16 * half;
        let mask = (mask >> shift) as u16;
        if mask != 0 {
            engine.write(offset + 2 * half as usize, (value >> shift) as u16, mask);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXMEMCNT: u32 = 0x0400_0204;
    const GBA_ROM: u32 = 0x0800_0000;
    const GBA_RAM: u32 = 0x0A00_0000;

    #[test]
    fn main_ram_is_the_same_4m_on_both_cpus_and_mirrors() {
        let mut buses = Buses::default();
        buses.arm9().write(0x0200_1000, Width::Word, 0x1234_5678);
        assert_eq!(buses.arm7().read(0x0240_1000, Width::Word), 0x1234_5678);
        buses.arm7().write(0x02FF_FFFE, Width::Half, 0xABCD);
        assert_eq!(buses.arm9().read(0x023F_FFFE, Width::Half), 0xABCD);
    }

    #[test]
    fn exmemcnt_high_bits_belong_to_the_arm9_and_low_bits_to_each_cpu() {
        let mut buses = Buses::default();
        buses.arm9().write(EXMEMCNT, Width::Half, 0x8885);
        buses.arm7().write(EXMEMCNT, Width::Half, 0x0003);
        assert_eq!(buses.arm9().read(EXMEMCNT, Width::Half), 0x8885);
        assert_eq!(buses.arm7().read(EXMEMCNT, Width::Half), 0x8883);
    }

    #[test]
    fn exmemcnt_unused_bits_read_as_zero() {
        let mut buses = Buses::default();
        buses.arm9().write(EXMEMCNT, Width::Half, 0xFFFF);
        assert_eq!(buses.arm9().read(EXMEMCNT, Width::Half), 0xE8FF);
    }

    #[test]
    fn the_gba_slot_owner_sees_an_empty_slot_and_the_other_cpu_zero() {
        let mut buses = Buses::default();
        assert_eq!(buses.arm9().read(GBA_ROM + 0x10, Width::Half), 0x0008);
        assert_eq!(buses.arm9().read(GBA_ROM + 0x10, Width::Word), 0x0009_0008);
        assert_eq!(buses.arm9().read(GBA_RAM + 0x10, Width::Byte), 0xFF);
        assert_eq!(buses.arm7().read(GBA_ROM + 0x10, Width::Half), 0);
        assert_eq!(buses.arm7().read(GBA_RAM, Width::Byte), 0);
    }

    #[test]
    fn handing_the_gba_slot_to_the_arm7_swaps_who_sees_it() {
        let mut buses = Buses::default();
        buses.arm9().write(EXMEMCNT, Width::Half, 0x0080);
        assert_eq!(buses.arm9().read(GBA_ROM + 0x10, Width::Half), 0);
        assert_eq!(buses.arm7().read(GBA_ROM + 0x10, Width::Half), 0x0008);
    }

    #[test]
    fn a_bios_image_reads_from_its_start_and_zero_past_its_end() {
        let mut buses = Buses::default();
        buses.load_arm9_bios(&[0x11, 0x22, 0x33, 0x44]);
        buses.load_arm7_bios(&[0xAA, 0xBB]);
        assert_eq!(buses.arm9().read(0xFFFF_0000, Width::Word), 0x4433_2211);
        assert_eq!(buses.arm9().read(0xFFFF_0004, Width::Word), 0);
        assert_eq!(buses.arm7().read(0x0000_0000, Width::Half), 0xBBAA);
        assert_eq!(buses.arm7().read(0x0000_0002, Width::Half), 0);
    }

    #[test]
    fn bios_writes_are_ignored() {
        let mut buses = Buses::default();
        buses.load_arm7_bios(&[1, 2, 3, 4]);
        buses.arm7().write(0, Width::Word, 0);
        assert_eq!(buses.arm7().read(0, Width::Word), 0x0403_0201);
    }

    #[test]
    fn lanes_put_a_narrow_value_where_its_address_points() {
        assert_eq!(
            lanes(0x0400_0206, Width::Half, 0x1234),
            (0x1234_0000, 0xFFFF_0000)
        );
        assert_eq!(
            lanes(0x0400_0203, Width::Byte, 0xAB),
            (0xAB00_0000, 0xFF00_0000)
        );
        assert_eq!(lanes(0x0400_0200, Width::Word, 7), (7, 0xFFFF_FFFF));
    }

    #[test]
    fn a_vblank_starts_the_frame_and_raises_each_enabled_irq() {
        let mut buses = Buses::default();
        buses.arm9().write(0x0400_0004, Width::Half, 1 << 3);
        let to_vblank = super::super::lcd::LINE_CYCLES * u32::from(VISIBLE_LINES);
        assert!(!buses.elapse(to_vblank - 1));
        assert!(buses.elapse(1));
        assert_eq!(buses.interrupts(Cpu::Arm9).flags(), Irq::VBLANK);
        assert_eq!(buses.interrupts(Cpu::Arm7).flags(), Irq::empty());
    }

    #[test]
    fn either_cpu_can_renumber_the_next_line() {
        let mut buses = Buses::default();
        let line = super::super::lcd::LINE_CYCLES;
        buses.elapse(line * 200);
        buses.arm7().write(0x0400_0006, Width::Half, 195);
        assert_eq!(buses.lcd().line(), 200);
        buses.elapse(line);
        assert_eq!(buses.lcd().line(), 195);
        buses.arm9().write(0x0400_0006, Width::Half, 199);
        buses.elapse(line);
        assert_eq!(buses.lcd().line(), 199);
    }

    #[test]
    fn engine_a_goes_to_the_top_screen_when_the_screens_are_swapped() {
        let mut buses = Buses::default();
        buses.arm9().write(0x0400_0304, Width::Half, 0x8203);
        buses.arm9().write(0x0400_0240, Width::Byte, 0x80);
        buses.arm9().write(0x0680_0000, Width::Half, 0x001F);
        buses.arm9().write(0x0400_0000, Width::Word, 2 << 16);
        buses.arm9().write(0x0400_1000, Width::Word, 0);
        buses.elapse(super::super::lcd::HBLANK_START);
        assert_eq!(buses.screens().top()[0], color::widen(0x001F));
        assert_eq!(buses.screens().bottom()[0], color::WHITE);
        buses.arm9().write(0x0400_0304, Width::Half, 0x0203);
        buses.arm9().write(0x0680_0200, Width::Half, 0x001F);
        buses.elapse(super::super::lcd::LINE_CYCLES);
        assert_eq!(buses.screens().bottom()[WIDTH], color::widen(0x001F));
    }
}
