pub const PAGE_BITS: usize = 10;
pub const UNMARKED_PAGES: usize = 1 << 12;
pub const RECENT: usize = 32;
static UNMARKED: [bool; UNMARKED_PAGES] = [false; UNMARKED_PAGES];

#[derive(Debug, Clone, Default)]
pub struct CodeMarks {
    flags: Vec<bool>,
    gens: Vec<u32>,
    hit: bool,
    stamp: u32,
    recent: [u32; RECENT],
}

impl CodeMarks {
    pub const HIT: usize = std::mem::offset_of!(CodeMarks, hit);

    #[inline(always)]
    pub fn written(&mut self, offset: usize) {
        let page = offset >> PAGE_BITS;
        if let Some(flag) = self.flags.get_mut(page)
            && *flag
        {
            *flag = false;
            self.gens[page] = self.gens[page].wrapping_add(1);
            self.hit = true;
            self.recent[self.stamp as usize % RECENT] = page as u32;
            self.stamp = self.stamp.wrapping_add(1);
        }
    }

    pub fn mark(&mut self, page: usize, len: usize) -> u32 {
        if self.flags.is_empty() {
            let pages = len.div_ceil(1 << PAGE_BITS);
            self.flags = vec![false; pages];
            self.gens = vec![0; pages];
        }
        self.flags[page] = true;
        self.gens[page]
    }

    pub fn generation(&self, page: usize) -> Option<u32> {
        self.gens.get(page).copied()
    }

    pub fn pages(&self) -> *const bool {
        if self.flags.is_empty() {
            UNMARKED.as_ptr()
        } else {
            self.flags.as_ptr()
        }
    }

    pub fn take_hit(&mut self) -> bool {
        std::mem::take(&mut self.hit)
    }

    pub fn stamp(&self) -> u32 {
        self.stamp
    }

    #[inline(always)]
    pub fn quiet(&self, seen: u32) -> bool {
        !self.hit & (self.stamp == seen)
    }

    pub fn written_page(&self, stamp: u32) -> Option<usize> {
        let back = self.stamp.wrapping_sub(stamp) as usize;
        (1..=RECENT)
            .contains(&back)
            .then(|| self.recent[stamp as usize % RECENT] as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEN: usize = 4 << PAGE_BITS;

    #[test]
    fn a_write_to_a_marked_page_moves_its_generation_once() {
        let mut marks = CodeMarks::default();
        let first = marks.mark(1, LEN);
        marks.written(1 << PAGE_BITS);
        marks.written((1 << PAGE_BITS) + 4);
        assert_eq!(marks.generation(1), Some(first + 1));
        assert!(marks.take_hit());
        assert!(!marks.take_hit());
    }

    #[test]
    fn each_unmarking_write_is_logged_once_by_stamp() {
        let mut marks = CodeMarks::default();
        marks.mark(1, LEN);
        marks.mark(3, LEN);
        let seen = marks.stamp();
        marks.written(3 << PAGE_BITS);
        marks.written(1 << PAGE_BITS);
        marks.written((1 << PAGE_BITS) + 8);
        assert_eq!(marks.stamp(), seen.wrapping_add(2));
        assert_eq!(marks.written_page(seen), Some(3));
        assert_eq!(marks.written_page(seen.wrapping_add(1)), Some(1));
        assert_eq!(marks.written_page(seen.wrapping_add(2)), None);
    }

    #[test]
    fn a_log_older_than_the_ring_is_lost() {
        let mut marks = CodeMarks::default();
        let seen = marks.stamp();
        for _ in 0..=RECENT {
            marks.mark(2, LEN);
            marks.written(2 << PAGE_BITS);
        }
        assert_eq!(marks.written_page(seen), None);
        assert_eq!(marks.written_page(seen.wrapping_add(1)), Some(2));
    }

    #[test]
    fn writes_to_unmarked_pages_change_nothing() {
        let mut marks = CodeMarks::default();
        marks.written(0);
        assert_eq!(marks.generation(0), None);
        marks.mark(1, LEN);
        marks.written(2 << PAGE_BITS);
        assert_eq!(marks.generation(2), Some(0));
        assert!(!marks.take_hit());
    }
}
