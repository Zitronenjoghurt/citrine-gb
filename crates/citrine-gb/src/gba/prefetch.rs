use super::region::Region;
use super::wait::{Access, WaitControl, Width};

const CAPACITY: u32 = 8;
const ROM_PAGE: u32 = 0x2_0000;
const ROM_START: u32 = 0x0800_0000;
const ROM_SPAN: u32 = 0x0600_0000;
const NEVER: u64 = u64::MAX;

#[derive(Debug, Default, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "Saved", into = "Saved"))]
pub struct Prefetch {
    pub(crate) clock: u64,
    pub(crate) due: u64,
    pub(crate) head: u32,
    pub(crate) count: u32,
    pub(crate) paused: bool,
    pub(crate) branched: bool,
    since: u64,
    progress: u32,
    cost: u32,
    page_cost: u32,
    parked: u32,
    active: bool,
    flowing: bool,
    stuck: bool,
    following: bool,
    force_n: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct Saved {
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

impl From<Saved> for Prefetch {
    fn from(saved: Saved) -> Self {
        Self {
            head: saved.head,
            count: saved.count,
            paused: saved.paused,
            branched: saved.branched,
            progress: saved.progress,
            active: saved.active,
            flowing: saved.active && !saved.paused,
            stuck: saved.stuck,
            following: saved.following,
            force_n: saved.force_n,
            ..Self::default()
        }
    }
}

impl From<Prefetch> for Saved {
    fn from(prefetch: Prefetch) -> Self {
        prefetch.saved()
    }
}

impl PartialEq for Prefetch {
    fn eq(&self, other: &Self) -> bool {
        self.saved() == other.saved()
    }
}

impl Eq for Prefetch {}

impl std::hash::Hash for Prefetch {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.saved().hash(state);
    }
}

impl Prefetch {
    pub(crate) fn saved(&self) -> Saved {
        let mut settled = *self;
        settled.settle();
        Saved {
            active: settled.active,
            paused: settled.paused,
            head: if settled.parked != 0 {
                settled.parked
            } else {
                settled.head
            },
            count: settled.count,
            progress: settled.progress,
            force_n: settled.force_n,
            stuck: settled.stuck,
            following: settled.following,
            branched: settled.branched,
        }
    }

    #[inline(always)]
    pub fn hit(&mut self, addr: u32, halves: u32) -> bool {
        if self.head != addr || self.clock >= self.due || !self.holds(halves) {
            return false;
        }
        self.take_hit(addr, halves);
        true
    }

    #[inline(always)]
    fn holds(&self, halves: u32) -> bool {
        self.count >= halves && !(self.paused && self.count == halves)
    }

    #[inline(always)]
    fn take_hit(&mut self, addr: u32, halves: u32) {
        self.branched = false;
        self.count -= halves;
        self.head = addr.wrapping_add(2 * halves);
        self.clock += 1;
    }

    pub fn fetch(&mut self, wait: WaitControl, addr: u32, width: Width, access: Access) -> u32 {
        let branched = std::mem::take(&mut self.branched);
        if !wait.prefetch() {
            self.stop();
            return wait.cycles(addr, width, access);
        }
        let region = Region::of(addr);
        match region {
            Region::Rom(_) => {}
            Region::Sram => {
                return self.interrupt() + wait.region_cycles(region, addr, width, access);
            }
            _ => {
                let cycles = wait.region_cycles(region, addr, width, access);
                self.run(cycles);
                return cycles;
            }
        }
        self.settle();
        let bytes = width.bytes() as u32;
        let halves = bytes.div_ceil(2);
        if !self.active || addr & !(bytes - 1) != self.head {
            return self.flowing_glitch() + self.miss(wait, addr, width, access);
        }
        if branched && self.count == 0 {
            return self.miss(wait, addr, width, access);
        }
        if self.count >= halves {
            self.take(halves);
            self.clock += 1;
            return 1;
        }
        let mut cycles = 0;
        let following = self.following || self.progress > 0;
        while self.count < halves && !self.paused && !self.stuck {
            cycles += self.cost - self.progress;
            self.complete();
        }
        self.following = following;
        if self.count < halves {
            return cycles + self.flowing_glitch() + self.miss(wait, addr, width, access);
        }
        self.take(halves);
        self.refresh();
        cycles
    }

