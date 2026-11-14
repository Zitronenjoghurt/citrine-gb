mod io;

use super::{ARM9_BIOS_WINDOW, Arm9Memory, MAIN_RAM_SIZE, Shared, Waits, read_bios};
use crate::gba::cpu::{CoprocessorRegister, CpuBus, read_words, write_words};
use crate::gba::open_bus::lane;
use crate::gba::wait::{Access, Width};
#[cfg(jit)]
use crate::jit::{
    Cache, CodeRam, CodeSpan, Cp15Cells, Fast, FastBus, JitBus, PAGE_BITS, PAGE_SIZE, Page, Pages,
    Spec, Stamp, UNMARKED_PAGES, WINDOWS, Window,
};
use crate::nds::Cpu;
#[cfg(jit)]
use crate::nds::cp15::CacheMap;
use crate::nds::cp15::{Cp15, Cp15Write, Fetch};
use crate::nds::dma::{Area, DmaBus, Run, Start, Timing};
#[cfg(jit)]
use crate::nds::tcm::{DTCM_SIZE, ITCM_SIZE};
use crate::nds::vram::Purpose;

const BIOS_BASE: u32 = 0xFFFF_0000;
#[cfg(jit)]
const MAIN_RAM_BASE: u32 = 0x0200_0000;
#[cfg(jit)]
const MAIN_RAM_WINDOW: u64 = 0x0100_0000;
const PALETTE_OAM_MASK: usize = 0x7FF;
const VRAM_ENGINE_MASK: usize = 0x1F_FFFF;
const VRAM_LCDC_MASK: usize = 0xF_FFFF;
const FAST_CYCLES: u32 = 1;
const CACHE_LOOKUP_CYCLES: u32 = 3;
const CACHE_LINE_MASK: u32 = 0x1F;
#[cfg(jit)]
const _: () = assert!(
    MAIN_RAM_SIZE >> PAGE_BITS <= UNMARKED_PAGES && ITCM_SIZE >> PAGE_BITS <= UNMARKED_PAGES
);
const MAIN_RAM_WAITS: Waits = Waits::new(16, 2, 18, 4);
const WIDE_WAITS: Waits = Waits::new(8, 2, 8, 2);
const NARROW_WAITS: Waits = Waits::new(8, 2, 10, 4);
const GBA_SLOT_WAITS: Waits = Waits::new(26, 12, 38, 24);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    MainRam,
    SharedWram,
    Io,
    Palette,
    Vram(Purpose),
    Oam,
    GbaSlot,
    Bios,
    Unmapped,
}

impl Region {
    fn of(addr: u32) -> Self {
        match addr >> 24 {
            0x02 => Self::MainRam,
            0x03 => Self::SharedWram,
            0x04 => Self::Io,
            0x05 => Self::Palette,
            0x06 => Self::Vram(match addr >> 21 & 7 {
                0 => Purpose::BgA,
                1 => Purpose::BgB,
                2 => Purpose::ObjA,
                3 => Purpose::ObjB,
                _ => Purpose::Lcdc,
            }),
            0x07 => Self::Oam,
            0x08..=0x0A => Self::GbaSlot,
            0xFF if addr >= BIOS_BASE && addr - BIOS_BASE < ARM9_BIOS_WINDOW as u32 => Self::Bios,
            _ => Self::Unmapped,
        }
    }

    fn waits(self) -> Waits {
        match self {
            Self::MainRam => MAIN_RAM_WAITS,
            Self::Palette | Self::Vram(_) => NARROW_WAITS,
            Self::GbaSlot => GBA_SLOT_WAITS,
            Self::SharedWram | Self::Io | Self::Oam | Self::Bios | Self::Unmapped => WIDE_WAITS,
        }
    }
}

pub struct Arm9Bus<'a> {
    shared: &'a mut Shared,
    own: &'a mut Arm9Memory,
}

impl<'a> Arm9Bus<'a> {
    pub(super) fn new(shared: &'a mut Shared, own: &'a mut Arm9Memory) -> Self {
        Self { shared, own }
    }

    #[inline(always)]
    pub fn cpu_read(&mut self, addr: u32, width: Width, fetch: Fetch) -> u32 {
        match self.own.tcm.read(addr, width, fetch) {
            Some(value) => value,
            None => self.read(addr, width),
        }
    }

    pub fn cpu_write(&mut self, addr: u32, width: Width, value: u32) {
        if !self.own.tcm.write(addr, width, value) {
            self.write(addr, width, value);
        }
    }

    pub fn read(&mut self, addr: u32, width: Width) -> u32 {
        self.read_region(Region::of(addr), addr, width)
    }

    #[inline(always)]
    fn read_region(&mut self, region: Region, addr: u32, width: Width) -> u32 {
        match region {
            Region::MainRam => self.read_main(addr, width),
            _ => self.read_elsewhere(region, addr, width),
        }
    }

    #[inline(never)]
    fn read_elsewhere(&mut self, region: Region, addr: u32, width: Width) -> u32 {
        let offset = addr as usize;
        match region {
            Region::MainRam => self.shared.main_ram.read(offset, width),
            Region::SharedWram => self.shared.wram.read(Cpu::Arm9, offset, width).unwrap_or(0),
            Region::Io => self.read_io(addr, width),
            Region::Palette => self.own.palette.read(offset & PALETTE_OAM_MASK, width),
            Region::Vram(purpose) => {
                self.shared
                    .vram
                    .read(purpose, vram_offset(purpose, addr), width)
            }
            Region::Oam => self.own.oam.read(offset & PALETTE_OAM_MASK, width),
            Region::GbaSlot => self.shared.read_gba_slot(false, addr, width),
            Region::Bios => read_bios(&self.own.bios, (addr - BIOS_BASE) as usize, width),
            Region::Unmapped => 0,
        }
    }

    pub fn write(&mut self, addr: u32, width: Width, value: u32) {
        let offset = addr as usize;
        let region = Region::of(addr);
        if width == Width::Byte && matches!(region, Region::Palette | Region::Vram(_) | Region::Oam)
        {
            return;
        }
        match region {
            Region::MainRam => self.shared.main_ram.write(offset, width, value),
            Region::SharedWram => {
                self.shared.wram.write(Cpu::Arm9, offset, width, value);
            }
            Region::Io => self.write_io(addr, width, value),
            Region::Palette => self
                .own
                .palette
                .write(offset & PALETTE_OAM_MASK, width, value),
            Region::Vram(purpose) => {
                let offset = vram_offset(purpose, addr);
                self.shared.vram.write(purpose, offset, width, value);
            }
            Region::Oam => self.own.oam.write(offset & PALETTE_OAM_MASK, width, value),
            Region::GbaSlot | Region::Bios | Region::Unmapped => {}
        }
    }

    pub fn take_stall(&mut self) -> u32 {
        std::mem::take(&mut self.own.stall)
    }

    pub fn take_attention(&mut self) -> bool {
        std::mem::take(&mut self.own.attention)
    }

    #[inline(always)]
    pub fn take_events(&mut self) -> (bool, u32) {
        if !self.own.pending {
            return (false, 0);
        }
        self.own.pending = false;
        (self.take_attention(), self.take_stall())
    }

    pub fn peek_plain(&self, addr: u32, width: Width, fetch: Fetch, io: bool) -> Option<u32> {
        if let Some(value) = self.own.tcm.read(addr, width, fetch) {
            return Some(value);
        }
        let offset = addr as usize;
        match Region::of(addr) {
            Region::MainRam => Some(self.shared.main_ram.read(offset, width)),
            Region::SharedWram => {
                Some(self.shared.wram.read(Cpu::Arm9, offset, width).unwrap_or(0))
            }
            Region::Bios => Some(read_bios(
                &self.own.bios,
                (addr - BIOS_BASE) as usize,
                width,
            )),
            Region::Io if io => Some(lane(peek_io(self.shared, self.own, addr), addr, width)),
            _ => None,
        }
    }

    pub fn ahead(&self) -> u32 {
        self.own.ahead
    }

    pub fn set_ahead(&mut self, ahead: u32) {
        self.own.ahead = ahead;
    }

    pub fn cp15(&self) -> &Cp15 {
        &self.own.cp15
    }

    pub fn run_dma(&mut self) -> u32 {
        let mut total = 0;
        while self.own.dma.pending() {
            let geometry = self.own.dma.pending_from(Start::GeometryFifo);
            let idle = self.own.dma.idle_with_fill();
            let mut dma = std::mem::replace(&mut self.own.dma, idle);
            let (irq, cycles) = dma.run_timed(self);
            self.own.dma = dma;
            self.own.stall += cycles * 2;
            self.own.pending = true;
            self.shared.irq[0].request(irq);
            total += cycles;
            if geometry
                && self.own.dma.waiting(Start::GeometryFifo)
                && self.own.three_d.dma_request()
            {
                self.own.dma.trigger(Start::GeometryFifo);
            }
        }
        total
    }

    #[inline(always)]
    fn waits(&self, addr: u32, width: Width, access: Access, fetch: Fetch) -> u32 {
        if fetch == Fetch::Code && second_half(addr, width, access) {
            return 0;
        }
        if self.own.tcm.hits(addr, fetch) {
            return FAST_CYCLES;
        }
        self.bus_cycles(Region::of(addr), addr, width, access, fetch)
    }

    #[inline(always)]
    fn read_main(&self, addr: u32, width: Width) -> u32 {
        self.shared
            .main_ram
            .read_sized::<MAIN_RAM_SIZE>(addr as usize, width)
    }

    #[inline(always)]
    fn bus_cycles(
        &self,
        region: Region,
        addr: u32,
        width: Width,
        access: Access,
        fetch: Fetch,
    ) -> u32 {
        let lookup = access == Access::NonSequential;
        match (fetch, self.own.caches.cached(addr, fetch)) {
            (Fetch::Code, true) if lookup || addr & CACHE_LINE_MASK == 0 => CACHE_LOOKUP_CYCLES,
            (Fetch::Data, true) if lookup => CACHE_LOOKUP_CYCLES,
            (_, true) => FAST_CYCLES,
            (Fetch::Code, false) => region.waits().cycles(Width::Word, Access::NonSequential),
            (Fetch::Data, false) => region.waits().cycles(width, access),
        }
    }

    #[inline(always)]
    fn near_write(&mut self, addr: u32, width: Width, value: u32, access: Access) -> Option<u32> {
        if self.own.tcm.write(addr, width, value) {
            if self.own.tcm.hits(addr, Fetch::Data) {
                return Some(FAST_CYCLES);
            }
            return Some(self.bus_cycles(Region::of(addr), addr, width, access, Fetch::Data));
        }
        if addr >> 24 == 0x02 {
            let cycles = self.bus_cycles(Region::MainRam, addr, width, access, Fetch::Data);
            self.shared.main_ram.write(addr as usize, width, value);
            return Some(cycles);
        }
        None
    }

    #[inline(always)]
    fn read_io_words(&mut self, addr: u32, values: &mut [u32]) -> u32 {
        let mut cycles = 0;
        let mut access = Access::NonSequential;
        for (index, value) in values.iter_mut().enumerate() {
            let at = addr + 4 * index as u32;
            cycles += self.bus_cycles(Region::Io, at, Width::Word, access, Fetch::Data);
            *value = self.read_io(at, Width::Word);
            access = Access::Sequential;
        }
        cycles
    }

    #[inline(always)]
    fn write_io_words(&mut self, addr: u32, values: &[u32]) -> u32 {
        let mut cycles = 0;
        let mut access = Access::NonSequential;
        for (index, &value) in values.iter().enumerate() {
            let at = addr + 4 * index as u32;
            cycles += self.bus_cycles(Region::Io, at, Width::Word, access, Fetch::Data);
            self.write_io(at, Width::Word, value);
            access = Access::Sequential;
        }
        cycles
    }

    #[inline(always)]
    fn main_ram_run(&self, addr: u32, words: usize) -> u32 {
        let mut cycles = 0;
        let mut access = Access::NonSequential;
        for index in 0..words as u32 {
            let at = addr + 4 * index;
            cycles += self.bus_cycles(Region::MainRam, at, Width::Word, access, Fetch::Data);
            access = Access::Sequential;
        }
        cycles
    }

    #[inline(never)]
    fn timed_read_elsewhere(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        if addr >> 24 == 0x04 {
            let cycles = self.bus_cycles(Region::Io, addr, width, access, Fetch::Data);
            return (cycles, self.read_io(addr, width));
        }
        let region = Region::of(addr);
        let cycles = self.bus_cycles(region, addr, width, access, Fetch::Data);
        (cycles, self.read_region(region, addr, width))
    }

