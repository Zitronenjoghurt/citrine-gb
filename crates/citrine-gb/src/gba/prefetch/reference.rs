use crate::gba::region::Region;
use crate::gba::wait::{Access, WaitControl, Width};

const CAPACITY: u32 = 8;
const ROM_PAGE: u32 = 0x2_0000;
const ROM_START: u32 = 0x0800_0000;
const ROM_SPAN: u32 = 0x0600_0000;

#[derive(Debug, Default, Clone, Copy)]
pub struct Reference {
    active: bool,
    paused: bool,
    head: u32,
    count: u32,
    progress: u32,
    force_n: bool,
    stuck: bool,
    following: bool,
    branched: bool,
}

impl Reference {
    pub fn saved(&self) -> super::Saved {
        super::Saved {
            active: self.active,
            paused: self.paused,
            head: self.head,
            count: self.count,
            progress: self.progress,
            force_n: self.force_n,
            stuck: self.stuck,
            following: self.following,
            branched: self.branched,
        }
    }

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
            self.run_one(wait);
            return 1;
        }
        let mut cycles = 0;
        let following = self.following || self.progress > 0;
        while self.count < halves && !self.paused && !self.stuck {
            let remaining = self.need(wait) - self.progress;
            cycles += remaining;
            if remaining == 0 {
                self.run(wait, remaining);
            } else {
                self.complete();
            }
        }
        self.following = following;
        if self.count < halves {
            return cycles + self.glitch(wait) + self.miss(wait, addr, width, access);
        }
        self.take(halves);
        cycles
    }

    #[inline(always)]
    pub fn fetch_ahead(&mut self, wait: WaitControl, addr: u32, halves: u32) -> Option<u32> {
        if !wait.prefetch() || !self.active || addr != self.head {
            return None;
        }
        if self.count >= halves {
            self.branched = false;
            self.take(halves);
            self.run_one(wait);
            return Some(1);
        }
        if halves > 1 || self.branched || self.paused || self.stuck {
            return None;
        }
        let remaining = wait.rom_sequential_half(addr).checked_sub(self.progress)?;
        if remaining == 0 {
            return None;
        }
        self.following |= self.progress > 0;
        self.progress = 0;
        self.head = addr.wrapping_add(2);
        self.stuck = self.head.is_multiple_of(ROM_PAGE);
        Some(remaining)
    }

    #[inline(always)]
    pub fn fetch_elsewhere(
        &mut self,
        wait: WaitControl,
        addr: u32,
        aligned: u32,
        width: Width,
        access: Access,
    ) -> Option<u32> {
        if !wait.prefetch() || (self.active && aligned == self.head) {
            return None;
        }
        self.branched = false;
        Some(self.glitch(wait) + self.miss(wait, addr, width, access))
    }

    #[inline(always)]
    pub fn fetch_in_one(&mut self, wait: WaitControl) {
        self.branched = false;
        if wait.prefetch() {
            self.run_one(wait);
        } else {
            self.stop();
        }
    }

    #[inline(always)]
    pub fn run(&mut self, wait: WaitControl, cycles: u32) {
        if !wait.prefetch() || !self.active || self.paused || cycles == 0 {
            return;
        }
        if !self.stuck && self.progress + cycles < self.need(wait) {
            self.following |= self.progress > 0 || cycles > 1;
            self.progress += cycles;
            return;
        }
        self.run_long(wait, cycles);
    }

    #[inline(never)]
    fn run_long(&mut self, wait: WaitControl, mut cycles: u32) {
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

    #[inline(always)]
    pub fn tick(&mut self, wait: WaitControl) {
        if wait.prefetch() {
            self.run_one(wait);
        }
    }

    #[inline(always)]
    fn run_one(&mut self, wait: WaitControl) {
        if self.active && !self.paused && !self.stuck {
            let remaining = self.need(wait) - self.progress;
            if remaining > 0 {
                self.following |= self.progress > 0;
                if remaining > 1 {
                    self.progress += 1;
                } else {
                    self.complete();
                }
                return;
            }
        }
        self.run(wait, 1);
    }

    pub fn interrupt(&mut self, wait: WaitControl) -> u32 {
        let cycles = self.glitch(wait);
        self.stop();
        cycles
    }

    pub fn retime(&mut self, wait: WaitControl) {
        if self.progress > 0 {
            self.progress = self.progress.min(self.need(wait) - 1);
        }
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

    #[inline(always)]
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

    #[inline(always)]
    fn need(&self, wait: WaitControl) -> u32 {
        let tail = self.tail();
        if tail.wrapping_sub(ROM_START) < ROM_SPAN {
            wait.rom_sequential_half(tail)
        } else {
            wait.cycles(tail, Width::Half, Access::Sequential)
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
