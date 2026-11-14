use super::{CacheInputs, Cp15, DCACHE_ON, Fetch, ICACHE_ON, PROTECTION_ON};

const PAGE_BITS: u32 = 12;
const LOW_PAGES: usize = 0x1_0000;
const HIGH_FIRST: u32 = 0xF_FFF0;
const HIGH_PAGES: usize = 0x10;
const ALL_PAGES: u64 = 1 << 20;
const CODE: u8 = 1;
const DATA: u8 = 2;

#[cfg(jit)]
static GENERATIONS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

#[derive(Debug, Clone)]
pub struct CacheMap {
    low: Box<[u8]>,
    high: [u8; HIGH_PAGES],
    inputs: Option<CacheInputs>,
    #[cfg(jit)]
    generation: u32,
}

impl Default for CacheMap {
    fn default() -> Self {
        Self {
            low: vec![0; LOW_PAGES].into_boxed_slice(),
            high: [0; HIGH_PAGES],
            inputs: None,
            #[cfg(jit)]
            generation: 0,
        }
    }
}

impl CacheMap {
    #[inline(always)]
    pub fn cached(&self, addr: u32, fetch: Fetch) -> bool {
        let side = match fetch {
            Fetch::Code => CODE,
            Fetch::Data => DATA,
        };
        let page = addr >> PAGE_BITS;
        let flags = match self.low.get(page as usize) {
            Some(&flags) => flags,
            None if page >= HIGH_FIRST => self.high[(page - HIGH_FIRST) as usize],
            None => 0,
        };
        flags & side != 0
    }

    pub fn refresh(&mut self, cp15: &Cp15) {
        let inputs = cp15.cache_inputs();
        if self.inputs.as_ref() == Some(&inputs) {
            return;
        }
        self.low.fill(0);
        self.high.fill(0);
        if inputs.control & PROTECTION_ON != 0 {
            let on = [ICACHE_ON, DCACHE_ON].map(|bit| inputs.control & bit != 0);
            for (first, pages, sides) in inputs.protection.cached_pages() {
                let flags =
                    (u8::from(on[0] && sides[0]) * CODE) | (u8::from(on[1] && sides[1]) * DATA);
                self.fill(u64::from(first), u64::from(first) + u64::from(pages), flags);
            }
        }
        self.inputs = Some(inputs);
        #[cfg(jit)]
        {
            self.generation = GENERATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[cfg(jit)]
    pub const PAGE_BITS: u32 = PAGE_BITS;

    #[cfg(jit)]
    pub const DATA: u8 = DATA;

    #[cfg(jit)]
    pub const CODE: u8 = CODE;

    #[cfg(jit)]
    pub fn pages(&self) -> *const u8 {
        self.low.as_ptr()
    }

    #[cfg(jit)]
    pub fn reaches(addr: u32) -> bool {
        ((addr >> PAGE_BITS) as usize) < LOW_PAGES
    }

    #[cfg(jit)]
    pub fn filled(&self) -> bool {
        self.inputs.is_some()
    }

    #[cfg(jit)]
    pub fn generation(&self) -> u32 {
        self.generation
    }

    fn fill(&mut self, start: u64, end: u64, flags: u8) {
        let end = end.min(ALL_PAGES);
        let low = start.min(LOW_PAGES as u64) as usize..end.min(LOW_PAGES as u64) as usize;
        self.low[low].fill(flags);
        let high = start.max(u64::from(HIGH_FIRST))..end;
        if !high.is_empty() {
            let first = (high.start - u64::from(HIGH_FIRST)) as usize;
            let last = (high.end - u64::from(HIGH_FIRST)) as usize;
            self.high[first..last].fill(flags);
        }
    }
}
