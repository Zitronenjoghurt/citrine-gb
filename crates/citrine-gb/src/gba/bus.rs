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
use crate::console::Keys;
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
    bios: Box<[u8]>,
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::bytes"))]
    ewram: Box<[u8]>,
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::bytes"))]
    iwram: Box<[u8]>,
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::bytes"))]
    io: Box<[u8]>,
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::bytes"))]
    palette: Box<[u8]>,
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::bytes"))]
    vram: Box<[u8]>,
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::bytes"))]
    oam: Box<[u8]>,
    #[cfg_attr(feature = "serde", serde(skip))]
    rom: Arc<[u8]>,
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
}

impl Bus {
    pub fn new(rom: &[u8]) -> Self {
        Self {
            bios: zeroed(BIOS_SIZE),
            ewram: zeroed(EWRAM_SIZE),
            iwram: zeroed(IWRAM_SIZE),
            io: zeroed(IO_SIZE),
            palette: zeroed(PALETTE_SIZE),
            vram: zeroed(VRAM_SIZE),
            oam: zeroed(OAM_SIZE),
            rom: rom[..rom.len().min(ROM_MAX_SIZE)].into(),
            backup: Backup::for_rom(rom),
            cart: Cart::for_rom(rom),
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
        }
    }

    pub fn load_bios(&mut self, bios: &[u8]) {
        let len = bios.len().min(BIOS_SIZE);
        self.bios[..len].copy_from_slice(&bios[..len]);
        self.bios[len..].fill(0);
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
        } = self;
        ewram.copy_from_slice(&source.ewram);
        iwram.copy_from_slice(&source.iwram);
        io.copy_from_slice(&source.io);
        palette.copy_from_slice(&source.palette);
        vram.copy_from_slice(&source.vram);
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
        serial.clone_from(&source.serial);
        sound.copy_state_from(&source.sound);
        *clock = source.clock;
        *dma_check = 0;
        *after_dma = source.after_dma;
        *prepaid = 0;
    }

    pub fn adopt_rom(&mut self, source: &Bus) {
        if !Arc::ptr_eq(&self.rom, &source.rom) {
            self.rom = Arc::clone(&source.rom);
        }
        if self.bios != source.bios {
            self.bios.clone_from(&source.bios);
        }
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

    pub fn sound(&self) -> &Sound {
        &self.sound
    }

    pub fn sound_mut(&mut self) -> &mut Sound {
        &mut self.sound
    }

    pub fn step(&mut self, cycles: u32) {
        self.stall_rest_of_dma();
        self.cart.tick(cycles);
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
        (&self.vram, &self.palette, &self.oam)
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
            Region::Bios => Some(&self.bios),
            Region::Ewram => Some(&self.ewram),
            Region::Iwram => Some(&self.iwram),
            Region::Palette => Some(&self.palette),
            Region::Vram => Some(&self.vram),
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
        let region = Region::of(addr);
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
                self.cart.write_gpio16(aligned, value as u16);
                if width == Width::Word {
                    self.cart.write_gpio16(aligned + 2, (value >> 16) as u16);
                }
            }
            (Region::Bios | Region::Rom(_) | Region::Unmapped, _) => {}
            (Region::Ewram, _) => write_le(&mut self.ewram, aligned, width, value),
            (Region::Iwram, _) => write_le(&mut self.iwram, aligned, width, value),
            (Region::Io, _) => self.write_io(aligned, width, value),
            (Region::Palette, Width::Byte) => {
                write_le(&mut self.palette, aligned & !1, Width::Half, value * 0x0101)
            }
            (Region::Palette, _) => write_le(&mut self.palette, aligned, width, value),
            (Region::Vram, _) if self.in_vram_hole(addr) => {}
            (Region::Vram, Width::Byte) => {
                if aligned < self.vram_bg_end() {
                    write_le(&mut self.vram, aligned & !1, Width::Half, value * 0x0101);
                }
            }
            (Region::Vram, _) => write_le(&mut self.vram, aligned, width, value),
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
            WAITCNT => self.wait.write(value, mask),
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
    memory[offset..offset + width.bytes()]
        .iter()
        .rev()
        .fold(0, |value, &byte| value << 8 | u32::from(byte))
}

fn write_le(memory: &mut [u8], offset: usize, width: Width, value: u32) {
    let bytes = value.to_le_bytes();
    memory[offset..offset + width.bytes()].copy_from_slice(&bytes[..width.bytes()]);
}

mod cpu_access;
mod video;

#[cfg(test)]
mod tests;