    #[inline(always)]
    pub fn fetch_ahead(&mut self, addr: u32, halves: u32) -> Option<u32> {
        if addr != self.head {
            return None;
        }
        if self.clock < self.due {
            if self.count == 0 {
                if halves == 1 && !(self.branched | self.stuck) {
                    return Some(self.wait_in_flight(addr));
                }
            } else if self.holds(halves) {
                self.take_hit(addr, halves);
                return Some(1);
            }
        }
        self.fetch_ahead_settled(addr, halves)
    }

    #[inline(always)]
    fn wait_in_flight(&mut self, addr: u32) -> u32 {
        let remaining = (self.due - self.clock) as u32;
        if !self.following {
            self.following = self.clock > self.since || self.progress > 0;
        }
        self.progress = 0;
        self.since = self.clock;
        self.head = addr.wrapping_add(2);
        if self.head.is_multiple_of(ROM_PAGE) {
            self.stuck = true;
            self.cost = self.page_cost;
            self.due = NEVER;
        } else {
            self.due = self.clock + u64::from(self.cost);
        }
        remaining
    }

    #[inline(never)]
    fn fetch_ahead_settled(&mut self, addr: u32, halves: u32) -> Option<u32> {
        self.settle();
        if self.count >= halves {
            self.branched = false;
            self.take(halves);
            self.clock += 1;
            self.refresh();
            return Some(1);
        }
        if halves > 1 || self.branched || self.paused || self.stuck {
            return None;
        }
        Some(self.wait_in_flight(addr))
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
        if !wait.prefetch() || aligned == self.head {
            return None;
        }
        self.settle();
        self.branched = false;
        Some(self.flowing_glitch() + self.miss(wait, addr, width, access))
    }

    #[inline(always)]
    pub fn fetch_in_one(&mut self, wait: WaitControl) {
        self.branched = false;
        if wait.prefetch() {
            self.clock += 1;
        } else {
            self.stop();
        }
    }

    #[inline(always)]
    pub fn run(&mut self, cycles: u32) {
        self.clock += u64::from(cycles);
    }

    pub fn interrupt(&mut self) -> u32 {
        self.settle();
        let cycles = self.glitch();
        self.stop();
        cycles
    }

    pub fn retime(&mut self, wait: WaitControl) {
        self.settle();
        if self.active {
            let head = if self.parked != 0 {
                self.parked
            } else {
                self.head
            };
            if wait.prefetch() {
                self.head = head;
                self.parked = 0;
            } else {
                self.head = 0;
                self.parked = head;
            }
            self.flowing = !self.paused && self.parked == 0;
            self.price(wait, head.wrapping_add(2 * self.count));
        }
        if self.progress > 0 {
            self.progress = self.progress.min(self.cost - 1);
        }
        self.refresh();
    }

    pub fn branch(&mut self) {
        self.branched = !self.follows();
    }

    fn follows(&self) -> bool {
        if self.following {
            return true;
        }
        let pending = self.clock - self.since;
        pending > 0
            && self.flowing
            && (self.stuck
                || self.progress > 0
                || pending > 1
                || pending >= u64::from(self.cost - self.progress))
    }

    pub fn stop(&mut self) {
        *self = Self {
            clock: self.clock,
            since: self.clock,
            due: NEVER,
            force_n: self.force_n,
            ..Self::default()
        };
    }

    fn settle(&mut self) {
        let pending = self.clock - self.since;
        self.since = self.clock;
        if pending == 0 || !self.flowing {
            return;
        }
        if self.stuck {
            self.progress = ((u64::from(self.progress) + pending) % u64::from(self.cost)) as u32;
            self.following = true;
            return;
        }
        self.following |= self.progress > 0 || pending > 1;
        let mut cycles = pending;
        loop {
            let remaining = u64::from(self.cost - self.progress);
            if cycles < remaining {
                self.progress += cycles as u32;
                break;
            }
            cycles -= remaining;
            self.complete();
            if cycles == 0 || self.paused {
                break;
            }
            if self.stuck {
                self.progress = (cycles % u64::from(self.cost)) as u32;
                break;
            }
        }
        self.refresh();
    }

    fn refresh(&mut self) {
        self.due = if self.flowing && !self.stuck {
            self.since + u64::from(self.cost - self.progress)
        } else {
            NEVER
        };
    }

    fn glitch(&self) -> u32 {
        u32::from(self.active && !self.paused && self.cost - self.progress == 1)
    }

