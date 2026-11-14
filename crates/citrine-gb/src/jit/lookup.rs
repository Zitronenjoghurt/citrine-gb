use super::{CodeRam, PcHasher};
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;
use std::mem::offset_of;

const BITS: u32 = 16;
const SLOTS: usize = 1 << BITS;
const NO_KEY: u32 = 2;
const SPARSE: usize = SLOTS / 8;

pub type Enter = (usize, u32, *const u32);

pub const SLOT_PIPELINE: u32 = offset_of!(Slot, pipeline) as u32;
pub const SLOT_GENERATION: u32 = offset_of!(Slot, generation) as u32;
pub const SLOT_LINKS: u32 = offset_of!(Slot, links) as u32;
pub const LINKS: usize = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub slot: usize,
    pub jump: usize,
    pub words: usize,
    pub pc: [u32; 2],
    pub generation: u32,
    pub start: u32,
    pub entry: u32,
}

impl Link {
    pub const NONE: Self = Self {
        slot: 0,
        jump: 0,
        words: 0,
        pc: [0; 2],
        generation: 0,
        start: 0,
        entry: 0,
    };
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Slot {
    tag: u32,
    start: u32,
    pipeline: [u32; 2],
    code: usize,
    words: *const u32,
    generation: u32,
    links: [Link; LINKS],
}

unsafe impl Send for Slot {}
unsafe impl Sync for Slot {}

impl Slot {
    const EMPTY: Self = Self {
        tag: 0,
        start: 0,
        pipeline: [0; 2],
        code: 0,
        words: std::ptr::null(),
        generation: 0,
        links: [Link::NONE; LINKS],
    };
}

type Pages = HashMap<u32, Vec<u32>, BuildHasherDefault<PcHasher>>;
type Keys = HashSet<u32, BuildHasherDefault<PcHasher>>;

#[derive(Default)]
pub struct Lookup {
    slots: Option<Box<[Slot; SLOTS]>>,
    pages: Pages,
    listed: Keys,
    generation: u32,
}

impl Lookup {
    #[cfg(test)]
    fn find(&self, key: u32, pipeline: [u32; 2]) -> Option<Enter> {
        self.find_slot(key, pipeline).map(|(enter, _)| enter)
    }

    #[inline(always)]
    pub fn find_slot(&self, key: u32, pipeline: [u32; 2]) -> Option<(Enter, usize)> {
        let slot = &self.slots.as_ref()?[index(key)];
        (slot.tag == key ^ NO_KEY && slot.pipeline == pipeline).then_some((
            (slot.code, slot.start, slot.words),
            slot as *const Slot as usize,
        ))
    }

    pub fn link(&mut self, from: usize, cell: usize, link: Link) {
        let Some(slots) = &mut self.slots else {
            return;
        };
        let (Some(from), Some(to)) = (held(slots, from), held(slots, link.slot)) else {
            return;
        };
        let generation = slots[to].generation;
        if let Some(cell) = slots[from].links.get_mut(cell) {
            *cell = Link { generation, ..link };
        }
    }

