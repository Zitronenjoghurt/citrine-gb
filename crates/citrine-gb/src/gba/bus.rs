use super::cart::Cart;
#[cfg(feature = "debug")]
use super::debug_print::DebugPrint;
use super::dma::{Dma, Timing};
use super::interrupt::{Interrupts, Irq};
use super::keypad::Keypad;
use super::open_bus::OpenBus;
use super::power::Power;
use super::ppu::Ppu;
use super::prefetch::Prefetch;
use super::region::{
    BIOS_SIZE, EWRAM_SIZE, IO_SIZE, IWRAM_SIZE, OAM_SIZE, PALETTE_SIZE, ROM_MAX_SIZE, Region,
    VRAM_SIZE,
};
use super::save::Backup;
use super::serial::{LinkMessage, REGISTERS_LEN, Serial};
use super::sound::{FIFO_ADDRESSES, Sound};
use super::timer::{Overflows, Timers};
use super::wait::{Access, WaitControl, Width};
use crate::cheats::PatchedRom;
use crate::console::Keys;
use crate::paged::Paged;
use cpu_access::Clock;
use std::sync::Arc;

const DISPCNT: usize = 0x000;
const DISPSTAT: usize = 0x004;
const VCOUNT: usize = 0x006;
const BGCNT_FIRST: usize = 0x008;
const BGCNT_LAST: usize = 0x00E;
const BGOFS_FIRST: usize = 0x010;
const BGOFS_LAST: usize = 0x01E;
const AFFINE_FIRST: usize = 0x020;
const AFFINE_LAST: usize = 0x03E;
const WINDOW_FIRST: usize = 0x040;
const MOSAIC: usize = 0x04C;
const BLEND_FIRST: usize = 0x050;
const BLEND_LAST: usize = 0x054;
const SOUND_FIRST: usize = 0x060;
const SOUND_LAST: usize = 0x0A6;
const DMA_FIRST: usize = 0x0B0;
const DMA_LAST: usize = 0x0DE;
const TIMER_FIRST: usize = 0x100;
const TIMER_LAST: usize = 0x10E;
const SIO_FIRST: usize = 0x120;
const SIO_LAST: usize = SIO_FIRST + REGISTERS_LEN - 2;
const KEYINPUT: usize = 0x130;
const KEYCNT: usize = 0x132;
const RCNT: usize = 0x134;
const IE: usize = 0x200;
const IF: usize = 0x202;
const WAITCNT: usize = 0x204;
const IME: usize = 0x208;
const POSTFLG: usize = 0x300;
const HALTCNT_MASK: u16 = 0xFF00;
const VRAM_BG_TILED: usize = 0x1_0000;
const VRAM_BG_BITMAP: usize = 0x1_4000;
const EEPROM_DMA: usize = 3;
const EEPROM_REGION: u32 = 0x0D;
const EEPROM_SMALL_ROM: usize = 0x100_0000;
const EEPROM_LARGE_ROM_WINDOW: u32 = 0x00FF_FF00;
const BIOS_BOOT_LATCH: usize = 0xE4;

const FIFO_TICK_DELAY: u32 = 1;

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Bus {
    #[cfg_attr(feature = "serde", serde(skip))]
    bios: Arc<[u8]>,
    ewram: Paged,
    iwram: Paged,
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::bytes"))]
    io: Box<[u8]>,
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::bytes"))]
    palette: Box<[u8]>,
    vram: Paged,
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::bytes"))]
    oam: Box<[u8]>,
    #[cfg_attr(feature = "serde", serde(skip))]
    rom: Arc<[u8]>,
    #[cfg_attr(feature = "serde", serde(skip))]
    plain_rom_end: usize,
    #[cfg_attr(feature = "serde", serde(skip))]
    patches: PatchedRom,
    backup: Backup,
    cart: Cart,
    wait: WaitControl,
    keypad: Keypad,
    interrupts: Interrupts,
    timers: Timers,
    dma: Dma,
    ppu: Ppu,
    power: Power,
    prefetch: Prefetch,
    open_bus: OpenBus,
    serial: Serial,
    sound: Sound,
    clock: Clock,
    #[cfg(feature = "debug")]
    #[cfg_attr(feature = "serde", serde(skip))]
    debug_print: DebugPrint,
    #[cfg_attr(feature = "serde", serde(skip))]
    dma_check: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    after_dma: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    prepaid: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    owed: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    quiet: u32,
    #[cfg(jit)]
    #[cfg_attr(feature = "serde", serde(skip))]
    code: jit::Code,
}

impl Bus {
    pub fn new(rom: &[u8]) -> Self {
        Self::with_rom(rom.into())
    }

    pub fn with_rom(rom: Arc<[u8]>) -> Self {
        let rom = if rom.len() > ROM_MAX_SIZE {
            rom[..ROM_MAX_SIZE].into()
        } else {
            rom
        };
        Self {
            bios: zeroed(BIOS_SIZE).into(),
            ewram: Paged::zeroed(EWRAM_SIZE),
            iwram: Paged::zeroed(IWRAM_SIZE),
            io: zeroed(IO_SIZE),
            palette: zeroed(PALETTE_SIZE),
            vram: Paged::zeroed(VRAM_SIZE),
            oam: zeroed(OAM_SIZE),
            backup: Backup::for_rom(&rom),
            cart: Cart::for_rom(&rom),
            plain_rom_end: Cart::plain_rom_end(rom.len()),
            rom,
            patches: PatchedRom::default(),
            wait: WaitControl::default(),
            keypad: Keypad::default(),
            interrupts: Interrupts::default(),
            timers: Timers::default(),
            dma: Dma::default(),
            ppu: Ppu::default(),
            power: Power::default(),
            prefetch: Prefetch::default(),
            open_bus: OpenBus::default(),
            serial: Serial::default(),
            sound: Sound::default(),
            clock: Clock::default(),
            #[cfg(feature = "debug")]
            debug_print: DebugPrint::default(),
            dma_check: 0,
            after_dma: false,
            prepaid: 0,
            owed: 0,
            quiet: 0,
            #[cfg(jit)]
            code: jit::Code::default(),
        }
    }

    pub fn load_bios(&mut self, bios: &[u8]) {
        let len = bios.len().min(BIOS_SIZE);
        let mut image = zeroed(BIOS_SIZE);
        image[..len].copy_from_slice(&bios[..len]);
        self.share_bios(image.into());
    }

    pub fn share_bios(&mut self, bios: Arc<[u8]>) {
        if bios.len() != BIOS_SIZE {
            return self.load_bios(&bios);
        }
        self.bios = bios;
        #[cfg(jit)]
        self.code.moved();
        self.open_bus
            .latch_bios(read_le(&self.bios, BIOS_BOOT_LATCH, Width::Word));
    }

    pub fn copy_state_from(&mut self, source: &Bus) {
        let Bus {
            bios: _,
            ewram,
            iwram,
            io,
            palette,
            vram,
            oam,
            rom: _,
            plain_rom_end: _,
            patches: _,
            backup,
            cart,
            wait,
            keypad,
            interrupts,
            timers,
            dma,
            ppu,
            power,
            prefetch,
            open_bus,
            serial,
            sound,
            clock,
            #[cfg(feature = "debug")]
                debug_print: _,
            dma_check,
            after_dma,
            prepaid,
            owed,
            quiet,
            #[cfg(jit)]
            code,
        } = self;
        #[cfg(jit)]
        code.moved();
        ewram.copy_from(&source.ewram);
        iwram.copy_from(&source.iwram);
        io.copy_from_slice(&source.io);
        palette.copy_from_slice(&source.palette);
        vram.copy_from(&source.vram);
        oam.copy_from_slice(&source.oam);
        backup.clone_from(&source.backup);
        cart.clone_from(&source.cart);
        *wait = source.wait;
        *keypad = source.keypad;
        *interrupts = source.interrupts;
        *timers = source.timers;
        *dma = source.dma;
        ppu.copy_state_from(&source.ppu);
        *power = source.power;
        *prefetch = source.prefetch;
        *open_bus = source.open_bus;
        serial.copy_keeping_traffic(&source.serial);
        sound.copy_state_from(&source.sound);
        *clock = source.clock;
        *dma_check = 0;
        *after_dma = source.after_dma;
        *prepaid = 0;
        *owed = source.owed;
        *quiet = 0;
    }