    #[inline(never)]
    fn timed_write_elsewhere(
        &mut self,
        addr: u32,
        width: Width,
        value: u32,
        access: Access,
    ) -> u32 {
        if addr >> 24 == 0x04 {
            let cycles = self.bus_cycles(Region::Io, addr, width, access, Fetch::Data);
            self.write_io(addr, width, value);
            return cycles;
        }
        let region = Region::of(addr);
        let cycles = self.bus_cycles(region, addr, width, access, Fetch::Data);
        self.write(addr, width, value);
        cycles
    }

    #[inline(never)]
    fn timed_fetch_elsewhere(
        &mut self,
        addr: u32,
        width: Width,
        access: Access,
        free: bool,
    ) -> (u32, u32) {
        let region = Region::of(addr);
        let cycles = if free {
            0
        } else {
            self.bus_cycles(region, addr, width, access, Fetch::Code)
        };
        let value = match region {
            Region::Bios => read_bios(&self.own.bios, (addr - BIOS_BASE) as usize, width),
            _ => self.read_region(region, addr, width),
        };
        (cycles, value)
    }
}

fn second_half(addr: u32, width: Width, access: Access) -> bool {
    width == Width::Half && addr & 2 != 0 && access == Access::Sequential
}

fn vram_offset(purpose: Purpose, addr: u32) -> usize {
    match purpose {
        Purpose::Lcdc => addr as usize & VRAM_LCDC_MASK,
        _ => addr as usize & VRAM_ENGINE_MASK,
    }
}

impl CpuBus for Arm9Bus<'_> {
    fn fetch(&mut self, addr: u32, width: Width, _access: Access) -> u32 {
        self.cpu_read(addr, width, Fetch::Code)
    }

    fn read(&mut self, addr: u32, width: Width, _access: Access) -> u32 {
        self.cpu_read(addr, width, Fetch::Data)
    }

    fn write(&mut self, addr: u32, width: Width, value: u32, _access: Access) {
        self.cpu_write(addr, width, value);
    }

    fn idle(&mut self) {}

    fn cycles(&self, addr: u32, width: Width, access: Access) -> u32 {
        self.waits(addr, width, access, Fetch::Data)
    }

    fn code_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.waits(addr, width, access, Fetch::Code)
    }

    #[inline(always)]
    fn timed_fetch(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        let addr = addr & !(width.bytes() as u32 - 1);
        let free = second_half(addr, width, access);
        if let Some(value) = self.own.tcm.itcm_read(addr, width) {
            return (if free { 0 } else { FAST_CYCLES }, value);
        }
        if addr >> 24 == 0x02 {
            let cycles = if free {
                0
            } else {
                self.bus_cycles(Region::MainRam, addr, width, access, Fetch::Code)
            };
            return (cycles, self.read_main(addr, width));
        }
        self.timed_fetch_elsewhere(addr, width, access, free)
    }

    #[inline(always)]
    fn quick_fetch(&mut self, addr: u32, width: Width) -> Option<(u32, u32)> {
        let addr = addr & !(width.bytes() as u32 - 1);
        let free = second_half(addr, width, Access::Sequential);
        if let Some(value) = self.own.tcm.itcm_read(addr, width) {
            return Some((if free { 0 } else { FAST_CYCLES }, value));
        }
        if addr >> 24 == 0x02 {
            let cycles = if free {
                0
            } else {
                self.bus_cycles(
                    Region::MainRam,
                    addr,
                    width,
                    Access::Sequential,
                    Fetch::Code,
                )
            };
            return Some((cycles, self.read_main(addr, width)));
        }
        None
    }

    #[inline(always)]
    fn thumb_words(&self) -> bool {
        true
    }

    #[inline(always)]
    fn dropped_fetch(&mut self, addr: u32, width: Width) {
        if addr >> 24 != 0x02 && !self.own.tcm.hits(addr, Fetch::Code) {
            self.timed_fetch(addr, width, Access::Sequential);
        }
    }

    #[inline(always)]
    fn timed_refill(&mut self, addr: u32, width: Width) -> [(u32, u32); 2] {
        let size = width.bytes() as u32;
        let addr = addr & !(size - 1);
        let next = addr.wrapping_add(size);
        let free = second_half(next, width, Access::Sequential);
        if let Some(first) = self.own.tcm.itcm_read(addr, width) {
            let second = match self.own.tcm.itcm_read(next, width) {
                Some(second) => (if free { 0 } else { FAST_CYCLES }, second),
                None => self.timed_fetch(next, width, Access::Sequential),
            };
            return [(FAST_CYCLES, first), second];
        }
        if addr >> 24 == 0x02 && next & CACHE_LINE_MASK != 0 {
            let (first, rest) = if self.own.caches.cached(addr, Fetch::Code) {
                (CACHE_LOOKUP_CYCLES, FAST_CYCLES)
            } else {
                let word = Region::MainRam
                    .waits()
                    .cycles(Width::Word, Access::NonSequential);
                (word, word)
            };
            return [
                (first, self.read_main(addr, width)),
                (if free { 0 } else { rest }, self.read_main(next, width)),
            ];
        }
        [
            self.timed_fetch(addr, width, Access::NonSequential),
            self.timed_fetch(next, width, Access::Sequential),
        ]
    }

    #[inline(always)]
    fn timed_read(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        if let Some(value) = self.own.tcm.read(addr, width, Fetch::Data) {
            return (FAST_CYCLES, value);
        }
        if addr >> 24 == 0x02 {
            let cycles = self.bus_cycles(Region::MainRam, addr, width, access, Fetch::Data);
            return (cycles, self.read_main(addr, width));
        }
        self.timed_read_elsewhere(addr, width, access)
    }

    #[inline(always)]
    fn timed_write(&mut self, addr: u32, width: Width, value: u32, access: Access) -> u32 {
        match self.near_write(addr, width, value, access) {
            Some(cycles) => cycles,
            None => self.timed_write_elsewhere(addr, width, value, access),
        }
    }

    #[inline(always)]
    fn quick_write(&mut self, addr: u32, width: Width, value: u32) -> Option<u32> {
        self.near_write(addr, width, value, Access::NonSequential)
    }

    #[inline(always)]
    fn near_fetch(&self, addr: u32, width: Width) -> Option<(u32, u32)> {
        let addr = addr & !(width.bytes() as u32 - 1);
        let free = second_half(addr, width, Access::Sequential);
        if self.own.tcm.hits(addr, Fetch::Code) {
            let value = self.own.tcm.near_read(addr, width)?;
            return Some((if free { 0 } else { FAST_CYCLES }, value));
        }
        if addr >> 24 != 0x02 {
            return None;
        }
        let value = self
            .shared
            .main_ram
            .get_sized::<MAIN_RAM_SIZE>(addr as usize, width)?;
        let cycles = if free {
            0
        } else {
            self.bus_cycles(
                Region::MainRam,
                addr,
                width,
                Access::Sequential,
                Fetch::Code,
            )
        };
        Some((cycles, value))
    }

    #[inline(always)]
    fn near_drop(&self, addr: u32) -> bool {
        addr >> 24 == 0x02 || self.own.tcm.hits(addr, Fetch::Code)
    }

    #[inline(always)]
    fn near_refill(&self, addr: u32, width: Width) -> Option<[(u32, u32); 2]> {
        let size = width.bytes() as u32;
        let addr = addr & !(size - 1);
        let next = addr.wrapping_add(size);
        let free = second_half(next, width, Access::Sequential);
        if self.own.tcm.hits(addr, Fetch::Code) {
            if !self.own.tcm.hits(next, Fetch::Code) {
                return None;
            }
            let first = self.own.tcm.near_read(addr, width)?;
            let second = self.own.tcm.near_read(next, width)?;
            return Some([
                (FAST_CYCLES, first),
                (if free { 0 } else { FAST_CYCLES }, second),
            ]);
        }
        if addr >> 24 != 0x02 || next & CACHE_LINE_MASK == 0 {
            return None;
        }
        let ram = &self.shared.main_ram;
        let first = ram.get_sized::<MAIN_RAM_SIZE>(addr as usize, width)?;
        let second = ram.get_sized::<MAIN_RAM_SIZE>(next as usize, width)?;
        let (first_cycles, rest) = if self.own.caches.cached(addr, Fetch::Code) {
            (CACHE_LOOKUP_CYCLES, FAST_CYCLES)
        } else {
            let word = Region::MainRam
                .waits()
                .cycles(Width::Word, Access::NonSequential);
            (word, word)
        };
        Some([(first_cycles, first), (if free { 0 } else { rest }, second)])
    }

    #[inline(always)]
    fn near_read(&self, addr: u32, width: Width) -> Option<(u32, u32)> {
        if self.own.tcm.hits(addr, Fetch::Data) {
            return Some((FAST_CYCLES, self.own.tcm.near_read(addr, width)?));
        }
        if addr >> 24 != 0x02 {
            return None;
        }
        let value = self
            .shared
            .main_ram
            .get_sized::<MAIN_RAM_SIZE>(addr as usize, width)?;
        let cycles = self.bus_cycles(
            Region::MainRam,
            addr,
            width,
            Access::NonSequential,
            Fetch::Data,
        );
        Some((cycles, value))
    }

    #[inline(always)]
    fn near_words(&self, addr: u32, count: usize) -> Option<(u32, &[[u8; 4]])> {
        let last = addr.checked_add(4 * (count as u32 - 1))?;
        if let Some(offset) = self.own.tcm.dtcm_run(addr, last) {
            let words = self.own.tcm.dtcm_words(offset, count)?;
            return Some((FAST_CYCLES * count as u32, words));
        }
        if addr >> 24 != 0x02 || last >> 24 != 0x02 || !self.own.tcm.misses_in_main_ram(addr, last)
        {
            return None;
        }
        let words = self.shared.main_ram.words(addr as usize, count)?;
        Some((self.main_ram_run(addr, count), words))
    }

    #[inline(always)]
    fn near_words_mut(&mut self, addr: u32, count: usize) -> Option<(u32, &mut [[u8; 4]])> {
        let last = addr.checked_add(4 * (count as u32 - 1))?;
        if let Some(offset) = self.own.tcm.dtcm_run(addr, last) {
            let words = self.own.tcm.dtcm_words_mut(offset, count)?;
            return Some((FAST_CYCLES * count as u32, words));
        }
        if addr >> 24 != 0x02 || last >> 24 != 0x02 || !self.own.tcm.misses_in_main_ram(addr, last)
        {
            return None;
        }
        let cycles = self.main_ram_run(addr, count);
        let words = self.shared.main_ram.words_mut(addr as usize, count)?;
        Some((cycles, words))
    }

    #[inline(always)]
    fn timed_read_block(&mut self, addr: u32, values: &mut [u32]) -> u32 {
        if let Some(last) = addr.checked_add(4 * (values.len() as u32 - 1)) {
            if self.own.tcm.read_dtcm_words(addr, last, values) {
                return FAST_CYCLES * values.len() as u32;
            }
            if addr >> 24 == 0x02
                && last >> 24 == 0x02
                && self.own.tcm.misses_in_main_ram(addr, last)
                && self.shared.main_ram.read_words(addr as usize, values)
            {
                return self.main_ram_run(addr, values.len());
            }
            if addr >> 24 == 0x04 && last >> 24 == 0x04 && self.own.tcm.misses(addr, last) {
                return self.read_io_words(addr, values);
            }
        }
        read_words(self, addr, values)
    }

    #[inline(always)]
    fn timed_write_block(&mut self, addr: u32, values: &[u32]) -> u32 {
        if let Some(last) = addr.checked_add(4 * (values.len() as u32 - 1)) {
            if self.own.tcm.write_dtcm_words(addr, last, values) {
                return FAST_CYCLES * values.len() as u32;
            }
            if addr >> 24 == 0x02
                && last >> 24 == 0x02
                && self.own.tcm.misses_in_main_ram(addr, last)
                && self.shared.main_ram.write_words(addr as usize, values)
            {
                return self.main_ram_run(addr, values.len());
            }
            if addr >> 24 == 0x04 && last >> 24 == 0x04 && self.own.tcm.misses(addr, last) {
                return self.write_io_words(addr, values);
            }
        }
        write_words(self, addr, values)
    }

    fn settle(&mut self, cycles: u32) -> u32 {
        if self.own.ahead == 0 && cycles != 0 {
            return cycles;
        }
        if cycles == 0 {
            self.own.ahead += 1;
            return 1;
        }
        let repaid = self.own.ahead.min(cycles - 1);
        self.own.ahead -= repaid;
        cycles - repaid
    }

    fn bus_value(&self) -> u32 {
        0
    }

    fn drive(&mut self, _value: u32) {}

    fn coprocessor_read(&mut self, register: CoprocessorRegister) -> Option<u32> {
        self.own.cp15.read(register)
    }

    fn coprocessor_write(&mut self, register: CoprocessorRegister, value: u32) -> bool {
        self.own.attention = true;
        self.own.pending = true;
        let result = self.own.cp15.write(register, value);
        self.own.refresh_caches();
        match result {
            Cp15Write::Done => true,
            Cp15Write::TcmMoved => {
                self.own.place_tcm();
                true
            }
            Cp15Write::Halt => {
                self.own.halted = true;
                true
            }
            Cp15Write::Undefined => false,
        }
    }

    fn vector_base(&self) -> u32 {
        self.own.cp15.vector_base()
    }
}

