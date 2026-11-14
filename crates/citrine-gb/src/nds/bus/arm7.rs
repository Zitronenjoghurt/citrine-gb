mod io;

use super::{ARM7_BIOS_WINDOW, Arm7Memory, Shared, Waits, read_bios};
#[cfg(jit)]
use super::{ARM7_WRAM_SIZE, MAIN_RAM_SIZE};
use crate::gba::cpu::{CpuBus, Jump, read_words, write_words};
use crate::gba::open_bus::lane;
use crate::gba::wait::{Access, Width};
#[cfg(jit)]
use crate::jit::{CodeRam, CodeSpan, Fast, FastBus, JitBus, PAGE_SIZE, Page, Spec, Stamp, WINDOWS};
use crate::nds::Cpu;
use crate::nds::dma::{Area, Dma, DmaBus, Timing};
use crate::nds::sound::mixer::SoundMemory;
use crate::nds::vram::Purpose;
use crate::nds::wifi;
#[cfg(jit)]
use crate::nds::wram::SHARED_WRAM_SIZE;

const ARM7_WRAM_BASE: u32 = 0x0380_0000;
const FAST_CYCLES: u32 = 1;
const MAIN_RAM_WAITS: Waits = Waits::new(8, 1, 9, 2);
const VRAM_WAITS: Waits = Waits::new(1, 1, 2, 2);
const GBA_SLOT_WAITS: Waits = Waits::new(13, 6, 19, 12);
const WIFI_END: u32 = wifi::BASE + wifi::SIZE;
const WIFI_WS1: u32 = 0x8000;
const WIFI_FIRST_WAITS: [u32; 4] = [10, 8, 6, 18];
const WIFI_POWER: u16 = 1 << 1;
const CODE_PAGE_BITS: u32 = 12;
const CODE_PAGE_MASK: u32 = (1 << CODE_PAGE_BITS) - 1;
const CODE_PAGES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    Bios,
    MainRam,
    SharedWram,
    Wram,
    Io,
    Wifi,
    Vram,
    GbaSlot,
    Unmapped,
}

impl Region {
    fn of(addr: u32) -> Self {
        match addr >> 24 {
            0x00 if (addr as usize) < ARM7_BIOS_WINDOW => Self::Bios,
            0x02 => Self::MainRam,
            0x03 if addr < ARM7_WRAM_BASE => Self::SharedWram,
            0x03 => Self::Wram,
            0x04 if (wifi::BASE..WIFI_END).contains(&addr) => Self::Wifi,
            0x04 => Self::Io,
            0x06 => Self::Vram,
            0x08..=0x0A => Self::GbaSlot,
            _ => Self::Unmapped,
        }
    }