    fn flowing_glitch(&self) -> u32 {
        u32::from(self.flowing && self.cost - self.progress == 1)
    }

    fn miss(&mut self, wait: WaitControl, addr: u32, width: Width, access: Access) -> u32 {
        let access = if self.force_n {
            Access::NonSequential
        } else {
            access
        };
        let cycles = wait.rom_cycles(addr, width, access);
        let head = addr.wrapping_add(width.bytes() as u32);
        *self = Self {
            clock: self.clock,
            since: self.clock,
            active: true,
            flowing: true,
            stuck: head.is_multiple_of(ROM_PAGE),
            head,
            ..Self::default()
        };
        self.price(wait, head);
        self.refresh();
        cycles
    }

    fn price(&mut self, wait: WaitControl, tail: u32) {
        self.cost = need(wait, tail);
        self.page_cost = need(wait, (tail | (ROM_PAGE - 1)).wrapping_add(1));
    }

    fn complete(&mut self) {
        self.following = true;
        self.count += 1;
        self.progress = 0;
        self.paused = self.count == CAPACITY;
        self.flowing = !self.paused;
        self.stuck = !self.paused && self.tail().is_multiple_of(ROM_PAGE);
        if self.stuck {
            self.cost = self.page_cost;
        }
    }

    fn tail(&self) -> u32 {
        self.head.wrapping_add(2 * self.count)
    }

    fn take(&mut self, halves: u32) {
        self.count -= halves;
        self.head = self.head.wrapping_add(2 * halves);
        if self.paused && self.count == 0 {
            *self = Self {
                clock: self.clock,
                since: self.clock,
                due: NEVER,
                force_n: true,
                ..Self::default()
            };
        }
    }
}

fn need(wait: WaitControl, tail: u32) -> u32 {
    if tail.wrapping_sub(ROM_START) < ROM_SPAN {
        wait.rom_sequential_half(tail)
    } else {
        wait.cycles(tail, Width::Half, Access::Sequential)
    }
}

#[cfg(test)]
mod reference;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::bus::Bus;
    use crate::gba::cpu::{Cpu, CpuBus, CpuState};
    use Access::{NonSequential, Sequential};
    use Width::{Half, Word};

    const ROM: u32 = 0x0800_0100;
    const WAITCNT: u32 = 0x0400_0204;

    fn wait(bits: u16) -> WaitControl {
        let mut wait = WaitControl::default();
        wait.write(bits, 0xFFFF);
        wait
    }

    fn primed(wait: WaitControl) -> Prefetch {
        let mut prefetch = Prefetch::default();
        prefetch.fetch(wait, ROM, Half, NonSequential);
        prefetch
    }

    fn halves(prefetch: &mut Prefetch, wait: WaitControl, from: u32, count: u32) -> Vec<u32> {
        (0..count)
            .map(|i| prefetch.fetch(wait, from + 2 * i, Half, Sequential))
            .collect()
    }

    #[test]
    fn a_miss_costs_a_normal_access() {
        let mut prefetch = Prefetch::default();
        assert_eq!(prefetch.fetch(wait(0x4014), ROM, Half, NonSequential), 4);
        assert_eq!(
            prefetch.fetch(wait(0x4014), ROM + 0x40, Word, NonSequential),
            6
        );
    }

    #[test]
    fn straight_line_code_without_free_cycles_costs_sequential_reads() {
        let wait = wait(0x4014);
        let mut prefetch = primed(wait);
        assert_eq!(halves(&mut prefetch, wait, ROM + 2, 3), [2, 2, 2]);
    }

    #[test]
    fn internal_cycles_fill_the_buffer() {
        let wait = wait(0x4014);
        let mut prefetch = primed(wait);
        prefetch.run(6);
        assert_eq!(halves(&mut prefetch, wait, ROM + 2, 4), [1, 1, 1, 1]);
    }

    #[test]
    fn a_halfword_in_flight_costs_only_its_remaining_cycles() {
        let wait = wait(0x4000);
        let mut prefetch = primed(wait);
        prefetch.run(1);
        assert_eq!(prefetch.fetch(wait, ROM + 2, Half, Sequential), 2);
    }