#[cfg(jit)]
impl JitBus for Arm9Bus<'_> {
    const CP15: bool = true;
    const WRITABLE: &'static [CodeRam] = &[CodeRam::Main, CodeRam::Itcm];

    fn code_layout(&self) -> u64 {
        let itcm = self.own.tcm.itcm_region();
        itcm.virtual_size << 3
            | u64::from(itcm.enabled) << 2
            | u64::from(itcm.load_mode) << 1
            | u64::from(self.own.caches.generation()) << 40
    }

    fn code_span(&self, addr: u32) -> Option<CodeSpan> {
        if self.own.tcm.hits(addr, Fetch::Code) {
            return Some(CodeSpan {
                ram: CodeRam::Itcm,
                offset: addr & (ITCM_SIZE as u32 - 1),
            });
        }
        match Region::of(addr) {
            Region::MainRam => Some(CodeSpan {
                ram: CodeRam::Main,
                offset: addr & (MAIN_RAM_SIZE as u32 - 1),
            }),
            Region::Bios => Some(CodeSpan {
                ram: CodeRam::Bios,
                offset: addr - BIOS_BASE,
            }),
            _ => None,
        }
    }

    fn code_word(&self, span: CodeSpan) -> u32 {
        let offset = span.offset as usize;
        match span.ram {
            CodeRam::Main => self.shared.main_ram.read(offset, Width::Word),
            CodeRam::Itcm => self.own.tcm.itcm_ram().read(offset, Width::Word),
            CodeRam::Bios => read_bios(&self.own.bios, offset, Width::Word),
            CodeRam::Wram | CodeRam::SharedWram | CodeRam::Rom => 0,
        }
    }

    fn mark_code(&mut self, ram: CodeRam, page: usize) -> u32 {
        if self.code_generation(ram, page).is_none() {
            self.own.fast.moved();
        }
        match ram {
            CodeRam::Main => self.shared.main_ram.code_mut().mark(page, MAIN_RAM_SIZE),
            CodeRam::Itcm => self.own.tcm.itcm_ram_mut().code_mut().mark(page, ITCM_SIZE),
            CodeRam::Bios | CodeRam::Wram | CodeRam::SharedWram | CodeRam::Rom => 0,
        }
    }

    fn code_generation(&self, ram: CodeRam, page: usize) -> Option<u32> {
        match ram {
            CodeRam::Main => self.shared.main_ram.code().generation(page),
            CodeRam::Itcm => self.own.tcm.itcm_ram().code().generation(page),
            CodeRam::Bios => Some(0),
            CodeRam::Wram | CodeRam::SharedWram | CodeRam::Rom => None,
        }
    }

    fn take_code_hit(&mut self) -> bool {
        let main = self.shared.main_ram.code_mut().take_hit();
        let itcm = self.own.tcm.itcm_ram_mut().code_mut().take_hit();
        main || itcm
    }

    fn take_stall(&mut self) -> u32 {
        std::mem::take(&mut self.own.stall)
    }

    fn stall_flag(&self) -> *const u32 {
        &self.own.stall
    }

    fn cp15_cells(&mut self) -> Cp15Cells {
        let attention = if self.own.caches.filled() {
            &raw mut self.own.attention
        } else {
            std::ptr::null_mut()
        };
        Cp15Cells {
            attention,
            pending: &raw mut self.own.pending,
            dtcm: self.own.cp15.dtcm_word(),
        }
    }

    fn code_stamp(&self, ram: CodeRam) -> u32 {
        match ram {
            CodeRam::Main => self.shared.main_ram.code().stamp(),
            CodeRam::Itcm => self.own.tcm.itcm_ram().code().stamp(),
            CodeRam::Bios | CodeRam::Wram | CodeRam::SharedWram | CodeRam::Rom => 0,
        }
    }

    fn written_page(&self, ram: CodeRam, stamp: u32) -> Option<usize> {
        match ram {
            CodeRam::Main => self.shared.main_ram.code().written_page(stamp),
            CodeRam::Itcm => self.own.tcm.itcm_ram().code().written_page(stamp),
            CodeRam::Bios | CodeRam::Wram | CodeRam::SharedWram | CodeRam::Rom => None,
        }
    }

    fn runs_on(&self, masked: bool) -> bool {
        !self.own.halted && !self.shared.cart.armed() && (masked || !self.shared.irq[0].line())
    }
}

#[cfg(jit)]
impl FastBus for Arm9Bus<'_> {
    const SPECS: [Spec; WINDOWS] = [
        Spec {
            len: ITCM_SIZE as u32,
            code: true,
            cache: None,
            half: [FAST_CYCLES; 2],
            word: [FAST_CYCLES; 2],
        },
        Spec {
            len: DTCM_SIZE as u32,
            code: false,
            cache: None,
            half: [FAST_CYCLES; 2],
            word: [FAST_CYCLES; 2],
        },
        Spec {
            len: MAIN_RAM_SIZE as u32,
            code: true,
            cache: Some(Cache {
                shift: CacheMap::PAGE_BITS,
                bit: CacheMap::DATA,
                code: CacheMap::CODE,
                line: CACHE_LINE_MASK,
                cycles: [CACHE_LOOKUP_CYCLES, FAST_CYCLES],
            }),
            half: MAIN_RAM_WAITS.half,
            word: MAIN_RAM_WAITS.word,
        },
    ];

    const PAGES: bool = true;

    #[inline(always)]
    fn fast(&mut self) -> *mut Fast {
        let inputs = [
            self.shared.main_ram.code().pages().addr(),
            self.own.tcm.itcm_ram().code().pages().addr(),
            self.own.caches.generation() as usize,
            0,
        ];
        let stamp = self.own.fast.stamp(self.own, self.shared, inputs);
        match self.own.fast.get(stamp) {
            Some(table) => table,
            None => self.refill_fast(stamp),
        }
    }
}

#[cfg(jit)]
impl Arm9Bus<'_> {
    #[cold]
    #[inline(never)]
    fn refill_fast(&mut self, stamp: Stamp) -> *mut Fast {
        let windows = match self.own.tcm.windows() {
            Some([itcm, dtcm]) => [
                itcm,
                dtcm,
                self.shared.main_ram.window(MAIN_RAM_BASE, MAIN_RAM_WINDOW),
            ],
            None => [Window::CLOSED; WINDOWS],
        };
        let caches = &self.own.caches;
        let pages = self.own.fast.pages();
        fill_pages(pages, &windows, |index, addr| {
            if index < 2 {
                return Page::meta(FAST_CYCLES, FAST_CYCLES, FAST_CYCLES);
            }
            if caches.cached(addr, Fetch::Data) {
                return Page::meta(CACHE_LOOKUP_CYCLES, CACHE_LOOKUP_CYCLES, FAST_CYCLES);
            }
            let waits = Region::MainRam.waits();
            Page::meta(
                waits.cycles(Width::Half, Access::NonSequential),
                waits.cycles(Width::Word, Access::NonSequential),
                waits.cycles(Width::Word, Access::Sequential),
            )
        });
        let table = Fast {
            debt: &raw mut self.own.ahead,
            cache: self.own.caches.pages(),
            windows,
            pages: pages.table(),
        };
        self.own.fast.keep(stamp, table)
    }
}

#[cfg(jit)]
const MAPPED_PAGES: u64 = 1 << 14;

#[cfg(jit)]
fn fill_pages(pages: &mut Pages, windows: &[Window; WINDOWS], meta: impl Fn(usize, u32) -> u64) {
    let size = u64::from(PAGE_SIZE);
    let touches = |window: &Window, addr: u64| {
        let base = u64::from(window.base);
        window.size != 0 && addr + size > base && addr < base + window.size
    };
    for (index, window) in windows.iter().enumerate() {
        let len = Arm9Bus::SPECS[index].len;
        if window.size < size || window.base % PAGE_SIZE != 0 || len < PAGE_SIZE {
            continue;
        }
        let base = u64::from(window.base);
        for page in 0..(window.size / size).min(MAPPED_PAGES) {
            let addr = base + page * size;
            if windows[..index]
                .iter()
                .any(|earlier| touches(earlier, addr))
            {
                continue;
            }
            let addr = addr as u32;
            pages.set(addr, Page::of(window, len, addr, meta(index, addr)));
        }
    }
}

impl DmaBus for Arm9Bus<'_> {
    fn read_half(&mut self, address: u32) -> u16 {
        self.read(address, Width::Half) as u16
    }

    fn read_word(&mut self, address: u32) -> u32 {
        self.read(address, Width::Word)
    }

    fn write_half(&mut self, address: u32, value: u16) {
        self.write(address, Width::Half, u32::from(value));
    }

    fn write_word(&mut self, address: u32, value: u32) {
        self.write(address, Width::Word, value);
    }

    fn copy(&mut self, run: &Run) {
        if !self.copy_to_gxfifo(run) {
            run.copy_each(self);
        }
    }

    fn timing(&self, address: u32) -> Timing {
        match Region::of(address) {
            Region::MainRam => Timing::new(Area::MainRam, 16, 8, 1),
            Region::SharedWram => Timing::new(Area::Plain(1), 32, 1, 1),
            Region::Io => Timing::new(Area::Plain(2), 32, 1, 1),
            Region::Palette => Timing::new(Area::Plain(3), 16, 1, 1),
            Region::Vram(_) => Timing::new(Area::Plain(4), 16, 1, 1),
            Region::Oam => Timing::new(Area::Plain(5), 32, 1, 1),
            Region::Bios => Timing::new(Area::Plain(6), 32, 1, 1),
            Region::GbaSlot => self
                .shared
                .gba_slot_dma_timing(false, address)
                .set_by_registers(),
            Region::Unmapped => Timing::UNMAPPED,
        }
    }
}

pub(super) fn peek_io(shared: &Shared, own: &Arm9Memory, addr: u32) -> u32 {
    io::Arm9Io { shared, own }.peek(addr)
}

#[cfg(test)]
mod tests {
    use super::super::Buses;
    use super::*;
    use crate::gba::cpu::{Arm9, CpuState};
    #[cfg(jit)]
    use crate::jit::{HALF_CYCLES, SEQUENTIAL_CYCLES, WORD_CYCLES};
    use crate::nds::cp15::{DIRECT_BOOT_CONTROL, TcmRegion};
    use crate::nds::vram::Bank;

    const ON: u8 = 0x80;
    const VRAMCNT: u32 = 0x0400_0240;
    const WRAMCNT_ADDR: u32 = 0x0400_0247;

    fn vramcnt(buses: &mut Buses, bank: Bank, value: u8) {
        let register = match bank {
            Bank::H => 0x0400_0248,
            Bank::I => 0x0400_0249,
            _ => VRAMCNT + bank as u32,
        };
        buses
            .arm9()
            .write(register, Width::Byte, u32::from(ON | value));
    }

    #[test]
    fn a_dma_from_its_fill_register_copies_the_fill_value() {
        let mut buses = Buses::default();
        let mut bus = buses.arm9();
        bus.write(0x0400_00EC, Width::Word, 0x0000_00C0);
        bus.write(0x0400_00D4, Width::Word, 0x0400_00EC);
        bus.write(0x0400_00D8, Width::Word, 0x0200_0100);
        bus.write(0x0400_00DC, Width::Word, 0x8500_0004);
        for offset in [0, 4, 8, 12] {
            assert_eq!(bus.read(0x0200_0100 + offset, Width::Word), 0xC0);
        }
    }

    #[test]
    fn shared_wram_follows_wramcnt_and_mirrors_across_16m() {
        let mut buses = Buses::default();
        buses.arm9().write(WRAMCNT_ADDR, Width::Byte, 0);
        buses.arm9().write(0x0300_0000, Width::Word, 0x1111_1111);
        buses.arm9().write(0x0300_4000, Width::Word, 0x2222_2222);
        assert_eq!(buses.arm9().read(0x03FF_8000, Width::Word), 0x1111_1111);
        buses.arm9().write(WRAMCNT_ADDR, Width::Byte, 1);
        assert_eq!(buses.arm9().read(0x0300_0000, Width::Word), 0x2222_2222);
        buses.arm9().write(WRAMCNT_ADDR, Width::Byte, 2);
        assert_eq!(buses.arm9().read(0x0300_4000, Width::Word), 0x1111_1111);
    }