    #[inline(always)]
    fn cycles(self, width: Width, access: Access) -> u32 {
        u32::from(REGION_CYCLES[self as usize][width as usize * 2 + access as usize])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageRam {
    Bios,
    Main,
    Wram,
    Shared,
}

#[derive(Debug, Clone, Copy)]
struct CodePage {
    page: u32,
    region: Region,
    ram: PageRam,
    start: u32,
    cycles: [u8; 6],
}

impl Default for CodePage {
    fn default() -> Self {
        Self {
            page: u32::MAX,
            region: Region::Unmapped,
            ram: PageRam::Main,
            start: 0,
            cycles: FAST_ROW,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CodePages {
    now: CodePage,
    known: [CodePage; CODE_PAGES],
    layout: u8,
}

impl CodePages {
    fn new(layout: u8) -> Self {
        Self {
            now: CodePage::default(),
            known: [CodePage::default(); CODE_PAGES],
            layout,
        }
    }

    #[inline(always)]
    fn slot(page: u32) -> usize {
        (page.wrapping_mul(0x9E37_79B9) >> 26) as usize
    }
}

impl Default for CodePages {
    fn default() -> Self {
        Self::new(0)
    }
}

const fn cycle_row(waits: Waits) -> [u8; 6] {
    let [half_n, half_s] = waits.half;
    let [word_n, word_s] = waits.word;
    [
        half_n as u8,
        half_s as u8,
        half_n as u8,
        half_s as u8,
        word_n as u8,
        word_s as u8,
    ]
}

const FAST_ROW: [u8; 6] = [FAST_CYCLES as u8; 6];

const REGION_CYCLES: [[u8; 6]; 9] = [
    FAST_ROW,
    cycle_row(MAIN_RAM_WAITS),
    FAST_ROW,
    FAST_ROW,
    FAST_ROW,
    FAST_ROW,
    cycle_row(VRAM_WAITS),
    cycle_row(GBA_SLOT_WAITS),
    FAST_ROW,
];

#[derive(Debug, Clone, Copy)]
struct Fetch {
    addr: u32,
    width: Width,
    region: Region,
}

impl Fetch {
    const NONE: Self = Self {
        addr: 0,
        width: Width::Byte,
        region: Region::Bios,
    };
}

const CODE: u16 = 1 << 0;
const TARGET: u16 = 1 << 1;
const REFILLING: u16 = 1 << 2;
const LOADED: u16 = 1 << 3;
const STORED: u16 = 1 << 4;
const DATA: u16 = 1 << 5;
const DATA_IN_MAIN_RAM: u16 = 1 << 6;
const IDLE: u16 = 1 << 7;
const BRANCH: u16 = 1 << 8;
const BLOCK_LOAD: u16 = 1 << 9;

#[derive(Debug, Clone, Copy)]
struct Charge {
    code: Fetch,
    target: Fetch,
    refill: u32,
    data: u32,
    idle: u32,
    flags: u16,
}

impl Charge {
    const NONE: Self = Self {
        code: Fetch::NONE,
        target: Fetch::NONE,
        refill: 0,
        data: 0,
        idle: 0,
        flags: 0,
    };

    #[inline(always)]
    fn code(&mut self, fetch: Fetch, cycles: u32) {
        if self.flags & REFILLING != 0 {
            if self.flags & TARGET == 0 {
                self.target = fetch;
                self.flags |= TARGET;
            }
            self.refill += cycles;
        } else if self.flags & CODE == 0 {
            self.code = fetch;
            self.flags |= CODE;
        }
    }

    #[inline(always)]
    fn run(&mut self, region: Region, cycles: u32, flag: u16) {
        self.data = cycles;
        self.flags = self.flags & !DATA_IN_MAIN_RAM
            | DATA
            | flag
            | if region == Region::MainRam {
                DATA_IN_MAIN_RAM
            } else {
                0
            };
    }
}

fn overlapped(code: u32, data: u32) -> u32 {
    (code + data).saturating_sub(3).max(code).max(data)
}

pub struct Arm7Bus<'a> {
    shared: &'a mut Shared,
    own: &'a mut Arm7Memory,
    charge: Charge,
    #[cfg(jit)]
    debt: u32,
}

impl<'a> Arm7Bus<'a> {
    pub(super) fn new(shared: &'a mut Shared, own: &'a mut Arm7Memory) -> Self {
        if own.code.layout != shared.wram.control() {
            own.code = CodePages::new(shared.wram.control());
        }
        Self {
            shared,
            own,
            charge: Charge::NONE,
            #[cfg(jit)]
            debt: 0,
        }
    }

    #[inline(always)]
    fn code_cost(&self, fetch: Fetch, access: Access) -> u32 {
        self.region_cycles(fetch.region, fetch.addr, fetch.width, access)
    }

    #[inline(always)]
    fn region_cycles(&self, region: Region, addr: u32, width: Width, access: Access) -> u32 {
        match region {
            Region::Wifi => self.wifi_waits(addr).cycles(width, access),
            region => region.cycles(width, access),
        }
    }

    #[inline(always)]
    fn charge_code(&mut self, fetch: Fetch, cycles: u32) {
        self.charge.code(fetch, cycles);
    }

    #[inline(always)]
    fn charge_data(&mut self, region: Region, cycles: u32, access: Access) {
        let charge = &mut self.charge;
        match access {
            Access::NonSequential => {
                charge.data = cycles;
                charge.flags = charge.flags & !DATA_IN_MAIN_RAM
                    | DATA
                    | if region == Region::MainRam {
                        DATA_IN_MAIN_RAM
                    } else {
                        0
                    };
            }
            Access::Sequential => {
                charge.data += cycles;
                charge.flags |= DATA;
            }
        }
    }

    #[inline(always)]
    fn word_run(&self, addr: u32, words: usize) -> Option<(Region, u32)> {
        let last = addr.checked_add(4 * (words as u32 - 1))?;
        let region = match (addr >> 24, last >> 24) {
            (0x02, 0x02) => Region::MainRam,
            (0x03, 0x03) if addr >= ARM7_WRAM_BASE => Region::Wram,
            _ => return None,
        };
        let cycles = region.cycles(Width::Word, Access::NonSequential)
            + (words as u32 - 1) * region.cycles(Width::Word, Access::Sequential);
        Some((region, cycles))
    }

    #[inline(always)]
    fn charge_run(&mut self, region: Region, cycles: u32, flag: u16) {
        self.charge.run(region, cycles, flag);
    }

    fn charged(&self) -> u32 {
        let charge = &self.charge;
        let flags = charge.flags;
        if flags & BRANCH != 0 || flags & CODE == 0 {
            return charge.refill;
        }
        let code = if flags & (BLOCK_LOAD | TARGET) == BLOCK_LOAD | TARGET {
            charge.target
        } else {
            charge.code
        };
        if flags & (LOADED | STORED | IDLE) == 0 {
            return self.code_cost(code, Access::Sequential) + charge.refill;
        }
        let numc = self.code_cost(code, Access::NonSequential);
        let code_in_main_ram = code.region == Region::MainRam;
        let data_in_main_ram = flags & DATA_IN_MAIN_RAM != 0;
        let numd = charge.data;
        let base = if flags & LOADED != 0 {
            match (data_in_main_ram, code_in_main_ram) {
                (true, true) => numc + numd,
                (true, false) => overlapped(numc + 1, numd),
                (false, true) => overlapped(numc, numd + 1),
                (false, false) => numc + numd + 1,
            }
        } else if flags & STORED != 0 {
            if data_in_main_ram == code_in_main_ram {
                numc + numd
            } else {
                overlapped(numc, numd)
            }
        } else {
            numc + charge.idle
        };
        base + charge.refill
    }

    #[inline(never)]
    fn settle_touched(&mut self, cycles: u32) -> u32 {
        let charged = if cycles == 0 { 0 } else { self.charged() };
        self.charge = Charge::NONE;
        charged
    }

    pub fn read(&mut self, addr: u32, width: Width) -> u32 {
        self.read_region(Region::of(addr), addr, width)
    }

    #[inline(never)]
    fn timed_fetch_elsewhere(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        let page = addr >> CODE_PAGE_BITS;
        let slot = CodePages::slot(page);
        let known = self.own.code.known[slot];
        if known.page == page {
            self.own.code.now = known;
            return self.code_fetch(known, addr, width, access);
        }
        let code = self.code_page(addr);
        self.own.code.now = code;
        if code.page == page {
            self.own.code.known[slot] = code;
        }
        let region = Region::of(addr);
        let cycles = self.region_cycles(region, addr, width, access);
        self.charge_code(
            Fetch {
                addr,
                width,
                region,
            },
            cycles,
        );
        let value = self.read_region(region, addr & !(width.bytes() as u32 - 1), width);
        (cycles, value)
    }

    fn code_page(&self, addr: u32) -> CodePage {
        let page = addr >> CODE_PAGE_BITS;
        let first = page << CODE_PAGE_BITS;
        let region = Region::of(first);
        let size = (CODE_PAGE_MASK + 1) as usize;
        let wram = || self.own.wram.inside(first as usize, size);
        let (ram, start) = match region {
            Region::Bios => (PageRam::Bios, Some(first as usize)),
            Region::MainRam => (
                PageRam::Main,
                self.shared.main_ram.inside(first as usize, size),
            ),
            Region::Wram => (PageRam::Wram, wram()),
            Region::SharedWram => match self.shared.wram.code_offset(Cpu::Arm7, first as usize) {
                Some(start) => (PageRam::Shared, self.shared.wram.ram().inside(start, size)),
                None => (PageRam::Wram, wram()),
            },
            _ => return CodePage::default(),
        };
        let Some(start) = start else {
            return CodePage::default();
        };
        CodePage {
            page,
            region,
            ram,
            start: start as u32,
            cycles: REGION_CYCLES[region as usize],
        }
    }

    #[inline(always)]
    fn code_fetch(
        &mut self,
        code: CodePage,
        addr: u32,
        width: Width,
        access: Access,
    ) -> (u32, u32) {
        let region = code.region;
        let cycles = u32::from(code.cycles[width as usize * 2 + access as usize]);
        self.charge_code(
            Fetch {
                addr,
                width,
                region,
            },
            cycles,
        );
        (cycles, self.code_value(code, addr, width))
    }

    #[inline(always)]
    fn code_value(&self, code: CodePage, addr: u32, width: Width) -> u32 {
        let offset = (code.start | (addr & CODE_PAGE_MASK)) as usize;
        match code.ram {
            PageRam::Bios => read_bios(&self.own.bios, offset, width),
            PageRam::Main => self.shared.main_ram.read_inside(offset, width),
            PageRam::Wram => self.own.wram.read_inside(offset, width),
            PageRam::Shared => self.shared.wram.ram().read_inside(offset, width),
        }
    }

    #[inline(always)]
    fn read_region(&mut self, region: Region, addr: u32, width: Width) -> u32 {
        let offset = addr as usize;
        match region {
            Region::Bios => read_bios(&self.own.bios, offset, width),
            Region::MainRam => self.shared.main_ram.read(offset, width),
            Region::SharedWram => match self.shared.wram.read(Cpu::Arm7, offset, width) {
                Some(value) => value,
                None => self.own.wram.read(offset, width),
            },
            Region::Wram => self.own.wram.read(offset, width),
            Region::Io => self.read_io(addr, width),
            Region::Wifi => self.read_wifi(offset, width),
            Region::Vram => self.shared.vram.read(Purpose::Arm7, offset, width),
            Region::GbaSlot => self.shared.read_gba_slot(true, addr, width),
            Region::Unmapped => 0,
        }
    }

    pub fn write(&mut self, addr: u32, width: Width, value: u32) {
        self.write_region(Region::of(addr), addr, width, value);
    }

    #[inline(always)]
    fn write_region(&mut self, region: Region, addr: u32, width: Width, value: u32) {
        let offset = addr as usize;
        match region {
            Region::MainRam => self.shared.main_ram.write(offset, width, value),
            Region::SharedWram => {
                if !self.shared.wram.write(Cpu::Arm7, offset, width, value) {
                    self.own.wram.write(offset, width, value);
                }
            }
            Region::Wram => self.own.wram.write(offset, width, value),
            Region::Io => self.write_io(addr, width, value),
            Region::Wifi => self.write_wifi(offset, width, value),
            Region::Vram => self.shared.vram.write(Purpose::Arm7, offset, width, value),
            Region::Bios | Region::GbaSlot | Region::Unmapped => {}
        }
    }

    fn read_wifi(&mut self, offset: usize, width: Width) -> u32 {
        self.own.attention = true;
        self.own.pending = true;
        let wifi = &mut self.own.wifi;
        match width {
            Width::Byte => u32::from(wifi.read8(offset)),
            Width::Half => u32::from(wifi.read16(offset)),
            Width::Word => wifi.read32(offset),
        }
    }

    fn write_wifi(&mut self, offset: usize, width: Width, value: u32) {
        self.own.attention = true;
        self.own.pending = true;
        let wifi = &mut self.own.wifi;
        match width {
            Width::Byte => {}
            Width::Half => wifi.write16(offset, value as u16),
            Width::Word => wifi.write32(offset, value),
        }
    }

    fn wifi_dma_timing(&self, addr: u32) -> Timing {
        let area = Area::Slow(if addr & WIFI_WS1 == 0 { 5 } else { 6 });
        if self.own.power & WIFI_POWER == 0 {
            return Timing::new(area, 32, 1, 1);
        }
        let waits = self.wifi_waits(addr);
        Timing::new(area, 16, waits.half[0], waits.half[1])
    }

    fn wifi_waits(&self, addr: u32) -> Waits {
        if self.own.power & WIFI_POWER == 0 {
            return Waits::new(1, 1, 1, 1);
        }
        let control = self.own.wifi_waitcnt;
        let (first, fast, slow) = if addr & WIFI_WS1 == 0 {
            (control & 3, control & 1 << 2 != 0, 6)
        } else {
            (control >> 3 & 3, control & 1 << 5 != 0, 10)
        };
        let n = WIFI_FIRST_WAITS[usize::from(first)];
        let s = if fast { 4 } else { slow };
        Waits::new(n, s, n + s, 2 * s)
    }

    pub fn run_dma(&mut self) -> u32 {
        if !self.own.dma.pending() {
            return 0;
        }
        let mut dma = std::mem::replace(&mut self.own.dma, Dma::new(Cpu::Arm7));
        let (irq, cycles) = dma.run_timed(self);
        self.own.dma = dma;
        self.own.stall += cycles;
        self.own.pending = true;
        self.shared.irq[1].request(irq);
        cycles
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

    pub fn peek_plain(&self, addr: u32, width: Width, io: bool) -> Option<u32> {
        let offset = addr as usize;
        match Region::of(addr) {
            Region::Bios => Some(read_bios(&self.own.bios, offset, width)),
            Region::MainRam => Some(self.shared.main_ram.read(offset, width)),
            Region::SharedWram => Some(match self.shared.wram.read(Cpu::Arm7, offset, width) {
                Some(value) => value,
                None => self.own.wram.read(offset, width),
            }),
            Region::Wram => Some(self.own.wram.read(offset, width)),
            Region::Io if io => Some(lane(peek_io(self.shared, self.own, addr), addr, width)),
            _ => None,
        }
    }
}

impl CpuBus for Arm7Bus<'_> {
    fn fetch(&mut self, addr: u32, width: Width, _access: Access) -> u32 {
        Arm7Bus::read(self, addr, width)
    }

    fn read(&mut self, addr: u32, width: Width, _access: Access) -> u32 {
        self.charge.flags |= LOADED;
        Arm7Bus::read(self, addr, width)
    }

    fn write(&mut self, addr: u32, width: Width, value: u32, _access: Access) {
        self.charge.flags |= STORED;
        Arm7Bus::write(self, addr, width, value);
    }

    fn idle(&mut self) {
        self.charge.idle += 1;
        self.charge.flags |= IDLE;
    }

    fn code_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        let region = Region::of(addr);
        let cycles = self.region_cycles(region, addr, width, access);
        self.charge_code(
            Fetch {
                addr,
                width,
                region,
            },
            cycles,
        );
        cycles
    }

    fn data_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        let region = Region::of(addr);
        let cycles = self.region_cycles(region, addr, width, access);
        self.charge_data(region, cycles, access);
        cycles
    }

    #[inline(always)]
    fn timed_fetch(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        let code = self.own.code.now;
        if addr >> CODE_PAGE_BITS != code.page {
            return self.timed_fetch_elsewhere(addr, width, access);
        }
        self.code_fetch(code, addr, width, access)
    }

    #[inline(always)]
    fn timed_read(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        let region = Region::of(addr);
        let cycles = self.region_cycles(region, addr, width, access);
        self.charge_data(region, cycles, access);
        self.charge.flags |= LOADED;
        (cycles, self.read_region(region, addr, width))
    }

    #[inline(always)]
    fn timed_write(&mut self, addr: u32, width: Width, value: u32, access: Access) -> u32 {
        let region = Region::of(addr);
        let cycles = self.region_cycles(region, addr, width, access);
        self.charge_data(region, cycles, access);
        self.charge.flags |= STORED;
        self.write_region(region, addr, width, value);
        cycles
    }

    #[inline(always)]
    fn timed_read_block(&mut self, addr: u32, values: &mut [u32]) -> u32 {
        if let Some((region, cycles)) = self.word_run(addr, values.len()) {
            let ram = match region {
                Region::MainRam => &self.shared.main_ram,
                _ => &self.own.wram,
            };
            if ram.read_words(addr as usize, values) {
                self.charge_run(region, cycles, LOADED);
                return cycles;
            }
        }
        read_words(self, addr, values)
    }

    #[inline(always)]
    fn timed_write_block(&mut self, addr: u32, values: &[u32]) -> u32 {
        if let Some((region, cycles)) = self.word_run(addr, values.len()) {
            let ram = match region {
                Region::MainRam => &mut self.shared.main_ram,
                _ => &mut self.own.wram,
            };
            if ram.write_words(addr as usize, values) {
                self.charge_run(region, cycles, STORED);
                return cycles;
            }
        }
        write_words(self, addr, values)
    }

    #[inline(always)]
    fn near_jumps(&self) -> bool {
        true
    }

    #[inline(always)]
    fn near_jump(
        &mut self,
        pc: u32,
        pc_width: Width,
        access: Access,
        target: u32,
        width: Width,
    ) -> Option<(u32, [u32; 2])> {
        let code = self.own.code.now;
        let next = target.wrapping_add(width.bytes() as u32);
        if pc >> CODE_PAGE_BITS != code.page
            || target >> CODE_PAGE_BITS != code.page
            || next >> CODE_PAGE_BITS != code.page
        {
            return None;
        }
        let dropped = u32::from(code.cycles[pc_width as usize * 2 + access as usize]);
        self.charge_code(
            Fetch {
                addr: pc,
                width: pc_width,
                region: code.region,
            },
            dropped,
        );
        self.jump(Jump::Branch);
        self.branch();
        let (first_cycles, first) = self.code_fetch(code, target, width, Access::NonSequential);
        let (second_cycles, second) = self.code_fetch(code, next, width, Access::Sequential);
        Some((dropped + first_cycles + second_cycles, [first, second]))
    }

    #[inline(always)]
    fn near_blocks(&self) -> bool {
        true
    }

    #[inline(always)]
    fn near_block_words(
        &mut self,
        pc: u32,
        width: Width,
        access: Access,
        addr: u32,
        count: usize,
    ) -> Option<(u32, u32, &[[u8; 4]])> {
        let code = self.own.code.now;
        if pc >> CODE_PAGE_BITS != code.page {
            return None;
        }
        let (region, cycles) = self.word_run(addr, count)?;
        let next = self.code_value(code, pc, width);
        let ram = match region {
            Region::MainRam => &self.shared.main_ram,
            _ => &self.own.wram,
        };
        let words = ram.words(addr as usize, count)?;
        let fetched = u32::from(code.cycles[width as usize * 2 + access as usize]);
        self.charge.code(
            Fetch {
                addr: pc,
                width,
                region: code.region,
            },
            fetched,
        );
        self.charge.run(region, cycles, LOADED);
        Some((fetched + cycles, next, words))
    }

    #[inline(always)]
    fn near_block_words_mut(
        &mut self,
        pc: u32,
        width: Width,
        access: Access,
        addr: u32,
        count: usize,
    ) -> Option<(u32, u32, &mut [[u8; 4]])> {
        let code = self.own.code.now;
        if pc >> CODE_PAGE_BITS != code.page {
            return None;
        }
        let (region, cycles) = self.word_run(addr, count)?;
        let next = self.code_value(code, pc, width);
        let ram = match region {
            Region::MainRam => &mut self.shared.main_ram,
            _ => &mut self.own.wram,
        };
        let words = ram.words_mut(addr as usize, count)?;
        let fetched = u32::from(code.cycles[width as usize * 2 + access as usize]);
        self.charge.code(
            Fetch {
                addr: pc,
                width,
                region: code.region,
            },
            fetched,
        );
        self.charge.run(region, cycles, STORED);
        Some((fetched + cycles, next, words))
    }

    fn branch(&mut self) {
        let charge = &mut self.charge;
        charge.flags = charge.flags & !TARGET | REFILLING;
        charge.refill = 0;
    }

    fn jump(&mut self, jump: Jump) {
        let flag = match jump {
            Jump::Branch => BRANCH,
            Jump::BlockLoad => BLOCK_LOAD,
        };
        self.charge.flags = self.charge.flags & !(BRANCH | BLOCK_LOAD) | flag;
    }

    #[inline(always)]
    fn settle(&mut self, cycles: u32) -> u32 {
        let flags = self.charge.flags;
        if flags & !CODE == 0 {
            self.charge.flags = 0;
            return if flags != 0 && cycles != 0 {
                self.code_cost(self.charge.code, Access::Sequential)
            } else {
                0
            };
        }
        self.settle_touched(cycles)
    }

    fn cycles(&self, addr: u32, width: Width, access: Access) -> u32 {
        self.region_cycles(Region::of(addr), addr, width, access)
    }

    fn bus_value(&self) -> u32 {
        0
    }

    fn drive(&mut self, _value: u32) {}
}

#[cfg(jit)]
impl JitBus for Arm7Bus<'_> {
    const WRITABLE: &'static [CodeRam] = &[CodeRam::Main, CodeRam::Wram, CodeRam::SharedWram];

    fn code_layout(&self) -> u64 {
        u64::from(self.shared.wram.control())
    }

    fn code_span(&self, addr: u32) -> Option<CodeSpan> {
        let offset = addr as usize;
        let (ram, offset) = match Region::of(addr) {
            Region::Bios => (CodeRam::Bios, offset),
            Region::MainRam => (CodeRam::Main, offset & (MAIN_RAM_SIZE - 1)),
            Region::SharedWram => match self.shared.wram.code_offset(Cpu::Arm7, offset) {
                Some(offset) => (CodeRam::SharedWram, offset),
                None => (CodeRam::Wram, offset & (ARM7_WRAM_SIZE - 1)),
            },
            Region::Wram => (CodeRam::Wram, offset & (ARM7_WRAM_SIZE - 1)),
            _ => return None,
        };
        Some(CodeSpan {
            ram,
            offset: offset as u32,
        })
    }

    fn code_word(&self, span: CodeSpan) -> u32 {
        let offset = span.offset as usize;
        match span.ram {
            CodeRam::Main => self.shared.main_ram.read(offset, Width::Word),
            CodeRam::Wram => self.own.wram.read(offset, Width::Word),
            CodeRam::SharedWram => self.shared.wram.ram().read(offset, Width::Word),
            CodeRam::Bios => read_bios(&self.own.bios, offset, Width::Word),
            CodeRam::Itcm | CodeRam::Rom => 0,
        }
    }

    fn mark_code(&mut self, ram: CodeRam, page: usize) -> u32 {
        match ram {
            CodeRam::Main => self.shared.main_ram.code_mut().mark(page, MAIN_RAM_SIZE),
            CodeRam::Wram => self.own.wram.code_mut().mark(page, ARM7_WRAM_SIZE),
            CodeRam::SharedWram => {
                let ram = self.shared.wram.ram_mut();
                ram.code_mut().mark(page, SHARED_WRAM_SIZE)
            }
            CodeRam::Bios | CodeRam::Itcm | CodeRam::Rom => 0,
        }
    }

    fn code_generation(&self, ram: CodeRam, page: usize) -> Option<u32> {
        match ram {
            CodeRam::Main => self.shared.main_ram.code().generation(page),
            CodeRam::Wram => self.own.wram.code().generation(page),
            CodeRam::SharedWram => self.shared.wram.ram().code().generation(page),
            CodeRam::Bios => Some(0),
            CodeRam::Itcm | CodeRam::Rom => None,
        }
    }

    fn take_code_hit(&mut self) -> bool {
        let main = self.shared.main_ram.code_mut().take_hit();
        let wram = self.own.wram.code_mut().take_hit();
        let shared = self.shared.wram.ram_mut().code_mut().take_hit();
        main | wram | shared
    }

    fn take_stall(&mut self) -> u32 {
        std::mem::take(&mut self.own.stall)
    }

    fn stall_flag(&self) -> *const u32 {
        &self.own.stall
    }

    fn code_stamp(&self, ram: CodeRam) -> u32 {
        match ram {
            CodeRam::Main => self.shared.main_ram.code().stamp(),
            CodeRam::Wram => self.own.wram.code().stamp(),
            CodeRam::SharedWram => self.shared.wram.ram().code().stamp(),
            CodeRam::Bios | CodeRam::Itcm | CodeRam::Rom => 0,
        }
    }

    fn written_page(&self, ram: CodeRam, stamp: u32) -> Option<usize> {
        match ram {
            CodeRam::Main => self.shared.main_ram.code().written_page(stamp),
            CodeRam::Wram => self.own.wram.code().written_page(stamp),
            CodeRam::SharedWram => self.shared.wram.ram().code().written_page(stamp),
            CodeRam::Bios | CodeRam::Itcm | CodeRam::Rom => None,
        }
    }

    fn runs_on(&self, masked: bool) -> bool {
        !self.own.halted
            && !(self.shared.cart.armed() || self.own.spi.armed())
            && (masked || !self.shared.irq[1].line())
    }
}

#[cfg(jit)]
const MAIN_RAM_BASE: u32 = 0x0200_0000;
#[cfg(jit)]
const SHARED_WRAM_BASE: u32 = 0x0300_0000;
#[cfg(jit)]
const MAIN_RAM_WINDOW: u64 = 0x0100_0000;
#[cfg(jit)]
const WRAM_WINDOW: u64 = 0x0080_0000;

#[cfg(jit)]
fn page_meta(region: Region) -> u64 {
    let code = [
        Region::Wram.cycles(Width::Word, Access::NonSequential),
        Region::MainRam.cycles(Width::Half, Access::NonSequential),
        Region::MainRam.cycles(Width::Word, Access::NonSequential),
    ];
    Page::meta_v4(
        [
            region.cycles(Width::Half, Access::NonSequential),
            region.cycles(Width::Word, Access::NonSequential),
            region.cycles(Width::Word, Access::Sequential),
            region.cycles(Width::Half, Access::Sequential),
        ],
        region == Region::MainRam,
        code,
    )
}

#[cfg(jit)]
impl FastBus for Arm7Bus<'_> {
    const SPECS: [Spec; WINDOWS] = [Spec {
        len: 0,
        code: false,
        cache: None,
        half: [0; 2],
        word: [0; 2],
    }; WINDOWS];

