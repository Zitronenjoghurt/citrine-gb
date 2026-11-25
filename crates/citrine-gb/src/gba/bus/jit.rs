use super::cpu_access::{IWRAM_PAGE, PLAIN_ROM_SPAN, PLAIN_ROM_START, ROM_MIRROR_MASK};
use super::{Bus, read_le};
use crate::gba::cart::Cart;
use crate::gba::prefetch::{CAPACITY, ROM_PAGE};
use crate::gba::region::{BIOS_SIZE, EWRAM_SIZE, IWRAM_SIZE, Region};
use crate::gba::wait::{EWRAM_CYCLES, PREFETCH, Width};
use crate::jit::{
    CODE_RAMS, CodeMarks, CodeRam, CodeSpan, Fast, FastBus, JitBus, Kept, PAGE_SIZE, Page,
    QuickFetch, Spec, Stamp, WINDOWS, Window,
};
use std::mem::offset_of;

const EWRAM_BASE: u32 = 0x0200_0000;
const IWRAM_BASE: u32 = 0x0300_0000;
const IO: u32 = 0x04;
const GAME_PAK: u32 = 0x08;
const EEPROM: u32 = 0x0D;
const _: () = assert!(offset_of!(Bus, open_bus.last) == offset_of!(Bus, open_bus.addr) + 4);

static NO_STALL: u32 = 0;

#[derive(Debug, Clone, Default)]
pub(super) struct Code {
    ewram: CodeMarks,
    iwram: CodeMarks,
    rom: u32,
    fast: Kept,
}

impl Code {
    #[inline(always)]
    pub(super) fn written(&mut self, region: Region, offset: usize) {
        match region {
            Region::Ewram => self.ewram.written(offset),
            Region::Iwram => self.iwram.written(offset),
            _ => {}
        }
    }

    pub(super) fn rom_changed(&mut self) {
        self.rom = self.rom.wrapping_add(1);
        self.fast.moved();
    }

    pub(super) fn moved(&mut self) {
        self.fast.moved();
    }
}

impl Bus {
    pub(crate) fn jit_budget(&self) -> u32 {
        let line = self.interrupts.until_line().unwrap_or(u32::MAX);
        let event = if self.quiet > self.owed {
            self.quiet
        } else {
            self.until_event()
        };
        event
            .min(line)
            .min(self.dma_check)
            .saturating_sub(self.owed)
    }

    fn rom_span(&self, addr: u32) -> Option<usize> {
        if addr >> 24 == EEPROM {
            return None;
        }
        let offset = Region::of(addr).offset(addr);
        let word = offset & !3;
        let inside = word + 4 <= self.rom.len();
        (inside && !Cart::in_gpio(word) && !Cart::in_gpio(word + 3)).then_some(offset)
    }
}

impl JitBus for Bus {
    const TIMED_FETCH: bool = true;
    const QUICK_FETCH: Option<QuickFetch> = Some(QuickFetch {
        now: offset_of!(Bus, clock.now) as i32,
        sample: offset_of!(Bus, clock.sample) as i32,
        dma_check: offset_of!(Bus, dma_check) as i32,
        plain_rom_end: offset_of!(Bus, plain_rom_end) as i32,
        head: offset_of!(Bus, prefetch.head) as i32,
        count: offset_of!(Bus, prefetch.count) as i32,
        clock: offset_of!(Bus, prefetch.clock) as i32,
        due: offset_of!(Bus, prefetch.due) as i32,
        since: offset_of!(Bus, prefetch.since) as i32,
        progress: offset_of!(Bus, prefetch.progress) as i32,
        cost: offset_of!(Bus, prefetch.cost) as i32,
        paused: offset_of!(Bus, prefetch.paused) as i32,
        branched: offset_of!(Bus, prefetch.branched) as i32,
        stuck: offset_of!(Bus, prefetch.stuck) as i32,
        following: offset_of!(Bus, prefetch.following) as i32,
        flowing: offset_of!(Bus, prefetch.flowing) as i32,
        active: offset_of!(Bus, prefetch.active) as i32,
        force_n: offset_of!(Bus, prefetch.force_n) as i32,
        parked: offset_of!(Bus, prefetch.parked) as i32,
        page_cost: offset_of!(Bus, prefetch.page_cost) as i32,
        open_addr: offset_of!(Bus, open_bus.addr) as i32,
        open_last: offset_of!(Bus, open_bus.last) as i32,
        open_iwram: offset_of!(Bus, open_bus.iwram) as i32,
        open_bios: offset_of!(Bus, open_bus.bios) as i32,
        wait_bits: offset_of!(Bus, wait.bits) as i32,
        first_halves: offset_of!(Bus, wait.first_halves) as i32,
        sequential_halves: offset_of!(Bus, wait.sequential_halves) as i32,
        code_hits: [
            (offset_of!(Bus, code.ewram) + CodeMarks::HIT) as i32,
            (offset_of!(Bus, code.iwram) + CodeMarks::HIT) as i32,
        ],
        prefetch_bit: PREFETCH as u32,
        iwram_page: IWRAM_PAGE,
        bios_end: BIOS_SIZE as u32,
        rom_start: PLAIN_ROM_START,
        rom_span: PLAIN_ROM_SPAN,
        rom_mirror: ROM_MIRROR_MASK,
        rom_skip: Cart::PLAIN_ROM_SKIP as u32,
        rom_page: ROM_PAGE,
        capacity: CAPACITY,
    });
    const WRITABLE: &'static [CodeRam] = &[CodeRam::Main, CodeRam::Wram, CodeRam::Rom];