    pub fn share_pages_into(&mut self, slot: &mut Bus) {
        slot.ewram.snapshot_from(&mut self.ewram);
        slot.iwram.snapshot_from(&mut self.iwram);
        slot.vram.snapshot_from(&mut self.vram);
    }

    #[cfg(feature = "persistence")]
    pub(crate) fn has_hardware_sizes(&self) -> bool {
        self.ewram.len() == EWRAM_SIZE
            && self.iwram.len() == IWRAM_SIZE
            && self.io.len() == IO_SIZE
            && self.palette.len() == PALETTE_SIZE
            && self.vram.len() == VRAM_SIZE
            && self.oam.len() == OAM_SIZE
    }

    pub(crate) fn clamp_loaded(&mut self) {
        self.interrupts.clamp_loaded();
        self.ppu.clamp_loaded();
        self.sound.clamp_loaded();
    }

    pub fn adopt_rom(&mut self, source: &Bus) -> bool {
        let changed = !Arc::ptr_eq(&self.rom, &source.rom);
        if changed {
            self.rom = Arc::clone(&source.rom);
            self.plain_rom_end = source.plain_rom_end;
            #[cfg(jit)]
            self.code.rom_changed();
        }
        self.patches.clone_from(&source.patches);
        if !Arc::ptr_eq(&self.bios, &source.bios) {
            self.bios = Arc::clone(&source.bios);
            #[cfg(jit)]
            self.code.moved();
        }
        changed
    }

    pub fn retime_prefetch(&mut self) {
        self.prefetch.retime(self.wait);
    }

    pub fn set_blind(&mut self, blind: bool) {
        self.ppu.set_blind(blind);
    }

    pub fn set_silent(&mut self, silent: bool) {
        self.sound.set_silent(silent);
    }

    pub fn power(&self) -> Power {
        self.power
    }

    pub fn wake(&mut self) {
        self.power = Power::Running;
    }

    pub fn settle_halt(&mut self) -> bool {
        let halting = self.power == Power::Halting;
        self.power = self.power.settled();
        halting
    }

    pub fn rom(&self) -> &[u8] {
        &self.rom
    }

    pub fn shared_rom(&self) -> Arc<[u8]> {
        self.patches.pristine(&self.rom).clone()
    }

    pub(crate) fn pristine_rom(&self) -> &[u8] {
        self.patches.pristine(&self.rom)
    }

    pub(crate) fn set_rom_patches(&mut self, patches: &[(usize, u8)]) -> bool {
        let before = Arc::clone(&self.rom);
        self.patches.set(&mut self.rom, patches);
        #[cfg(jit)]
        self.code.rom_changed();
        !Arc::ptr_eq(&before, &self.rom)
    }

    pub fn wait(&self) -> WaitControl {
        self.wait
    }

    pub fn keypad(&self) -> Keypad {
        self.keypad
    }

    pub fn interrupts(&self) -> Interrupts {
        self.interrupts
    }

    pub fn timers(&self) -> Timers {
        self.timers
    }

    pub fn dma(&self) -> Dma {
        self.dma
    }

    pub fn iwram(&self) -> &[u8] {
        &self.iwram
    }

    pub fn ewram(&self) -> &[u8] {
        &self.ewram
    }

    pub fn backup(&self) -> &Backup {
        &self.backup
    }

    pub fn backup_mut(&mut self) -> &mut Backup {
        &mut self.backup
    }

    pub fn cart(&self) -> &Cart {
        &self.cart
    }

    pub fn cart_mut(&mut self) -> &mut Cart {
        &mut self.cart
    }

    pub fn serial(&self) -> &Serial {
        &self.serial
    }

    pub fn serial_mut(&mut self) -> &mut Serial {
        &mut self.serial
    }

    pub fn sound(&self) -> &Sound {
        &self.sound
    }

    pub fn sound_mut(&mut self) -> &mut Sound {
        &mut self.sound
    }

    pub fn step(&mut self, cycles: u32) {
        self.stall_rest_of_dma();
        self.cart.tick(cycles.saturating_sub(self.clock.cart()));
        let serial_start = self.take_serial_start();
        let last = self.timers_step_last(cycles);
        let (irq, at) = self.serial.step_at(cycles.saturating_sub(serial_start));
        self.interrupts.raise(irq, serial_start + at);
        self.tick_fifos(last);
        self.sound.finish_step(cycles);
    }

    fn tick_fifos(&mut self, overflows: Overflows) {
        let requests = self.sound.timer_overflows(overflows, &self.timers);
        for (index, (fifo, requested)) in FIFO_ADDRESSES.into_iter().zip(requests).enumerate() {
            if requested {
                let at = overflows.first_at(self.sound.fifo_timer(index)) + FIFO_TICK_DELAY;
                self.request_fifo_dma(fifo, at);
            }
        }
    }

    fn fifo_dma_armed(&self) -> bool {
        self.sound.master_enabled()
            && (1..=2).any(|channel| {
                self.dma.special_armed(channel) && FIFO_ADDRESSES.contains(&self.dma.dest(channel))
            })
    }

    #[cfg(feature = "debug")]
    pub fn video_memory(&self) -> (&[u8], &[u8], &[u8]) {
        (&self.vram[..], &self.palette, &self.oam)
    }

    pub fn ppu(&self) -> &Ppu {
        &self.ppu
    }

    pub fn ppu_mut(&mut self) -> &mut Ppu {
        &mut self.ppu
    }

    pub fn step_video(&mut self, cycles: u32) {
        let start = self.take_video_start();
        self.run_video(start, cycles.saturating_sub(start));
        let lead = self.take_irq_lead();
        self.interrupts.elapse_sampled(cycles, lead);
        if self.dma_check != u32::MAX {
            self.dma.elapse(cycles);
            self.dma_check = self.dma_check.saturating_sub(cycles);
        }
    }

    pub fn elapse_interrupts(&mut self, cycles: u32) {
        self.interrupts.elapse(cycles);
    }

    pub fn plug_link(&mut self, unit: usize, units: usize) {
        self.serial.plug(unit, units);
    }

    pub fn unplug_link(&mut self) {
        let irq = self.serial.unplug();
        self.interrupts.request(irq);
    }

    pub fn take_link_messages(&mut self) -> Vec<LinkMessage> {
        self.serial.take_messages()
    }

    pub fn receive_link_message(&mut self, message: LinkMessage) {
        let irq = self.serial.receive(message);
        self.interrupts.request(irq);
    }

    #[cold]
    #[inline(never)]
    fn request_fifo_dma(&mut self, fifo: u32, at: u32) {
        for channel in 1..=2 {
            if self.dma.dest(channel) == fifo && self.dma.request_special(channel, at) {
                self.dma_check = 0;
            }
        }
    }

    pub fn start_dma(&mut self, timing: Timing) {
        self.dma.trigger(timing);
        self.dma_check = 0;
    }

    pub fn start_special_dma(&mut self, channel: usize) {
        self.dma.trigger_special(channel);
        self.dma_check = 0;
    }

    fn quiet_span(&self) -> u32 {
        if !self.interrupts.idle() || self.dma.active() || self.serial.active() || self.prepaid > 0
        {
            return 0;
        }
        let timers = self.timers.until_overflow().unwrap_or(u32::MAX);
        self.ppu
            .until_effect(self.interrupts.ie(), self.dma.capturing(), timers)
    }

    pub fn until_event(&self) -> u32 {
        [
            self.interrupts.until_wake(),
            self.timers.until_overflow(),
            self.serial.until_done(),
            self.dma.due(),
        ]
        .into_iter()
        .flatten()
        .fold(self.ppu.until_event(), u32::min)
    }

    pub fn dma_active(&self) -> bool {
        self.dma.active()
    }

    pub fn set_keys(&mut self, keys: Keys) {
        if self.keypad.set_keys(keys) {
            self.interrupts.request(Irq::KEYPAD);
        }
    }

    pub fn cycles(&self, addr: u32, width: Width, access: Access) -> u32 {
        self.wait.cycles(addr, width, access)
    }

    pub fn read8(&self, addr: u32) -> u8 {
        self.read(addr, Width::Byte) as u8
    }

    pub fn read16(&self, addr: u32) -> u16 {
        self.read(addr, Width::Half) as u16
    }

    pub fn read32(&self, addr: u32) -> u32 {
        self.read(addr, Width::Word)
    }