    #[test]
    fn an_arm_word_takes_two_halfwords_in_one_cycle() {
        let wait = wait(0x4014);
        let mut prefetch = Prefetch::default();
        prefetch.fetch(wait, ROM, Word, NonSequential);
        prefetch.run(4);
        assert_eq!(prefetch.fetch(wait, ROM + 4, Word, Sequential), 1);
    }

    #[test]
    fn an_arm_word_with_nothing_buffered_costs_a_sequential_word() {
        let wait = wait(0x4014);
        let mut prefetch = Prefetch::default();
        prefetch.fetch(wait, ROM, Word, NonSequential);
        assert_eq!(prefetch.fetch(wait, ROM + 4, Word, Sequential), 4);
    }

    #[test]
    fn a_branch_to_the_next_address_with_nothing_buffered_is_non_sequential() {
        let wait = wait(0x4014);
        let mut prefetch = primed(wait);
        prefetch.run(1);
        prefetch.branch();
        assert_eq!(prefetch.fetch(wait, ROM + 2, Half, NonSequential), 4);
    }

    #[test]
    fn a_branch_to_a_buffered_address_reads_from_the_buffer() {
        let wait = wait(0x4014);
        let mut prefetch = primed(wait);
        prefetch.run(6);
        prefetch.branch();
        assert_eq!(prefetch.fetch(wait, ROM + 2, Half, NonSequential), 1);
    }

    #[test]
    fn a_branch_elsewhere_empties_the_buffer() {
        let wait = wait(0x4014);
        let mut prefetch = primed(wait);
        prefetch.run(6);
        prefetch.branch();
        assert_eq!(prefetch.fetch(wait, ROM + 6, Half, NonSequential), 4);
        assert_eq!(prefetch.fetch(wait, ROM + 8, Half, Sequential), 2);
    }

    #[test]
    fn a_hit_takes_its_halfword_before_the_cycle_that_would_fill_the_buffer() {
        let wait = wait(0x4014);
        let mut prefetch = primed(wait);
        prefetch.run(15);
        assert_eq!(halves(&mut prefetch, wait, ROM + 2, 8), [1; 8]);
        assert_eq!(prefetch.fetch(wait, ROM + 18, Half, Sequential), 1);
    }

    #[test]
    fn a_full_buffer_pauses_until_drained_then_restarts_non_sequential() {
        let wait = wait(0x4014);
        let mut prefetch = primed(wait);
        prefetch.run(100);
        assert_eq!(halves(&mut prefetch, wait, ROM + 2, 8), [1; 8]);
        assert_eq!(prefetch.fetch(wait, ROM + 18, Half, Sequential), 4);
    }

    #[test]
    fn the_buffer_stops_at_a_128k_page() {
        let wait = wait(0x4014);
        let mut prefetch = Prefetch::default();
        prefetch.fetch(wait, 0x0801_FFF8, Half, NonSequential);
        prefetch.run(99);
        assert_eq!(halves(&mut prefetch, wait, 0x0801_FFFA, 3), [1, 1, 1]);
        assert_eq!(prefetch.fetch(wait, 0x0802_0000, Half, Sequential), 4);
    }

    #[test]
    fn at_a_128k_page_the_buffer_keeps_fetching_without_filling() {
        let wait = wait(0x4014);
        let mut prefetch = Prefetch::default();
        prefetch.fetch(wait, 0x0801_FFF8, Half, NonSequential);
        prefetch.run(100);
        assert_eq!(halves(&mut prefetch, wait, 0x0801_FFFA, 3), [1, 1, 1]);
        assert_eq!(prefetch.fetch(wait, 0x0802_0000, Half, Sequential), 1 + 4);
    }

    #[test]
    fn a_branch_to_a_128k_page_on_the_last_cycle_of_a_stuck_fetch_costs_one_more() {
        let wait = wait(0x4014);
        let mut prefetch = Prefetch::default();
        prefetch.fetch(wait, 0x0801_FFFC, Half, NonSequential);
        prefetch.run(5);
        assert_eq!(
            prefetch.fetch(wait, 0x0802_0000, Half, NonSequential),
            1 + 4
        );
    }

    #[test]
    fn code_outside_the_game_pak_leaves_the_buffer_filling() {
        let wait = wait(0x4014);
        let mut prefetch = primed(wait);
        prefetch.run(6);
        assert_eq!(prefetch.fetch(wait, 0x0300_0000, Half, NonSequential), 1);
        assert_eq!(prefetch.fetch(wait, ROM + 2, Half, NonSequential), 1);
    }