    pub fn fill(
        &mut self,
        key: u32,
        pipeline: [u32; 2],
        (code, start, words): Enter,
        pages: &[(CodeRam, usize)],
    ) {
        debug_assert!(key != NO_KEY);
        if self.slots.is_none() {
            self.slots = vec![Slot::EMPTY; SLOTS].into_boxed_slice().try_into().ok();
        }
        let Some(slots) = &mut self.slots else {
            return;
        };
        if self.listed.insert(key) {
            for &(ram, page) in pages {
                let keys = self.pages.entry(page_key(ram, page)).or_default();
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            for slot in slots.iter_mut() {
                slot.links = [Link::NONE; LINKS];
                slot.generation = u32::from(slot.tag != 0);
            }
            self.generation = 2;
        }
        slots[index(key)] = Slot {
            tag: key ^ NO_KEY,
            start,
            pipeline,
            code,
            words,
            generation: self.generation,
            links: [Link::NONE; LINKS],
        };
    }

    pub fn forget(&mut self, key: u32) {
        self.listed.remove(&key);
        self.empty(key);
    }

    pub fn forget_page(&mut self, ram: CodeRam, page: usize) -> u64 {
        let Some(keys) = self.pages.remove(&page_key(ram, page)) else {
            return 0;
        };
        for &key in &keys {
            self.listed.remove(&key);
            self.empty(key);
        }
        keys.len() as u64
    }

    pub fn clear(&mut self) {
        if let Some(slots) = &mut self.slots {
            if self.listed.len() < SPARSE {
                for &key in &self.listed {
                    let slot = &mut slots[index(key)];
                    if slot.tag == key ^ NO_KEY {
                        *slot = Slot::EMPTY;
                    }
                }
            } else {
                slots.fill(Slot::EMPTY);
            }
        }
        self.listed.clear();
        self.pages.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.listed.is_empty()
    }

    fn empty(&mut self, key: u32) {
        if let Some(slots) = &mut self.slots {
            let slot = &mut slots[index(key)];
            if slot.tag == key ^ NO_KEY {
                *slot = Slot::EMPTY;
            }
        }
    }
}

fn held(slots: &[Slot; SLOTS], at: usize) -> Option<usize> {
    let offset = at.checked_sub(slots.as_ptr() as usize)?;
    let index = offset / size_of::<Slot>();
    (offset % size_of::<Slot>() == 0 && index < SLOTS).then_some(index)
}

fn index(key: u32) -> usize {
    (key.wrapping_mul(0x9E37_79B1) >> (32 - BITS)) as usize
}

fn page_key(ram: CodeRam, page: usize) -> u32 {
    (ram as u32) << 24 | page as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORDS: [u32; 3] = [1, 2, 3];

    fn enter(code: usize) -> Enter {
        (code, 0, WORDS.as_ptr())
    }

    #[test]
    fn a_filled_key_is_found_only_with_its_pipeline() {
        let mut lookup = Lookup::default();
        assert!(lookup.find(0x0200_0000, [1, 2]).is_none());
        lookup.fill(0x0200_0000, [1, 2], enter(7), &[(CodeRam::Main, 0)]);
        assert_eq!(lookup.find(0x0200_0000, [1, 2]).map(|e| e.0), Some(7));
        assert!(lookup.find(0x0200_0000, [1, 3]).is_none());
        assert!(lookup.find(0x0200_0001, [1, 2]).is_none());
    }

    #[test]
    fn an_empty_slot_never_matches_any_aligned_key() {
        let mut lookup = Lookup::default();
        lookup.fill(0x0200_0100, [0, 0], enter(7), &[(CodeRam::Main, 0)]);
        for key in [0, 1, 0x0200_0000, 0xFFFF_0000, 0x0000_0003] {
            assert!(lookup.find(key, [0, 0]).is_none(), "{key:#x}");
        }
    }

    #[test]
    fn a_written_page_drops_every_block_on_it_and_no_other() {
        let mut lookup = Lookup::default();
        lookup.fill(0x0200_0000, [1, 2], enter(7), &[(CodeRam::Main, 0)]);
        let straddles = [(CodeRam::Main, 0), (CodeRam::Main, 1)];
        lookup.fill(0x0200_03FC, [1, 2], enter(8), &straddles);
        lookup.fill(0x0200_0400, [1, 2], enter(9), &[(CodeRam::Main, 1)]);
        lookup.fill(0x0000_0000, [1, 2], enter(6), &[(CodeRam::Itcm, 0)]);
        assert_eq!(lookup.forget_page(CodeRam::Main, 1), 2);
        assert!(lookup.find(0x0200_03FC, [1, 2]).is_none());
        assert!(lookup.find(0x0200_0400, [1, 2]).is_none());
        assert!(lookup.find(0x0200_0000, [1, 2]).is_some());
        assert!(lookup.find(0x0000_0000, [1, 2]).is_some());
        assert_eq!(lookup.forget_page(CodeRam::Main, 1), 0);
    }

    #[test]
    fn a_refilled_block_is_dropped_again_by_its_other_page() {
        let mut lookup = Lookup::default();
        let straddles = [(CodeRam::Main, 0), (CodeRam::Main, 1)];
        lookup.fill(0x0200_03FC, [1, 2], enter(8), &straddles);
        lookup.forget_page(CodeRam::Main, 0);
        lookup.fill(0x0200_03FC, [1, 2], enter(8), &straddles);
        assert_eq!(lookup.forget_page(CodeRam::Main, 1), 1);
        assert!(lookup.find(0x0200_03FC, [1, 2]).is_none());
        assert!(lookup.is_empty());
    }

    #[test]
    fn a_cleared_lookup_finds_no_key_it_held_however_many() {
        for count in [3, SPARSE, SLOTS] {
            let mut lookup = Lookup::default();
            let keys: Vec<u32> = (0..count as u32).map(|i| 0x0200_0000 + i * 6).collect();
            for (i, &key) in keys.iter().enumerate() {
                lookup.fill(key, [1, 2], enter(i), &[(CodeRam::Main, i / 512)]);
            }
            lookup.clear();
            assert!(lookup.is_empty());
            assert!(
                keys.iter().all(|&key| lookup.find(key, [1, 2]).is_none()),
                "{count}"
            );
            lookup.fill(keys[0], [1, 2], enter(1), &[(CodeRam::Main, 0)]);
            assert_eq!(lookup.forget_page(CodeRam::Main, 0), 1);
        }
    }

    fn slot(lookup: &Lookup, key: u32) -> usize {
        lookup.find_slot(key, [1, 2]).map_or(0, |(_, slot)| slot)
    }

    fn linked(lookup: &Lookup, from: usize, cell: usize) -> Option<u32> {
        let slots = lookup.slots.as_ref()?;
        let link = slots[held(slots, from)?].links[cell];
        let to = &slots[held(slots, link.slot)?];
        (link.slot != 0 && to.generation == link.generation).then_some(link.start)
    }

    fn link_to(lookup: &mut Lookup, from: u32, cell: usize, to: u32) {
        let (from, to) = (slot(lookup, from), slot(lookup, to));
        let link = Link {
            slot: to,
            start: 5,
            ..Link::NONE
        };
        lookup.link(from, cell, link);
    }

    #[test]
    fn a_link_holds_until_its_target_is_forgotten_written_or_cleared() {
        let (from, to) = (0x0200_0000, 0x0200_1000);
        let drops: [fn(&mut Lookup); 3] = [
            |lookup| lookup.forget(0x0200_1000),
            |lookup| {
                lookup.forget_page(CodeRam::Main, 1);
            },
            |lookup| lookup.fill(0x0200_1000, [1, 2], enter(9), &[(CodeRam::Main, 1)]),
        ];
        for undo in drops {
            let mut lookup = Lookup::default();
            lookup.fill(from, [1, 2], enter(7), &[(CodeRam::Main, 0)]);
            lookup.fill(to, [1, 2], enter(8), &[(CodeRam::Main, 1)]);
            link_to(&mut lookup, from, 1, to);
            let source = slot(&lookup, from);
            assert_eq!(linked(&lookup, source, 1), Some(5));
            assert_eq!(linked(&lookup, source, 0), None);
            undo(&mut lookup);
            assert_eq!(linked(&lookup, source, 1), None);
        }
        let mut lookup = Lookup::default();
        lookup.fill(from, [1, 2], enter(7), &[(CodeRam::Main, 0)]);
        lookup.fill(to, [1, 2], enter(8), &[(CodeRam::Main, 1)]);
        link_to(&mut lookup, from, 0, to);
        let source = slot(&lookup, from);
        lookup.clear();
        lookup.fill(from, [1, 2], enter(7), &[(CodeRam::Main, 0)]);
        lookup.fill(to, [1, 2], enter(8), &[(CodeRam::Main, 1)]);
        assert_eq!(linked(&lookup, source, 0), None);
    }

    #[test]
    fn a_refilled_source_drops_its_links_and_strangers_link_nothing() {
        let (from, to) = (0x0200_0000, 0x0200_1000);
        let mut lookup = Lookup::default();
        lookup.fill(from, [1, 2], enter(7), &[(CodeRam::Main, 0)]);
        lookup.fill(to, [1, 2], enter(8), &[(CodeRam::Main, 1)]);
        link_to(&mut lookup, from, 0, to);
        let source = slot(&lookup, from);
        lookup.fill(from, [1, 2], enter(6), &[(CodeRam::Main, 0)]);
        assert_eq!(linked(&lookup, source, 0), None);
        let target = slot(&lookup, to);
        for stranger in [0, 1, source + 8, target + 1] {
            let link = Link {
                slot: target,
                ..Link::NONE
            };
            lookup.link(stranger, 0, link);
            lookup.link(
                source,
                0,
                Link {
                    slot: stranger,
                    ..Link::NONE
                },
            );
        }
        assert_eq!(linked(&lookup, source, 0), None);
        lookup.link(
            source,
            LINKS,
            Link {
                slot: target,
                ..Link::NONE
            },
        );
        assert_eq!(linked(&lookup, source, LINKS - 1), None);
        assert_eq!(slot(&lookup, from), source);
    }

    #[test]
    fn a_forgotten_key_leaves_a_colliding_neighbour_alone() {
        let mut lookup = Lookup::default();
        let first = 0x0200_0000;
        let other = (first + 2..)
            .step_by(2)
            .find(|&key| index(key) == index(first))
            .expect("some key shares the slot");
        lookup.fill(first, [1, 2], enter(7), &[(CodeRam::Main, 0)]);
        lookup.fill(other, [1, 2], enter(8), &[(CodeRam::Main, 0)]);
        lookup.forget(first);
        assert_eq!(lookup.find(other, [1, 2]).map(|e| e.0), Some(8));
    }
}