    pub fn write8(&mut self, addr: u32, value: u8) {
        self.write(addr, Width::Byte, u32::from(value));
    }

    pub fn write16(&mut self, addr: u32, value: u16) {
        self.write(addr, Width::Half, u32::from(value));
    }

    pub fn write32(&mut self, addr: u32, value: u32) {
        self.write(addr, Width::Word, value);
    }

    #[cfg(feature = "debug")]
    pub(crate) fn rom_len(&self) -> usize {
        self.rom.len()
    }

    #[cfg(feature = "debug")]
    pub fn debug_print(&self) -> &DebugPrint {
        &self.debug_print
    }

    #[cfg(feature = "debug")]
    pub fn debug_print_mut(&mut self) -> &mut DebugPrint {
        &mut self.debug_print
    }

    #[cfg(feature = "debug")]
    pub(crate) fn memory(&self, region: Region) -> Option<&[u8]> {
        match region {
            Region::Bios => Some(&self.bios[..]),
            Region::Ewram => Some(&self.ewram[..]),
            Region::Iwram => Some(&self.iwram[..]),
            Region::Palette => Some(&self.palette),
            Region::Vram => Some(&self.vram[..]),
            Region::Oam => Some(&self.oam),
            Region::Io | Region::Rom(_) | Region::Sram | Region::Unmapped => None,
        }
    }

    #[cfg(feature = "debug")]
    pub(crate) fn peek8(&self, addr: u32) -> u8 {
        match Region::of(addr) {
            region @ Region::Rom(_) => self.read_rom(region.offset(addr), Width::Byte) as u8,
            _ => self.read(addr, Width::Byte) as u8,
        }
    }

    #[cfg(feature = "debug")]
    pub(crate) fn poke8(&mut self, addr: u32, value: u8) {
        match Region::of(addr) {
            Region::Palette | Region::Vram | Region::Oam => {
                let aligned = addr & !1;
                let shift = 8 * (addr & 1);
                let old = self.read(aligned, Width::Half);
                let new = (old & !(0xFF << shift)) | u32::from(value) << shift;
                self.write(aligned, Width::Half, new);
            }
            Region::Ewram | Region::Iwram | Region::Io => {
                self.write(addr, Width::Byte, u32::from(value))
            }
            Region::Bios | Region::Rom(_) | Region::Sram | Region::Unmapped => {}
        }
    }

    pub(crate) fn read(&self, addr: u32, width: Width) -> u32 {
        self.read_in(Region::of(addr), addr, width)
    }

    fn read_in(&self, region: Region, addr: u32, width: Width) -> u32 {
        let offset = region.offset(addr);
        let aligned = offset & !(width.bytes() - 1);
        match region {
            Region::Bios => read_le(&self.bios, aligned, width),
            Region::Ewram => read_le(&self.ewram, aligned, width),
            Region::Iwram => read_le(&self.iwram, aligned, width),
            Region::Io => self.read_io(aligned, width),
            Region::Palette => read_le(&self.palette, aligned, width),
            Region::Vram if self.in_vram_hole(addr) => 0,
            Region::Vram => read_le(&self.vram, aligned, width),
            Region::Oam => read_le(&self.oam, aligned, width),
            Region::Rom(_) if self.backup.uses_eeprom() && self.in_eeprom_window(addr) => {
                u32::from(self.backup.read_eeprom())
            }
            Region::Rom(_) => self.read_rom(aligned, width),
            Region::Sram => u32::from(self.read_sram(offset)) * splat(width),
            Region::Unmapped => 0,
        }
    }

    pub(crate) fn write(&mut self, addr: u32, width: Width, value: u32) {
        #[cfg(feature = "debug")]
        if DebugPrint::covers(addr) {
            self.debug_print.write(addr, width, value);
            return;
        }
        let region = Region::of(addr);
        let offset = region.offset(addr);
        let aligned = offset & !(width.bytes() - 1);
        match (region, width) {
            (Region::Rom(_), _) if self.in_eeprom_window(addr) => {
                self.backup.write_eeprom(value as u16)
            }
            (Region::Rom(_), Width::Half | Width::Word) if Cart::in_gpio(aligned) => {
                self.catch_up_to_step_start();
                self.cart.write_gpio16(aligned, value as u16);
                if width == Width::Word {
                    self.cart.write_gpio16(aligned + 2, (value >> 16) as u16);
                }
            }
            (Region::Bios | Region::Rom(_) | Region::Unmapped, _) => {}
            (Region::Ewram, _) => {
                write_le(self.ewram.touch(aligned), aligned, width, value);
                #[cfg(jit)]
                self.code.written(region, aligned);
            }
            (Region::Iwram, _) => {
                write_le(self.iwram.touch(aligned), aligned, width, value);
                #[cfg(jit)]
                self.code.written(region, aligned);
            }
            (Region::Io, _) => self.write_io(aligned, width, value),
            (Region::Palette, Width::Byte) => {
                self.ppu.palette_written();
                write_le(&mut self.palette, aligned & !1, Width::Half, value * 0x0101)
            }
            (Region::Palette, _) => {
                self.ppu.palette_written();
                write_le(&mut self.palette, aligned, width, value)
            }
            (Region::Vram, _) if self.in_vram_hole(addr) => {}
            (Region::Vram, Width::Byte) => {
                if aligned < self.vram_bg_end() {
                    write_le(
                        self.vram.touch(aligned),
                        aligned & !1,
                        Width::Half,
                        value * 0x0101,
                    );
                }
            }
            (Region::Vram, _) => write_le(self.vram.touch(aligned), aligned, width, value),
            (Region::Oam, Width::Byte) => {}
            (Region::Oam, _) => {
                let ahead = self.clock.before_now_since_video();
                self.ppu
                    .oam_written(&self.oam, ahead, aligned, width.bytes());
                write_le(&mut self.oam, aligned, width, value)
            }
            (Region::Sram, _) => {
                let lane = offset & (width.bytes() - 1);
                let byte = (value >> (8 * lane)) as u8;
                if self.cart.in_tilt(offset) {
                    self.cart.write_tilt(offset, byte);
                } else {
                    self.backup.write8(offset, byte);
                }
            }
        }
    }

    fn read_rom(&self, aligned: usize, width: Width) -> u32 {
        if let Some(value) = self.plain_rom(aligned, width) {
            return value;
        }
        let gpio = self.cart.shows_gpio(aligned);
        (0..width.bytes()).fold(0, |value, i| {
            let offset = aligned + i;
            let gpio = gpio.then(|| self.cart.read_gpio8(offset)).flatten();
            let byte = gpio
                .or_else(|| self.rom.get(offset).copied())
                .unwrap_or_else(|| ((offset >> 1) as u16).to_le_bytes()[offset & 1]);
            value | u32::from(byte) << (8 * i)
        })
    }

    #[inline(always)]
    fn plain_rom(&self, aligned: usize, width: Width) -> Option<u32> {
        (Cart::in_plain_rom(aligned, self.plain_rom_end)
            || aligned + width.bytes() <= self.rom.len() && !self.cart.shows_gpio(aligned))
        .then(|| read_le(&self.rom, aligned, width))
    }

    fn read_io(&self, aligned: usize, width: Width) -> u32 {
        match width {
            Width::Byte => {
                let half = self.read_io16(aligned & !1);
                u32::from(half.to_le_bytes()[aligned & 1])
            }
            Width::Half => u32::from(self.read_io16(aligned)),
            Width::Word => {
                u32::from(self.read_io16(aligned)) | u32::from(self.read_io16(aligned + 2)) << 16
            }
        }
    }

    fn write_io(&mut self, aligned: usize, width: Width, value: u32) {
        match width {
            Width::Byte => {
                let shift = 8 * (aligned & 1);
                self.write_io16(aligned & !1, (value as u16) << shift, 0xFF << shift);
            }
            Width::Half => self.write_io16(aligned, value as u16, 0xFFFF),
            Width::Word => {
                self.write_io16(aligned, value as u16, 0xFFFF);
                self.write_io16(aligned + 2, (value >> 16) as u16, 0xFFFF);
            }
        }
    }

