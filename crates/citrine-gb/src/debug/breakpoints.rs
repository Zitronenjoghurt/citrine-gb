use crate::debug::code::Address;
use crate::debug::step::Stop;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Access {
    Read,
    Write,
    Change,
}

impl Access {
    fn matches(self, seen: Access) -> bool {
        self == seen || (self == Access::Write && seen == Access::Change)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Watch {
    pub from: Address,
    pub len: u32,
    pub access: Access,
}

impl Watch {
    pub fn covers(&self, at: Address) -> bool {
        at.block == self.from.block
            && at.bank == self.from.bank
            && at.offset >= self.from.offset
            && at.offset - self.from.offset < self.len
    }

    pub fn last(&self) -> Address {
        self.from.offset_by(i64::from(self.len.max(1)) - 1)
    }
}

#[derive(Debug, Default)]
pub struct Breakpoints {
    code: HashSet<Address>,
    watches: Vec<Watch>,
    pub(crate) instruction: u16,
    pending: Option<Stop>,
    last: Option<Stop>,
}

impl Breakpoints {
    pub fn contains(&self, at: &Address) -> bool {
        self.code.contains(at)
    }

    pub fn insert(&mut self, at: Address) -> bool {
        self.code.insert(at)
    }

    pub fn remove(&mut self, at: &Address) -> bool {
        self.code.remove(at)
    }

    pub fn toggle(&mut self, at: Address) {
        if !self.code.remove(&at) {
            self.code.insert(at);
        }
    }

    pub fn code(&self) -> Vec<Address> {
        let mut code: Vec<Address> = self.code.iter().copied().collect();
        code.sort();
        code
    }

    pub fn watch(&mut self, watch: Watch) {
        if !self.watches.contains(&watch) {
            self.watches.push(watch);
        }
    }

    pub fn unwatch(&mut self, watch: Watch) {
        self.watches.retain(|kept| *kept != watch);
    }

    pub fn watches(&self) -> &[Watch] {
        &self.watches
    }

    pub fn is_empty(&self) -> bool {
        self.code.is_empty() && self.watches.is_empty()
    }

    pub fn last_stop(&self) -> Option<Stop> {
        self.last
    }

    pub fn clear_last_stop(&mut self) {
        self.last = None;
    }

    #[inline]
    pub(crate) fn watching(&self) -> bool {
        !self.watches.is_empty()
    }

    #[inline]
    pub(crate) fn armed(&self) -> bool {
        !self.code.is_empty() || self.pending.is_some()
    }

    pub(crate) fn check(&mut self, at: Address, seen: Access, by: Option<Address>) {
        if self.pending.is_some() {
            return;
        }
        if let Some(watch) = self
            .watches
            .iter()
            .find(|watch| watch.covers(at) && watch.access.matches(seen))
        {
            self.pending = Some(Stop::Watchpoint {
                at,
                access: watch.access,
                by,
            });
        }
    }

    pub(crate) fn take_pending(&mut self) -> Option<Stop> {
        let stop = self.pending.take();
        if stop.is_some() {
            self.last = stop;
        }
        stop
    }

    pub(crate) fn record(&mut self, stop: Stop) {
        self.last = Some(stop);
    }
}