    const PAGES: bool = true;

    #[inline(always)]
    fn fast(&mut self) -> *mut Fast {
        let inputs = [
            self.shared.main_ram.code().pages().addr(),
            self.own.wram.code().pages().addr(),
            self.shared.wram.ram().code().pages().addr(),
            usize::from(self.shared.wram.control()),
        ];
        let stamp = self.own.fast.stamp(self.own, self.shared, inputs);
        let table = match self.own.fast.get(stamp) {
            Some(table) => table,
            None => self.refill_fast(stamp),
        };
        unsafe { (*table).debt = &raw mut self.debt };
        table
    }

    fn code_class(start: u32, end: u32, thumb: bool) -> u32 {
        let region = Region::of(start);
        if Region::of(end) != region {
            return 0;
        }
        match region {
            Region::Bios | Region::SharedWram | Region::Wram => 1,
            Region::MainRam if thumb => 2,
            Region::MainRam => 3,
            Region::Io | Region::Wifi | Region::Vram | Region::GbaSlot | Region::Unmapped => 0,
        }
    }
}

#[cfg(jit)]
impl Arm7Bus<'_> {
    #[cold]
    #[inline(never)]
    fn refill_fast(&mut self, stamp: Stamp) -> *mut Fast {
        let main = self.shared.main_ram.window(MAIN_RAM_BASE, MAIN_RAM_WINDOW);
        let wram = self.own.wram.window(ARM7_WRAM_BASE, WRAM_WINDOW);
        let shared = self
            .shared
            .wram
            .fast_window(Cpu::Arm7, SHARED_WRAM_BASE, WRAM_WINDOW);
        let low = match shared {
            Some(low) => low,
            None => (
                self.own.wram.window(SHARED_WRAM_BASE, WRAM_WINDOW),
                ARM7_WRAM_SIZE as u32,
            ),
        };
        let pages = self.own.fast.pages();
        for (window, len, region) in [
            (main, MAIN_RAM_SIZE as u32, Region::MainRam),
            (low.0, low.1, Region::Wram),
            (wram, ARM7_WRAM_SIZE as u32, Region::Wram),
        ] {
            let meta = page_meta(region);
            for page in 0..window.size / u64::from(PAGE_SIZE) {
                let addr = window.base + page as u32 * PAGE_SIZE;
                pages.set(addr, Page::of(&window, len, addr, meta));
            }
        }
        let table = Fast {
            pages: pages.table(),
            ..Fast::CLOSED
        };
        self.own.fast.keep(stamp, table)
    }
}