    fn read_io16(&self, offset: usize) -> u16 {
        match offset {
            DISPCNT => self.ppu.dispcnt(),
            DISPSTAT => self.ppu.dispstat(),
            VCOUNT => self.ppu.vcount(),
            BGCNT_FIRST..=BGCNT_LAST => self.ppu.bgcnt((offset - BGCNT_FIRST) / 2),
            BGOFS_FIRST..=BGOFS_LAST => 0,
            AFFINE_FIRST..=MOSAIC | BLEND_FIRST..=BLEND_LAST => self.ppu.read_register(offset),
            KEYINPUT => self.keypad.keyinput(),
            KEYCNT => self.keypad.keycnt(),
            IE => self.interrupts.ie(),
            IF => self.interrupts.if_at(self.clock.now()),
            WAITCNT => self.wait.read(),
            IME => self.interrupts.ime(),
            SOUND_FIRST..=SOUND_LAST => self.sound.read(offset - SOUND_FIRST),
            DMA_FIRST..=DMA_LAST => self.dma.read(offset - DMA_FIRST),
            TIMER_FIRST..=TIMER_LAST => self.timers.read(offset - TIMER_FIRST),
            SIO_FIRST..=SIO_LAST => self.serial.read(offset - SIO_FIRST),
            RCNT => self.serial.rcnt(),
            _ => u16::from_le_bytes([self.io[offset], self.io[offset + 1]]),
        }
    }

    fn write_io16(&mut self, offset: usize, value: u16, mask: u16) {
        match offset {
            DISPCNT => self.ppu.write_dispcnt(value, mask),
            DISPSTAT => self.ppu.write_dispstat(value, mask),
            BGCNT_FIRST..=BGCNT_LAST => {
                self.ppu
                    .write_bgcnt((offset - BGCNT_FIRST) / 2, value, mask)
            }
            BGOFS_FIRST..=BGOFS_LAST => {
                let index = (offset - BGOFS_FIRST) / 4;
                if offset & 2 == 0 {
                    self.ppu.write_bghofs(index, value, mask);
                } else {
                    self.ppu.write_bgvofs(index, value, mask);
                }
            }
            AFFINE_FIRST..=AFFINE_LAST | WINDOW_FIRST..=MOSAIC | BLEND_FIRST..=BLEND_LAST => {
                self.ppu.write_register(offset, value, mask)
            }
            VCOUNT | KEYINPUT => {}
            KEYCNT => {
                if self.keypad.write_keycnt(value, mask) {
                    self.interrupts.request(Irq::KEYPAD);
                }
            }
            IE => self.interrupts.write_ie(value, mask, self.clock.now()),
            IF => self.interrupts.acknowledge(value, mask, self.clock.now()),
            WAITCNT => {
                self.wait.write(value, mask);
                self.prefetch.retime(self.wait);
            }
            IME => self.interrupts.write_ime(value, mask, self.clock.now()),
            SOUND_FIRST..=SOUND_LAST => self.sound.write(offset - SOUND_FIRST, value, mask),
            DMA_FIRST..=DMA_LAST => {
                self.dma
                    .write(offset - DMA_FIRST, value, mask, self.clock.now());
                self.dma_check = 0;
            }
            TIMER_FIRST..=TIMER_LAST => {
                self.timers.write(offset - TIMER_FIRST, value, mask);
                self.dma_check = 0;
            }
            POSTFLG => {
                if mask & HALTCNT_MASK != 0 {
                    self.power = Power::from_haltcnt((value >> 8) as u8);
                    self.quiet = 0;
                }
                self.io[offset] = (self.io[offset] & !mask as u8) | (value & mask) as u8;
            }
            SIO_FIRST..=SIO_LAST => {
                self.serial
                    .write_at(offset - SIO_FIRST, value, mask, self.clock.since_serial())
            }
            RCNT => self.serial.write_rcnt(value, mask),
            _ => {
                let old = u16::from_le_bytes([self.io[offset], self.io[offset + 1]]);
                let new = (old & !mask) | (value & mask);
                self.io[offset..offset + 2].copy_from_slice(&new.to_le_bytes());
            }
        }
    }

    fn read_sram(&self, offset: usize) -> u8 {
        self.cart
            .in_tilt(offset)
            .then(|| self.cart.read_tilt(offset))
            .flatten()
            .unwrap_or_else(|| self.backup.read8(offset))
    }

    fn in_eeprom_window(&self, addr: u32) -> bool {
        addr >> 24 == EEPROM_REGION
            && (self.rom.len() <= EEPROM_SMALL_ROM
                || addr & EEPROM_LARGE_ROM_WINDOW == EEPROM_LARGE_ROM_WINDOW)
    }

    fn vram_bg_end(&self) -> usize {
        if self.ppu.bitmap_mode() {
            VRAM_BG_BITMAP
        } else {
            VRAM_BG_TILED
        }
    }
}

fn zeroed(size: usize) -> Box<[u8]> {
    vec![0; size].into_boxed_slice()
}

fn splat(width: Width) -> u32 {
    match width {
        Width::Byte => 0x01,
        Width::Half => 0x0101,
        Width::Word => 0x0101_0101,
    }
}

fn read_le(memory: &[u8], offset: usize, width: Width) -> u32 {
    match width {
        Width::Byte => u32::from(memory[offset]),
        Width::Half => u32::from(u16::from_le_bytes(
            memory[offset..offset + 2].try_into().unwrap(),
        )),
        Width::Word => u32::from_le_bytes(memory[offset..offset + 4].try_into().unwrap()),
    }
}

fn write_le(memory: &mut [u8], offset: usize, width: Width, value: u32) {
    match width {
        Width::Byte => memory[offset] = value as u8,
        Width::Half => memory[offset..offset + 2].copy_from_slice(&(value as u16).to_le_bytes()),
        Width::Word => memory[offset..offset + 4].copy_from_slice(&value.to_le_bytes()),
    }
}

mod cpu_access;
mod idle;
#[cfg(jit)]
mod jit;
mod video;

#[cfg(test)]
mod tests {
    use super::*;

    fn rom_counting(len: usize) -> Vec<u8> {
        (0..len).map(|i| i as u8).collect()
    }