    #[test]
    fn shared_wram_reads_zero_and_drops_writes_when_the_arm9_has_none() {
        let mut buses = Buses::default();
        buses.arm9().write(0x0300_0000, Width::Word, 7);
        buses.arm9().write(WRAMCNT_ADDR, Width::Byte, 3);
        assert_eq!(buses.arm9().read(0x0300_0000, Width::Word), 0);
        buses.arm9().write(0x0300_0000, Width::Word, 9);
        buses.arm9().write(WRAMCNT_ADDR, Width::Byte, 0);
        assert_eq!(buses.arm9().read(0x0300_0000, Width::Word), 7);
    }

    #[test]
    fn wramcnt_reads_back_its_mode() {
        let mut buses = Buses::default();
        buses.arm9().write(WRAMCNT_ADDR, Width::Byte, 0xFE);
        assert_eq!(buses.arm9().read(WRAMCNT_ADDR, Width::Byte), 2);
        assert_eq!(buses.arm9().read(0x0400_0244, Width::Word), 0x0200_0000);
    }

    #[test]
    fn palettes_hold_both_engines_in_2k_and_mirror() {
        let mut buses = Buses::default();
        buses.arm9().write(0x0500_0000, Width::Half, 0x001F);
        buses.arm9().write(0x0500_0600, Width::Half, 0x7C00);
        assert_eq!(buses.palette()[0x000..0x002], [0x1F, 0x00]);
        assert_eq!(buses.palette()[0x600..0x602], [0x00, 0x7C]);
        assert_eq!(buses.arm9().read(0x05FF_FE00, Width::Half), 0x7C00);
    }

    #[test]
    fn oam_holds_both_engines_in_2k_and_mirrors() {
        let mut buses = Buses::default();
        buses.arm9().write(0x0700_0400, Width::Word, 0x0102_0304);
        assert_eq!(buses.oam()[0x400], 0x04);
        assert_eq!(buses.arm9().read(0x0700_0C00, Width::Word), 0x0102_0304);
    }

    #[test]
    fn byte_writes_to_palette_vram_and_oam_are_ignored() {
        let mut buses = Buses::default();
        vramcnt(&mut buses, Bank::A, 0);
        for addr in [0x0500_0000, 0x0680_0000, 0x0700_0000] {
            buses.arm9().write(addr, Width::Byte, 0xFF);
            assert_eq!(buses.arm9().read(addr, Width::Byte), 0);
            buses.arm9().write(addr, Width::Half, 0xABCD);
            assert_eq!(buses.arm9().read(addr, Width::Byte), 0xCD);
        }
    }

    #[test]
    fn each_vram_window_reaches_its_purpose() {
        let mut buses = Buses::default();
        vramcnt(&mut buses, Bank::A, 1);
        vramcnt(&mut buses, Bank::B, 2);
        vramcnt(&mut buses, Bank::C, 4);
        vramcnt(&mut buses, Bank::D, 4);
        vramcnt(&mut buses, Bank::E, 0);
        let windows = [
            (0x0600_0000, Bank::A),
            (0x0620_0000, Bank::C),
            (0x0640_0000, Bank::B),
            (0x0660_0000, Bank::D),
            (0x0688_0000, Bank::E),
        ];
        for (index, (addr, bank)) in (1u8..).zip(windows) {
            buses.arm9().write(addr, Width::Half, u32::from(index));
            assert_eq!(buses.vram().bank(bank)[0], index);
        }
    }

    #[test]
    fn vram_windows_mirror_across_their_2m_blocks() {
        let mut buses = Buses::default();
        vramcnt(&mut buses, Bank::A, 1);
        vramcnt(&mut buses, Bank::H, 1);
        buses.arm9().write(0x0618_0010, Width::Half, 0x1234);
        assert_eq!(buses.arm9().read(0x0600_0010, Width::Half), 0x1234);
        buses.arm9().write(0x0622_0020, Width::Half, 0x5678);
        assert_eq!(buses.arm9().read(0x0620_0020, Width::Half), 0x5678);
    }

    #[test]
    fn lcdc_mirrors_every_1m_up_to_the_end_of_vram() {
        let mut buses = Buses::default();
        vramcnt(&mut buses, Bank::I, 0);
        buses.arm9().write(0x068A_0000, Width::Half, 0x0BAD);
        assert_eq!(buses.arm9().read(0x06FA_0000, Width::Half), 0x0BAD);
        assert_eq!(buses.arm9().read(0x068A_4000, Width::Half), 0);
    }

    #[test]
    fn unmapped_vram_reads_zero_and_drops_writes() {
        let mut buses = Buses::default();
        buses.arm9().write(0x0600_0000, Width::Word, 5);
        assert_eq!(buses.arm9().read(0x0600_0000, Width::Word), 0);
    }

    #[test]
    fn a_word_write_sets_four_vram_banks_at_once() {
        let mut buses = Buses::default();
        buses.arm9().write(VRAMCNT, Width::Word, 0x8484_8281);
        assert_eq!(buses.vram().control(Bank::A), 0x81);
        assert_eq!(buses.vram().control(Bank::D), 0x84);
        assert_eq!(buses.arm7().read(0x0400_0240, Width::Byte), 0);
    }

    #[test]
    fn tcm_comes_before_the_bus_for_the_cpu_only() {
        let mut buses = Buses::default();
        buses.tcm_mut().place_dtcm(TcmRegion {
            base: 0x0200_0000,
            virtual_size: 0x4000,
            enabled: true,
            load_mode: false,
        });
        let mut bus = buses.arm9();
        bus.cpu_write(0x0200_0000, Width::Word, 1);
        bus.write(0x0200_0000, Width::Word, 2);
        assert_eq!(bus.cpu_read(0x0200_0000, Width::Word, Fetch::Data), 1);
        assert_eq!(bus.cpu_read(0x0200_0000, Width::Word, Fetch::Code), 2);
        assert_eq!(bus.read(0x0200_0000, Width::Word), 2);
    }

    #[test]
    fn the_cpu_bus_trait_reaches_itcm_and_dtcm() {
        let mut buses = Buses::default();
        let on = |base, virtual_size| TcmRegion {
            base,
            virtual_size,
            enabled: true,
            load_mode: false,
        };
        buses.tcm_mut().place_itcm(on(0, 0x200_0000));
        buses.tcm_mut().place_dtcm(on(0x027C_0000, 0x4000));
        let mut bus = buses.arm9();
        CpuBus::write(
            &mut bus,
            0x01FF_8000,
            Width::Word,
            0xE12F_FF1E,
            Access::NonSequential,
        );
        CpuBus::write(
            &mut bus,
            0x027C_3FFC,
            Width::Word,
            0x0380_0000,
            Access::NonSequential,
        );
        assert_eq!(
            bus.fetch(0, Width::Word, Access::NonSequential),
            0xE12F_FF1E
        );
        assert_eq!(
            CpuBus::read(&mut bus, 0x027C_3FFC, Width::Word, Access::Sequential),
            0x0380_0000
        );
    }

    #[test]
    fn undefined_io_and_unmapped_blocks_read_zero() {
        let mut buses = Buses::default();
        for addr in [
            0x0400_0070,
            0x0410_0000,
            0x0100_0000,
            0x0B00_0000,
            0xFFFF_8000,
        ] {
            buses.arm9().write(addr, Width::Word, 0xFFFF_FFFF);
            assert_eq!(buses.arm9().read(addr, Width::Word), 0);
        }
    }

    const CONTROL: CoprocessorRegister = CoprocessorRegister {
        coprocessor: 15,
        opcode1: 0,
        crn: 1,
        crm: 0,
        opcode2: 0,
    };
    const CACHES_ON: u32 = DIRECT_BOOT_CONTROL | 0x1005;
    const UNCACHED_WORD: u32 = 18;
    const NOP: u32 = 0xE1A0_0000;

    fn uncached() -> Buses {
        let mut buses = Buses::default();
        buses.boot_cp15();
        buses
    }

    fn cached() -> Buses {
        let mut buses = uncached();
        CpuBus::coprocessor_write(&mut buses.arm9(), CONTROL, CACHES_ON);
        buses
    }

    fn arm9_at(buses: &mut Buses, entry: u32, code: &[u32], r0: u32) -> Arm9 {
        for (addr, &word) in (entry..).step_by(4).zip(code) {
            buses.arm9().write(addr, Width::Word, word);
        }
        let mut state = CpuState::default();
        state.r[0] = r0;
        state.r[15] = entry + 8;
        state.pipeline = [code[0], code[1]];
        state.next_fetch = Access::Sequential;
        Arm9::from_state(&state)
    }

    fn fetch_cycles(buses: &mut Buses, addr: u32, width: Width, access: Access) -> u32 {
        buses.arm9().timed_fetch(addr, width, access).0
    }

    fn read_cycles(buses: &mut Buses, addr: u32, access: Access) -> u32 {
        CpuBus::timed_read(&mut buses.arm9(), addr, Width::Word, access).0
    }

    #[test]
    fn cached_code_costs_3_at_a_line_start_or_after_a_jump_and_1_inside_a_line() {
        let mut buses = cached();
        let s = Access::Sequential;
        assert_eq!(fetch_cycles(&mut buses, 0x0200_0020, Width::Word, s), 3);
        assert_eq!(fetch_cycles(&mut buses, 0x0200_0024, Width::Word, s), 1);
        assert_eq!(fetch_cycles(&mut buses, 0xFFFF_0004, Width::Word, s), 1);
        let n = Access::NonSequential;
        assert_eq!(fetch_cycles(&mut buses, 0x0200_0028, Width::Word, n), 3);
    }

    #[test]
    fn cached_data_costs_3_nonsequential_and_1_sequential() {
        let mut buses = cached();
        assert_eq!(
            read_cycles(&mut buses, 0x0200_0024, Access::NonSequential),
            3
        );
        assert_eq!(read_cycles(&mut buses, 0x0200_0028, Access::Sequential), 1);
        let mut bus = buses.arm9();
        assert_eq!(
            bus.data_cycles(0x0200_0024, Width::Byte, Access::NonSequential),
            3
        );
    }

    #[test]
    fn uncached_code_pays_a_nonsequential_word_on_every_fetch() {
        let mut buses = uncached();
        for access in [Access::NonSequential, Access::Sequential] {
            let cycles = fetch_cycles(&mut buses, 0x0200_0004, Width::Word, access);
            assert_eq!(cycles, UNCACHED_WORD);
        }
    }

    #[test]
    fn the_second_thumb_halfword_of_a_word_is_free() {
        let mut buses = uncached();
        let s = Access::Sequential;
        assert_eq!(fetch_cycles(&mut buses, 0x0200_0006, Width::Half, s), 0);
        assert_eq!(
            fetch_cycles(&mut buses, 0x0200_0004, Width::Half, s),
            UNCACHED_WORD
        );
    }

    #[test]
    fn an_uncached_region_inside_cached_main_ram_reaches_the_bus() {
        let mut buses = cached();
        let region_7 = CoprocessorRegister {
            crn: 6,
            crm: 7,
            ..CONTROL
        };
        CpuBus::coprocessor_write(&mut buses.arm9(), region_7, 0x0200_0017);
        let n = Access::NonSequential;
        assert_eq!(read_cycles(&mut buses, 0x0200_0FFC, n), UNCACHED_WORD);
        assert_eq!(read_cycles(&mut buses, 0x0200_1000, n), 3);
    }

    #[test]
    fn disabling_a_protection_region_stops_caching_its_pages() {
        let mut buses = cached();
        let region_1 = CoprocessorRegister {
            crn: 6,
            crm: 1,
            ..CONTROL
        };
        CpuBus::coprocessor_write(&mut buses.arm9(), region_1, 0);
        let n = Access::NonSequential;
        assert_eq!(read_cycles(&mut buses, 0x0200_0000, n), UNCACHED_WORD);
    }

    #[test]
    fn a_refill_costs_and_reads_what_two_fetches_do() {
        let starts = [
            0x0000_0100,
            0x01FF_FFFC,
            0x01FF_FFFE,
            0x0200_0000,
            0x0200_0012,
            0x0200_001C,
            0x0200_001E,
            0x02FF_FFFC,
            0x0300_0010,
            0x0600_0000,
            0xFFFF_0010,
        ];
        let (n, s) = (Access::NonSequential, Access::Sequential);
        for mut buses in [uncached(), cached()] {
            let mut bus = buses.arm9();
            for &start in &starts {
                bus.write(start, Width::Word, start ^ 0x5A5A_5A5A);
                bus.write(start + 4, Width::Word, start ^ 0xA5A5_A5A5);
            }
            for &start in &starts {
                for width in [Width::Word, Width::Half] {
                    let next = start + width.bytes() as u32;
                    let two = [
                        bus.timed_fetch(start, width, n),
                        bus.timed_fetch(next, width, s),
                    ];
                    let refill = bus.timed_refill(start, width);
                    assert_eq!(refill, two, "{start:08X} {width:?}");
                }
            }
        }
    }