impl SoundMemory for Arm7Bus<'_> {
    fn read_word(&mut self, address: u32) -> u32 {
        self.read(address, Width::Word)
    }

    fn write_word(&mut self, address: u32, value: u32) {
        self.write(address, Width::Word, value);
    }
}

impl DmaBus for Arm7Bus<'_> {
    fn read_half(&mut self, address: u32) -> u16 {
        Arm7Bus::read(self, address, Width::Half) as u16
    }

    fn read_word(&mut self, address: u32) -> u32 {
        Arm7Bus::read(self, address, Width::Word)
    }

    fn write_half(&mut self, address: u32, value: u16) {
        Arm7Bus::write(self, address, Width::Half, u32::from(value));
    }

    fn write_word(&mut self, address: u32, value: u32) {
        Arm7Bus::write(self, address, Width::Word, value);
    }

    fn timing(&self, address: u32) -> Timing {
        match Region::of(address) {
            Region::Bios => Timing::new(Area::Plain(1), 32, 1, 1),
            Region::MainRam => Timing::new(Area::MainRam, 16, 8, 1),
            Region::SharedWram | Region::Wram => Timing::new(Area::Plain(2), 32, 1, 1),
            Region::Io => Timing::new(Area::Plain(3), 32, 1, 1),
            Region::Wifi => self.wifi_dma_timing(address).set_by_registers(),
            Region::Vram => Timing::new(Area::Plain(4), 16, 1, 1),
            Region::GbaSlot => self
                .shared
                .gba_slot_dma_timing(true, address)
                .set_by_registers(),
            Region::Unmapped => Timing::UNMAPPED,
        }
    }
}

pub(super) fn peek_io(shared: &Shared, own: &Arm7Memory, addr: u32) -> u32 {
    io::Arm7Io { shared, own }.peek(addr)
}

#[cfg(test)]
mod tests {
    use super::super::Buses;
    use super::*;
    use crate::gba::cpu::{Cpu as Arm7Core, CpuState};
    use crate::gba::cpu::{CpuBus, Exception, read_words, write_words};
    use crate::gba::wait::{Access, Width};
    use crate::nds::wifi::registers::*;

    const WRAMCNT: u32 = 0x0400_0247;
    const VRAMCNT_C: u32 = 0x0400_0242;
    const VRAMCNT_D: u32 = 0x0400_0243;

    fn with_wram_mode(mode: u32) -> Buses {
        let mut buses = Buses::default();
        buses.arm9().write(WRAMCNT, Width::Byte, mode);
        buses
    }

    #[test]
    fn arm7_wram_is_64k_mirrored_up_to_the_end_of_its_block() {
        let mut buses = Buses::default();
        buses.arm7().write(0x0380_FFFC, Width::Word, 0x0380_1234);
        assert_eq!(buses.arm7().read(0x03FF_FFFC, Width::Word), 0x0380_1234);
    }

    #[test]
    fn with_no_shared_wram_the_arm7_sees_its_own_wram_at_3000000() {
        let mut buses = with_wram_mode(0);
        buses.arm7().write(0x0380_0010, Width::Word, 0xCAFE_F00D);
        assert_eq!(buses.arm7().read(0x0300_0010, Width::Word), 0xCAFE_F00D);
        buses.arm7().write(0x037F_0020, Width::Word, 3);
        assert_eq!(buses.arm7().read(0x0380_0020, Width::Word), 3);
    }

    #[test]
    fn shared_wram_follows_wramcnt_for_the_arm7() {
        let mut buses = with_wram_mode(0);
        buses.arm9().write(0x0300_0000, Width::Word, 0x1111_1111);
        buses.arm9().write(0x0300_4000, Width::Word, 0x2222_2222);
        buses.arm9().write(WRAMCNT, Width::Byte, 1);
        assert_eq!(buses.arm7().read(0x0300_4000, Width::Word), 0x1111_1111);
        buses.arm9().write(WRAMCNT, Width::Byte, 2);
        assert_eq!(buses.arm7().read(0x0300_0000, Width::Word), 0x2222_2222);
        buses.arm9().write(WRAMCNT, Width::Byte, 3);
        assert_eq!(buses.arm7().read(0x0300_4000, Width::Word), 0x2222_2222);
    }