    fn bus() -> Bus {
        Bus::new(&rom_counting(0x100))
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_state_whose_memory_sizes_differ_from_the_hardware_is_refused() {
        use crate::console::Console;
        use crate::gba::machine::Gba;
        let program = rom_counting(0x100);
        let wrong: [fn(&mut Bus); 10] = [
            |bus| bus.ewram = Paged::zeroed(EWRAM_SIZE * 2),
            |bus| bus.ewram = Paged::zeroed(0x1000),
            |bus| bus.iwram = Paged::zeroed(IWRAM_SIZE * 2),
            |bus| bus.iwram = Paged::zeroed(0),
            |bus| bus.vram = Paged::zeroed(0x1000),
            |bus| bus.io = zeroed(0x10),
            |bus| bus.palette = zeroed(PALETTE_SIZE * 2),
            |bus| bus.oam = zeroed(0),
            |bus| bus.vram = Paged::zeroed(VRAM_SIZE * 2),
            |bus| bus.io = zeroed(IO_SIZE * 2),
        ];
        for (index, wrong) in wrong.into_iter().enumerate() {
            let mut gba = Gba::new(&program);
            wrong(gba.bus_mut());
            let state = gba.save_state(0).unwrap();
            assert!(Gba::new(&program).restore(&state).is_err(), "case {index}");
        }
        let state = Gba::new(&program).save_state(0).unwrap();
        assert!(Gba::new(&program).restore(&state).is_ok());
    }

    #[test]
    fn bios_reads_the_loaded_image_and_ignores_writes() {
        let mut bus = bus();
        bus.load_bios(&[0x11, 0x22, 0x33, 0x44]);
        assert_eq!(bus.read32(0x0000_0000), 0x4433_2211);
        assert_eq!(bus.read16(0x0000_0002), 0x4433);
        assert_eq!(bus.read8(0x0000_0001), 0x22);
        bus.write32(0x0000_0000, 0xDEAD_BEEF);
        assert_eq!(bus.read32(0x0000_0000), 0x4433_2211);
        assert_eq!(bus.read32(0x0000_4000), 0);
    }

    #[test]
    fn ewram_is_256k_and_mirrors_through_the_region() {
        let mut bus = bus();
        bus.write32(0x0200_0000, 0x1122_3344);
        bus.write8(0x0203_FFFF, 0xAB);
        assert_eq!(bus.read32(0x0204_0000), 0x1122_3344);
        assert_eq!(bus.read32(0x02FC_0000), 0x1122_3344);
        assert_eq!(bus.read8(0x02FF_FFFF), 0xAB);
        assert_eq!(bus.read8(0x0200_0000), 0x44);
    }

    #[test]
    fn iwram_is_32k_and_mirrors_through_the_region() {
        let mut bus = bus();
        bus.write32(0x0300_7FFC, 0x0300_1234);
        assert_eq!(bus.read32(0x03FF_FFFC), 0x0300_1234);
        assert_eq!(bus.read32(0x0300_FFFC), 0x0300_1234);
        bus.write16(0x0300_8000, 0xBEEF);
        assert_eq!(bus.read16(0x0300_0000), 0xBEEF);
    }

    #[test]
    fn wide_accesses_align_down() {
        let mut bus = bus();
        bus.write32(0x0300_0002, 0x1122_3344);
        assert_eq!(bus.read32(0x0300_0000), 0x1122_3344);
        assert_eq!(bus.read32(0x0300_0003), 0x1122_3344);
        assert_eq!(bus.read16(0x0300_0001), 0x3344);
        bus.write16(0x0300_0001, 0xAAAA);
        assert_eq!(bus.read32(0x0300_0000), 0x1122_AAAA);
    }

    #[test]
    fn io_keeps_plain_registers_and_ends_at_3ff() {
        let mut bus = bus();
        bus.write16(0x0400_0000, 0x0403);
        assert_eq!(bus.read16(0x0400_0000), 0x0403);
        bus.write8(0x0400_0001, 0x1F);
        assert_eq!(bus.read16(0x0400_0000), 0x1F03);
        bus.write32(0x0400_0400, 0xFFFF_FFFF);
        assert_eq!(bus.read32(0x0400_0400), 0);
    }

    #[test]
    fn waitcnt_reads_and_writes_through_the_bus() {
        let mut bus = bus();
        bus.write16(0x0400_0204, 0x4317);
        assert_eq!(bus.read16(0x0400_0204), 0x4317);
        assert_eq!(bus.wait().read(), 0x4317);
        assert_eq!(
            bus.cycles(0x0800_0000, Width::Half, Access::NonSequential),
            4
        );
        bus.write8(0x0400_0205, 0x00);
        assert_eq!(bus.read16(0x0400_0204), 0x0017);
        bus.write32(0x0400_0204, 0xFFFF_FFFF);
        assert_eq!(bus.read8(0x0400_0205), 0x5F);
    }

    #[test]
    fn palette_is_1k_mirrored_and_byte_writes_fill_the_halfword() {
        let mut bus = bus();
        bus.write16(0x0500_0000, 0x7FFF);
        assert_eq!(bus.read16(0x0500_0400), 0x7FFF);
        assert_eq!(bus.read16(0x05FF_FC00), 0x7FFF);
        bus.write8(0x0500_0003, 0x12);
        assert_eq!(bus.read16(0x0500_0002), 0x1212);
        bus.write32(0x0500_03FC, 0xAABB_CCDD);
        assert_eq!(bus.read32(0x0500_07FC), 0xAABB_CCDD);
    }

    #[test]
    fn vram_is_96k_with_the_upper_32k_mirroring_the_obj_bank() {
        let mut bus = bus();
        bus.write16(0x0601_0000, 0x1234);
        assert_eq!(bus.read16(0x0601_8000), 0x1234);
        bus.write16(0x0601_FFFE, 0x5678);
        assert_eq!(bus.read16(0x0601_7FFE), 0x5678);
        bus.write32(0x0600_0000, 0xCAFE_F00D);
        assert_eq!(bus.read32(0x0602_0000), 0xCAFE_F00D);
        assert_eq!(bus.read32(0x06FE_0000), 0xCAFE_F00D);
    }

    #[test]
    fn vram_byte_writes_fill_bg_and_skip_obj_in_tiled_modes() {
        let mut bus = bus();
        bus.write8(0x0600_FFFF, 0x42);
        assert_eq!(bus.read16(0x0600_FFFE), 0x4242);
        bus.write8(0x0601_0000, 0x42);
        assert_eq!(bus.read16(0x0601_0000), 0);
    }

    #[test]
    fn vram_byte_writes_reach_further_in_bitmap_modes() {
        let mut bus = bus();
        bus.write16(0x0400_0000, 0x0003);
        bus.write8(0x0601_3FFE, 0x42);
        assert_eq!(bus.read16(0x0601_3FFE), 0x4242);
        bus.write8(0x0601_4000, 0x42);
        assert_eq!(bus.read16(0x0601_4000), 0);
    }

    #[test]
    fn oam_is_1k_mirrored_and_ignores_byte_writes() {
        let mut bus = bus();
        bus.write32(0x0700_0000, 0x0102_0304);
        assert_eq!(bus.read32(0x0700_0400), 0x0102_0304);
        assert_eq!(bus.read32(0x07FF_FC00), 0x0102_0304);
        bus.write8(0x0700_0000, 0xFF);
        assert_eq!(bus.read32(0x0700_0000), 0x0102_0304);
        bus.write16(0x0700_0002, 0xFFFF);
        assert_eq!(bus.read32(0x0700_0000), 0xFFFF_0304);
    }

    #[test]
    fn rom_reads_little_endian_in_all_three_windows() {
        let bus = bus();
        for base in [0x0800_0000, 0x0A00_0000, 0x0C00_0000] {
            assert_eq!(bus.read32(base + 4), 0x0706_0504, "{base:08X}");
            assert_eq!(bus.read16(base + 6), 0x0706, "{base:08X}");
            assert_eq!(bus.read8(base + 5), 0x05, "{base:08X}");
        }
        assert_eq!(bus.read8(0x0900_0005), 0);
    }

    #[test]
    fn rom_mirrors_every_32m() {
        let bus = Bus::new(&rom_counting(0x200_0000 + 0x10));
        assert_eq!(bus.read8(0x0900_0001), 0x01);
        assert_eq!(bus.read8(0x0B00_0001), 0x01);
        assert_eq!(bus.read8(0x0D00_00FF), 0xFF);
    }

    #[test]
    fn rom_past_its_end_reads_the_address_halves() {
        let bus = bus();
        assert_eq!(bus.read16(0x0800_0100), 0x0080);
        assert_eq!(bus.read16(0x0800_0102), 0x0081);
        assert_eq!(bus.read32(0x0800_0100), 0x0081_0080);
        assert_eq!(bus.read8(0x0800_0103), 0x00);
        assert_eq!(bus.read16(0x0A01_FFFE), 0xFFFF);
        assert_eq!(bus.read16(0x0802_0000), 0x0000);
    }

    #[test]
    fn rom_ignores_writes() {
        let mut bus = bus();
        bus.write32(0x0800_0000, 0xFFFF_FFFF);
        assert_eq!(bus.read32(0x0800_0000), 0x0302_0100);
    }

    #[test]
    fn sram_is_64k_on_an_8_bit_bus() {
        let mut bus = bus();
        assert_eq!(bus.read8(0x0E00_0000), 0xFF);
        bus.write8(0x0E00_0001, 0x5A);
        assert_eq!(bus.read8(0x0E01_0001), 0x5A);
        assert_eq!(bus.read8(0x0F00_0001), 0x5A);
        assert_eq!(bus.read16(0x0E00_0001), 0x5A5A);
        assert_eq!(bus.read32(0x0E00_0001), 0x5A5A_5A5A);
    }

    #[test]
    fn sram_wide_writes_store_the_addressed_byte_lane() {
        let mut bus = bus();
        bus.write16(0x0E00_0011, 0xAABB);
        assert_eq!(bus.read8(0x0E00_0011), 0xAA);
        assert_eq!(bus.read8(0x0E00_0010), 0xFF);
        bus.write32(0x0E00_0022, 0x1122_3344);
        assert_eq!(bus.read8(0x0E00_0022), 0x22);
        assert_eq!(bus.read8(0x0E00_0020), 0xFF);
    }

    #[test]
    fn unmapped_reads_zero_and_ignores_writes() {
        let mut bus = bus();
        for addr in [0x0000_4000, 0x0100_0000, 0x1000_0000, 0xFFFF_FFFC] {
            bus.write32(addr, 0xFFFF_FFFF);
            assert_eq!(bus.read32(addr), 0, "{addr:08X}");
        }
    }

    #[test]
    fn rom_is_capped_at_32m() {
        let bus = Bus::new(&vec![0xAA; 0x200_0000 + 4]);
        assert_eq!(bus.rom.len(), 0x200_0000);
    }

    #[test]
    fn keyinput_reads_through_the_bus_and_ignores_writes() {
        let mut bus = bus();
        assert_eq!(bus.read16(0x0400_0130), 0x03FF);
        bus.set_keys(Keys::A | Keys::L);
        assert_eq!(bus.read16(0x0400_0130), 0x01FE);
        assert_eq!(bus.read8(0x0400_0131), 0x01);
        bus.write16(0x0400_0130, 0x0000);
        assert_eq!(bus.read16(0x0400_0130), 0x01FE);
    }

    #[test]
    fn keycnt_shares_a_word_with_keyinput() {
        let mut bus = bus();
        bus.write32(0x0400_0130, 0xC00F_0000);
        assert_eq!(bus.read32(0x0400_0130), 0xC00F_03FF);
        assert_eq!(bus.keypad().keycnt(), 0xC00F);
    }

    #[test]
    fn keypad_irq_sets_if_bit_12_in_any_of_mode() {
        let mut bus = bus();
        bus.write16(0x0400_0132, 0x4000 | Keys::START.bits());
        bus.set_keys(Keys::A);
        assert_eq!(bus.read16(0x0400_0202), 0);
        bus.set_keys(Keys::START);
        assert_eq!(bus.read16(0x0400_0202), 0);
        bus.step_video(1);
        assert_eq!(bus.read16(0x0400_0202), Irq::KEYPAD.bits());
    }

    #[test]
    fn keypad_irq_in_all_of_mode_waits_for_every_key() {
        let mut bus = bus();
        let combo = Keys::A | Keys::B | Keys::SELECT | Keys::START;
        bus.write16(0x0400_0132, 0xC000 | combo.bits());
        bus.set_keys(Keys::A | Keys::B | Keys::START);
        assert_eq!(bus.interrupts().if_(), 0);
        bus.set_keys(combo);
        bus.step_video(1);
        assert_eq!(bus.interrupts().if_(), Irq::KEYPAD.bits());
    }

    #[test]
    fn keycnt_write_with_the_keys_held_raises_the_irq() {
        let mut bus = bus();
        bus.set_keys(Keys::R);
        bus.write16(0x0400_0132, Keys::R.bits());
        assert_eq!(bus.interrupts().if_(), 0);
        bus.write8(0x0400_0133, 0x41);
        assert_eq!(bus.keypad().keycnt(), 0x4100);
        assert_eq!(bus.interrupts().if_(), 0);
        bus.step_video(1);
        assert_eq!(bus.interrupts().if_(), Irq::KEYPAD.bits());
    }

    #[test]
    fn keypad_irq_is_acknowledged_by_writing_one_to_if() {
        let mut bus = bus();
        bus.write16(0x0400_0200, Irq::KEYPAD.bits());
        bus.write16(0x0400_0208, 1);
        bus.write16(0x0400_0132, 0x4000 | Keys::A.bits());
        bus.set_keys(Keys::A);
        assert!(bus.interrupts().asserted());
        bus.write16(0x0400_0202, Irq::KEYPAD.bits());
        assert_eq!(bus.read16(0x0400_0202), 0);
        assert!(!bus.interrupts().asserted());
    }

    #[test]
    fn ie_if_and_ime_read_back_through_the_bus() {
        let mut bus = bus();
        bus.write32(0x0400_0200, 0xFFFF_FFFF);
        assert_eq!(bus.read32(0x0400_0200), 0x0000_3FFF);
        bus.write32(0x0400_0208, 0xFFFF_FFFF);
        assert_eq!(bus.read16(0x0400_0208), 1);
    }

    const TM0CNT: u32 = 0x0400_0100;
    const DMA_BASE: u32 = 0x0400_00B0;
    const IF_ADDR: u32 = 0x0400_0202;

    fn dma_registers(
        bus: &mut Bus,
        channel: u32,
        source: u32,
        dest: u32,
        count: u16,
        control: u16,
    ) {
        let base = DMA_BASE + 12 * channel;
        bus.write32(base, source);
        bus.write32(base + 4, dest);
        bus.write16(base + 8, count);
        bus.write16(base + 10, control | 0x8000);
    }

    #[test]
    fn a_32_bit_timer_write_starts_from_the_new_reload() {
        let mut bus = bus();
        bus.write32(TM0CNT, 0x00C0_FFFE);
        bus.step(1);
        assert_eq!(bus.read16(TM0CNT), 0);
        assert_eq!(bus.read16(TM0CNT + 2), 0x00C0);
        bus.step(1);
        assert_eq!(bus.read16(TM0CNT), 0xFFFE);
        bus.step(1);
        assert_eq!(bus.read16(TM0CNT), 0xFFFF);
        assert_eq!(bus.read16(IF_ADDR), 0);
        bus.step(1);
        assert_eq!(bus.read16(TM0CNT), 0xFFFE);
        bus.step_video(1);
        assert_eq!(bus.read16(IF_ADDR), 0);
        bus.step(1);
        bus.step_video(1);
        assert_eq!(bus.read16(IF_ADDR), Irq::TIMER0.bits());
    }

    #[test]
    fn timer_registers_take_byte_writes() {
        let mut bus = bus();
        bus.write8(TM0CNT + 4, 0x34);
        bus.write8(TM0CNT + 5, 0x12);
        bus.write8(TM0CNT + 6, 0x80);
        bus.step(2);
        assert_eq!(bus.read16(TM0CNT + 4), 0x1234);
        assert_eq!(bus.read8(TM0CNT + 6), 0x80);
    }

    #[test]
    fn step_without_timers_requests_nothing() {
        let mut bus = bus();
        bus.write16(TM0CNT, 0xFFFF);
        bus.write16(TM0CNT + 2, 0x0040);
        assert_eq!(bus.timers_step(100_000), Overflows::default());
        assert_eq!(bus.read16(IF_ADDR), 0);
    }

    #[test]
    fn immediate_dma_copies_and_reports_its_cycles() {
        let mut bus = bus();
        for i in 0..4 {
            bus.write32(0x0200_0000 + 4 * i, 0x1111_1111 * (i + 1));
        }
        dma_registers(&mut bus, 3, 0x0200_0000, 0x0300_0000, 4, 0x4400);
        assert!(bus.dma_active());
        assert_eq!(bus.run_dma(), 2 + 2 + 4 * 6 + 4);
        for i in 0..4 {
            assert_eq!(bus.read32(0x0300_0000 + 4 * i), 0x1111_1111 * (i + 1));
        }
        assert!(!bus.dma_active());
        assert_eq!(bus.read16(DMA_BASE + 12 * 3 + 10) & 0x8000, 0);
        assert_eq!(bus.interrupts().raised(), Irq::DMA3.bits());
    }

    #[test]
    fn a_dma_irq_shows_in_if_the_cycle_after_the_dma_gives_the_bus_back() {
        let mut bus = bus();
        dma_registers(&mut bus, 3, 0x0200_0000, 0x0300_0000, 4, 0x4400);
        let cycles = bus.run_dma();
        assert_eq!(bus.interrupts().if_at(cycles), 0);
        assert_eq!(bus.interrupts().if_at(cycles + 1), Irq::DMA3.bits());
    }

    #[test]
    fn vblank_dma_runs_only_after_the_vblank_call() {
        let mut bus = bus();
        bus.write16(0x0200_0000, 0xBEEF);
        dma_registers(&mut bus, 1, 0x0200_0000, 0x0300_0000, 1, 0x1000);
        assert_eq!(bus.run_dma(), 0);
        bus.start_dma(Timing::HBlank);
        assert_eq!(bus.run_dma(), 0);
        bus.start_dma(Timing::VBlank);
        assert!(bus.run_dma() > 0);
        assert_eq!(bus.read16(0x0300_0000), 0xBEEF);
    }

    #[test]
    fn channels_started_together_run_lower_first() {
        let mut bus = bus();
        bus.write16(0x0200_0000, 0xAAAA);
        bus.write16(0x0300_0000, 0x5555);
        dma_registers(&mut bus, 2, 0x0300_0000, 0x0300_0010, 1, 0x2000);
        dma_registers(&mut bus, 1, 0x0200_0000, 0x0300_0000, 1, 0x2000);
        bus.start_dma(Timing::HBlank);
        bus.run_dma();
        assert_eq!(bus.read16(0x0300_0010), 0xAAAA);
    }

    #[test]
    fn dma0_masks_rom_down_to_the_bios_and_gets_its_latch() {
        let mut bus = bus();
        bus.load_bios(&[0x11, 0x22]);
        bus.write16(0x0300_0000, 0xFFFF);
        dma_registers(&mut bus, 0, 0x0800_0000, 0x0300_0000, 1, 0);
        dma_registers(&mut bus, 3, 0x0800_0000, 0x0300_0002, 1, 0);
        bus.run_dma();
        assert_eq!(bus.read16(0x0300_0000), 0);
        assert_eq!(bus.read16(0x0300_0002), 0x0100);
    }

    #[test]
    fn dma_from_the_bios_repeats_the_last_word_it_moved() {
        let mut bus = bus();
        bus.load_bios(&[0x11, 0x22, 0x33, 0x44]);
        bus.write32(0x0300_0100, 0xAABB_CCDD);
        dma_registers(&mut bus, 1, 0x0300_0100, 0x0300_0000, 1, 0x0400);
        bus.run_dma();
        dma_registers(&mut bus, 1, 0x0000_0000, 0x0300_0010, 2, 0x0400);
        bus.run_dma();
        assert_eq!(bus.read32(0x0300_0010), 0xAABB_CCDD);
        assert_eq!(bus.read32(0x0300_0014), 0xAABB_CCDD);
    }

    #[test]
    fn halfword_dma_from_unmapped_memory_repeats_the_last_half_on_both_lanes() {
        let mut bus = bus();
        bus.write16(0x0300_0100, 0xCCDD);
        dma_registers(&mut bus, 3, 0x0300_0100, 0x0300_0000, 1, 0);
        bus.run_dma();
        dma_registers(&mut bus, 3, 0x0100_0000, 0x0300_0020, 2, 0);
        bus.run_dma();
        assert_eq!(bus.read32(0x0300_0020), 0xCCDD_CCDD);
    }

    #[test]
    fn each_dma_channel_keeps_its_own_latch() {
        let mut bus = bus();
        bus.write32(0x0300_0100, 0x1111_1111);
        bus.write32(0x0300_0104, 0x2222_2222);
        dma_registers(&mut bus, 1, 0x0300_0100, 0x0300_0000, 1, 0x0400);
        dma_registers(&mut bus, 2, 0x0300_0104, 0x0300_0000, 1, 0x0400);
        bus.run_dma();
        dma_registers(&mut bus, 1, 0x0000_0000, 0x0300_0010, 1, 0x0400);
        bus.run_dma();
        assert_eq!(bus.read32(0x0300_0010), 0x1111_1111);
    }

    #[test]
    fn dma3_writes_to_rom_addresses_change_nothing() {
        let mut bus = bus();
        bus.write16(0x0300_0000, 0xFFFF);
        dma_registers(&mut bus, 3, 0x0300_0000, 0x0800_0000, 1, 0);
        bus.run_dma();
        assert_eq!(bus.read16(0x0800_0000), 0x0100);
    }

    #[test]
    fn special_dma_starts_from_its_hook() {
        let mut bus = bus();
        bus.write32(0x0200_0000, 0x1234_5678);
        dma_registers(&mut bus, 1, 0x0200_0000, 0x0400_00A0, 0, 0x3200);
        bus.start_special_dma(1);
        assert_eq!(bus.run_dma(), 2 + 6 + 3 * 6 + 4);
        assert_eq!(bus.read32(0x0400_00A0), 0);
        assert_ne!(bus.read16(DMA_BASE + 12 + 10) & 0x8000, 0);
    }

    #[test]
    fn an_hblank_dma_takes_over_a_running_dma_between_units() {
        let mut bus = bus();
        let units = 2000;
        for i in 0..units {
            bus.write16(0x0200_0000 + 2 * i, 0xAAAA);
        }
        let last = 0x0300_0000 + 2 * (units - 1);
        dma_registers(&mut bus, 0, last, 0x0300_4000, 1, 0x2000);
        dma_registers(&mut bus, 3, 0x0200_0000, 0x0300_0000, units as u16, 0);
        bus.run_dma();
        assert_eq!(bus.read16(0x0300_4000), 0);
        assert_eq!(bus.read16(last), 0xAAAA);
        assert_eq!(bus.read16(DMA_BASE + 12 * 3 + 10) & 0x8000, 0);
    }

    fn bus_with_hblank_marker_dma() -> Bus {
        let mut bus = bus();
        bus.write32(0x0200_F000, 0xBEEF_CAFE);
        dma_registers(&mut bus, 0, 0x0200_F000, 0x0300_7000, 1, 0x2400);
        bus
    }

    #[test]
    fn an_hblank_dma_due_when_a_dma_ends_runs_before_the_next_cpu_access() {
        for lead in 1..=3 {
            let mut bus = bus_with_hblank_marker_dma();
            for _ in 0..lead {
                bus.cpu_idle();
            }
            dma_registers(&mut bus, 3, 0x0800_0000, 0x0900_0000, 166, 0);
            let mut addr = 0x0800_0000;
            let mut access = Access::NonSequential;
            while bus.dma_cycles() == 0 {
                bus.code_cycles(addr, Width::Half, access);
                addr += 2;
                access = Access::Sequential;
            }
            assert_eq!(
                bus.read32(0x0300_7000),
                0xBEEF_CAFE,
                "{lead} idle cycles first"
            );
        }
    }

    #[test]
    fn the_first_cpu_access_after_a_dma_is_nonsequential() {
        let mut bus = bus();
        dma_registers(&mut bus, 3, 0x0200_0000, 0x0300_0000, 1, 0);
        let wait = WaitControl::default();
        let n = wait.cycles(0x0800_0002, Width::Half, Access::NonSequential);
        let s = wait.cycles(0x0800_0002, Width::Half, Access::Sequential);
        assert_ne!(n, s);
        bus.code_cycles(0x0800_0000, Width::Half, Access::NonSequential);
        assert_eq!(bus.dma_cycles(), 0);
        assert_eq!(
            bus.code_cycles(0x0800_0002, Width::Half, Access::Sequential),
            n
        );
        assert_ne!(bus.dma_cycles(), 0);
        assert_eq!(
            bus.code_cycles(0x0800_0004, Width::Half, Access::Sequential),
            s
        );
    }

    #[test]
    fn siocnt_reads_the_busy_bit_at_the_access_cycle() {
        let siocnt = 0x0400_0128;
        let cycles = 8 * 8 + 6;
        for (idle, busy) in [(cycles - 1, 0x80), (cycles, 0)] {
            let mut bus = bus();
            bus.start_cpu_step();
            bus.cpu_write(siocnt, Width::Half, 0x0083);
            for _ in 0..idle {
                bus.cpu_idle();
            }
            assert_eq!(bus.cpu_read(siocnt, Width::Half) & 0x80, busy);
        }
    }

    fn prefetching_bus_with_dma(source: u32) -> Bus {
        let mut bus = bus();
        bus.write16(0x0400_0204, 0x4014);
        dma_registers(&mut bus, 3, source, 0x0300_0000, 4, 0x0400);
        bus.code_cycles(0x0800_0000, Width::Half, Access::NonSequential);
        assert_eq!(bus.dma_cycles(), 0);
        bus
    }

    #[test]
    fn the_prefetch_buffer_keeps_filling_while_a_dma_runs_off_the_game_pak() {
        let mut bus = prefetching_bus_with_dma(0x0200_0000);
        assert_eq!(
            bus.code_cycles(0x0800_0002, Width::Half, Access::Sequential),
            1
        );
        assert_ne!(bus.dma_cycles(), 0);
    }

    #[test]
    fn a_dma_from_the_game_pak_drops_the_prefetch_buffer() {
        let mut bus = prefetching_bus_with_dma(0x0800_0080);
        let n = bus
            .wait
            .cycles(0x0800_0002, Width::Half, Access::NonSequential);
        assert_eq!(
            bus.code_cycles(0x0800_0002, Width::Half, Access::Sequential),
            n
        );
        assert_ne!(bus.dma_cycles(), 0);
    }

    #[test]
    fn a_held_instruction_skips_the_cycles_it_ran_before_the_halt() {
        let mut bus = bus();
        bus.start_cpu_step();
        bus.resume_held(1, 2);
        assert_eq!(
            bus.code_cycles(0x0300_0000, Width::Word, Access::Sequential),
            0
        );
        assert_eq!(
            bus.data_cycles(0x0300_0100, Width::Word, Access::NonSequential),
            0
        );
        assert_eq!(
            bus.code_cycles(0x0300_0004, Width::Word, Access::Sequential),
            1
        );
        assert_eq!(bus.step_cycles(), 2);
        assert_eq!(bus.end_held(), 0);
    }

    struct Xorshift(u32);

    impl Xorshift {
        fn below(&mut self, n: u32) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0 % n
        }
    }