    #[test]
    fn with_bit_14_off_every_fetch_costs_the_wait_table() {
        let wait = wait(0x0014);
        let mut prefetch = primed(wait);
        prefetch.run(100);
        assert_eq!(halves(&mut prefetch, wait, ROM + 2, 3), [2, 2, 2]);
        assert_eq!(prefetch.fetch(wait, ROM + 8, Half, NonSequential), 4);
    }

    #[test]
    fn a_buffer_parked_by_bit_14_off_never_answers_a_fetch_ahead() {
        let on = wait(0x4014);
        let off = wait(0x0014);
        let mut prefetch = primed(on);
        prefetch.run(6);
        prefetch.retime(off);
        assert_eq!(prefetch.fetch_ahead(ROM + 2, 1), None);
        let mut loaded = Prefetch::from(prefetch.saved());
        loaded.retime(off);
        assert_eq!(loaded.saved(), prefetch.saved());
        assert_eq!(loaded.fetch_ahead(ROM + 2, 1), None);
        loaded.retime(on);
        assert_eq!(loaded.fetch_ahead(ROM + 2, 1), Some(1));
    }

    fn bus_with(waitcnt: u16) -> Bus {
        let mut bus = Bus::new(&[0; 0x200]);
        bus.write16(WAITCNT, waitcnt);
        bus
    }

    #[test]
    fn work_ram_reads_fill_the_buffer() {
        let mut bus = bus_with(0x4014);
        bus.code_cycles(ROM, Half, NonSequential);
        bus.data_cycles(0x0200_0000, Word, NonSequential);
        assert_eq!(bus.code_cycles(ROM + 2, Half, NonSequential), 1);
    }

    #[test]
    fn game_pak_data_reads_empty_the_buffer() {
        let mut bus = bus_with(0x4014);
        bus.code_cycles(ROM, Half, NonSequential);
        for _ in 0..6 {
            CpuBus::idle(&mut bus);
        }
        bus.data_cycles(ROM + 0x80, Word, NonSequential);
        assert_eq!(bus.code_cycles(ROM + 2, Half, NonSequential), 4);
    }

    #[test]
    fn game_pak_writes_empty_the_buffer() {
        let mut bus = bus_with(0x4014);
        bus.code_cycles(ROM, Half, NonSequential);
        bus.data_cycles(0x0200_0000, Word, NonSequential);
        bus.data_cycles(0x0E00_0000, Width::Byte, NonSequential);
        assert_eq!(bus.code_cycles(ROM + 2, Half, NonSequential), 4);
    }

    #[test]
    fn a_cheaper_waitcnt_finishes_the_halfword_in_flight() {
        for (addr, idles, waitcnt) in [(ROM, 2, 0x4010), (0x0C00_0100, 8, 0x4400)] {
            let mut bus = bus_with(0x4000);
            bus.code_cycles(addr, Half, NonSequential);
            for _ in 0..idles {
                CpuBus::idle(&mut bus);
            }
            bus.write16(WAITCNT, waitcnt);
            assert_eq!(bus.code_cycles(addr + 2, Half, Sequential), 1, "{addr:08X}");
        }
    }

    const THUMB_PROGRAM: [u16; 6] = [0x2102, 0x0609, 0x6808, 0xE7FF, 0x46C0, 0xE7FE];

    fn thumb_steps(waitcnt: u16) -> Vec<u32> {
        let rom: Vec<u8> = THUMB_PROGRAM.iter().flat_map(|h| h.to_le_bytes()).collect();
        let mut bus = Bus::new(&rom);
        bus.write16(WAITCNT, waitcnt);
        let state = CpuState {
            cpsr: 0x3F,
            pipeline: [0x2102, 0x0609],
            next_fetch: Sequential,
            r: {
                let mut r = [0; 16];
                r[15] = 0x0800_0004;
                r
            },
            ..CpuState::default()
        };
        let mut cpu = Cpu::from_state(&state);
        (0..4).map(|_| cpu.step(&mut bus)).collect()
    }

    #[test]
    fn a_thumb_program_with_bit_14_off_keeps_todays_counts() {
        assert_eq!(thumb_steps(0x0000), [3, 3, 10, 13]);
    }

    #[test]
    fn a_thumb_program_with_bit_14_on_reads_from_the_buffer() {
        assert_eq!(thumb_steps(0x4000), [3, 3, 10, 10]);
    }

