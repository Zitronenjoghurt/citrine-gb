use std::alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error};
use std::ops::Range;
use std::ptr::{self, NonNull};

pub const WINDOWS: usize = 3;
pub const INPUTS: usize = 4;
pub const PAGE_SHIFT: u32 = 12;
pub const PAGE_SIZE: u32 = 1 << PAGE_SHIFT;
pub const PAGE_COUNT: usize = 1 << (32 - PAGE_SHIFT);
pub const HALF_CYCLES: u32 = 0;
pub const WORD_CYCLES: u32 = 8;
pub const SEQUENTIAL_CYCLES: u32 = 16;
pub const LONG_HALF: u32 = 24;
pub const LONG_WORD: u32 = 25;
pub const LONG_AFTER: u32 = 6;
pub const MAIN: u32 = 26;
pub const CODE_CYCLES: u32 = 24;
pub const CODE_CLASSES: usize = 3;
pub const HALF_SEQUENTIAL: u32 = 56;
const _: () = assert!(HALF_SEQUENTIAL >= CODE_CYCLES + 8 * (CODE_CLASSES as u32 + 1));

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Window {
    pub bytes: *mut u8,
    pub dirty: *mut u8,
    pub code: *const bool,
    pub size: u64,
    pub base: u32,
}

impl Window {
    pub const CLOSED: Self = Self {
        bytes: ptr::null_mut(),
        dirty: ptr::null_mut(),
        code: ptr::null(),
        size: 0,
        base: 0,
    };

    pub fn holds(&self, addr: u32) -> bool {
        u64::from(addr.wrapping_sub(self.base)) < self.size
    }

    pub fn skip(self, offset: u32) -> Self {
        Self {
            bytes: self.bytes.wrapping_add(offset as usize),
            dirty: self.dirty.wrapping_add((offset >> PAGE_SHIFT) as usize),
            code: self
                .code
                .wrapping_add((offset >> super::marks::PAGE_BITS) as usize),
            ..self
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Fast {
    pub debt: *mut u32,
    pub cache: *const u8,
    pub windows: [Window; WINDOWS],
    pub pages: *const Page,
    pub rom: *const u8,
    pub bios: *const u8,
}

impl Fast {
    pub const CLOSED: Self = Self {
        debt: ptr::null_mut(),
        cache: ptr::null(),
        windows: [Window::CLOSED; WINDOWS],
        pages: ptr::null(),
        rom: ptr::null(),
        bios: ptr::null(),
    };
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub addend: u64,
    pub meta: u64,
    pub dirty: *mut u8,
    pub code: *const bool,
}

impl Page {
    pub const SLOW: Self = Self {
        addend: 0,
        meta: 0,
        dirty: ptr::null_mut(),
        code: ptr::null(),
    };

    pub fn meta(half: u32, word: u32, sequential: u32) -> u64 {
        let long = |cycles: u32, flag: u32| u64::from(cycles > LONG_AFTER) << flag;
        u64::from(half) << HALF_CYCLES
            | u64::from(word) << WORD_CYCLES
            | u64::from(sequential) << SEQUENTIAL_CYCLES
            | long(half, LONG_HALF)
            | long(word, LONG_WORD)
    }

    pub fn meta_v4(data: [u32; 4], main: bool, code: [u32; CODE_CLASSES]) -> u64 {
        let [half, word, sequential, half_sequential] = data;
        let code = code.iter().enumerate().fold(0, |meta, (index, &cycles)| {
            meta | u64::from(cycles) << (CODE_CYCLES + 8 * (index as u32 + 1))
        });
        u64::from(half) << HALF_CYCLES
            | u64::from(word) << WORD_CYCLES
            | u64::from(sequential) << SEQUENTIAL_CYCLES
            | u64::from(main) << MAIN
            | u64::from(half_sequential) << HALF_SEQUENTIAL
            | code
    }

    pub fn of(window: &Window, len: u32, addr: u32, meta: u64) -> Self {
        let offset = addr.wrapping_sub(window.base) & (len - 1);
        Self {
            addend: (window.bytes as u64)
                .wrapping_add(u64::from(offset))
                .wrapping_sub(u64::from(addr)),
            meta,
            dirty: window.dirty.wrapping_add((offset >> PAGE_SHIFT) as usize),
            code: window
                .code
                .wrapping_add((offset >> super::marks::PAGE_BITS) as usize),
        }
    }
}

pub struct Pages {
    table: NonNull<Page>,
    filled: Vec<Range<usize>>,
}

impl Pages {
    fn layout() -> Layout {
        Layout::array::<Page>(PAGE_COUNT).expect("page table layout")
    }

    fn new() -> Self {
        let layout = Self::layout();
        let table = unsafe { alloc_zeroed(layout) }.cast::<Page>();
        let Some(table) = NonNull::new(table) else {
            handle_alloc_error(layout);
        };
        Self {
            table,
            filled: Vec::new(),
        }
    }

    fn entries(&mut self) -> &mut [Page] {
        unsafe { std::slice::from_raw_parts_mut(self.table.as_ptr(), PAGE_COUNT) }
    }

    fn clear(&mut self) {
        let filled = std::mem::take(&mut self.filled);
        let entries = self.entries();
        for range in filled {
            entries[range].fill(Page::SLOW);
        }
    }

    pub fn set(&mut self, addr: u32, page: Page) {
        let index = (addr >> PAGE_SHIFT) as usize;
        self.entries()[index] = page;
        match self.filled.last_mut() {
            Some(range) if range.end == index => range.end += 1,
            _ => self.filled.push(index..index + 1),
        }
    }

    pub fn get(&self, addr: u32) -> Page {
        unsafe { *self.table.as_ptr().add((addr >> PAGE_SHIFT) as usize) }
    }

    pub fn table(&self) -> *const Page {
        self.table.as_ptr()
    }
}

impl Drop for Pages {
    fn drop(&mut self) {
        unsafe { dealloc(self.table.as_ptr().cast(), Self::layout()) };
    }
}

impl std::fmt::Debug for Pages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pages")
            .field("filled", &self.filled)
            .finish()
    }
}

#[derive(Debug, Default, Clone, Copy, Eq)]
pub struct Stamp {
    pub owner: usize,
    pub shared: usize,
    pub moves: u32,
    pub inputs: [usize; INPUTS],
}

impl PartialEq for Stamp {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        let inputs = self
            .inputs
            .iter()
            .zip(other.inputs)
            .fold(0, |differ, (&a, b)| differ | (a ^ b));
        (self.owner ^ other.owner)
            | (self.shared ^ other.shared)
            | (self.moves ^ other.moves) as usize
            | inputs
            == 0
    }
}