    fn assert_same_bus(fast: &Bus, slow: &Bus, seed: u32, step: u32) {
        assert_eq!(fast.prefetch, slow.prefetch, "seed {seed} step {step}");
        assert_eq!(fast.open_bus, slow.open_bus, "seed {seed} step {step}");
        assert_eq!(
            format!("{:?}", fast.clock),
            format!("{:?}", slow.clock),
            "seed {seed} step {step}"
        );
    }

    #[test]
    fn timed_fetches_and_reads_match_paying_the_cycles_first() {
        use crate::gba::cpu::CpuBus;
        let rom = rom_counting(0x4_0000);
        let targets = [
            0x0800_0000,
            0x0801_FFE0,
            0x0803_FFE0,
            0x0A01_FFF0,
            0x0C00_0100,
            0x0D00_0000,
            0x0300_0000,
            0x0200_0000,
        ];
        for seed in 1..=64u32 {
            let mut rng = Xorshift(seed.wrapping_mul(0x9E37_79B9));
            let mut fast = Bus::new(&rom);
            let mut slow = Bus::new(&rom);
            let mut pc = 0x0800_0000;
            let mut thumb = true;
            let mut access = Access::NonSequential;
            for bus in [&mut fast, &mut slow] {
                bus.write16(0x0400_0204, 0x4317);
            }
            for step in 0..4000 {
                match rng.below(24) {
                    0 => {
                        pc = targets[rng.below(8) as usize] + 4 * rng.below(8);
                        access = Access::NonSequential;
                        fast.branch();
                        slow.branch();
                    }
                    1 => {
                        thumb = !thumb;
                        pc &= !3;
                        access = Access::NonSequential;
                        fast.branch();
                        slow.branch();
                    }
                    2 => {
                        let idles = 1 + rng.below(4);
                        for bus in [&mut fast, &mut slow] {
                            (0..idles).for_each(|_| bus.cpu_idle());
                        }
                    }
                    3 | 4 => {
                        let base = [
                            0x0200_0000,
                            0x0300_0000,
                            0x0800_0000,
                            0x0A00_0000,
                            0x0E00_0000,
                        ][rng.below(5) as usize];
                        let addr = base + 4 * rng.below(0x100);
                        let width = [Width::Byte, Width::Half, Width::Word][rng.below(3) as usize];
                        let data = if rng.below(2) == 0 {
                            Access::Sequential
                        } else {
                            Access::NonSequential
                        };
                        let fast_read = CpuBus::timed_read(&mut fast, addr, width, data);
                        let slow_read = (
                            slow.data_cycles(addr, width, data),
                            slow.cpu_read(addr, width),
                        );
                        assert_eq!(fast_read, slow_read, "seed {seed} step {step}");
                    }
                    5 => {
                        let prefetch = if rng.below(4) == 0 { 0 } else { 0x4000 };
                        let bits = rng.below(0x4000) as u16 | prefetch;
                        for bus in [&mut fast, &mut slow] {
                            bus.write16(0x0400_0204, bits);
                        }
                    }
                    6 => {
                        let source = [0x0200_0000, 0x0800_0080][rng.below(2) as usize];
                        for bus in [&mut fast, &mut slow] {
                            dma_registers(bus, 3, source, 0x0300_1000, 4, 0x0400);
                        }
                    }
                    7 => {
                        fast.start_cpu_step();
                        slow.start_cpu_step();
                    }
                    _ => {
                        let width = if thumb { Width::Half } else { Width::Word };
                        let aligned = pc & !(width.bytes() as u32 - 1);
                        let fetched = CpuBus::timed_fetch(&mut fast, pc, width, access);
                        let cycles = slow.code_cycles(pc, width, access);
                        let expected = (cycles, slow.fetch_code(aligned, width));
                        assert_eq!(fetched, expected, "seed {seed} step {step} at {pc:08X}");
                        pc = pc.wrapping_add(width.bytes() as u32);
                        access = Access::Sequential;
                    }
                }
                assert_same_bus(&fast, &slow, seed, step);
            }
        }
    }

