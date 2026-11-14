use std::ops::Deref;
use std::sync::Arc;

const PAGE_SIZE: usize = 0x1000;
pub(crate) const PAGE_SHIFT: usize = 12;

type Page = [u8; PAGE_SIZE];

pub(crate) struct Paged {
    bytes: Box<[u8]>,
    pages: Box<[Arc<Page>]>,
    dirty: Box<[u8]>,
    len: usize,
}

fn dirty_pages(len: usize) -> usize {
    len.div_ceil(PAGE_SIZE)
}

impl Paged {
    pub(crate) fn zeroed(size: usize) -> Self {
        Self::from_bytes(vec![0; size].into_boxed_slice())
    }

    pub(crate) fn from_bytes(bytes: Box<[u8]>) -> Self {
        Self {
            dirty: vec![0; dirty_pages(bytes.len())].into_boxed_slice(),
            len: bytes.len(),
            bytes,
            pages: Box::default(),
        }
    }

    pub(crate) fn touch(&mut self, offset: usize) -> &mut [u8] {
        let page = offset >> PAGE_SHIFT;
        self.dirty[page] = 1;
        &mut self.bytes
    }

    #[cfg(jit)]
    pub(crate) fn raw(&mut self) -> (*mut u8, usize, *mut u8) {
        (
            self.bytes.as_mut_ptr(),
            self.bytes.len(),
            self.dirty.as_mut_ptr(),
        )
    }

    pub(crate) fn is_snapshot(&self) -> bool {
        self.bytes.is_empty() && !self.pages.is_empty()
    }

    pub(crate) fn copy_from(&mut self, source: &Paged) {
        self.len = source.len;
        match (self.is_snapshot(), source.is_snapshot()) {
            (false, false) => {
                if self.bytes.len() == source.bytes.len() {
                    self.bytes.copy_from_slice(&source.bytes);
                    self.dirty.copy_from_slice(&source.dirty);
                } else {
                    self.bytes.clone_from(&source.bytes);
                    self.dirty.clone_from(&source.dirty);
                }
                self.pages.clone_from(&source.pages);
            }
            (false, true) => self.load(source),
            (true, false) => self.share(source),
            (true, true) => self.pages.clone_from(&source.pages),
        }
    }

    pub(crate) fn snapshot_from(&mut self, live: &mut Paged) {
        self.len = live.len;
        if live.is_snapshot() {
            self.pages.clone_from(&live.pages);
            return;
        }
        self.share(live);
        live.pages.clone_from(&self.pages);
        live.dirty.fill(0);
    }

    fn page_count(&self) -> usize {
        self.bytes.len().div_ceil(PAGE_SIZE)
    }

    fn changed(&self, page: usize) -> bool {
        self.pages.len() <= page || self.dirty[page] != 0
    }

    fn page_of(&self, page: usize) -> Arc<Page> {
        if !self.changed(page) {
            return self.pages[page].clone();
        }
        let mut copy = [0; PAGE_SIZE];
        let bytes = &self.bytes[page * PAGE_SIZE..self.bytes.len().min((page + 1) * PAGE_SIZE)];
        copy[..bytes.len()].copy_from_slice(bytes);
        Arc::new(copy)
    }

    fn share(&mut self, live: &Paged) {
        let count = live.page_count();
        self.bytes = Box::default();
        self.dirty = Box::default();
        if self.pages.len() != count {
            self.pages = (0..count).map(|page| live.page_of(page)).collect();
            return;
        }
        for (page, (slot, bytes)) in self
            .pages
            .iter_mut()
            .zip(live.bytes.chunks(PAGE_SIZE))
            .enumerate()
        {
            if !live.changed(page) {
                if !Arc::ptr_eq(slot, &live.pages[page]) {
                    *slot = live.pages[page].clone();
                }
                continue;
            }
            match Arc::get_mut(slot) {
                Some(owned) => {
                    owned[..bytes.len()].copy_from_slice(bytes);
                    owned[bytes.len()..].fill(0);
                }
                None => *slot = live.page_of(page),
            }
        }
    }