    #[test]
    fn a_branch_away_abandons_the_halfword_in_flight() {
        let wait = wait(0x4000);
        let mut prefetch = primed(wait);
        prefetch.run(1);
        assert_eq!(prefetch.fetch(wait, ROM + 0x40, Half, NonSequential), 5);
    }

    #[test]
    fn a_branch_away_on_the_last_cycle_of_a_fetch_costs_one_more() {
        let wait = wait(0x4000);
        let mut prefetch = primed(wait);
        prefetch.run(2);
        assert_eq!(prefetch.fetch(wait, ROM + 0x40, Half, NonSequential), 1 + 5);
    }

    #[test]
    fn a_branch_away_between_halfwords_costs_nothing_extra() {
        let wait = wait(0x4000);
        let mut prefetch = primed(wait);
        prefetch.run(3);
        assert_eq!(prefetch.fetch(wait, ROM + 0x40, Half, NonSequential), 5);
    }

    #[test]
    fn a_game_pak_data_read_on_the_last_cycle_of_a_fetch_costs_one_more() {
        let mut bus = bus_with(0x4014);
        bus.code_cycles(ROM, Half, NonSequential);
        CpuBus::idle(&mut bus);
        assert_eq!(bus.data_cycles(ROM + 0x80, Half, NonSequential), 1 + 4);
    }

    #[test]
    fn a_game_pak_data_read_abandons_the_halfword_in_flight() {
        let mut bus = bus_with(0x4000);
        bus.code_cycles(ROM, Half, NonSequential);
        CpuBus::idle(&mut bus);
        assert_eq!(bus.data_cycles(ROM + 0x80, Half, NonSequential), 5);
    }

    #[test]
    fn a_branch_to_the_address_being_fetched_pays_only_the_rest_of_the_fetch() {
        let wait = wait(0x4014);
        let mut prefetch = primed(wait);
        prefetch.run(1);
        assert_eq!(prefetch.fetch(wait, ROM + 2, Half, Sequential), 1);
        prefetch.run(1);
        prefetch.branch();
        assert_eq!(prefetch.fetch(wait, ROM + 4, Half, NonSequential), 1);
    }

    #[test]
    fn a_branch_in_the_first_fetch_after_a_miss_pays_a_full_access() {
        let wait = wait(0x4014);
        let mut prefetch = primed(wait);
        prefetch.run(1);
        prefetch.branch();
        assert_eq!(prefetch.fetch(wait, ROM + 2, Half, NonSequential), 4);
    }

    #[test]
    fn a_misaligned_arm_fetch_never_hits_the_buffer() {
        let wait = wait(0x4014);
        let mut prefetch = Prefetch::default();
        prefetch.fetch(wait, ROM + 2, Word, NonSequential);
        prefetch.run(100);
        assert_eq!(
            prefetch.fetch(wait, ROM + 6, Word, Sequential),
            wait.cycles(ROM + 6, Word, Sequential)
        );
    }

    #[test]
    fn a_return_from_the_bios_to_other_rom_code_pays_the_cycle_the_buffer_holds_the_bus() {
        let wait = wait(0x4014);
        let mut prefetch = primed(wait);
        prefetch.run(2);
        assert_eq!(prefetch.fetch(wait, 0x0000_0128, Word, Sequential), 1);
        prefetch.branch();
        assert_eq!(
            prefetch.fetch(wait, ROM + 0x40, Word, NonSequential),
            1 + wait.cycles(ROM + 0x40, Word, NonSequential)
        );
    }

    #[test]
    fn code_from_sram_drops_the_buffer() {
        let wait = wait(0x4014);
        let mut prefetch = primed(wait);
        prefetch.run(6);
        prefetch.fetch(wait, 0x0E00_0000, Half, NonSequential);
        assert_eq!(prefetch.fetch(wait, ROM + 2, Half, NonSequential), 4);
    }

    struct Lcg(u64);

