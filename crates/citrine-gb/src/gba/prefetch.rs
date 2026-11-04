use super::region::Region;
use super::wait::{Access, WaitControl, Width};

const CAPACITY: u32 = 8;
const ROM_PAGE: u32 = 0x2_0000;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Prefetch {
    active: bool,
    paused: bool,
    head: u32,
    count: u32,
    progress: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    force_n: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    stuck: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    following: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    branched: bool,
}

impl Prefetch {
    pub fn fetch(&mut self, wait: WaitControl, addr: u32, width: Width, access: Access) -> u32 {
        let branched = std::mem::take(&mut self.branched);
        if !wait.prefetch() {
            self.stop();
            return wait.cycles(addr, width, access);
        }
        match Region::of(addr) {
            Region::Rom(_) => {}
            Region::Sram => return self.interrupt(wait) + wait.cycles(addr, width, access),
            _ => {
                let cycles = wait.cycles(addr, width, access);
                self.run(wait, cycles);
                return cycles;
            }
        }
        let bytes = width.bytes() as u32;
        let halves = bytes.div_ceil(2);
        if !self.active || addr & !(bytes - 1) != self.head {
            return self.glitch(wait) + self.miss(wait, addr, width, access);
        }
        if branched && self.count == 0 {
            return self.miss(wait, addr, width, access);
        }
        if self.count >= halves {
            self.take(halves);
            self.run(wait, 1);
            return 1;
        }
        let mut cycles = 0;
        let following = self.following || self.progress > 0;
        while self.count < halves && !self.paused && !self.stuck {
            let remaining = self.need(wait) - self.progress;
            cycles += remaining;
            self.run(wait, remaining);
        }
        self.following = following;
        if self.count < halves {
            return cycles + self.glitch(wait) + self.miss(wait, addr, width, access);
        }
        self.take(halves);
        cycles
    }

    pub fn run(&mut self, wait: WaitControl, mut cycles: u32) {
        if !wait.prefetch() {
            return;
        }
        while cycles > 0 && self.active && !self.paused {
            if self.stuck {
                self.progress = (self.progress + cycles) % self.need(wait);
                self.following = true;
                return;
            }
            let remaining = self.need(wait) - self.progress;
            self.following |= self.progress > 0 || cycles > 1;
            if cycles < remaining {
                self.progress += cycles;
                return;
            }
            cycles -= remaining;
            self.complete();
        }
    }

    pub fn interrupt(&mut self, wait: WaitControl) -> u32 {
        let cycles = self.glitch(wait);
        self.stop();
        cycles
    }

    pub fn branch(&mut self) {
        self.branched = !self.following;
    }

    pub fn stop(&mut self) {
        *self = Self {
            force_n: self.force_n,
            ..Self::default()
        };
    }

    fn glitch(&self, wait: WaitControl) -> u32 {
        u32::from(self.active && !self.paused && self.need(wait) - self.progress == 1)
    }

    fn miss(&mut self, wait: WaitControl, addr: u32, width: Width, access: Access) -> u32 {
        let access = if self.force_n {
            Access::NonSequential
        } else {
            access
        };
        let cycles = wait.cycles(addr, width, access);
        let head = addr.wrapping_add(width.bytes() as u32);
        *self = Self {
            active: true,
            stuck: head.is_multiple_of(ROM_PAGE),
            head,
            ..Self::default()
        };
        cycles
    }

    fn tail(&self) -> u32 {
        self.head.wrapping_add(2 * self.count)
    }

    fn need(&self, wait: WaitControl) -> u32 {
        match Region::of(self.tail()) {
            Region::Rom(state) => 1 + wait.rom_second_waits(state),
            _ => wait.cycles(self.tail(), Width::Half, Access::Sequential),
        }
    }

    fn complete(&mut self) {
        self.following = true;
        self.count += 1;
        self.progress = 0;
        self.paused = self.count == CAPACITY;
        self.stuck = !self.paused && self.tail().is_multiple_of(ROM_PAGE);
    }

    fn take(&mut self, halves: u32) {
        self.count -= halves;
        self.head = self.head.wrapping_add(2 * halves);
        if self.paused && self.count == 0 {
            *self = Self {
                force_n: true,
                ..Self::default()
            };
        }
    }
}

#[cfg(test)]
mod tests;