    fn load(&mut self, snapshot: &Paged) {
        let size = snapshot.len;
        if self.bytes.len() != size {
            self.bytes = vec![0; size].into_boxed_slice();
            self.dirty = vec![0; dirty_pages(size)].into_boxed_slice();
            self.pages = Box::default();
        }
        for (page, from) in snapshot.pages.iter().enumerate() {
            if self.changed(page) || !Arc::ptr_eq(&self.pages[page], from) {
                let start = page * PAGE_SIZE;
                let end = size.min(start + PAGE_SIZE);
                self.bytes[start..end].copy_from_slice(&from[..end - start]);
            }
        }
        self.pages.clone_from(&snapshot.pages);
        self.dirty.fill(0);
    }

    fn join_into(&self, out: &mut Vec<u8>) {
        out.clear();
        out.reserve(self.len);
        for page in &self.pages {
            let left = self.len - out.len();
            out.extend_from_slice(&page[..left.min(PAGE_SIZE)]);
        }
    }

    fn assembled(&self) -> std::borrow::Cow<'_, [u8]> {
        if self.is_snapshot() {
            let mut bytes = Vec::with_capacity(self.len);
            self.join_into(&mut bytes);
            std::borrow::Cow::Owned(bytes)
        } else {
            std::borrow::Cow::Borrowed(&self.bytes)
        }
    }
}