    #[test]
    fn iwram_runs_match_fetching_one_at_a_time() {
        let rom = rom_counting(0x4_0000);
        let (mut flat, mut idled) = (0, 0);
        for seed in 1..=64u32 {
            let mut rng = Xorshift(seed.wrapping_mul(0x85EB_CA6B));
            let mut fast = Bus::new(&rom);
            let mut slow = Bus::new(&rom);
            for bus in [&mut fast, &mut slow] {
                bus.write16(0x0400_0204, 0x4317);
                for word in 0..0x40u32 {
                    bus.cpu_write(
                        0x0300_0000 + 4 * word,
                        Width::Word,
                        word.wrapping_mul(0x9E37_79B9),
                    );
                }
            }
            for step in 0..2000 {
                match rng.below(8) {
                    0 => {
                        let idles = 1 + rng.below(4);
                        for bus in [&mut fast, &mut slow] {
                            (0..idles).for_each(|_| bus.cpu_idle());
                        }
                    }
                    1 => {
                        let prefetch = if rng.below(3) == 0 { 0 } else { 0x4000 };
                        let bits = rng.below(0x4000) as u16 | prefetch;
                        for bus in [&mut fast, &mut slow] {
                            bus.write16(0x0400_0204, bits);
                        }
                    }
                    2 => {
                        let source = [0x0200_0000, 0x0800_0080][rng.below(2) as usize];
                        for bus in [&mut fast, &mut slow] {
                            dma_registers(bus, 3, source, 0x0300_1000, 4, 0x0400);
                        }
                    }
                    3 => {
                        fast.start_cpu_step();
                        slow.start_cpu_step();
                    }
                    4 => {
                        fast.branch();
                        slow.branch();
                    }
                    _ => {
                        let width = if rng.below(2) == 0 {
                            Width::Half
                        } else {
                            Width::Word
                        };
                        let step_bytes = width.bytes() as u32;
                        let base = [0x0300_0000, 0x0300_7FF0, 0x03FF_FFF8, 0x02FF_FFF8]
                            [rng.below(4) as usize];
                        let addr = base + step_bytes * rng.below(8);
                        let count = 2 + rng.below(8);
                        let idles: Vec<u32> = (0..count)
                            .map(|index| {
                                if index + 1 < count && rng.below(3) == 0 {
                                    1 + rng.below(4)
                                } else {
                                    0
                                }
                            })
                            .collect();
                        let total = count + idles.iter().sum::<u32>();
                        let room = 1 + rng.below(24);
                        let ran = fast.iwram_run(addr, width, count, total, room);
                        flat += u32::from(ran);
                        idled += u32::from(ran && total > count);
                        let mut access = Access::NonSequential;
                        for (index, &idle) in (0..count).zip(&idles) {
                            let at = addr.wrapping_add(index * step_bytes);
                            let (cycles, value) = slow.timed_code(at, width, access);
                            if ran {
                                assert_eq!(cycles, 1, "seed {seed} step {step} at {at:08X}");
                            } else {
                                let fetched = fast.timed_code(at, width, access);
                                assert_eq!(fetched, (cycles, value), "seed {seed} step {step}");
                                (0..idle).for_each(|_| fast.cpu_idle());
                            }
                            (0..idle).for_each(|_| slow.cpu_idle());
                            access = Access::Sequential;
                        }
                    }
                }
                assert_same_bus(&fast, &slow, seed, step);
            }
        }
        assert!(flat > 1000, "only {flat} flat runs");
        assert!(idled > 200, "only {idled} flat runs with idles");
    }
}