#[derive(Debug)]
pub struct Kept {
    table: Fast,
    stamp: Stamp,
    moves: u32,
    pages: Option<Pages>,
}

unsafe impl Send for Kept {}
unsafe impl Sync for Kept {}

impl Default for Kept {
    fn default() -> Self {
        Self {
            table: Fast::CLOSED,
            stamp: Stamp::default(),
            moves: 0,
            pages: None,
        }
    }
}

impl Clone for Kept {
    fn clone(&self) -> Self {
        Self::default()
    }

    fn clone_from(&mut self, _: &Self) {
        self.stamp = Stamp::default();
    }
}

impl Kept {
    pub fn moved(&mut self) {
        self.moves = self.moves.wrapping_add(1);
    }

    #[inline(always)]
    pub fn stamp<O, S>(&self, owner: &O, shared: &S, inputs: [usize; INPUTS]) -> Stamp {
        Stamp {
            owner: ptr::from_ref(owner).addr(),
            shared: ptr::from_ref(shared).addr(),
            moves: self.moves,
            inputs,
        }
    }

    #[inline(always)]
    pub fn get(&mut self, stamp: Stamp) -> Option<*mut Fast> {
        (self.stamp == stamp).then_some(&raw mut self.table)
    }

    pub fn pages(&mut self) -> &mut Pages {
        let pages = self.pages.get_or_insert_with(Pages::new);
        pages.clear();
        pages
    }