impl Clone for Paged {
    fn clone(&self) -> Self {
        let bytes = self.assembled().into_owned().into_boxed_slice();
        Self {
            dirty: if self.is_snapshot() {
                vec![0; dirty_pages(bytes.len())].into_boxed_slice()
            } else {
                self.dirty.clone()
            },
            bytes,
            pages: self.pages.clone(),
            len: self.len,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.copy_from(source);
    }
}

impl Deref for Paged {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Paged {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if !self.is_snapshot() {
            return crate::utils::bytes::serialize(&self.bytes, serializer);
        }
        thread_local! {
            static JOINED: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
        }
        JOINED.with_borrow_mut(|joined| {
            self.join_into(joined);
            crate::utils::bytes::serialize(joined, serializer)
        })
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Paged {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        crate::utils::bytes::deserialize(deserializer).map(Self::from_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{PAGE_SIZE, Paged};
    use std::sync::Arc;

    fn written(size: usize, writes: &[(usize, u8)]) -> Paged {
        let mut memory = Paged::zeroed(size);
        write(&mut memory, writes);
        memory
    }

    fn write(memory: &mut Paged, writes: &[(usize, u8)]) {
        for &(offset, value) in writes {
            memory.touch(offset)[offset] = value;
        }
    }

    fn snapshot(live: &mut Paged) -> Paged {
        let mut slot = Paged::zeroed(0);
        slot.snapshot_from(live);
        slot
    }

    fn bytes_of(memory: &Paged) -> Vec<u8> {
        memory.assembled().into_owned()
    }

    #[test]
    fn a_snapshot_holds_the_bytes_it_was_taken_from() {
        let mut live = written(4 * PAGE_SIZE, &[(5, 1), (PAGE_SIZE * 3 + 7, 2)]);
        let slot = snapshot(&mut live);
        assert!(slot.is_snapshot());
        assert_eq!(bytes_of(&slot), bytes_of(&live));
    }

    #[test]
    fn a_snapshot_shares_the_pages_nothing_wrote_since_the_last_one() {
        let mut live = written(4 * PAGE_SIZE, &[(5, 1)]);
        let first = snapshot(&mut live);
        write(&mut live, &[(PAGE_SIZE * 2, 9)]);
        let second = snapshot(&mut live);
        let shared = |page: usize| Arc::ptr_eq(&first.pages[page], &second.pages[page]);
        assert_eq!(
            [shared(0), shared(1), shared(2), shared(3)],
            [true, true, false, true]
        );
        assert_eq!(second.pages[2][0], 9);
        assert_eq!(first.pages[2][0], 0);
    }

    #[test]
    fn loading_an_older_snapshot_undoes_every_write_since() {
        let mut live = written(4 * PAGE_SIZE, &[(5, 1)]);
        let old = snapshot(&mut live);
        let expected = bytes_of(&live);
        write(&mut live, &[(PAGE_SIZE + 1, 3)]);
        let _newer = snapshot(&mut live);
        write(&mut live, &[(5, 4), (PAGE_SIZE * 3, 5)]);
        live.copy_from(&old);
        assert_eq!(bytes_of(&live), expected);
        write(&mut live, &[(PAGE_SIZE * 2, 6)]);
        let again = snapshot(&mut live);
        live.copy_from(&old);
        assert_eq!(bytes_of(&live), expected);
        assert_eq!(again.pages[2][0], 6);
    }

    #[test]
    fn copying_live_into_a_snapshot_leaves_the_live_memory_untouched() {
        let mut live = written(2 * PAGE_SIZE, &[(1, 1)]);
        let mut slot = snapshot(&mut live);
        write(&mut live, &[(PAGE_SIZE, 2)]);
        slot.copy_from(&live);
        assert!(slot.is_snapshot());
        assert_eq!(bytes_of(&slot), bytes_of(&live));
        let next = snapshot(&mut live);
        assert_eq!(bytes_of(&next), bytes_of(&live));
    }

    #[test]
    fn a_memory_smaller_than_a_page_keeps_its_length_through_a_snapshot() {
        let mut live = written(0x800, &[(3, 1), (0x7FF, 2)]);
        let slot = snapshot(&mut live);
        assert_eq!(bytes_of(&slot), bytes_of(&live));
        let mut other = written(0x800, &[(4, 5)]);
        other.copy_from(&slot);
        assert_eq!(&*other, &*live);
        assert_eq!(&*slot.clone(), &*live);
    }

    #[test]
    fn a_clone_of_a_snapshot_is_live_memory_again() {
        let mut live = written(2 * PAGE_SIZE, &[(1, 1), (PAGE_SIZE, 2)]);
        let slot = snapshot(&mut live);
        let mut clone = slot.clone();
        assert!(!clone.is_snapshot());
        assert_eq!(&*clone, &*live);
        write(&mut clone, &[(2, 3)]);
        assert_eq!(clone[2], 3);
        assert_eq!(bytes_of(&snapshot(&mut clone)), bytes_of(&clone));
    }

    #[test]
    fn a_snapshot_of_a_large_memory_shares_every_page_nothing_wrote() {
        let pages = 1024;
        let mut live = written(pages * PAGE_SIZE, &[(5, 1)]);
        let first = snapshot(&mut live);
        write(&mut live, &[(PAGE_SIZE * 64, 7), (PAGE_SIZE * 1000 + 3, 8)]);
        let second = snapshot(&mut live);
        let unshared: Vec<usize> = (0..pages)
            .filter(|&page| !Arc::ptr_eq(&first.pages[page], &second.pages[page]))
            .collect();
        assert_eq!(unshared, [64, 1000]);
        assert_eq!(bytes_of(&second), bytes_of(&live));
        write(&mut live, &[(PAGE_SIZE * 700, 9)]);
        live.copy_from(&first);
        assert_eq!(bytes_of(&live), bytes_of(&first));
    }

    #[test]
    fn clone_from_keeps_each_side_in_its_own_form() {
        let mut live = written(2 * PAGE_SIZE, &[(1, 1)]);
        let mut slot = snapshot(&mut live);
        write(&mut live, &[(PAGE_SIZE + 2, 2)]);
        slot.clone_from(&live);
        assert!(slot.is_snapshot());
        assert_eq!(bytes_of(&slot), bytes_of(&live));
        let mut other = Paged::zeroed(2 * PAGE_SIZE);
        other.clone_from(&slot);
        assert!(!other.is_snapshot());
        assert_eq!(&*other, &*live);
    }

    #[test]
    fn a_full_copy_of_live_memory_stays_live() {
        let mut live = written(2 * PAGE_SIZE, &[(1, 1)]);
        let _slot = snapshot(&mut live);
        write(&mut live, &[(PAGE_SIZE, 2)]);
        let mut copy = Paged::zeroed(2 * PAGE_SIZE);
        copy.copy_from(&live);
        assert!(!copy.is_snapshot());
        assert_eq!(&*copy, &*live);
        let taken = snapshot(&mut copy);
        assert_eq!(bytes_of(&taken), bytes_of(&live));
    }
}
