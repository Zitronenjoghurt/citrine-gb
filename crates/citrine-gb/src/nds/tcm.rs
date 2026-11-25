use super::cp15::{Fetch, TcmRegion};
use super::ram::Ram;
use crate::gba::wait::Width;
#[cfg(jit)]
use crate::jit;

pub const ITCM_SIZE: usize = 0x8000;
pub const DTCM_SIZE: usize = 0x4000;

const MAIN_RAM_FIRST: u64 = 0x0200_0000;
const MAIN_RAM_END: u64 = 0x0300_0000;

#[derive(Debug, Default, Clone, Copy)]
struct Window {
    base: u32,
    reads: u64,
    writes: u64,
}

impl Window {
    fn of(region: TcmRegion) -> Self {
        let size = if region.enabled {
            region.virtual_size.min((1 << 32) - u64::from(region.base))
        } else {
            0
        };
        Self {
            base: region.base,
            reads: if region.load_mode { 0 } else { size },
            writes: size,
        }
    }

    #[inline(always)]
    fn readable(self, addr: u32) -> bool {
        u64::from(addr.wrapping_sub(self.base)) < self.reads
    }

    #[inline(always)]
    fn covers(self, addr: u32) -> bool {
        u64::from(addr.wrapping_sub(self.base)) < self.writes
    }

    #[inline(always)]
    fn touches(self, first: u32, last: u32) -> bool {
        self.writes != 0 && (self.covers(first) || self.base.wrapping_sub(first) <= last - first)
    }

    fn apart(self, first: u64, end: u64) -> bool {
        self.writes == 0
            || u64::from(self.base) + self.writes <= first
            || u64::from(self.base) >= end
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Clear {
    dtcm: bool,
    main_ram: bool,
}

impl Clear {
    fn of(itcm: Window, dtcm: Window) -> Self {
        let dtcm_first = u64::from(dtcm.base);
        Self {
            dtcm: dtcm.reads == 0 || itcm.apart(dtcm_first, dtcm_first + dtcm.reads),
            main_ram: itcm.apart(MAIN_RAM_FIRST, MAIN_RAM_END),
        }
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "TcmState"))]
pub struct Tcm {
    itcm: Ram,
    dtcm: Ram,
    itcm_region: TcmRegion,
    dtcm_region: TcmRegion,
    #[cfg_attr(feature = "serde", serde(skip))]
    itcm_window: Window,
    #[cfg_attr(feature = "serde", serde(skip))]
    dtcm_window: Window,
    #[cfg_attr(feature = "serde", serde(skip))]
    clear: Clear,
}

#[cfg(feature = "serde")]
#[derive(serde::Deserialize)]
struct TcmState {
    itcm: Ram,
    dtcm: Ram,
    itcm_region: TcmRegion,
    dtcm_region: TcmRegion,
}

#[cfg(feature = "serde")]
impl From<TcmState> for Tcm {
    fn from(state: TcmState) -> Self {
        let (itcm_window, dtcm_window) =
            (Window::of(state.itcm_region), Window::of(state.dtcm_region));
        Self {
            itcm: state.itcm,
            dtcm: state.dtcm,
            itcm_region: state.itcm_region,
            dtcm_region: state.dtcm_region,
            itcm_window,
            dtcm_window,
            clear: Clear::of(itcm_window, dtcm_window),
        }
    }
}

impl Clone for Tcm {
    fn clone(&self) -> Self {
        Self {
            itcm: self.itcm.clone(),
            dtcm: self.dtcm.clone(),
            itcm_region: self.itcm_region,
            dtcm_region: self.dtcm_region,
            itcm_window: self.itcm_window,
            dtcm_window: self.dtcm_window,
            clear: self.clear,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        let Self {
            itcm,
            dtcm,
            itcm_region,
            dtcm_region,
            itcm_window,
            dtcm_window,
            clear,
        } = self;
        itcm.clone_from(&source.itcm);
        dtcm.clone_from(&source.dtcm);
        *itcm_region = source.itcm_region;
        *dtcm_region = source.dtcm_region;
        *itcm_window = source.itcm_window;
        *dtcm_window = source.dtcm_window;
        *clear = source.clear;
    }
}

impl Default for Tcm {
    fn default() -> Self {
        Self {
            itcm: Ram::new(ITCM_SIZE),
            dtcm: Ram::new(DTCM_SIZE),
            itcm_region: TcmRegion::default(),
            dtcm_region: TcmRegion::default(),
            itcm_window: Window::default(),
            dtcm_window: Window::default(),
            clear: Clear::of(Window::default(), Window::default()),
        }
    }
}

impl Tcm {
    pub fn place_itcm(&mut self, region: TcmRegion) {
        self.itcm_region = TcmRegion { base: 0, ..region };
        self.itcm_window = Window::of(self.itcm_region);
        self.clear = Clear::of(self.itcm_window, self.dtcm_window);
    }