    fn flat_code(pc: u32) -> bool {
        pc >> 24 == IWRAM_PAGE
    }

    fn leaves(addr: u32, store: bool) -> bool {
        let area = addr >> 24;
        store && (area == IO || area >= GAME_PAK)
    }

    fn code_layout(&self) -> u64 {
        0
    }

    fn code_span(&self, addr: u32) -> Option<CodeSpan> {
        let region = Region::of(addr);
        let (ram, offset) = match region {
            Region::Bios => (CodeRam::Bios, region.offset(addr)),
            Region::Ewram => (CodeRam::Main, region.offset(addr)),
            Region::Iwram => (CodeRam::Wram, region.offset(addr)),
            Region::Rom(_) => (CodeRam::Rom, self.rom_span(addr)?),
            _ => return None,
        };
        Some(CodeSpan {
            ram,
            offset: offset as u32,
        })
    }

    fn code_word(&self, span: CodeSpan) -> u32 {
        let offset = span.offset as usize & !3;
        match span.ram {
            CodeRam::Bios => read_le(&self.bios, offset, Width::Word),
            CodeRam::Main => read_le(&self.ewram, offset, Width::Word),
            CodeRam::Wram => read_le(&self.iwram, offset, Width::Word),
            CodeRam::Rom => read_le(&self.rom, offset, Width::Word),
            CodeRam::Itcm | CodeRam::SharedWram => 0,
        }
    }

    fn rom_literal(&self, addr: u32) -> Option<u32> {
        if addr & 3 != 0 || addr.wrapping_sub(PLAIN_ROM_START) >= PLAIN_ROM_SPAN {
            return None;
        }
        let offset = (addr & ROM_MIRROR_MASK) as usize;
        Cart::in_plain_rom(offset, self.plain_rom_end)
            .then(|| read_le(&self.rom, offset, Width::Word))
    }

    fn mark_code(&mut self, ram: CodeRam, page: usize) -> u32 {
        match ram {
            CodeRam::Main => self.code.ewram.mark(page, EWRAM_SIZE),
            CodeRam::Wram => self.code.iwram.mark(page, IWRAM_SIZE),
            CodeRam::Rom => self.code.rom,
            CodeRam::Bios | CodeRam::Itcm | CodeRam::SharedWram => 0,
        }
    }

    fn code_generation(&self, ram: CodeRam, page: usize) -> Option<u32> {
        match ram {
            CodeRam::Main => self.code.ewram.generation(page),
            CodeRam::Wram => self.code.iwram.generation(page),
            CodeRam::Rom => Some(self.code.rom),
            CodeRam::Bios => Some(0),
            CodeRam::Itcm | CodeRam::SharedWram => None,
        }
    }

    fn take_code_hit(&mut self) -> bool {
        let ewram = self.code.ewram.take_hit();
        let iwram = self.code.iwram.take_hit();
        ewram | iwram
    }

    fn take_stall(&mut self) -> u32 {
        0
    }

    fn stall_flag(&self) -> *const u32 {
        &NO_STALL
    }

    #[inline(always)]
    fn fetch_flat(&mut self, addr: u32, width: Width, count: u32, cycles: u32, room: u32) -> bool {
        self.iwram_run(addr, width, count, cycles, room)
    }

    fn code_stamp(&self, ram: CodeRam) -> u32 {
        match ram {
            CodeRam::Main => self.code.ewram.stamp(),
            CodeRam::Wram => self.code.iwram.stamp(),
            CodeRam::Rom => self.code.rom,
            CodeRam::Bios | CodeRam::Itcm | CodeRam::SharedWram => 0,
        }
    }

    #[inline(always)]
    fn code_quiet(&self, seen: &[u32; CODE_RAMS]) -> bool {
        self.code.ewram.quiet(seen[CodeRam::Main as usize])
            & self.code.iwram.quiet(seen[CodeRam::Wram as usize])
            & (self.code.rom == seen[CodeRam::Rom as usize])
    }

    fn written_page(&self, ram: CodeRam, stamp: u32) -> Option<usize> {
        match ram {
            CodeRam::Main => self.code.ewram.written_page(stamp),
            CodeRam::Wram => self.code.iwram.written_page(stamp),
            CodeRam::Bios | CodeRam::Itcm | CodeRam::SharedWram | CodeRam::Rom => None,
        }
    }
}

impl Bus {
    #[cold]
    #[inline(never)]
    fn refill_fast(&mut self, stamp: Stamp) -> *mut Fast {
        let ewram = self.ewram.raw();
        let iwram = self.iwram.raw();
        let pages = self.code.fast.pages();
        for ((bytes, len, dirty), hardware, code, base, meta) in [
            (
                ewram,
                EWRAM_SIZE,
                self.code.ewram.pages(),
                EWRAM_BASE,
                Page::meta(EWRAM_CYCLES, 2 * EWRAM_CYCLES, 0),
            ),
            (
                iwram,
                IWRAM_SIZE,
                self.code.iwram.pages(),
                IWRAM_BASE,
                Page::meta(1, 1, 0),
            ),
        ] {
            if len != hardware {
                continue;
            }
            let window = Window {
                bytes,
                dirty,
                code,
                size: len as u64,
                base,
            };
            for page in 0..len as u32 / PAGE_SIZE {
                let addr = base + page * PAGE_SIZE;
                pages.set(addr, Page::of(&window, len as u32, addr, meta));
            }
        }
        let table = Fast {
            pages: pages.table(),
            rom: self.rom.as_ptr(),
            bios: self.bios.as_ptr(),
            ..Fast::CLOSED
        };
        self.code.fast.keep(stamp, table)
    }
}