    #[test]
    fn a_timed_write_costs_and_stores_what_a_plain_write_does() {
        let widths = [
            (Width::Word, Access::NonSequential),
            (Width::Half, Access::Sequential),
            (Width::Byte, Access::NonSequential),
        ];
        for setup in [uncached, cached] {
            let (mut plain, mut timed) = (setup(), setup());
            let dtcm = plain.cp15().dtcm().base;
            let starts = [
                0x0000_0100,
                dtcm + 0x10,
                0x0200_0010,
                0x0200_0020,
                0x0300_0010,
                0x0500_0010,
                0x0600_0010,
                0x0700_0010,
            ];
            for start in starts {
                for (width, access) in widths {
                    let value = start ^ 0x5A5A_5A5A;
                    let mut bus = plain.arm9();
                    let cycles = bus.data_cycles(start, width, access);
                    CpuBus::write(&mut bus, start, width, value, access);
                    let timed_cycles = timed.arm9().timed_write(start, width, value, access);
                    assert_eq!(timed_cycles, cycles, "{start:08X} {width:?}");
                    assert_eq!(
                        timed.arm9().cpu_read(start, Width::Word, Fetch::Data),
                        plain.arm9().cpu_read(start, Width::Word, Fetch::Data),
                        "{start:08X} {width:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_free_instruction_counts_1_cycle_and_later_ones_repay_it() {
        let mut buses = Buses::default();
        let mut bus = buses.arm9();
        assert_eq!(bus.settle(0), 1);
        assert_eq!(bus.settle(0), 1);
        assert_eq!(bus.settle(2), 1);
        assert_eq!(bus.settle(5), 4);
        assert_eq!(bus.settle(3), 3);
    }

    #[test]
    fn a_settled_instruction_never_takes_0_cycles() {
        let mut buses = Buses::default();
        let mut bus = buses.arm9();
        for ahead in [0, 1, 2, 7, u32::MAX - 1] {
            for cycles in 0..12 {
                bus.set_ahead(ahead);
                assert_ne!(bus.settle(cycles), 0, "{ahead} {cycles}");
            }
        }
    }

    #[test]
    fn a_cached_store_hides_its_fetch_behind_the_data_access() {
        let mut buses = cached();
        let strb = 0xE5C0_1000;
        let mut cpu = arm9_at(&mut buses, 0x0200_0000, &[strb, NOP, NOP], 0x0200_0100);
        assert_eq!(cpu.step(&mut buses.arm9()), 3);
    }

    #[test]
    fn an_uncached_load_overlaps_its_fetch_by_6_cycles() {
        let mut buses = uncached();
        let ldr = 0xE590_2000;
        let mut cpu = arm9_at(&mut buses, 0x0200_0000, &[ldr, NOP, NOP], 0x0200_0100);
        assert_eq!(cpu.step(&mut buses.arm9()), 2 * UNCACHED_WORD - 6);
    }

    const THUMB: u32 = 1 << 5;
    const THUMB_NOP: u16 = 0x46C0;
    const THUMB_BACK_4: u16 = 0xE7FC;
    const THUMB_BACK_6: u16 = 0xE7FD;
    const MOV_R2_1: u16 = 0x2201;
    const MOV_R2_2: u16 = 0x2202;
    const STRH_R1_R0: u16 = 0x8001;

    fn thumb_at(buses: &mut Buses, entry: u32, code: &[u16], r0: u32, r1: u32) -> Arm9 {
        for (addr, &half) in (entry..).step_by(2).zip(code) {
            buses.arm9().write(addr, Width::Half, u32::from(half));
        }
        let mut state = CpuState::default();
        state.r[0] = r0;
        state.r[1] = r1;
        state.r[15] = entry + 4;
        state.cpsr |= THUMB;
        state.pipeline = [u32::from(code[0]), u32::from(code[1])];
        state.next_fetch = Access::Sequential;
        Arm9::from_state(&state)
    }

    #[test]
    fn a_thumb_store_into_the_fetched_word_runs_the_old_upper_half_until_a_branch() {
        let mut buses = uncached();
        let entry = 0x0200_0000;
        let code = [STRH_R1_R0, THUMB_NOP, THUMB_BACK_4, MOV_R2_1];
        let new = u32::from(MOV_R2_2);
        let mut cpu = thumb_at(&mut buses, entry, &code, entry + 6, new);
        cpu.step(&mut buses.arm9());
        assert_eq!(buses.arm9().read(entry + 6, Width::Half), new);
        cpu.step(&mut buses.arm9());
        assert_eq!(
            cpu.pipeline(),
            [u32::from(THUMB_BACK_4), u32::from(MOV_R2_1)]
        );
        cpu.step(&mut buses.arm9());
        assert_eq!(
            cpu.pipeline(),
            [u32::from(STRH_R1_R0), u32::from(THUMB_NOP)]
        );
        cpu.step(&mut buses.arm9());
        cpu.step(&mut buses.arm9());
        assert_eq!(cpu.pipeline(), [u32::from(THUMB_BACK_4), new]);
    }

    #[test]
    fn a_branch_into_an_upper_half_refetches_the_word_after_it() {
        let mut buses = uncached();
        let entry = 0x0200_0000;
        let code = [STRH_R1_R0, THUMB_NOP, THUMB_BACK_6, MOV_R2_1];
        let new = u32::from(MOV_R2_2);
        let mut cpu = thumb_at(&mut buses, entry, &code, entry + 6, new);
        cpu.step(&mut buses.arm9());
        cpu.step(&mut buses.arm9());
        assert_eq!(
            cpu.pipeline(),
            [u32::from(THUMB_BACK_6), u32::from(MOV_R2_1)]
        );
        cpu.step(&mut buses.arm9());
        assert_eq!(
            cpu.pipeline(),
            [u32::from(THUMB_NOP), u32::from(THUMB_BACK_6)]
        );
        cpu.step(&mut buses.arm9());
        assert_eq!(cpu.pipeline(), [u32::from(THUMB_BACK_6), new]);
    }

    #[cfg(jit)]
    #[test]
    fn a_jit_run_drops_the_fetched_thumb_word() {
        let mut buses = uncached();
        let entry = 0x0200_0000;
        let code = [THUMB_NOP, THUMB_NOP, THUMB_NOP, MOV_R2_1];
        let new = u32::from(MOV_R2_2);
        let mut cpu = thumb_at(&mut buses, entry, &code, 0, 0);
        cpu.step(&mut buses.arm9());
        cpu = cpu.landed_here();
        buses.arm9().write(entry + 6, Width::Half, new);
        crate::jit::Jit::default().run(&mut cpu, &mut buses.arm9(), 0);
        cpu.step(&mut buses.arm9());
        assert_eq!(cpu.pipeline(), [u32::from(THUMB_NOP), new]);
    }

    #[test]
    fn a_taken_branch_pays_only_its_two_refetches() {
        for (branch, cycles) in [(0xEA00_000E, 4), (0xEA00_0015, 6)] {
            let mut buses = cached();
            let mut cpu = arm9_at(&mut buses, 0x0200_0000, &[branch, NOP], 0);
            assert_eq!(cpu.step(&mut buses.arm9()), cycles);
        }
    }

    fn placed(load_mode: bool) -> Buses {
        let mut buses = cached();
        let region_7 = CoprocessorRegister {
            crn: 6,
            crm: 7,
            ..CONTROL
        };
        CpuBus::coprocessor_write(&mut buses.arm9(), region_7, 0x0200_0017);
        let on = |base, virtual_size| TcmRegion {
            base,
            virtual_size,
            enabled: true,
            load_mode,
        };
        buses.tcm_mut().place_itcm(on(0, 0x200_0000));
        buses.tcm_mut().place_dtcm(on(0x027C_0000, 0x4000));
        buses
    }

    const DTCM_PLACE: CoprocessorRegister = CoprocessorRegister {
        crn: 9,
        crm: 1,
        ..CONTROL
    };

    fn block_matches_single_words(buses: &Buses, addr: u32, words: usize) {
        let mut block = buses.clone();
        let mut single = buses.clone();
        let (mut fast, mut slow) = ([0; 16], [0; 16]);
        let cycles = (
            CpuBus::timed_read_block(&mut block.arm9(), addr, &mut fast[..words]),
            read_words(&mut single.arm9(), addr, &mut slow[..words]),
        );
        assert_eq!((cycles.0, fast), (cycles.1, slow), "read {addr:08X}");
        let values: Vec<u32> = (1..=words as u32).map(|v| v * 0x0101_0101).collect();
        let cycles = (
            CpuBus::timed_write_block(&mut block.arm9(), addr, &values),
            write_words(&mut single.arm9(), addr, &values),
        );
        assert_eq!(cycles.0, cycles.1, "write {addr:08X}");
        assert!(block.main_ram() == single.main_ram(), "main ram {addr:08X}");
        for at in (0..words as u32 + 4).map(|index| addr.wrapping_sub(8).wrapping_add(4 * index)) {
            let read = |buses: &mut Buses| buses.arm9().cpu_read(at, Width::Word, Fetch::Data);
            assert_eq!(
                read(&mut block),
                read(&mut single),
                "{at:08X} after {addr:08X}"
            );
        }
    }

    fn seeded(mut buses: Buses) -> Buses {
        for addr in [
            0x0200_0100,
            0x023F_FFF0,
            0x0200_0000,
            0x027B_FFF0,
            0x027C_3FF0,
        ] {
            for at in (addr..).step_by(4).take(8) {
                buses.arm9().cpu_write(at, Width::Word, at ^ 0x5A5A_5A5A);
            }
        }
        buses
    }

    #[test]
    fn a_block_run_matches_single_words_inside_and_across_every_edge() {
        let edges = [
            (0x0200_0100, 8),
            (0x0200_0101, 3),
            (0x023F_FFF8, 4),
            (0x02FF_FFF8, 4),
            (0x027B_FFF8, 4),
            (0x027C_0010, 6),
            (0x027C_3FF8, 4),
            (0xFFFF_FFF8, 4),
        ];
        for buses in [uncached(), cached(), placed(false), placed(true)] {
            let buses = seeded(buses);
            for (addr, words) in edges {
                block_matches_single_words(&buses, addr, words);
            }
        }
    }

    #[test]
    fn a_block_run_follows_the_dtcm_when_cp15_moves_it() {
        let mut buses = seeded(placed(false));
        CpuBus::coprocessor_write(&mut buses.arm9(), DTCM_PLACE, 0x0230_000A);
        for (addr, words) in [
            (0x027C_0010, 4),
            (0x022F_FFF8, 4),
            (0x0230_0010, 4),
            (0x0230_3FF8, 4),
        ] {
            block_matches_single_words(&buses, addr, words);
        }
    }

    #[cfg(jit)]
    #[test]
    fn a_block_store_over_compiled_main_ram_code_makes_it_stale() {
        let mut buses = Buses::default();
        let mut bus = buses.arm9();
        bus.mark_code(CodeRam::Main, 2);
        let generation = bus.code_generation(CodeRam::Main, 2);
        let addr = 0x0200_0000 + (2 << PAGE_BITS) as u32 - 4;
        CpuBus::timed_write_block(&mut bus, addr, &[1, 2, 3]);
        assert!(bus.take_code_hit());
        assert_ne!(bus.code_generation(CodeRam::Main, 2), generation);
    }

    #[cfg(jit)]
    fn fast_cycles(fast: &Fast, spec: Spec, addr: u32, width: Width, access: Access) -> u32 {
        let slot = usize::from(access == Access::Sequential);
        let uncached = match width {
            Width::Word => spec.word[slot],
            Width::Byte | Width::Half => spec.half[slot],
        };
        match spec.cache {
            Some(cache) => {
                let flags = unsafe { *fast.cache.add((addr >> cache.shift) as usize) };
                if flags & cache.bit != 0 {
                    cache.cycles[slot]
                } else {
                    uncached
                }
            }
            None => uncached,
        }
    }

    #[cfg(jit)]
    #[test]
    fn the_fast_windows_reach_the_bytes_and_cost_the_cycles_of_the_bus() {
        let mut buses = placed(false);
        let mut bus = buses.arm9();
        let addrs = [
            0x0000_0010,
            0x01FF_8004,
            0x027C_0010,
            0x027C_3FFC,
            0x027C_4000,
            0x0200_0024,
            0x0200_0FFC,
            0x023F_FFF8,
            0x02FF_FFFC,
            0x0300_0000,
            0x0400_0000,
            0x0600_0000,
            0x0B00_0000,
        ];
        for (addr, value) in addrs.into_iter().zip(0x1111_1111u32..) {
            CpuBus::write(&mut bus, addr, Width::Word, value, Access::NonSequential);
            let fast = unsafe { *bus.fast() };
            let window = fast.windows.iter().position(|window| window.holds(addr));
            let inline = bus.own.tcm.hits(addr, Fetch::Data) || Region::of(addr) == Region::MainRam;
            assert_eq!(window.is_some(), inline, "{addr:08X}");
            let Some(index) = window else {
                continue;
            };
            let (window, spec) = (fast.windows[index], Arm9Bus::SPECS[index]);
            let offset = (addr - window.base) & (spec.len - 1);
            let host = unsafe {
                window
                    .bytes
                    .add(offset as usize)
                    .cast::<u32>()
                    .read_unaligned()
            };
            let read = CpuBus::read(&mut bus, addr, Width::Word, Access::NonSequential);
            assert_eq!((host, read), (value, value), "{addr:08X}");
            for width in [Width::Byte, Width::Half, Width::Word] {
                for access in [Access::NonSequential, Access::Sequential] {
                    assert_eq!(
                        fast_cycles(&fast, spec, addr, width, access),
                        bus.cycles(addr, width, access),
                        "{addr:08X} {width:?} {access:?}"
                    );
                }
            }
        }
    }

    #[cfg(jit)]
    fn page_of(bus: &mut Arm9Bus, addr: u32) -> Page {
        unsafe { *(*bus.fast()).pages.add((addr >> 12) as usize) }
    }

    #[cfg(jit)]
    fn pages_match_the_bus(bus: &mut Arm9Bus, addrs: &[u32]) {
        for (&addr, value) in addrs.iter().zip(0x2222_2222u32..) {
            CpuBus::write(bus, addr, Width::Word, value, Access::NonSequential);
            let page = page_of(bus, addr);
            let inline = bus.own.tcm.hits(addr, Fetch::Data) || Region::of(addr) == Region::MainRam;
            assert_eq!(page.meta != 0, inline, "{addr:08X}");
            if !inline {
                continue;
            }
            let host = page.addend.wrapping_add(u64::from(addr)) as *const u32;
            assert_eq!(unsafe { host.read_unaligned() }, value, "{addr:08X}");
            assert_ne!(unsafe { *page.dirty }, 0, "{addr:08X}");
            let field = |shift: u32| ((page.meta >> shift) & 0xFF) as u32;
            let cycles = |width, access| bus.cycles(addr, width, access);
            assert_eq!(
                [HALF_CYCLES, WORD_CYCLES, SEQUENTIAL_CYCLES].map(field),
                [
                    cycles(Width::Half, Access::NonSequential),
                    cycles(Width::Word, Access::NonSequential),
                    cycles(Width::Word, Access::Sequential),
                ],
                "{addr:08X}"
            );
            assert_eq!(
                cycles(Width::Byte, Access::NonSequential),
                field(HALF_CYCLES)
            );
        }
    }

    #[cfg(jit)]
    #[test]
    fn the_page_table_reaches_the_bytes_and_costs_the_cycles_of_the_bus() {
        let mut buses = placed(false);
        let mut bus = buses.arm9();
        let addrs = [
            0x0000_0010,
            0x01FF_8004,
            0x027C_0010,
            0x027C_3FFC,
            0x027C_4000,
            0x0200_0024,
            0x0200_0FFC,
            0x0230_0010,
            0x023F_FFF8,
            0x02FF_FFFC,
            0x0300_0000,
            0x0400_0000,
            0x0600_0000,
            0x0B00_0000,
        ];
        pages_match_the_bus(&mut bus, &addrs);
        let region_7 = CoprocessorRegister {
            crn: 6,
            crm: 7,
            ..CONTROL
        };
        CpuBus::coprocessor_write(&mut bus, region_7, 0x0230_0017);
        assert_ne!(
            page_of(&mut bus, 0x0230_0010).meta,
            page_of(&mut bus, 0x0230_1010).meta
        );
        pages_match_the_bus(&mut bus, &addrs);
    }

    #[cfg(jit)]
    #[test]
    fn a_page_partly_under_a_tcm_stays_on_the_helpers() {
        let mut buses = placed(false);
        buses.tcm_mut().place_dtcm(TcmRegion {
            base: 0x027C_0000,
            virtual_size: 0x4000,
            enabled: true,
            load_mode: false,
        });
        let mut bus = buses.arm9();
        assert_ne!(page_of(&mut bus, 0x027C_3000).meta, 0);
        assert_ne!(page_of(&mut bus, 0x027C_4000).meta, 0);
        bus.own.tcm.place_dtcm(TcmRegion {
            base: 0x027C_0800,
            virtual_size: 0x800,
            enabled: true,
            load_mode: false,
        });
        bus.own.fast.moved();
        assert_eq!(page_of(&mut bus, 0x027C_0000).meta, 0);
        assert_ne!(page_of(&mut bus, 0x027C_1000).meta, 0);
    }

    #[cfg(jit)]
    #[test]
    fn the_cache_table_reaches_every_main_ram_page() {
        let end = MAIN_RAM_BASE as u64 + MAIN_RAM_WINDOW - 1;
        assert!(CacheMap::reaches(end as u32));
    }

    #[cfg(jit)]
    #[test]
    fn a_tcm_in_load_mode_sends_every_access_to_the_helpers() {
        let mut buses = placed(true);
        let fast = unsafe { *buses.arm9().fast() };
        assert!(fast.windows.iter().all(|window| window.size == 0));
    }

    #[cfg(jit)]
    #[test]
    fn a_store_to_a_code_page_shows_in_the_window_flags() {
        let mut buses = placed(false);
        let mut bus = buses.arm9();
        mark(bus.fast());
        bus.mark_code(CodeRam::Main, 3);
        assert!(!marked(bus.fast()));
        let main = unsafe { (*bus.fast()).windows[2] };
        mark(bus.fast());
        bus.mark_code(CodeRam::Main, 4);
        assert!(marked(bus.fast()));
        let flags = unsafe { std::slice::from_raw_parts(main.code, MAIN_RAM_SIZE >> PAGE_BITS) };
        assert_eq!(flags.iter().filter(|&&flag| flag).count(), 2);
        assert!(flags[3] && flags[4]);
    }

    #[cfg(jit)]
    const SEEN: *mut u8 = std::ptr::dangling_mut();

    #[cfg(jit)]
    fn mark(table: *mut Fast) -> *mut u8 {
        unsafe {
            let bytes = (*table).windows[2].bytes;
            (*table).windows[2].bytes = SEEN;
            bytes
        }
    }

    #[cfg(jit)]
    fn marked(table: *mut Fast) -> bool {
        unsafe { (*table).windows[2].bytes == SEEN }
    }

    #[cfg(jit)]
    fn current(bus: &mut Arm9Bus) -> bool {
        let table = unsafe { *bus.fast() };
        table.debt == &raw mut bus.own.ahead
            && table.cache == bus.own.caches.pages()
            && table.windows[2].bytes.cast_const() == bus.shared.main_ram.bytes().as_ptr()
            && table.windows[1].base == bus.own.tcm.dtcm_region().base
    }

    #[cfg(jit)]
    const DTCM_REGION: CoprocessorRegister = CoprocessorRegister {
        crn: 9,
        crm: 1,
        ..CONTROL
    };

    #[cfg(jit)]
    #[test]
    fn a_run_with_no_change_reuses_the_kept_table() {
        let mut buses = placed(false);
        let first = buses.arm9().fast();
        mark(first);
        let mut bus = buses.arm9();
        CpuBus::write(&mut bus, 0x0200_0000, Width::Word, 1, Access::NonSequential);
        CpuBus::coprocessor_write(&mut bus, CONTROL, CACHES_ON);
        let again = buses.arm9().fast();
        assert_eq!(again, first);
        assert!(marked(again));
    }

    #[cfg(jit)]
    #[test]
    fn a_cp15_tcm_write_rebuilds_the_kept_table() {
        let mut buses = cached();
        let mut bus = buses.arm9();
        CpuBus::coprocessor_write(&mut bus, DTCM_REGION, 0x027C_000A);
        assert_eq!(unsafe { (*bus.fast()).windows[1].base }, 0x027C_0000);
        mark(bus.fast());
        CpuBus::coprocessor_write(&mut bus, DTCM_REGION, 0x0080_000A);
        assert!(!marked(bus.fast()));
        assert_eq!(unsafe { (*bus.fast()).windows[1].base }, 0x0080_0000);
        assert!(current(&mut bus));
    }

    #[cfg(jit)]
    #[test]
    fn a_reallocated_or_moved_memory_rebuilds_the_kept_table() {
        let mut buses = placed(false);
        mark(buses.arm9().fast());
        let old = buses.main_ram().as_ptr();
        buses = buses.clone();
        assert_ne!(buses.main_ram().as_ptr(), old);
        assert!(!marked(buses.arm9().fast()));
        assert!(current(&mut buses.arm9()));
        mark(buses.arm9().fast());
        let mut moved = Box::new(buses);
        assert!(!marked(moved.arm9().fast()));
        assert!(current(&mut moved.arm9()));
    }

    #[cfg(jit)]
    #[test]
    fn a_compiled_run_goes_on_only_while_the_machine_has_nothing_to_do() {
        use crate::nds::interrupt::Irq;
        let mut buses = Buses::default();
        assert!(buses.arm9().runs_on(false));
        buses.shared.irq[0].request(Irq::VBLANK);
        buses
            .arm9()
            .write(0x0400_0210, Width::Word, Irq::VBLANK.bits());
        buses.arm9().write(0x0400_0208, Width::Word, 1);
        assert!(!buses.arm9().runs_on(false));
        assert!(buses.arm9().runs_on(true));
        buses.arm9().write(0x0400_0208, Width::Word, 0);
        buses.arm9.halted = true;
        assert!(!buses.arm9().runs_on(false));
        buses.arm9.halted = false;
        buses.arm9().write(0x0400_01A0, Width::Half, 0x8000);
        buses.arm9().write(0x0400_01A4, Width::Word, 0x8100_0000);
        assert!(buses.slice_cut());
        assert!(!buses.arm9().runs_on(false));
    }

    struct Far<'a>(Arm9Bus<'a>);

    impl CpuBus for Far<'_> {
        fn fetch(&mut self, addr: u32, width: Width, access: Access) -> u32 {
            CpuBus::fetch(&mut self.0, addr, width, access)
        }

        fn read(&mut self, addr: u32, width: Width, access: Access) -> u32 {
            CpuBus::read(&mut self.0, addr, width, access)
        }

        fn write(&mut self, addr: u32, width: Width, value: u32, access: Access) {
            CpuBus::write(&mut self.0, addr, width, value, access)
        }

        fn idle(&mut self) {
            CpuBus::idle(&mut self.0)
        }

        fn cycles(&self, addr: u32, width: Width, access: Access) -> u32 {
            CpuBus::cycles(&self.0, addr, width, access)
        }

        fn code_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
            CpuBus::code_cycles(&mut self.0, addr, width, access)
        }

        fn timed_fetch(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
            CpuBus::timed_fetch(&mut self.0, addr, width, access)
        }

        fn quick_fetch(&mut self, addr: u32, width: Width) -> Option<(u32, u32)> {
            CpuBus::quick_fetch(&mut self.0, addr, width)
        }

        fn timed_refill(&mut self, addr: u32, width: Width) -> [(u32, u32); 2] {
            CpuBus::timed_refill(&mut self.0, addr, width)
        }

        fn thumb_words(&self) -> bool {
            CpuBus::thumb_words(&self.0)
        }

        fn dropped_fetch(&mut self, addr: u32, width: Width) {
            CpuBus::dropped_fetch(&mut self.0, addr, width)
        }

        fn timed_read(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
            CpuBus::timed_read(&mut self.0, addr, width, access)
        }

        fn settle(&mut self, cycles: u32) -> u32 {
            CpuBus::settle(&mut self.0, cycles)
        }

        fn bus_value(&self) -> u32 {
            CpuBus::bus_value(&self.0)
        }

        fn drive(&mut self, value: u32) {
            CpuBus::drive(&mut self.0, value)
        }

        fn timed_write(&mut self, addr: u32, width: Width, value: u32, access: Access) -> u32 {
            CpuBus::timed_write(&mut self.0, addr, width, value, access)
        }

        fn quick_write(&mut self, addr: u32, width: Width, value: u32) -> Option<u32> {
            CpuBus::quick_write(&mut self.0, addr, width, value)
        }

        fn timed_read_block(&mut self, addr: u32, values: &mut [u32]) -> u32 {
            CpuBus::timed_read_block(&mut self.0, addr, values)
        }

        fn timed_write_block(&mut self, addr: u32, values: &[u32]) -> u32 {
            CpuBus::timed_write_block(&mut self.0, addr, values)
        }

        fn coprocessor_read(&mut self, register: CoprocessorRegister) -> Option<u32> {
            CpuBus::coprocessor_read(&mut self.0, register)
        }

        fn coprocessor_write(&mut self, register: CoprocessorRegister, value: u32) -> bool {
            CpuBus::coprocessor_write(&mut self.0, register, value)
        }

        fn vector_base(&self) -> u32 {
            CpuBus::vector_base(&self.0)
        }
    }

    #[test]
    fn loads_step_the_same_with_or_without_the_near_reads() {
        let loads: [u32; 11] = [
            0xE590_2000,
            0xE5D0_2001,
            0xE5B0_2004,
            0xE490_2004,
            0xE59F_2000,
            0xE5B0_0004,
            0xE1D0_20B2,
            0xE1D0_20D1,
            0xE1D0_20F2,
            0xE1D0_20F1,
            0xE590_F000,
        ];
        let on = |base, virtual_size| TcmRegion {
            base,
            virtual_size,
            enabled: true,
            load_mode: false,
        };
        for mut buses in [uncached(), cached()] {
            buses.tcm_mut().place_itcm(on(0, 0x200_0000));
            buses.tcm_mut().place_dtcm(on(0x027C_0000, 0x4000));
            for (base, near) in [
                (0x0200_0101, true),
                (0x027C_0101, true),
                (0x0000_2101, true),
                (0x0300_0101, false),
                (0x0500_0101, false),
            ] {
                for (at, word) in [(base & !3, 0x89AB_CDEF), ((base & !3) + 4, 0x7654_3210)] {
                    buses.arm9().cpu_write(at, Width::Word, word);
                }
                for (width, at) in [Width::Byte, Width::Half, Width::Word]
                    .into_iter()
                    .flat_map(|width| [(width, base), (width, base & !3)])
                {
                    let quick = CpuBus::near_read(&buses.arm9(), at, width);
                    let timed =
                        CpuBus::timed_read(&mut buses.arm9(), at, width, Access::NonSequential);
                    assert_eq!(quick, near.then_some(timed), "{at:08X}");
                }
            }
            for (entry, near) in [
                (0x0200_0000, true),
                (0x0000_1000, true),
                (0x0300_0000, false),
            ] {
                for width in [Width::Half, Width::Word] {
                    for at in [entry, entry + 2] {
                        let quick = CpuBus::near_fetch(&buses.arm9(), at, width);
                        let timed = buses.arm9().timed_fetch(at, width, Access::Sequential);
                        assert_eq!(quick, near.then_some(timed), "{at:08X}");
                    }
                }
                for (op, base) in loads.into_iter().flat_map(|op| {
                    [
                        0x0200_0101,
                        0x027C_0101,
                        0x0000_2101,
                        0x0300_0101,
                        0x0500_0101,
                    ]
                    .map(|base| (op, base))
                }) {
                    let mut run = |far: bool| {
                        let mut cpu = arm9_at(&mut buses, entry, &[op, NOP, NOP, NOP], base);
                        let mut step = |cpu: &mut Arm9| {
                            if far {
                                cpu.step(&mut Far(buses.arm9()))
                            } else {
                                cpu.step(&mut buses.arm9())
                            }
                        };
                        let cycles = [step(&mut cpu), step(&mut cpu)];
                        (cycles, cpu.state())
                    };
                    let quick = run(false);
                    assert_eq!(run(true), quick, "{op:08X} at {base:08X} from {entry:08X}");
                }
            }
        }
    }

    #[test]
    fn thumb_loads_step_the_same_with_or_without_the_near_reads() {
        let loads: [u16; 11] = [
            0x6802, 0x6842, 0x7842, 0x8842, 0x4A00, 0x5642, 0x5E42, 0x5A42, 0x5842, 0x5C42, 0x9A00,
        ];
        let bases = [
            0x0200_0101,
            0x027C_0101,
            0x0000_2101,
            0x0300_0101,
            0x0500_0101,
        ];
        let on = |base, virtual_size| TcmRegion {
            base,
            virtual_size,
            enabled: true,
            load_mode: false,
        };
        for mut buses in [uncached(), cached()] {
            buses.tcm_mut().place_itcm(on(0, 0x200_0000));
            buses.tcm_mut().place_dtcm(on(0x027C_0000, 0x4000));
            for base in bases {
                for (at, word) in [(base & !3, 0x89AB_CDEF), ((base & !3) + 4, 0x7654_3210)] {
                    buses.arm9().cpu_write(at, Width::Word, word);
                }
            }
            for entry in [0x0200_0000, 0x0200_0002, 0x0000_1000, 0x0300_0000] {
                for (op, base) in loads
                    .into_iter()
                    .flat_map(|op| bases.map(|base| (op, base)))
                {
                    for code in [
                        [op, THUMB_NOP, THUMB_NOP, THUMB_NOP],
                        [THUMB_NOP, op, THUMB_NOP, THUMB_NOP],
                    ] {
                        let mut run = |far: bool| {
                            buses.arm9().set_ahead(0);
                            let mut cpu = thumb_at(&mut buses, entry, &code, base, 2);
                            let mut step = |cpu: &mut Arm9| {
                                if far {
                                    cpu.step(&mut Far(buses.arm9()))
                                } else {
                                    cpu.step(&mut buses.arm9())
                                }
                            };
                            let cycles = [step(&mut cpu), step(&mut cpu), step(&mut cpu)];
                            (cycles, cpu.state())
                        };
                        let quick = run(false);
                        assert_eq!(
                            run(true),
                            quick,
                            "{code:04X?} at {base:08X} from {entry:08X}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn stores_step_the_same_with_or_without_the_near_fetches() {
        let arm: [u32; 7] = [
            0xE580_0000,
            0xE5C0_0001,
            0xE5A0_0004,
            0xE480_0004,
            0xE1C0_00B2,
            0xE58F_0000,
            0xE5AF_0004,
        ];
        let thumb: [u16; 5] = [0x6000, 0x7040, 0x8040, 0x5040, 0x9000];
        let bases = [
            0x0200_0101,
            0x027C_0101,
            0x0000_2101,
            0x0300_0101,
            0x0500_0101,
        ];
        let on = |base, virtual_size| TcmRegion {
            base,
            virtual_size,
            enabled: true,
            load_mode: false,
        };
        for mut buses in [uncached(), cached()] {
            buses.tcm_mut().place_itcm(on(0, 0x200_0000));
            buses.tcm_mut().place_dtcm(on(0x027C_0000, 0x4000));
            for entry in [0x0200_0000, 0x0200_0002, 0x0000_1000, 0x0300_0000] {
                for base in bases {
                    let words = [base & !3, (base & !3) + 4];
                    let mut run = |far: bool, code: Result<u32, [u16; 4]>| {
                        buses.arm9().set_ahead(0);
                        for at in (0..4).map(|index| (entry & !3) + 4 * index) {
                            buses.arm9().cpu_write(at, Width::Word, 0);
                        }
                        for at in words {
                            buses.arm9().cpu_write(at, Width::Word, 0x89AB_CDEF);
                        }
                        let mut cpu = match code {
                            Ok(op) => arm9_at(&mut buses, entry & !3, &[op, NOP, NOP, NOP], base),
                            Err(code) => thumb_at(&mut buses, entry, &code, base, 2),
                        };
                        let mut step = |cpu: &mut Arm9| {
                            if far {
                                cpu.step(&mut Far(buses.arm9()))
                            } else {
                                cpu.step(&mut buses.arm9())
                            }
                        };
                        let cycles = [step(&mut cpu), step(&mut cpu), step(&mut cpu)];
                        let stored =
                            words.map(|at| buses.arm9().cpu_read(at, Width::Word, Fetch::Data));
                        (cycles, cpu.state(), stored)
                    };
                    let codes = arm
                        .map(Ok)
                        .into_iter()
                        .chain(thumb.into_iter().flat_map(|op| {
                            [
                                Err([op, THUMB_NOP, THUMB_NOP, THUMB_NOP]),
                                Err([THUMB_NOP, op, THUMB_NOP, THUMB_NOP]),
                            ]
                        }));
                    for code in codes {
                        let quick = run(false, code);
                        assert_eq!(
                            run(true, code),
                            quick,
                            "{code:04X?} at {base:08X} from {entry:08X}"
                        );
                    }
                }
            }
        }
    }

    fn seed_code(buses: &mut Buses, at: u32, thumb: bool) {
        let (under, over) = if thumb {
            (
                [0x2212_2111, 0x2414_2313, 0x2616_2515],
                [0x2202_2101, 0x2404_2303, 0x2606_2505],
            )
        } else {
            (
                [0xE3A0_1011, 0xE3A0_2012, 0xE3A0_3013],
                [0xE3A0_1001, 0xE3A0_2002, 0xE3A0_3003],
            )
        };
        for (index, (under, over)) in under.into_iter().zip(over).enumerate() {
            let at = (at & !3).wrapping_add(4 * index as u32);
            buses.arm9().write(at, Width::Word, under);
            buses.arm9().cpu_write(at, Width::Word, over);
        }
    }

    #[test]
    fn branches_step_the_same_with_or_without_the_near_refills() {
        let arm_targets = [
            0x0200_0100,
            0x0200_0118,
            0x0200_011C,
            0x01FF_FFFC,
            0x0000_2000,
            0x0000_201C,
            0x027C_0100,
            0x0300_0200,
        ];
        let thumb_deltas: [(i32, bool); 13] = [
            (0x100, false),
            (0x118, false),
            (0x11A, false),
            (-0x40, false),
            (0x7FE, false),
            (-6, false),
            (-8, false),
            (0x18, true),
            (0x1A, true),
            (0xFE, true),
            (-6, true),
            (-8, true),
            (-0x100, true),
        ];
        let on = |base, virtual_size| TcmRegion {
            base,
            virtual_size,
            enabled: true,
            load_mode: false,
        };
        for mut buses in [uncached(), cached()] {
            buses.tcm_mut().place_itcm(on(0, 0x200_0000));
            buses.tcm_mut().place_dtcm(on(0x027C_0000, 0x4000));
            for entry in [0x0200_0000, 0x0200_0002, 0x0000_1000, 0x0300_0000] {
                let arm = arm_targets.into_iter().flat_map(|target: u32| {
                    let offset = target.wrapping_sub((entry & !3) + 8);
                    let reach = offset.wrapping_add(1 << 25) < 1 << 26;
                    let offset = (offset >> 2) & 0x00FF_FFFF;
                    [0xEA00_0000 | offset, 0xEB00_0000 | offset]
                        .into_iter()
                        .filter(move |_| reach)
                        .map(move |op| (Ok(op), target))
                });
                let thumb = thumb_deltas.into_iter().flat_map(|(delta, conditional)| {
                    let target = (entry + 4).wrapping_add_signed(delta);
                    let (op, count) = if conditional {
                        (0xD000 | ((delta >> 1) as u16 & 0xFF), 2)
                    } else {
                        (0xE000 | ((delta >> 1) as u16 & 0x7FF), 1)
                    };
                    [op, op | 0x100]
                        .into_iter()
                        .take(count)
                        .map(move |op| (Err([op, THUMB_NOP, THUMB_NOP, THUMB_NOP]), target))
                });
                for (code, target) in arm.chain(thumb) {
                    let mut run = |far: bool| {
                        buses.arm9().set_ahead(0);
                        seed_code(&mut buses, target, code.is_err());
                        let mut cpu = match code {
                            Ok(op) => arm9_at(&mut buses, entry & !3, &[op, NOP, NOP, NOP], 0),
                            Err(code) => thumb_at(&mut buses, entry, &code, 0, 0),
                        };
                        let mut step = |cpu: &mut Arm9| {
                            if far {
                                cpu.step(&mut Far(buses.arm9()))
                            } else {
                                cpu.step(&mut buses.arm9())
                            }
                        };
                        let cycles = [
                            step(&mut cpu),
                            step(&mut cpu),
                            step(&mut cpu),
                            step(&mut cpu),
                        ];
                        (cycles, format!("{cpu:?}"))
                    };
                    let quick = run(false);
                    assert_eq!(
                        run(true),
                        quick,
                        "{code:04X?} to {target:08X} from {entry:08X}"
                    );
                }
            }
        }
    }

    #[test]
    fn exchanges_step_the_same_with_or_without_the_near_refills() {
        let targets = [
            0x0200_0100,
            0x0200_0101,
            0x0200_0102,
            0x0200_0103,
            0x0200_011C,
            0x0200_011F,
            0x01FF_FFFC,
            0x01FF_FFFF,
            0x0000_2000,
            0x0000_201F,
            0x027C_0100,
            0x027C_0101,
            0x0300_0201,
        ];
        let long_deltas: [i32; 8] = [0x100, 0x11A, 0x11C, -0x40, 0x2000, -0x10_0000, -6, -8];
        let on = |base, virtual_size| TcmRegion {
            base,
            virtual_size,
            enabled: true,
            load_mode: false,
        };
        for mut buses in [uncached(), cached()] {
            buses.tcm_mut().place_itcm(on(0, 0x200_0000));
            buses.tcm_mut().place_dtcm(on(0x027C_0000, 0x4000));
            for entry in [0x0200_0000u32, 0x0200_0002, 0x0000_1000, 0x0300_0000] {
                let exchanges = targets.into_iter().flat_map(|target: u32| {
                    [
                        (Ok(0xE12F_FF10), target, target),
                        (Err([0x4700, THUMB_NOP]), target, target),
                        (Err([0x4780, THUMB_NOP]), target, target),
                        (Err([0x4686, 0xF800]), target | 1, target | 1),
                    ]
                });
                let long = long_deltas.into_iter().flat_map(|delta| {
                    let high = 0xF000 | ((delta >> 12) as u16 & 0x7FF);
                    let low = (delta >> 1) as u16 & 0x7FF;
                    let target = (entry + 4).wrapping_add_signed(delta);
                    [
                        (Err([high, 0xF800 | low]), 0, target | 1),
                        (Err([high, 0xE800 | low]), 0, target & !3),
                    ]
                });
                for (code, r0, landing) in exchanges.chain(long) {
                    if landing >= 0x0400_0000 {
                        continue;
                    }
                    let mut run = |far: bool| {
                        buses.arm9().set_ahead(0);
                        seed_code(&mut buses, landing & !1, landing & 1 != 0);
                        let mut cpu = match code {
                            Ok(op) => arm9_at(&mut buses, entry & !3, &[op, NOP, NOP, NOP], r0),
                            Err([first, second]) => {
                                let code = [first, second, THUMB_NOP, THUMB_NOP];
                                thumb_at(&mut buses, entry, &code, r0, 0)
                            }
                        };
                        let mut step = |cpu: &mut Arm9| {
                            if far {
                                cpu.step(&mut Far(buses.arm9()))
                            } else {
                                cpu.step(&mut buses.arm9())
                            }
                        };
                        let cycles = [
                            step(&mut cpu),
                            step(&mut cpu),
                            step(&mut cpu),
                            step(&mut cpu),
                        ];
                        (cycles, format!("{cpu:?}"))
                    };
                    let quick = run(false);
                    assert_eq!(
                        run(true),
                        quick,
                        "{code:04X?} with r0 {r0:08X} from {entry:08X}"
                    );
                }
            }
        }
    }

    #[test]
    fn blocks_step_the_same_with_or_without_the_near_words() {
        let arm: [u32; 17] = [
            0xE920_400E,
            0xE8B0_800E,
            0xE890_000E,
            0xE8A0_0007,
            0xE8B0_0003,
            0xE930_0001,
            0xE9B0_0006,
            0xE800_8002,
            0xE8B0_FFFF,
            0xE8B0_0000,
            0xE8A0_0000,
            0xE8B1_0003,
            0xE8A1_0003,
            0xE99F_0006,
            0xE8D0_0006,
            0xE92D_4FF0,
            0xE8BD_8FF0,
        ];
        let thumb: [u16; 12] = [
            0xB5F0, 0xB403, 0xBD03, 0xBC04, 0xBD00, 0xC006, 0xC806, 0xC803, 0xC903, 0xC103, 0xC800,
            0xC000,
        ];
        let bases: [u32; 9] = [
            0x0200_0101,
            0x0200_0FF8,
            0x023F_FFF8,
            0x027C_0101,
            0x027C_0008,
            0x027C_3FF8,
            0x0000_2101,
            0x0300_0101,
            0xFFFF_FFF8,
        ];
        let landings: [u32; 6] = [
            0x0200_8100,
            0x0200_8101,
            0x0200_8102,
            0x0000_4001,
            0x027C_2100,
            0x0300_4200,
        ];
        let on = |base, virtual_size| TcmRegion {
            base,
            virtual_size,
            enabled: true,
            load_mode: false,
        };
        let loads_pc = |code: Result<u32, [u16; 4]>| match code {
            Ok(op) => op & 0x0010_8000 == 0x0010_8000,
            Err(halves) => halves.iter().any(|half| half & 0xFF00 == 0xBD00),
        };
        for mut buses in [uncached(), cached()] {
            buses.tcm_mut().place_itcm(on(0, 0x200_0000));
            buses.tcm_mut().place_dtcm(on(0x027C_0000, 0x4000));
            for entry in [0x0200_0000, 0x0200_0002, 0x0000_1000, 0x0300_0000] {
                for base in bases {
                    let first = (base & !3).wrapping_sub(0x44);
                    let mut run = |far: bool, code: Result<u32, [u16; 4]>, landing: u32| {
                        buses.arm9().set_ahead(0);
                        for index in 0..35 {
                            let at = landing.wrapping_add(0x10 * index);
                            seed_code(&mut buses, at & !1, landing & 1 != 0);
                        }
                        for index in 0..35 {
                            let at = first.wrapping_add(4 * index);
                            let value = landing.wrapping_add(0x10 * index);
                            buses.arm9().cpu_write(at, Width::Word, value);
                        }
                        let cpu = match code {
                            Ok(op) => arm9_at(&mut buses, entry & !3, &[op, NOP, NOP, NOP], base),
                            Err(code) => thumb_at(&mut buses, entry, &code, base, base),
                        };
                        let mut state = cpu.state();
                        for (index, reg) in state.r.iter_mut().enumerate().take(15).skip(2) {
                            *reg = 0x0101_0101 * index as u32;
                        }
                        state.r[1] = base;
                        state.r[13] = base;
                        state.svc = [base, 0x5E5E_5E5E];
                        let mut cpu = Arm9::from_state(&state);
                        let mut step = |cpu: &mut Arm9| {
                            if far {
                                cpu.step(&mut Far(buses.arm9()))
                            } else {
                                cpu.step(&mut buses.arm9())
                            }
                        };
                        let cycles = [
                            step(&mut cpu),
                            step(&mut cpu),
                            step(&mut cpu),
                            step(&mut cpu),
                        ];
                        let stored: Vec<u32> = (0..35)
                            .map(|index| {
                                let at = first.wrapping_add(4 * index);
                                buses.arm9().cpu_read(at, Width::Word, Fetch::Data)
                            })
                            .collect();
                        (cycles, format!("{cpu:?}"), stored)
                    };
                    let codes = arm
                        .map(Ok)
                        .into_iter()
                        .chain(thumb.into_iter().flat_map(|op| {
                            [
                                Err([op, THUMB_NOP, THUMB_NOP, THUMB_NOP]),
                                Err([THUMB_NOP, op, THUMB_NOP, THUMB_NOP]),
                            ]
                        }));
                    for code in codes {
                        let tried = if loads_pc(code) {
                            &landings[..]
                        } else {
                            &landings[..1]
                        };
                        for &landing in tried {
                            let quick = run(false, code, landing);
                            assert_eq!(
                                run(true, code, landing),
                                quick,
                                "{code:04X?} landing {landing:08X} at {base:08X} from {entry:08X}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn io_and_bios_accesses_cost_and_act_as_the_plain_bus_does() {
        let ops = [
            (0x0400_0208, Width::Word, 1, Access::NonSequential),
            (0x0400_0210, Width::Word, 0x0004_0001, Access::Sequential),
            (0x0400_0280, Width::Half, 2, Access::NonSequential),
            (0x0400_0290, Width::Word, 1000, Access::Sequential),
            (0x0400_0298, Width::Word, 7, Access::NonSequential),
            (0x0400_0180, Width::Half, 0x0F00, Access::NonSequential),
            (0x0400_0181, Width::Byte, 0x05, Access::Sequential),
            (0x0400_0004, Width::Half, 0x0018, Access::Sequential),
            (0x0400_0300, Width::Byte, 0x03, Access::NonSequential),
            (0x0400_0440, Width::Word, 1, Access::NonSequential),
            (0x0400_0454, Width::Word, 0, Access::Sequential),
        ];
        let bios: Vec<u8> = (0..0x1000u32).map(|index| (index * 7) as u8).collect();
        let (mut fast, mut plain) = (Buses::default(), Buses::default());
        fast.load_arm9_bios(&bios);
        plain.load_arm9_bios(&bios);
        for (addr, width, value, access) in ops {
            let cycles = fast.arm9().timed_write(addr, width, value, access);
            let mut bus = plain.arm9();
            let expected = bus.cycles(addr, width, access);
            bus.write(addr, width, value);
            assert_eq!(cycles, expected, "store {addr:08X}");
            assert_eq!(fast.arm9().take_stall(), plain.arm9().take_stall());
            assert_eq!(fast.arm9().take_attention(), plain.arm9().take_attention());
        }
        for (addr, width, _, access) in ops.into_iter().chain([
            (0x0400_02A0, Width::Word, 0, Access::NonSequential),
            (0x0400_0600, Width::Word, 0, Access::Sequential),
            (0x0400_0130, Width::Half, 0, Access::NonSequential),
        ]) {
            let loaded = fast.arm9().timed_read(addr, width, access);
            let mut bus = plain.arm9();
            let expected = (bus.cycles(addr, width, access), bus.read(addr, width));
            assert_eq!(loaded, expected, "load {addr:08X}");
            assert_eq!(fast.arm9().take_attention(), plain.arm9().take_attention());
        }
        for (addr, width, access) in [
            (BIOS_BASE, Width::Word, Access::NonSequential),
            (BIOS_BASE + 0x0124, Width::Word, Access::Sequential),
            (BIOS_BASE + 0x0FFE, Width::Half, Access::Sequential),
            (BIOS_BASE + 0x0040, Width::Half, Access::NonSequential),
        ] {
            let fetched = fast.arm9().timed_fetch(addr, width, access);
            let mut bus = plain.arm9();
            let expected = (bus.code_cycles(addr, width, access), bus.read(addr, width));
            assert_eq!(fetched, expected, "fetch {addr:08X}");
            assert_ne!(fetched.1, 0, "fetch {addr:08X}");
        }
    }

    #[test]
    fn io_runs_match_single_words() {
        let on = |base, virtual_size, load_mode| TcmRegion {
            base,
            virtual_size,
            enabled: true,
            load_mode,
        };
        let placed = |itcm: Option<TcmRegion>, dtcm: TcmRegion| {
            let mut buses = Buses::default();
            if let Some(itcm) = itcm {
                buses.tcm_mut().place_itcm(itcm);
            }
            buses.tcm_mut().place_dtcm(dtcm);
            buses
        };
        let placements = [
            Buses::default(),
            placed(None, on(0x0400_0000, 0x4000, false)),
            placed(None, on(0x0400_02A0, 0x4000, false)),
            placed(None, on(0x03FF_C298, 0x4000, false)),
            placed(None, on(0x0400_0000, 0x4000, true)),
            placed(Some(on(0, 1 << 32, false)), on(0x027C_0000, 0x4000, false)),
            placed(
                Some(on(0, 0x0800_0000, true)),
                on(0x027C_0000, 0x4000, false),
            ),
        ];
        for buses in placements {
            for (addr, words) in [
                (0x0400_0280, 8),
                (0x0400_0290, 4),
                (0x0400_0298, 4),
                (0x0400_02A0, 4),
                (0x0400_0440, 6),
                (0x0400_0000, 16),
                (0x0400_0600, 3),
                (0x0400_0100, 4),
                (0x0400_0180, 4),
                (0x0400_0208, 4),
                (0x0410_0000, 2),
                (0x04FF_FFF0, 4),
                (0x04FF_FFF8, 4),
            ] {
                block_matches_single_words(&buses, addr, words);
                let mut block = buses.clone();
                let mut single = buses.clone();
                let values: Vec<u32> = (0..words as u32).map(|v| v * 3 + 1).collect();
                assert_eq!(
                    CpuBus::timed_write_block(&mut block.arm9(), addr, &values),
                    write_words(&mut single.arm9(), addr, &values),
                    "write {addr:08X}"
                );
                let flags = |buses: &mut Buses| {
                    let mut bus = buses.arm9();
                    (bus.take_stall(), bus.take_attention(), bus.take_events())
                };
                assert_eq!(flags(&mut block), flags(&mut single), "flags {addr:08X}");
                let (mut fast, mut slow) = ([0; 16], [0; 16]);
                assert_eq!(
                    (
                        CpuBus::timed_read_block(&mut block.arm9(), addr, &mut fast[..words]),
                        fast
                    ),
                    (
                        read_words(&mut single.arm9(), addr, &mut slow[..words]),
                        slow
                    ),
                    "read {addr:08X}"
                );
                assert_eq!(flags(&mut block), flags(&mut single), "flags {addr:08X}");
            }
        }
    }
}