    impl Lcg {
        fn below(&mut self, n: u32) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) % u64::from(n)) as u32
        }
    }

    struct Pair {
        lazy: Prefetch,
        eager: reference::Reference,
        wait: WaitControl,
    }

    const BASES: [u32; 7] = [
        ROM,
        0x0801_FFF0,
        0x09FF_FFF0,
        0x0BFF_FFF0,
        0x0CFF_FFF0,
        0x0DFF_FFF0,
        0x0A00_0100,
    ];

    impl Pair {
        fn rom_code(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
            let aligned = addr & !(width.bytes() as u32 - 1);
            let halves = if width == Word { 2 } else { 1 };
            let wait = self.wait;
            let lazy = match self.lazy.fetch_ahead(aligned, halves) {
                Some(cycles) => cycles,
                None => match self
                    .lazy
                    .fetch_elsewhere(wait, addr, aligned, width, access)
                {
                    Some(cycles) => cycles,
                    None => self.lazy.fetch(wait, addr, width, access),
                },
            };
            let eager = match self.eager.fetch_ahead(wait, aligned, halves) {
                Some(cycles) => cycles,
                None => match self
                    .eager
                    .fetch_elsewhere(wait, addr, aligned, width, access)
                {
                    Some(cycles) => cycles,
                    None => self.eager.fetch(wait, addr, width, access),
                },
            };
            (lazy, eager)
        }

        fn step(&mut self, rng: &mut Lcg) -> (u32, u32) {
            let wait = self.wait;
            match rng.below(20) {
                0..=7 => {
                    let head = self.lazy.saved().head;
                    let addr = if head != 0 && rng.below(4) != 0 {
                        head.wrapping_add(2 * rng.below(3))
                            .wrapping_sub(2 * rng.below(2))
                    } else {
                        BASES[rng.below(BASES.len() as u32) as usize] + 2 * rng.below(16)
                    };
                    let width = if rng.below(4) == 0 { Word } else { Half };
                    let access = if rng.below(3) == 0 {
                        NonSequential
                    } else {
                        Sequential
                    };
                    if addr.wrapping_sub(0x0800_0000) < 0x0500_0000 && rng.below(4) != 0 {
                        self.rom_code(addr, width, access)
                    } else {
                        (
                            self.lazy.fetch(wait, addr, width, access),
                            self.eager.fetch(wait, addr, width, access),
                        )
                    }
                }
                8 | 9 => {
                    let cycles = if rng.below(8) == 0 {
                        rng.below(400)
                    } else {
                        rng.below(12)
                    };
                    self.lazy.run(cycles);
                    self.eager.run(wait, cycles);
                    (0, 0)
                }
                10 => {
                    self.lazy.run(1);
                    self.eager.tick(wait);
                    (0, 0)
                }
                11 => {
                    self.lazy.fetch_in_one(wait);
                    self.eager.fetch_in_one(wait);
                    (0, 0)
                }
                12 | 13 => {
                    self.lazy.branch();
                    self.eager.branch();
                    (0, 0)
                }
                14 => (self.lazy.interrupt(), self.eager.interrupt(wait)),
                15 => {
                    let addr =
                        [0x0000_0128, 0x0200_0000, 0x0300_0000, 0x0E00_0000][rng.below(4) as usize];
                    (
                        self.lazy.fetch(wait, addr, Half, Sequential),
                        self.eager.fetch(wait, addr, Half, Sequential),
                    )
                }
                16 => {
                    let prefetch = if rng.below(5) == 0 { 0 } else { 0x4000 };
                    self.wait.write(rng.below(0x4000) as u16 | prefetch, 0xFFFF);
                    self.lazy.retime(self.wait);
                    self.eager.retime(self.wait);
                    (0, 0)
                }
                17 => {
                    self.lazy = Prefetch::from(self.lazy.saved());
                    self.lazy.retime(self.wait);
                    (0, 0)
                }
                _ => {
                    self.lazy.stop();
                    self.eager.stop();
                    (0, 0)
                }
            }
        }
    }

    #[test]
    fn the_lazy_buffer_matches_running_it_cycle_by_cycle() {
        for seed in 0..400u64 {
            let mut rng = Lcg(seed);
            let wait = wait(0x4000 | rng.below(0x4000) as u16);
            let mut pair = Pair {
                lazy: Prefetch::default(),
                eager: reference::Reference::default(),
                wait,
            };
            for step in 0..4000 {
                let (lazy, eager) = pair.step(&mut rng);
                assert_eq!(lazy, eager, "cycles, seed {seed} step {step}");
                assert_eq!(
                    pair.lazy.saved(),
                    pair.eager.saved(),
                    "state, seed {seed} step {step}"
                );
            }
        }
    }
}