    pub fn place_dtcm(&mut self, region: TcmRegion) {
        self.dtcm_region = region;
        self.dtcm_window = Window::of(region);
        self.clear = Clear::of(self.itcm_window, self.dtcm_window);
    }

    pub fn rams_mut(&mut self) -> [&mut Ram; 2] {
        [&mut self.itcm, &mut self.dtcm]
    }

    pub fn itcm_region(&self) -> TcmRegion {
        self.itcm_region
    }

    pub fn dtcm_region(&self) -> TcmRegion {
        self.dtcm_region
    }

    pub fn itcm(&self) -> &[u8] {
        self.itcm.bytes()
    }

    pub fn dtcm(&self) -> &[u8] {
        self.dtcm.bytes()
    }

    #[cfg(jit)]
    pub(crate) fn itcm_ram(&self) -> &Ram {
        &self.itcm
    }

    #[cfg(jit)]
    pub(crate) fn itcm_ram_mut(&mut self) -> &mut Ram {
        &mut self.itcm
    }

    #[cfg(jit)]
    pub(crate) fn windows(&mut self) -> Option<[jit::Window; 2]> {
        let (itcm, dtcm) = (self.itcm_window, self.dtcm_window);
        if itcm.reads != itcm.writes || dtcm.reads != dtcm.writes {
            return None;
        }
        let window = |ram: &mut Ram, window: Window, hardware: usize| {
            if window.writes == 0 {
                jit::Window::CLOSED
            } else {
                ram.window(window.base, window.writes, hardware)
            }
        };
        Some([
            window(&mut self.itcm, itcm, ITCM_SIZE),
            window(&mut self.dtcm, dtcm, DTCM_SIZE),
        ])
    }

    pub fn hits(&self, addr: u32, fetch: Fetch) -> bool {
        self.itcm_window.readable(addr) || fetch == Fetch::Data && self.dtcm_window.readable(addr)
    }

    #[inline(always)]
    pub fn itcm_read(&self, addr: u32, width: Width) -> Option<u32> {
        self.itcm_window
            .readable(addr)
            .then(|| self.itcm.read_sized::<ITCM_SIZE>(addr as usize, width))
    }

    #[inline(always)]
    pub fn near_read(&self, addr: u32, width: Width) -> Option<u32> {
        if self.itcm_window.readable(addr) {
            return self.itcm.get_sized::<ITCM_SIZE>(addr as usize, width);
        }
        if self.dtcm_window.readable(addr) {
            let offset = (addr - self.dtcm_window.base) as usize;
            return self.dtcm.get_sized::<DTCM_SIZE>(offset, width);
        }
        None
    }

    pub fn read(&self, addr: u32, width: Width, fetch: Fetch) -> Option<u32> {
        if self.itcm_window.readable(addr) {
            return Some(self.itcm.read_sized::<ITCM_SIZE>(addr as usize, width));
        }
        if fetch == Fetch::Data && self.dtcm_window.readable(addr) {
            let offset = (addr - self.dtcm_window.base) as usize;
            return Some(self.dtcm.read_sized::<DTCM_SIZE>(offset, width));
        }
        None
    }

    #[inline(always)]
    pub fn misses(&self, first: u32, last: u32) -> bool {
        !self.itcm_window.touches(first, last) && !self.dtcm_window.touches(first, last)
    }

    #[inline(always)]
    pub fn misses_in_main_ram(&self, first: u32, last: u32) -> bool {
        (self.clear.main_ram || !self.itcm_window.touches(first, last))
            && !self.dtcm_window.touches(first, last)
    }

    #[inline(always)]
    pub fn dtcm_run(&self, first: u32, last: u32) -> Option<usize> {
        let window = self.dtcm_window;
        (window.readable(first)
            && window.readable(last)
            && (self.clear.dtcm || !self.itcm_window.touches(first, last)))
        .then(|| (first - window.base) as usize)
    }

    #[inline(always)]
    pub fn read_dtcm_words(&self, first: u32, last: u32, values: &mut [u32]) -> bool {
        self.dtcm_run(first, last)
            .is_some_and(|offset| self.dtcm.read_words(offset, values))
    }

    #[inline(always)]
    pub fn write_dtcm_words(&mut self, first: u32, last: u32, values: &[u32]) -> bool {
        self.dtcm_run(first, last)
            .is_some_and(|offset| self.dtcm.write_words(offset, values))
    }

    #[inline(always)]
    pub fn dtcm_words(&self, offset: usize, count: usize) -> Option<&[[u8; 4]]> {
        self.dtcm.words(offset, count)
    }