    pub fn keep(&mut self, stamp: Stamp, table: Fast) -> *mut Fast {
        self.table = table;
        self.stamp = stamp;
        &raw mut self.table
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cache {
    pub shift: u32,
    pub bit: u8,
    pub code: u8,
    pub line: u32,
    pub cycles: [u32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub len: u32,
    pub code: bool,
    pub cache: Option<Cache>,
    pub half: [u32; 2],
    pub word: [u32; 2],
}

pub trait FastBus {
    const SPECS: [Spec; WINDOWS];
    const PAGES: bool = false;

    fn fast(&mut self) -> *mut Fast;

    fn code_class(_start: u32, _end: u32, _thumb: bool) -> u32 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_holds_its_range_and_a_closed_one_holds_nothing() {
        let window = Window {
            base: 0x027C_0000,
            size: 0x4000,
            ..Window::CLOSED
        };
        assert!(window.holds(0x027C_0000));
        assert!(window.holds(0x027C_3FFF));
        assert!(!window.holds(0x027C_4000));
        assert!(!window.holds(0x027B_FFFF));
        assert!(!Window::CLOSED.holds(0));
        let all = Window {
            size: 1 << 32,
            ..Window::CLOSED
        };
        assert!(all.holds(u32::MAX));
    }

    #[test]
    fn a_kept_table_answers_only_its_own_stamp() {
        let (owner, shared) = (1u8, 2u8);
        let inputs = [3; INPUTS];
        let mut kept = Kept::default();
        let stamp = kept.stamp(&owner, &shared, inputs);
        assert!(kept.get(stamp).is_none());
        let table = Fast {
            debt: ptr::dangling_mut(),
            ..Fast::CLOSED
        };
        kept.keep(stamp, table);
        assert_eq!(
            kept.get(stamp).map(|kept| unsafe { (*kept).debt }),
            Some(table.debt)
        );
        assert!(kept.get(kept.stamp(&shared, &shared, inputs)).is_none());
        assert!(kept.get(kept.stamp(&owner, &owner, inputs)).is_none());
        assert!(kept.get(kept.stamp(&owner, &shared, [4; INPUTS])).is_none());
        for word in 0..INPUTS {
            let mut one = inputs;
            one[word] ^= 1 << word;
            assert!(kept.get(kept.stamp(&owner, &shared, one)).is_none());
        }
        let mut copy = Kept::default();
        copy.keep(stamp, table);
        copy.clone_from(&kept);
        assert!(copy.get(stamp).is_none());
        kept.moved();
        assert!(kept.get(kept.stamp(&owner, &shared, inputs)).is_none());
        assert!(kept.clone().get(stamp).is_none());
    }

    #[test]
    fn a_page_entry_adds_its_addend_and_marks_its_own_dirty_and_code_bytes() {
        let mut bytes = [0u8; 0x8000];
        let mut dirty = [0u8; 8];
        let code = [false; 32];
        let window = Window {
            bytes: bytes.as_mut_ptr(),
            dirty: dirty.as_mut_ptr(),
            code: code.as_ptr(),
            size: 0x0200_0000,
            base: 0,
        };
        let page = Page::of(&window, 0x8000, 0x0001_3000, Page::meta(1, 1, 1));
        let host = page.addend.wrapping_add(0x0001_3004) as *const u8;
        assert_eq!(host, bytes[0x3004..].as_ptr());
        assert_eq!(page.dirty, dirty[3..].as_mut_ptr());
        assert_eq!(page.code, code[12..].as_ptr());
    }

    #[test]
    fn meta_packs_each_width_and_flags_the_long_ones() {
        let meta = Page::meta(16, 18, 4);
        assert_eq!((meta >> HALF_CYCLES) & 0xFF, 16);
        assert_eq!((meta >> WORD_CYCLES) & 0xFF, 18);
        assert_eq!((meta >> SEQUENTIAL_CYCLES) & 0xFF, 4);
        assert_ne!(meta & 1 << LONG_HALF, 0);
        assert_ne!(meta & 1 << LONG_WORD, 0);
        let meta = Page::meta(3, 6, 1);
        assert_eq!(meta & (1 << LONG_HALF | 1 << LONG_WORD), 0);
    }

    #[test]
    fn v4_meta_keeps_the_sequential_half_apart_from_the_code_classes() {
        let meta = Page::meta_v4([8, 9, 2, 1], true, [1, 8, 9]);
        assert_eq!((meta >> HALF_CYCLES) & 0xFF, 8);
        assert_eq!((meta >> WORD_CYCLES) & 0xFF, 9);
        assert_eq!((meta >> SEQUENTIAL_CYCLES) & 0xFF, 2);
        assert_eq!(meta >> HALF_SEQUENTIAL, 1);
        assert_ne!(meta & 1 << MAIN, 0);
        for (class, cycles) in [1, 8, 9].into_iter().enumerate() {
            assert_eq!(
                (meta >> (CODE_CYCLES + 8 * (class as u32 + 1))) & 0xFF,
                cycles
            );
        }
    }

    #[test]
    fn a_new_fill_clears_the_old_entries_and_leaves_the_rest_slow() {
        let mut kept = Kept::default();
        let page = Page {
            meta: 1,
            ..Page::SLOW
        };
        let pages = kept.pages();
        for addr in [0x0200_0000, 0x0200_1000, 0x0300_0000] {
            pages.set(addr, page);
        }
        assert_eq!(pages.get(0x0200_1FFF), page);
        assert_eq!(pages.get(0x0200_2000), Page::SLOW);
        let pages = kept.pages();
        assert_eq!(pages.get(0x0200_1000), Page::SLOW);
        assert_eq!(pages.get(0x0300_0000), Page::SLOW);
        assert_eq!(pages.get(0xFFFF_F000), Page::SLOW);
    }
}