impl FastBus for Bus {
    const SPECS: [Spec; WINDOWS] = [Spec {
        len: 0,
        code: false,
        cache: None,
        half: [0; 2],
        word: [0; 2],
    }; WINDOWS];

    fn fast(&mut self) -> *mut Fast {
        let (ewram, iwram) = (self.ewram.raw(), self.iwram.raw());
        let inputs = [
            ewram.0.addr(),
            iwram.0.addr(),
            self.code.ewram.pages().addr(),
            self.code.iwram.pages().addr(),
        ];
        let stamp = self.code.fast.stamp(&self.code, &self.code, inputs);
        match self.code.fast.get(stamp) {
            Some(table) => table,
            None => self.refill_fast(stamp),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::bios::CULT_OF_GBA;
    use crate::gba::prefetch::Prefetch;
    use crate::gba::wait::{Access, WaitControl};
    use crate::jit::{PAGE_BITS, WORD_CYCLES};
    use crate::paged::Paged;
    use std::sync::Arc;

    const NEVER: u64 = u64::MAX;
    const BASES: [u32; 6] = [
        0x0800_0100,
        0x0801_FFF0,
        0x09FF_FFF0,
        0x0A00_0100,
        0x0C01_FFF0,
        0x0CFF_FFE0,
    ];

    struct Lcg(u64);

    impl Lcg {
        fn below(&mut self, n: u32) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) % u64::from(n)) as u32
        }