    #[test]
    fn mode_3_makes_37f8000_a_continuous_96k_block() {
        let mut buses = with_wram_mode(3);
        buses.arm7().write(0x037F_8000, Width::Word, 1);
        buses.arm7().write(0x037F_FFFC, Width::Word, 2);
        buses.arm7().write(0x0380_0000, Width::Word, 3);
        assert_eq!(buses.arm7().read(0x0300_0000, Width::Word), 1);
        assert_eq!(buses.arm7().read(0x0300_7FFC, Width::Word), 2);
        assert_eq!(buses.arm7().read(0x0381_0000, Width::Word), 3);
    }

    #[test]
    fn a_fetch_from_shared_wram_follows_a_wramcnt_change() {
        let mut buses = with_wram_mode(3);
        buses.arm7().write(0x037F_8000, Width::Word, 1);
        buses.arm7().write(0x0380_8000, Width::Word, 2);
        let n = Access::NonSequential;
        assert_eq!(
            buses.arm7().timed_fetch(0x037F_8000, Width::Word, n),
            (1, 1)
        );
        assert_eq!(
            buses.arm7().timed_fetch(0x037F_8004, Width::Word, n),
            (1, 0)
        );
        buses.arm9().write(WRAMCNT, Width::Byte, 0);
        assert_eq!(
            buses.arm7().timed_fetch(0x037F_8000, Width::Word, n),
            (1, 2)
        );
    }

    #[test]
    fn a_fetch_back_in_a_page_it_left_follows_a_wramcnt_change() {
        let mut buses = with_wram_mode(3);
        buses.arm7().write(0x037F_8000, Width::Word, 1);
        buses.arm7().write(0x0380_8000, Width::Word, 2);
        buses.arm7().write(0x0380_0000, Width::Word, 3);
        let fetch = |buses: &mut Buses, addr| {
            buses
                .arm7()
                .timed_fetch(addr, Width::Word, Access::NonSequential)
        };
        assert_eq!(fetch(&mut buses, 0x037F_8000), (1, 1));
        assert_eq!(fetch(&mut buses, 0x0380_0000), (1, 3));
        assert_eq!(fetch(&mut buses, 0x037F_8000), (1, 1));
        assert_eq!(fetch(&mut buses, 0x0380_0004), (1, 0));
        assert_eq!(fetch(&mut buses, 0x037F_8000), (1, 1));
        assert_eq!(fetch(&mut buses, 0x037F_8004), (1, 0));
        buses.arm9().write(WRAMCNT, Width::Byte, 0);
        assert_eq!(fetch(&mut buses, 0x037F_8000), (1, 2));
        assert_eq!(fetch(&mut buses, 0x0380_0000), (1, 3));
        buses.arm9().write(WRAMCNT, Width::Byte, 3);
        assert_eq!(fetch(&mut buses, 0x037F_8000), (1, 1));
    }

    #[test]
    fn a_fetch_reads_a_store_into_its_own_page() {
        let mut buses = Buses::default();
        let mut bus = buses.arm7();
        let s = Access::Sequential;
        bus.write(0x0238_0000, Width::Word, 0xE1A0_0000);
        assert_eq!(
            bus.timed_fetch(0x0238_0000, Width::Word, s),
            (2, 0xE1A0_0000)
        );
        bus.write(0x0238_0002, Width::Half, 0xEAFF);
        assert_eq!(bus.timed_fetch(0x0238_0002, Width::Half, s), (1, 0xEAFF));
        assert_eq!(
            bus.timed_fetch(0x0238_0000, Width::Word, s),
            (2, 0xEAFF_0000)
        );
    }

    #[test]
    fn wramstat_shows_the_mode_and_ignores_writes() {
        let mut buses = with_wram_mode(2);
        buses.arm7().write(0x0400_0241, Width::Byte, 3);
        assert_eq!(buses.arm7().read(0x0400_0241, Width::Byte), 2);
    }

    #[test]
    fn banks_c_and_d_in_mst_2_are_arm7_vram_and_take_byte_writes() {
        let mut buses = Buses::default();
        buses.arm9().write(VRAMCNT_C, Width::Byte, 0x82);
        buses.arm9().write(VRAMCNT_D, Width::Byte, 0x8A);
        buses.arm7().write(0x0600_0001, Width::Byte, 0xC1);
        buses.arm7().write(0x0602_0002, Width::Byte, 0xD2);
        assert_eq!(buses.vram().bank(crate::nds::vram::Bank::C)[1], 0xC1);
        assert_eq!(buses.vram().bank(crate::nds::vram::Bank::D)[2], 0xD2);
        assert_eq!(buses.arm7().read(0x0604_0001, Width::Byte), 0xC1);
        assert_eq!(buses.arm7().read(0x0400_0240, Width::Byte), 0b11);
    }

    #[test]
    fn vram_not_given_to_the_arm7_reads_zero() {
        let mut buses = Buses::default();
        buses.arm9().write(VRAMCNT_C, Width::Byte, 0x80);
        buses.arm9().write(0x0684_0000, Width::Word, 9);
        assert_eq!(buses.arm7().read(0x0600_0000, Width::Word), 0);
        assert_eq!(buses.arm7().read(0x0400_0240, Width::Byte), 0);
    }

    #[test]
    fn palettes_and_oam_are_not_on_the_arm7_bus() {
        let mut buses = Buses::default();
        buses.arm9().write(0x0500_0000, Width::Half, 0x7FFF);
        buses.arm9().write(0x0700_0000, Width::Half, 0x7FFF);
        assert_eq!(buses.arm7().read(0x0500_0000, Width::Half), 0);
        assert_eq!(buses.arm7().read(0x0700_0000, Width::Half), 0);
    }

