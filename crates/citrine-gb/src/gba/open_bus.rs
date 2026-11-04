use super::region::Region;
use super::wait::Width;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OpenBus {
    addr: u32,
    last: u32,
    bios: u32,
    iwram: u32,
}

impl OpenBus {
    pub fn fetched_at(&mut self, addr: u32) {
        self.addr = addr;
    }

    pub fn read(&mut self, addr: u32, width: Width, value: u32) {
        self.last = match Region::of(addr) {
            Region::Iwram => {
                self.iwram = merge(self.iwram, addr, width, value);
                self.iwram
            }
            _ => spread(value, width),
        };
    }

    pub fn wrote(&mut self, addr: u32, width: Width, value: u32) {
        if Region::of(addr) == Region::Iwram {
            self.iwram = merge(self.iwram, addr, width, value);
        }
        self.last = merge(self.last, addr, width, value);
    }

    pub fn dma_read(&mut self, addr: u32, width: Width, value: u32) {
        if Region::of(addr) == Region::Iwram {
            self.iwram = merge(self.iwram, addr, width, value);
        }
        self.last = spread(value, width);
    }

    pub fn put(&mut self, word: u32) {
        self.last = word;
    }

    pub fn latch_bios(&mut self, word: u32) {
        self.bios = word;
    }

    pub fn in_bios(&self) -> bool {
        Region::of(self.addr) == Region::Bios
    }

    pub fn bios(&self) -> u32 {
        self.bios
    }

    pub fn value(&self) -> u32 {
        self.last
    }
}

fn spread(value: u32, width: Width) -> u32 {
    match width {
        Width::Byte => (value & 0xFF) * 0x0101_0101,
        Width::Half => (value & 0xFFFF) * 0x0001_0001,
        Width::Word => value,
    }
}

fn merge(word: u32, addr: u32, width: Width, value: u32) -> u32 {
    let (mask, shift) = match width {
        Width::Byte => (0xFF, 8 * (addr & 3)),
        Width::Half => (0xFFFF, 8 * (addr & 2)),
        Width::Word => return value,
    };
    (word & !(mask << shift)) | (value & mask) << shift
}

pub fn lane(word: u32, addr: u32, width: Width) -> u32 {
    match width {
        Width::Byte => word >> (8 * (addr & 3)) & 0xFF,
        Width::Half => word >> (8 * (addr & 2)) & 0xFFFF,
        Width::Word => word,
    }
}

#[cfg(test)]
mod tests;