        fn word(&mut self) -> u32 {
            self.below(1 << 16) << 16 | self.below(1 << 16)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Path {
        Hit,
        Flight,
        Settled,
        Paused,
        Drained,
        Elsewhere,
    }

    type Stubbed = Option<(Prefetch, u32, Path)>;

    fn half(table: u32, addr: u32) -> u32 {
        table >> (8 * (addr >> 25 & 3)) & 0xFF
    }

    fn thumb_stub(prefetch: &Prefetch, wait: WaitControl, t: u32, sequential: bool) -> Stubbed {
        let mut p = *prefetch;
        if t != p.head {
            return elsewhere(p, wait, t, sequential);
        }
        if (p.clock.wrapping_sub(p.due) | p.due) >> 63 == 0 {
            return settled(p, t);
        }
        if p.count == 0 {
            return flight(p, t);
        }
        if p.count < 1 + u32::from(p.paused) {
            return Some(drained(p));
        }
        p.branched = false;
        p.count -= 1;
        p.head = t + 2;
        p.clock += 1;
        Some((p, 1, Path::Hit))
    }

    fn flight(mut p: Prefetch, t: u32) -> Stubbed {
        if (t + 2).is_multiple_of(ROM_PAGE) || p.branched || p.stuck {
            return None;
        }
        p.following |= p.since.wrapping_sub(p.clock) >> 63 != 0 || p.progress != 0;
        let cycles = p.due.wrapping_sub(p.clock) as u32;
        p.progress = 0;
        p.since = p.clock;
        p.head = t + 2;
        p.due = p.clock + u64::from(p.cost);
        Some((p, cycles, Path::Flight))
    }

    fn settled(mut p: Prefetch, t: u32) -> Stubbed {
        if !p.flowing || p.stuck || p.cost == 0 {
            return None;
        }
        let cost = u64::from(p.cost);
        let mut d = p.clock - p.due;
        let mut c = p.count;
        let path = loop {
            c += 1;
            if c == CAPACITY {
                break Path::Paused;
            }
            if c > CAPACITY || (t + 2 * c).is_multiple_of(ROM_PAGE) {
                return None;
            }
            if d < cost {
                break Path::Settled;
            }
            d -= cost;
        };
        if path == Path::Paused {
            p.count = CAPACITY - 1;
            p.progress = 0;
            p.paused = true;
            p.flowing = false;
            p.due = NEVER;
        } else {
            p.count = c - 1;
            p.progress = d as u32;
            p.due = p.clock + cost - d;
        }
        p.since = p.clock;
        p.following = true;
        p.branched = false;
        p.head = t + 2;
        p.clock += 1;
        Some((p, 1, path))
    }

    fn elsewhere(mut p: Prefetch, wait: WaitControl, t: u32, sequential: bool) -> Stubbed {
        if wait.bits & PREFETCH == 0 || p.flowing || (t + 2).is_multiple_of(ROM_PAGE) {
            return None;
        }
        let cycles = if sequential && !p.force_n && !t.is_multiple_of(ROM_PAGE) {
            half(wait.sequential_halves, t)
        } else {
            half(wait.first_halves, t)
        };
        p.cost = half(wait.sequential_halves, t);
        p.since = p.clock;
        p.due = p.clock + u64::from(p.cost);
        p.page_cost = half(wait.sequential_halves, (t | (ROM_PAGE - 1)) + 1);
        p.head = t + 2;
        p.count = 0;
        p.progress = 0;
        p.parked = 0;
        p.paused = false;
        p.branched = false;
        p.stuck = false;
        p.following = false;
        p.force_n = false;
        p.active = true;
        p.flowing = true;
        Some((p, cycles, Path::Elsewhere))
    }

    fn drained(p: Prefetch) -> (Prefetch, u32, Path) {
        let drained = Prefetch {
            clock: p.clock + 1,
            since: p.clock,
            due: NEVER,
            force_n: true,
            ..Prefetch::default()
        };
        (drained, 1, Path::Drained)
    }

    fn in_rom(addr: u32) -> bool {
        addr.wrapping_sub(0x0800_0000) < 0x0500_0000
    }

    fn rom_fetch(p: &mut Prefetch, wait: WaitControl, addr: u32, width: Width, access: Access) {
        let aligned = addr & !(width.bytes() as u32 - 1);
        let halves = if width == Width::Word { 2 } else { 1 };
        if p.fetch_ahead(aligned, halves).is_none()
            && p.fetch_elsewhere(wait, addr, aligned, width, access)
                .is_none()
        {
            p.fetch(wait, addr, width, access);
        }
    }

    fn evolve(p: &mut Prefetch, wait: &mut WaitControl, rng: &mut Lcg, t: u32, access: Access) {
        match rng.below(20) {
            0..=7 => {
                let width = if rng.below(4) == 0 {
                    Width::Word
                } else {
                    Width::Half
                };
                if in_rom(t) && rng.below(4) != 0 {
                    rom_fetch(p, *wait, t, width, access);
                } else {
                    p.fetch(*wait, t, width, access);
                }
            }
            8..=10 => {
                let cycles = if rng.below(6) == 0 {
                    rng.below(60)
                } else {
                    rng.below(8)
                };
                p.run(cycles);
            }
            11 => p.fetch_in_one(*wait),
            12 | 13 => p.branch(),
            14 => {
                p.interrupt();
            }
            15 => {
                let addr =
                    [0x0000_0128, 0x0200_0000, 0x0300_0000, 0x0E00_0000][rng.below(4) as usize];
                p.fetch(*wait, addr, Width::Half, access);
            }
            16 => {
                let prefetch = if rng.below(5) == 0 { 0 } else { PREFETCH };
                wait.write(rng.below(0x4000) as u16 | prefetch, 0xFFFF);
                p.retime(*wait);
            }
            17 => {
                *p = Prefetch::from(p.saved());
                p.retime(*wait);
            }
            _ => p.stop(),
        }
    }

    #[test]
    fn the_inline_thumb_fetch_matches_the_buffer() {
        let mut seen = [0usize; 6];
        for seed in 0..300u64 {
            let mut rng = Lcg(seed);
            let mut wait = WaitControl::default();
            wait.write(rng.below(0x4000) as u16 | PREFETCH, 0xFFFF);
            let mut prefetch = Prefetch::default();
            let mut streak = 0;
            for step in 0..3000 {
                let head = prefetch.head;
                if streak == 0 && rng.below(40) == 0 {
                    streak = 1 + rng.below(12);
                }
                let t = if streak > 0 && in_rom(head) {
                    head
                } else if head != 0 && rng.below(4) != 0 {
                    head.wrapping_add(2 * rng.below(3))
                        .wrapping_sub(2 * rng.below(2))
                } else {
                    BASES[rng.below(BASES.len() as u32) as usize] + 2 * rng.below(16)
                };
                let sequential = rng.below(3) != 0;
                let access = if sequential {
                    Access::Sequential
                } else {
                    Access::NonSequential
                };
                if in_rom(t) {
                    let mut interpreted = prefetch;
                    let cycles = interpreted
                        .fetch_ahead(t, 1)
                        .or_else(|| interpreted.fetch_elsewhere(wait, t, t, Width::Half, access));
                    if let Some((stubbed, stub_cycles, path)) =
                        thumb_stub(&prefetch, wait, t, sequential)
                    {
                        seen[path as usize] += 1;
                        assert_eq!(
                            cycles,
                            Some(stub_cycles),
                            "cycles, {path:?}, seed {seed} step {step}"
                        );
                        assert_eq!(
                            format!("{stubbed:?}"),
                            format!("{interpreted:?}"),
                            "state, {path:?}, seed {seed} step {step}"
                        );
                    }
                    if streak > 0 && t == head && cycles.is_some() {
                        streak -= 1;
                        prefetch = interpreted;
                        continue;
                    }
                }
                streak = 0;
                evolve(&mut prefetch, &mut wait, &mut rng, t, access);
            }
        }
        assert!(seen.iter().all(|&count| count > 0), "{seen:?}");
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Load {
        Stopped,
        Paused,
        Even,
        Glitch,
        Full,
    }

    fn rom_load(
        p: &Prefetch,
        wait: WaitControl,
        addr: u32,
        width: Width,
    ) -> Option<(Prefetch, u32, Load)> {
        let (glitch, path) = if !p.flowing {
            if p.active && !p.paused {
                return None;
            }
            let path = if p.active {
                Load::Paused
            } else {
                Load::Stopped
            };
            (0, path)
        } else {
            if p.stuck || p.cost == 0 {
                return None;
            }
            let mut d = p.since + u64::from(p.cost - p.progress);
            let mut c = p.count;
            loop {
                if p.clock < d {
                    break if d - p.clock == 1 {
                        (1, Load::Glitch)
                    } else {
                        (0, Load::Even)
                    };
                }
                c += 1;
                if c == CAPACITY {
                    break (0, Load::Full);
                }
                if c > CAPACITY || p.head.wrapping_add(2 * c).is_multiple_of(ROM_PAGE) {
                    return None;
                }
                d += u64::from(p.cost);
            }
        };
        let mut cycles = half(wait.first_halves, addr);
        if width == Width::Word {
            cycles += half(wait.sequential_halves, addr);
        }
        let stopped = Prefetch {
            clock: p.clock + 1,
            since: p.clock,
            due: NEVER,
            force_n: p.force_n,
            ..Prefetch::default()
        };
        Some((stopped, cycles + glitch + 1, path))
    }

    #[test]
    fn the_inline_rom_load_matches_interrupt() {
        let mut seen = [0usize; 5];
        for seed in 0..300u64 {
            let mut rng = Lcg(seed);
            let mut wait = WaitControl::default();
            wait.write(rng.below(0x4000) as u16 | PREFETCH, 0xFFFF);
            let mut prefetch = Prefetch::default();
            for step in 0..3000 {
                let head = prefetch.head;
                let t = if head != 0 && rng.below(4) != 0 {
                    head.wrapping_add(2 * rng.below(3))
                        .wrapping_sub(2 * rng.below(2))
                } else {
                    BASES[rng.below(BASES.len() as u32) as usize] + 2 * rng.below(16)
                };
                let addr = BASES[rng.below(BASES.len() as u32) as usize] + rng.below(32);
                let width = [Width::Byte, Width::Half, Width::Word][rng.below(3) as usize];
                if let Some((stubbed, cycles, path)) = rom_load(&prefetch, wait, addr, width) {
                    seen[path as usize] += 1;
                    let mut interpreted = prefetch;
                    let wanted =
                        wait.region_cycles(Region::of(addr), addr, width, Access::NonSequential)
                            + interpreted.interrupt()
                            + 1;
                    interpreted.run(1);
                    assert_eq!(cycles, wanted, "cycles, {path:?}, seed {seed} step {step}");
                    assert_eq!(
                        format!("{stubbed:?}"),
                        format!("{interpreted:?}"),
                        "state, {path:?}, seed {seed} step {step}"
                    );
                    if rng.below(4) == 0 {
                        prefetch = interpreted;
                    }
                }
                let access = if rng.below(3) != 0 {
                    Access::Sequential
                } else {
                    Access::NonSequential
                };
                evolve(&mut prefetch, &mut wait, &mut rng, t, access);
            }
        }
        assert!(seen.iter().all(|&count| count > 0), "{seen:?}");
    }

    const BLOCK_BASES: [u32; 9] = [
        0x0300_0000,
        0x0300_03F8,
        0x0300_0FF4,
        0x0300_7FF8,
        0x0300_8100,
        0x0200_0000,
        0x0200_0FFC,
        0x0203_FFF0,
        0x0204_0100,
    ];

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Moved {
        IwramLoad,
        IwramStore,
        EwramLoad,
        EwramStore,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Bail {
        Unmapped,
        Crosses,
        CodeFirst,
        CodeLast,
        Hit,
        Dma,
    }

    fn ram_block(
        bus: &mut Bus,
        addr: u32,
        n: u32,
        store: bool,
        values: &mut [u32],
    ) -> Result<(u32, Moved), Bail> {
        let q = <Bus as JitBus>::QUICK_FETCH.expect("the GBA bus has quick fetches");
        let pages = unsafe { (*bus.fast()).pages };
        if pages.is_null() {
            return Err(Bail::Unmapped);
        }
        let page = unsafe { *pages.add((addr / PAGE_SIZE) as usize) };
        if page.meta == 0 {
            return Err(Bail::Unmapped);
        }
        if ((addr & (PAGE_SIZE - 1) & !3) + 4 * (n - 1)) & PAGE_SIZE != 0 {
            return Err(Bail::Crosses);
        }
        if store {
            let marked = |at: u32| unsafe { *page.code.add((at >> PAGE_BITS & 3) as usize) };
            if marked(addr) {
                return Err(Bail::CodeFirst);
            }
            if marked(addr + 4 * (n - 1)) {
                return Err(Bail::CodeLast);
            }
            let base = std::ptr::from_ref::<Bus>(bus).cast::<u8>();
            if q.code_hits
                .iter()
                .any(|&hit| unsafe { base.add(hit as usize).read() } != 0)
            {
                return Err(Bail::Hit);
            }
        }
        let c = u32::from((page.meta >> WORD_CYCLES) as u8);
        let t = c * n;
        let end = u64::from(bus.clock.now) + u64::from(t) - if store { u64::from(c) } else { 0 };
        if end >= u64::from(bus.dma_check) {
            return Err(Bail::Dma);
        }
        let end = end as u32;
        let host = std::ptr::with_exposed_provenance_mut::<u32>(
            page.addend.wrapping_add(u64::from(addr & !3)) as usize,
        );
        for (index, value) in values.iter_mut().enumerate() {
            let word = unsafe { host.add(index) };
            if store {
                unsafe { word.write_unaligned(*value) };
            } else {
                *value = unsafe { word.read_unaligned() };
            }
        }
        let iwram = addr & 1 << 24 != 0;
        let last = values[n as usize - 1];
        bus.open_bus.last = last;
        if iwram {
            bus.open_bus.iwram = last;
        }
        let cycles = if store {
            unsafe { page.dirty.write(1) };
            bus.clock.sample = end + 1;
            bus.clock.now = end + c;
            t
        } else {
            bus.clock.now = end + 1;
            bus.clock.sample = end + 1;
            t + 1
        };
        bus.prefetch.clock += u64::from(cycles);
        let moved = match (iwram, store) {
            (true, false) => Moved::IwramLoad,
            (true, true) => Moved::IwramStore,
            (false, false) => Moved::EwramLoad,
            (false, true) => Moved::EwramStore,
        };
        Ok((cycles, moved))
    }

    fn word_helpers(bus: &mut Bus, addr: u32, store: bool, values: &mut [u32]) -> u32 {
        let mut cycles = 0;
        for (index, value) in values.iter_mut().enumerate() {
            let at = addr + 4 * index as u32;
            let access = if index == 0 {
                Access::NonSequential
            } else {
                Access::Sequential
            };
            if store {
                cycles += JitBus::jit_write(bus, at, Width::Word, *value, access);
            } else {
                let (spent, loaded) = bus.timed_data(at, Width::Word, access);
                cycles += spent;
                *value = loaded;
            }
        }
        if !store {
            bus.cpu_idle();
            cycles += 1;
        }
        cycles
    }

    fn dirty(ram: &mut Paged) -> Vec<u8> {
        let (_, len, dirty) = ram.raw();
        unsafe { std::slice::from_raw_parts(dirty, len.div_ceil(PAGE_SIZE as usize)) }.to_vec()
    }

    fn put_word(bus: &mut Bus, addr: u32, value: u32) {
        let offset = Region::of(addr).offset(addr) & !3;
        let ram = if addr >> 24 == IWRAM_BASE >> 24 {
            &mut bus.iwram
        } else {
            &mut bus.ewram
        };
        ram.touch(offset)[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn mark(bus: &mut Bus, addr: u32, hit: bool) {
        let page = Region::of(addr).offset(addr) >> PAGE_BITS;
        let marks = if addr >> 24 == IWRAM_BASE >> 24 {
            bus.code.iwram.mark(page, IWRAM_SIZE);
            &mut bus.code.iwram
        } else {
            bus.code.ewram.mark(page, EWRAM_SIZE);
            &mut bus.code.ewram
        };
        if hit {
            marks.written(page << PAGE_BITS);
        }
    }

    #[test]
    fn the_inline_ram_block_matches_the_word_helpers() {
        let mut moved = [0usize; 4];
        let mut bails = [0usize; 6];
        for seed in 0..200u64 {
            let mut rng = Lcg(seed);
            let mut bus = Bus::new(&[0; 0x200]);
            bus.wait.write(rng.below(0x4000) as u16 | PREFETCH, 0xFFFF);
            for step in 0..24 {
                if rng.below(4) == 0 {
                    bus.code = Code::default();
                }
                for _ in 0..rng.below(4) {
                    let t = BASES[rng.below(BASES.len() as u32) as usize] + 2 * rng.below(16);
                    let access = if rng.below(3) != 0 {
                        Access::Sequential
                    } else {
                        Access::NonSequential
                    };
                    evolve(&mut bus.prefetch, &mut bus.wait, &mut rng, t, access);
                }
                let addr = BLOCK_BASES[rng.below(BLOCK_BASES.len() as u32) as usize] + rng.below(4);
                let n = 1 + rng.below(16);
                let store = rng.below(2) == 0;
                bus.clock.now = rng.below(1 << 30);
                bus.clock.sample = rng.below(1 << 30);
                bus.dma_check = bus.clock.now + rng.below(48);
                bus.open_bus.last = rng.word();
                bus.open_bus.iwram = rng.word();
                if rng.below(2) == 0 {
                    for index in 0..n {
                        let value = rng.word();
                        put_word(&mut bus, addr + 4 * index, value);
                    }
                }
                match rng.below(8) {
                    0 => mark(&mut bus, addr, false),
                    1 => mark(&mut bus, addr + 4 * (n - 1), false),
                    2 => mark(&mut bus, addr + 4 * (n - 1), true),
                    _ => {}
                }
                let mut values: Vec<u32> = (0..n).map(|_| rng.word()).collect();
                let mut wanted = values.clone();
                let mut reference = bus.clone();
                match ram_block(&mut bus, addr, n, store, &mut values) {
                    Ok((cycles, path)) => {
                        moved[path as usize] += 1;
                        let at = format!("{path:?} at {addr:#010x} x{n}, seed {seed} step {step}");
                        let spent = word_helpers(&mut reference, addr, store, &mut wanted);
                        if store {
                            assert!(!JitBus::take_code_hit(&mut reference), "hit, {at}");
                        }
                        assert_eq!(cycles, spent, "cycles, {at}");
                        assert_eq!(values, wanted, "values, {at}");
                        assert_eq!(bus.clock.now, reference.clock.now, "now, {at}");
                        assert_eq!(bus.clock.sample, reference.clock.sample, "sample, {at}");
                        assert_eq!(
                            format!("{:?}", bus.prefetch),
                            format!("{:?}", reference.prefetch),
                            "prefetch, {at}"
                        );
                        assert_eq!(bus.open_bus, reference.open_bus, "open bus, {at}");
                        assert!(bus.iwram[..] == reference.iwram[..], "iwram, {at}");
                        assert!(bus.ewram[..] == reference.ewram[..], "ewram, {at}");
                        assert_eq!(
                            dirty(&mut bus.iwram),
                            dirty(&mut reference.iwram),
                            "iwram dirty, {at}"
                        );
                        assert_eq!(
                            dirty(&mut bus.ewram),
                            dirty(&mut reference.ewram),
                            "ewram dirty, {at}"
                        );
                    }
                    Err(bail) => bails[bail as usize] += 1,
                }
                JitBus::take_code_hit(&mut bus);
            }
        }
        assert!(moved.iter().all(|&count| count > 0), "{moved:?}");
        assert!(bails.iter().all(|&count| count > 0), "{bails:?}");
    }

    const BIOS_TARGETS: [u32; 9] = [
        0x0000, 0x0008, 0x0018, 0x0128, 0x013C, 0x0140, 0x3FF8, 0x3FFC, 0x4000,
    ];

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum BiosBail {
        Outside,
        Unmapped,
        Dma,
        Off,
    }

    fn bios_branch(bus: &mut Bus, t: u32) -> Result<(u32, [u32; 2]), BiosBail> {
        let q = <Bus as JitBus>::QUICK_FETCH.expect("the GBA bus has quick fetches");
        if t > q.bios_end - 8 {
            return Err(BiosBail::Outside);
        }
        let bios = unsafe { (*bus.fast()).bios };
        if bios.is_null() {
            return Err(BiosBail::Unmapped);
        }
        let read =
            |at: u32| unsafe { std::ptr::read_unaligned(bios.add(at as usize).cast::<u32>()) };
        let words = [read(t), read(t + 4)];
        let now = bus.clock.now;
        if u64::from(now) + 1 >= u64::from(bus.dma_check) {
            return Err(BiosBail::Dma);
        }
        if !bus.wait.prefetch() {
            return Err(BiosBail::Off);
        }
        bus.clock.now = now + 2;
        bus.clock.sample = now + 2;
        bus.prefetch.clock += 2;
        bus.prefetch.branched = false;
        bus.open_bus.addr = t + 4;
        bus.open_bus.bios = words[1];
        bus.open_bus.last = words[1];
        Ok((2, words))
    }

    #[test]
    fn the_inline_bios_branch_matches_the_refill() {
        let mut inline = 0usize;
        let mut bails = [0usize; 4];
        for seed in 0..300u64 {
            let mut rng = Lcg(seed);
            let mut bus = Bus::new(&[0; 0x200]);
            bus.load_bios(&CULT_OF_GBA[..]);
            let prefetch = if rng.below(5) == 0 { 0 } else { PREFETCH };
            bus.wait.write(rng.below(0x4000) as u16 | prefetch, 0xFFFF);
            for step in 0..40 {
                for _ in 0..1 + rng.below(3) {
                    let t = BASES[rng.below(BASES.len() as u32) as usize] + 2 * rng.below(16);
                    let access = if rng.below(3) != 0 {
                        Access::Sequential
                    } else {
                        Access::NonSequential
                    };
                    evolve(&mut bus.prefetch, &mut bus.wait, &mut rng, t, access);
                }
                let t = if rng.below(3) == 0 {
                    4 * rng.below(0x1000)
                } else {
                    BIOS_TARGETS[rng.below(BIOS_TARGETS.len() as u32) as usize]
                };
                bus.clock.now = rng.below(1 << 30);
                bus.clock.sample = rng.below(1 << 30);
                bus.dma_check = if rng.below(4) == 0 {
                    bus.clock.now + rng.below(4)
                } else {
                    bus.clock.now + (1 << 20)
                };
                bus.open_bus.addr = rng.word();
                bus.open_bus.last = rng.word();
                bus.open_bus.bios = rng.word();
                bus.open_bus.iwram = rng.word();
                let mut reference = bus.clone();
                match bios_branch(&mut bus, t) {
                    Ok((cycles, words)) => {
                        inline += 1;
                        let at = format!("at {t:#06x}, seed {seed} step {step}");
                        reference.branch();
                        let (first, w1) =
                            reference.timed_code(t, Width::Word, Access::NonSequential);
                        let (second, w2) =
                            reference.timed_code(t + 4, Width::Word, Access::Sequential);
                        assert_eq!(cycles, first + second, "cycles, {at}");
                        assert_eq!(words, [w1, w2], "words, {at}");
                        assert_eq!(
                            format!("{:?}", bus.prefetch),
                            format!("{:?}", reference.prefetch),
                            "prefetch, {at}"
                        );
                        assert_eq!(bus.clock.now, reference.clock.now, "now, {at}");
                        assert_eq!(bus.clock.sample, reference.clock.sample, "sample, {at}");
                        assert_eq!(bus.open_bus, reference.open_bus, "open bus, {at}");
                    }
                    Err(bail) => bails[bail as usize] += 1,
                }
            }
        }
        assert!(inline > 0, "{bails:?}");
        for bail in [BiosBail::Outside, BiosBail::Dma, BiosBail::Off] {
            assert!(bails[bail as usize] > 0, "{bail:?}, {bails:?}");
        }
    }

    #[test]
    fn ram_of_the_wrong_size_gets_no_fast_pages() {
        let wrong = [
            (EWRAM_BASE, EWRAM_SIZE * 4),
            (EWRAM_BASE, PAGE_SIZE as usize),
            (IWRAM_BASE, IWRAM_SIZE * 8),
            (IWRAM_BASE, PAGE_SIZE as usize),
        ];
        for (case, (base, len)) in wrong.into_iter().enumerate() {
            let mut bus = Bus::new(&[0; 0x200]);
            if base == EWRAM_BASE {
                bus.ewram = Paged::zeroed(len);
            } else {
                bus.iwram = Paged::zeroed(len);
            }
            let pages = unsafe { (*bus.fast()).pages };
            for page in 0..0x40 {
                let entry = unsafe { *pages.add((base / PAGE_SIZE + page) as usize) };
                assert_eq!(entry.meta, 0, "case {case}, page {page}");
            }
        }
    }

    #[test]
    fn the_inline_bios_reads_follow_a_new_bios() {
        let mut bus = Bus::new(&[0; 0x200]);
        bus.load_bios(&CULT_OF_GBA[..]);
        assert_eq!(unsafe { (*bus.fast()).bios }, bus.bios.as_ptr());
        let old = Arc::clone(&bus.bios);
        bus.load_bios(&[0xA5; 0x100]);
        assert!(!Arc::ptr_eq(&old, &bus.bios));
        assert_eq!(unsafe { (*bus.fast()).bios }, bus.bios.as_ptr());
        let mut other = Bus::new(&[0; 0x200]);
        other.adopt_rom(&bus);
        other.load_bios(&[0x5A; 0x100]);
        let held = Arc::clone(&bus.bios);
        assert!(!bus.adopt_rom(&other));
        assert!(!Arc::ptr_eq(&held, &bus.bios));
        assert!(Arc::ptr_eq(&other.bios, &bus.bios));
        assert_eq!(unsafe { (*bus.fast()).bios }, bus.bios.as_ptr());
    }

    #[test]
    fn rom_literals_bake_only_where_the_stub_reads_plain_rom() {
        let rom: Vec<u8> = (0..0x2_0102usize).map(|i| (i * 7 + 3) as u8).collect();
        let mut bus = Bus::new(&rom);
        let q = <Bus as JitBus>::QUICK_FETCH.expect("the GBA bus has quick fetches");
        let stub = |bus: &Bus, addr: u32| {
            let offset = (addr & q.rom_mirror).wrapping_sub(q.rom_skip);
            addr & 3 == 0
                && addr.wrapping_sub(q.rom_start) < q.rom_span
                && (offset as usize) < bus.plain_rom_end
        };
        for addr in [
            0x0800_00C0,
            0x0800_00C4,
            0x0800_00C8,
            0x0802_0100,
            0x0802_0104,
            0x0D00_0000,
            0x0E00_0000,
            0x0800_00CE,
            0x0CFF_FFFC,
        ] {
            assert_eq!(bus.rom_literal(addr), None, "{addr:#010x}");
        }
        for addr in [0x0800_00CC, 0x0802_00FC, 0x0A00_00CC, 0x0C02_00FC] {
            let offset = (addr & ROM_MIRROR_MASK) as usize;
            assert_eq!(
                bus.rom_literal(addr),
                Some(read_le(&rom, offset, Width::Word)),
                "{addr:#010x}"
            );
        }
        let bases = [
            0x0700_0000,
            0x0800_0000,
            0x0900_0000,
            0x0A00_0000,
            0x0C00_0000,
            0x0D00_0000,
            0x0E00_0000,
        ];
        let mut rng = Lcg(7);
        let mut seen = [0usize; 2];
        for _ in 0..4000 {
            let base: u32 = bases[rng.below(bases.len() as u32) as usize];
            let addr = base + (rng.below(0x2_0200) & !3);
            let baked = bus.rom_literal(addr);
            assert_eq!(baked.is_some(), stub(&bus, addr), "{addr:#010x}");
            if let Some(word) = baked {
                let offset = (addr & ROM_MIRROR_MASK) as usize;
                assert_eq!(word, bus.read_rom(offset, Width::Word), "{addr:#010x}");
            }
            seen[usize::from(baked.is_some())] += 1;
        }
        assert!(seen.iter().all(|&count| count > 0), "{seen:?}");
        bus.set_rom_patches(&[(0xCC, 0x5A)]);
        assert_eq!(
            bus.rom_literal(0x0800_00CC).map(|word| word & 0xFF),
            Some(0x5A)
        );
    }

    #[test]
    fn io_loads_stay_in_the_block_and_io_stores_leave() {
        assert!(!<Bus as JitBus>::leaves(0x0400_0006, false));
        assert!(!<Bus as JitBus>::leaves(0x0400_0130, false));
        assert!(<Bus as JitBus>::leaves(0x0400_0208, true));
        assert!(<Bus as JitBus>::leaves(0x0E00_0000, true));
        assert!(!<Bus as JitBus>::leaves(0x0E00_0000, false));
        assert!(!<Bus as JitBus>::leaves(0x0300_0000, true));
    }
}