    #[test]
    fn the_wait_table_matches_each_region_s_waits() {
        let regions = [
            (Region::Bios, None),
            (Region::MainRam, Some(MAIN_RAM_WAITS)),
            (Region::SharedWram, None),
            (Region::Wram, None),
            (Region::Io, None),
            (Region::Wifi, None),
            (Region::Vram, Some(VRAM_WAITS)),
            (Region::GbaSlot, Some(GBA_SLOT_WAITS)),
            (Region::Unmapped, None),
        ];
        for (region, waits) in regions {
            for width in [Width::Byte, Width::Half, Width::Word] {
                for access in [Access::NonSequential, Access::Sequential] {
                    let expected =
                        waits.map_or(FAST_CYCLES, |waits: Waits| waits.cycles(width, access));
                    assert_eq!(
                        region.cycles(width, access),
                        expected,
                        "{region:?} {width:?} {access:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_cpu_bus_trait_reads_and_writes_the_same_memory() {
        let mut buses = Buses::default();
        let mut bus = buses.arm7();
        CpuBus::write(
            &mut bus,
            0x0380_0100,
            Width::Word,
            0xE3A0_0001,
            Access::NonSequential,
        );
        assert_eq!(
            bus.fetch(0x0380_0100, Width::Word, Access::NonSequential),
            0xE3A0_0001
        );
    }

    const POWCNT2: u32 = 0x0400_0304;
    const WIFIWAITCNT: u32 = 0x0400_0206;
    const IF: usize = 0xC;
    const WIFI_IRQ: u32 = 1 << 24;

    fn powered_wifi() -> Buses {
        let mut buses = Buses::default();
        buses.arm7().write(POWCNT2, Width::Half, 3);
        buses
    }

    fn wifi_reg(register: usize) -> u32 {
        wifi::BASE + register as u32
    }

    #[test]
    fn the_wifi_chip_reads_zero_until_powcnt2_powers_it() {
        let mut buses = Buses::default();
        assert_eq!(buses.arm7().read(wifi_reg(W_ID), Width::Half), 0);
        buses.arm7().write(POWCNT2, Width::Half, 3);
        assert_eq!(buses.arm7().read(wifi_reg(W_ID), Width::Half), 0x1440);
        buses.arm7().write(POWCNT2, Width::Half, 1);
        assert_eq!(buses.arm7().read(wifi_reg(W_ID), Width::Half), 0);
    }

    #[test]
    fn wifi_ram_and_registers_answer_in_both_wait_state_windows() {
        let mut buses = powered_wifi();
        buses.arm7().write(0x0480_4010, Width::Word, 0x1234_5678);
        assert_eq!(buses.arm7().read(0x0480_4010, Width::Word), 0x1234_5678);
        assert_eq!(buses.arm7().read(0x0480_C010, Width::Half), 0x5678);
        assert_eq!(buses.arm7().read(0x0480_4012, Width::Byte), 0x34);
        buses.arm7().write(wifi_reg(W_MACADDR), Width::Half, 0x0900);
        assert_eq!(
            buses
                .arm7()
                .read(0x0480_8000 + W_MACADDR as u32, Width::Half),
            0x0900
        );
        assert_eq!(buses.wifi().mac()[..2], [0x00, 0x09]);
    }

    #[test]
    fn the_arm9_sees_no_wifi_chip() {
        let mut buses = powered_wifi();
        assert_eq!(buses.arm9().read(wifi_reg(W_ID), Width::Half), 0);
    }

    #[test]
    fn a_wifi_interrupt_reaches_the_arm7_irq_flags() {
        let mut buses = powered_wifi();
        buses.arm7().write(wifi_reg(W_POWER_US), Width::Half, 0);
        buses.arm7().write(wifi_reg(W_IE), Width::Half, 1);
        buses.arm7().write(wifi_reg(W_IF_SET), Width::Half, 1);
        buses.elapse(8);
        assert_ne!(buses.interrupts(Cpu::Arm7).read(IF) & WIFI_IRQ, 0);
        assert_eq!(buses.interrupts(Cpu::Arm9).read(IF) & WIFI_IRQ, 0);
    }

    #[test]
    fn the_microsecond_counter_runs_with_the_bus_clock() {
        let mut buses = powered_wifi();
        buses.arm7().write(wifi_reg(W_POWER_US), Width::Half, 0);
        buses.arm7().write(wifi_reg(W_US_COUNTCNT), Width::Half, 1);
        buses.elapse(33_513_982 / 1000);
        let low = buses.arm7().read(wifi_reg(W_US_COUNT0), Width::Half);
        assert!((999..=1001).contains(&low), "{low}");
    }

    #[test]
    fn wifi_wait_states_follow_wifiwaitcnt_only_while_powered() {
        let mut buses = Buses::default();
        let ws0 = wifi::BASE;
        let ws1 = wifi::BASE + 0x8000;
        let cycles = |buses: &mut Buses, addr, width| {
            CpuBus::cycles(&buses.arm7(), addr, width, Access::NonSequential)
        };
        assert_eq!(cycles(&mut buses, ws0, Width::Half), 1);
        buses.arm7().write(POWCNT2, Width::Half, 3);
        assert_eq!(cycles(&mut buses, ws0, Width::Half), 10);
        assert_eq!(cycles(&mut buses, ws0, Width::Word), 16);
        assert_eq!(cycles(&mut buses, ws1, Width::Word), 20);
        buses.arm7().write(WIFIWAITCNT, Width::Half, 0x002E);
        assert_eq!(cycles(&mut buses, ws0, Width::Word), 10);
        assert_eq!(cycles(&mut buses, ws1, Width::Word), 12);
    }

    #[test]
    fn the_wireless_start_reads_back_every_register_it_writes() {
        const CHECKED: [(u32, u32); 27] = [
            (0x006, 0x003F),
            (0x018, 0xFFFF),
            (0x01A, 0xFFFF),
            (0x01C, 0xFFFF),
            (0x020, 0xFFFF),
            (0x022, 0xFFFF),
            (0x024, 0xFFFF),
            (0x02A, 0x07FF),
            (0x050, 0xFFFF),
            (0x052, 0xFFFF),
            (0x056, 0x0FFE),
            (0x058, 0x1FFE),
            (0x05A, 0x0FFE),
            (0x05C, 0x0FFF),
            (0x062, 0x1FFE),
            (0x064, 0x0FFF),
            (0x068, 0x1FFE),
            (0x06C, 0x0FFF),
            (0x074, 0x1FFE),
            (0x122, 0xFFFF),
            (0x124, 0xFFFF),
            (0x128, 0xFFFF),
            (0x130, 0x0FFF),
            (0x132, 0x8FFF),
            (0x134, 0xFFFF),
            (0x140, 0xFFFF),
            (0x142, 0xFFFF),
        ];
        let mut buses = Buses::default();
        buses.arm7().write(0x0400_0304, Width::Half, 2);
        buses.arm7().write(0x0480_80AE, Width::Half, 8);
        for pattern in [0xFFFF, 0x5A5A] {
            for (register, mask) in CHECKED {
                let address = 0x0480_8000 + register;
                buses.arm7().write(address, Width::Half, pattern & mask);
                assert_eq!(
                    buses.arm7().read(address, Width::Half),
                    pattern & mask,
                    "register {register:#05X} after writing {:#06X}",
                    pattern & mask
                );
            }
        }
    }

    #[cfg(jit)]
    fn page_of(bus: &mut Arm7Bus, addr: u32) -> Page {
        unsafe { *(*bus.fast()).pages.add((addr >> 12) as usize) }
    }

    #[cfg(jit)]
    fn model_cycles(meta: u64, class: u32, data: u32, load: bool) -> u32 {
        use crate::jit::{CODE_CYCLES, MAIN};
        let code = ((meta >> (CODE_CYCLES + 8 * class)) & 0xFF) as u32;
        let load = u32::from(load);
        let overlapped = |a: u32, b: u32| (a + b).saturating_sub(3).max(a).max(b);
        match (class & 2 != 0, meta >> MAIN & 1 != 0) {
            (true, true) => code + data,
            (false, false) => code + data + load,
            (true, false) => overlapped(code, data + load),
            (false, true) => overlapped(code + load, data),
        }
    }

    #[cfg(jit)]
    #[test]
    fn the_page_table_reaches_the_bytes_of_every_wram_mode() {
        let addrs = [
            0x0000_0010,
            0x0200_0024,
            0x023F_FFFC,
            0x02FF_F000,
            0x0300_0010,
            0x0300_4010,
            0x037F_FFFC,
            0x0380_0010,
            0x0380_FFFC,
            0x03FF_0000,
            0x0400_0000,
            0x0600_0000,
        ];
        for mode in 0..4 {
            let mut buses = with_wram_mode(mode);
            let mut bus = buses.arm7();
            for (&addr, value) in addrs.iter().zip(0x2222_2222u32..) {
                CpuBus::write(&mut bus, addr, Width::Word, value, Access::NonSequential);
                let page = page_of(&mut bus, addr);
                let region = Region::of(addr);
                let inline = matches!(region, Region::MainRam | Region::SharedWram | Region::Wram);
                assert_eq!(page.meta != 0, inline, "{mode} {addr:08X}");
                if !inline {
                    continue;
                }
                let host = page.addend.wrapping_add(u64::from(addr)) as *const u32;
                assert_eq!(unsafe { host.read_unaligned() }, value, "{mode} {addr:08X}");
                assert_eq!(bus.read(addr, Width::Word), value, "{mode} {addr:08X}");
                assert_ne!(unsafe { *page.dirty }, 0, "{mode} {addr:08X}");
            }
        }
    }

    #[cfg(jit)]
    #[test]
    fn the_page_table_costs_what_the_bus_charges() {
        let codes = [
            (0x0000_0100, false),
            (0x0300_0100, true),
            (0x0380_0100, false),
            (0x0200_0100, true),
            (0x0200_0100, false),
        ];
        let datas = [0x0200_1002, 0x0300_2002, 0x0380_3002];
        let mut buses = with_wram_mode(3);
        let mut bus = buses.arm7();
        for (pc, thumb) in codes {
            let class = Arm7Bus::code_class(pc, pc + 8, thumb);
            assert_ne!(class, 0);
            let code = if thumb { Width::Half } else { Width::Word };
            for addr in datas {
                let meta = page_of(&mut bus, addr).meta;
                for width in [Width::Byte, Width::Half, Width::Word] {
                    for load in [false, true] {
                        bus.code_cycles(pc, code, Access::Sequential);
                        if load {
                            bus.timed_read(addr, width, Access::NonSequential);
                            bus.idle();
                        } else {
                            bus.timed_write(addr, width, 0, Access::NonSequential);
                        }
                        let charged = bus.settle(1);
                        let data = bus.cycles(addr, width, Access::NonSequential);
                        assert_eq!(
                            model_cycles(meta, class, data, load),
                            charged,
                            "{pc:08X} {thumb} {addr:08X} {width:?} {load}"
                        );
                    }
                }
                for words in 1..=16 {
                    for load in [false, true] {
                        let at = addr & !3;
                        for word in 0..words {
                            let access = if word == 0 {
                                Access::NonSequential
                            } else {
                                Access::Sequential
                            };
                            if load {
                                bus.timed_read(at + 4 * word, Width::Word, access);
                            } else {
                                bus.timed_write(at + 4 * word, Width::Word, 0, access);
                            }
                        }
                        bus.code_cycles(pc, code, Access::Sequential);
                        if load {
                            bus.idle();
                        }
                        let charged = bus.settle(1);
                        let data = bus.cycles(at, Width::Word, Access::NonSequential)
                            + (words - 1) * bus.cycles(at, Width::Word, Access::Sequential);
                        assert_eq!(
                            model_cycles(meta, class, data, load),
                            charged,
                            "{pc:08X} {thumb} {at:08X} {words} {load}"
                        );
                    }
                }
            }
        }
        assert_eq!(Arm7Bus::code_class(0x0400_0000, 0x0400_0008, false), 0);
        assert_eq!(Arm7Bus::code_class(0x02FF_FFF8, 0x0300_0000, false), 0);
    }

    struct Far<'a>(Arm7Bus<'a>);

    impl CpuBus for Far<'_> {
        fn fetch(&mut self, addr: u32, width: Width, access: Access) -> u32 {
            CpuBus::fetch(&mut self.0, addr, width, access)
        }

        fn read(&mut self, addr: u32, width: Width, access: Access) -> u32 {
            CpuBus::read(&mut self.0, addr, width, access)
        }

        fn write(&mut self, addr: u32, width: Width, value: u32, access: Access) {
            CpuBus::write(&mut self.0, addr, width, value, access);
        }

        fn idle(&mut self) {
            self.0.idle();
        }

        fn cycles(&self, addr: u32, width: Width, access: Access) -> u32 {
            self.0.cycles(addr, width, access)
        }

        fn code_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
            self.0.code_cycles(addr, width, access)
        }

        fn data_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
            self.0.data_cycles(addr, width, access)
        }

        fn timed_fetch(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
            self.0.timed_fetch(addr, width, access)
        }

        fn timed_read(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
            self.0.timed_read(addr, width, access)
        }

        fn timed_write(&mut self, addr: u32, width: Width, value: u32, access: Access) -> u32 {
            self.0.timed_write(addr, width, value, access)
        }

        fn timed_read_block(&mut self, addr: u32, values: &mut [u32]) -> u32 {
            self.0.timed_read_block(addr, values)
        }

        fn timed_write_block(&mut self, addr: u32, values: &[u32]) -> u32 {
            self.0.timed_write_block(addr, values)
        }

        fn branch(&mut self) {
            self.0.branch();
        }

        fn jump(&mut self, jump: Jump) {
            self.0.jump(jump);
        }

        fn settle(&mut self, cycles: u32) -> u32 {
            self.0.settle(cycles)
        }

        fn bus_value(&self) -> u32 {
            self.0.bus_value()
        }

        fn drive(&mut self, value: u32) {
            self.0.drive(value);
        }
    }