    #[inline(always)]
    pub fn dtcm_words_mut(&mut self, offset: usize, count: usize) -> Option<&mut [[u8; 4]]> {
        self.dtcm.words_mut(offset, count)
    }

    #[inline(always)]
    pub fn write(&mut self, addr: u32, width: Width, value: u32) -> bool {
        if self.itcm_window.covers(addr) {
            self.itcm.write(addr as usize, width, value);
            return true;
        }
        if self.dtcm_window.covers(addr) {
            let offset = (addr - self.dtcm_window.base) as usize;
            self.dtcm.write(offset, width, value);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DTCM_BASE: u32 = 0x027C_0000;

    fn region(base: u32, virtual_size: u64) -> TcmRegion {
        TcmRegion {
            base,
            virtual_size,
            enabled: true,
            load_mode: false,
        }
    }

    fn placed() -> Tcm {
        let mut tcm = Tcm::default();
        tcm.place_itcm(region(0, 0x200_0000));
        tcm.place_dtcm(region(DTCM_BASE, 0x4000));
        tcm
    }

    #[test]
    fn disabled_tcm_covers_nothing() {
        let mut tcm = Tcm::default();
        assert_eq!(tcm.read(0, Width::Word, Fetch::Data), None);
        assert!(!tcm.write(0, Width::Word, 1));
    }

    #[test]
    fn itcm_sits_at_zero_whatever_base_is_asked_for() {
        let mut tcm = Tcm::default();
        tcm.place_itcm(region(0x0100_0000, 0x8000));
        assert_eq!(tcm.itcm_region().base, 0);
        assert!(tcm.write(0x10, Width::Word, 7));
        assert_eq!(tcm.read(0x10, Width::Word, Fetch::Code), Some(7));
    }

    #[test]
    fn itcm_mirrors_its_32k_across_a_larger_virtual_size() {
        let mut tcm = placed();
        tcm.write(0x0000_0040, Width::Word, 0x1234_5678);
        assert_eq!(
            tcm.read(0x01FF_8040, Width::Word, Fetch::Code),
            Some(0x1234_5678)
        );
        assert_eq!(tcm.read(0x0200_0040, Width::Word, Fetch::Data), None);
    }

    #[test]
    fn a_smaller_virtual_size_uses_only_the_first_part() {
        let mut tcm = Tcm::default();
        tcm.place_itcm(region(0, 0x1000));
        assert!(tcm.write(0x0FFC, Width::Word, 1));
        assert!(!tcm.write(0x1000, Width::Word, 1));
    }

    #[test]
    fn dtcm_sits_at_its_base_and_mirrors_16k() {
        let mut tcm = placed();
        tcm.place_dtcm(region(DTCM_BASE, 0x8000));
        tcm.write(DTCM_BASE + 0x4010, Width::Half, 0xBEEF);
        assert_eq!(
            tcm.read(DTCM_BASE + 0x10, Width::Half, Fetch::Data),
            Some(0xBEEF)
        );
        assert_eq!(tcm.read(DTCM_BASE - 2, Width::Half, Fetch::Data), None);
    }

    #[test]
    fn code_fetches_skip_dtcm() {
        let mut tcm = placed();
        tcm.write(DTCM_BASE, Width::Word, 1);
        assert_eq!(tcm.read(DTCM_BASE, Width::Word, Fetch::Code), None);
    }

    #[test]
    fn itcm_wins_where_the_two_overlap() {
        let mut tcm = Tcm::default();
        tcm.place_itcm(region(0, 0x8000));
        tcm.place_dtcm(region(0, 0x4000));
        tcm.write(0x20, Width::Word, 9);
        assert_eq!(tcm.read(0x20, Width::Word, Fetch::Data), Some(9));
        tcm.place_itcm(TcmRegion::default());
        assert_eq!(tcm.read(0x20, Width::Word, Fetch::Data), Some(0));
    }

    #[test]
    fn a_dtcm_run_that_reaches_the_itcm_leaves_it_to_the_bus() {
        let mut tcm = Tcm::default();
        tcm.place_itcm(region(0, 0x8000));
        tcm.place_dtcm(region(0x7000, 0x4000));
        let mut words = [0; 4];
        assert!(!tcm.read_dtcm_words(0x7FF0, 0x7FFC, &mut words));
        assert!(!tcm.write_dtcm_words(0x7FF8, 0x8004, &words));
        assert!(tcm.read_dtcm_words(0x8000, 0x800C, &mut words));
        tcm.place_itcm(TcmRegion::default());
        assert!(tcm.write_dtcm_words(0x7FF8, 0x8004, &words));
        assert!(placed().read_dtcm_words(DTCM_BASE, DTCM_BASE + 12, &mut words));
        let runs = [
            (0x7FF0, 0x7FFC),
            (0x7FF8, 0x8004),
            (0x8000, 0x800C),
            (DTCM_BASE, DTCM_BASE + 12),
            (DTCM_BASE + 0x3FF8, DTCM_BASE + 0x4004),
        ];
        let placements = [
            (region(0, 0x8000), region(0x7000, 0x4000)),
            (
                TcmRegion {
                    load_mode: true,
                    ..region(0, 0x8000)
                },
                region(0x7000, 0x4000),
            ),
            (region(0, 0x200_0000), region(DTCM_BASE, 0x4000)),
            (region(0, 0x1000_0000), region(DTCM_BASE, 0x4000)),
            (TcmRegion::default(), region(0x7000, 0x4000)),
        ];
        for (itcm, dtcm) in placements {
            let mut tcm = Tcm::default();
            tcm.place_itcm(itcm);
            tcm.place_dtcm(dtcm);
            for (first, last) in runs {
                let window = tcm.dtcm_window;
                let parent = window.readable(first)
                    && window.readable(last)
                    && !tcm.itcm_window.touches(first, last);
                assert_eq!(tcm.dtcm_run(first, last).is_some(), parent, "{first:08X}");
            }
        }
    }

    #[test]
    fn main_ram_runs_miss_only_where_neither_tcm_reaches() {
        let mut tcm = placed();
        let runs = [
            (0x0200_0000, 0x0200_000C),
            (DTCM_BASE - 8, DTCM_BASE + 4),
            (DTCM_BASE + 0x3FF8, DTCM_BASE + 0x4004),
            (0x02FF_FFF0, 0x02FF_FFFC),
        ];
        let check = |tcm: &Tcm| {
            for (first, last) in runs {
                assert_eq!(
                    tcm.misses_in_main_ram(first, last),
                    tcm.misses(first, last),
                    "{first:08X}"
                );
            }
        };
        check(&tcm);
        assert!(tcm.misses_in_main_ram(0x0200_0000, 0x0200_000C));
        assert!(!tcm.misses_in_main_ram(DTCM_BASE - 8, DTCM_BASE + 4));
        tcm.place_itcm(region(0, 0x400_0000));
        check(&tcm);
        assert!(!tcm.misses_in_main_ram(0x0200_0000, 0x0200_000C));
        tcm.place_itcm(region(0, 0x200_0008));
        check(&tcm);
        assert!(!tcm.misses_in_main_ram(0x0200_0000, 0x0200_000C));
        assert!(tcm.misses_in_main_ram(0x0200_0008, 0x0200_000C));
    }

    #[test]
    fn load_mode_takes_writes_but_leaves_reads_to_the_bus() {
        let mut tcm = Tcm::default();
        tcm.place_dtcm(TcmRegion {
            load_mode: true,
            ..region(DTCM_BASE, 0x4000)
        });
        assert!(tcm.write(DTCM_BASE, Width::Word, 5));
        assert_eq!(tcm.read(DTCM_BASE, Width::Word, Fetch::Data), None);
        tcm.place_dtcm(region(DTCM_BASE, 0x4000));
        assert_eq!(tcm.read(DTCM_BASE, Width::Word, Fetch::Data), Some(5));
    }

    #[test]
    fn a_region_near_the_top_does_not_wrap_to_zero() {
        let mut tcm = Tcm::default();
        tcm.place_dtcm(region(0xFFFF_0000, 1 << 32));
        assert!(tcm.write(0xFFFF_FFFC, Width::Word, 3));
        assert!(!tcm.write(0x10, Width::Word, 3));
        assert_eq!(tcm.read(0x10, Width::Word, Fetch::Data), None);
    }

    #[cfg(feature = "debug")]
    #[test]
    fn a_loaded_tcm_reads_where_it_was_placed() {
        let mut tcm = placed();
        tcm.write(DTCM_BASE + 8, Width::Word, 0xCAFE);
        let json = serde_json::to_string(&tcm).unwrap();
        let loaded: Tcm = serde_json::from_str(&json).unwrap();
        assert_eq!(
            loaded.read(DTCM_BASE + 8, Width::Word, Fetch::Data),
            Some(0xCAFE)
        );
        assert_eq!(loaded.read(0x01FF_8000, Width::Word, Fetch::Code), Some(0));
    }

    #[test]
    fn a_4g_virtual_size_covers_the_top_address() {
        let mut tcm = Tcm::default();
        tcm.place_dtcm(region(0, 1 << 32));
        assert!(tcm.write(0xFFFF_FFFC, Width::Word, 3));
    }
}