    #[test]
    fn jumps_step_the_same_with_or_without_the_near_jumps() {
        const NOP: u32 = 0xE1A0_0000;
        let moves = |thumb: bool| -> [u32; 3] {
            if thumb {
                [0x2202_2101, 0x2404_2303, 0x2606_2505]
            } else {
                [0xE3A0_1001, 0xE3A0_2002, 0xE3A0_3003]
            }
        };
        let deltas: [i32; 9] = [0x40, -0x20, 0x8, 0x0, 0x1000, -0x1000, 0xF00, -0x100, 0x7F0];
        let entries: [u32; 5] = [
            0x0238_0000,
            0x0238_0FF4,
            0x0238_0FF0,
            0x0380_0100,
            0x0380_0F00,
        ];
        for entry in entries {
            let at = entry + 4;
            let elsewhere = if entry >> 24 == 0x02 {
                0x0380_0200
            } else {
                0x0238_0200
            };
            let landings = deltas
                .map(|delta| at.wrapping_add(8).wrapping_add_signed(delta))
                .into_iter()
                .chain([elsewhere]);
            let mut cases = Vec::new();
            for landing in landings {
                let offset = (landing.wrapping_sub(at + 8) >> 2) & 0x00FF_FFFF;
                cases.push((0xEA00_0000 | offset, 0, landing));
                cases.push((0xEB00_0000 | offset, 0, landing));
                for r0 in [landing, landing | 1, landing | 2, landing | 3] {
                    cases.push((0xE12F_FF10, r0, r0));
                }
            }
            for (op, r0, landing) in cases {
                let thumb = landing & 1 != 0;
                let run = |far: bool| {
                    let mut buses = Buses::default();
                    let code = [NOP, op, NOP, NOP];
                    for (index, word) in code.into_iter().enumerate() {
                        buses
                            .arm7()
                            .write(entry + 4 * index as u32, Width::Word, word);
                    }
                    for (index, word) in moves(thumb).into_iter().enumerate() {
                        let addr = (landing & !3).wrapping_add(4 * index as u32);
                        buses.arm7().write(addr, Width::Word, word);
                    }
                    let mut state = CpuState::default();
                    state.r[0] = r0;
                    state.r[15] = entry + 8;
                    state.pipeline = [code[0], code[1]];
                    state.next_fetch = Access::Sequential;
                    let mut cpu = Arm7Core::from_state(&state);
                    let mut cycles = [0; 4];
                    for slot in &mut cycles {
                        *slot = if far {
                            cpu.step(&mut Far(buses.arm7()))
                        } else {
                            cpu.step(&mut buses.arm7())
                        };
                    }
                    (cycles, format!("{cpu:?}"))
                };
                assert_eq!(
                    run(true),
                    run(false),
                    "{op:08X} with r0 {r0:08X} from {entry:08X}"
                );
            }
        }
    }

    const NOP: u32 = 0xE1A0_0000;
    const THUMB_NOP: u16 = 0x46C0;
    const LDM_USER: u32 = 0xE8D0_0006;

    fn run_block(
        far: bool,
        code: Result<[u32; 2], u16>,
        entry: u32,
        base: u32,
        landing: u32,
    ) -> ([u32; 4], String, Vec<u32>) {
        let mut buses = Buses::default();
        let first = (base & !3).wrapping_sub(0x44);
        for index in 0..35 {
            let at = first.wrapping_add(4 * index);
            buses
                .arm7()
                .write(at, Width::Word, landing.wrapping_add(0x10 * index));
        }
        let fill = if code.is_err() {
            u32::from(THUMB_NOP) * 0x1_0001
        } else {
            NOP
        };
        for index in 0..0x90 {
            buses.arm7().write(landing + 4 * index, Width::Word, fill);
        }
        let mut state = CpuState::default();
        for (index, reg) in state.r.iter_mut().enumerate().take(15) {
            *reg = 0x0101_0101 * index as u32;
        }
        state.r[0] = base;
        state.r[1] = base;
        state.r[13] = 0x40;
        state.svc = [base, 0x5E5E_5E5E];
        state.next_fetch = Access::Sequential;
        match code {
            Ok([op, follow]) => {
                let at = entry & !3;
                for (index, word) in [NOP, op, follow, NOP, NOP].into_iter().enumerate() {
                    buses.arm7().write(at + 4 * index as u32, Width::Word, word);
                }
                state.r[15] = at + 8;
                state.pipeline = [NOP, op];
            }
            Err(op) => {
                for (index, half) in [THUMB_NOP, op, THUMB_NOP, THUMB_NOP]
                    .into_iter()
                    .enumerate()
                {
                    let at = entry + 2 * index as u32;
                    buses.arm7().write(at, Width::Half, u32::from(half));
                }
                state.cpsr |= 0x20;
                state.r[15] = entry + 4;
                state.pipeline = [u32::from(THUMB_NOP), u32::from(op)];
            }
        }
        let mut cpu = Arm7Core::from_state(&state);
        let mut cycles = [0; 4];
        for slot in &mut cycles {
            *slot = if far {
                cpu.step(&mut Far(buses.arm7()))
            } else {
                cpu.step(&mut buses.arm7())
            };
        }
        let stored = (0..35)
            .map(|index| {
                buses
                    .arm7()
                    .read(first.wrapping_add(4 * index), Width::Word)
            })
            .collect();
        (cycles, format!("{cpu:?}"), stored)
    }

    #[test]
    fn blocks_step_the_same_with_or_without_the_near_words() {
        let arm: [u32; 19] = [
            0xE92D_4FF0,
            0xE8BD_8FF0,
            0xE8BD_0FF0,
            0xE920_400E,
            0xE8B0_800E,
            0xE890_000E,
            0xE8A0_0007,
            0xE8A1_0007,
            0xE8B0_0003,
            0xE8B1_0003,
            0xE930_0001,
            0xE9B0_0006,
            0xE800_8002,
            0xE8B0_FFFF,
            0xE8A0_FFFF,
            0xE8B0_0000,
            0xE8A0_0000,
            0xE99F_0006,
            LDM_USER,
        ];
        let thumb: [u16; 12] = [
            0xB5F0, 0xB403, 0xBD03, 0xBC04, 0xBD00, 0xC006, 0xC806, 0xC803, 0xC903, 0xC103, 0xC800,
            0xC000,
        ];
        let bases: [u32; 12] = [
            0x0200_0101,
            0x0200_0008,
            0x0200_0FF8,
            0x023F_FFF8,
            0x02FF_FFF8,
            0x037F_FFF8,
            0x0380_0004,
            0x0380_0101,
            0x0380_FFF8,
            0x03FF_FFF8,
            0x0000_0100,
            0xFFFF_FFF8,
        ];
        let entries: [u32; 4] = [0x0238_0000, 0x0238_0FF4, 0x0238_0FFA, 0x0380_8000];
        let landings: [u32; 2] = [0x0238_4000, 0x0380_9000];
        let mut codes: Vec<Result<[u32; 2], u16>> = arm.map(|op| Ok([op, NOP])).into();
        for op in [0xE92D_4FF0, 0xE8BD_0FF0, 0xE8BD_8FF0, 0xE8AD_0003] {
            codes.push(Ok([LDM_USER, op]));
        }
        codes.extend(thumb.map(Err));
        for entry in entries {
            let over_code = [(entry & !3) + 12, entry + 6];
            for base in bases.into_iter().chain(over_code) {
                for &code in &codes {
                    let loads_pc = match code {
                        Ok(ops) => ops.iter().any(|op| op & 0x0010_8000 == 0x0010_8000),
                        Err(op) => op & 0xFF00 == 0xBD00,
                    };
                    let tried = if loads_pc {
                        &landings[..]
                    } else {
                        &landings[..1]
                    };
                    for &landing in tried {
                        assert_eq!(
                            run_block(true, code, entry, base, landing),
                            run_block(false, code, entry, base, landing),
                            "{code:08X?} landing {landing:08X} at {base:08X} from {entry:08X}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn near_words_take_runs_inside_main_ram_or_wram_from_the_code_page() {
        let mut buses = Buses::default();
        let mut bus = buses.arm7();
        CpuBus::timed_fetch(&mut bus, 0x0238_0000, Width::Word, Access::NonSequential);
        let mut near = |pc: u32, addr: u32| {
            let read = bus
                .near_block_words(pc, Width::Word, Access::Sequential, addr, 4)
                .is_some();
            let written = bus
                .near_block_words_mut(pc, Width::Word, Access::Sequential, addr, 4)
                .is_some();
            assert_eq!(read, written, "{pc:08X} {addr:08X}");
            read
        };
        assert!(near(0x0238_0004, 0x0200_0100));
        assert!(near(0x0238_0FFC, 0x0380_0101));
        assert!(near(0x0238_0004, 0x03FF_FFF0));
        assert!(!near(0x0238_1000, 0x0200_0100));
        assert!(!near(0x0238_0004, 0x023F_FFF8));
        assert!(!near(0x0238_0004, 0x02FF_FFF8));
        assert!(!near(0x0238_0004, 0x037F_FFF8));
        assert!(!near(0x0238_0004, 0x0380_FFF8));
        assert!(!near(0x0238_0004, 0x03FF_FFF8));
        assert!(!near(0x0238_0004, 0x0400_0100));
    }

    const WRAM: u32 = 0x0380_0000;

    const MAIN: u32 = 0x0238_0000;

    const ARM_MODE: u32 = 0x1F;

    const THUMB_MODE: u32 = 0x3F;

    fn cost(at: u32, code: &[u32], regs: &[(usize, u32)], words: &[(u32, u32)]) -> u32 {
        run(at, Width::Word, ARM_MODE, code, regs, words)
    }

    fn thumb_cost(at: u32, code: &[u32], regs: &[(usize, u32)], words: &[(u32, u32)]) -> u32 {
        run(at, Width::Half, THUMB_MODE, code, regs, words)
    }

    fn run(
        at: u32,
        width: Width,
        cpsr: u32,
        code: &[u32],
        regs: &[(usize, u32)],
        words: &[(u32, u32)],
    ) -> u32 {
        let mut buses = Buses::default();
        let size = width.bytes() as u32;
        let mut bus = buses.arm7();
        for (i, &op) in code.iter().enumerate() {
            bus.write(at + size * i as u32, width, op);
        }
        for &(addr, value) in words {
            bus.write(addr, Width::Word, value);
        }
        let mut state = CpuState {
            cpsr,
            pipeline: [code[0], code.get(1).copied().unwrap_or(0)],
            next_fetch: Access::Sequential,
            ..CpuState::default()
        };
        state.r[15] = at + 2 * size;
        for &(reg, value) in regs {
            state.r[reg] = value;
        }
        let mut cpu = Arm7Core::from_state(&state);
        cpu.step(&mut buses.arm7())
    }

    #[test]
    fn a_branch_costs_only_the_refill() {
        assert_eq!(cost(WRAM, &[0xEAFF_FFFE], &[], &[]), 2);
        assert_eq!(thumb_cost(WRAM, &[0xE7FE], &[], &[]), 2);
    }

    #[test]
    fn an_alu_op_costs_one_sequential_fetch() {
        assert_eq!(cost(WRAM, &[0xE1A0_0000], &[], &[]), 1);
        assert_eq!(cost(MAIN, &[0xE1A0_0000], &[], &[]), 2);
    }

    #[test]
    fn a_shift_by_register_costs_a_nonsequential_fetch_and_one_cycle() {
        assert_eq!(cost(MAIN, &[0xE1A0_0110], &[], &[]), 10);
    }

    #[test]
    fn a_load_outside_main_ram_adds_code_data_and_one_cycle() {
        assert_eq!(cost(WRAM, &[0xE591_0000], &[(1, WRAM + 0x1000)], &[]), 3);
    }

    #[test]
    fn a_load_from_main_ram_code_in_main_ram_adds_both_without_the_cycle() {
        assert_eq!(cost(MAIN, &[0xE591_0000], &[(1, MAIN + 0x1000)], &[]), 18);
    }

    #[test]
    fn a_store_across_main_ram_overlaps_code_and_data() {
        assert_eq!(cost(MAIN, &[0xE581_0000], &[(1, WRAM + 0x1000)], &[]), 9);
    }

    #[test]
    fn a_block_load_from_main_ram_overlaps_with_wram_code() {
        assert_eq!(cost(WRAM, &[0xE891_0005], &[(1, MAIN + 0x1000)], &[]), 11);
    }

    #[test]
    fn a_pop_into_pc_charges_its_load_in_the_code_it_returns_to() {
        let sp = WRAM + 0x2000;
        let cycles = thumb_cost(WRAM, &[0xBD00], &[(13, sp)], &[(sp, MAIN | 1)]);
        assert_eq!(cycles, 8 + 9);
    }

    #[test]
    fn an_irq_costs_only_the_vector_refill() {
        let mut buses = Buses::default();
        let state = CpuState {
            cpsr: ARM_MODE,
            pipeline: [0xE1A0_0000; 2],
            ..CpuState::default()
        };
        let mut cpu = Arm7Core::from_state(&CpuState {
            r: {
                let mut r = [0; 16];
                r[15] = MAIN + 8;
                r
            },
            ..state
        });
        assert_eq!(cpu.interrupt(&mut buses.arm7(), Exception::Irq), Some(2));
    }

    fn block_matches_single_words(buses: &Buses, code: u32, addr: u32, words: usize) {
        let mut block = buses.clone();
        let mut single = buses.clone();
        let (mut fast, mut slow) = ([0; 16], [0; 16]);
        let read = |buses: &mut Buses, values: &mut [u32], run: bool| {
            let mut bus = buses.arm7();
            bus.code_cycles(code, Width::Word, Access::Sequential);
            let cycles = if run {
                bus.timed_read_block(addr, values)
            } else {
                read_words(&mut bus, addr, values)
            };
            (cycles, bus.settle(cycles))
        };
        let cycles = (
            read(&mut block, &mut fast[..words], true),
            read(&mut single, &mut slow[..words], false),
        );
        assert_eq!((cycles.0, fast), (cycles.1, slow), "read {addr:08X}");
        let values: Vec<u32> = (1..=words as u32).map(|v| v * 0x0101_0101).collect();
        let write = |buses: &mut Buses, run: bool| {
            let mut bus = buses.arm7();
            bus.code_cycles(code, Width::Word, Access::Sequential);
            let cycles = if run {
                bus.timed_write_block(addr, &values)
            } else {
                write_words(&mut bus, addr, &values)
            };
            (cycles, bus.settle(cycles))
        };
        assert_eq!(
            write(&mut block, true),
            write(&mut single, false),
            "write {addr:08X}"
        );
        assert!(block.main_ram() == single.main_ram(), "main ram {addr:08X}");
        for at in (0..words as u32 + 4).map(|index| addr.wrapping_sub(8).wrapping_add(4 * index)) {
            let read = |buses: &mut Buses| buses.arm7().read(at, Width::Word);
            assert_eq!(
                read(&mut block),
                read(&mut single),
                "{at:08X} after {addr:08X}"
            );
        }
    }

    #[test]
    fn a_block_run_matches_single_words_inside_and_across_every_edge() {
        let mut buses = Buses::default();
        for addr in [0x0200_0100, 0x023F_FFF0, 0x037F_FFF0, 0x0380_FFF0] {
            for at in (addr..).step_by(4).take(8) {
                buses.arm7().write(at, Width::Word, at ^ 0x5A5A_5A5A);
            }
        }
        let edges = [
            (0x0200_0100, 8),
            (0x0200_0102, 3),
            (0x023F_FFF8, 4),
            (0x02FF_FFF8, 4),
            (0x037F_FFF8, 4),
            (0x0380_0010, 6),
            (0x0380_FFF8, 4),
            (0x0300_0010, 4),
            (0xFFFF_FFF8, 4),
        ];
        for code in [WRAM, MAIN] {
            for (addr, words) in edges {
                block_matches_single_words(&buses, code, addr, words);
            }
        }
    }

    #[cfg(jit)]
    #[test]
    fn a_block_store_over_compiled_code_makes_it_stale() {
        use crate::jit::{CodeRam, JitBus, PAGE_BITS};
        for (ram, base) in [(CodeRam::Main, 0x0200_0000), (CodeRam::Wram, WRAM)] {
            let mut buses = Buses::default();
            let mut bus = buses.arm7();
            bus.mark_code(ram, 2);
            let generation = bus.code_generation(ram, 2);
            let addr = base + (2 << PAGE_BITS) - 4;
            bus.timed_write_block(addr, &[1, 2, 3]);
            assert!(bus.take_code_hit(), "{ram:?}");
            assert_ne!(bus.code_generation(ram, 2), generation, "{ram:?}");
        }
    }
}
