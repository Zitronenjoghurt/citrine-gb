use bitflags::bitflags;

bitflags! {
    #[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct Irq: u16 {
        const VBLANK = 1 << 0;
        const HBLANK = 1 << 1;
        const VCOUNT = 1 << 2;
        const TIMER0 = 1 << 3;
        const TIMER1 = 1 << 4;
        const TIMER2 = 1 << 5;
        const TIMER3 = 1 << 6;
        const SERIAL = 1 << 7;
        const DMA0 = 1 << 8;
        const DMA1 = 1 << 9;
        const DMA2 = 1 << 10;
        const DMA3 = 1 << 11;
        const KEYPAD = 1 << 12;
        const GAMEPAK = 1 << 13;
    }
}

impl Irq {
    pub fn timer(index: usize) -> Self {
        Self::from_bits_truncate(Self::TIMER0.bits() << index)
    }

    pub fn dma(channel: usize) -> Self {
        Self::from_bits_truncate(Self::DMA0.bits() << channel)
    }
}

const FLAG_DELAY: u32 = 1;
const WAKE_DELAY: u32 = 2;
const LINE_DELAY: u32 = 3;
const STALLED_STAGES: u32 = LINE_DELAY - 1;
const QUEUE: usize = 4;
const STALLS: usize = 4;

fn past_stall(wait: u32, start: u32, end: u32) -> u32 {
    let first = start.max(wait.saturating_sub(STALLED_STAGES));
    if start >= wait || end <= first {
        return wait;
    }
    wait.max(end) + wait.min(end) - first
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
struct Stalls {
    spans: [(u32, u32); STALLS],
    len: u8,
}

impl Stalls {
    fn push(&mut self, start: u32, end: u32) {
        let len = usize::from(self.len);
        if len == STALLS {
            self.spans[STALLS - 1].1 = end;
            return;
        }
        self.spans[len] = (start, end);
        self.len += 1;
    }

    fn delay(&self, wait: u32) -> u32 {
        self.spans[..usize::from(self.len)]
            .iter()
            .fold(wait, |wait, &(start, end)| past_stall(wait, start, end))
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Hidden {
    bits: [u16; QUEUE],
    waits: [u32; QUEUE],
    len: u8,
}

impl Hidden {
    fn hide(&mut self, irq: Irq, wait: u32) {
        let len = usize::from(self.len);
        if len == QUEUE {
            self.bits.copy_within(1.., 0);
            self.waits.copy_within(1.., 0);
        }
        let index = len.min(QUEUE - 1);
        self.bits[index] = irq.bits();
        self.waits[index] = wait;
        self.len = (index + 1) as u8;
    }

    fn at(&self, now: u32) -> u16 {
        let len = usize::from(self.len);
        (0..len)
            .filter(|&index| self.waits[index] > now)
            .fold(0, |bits, index| bits | self.bits[index])
    }

    fn elapse(&mut self, cycles: u32) {
        let len = usize::from(self.len);
        let mut kept = 0;
        for index in 0..len {
            if self.waits[index] > cycles {
                self.bits[kept] = self.bits[index];
                self.waits[kept] = self.waits[index] - cycles;
                kept += 1;
            }
        }
        self.len = kept as u8;
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Delayed {
    now: bool,
    target: bool,
    waits: [u32; QUEUE],
    values: [bool; QUEUE],
    len: u8,
}

impl Delayed {
    fn set(&mut self, value: bool, wait: u32) {
        if value == self.target {
            return;
        }
        self.target = value;
        let len = usize::from(self.len);
        let mut index = len.min(QUEUE - 1);
        while index > 0 && self.waits[index - 1] > wait {
            index -= 1;
        }
        let end = len.min(QUEUE - 1);
        self.waits.copy_within(index..end, index + 1);
        self.values.copy_within(index..end, index + 1);
        self.waits[index] = wait;
        self.values[index] = value;
        self.len = (len + 1).min(QUEUE) as u8;
    }

    fn elapse(&mut self, cycles: u32) {
        let len = usize::from(self.len);
        let mut kept = 0;
        for index in 0..len {
            let wait = self.waits[index];
            if wait <= cycles {
                self.now = self.values[index];
            } else {
                self.waits[kept] = wait - cycles;
                self.values[kept] = self.values[index];
                kept += 1;
            }
        }
        self.len = kept as u8;
    }

    fn until_change(&self) -> Option<u32> {
        (self.len > 0).then(|| self.waits[0])
    }

    fn at(&self, at: u32) -> bool {
        let len = usize::from(self.len);
        (0..len)
            .take_while(|&index| self.waits[index] <= at)
            .last()
            .map_or(self.now, |index| self.values[index])
    }

    fn show(&mut self, seen: bool) {
        let len = usize::from(self.len);
        if seen == self.now || len == QUEUE {
            return;
        }
        self.waits.copy_within(..len, 1);
        self.values.copy_within(..len, 1);
        self.waits[0] = 0;
        self.values[0] = self.now;
        self.now = seen;
        self.len += 1;
    }

    fn stall(&mut self, start: u32, end: u32) {
        for wait in &mut self.waits[..usize::from(self.len)] {
            *wait = past_stall(*wait, start, end);
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Interrupts {
    enable: Irq,
    flags: Irq,
    master: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    line: Delayed,
    #[cfg_attr(feature = "serde", serde(default))]
    wake: Delayed,
    #[cfg_attr(feature = "serde", serde(default))]
    hidden: Hidden,
    #[cfg_attr(feature = "serde", serde(skip))]
    stalls: Stalls,
}

impl Interrupts {
    pub fn request(&mut self, irq: Irq) {
        self.raise(irq, 0);
    }

    #[inline(always)]
    pub fn raise(&mut self, irq: Irq, at: u32) {
        if irq.is_empty() || self.flags.contains(irq) {
            return;
        }
        self.flag(irq, at);
    }

    fn flag(&mut self, irq: Irq, at: u32) {
        self.flags |= irq;
        self.hidden.hide(irq, at + FLAG_DELAY);
        self.update(at);
    }

    fn update(&mut self, at: u32) {
        let available = self.enable.intersects(self.flags);
        self.wake.set(available, at + WAKE_DELAY);
        let wait = self.stalls.delay(at + LINE_DELAY);
        self.line.set(self.master && available, wait);
    }

    pub fn stall(&mut self, start: u32, end: u32) {
        if end <= start {
            return;
        }
        self.stalls.push(start, end);
        self.line.stall(start, end);
    }

    pub fn elapse_sampled(&mut self, cycles: u32, lead: u32) {
        if self.line.len == 0 {
            self.elapse(cycles);
            return;
        }
        let seen = self.line.at(cycles.saturating_sub(lead));
        self.elapse(cycles);
        self.line.show(seen);
    }

    pub fn elapse(&mut self, cycles: u32) {
        self.stalls.len = 0;
        if self.line.len > 0 {
            self.line.elapse(cycles);
        }
        if self.wake.len > 0 {
            self.wake.elapse(cycles);
        }
        if self.hidden.len > 0 {
            self.hidden.elapse(cycles);
        }
    }

    pub fn asserted(self) -> bool {
        self.master && self.enable.intersects(self.flags)
    }

    pub fn pending(self) -> bool {
        self.line.now
    }

    pub fn wakes(self) -> bool {
        self.wake.now
    }

    pub fn wakes_at(self, now: u32) -> bool {
        self.wake.at(now)
    }

    pub fn until_wake(self) -> Option<u32> {
        self.wake.until_change()
    }

    pub fn ie(self) -> u16 {
        self.enable.bits()
    }

    pub fn if_(self) -> u16 {
        self.if_at(0)
    }

    pub fn if_at(self, now: u32) -> u16 {
        self.flags.bits() & !self.hidden.at(now)
    }

    #[cfg(test)]
    pub fn raised(self) -> u16 {
        self.flags.bits()
    }

    pub fn ime(self) -> u16 {
        u16::from(self.master)
    }

    pub fn write_ie(&mut self, value: u16, mask: u16, at: u32) {
        let bits = (self.enable.bits() & !mask) | (value & mask);
        self.enable = Irq::from_bits_truncate(bits);
        self.update(at);
    }

    pub fn acknowledge(&mut self, value: u16, mask: u16, at: u32) {
        self.flags.remove(Irq::from_bits_truncate(value & mask));
        self.update(at);
    }

    pub fn write_ime(&mut self, value: u16, mask: u16, at: u32) {
        if mask & 1 != 0 {
            self.master = value & 1 != 0;
        }
        self.update(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_and_dma_sources_follow_their_index() {
        let timers: Vec<Irq> = (0..4).map(Irq::timer).collect();
        assert_eq!(timers, [Irq::TIMER0, Irq::TIMER1, Irq::TIMER2, Irq::TIMER3]);
        let channels: Vec<Irq> = (0..4).map(Irq::dma).collect();
        assert_eq!(channels, [Irq::DMA0, Irq::DMA1, Irq::DMA2, Irq::DMA3]);
    }

    #[test]
    fn ie_keeps_only_the_14_sources() {
        let mut interrupts = Interrupts::default();
        interrupts.write_ie(0xFFFF, 0xFFFF, 0);
        assert_eq!(interrupts.ie(), 0x3FFF);
        interrupts.write_ie(0x0000, 0x00FF, 0);
        assert_eq!(interrupts.ie(), 0x3F00);
    }

    #[test]
    fn writing_ones_to_if_clears_those_flags() {
        let mut interrupts = Interrupts::default();
        interrupts.request(Irq::KEYPAD | Irq::VBLANK);
        interrupts.elapse(FLAG_DELAY);
        interrupts.acknowledge(Irq::VBLANK.bits(), 0xFFFF, 0);
        assert_eq!(interrupts.if_(), Irq::KEYPAD.bits());
        interrupts.acknowledge(0xFFFF, 0x00FF, 0);
        assert_eq!(interrupts.if_(), Irq::KEYPAD.bits());
        interrupts.acknowledge(0xFFFF, 0xFF00, 0);
        assert_eq!(interrupts.if_(), 0);
    }

    #[test]
    fn ime_is_one_bit() {
        let mut interrupts = Interrupts::default();
        interrupts.write_ime(0xFFFF, 0xFFFF, 0);
        assert_eq!(interrupts.ime(), 1);
        interrupts.write_ime(0x0000, 0xFF00, 0);
        assert_eq!(interrupts.ime(), 1);
        interrupts.write_ime(0x0000, 0x00FF, 0);
        assert_eq!(interrupts.ime(), 0);
    }

    #[test]
    fn pending_needs_ime_ie_and_if() {
        let mut interrupts = Interrupts::default();
        interrupts.request(Irq::KEYPAD);
        interrupts.elapse(LINE_DELAY);
        assert!(!interrupts.pending());
        interrupts.write_ie(Irq::KEYPAD.bits(), 0xFFFF, 0);
        interrupts.elapse(LINE_DELAY);
        assert!(!interrupts.pending());
        interrupts.write_ime(1, 0xFFFF, 0);
        interrupts.elapse(LINE_DELAY);
        assert!(interrupts.pending());
        interrupts.write_ie(Irq::VBLANK.bits(), 0xFFFF, 0);
        interrupts.elapse(LINE_DELAY);
        assert!(!interrupts.pending());
    }

    #[test]
    fn a_halt_wakes_on_ie_and_if_even_with_ime_off() {
        let mut interrupts = Interrupts::default();
        interrupts.write_ie(Irq::VBLANK.bits(), 0xFFFF, 0);
        interrupts.elapse(LINE_DELAY);
        assert!(!interrupts.wakes());
        interrupts.request(Irq::VBLANK);
        interrupts.elapse(WAKE_DELAY);
        assert!(interrupts.wakes());
        assert!(!interrupts.pending());
    }

    #[test]
    fn a_raise_shows_in_if_after_one_cycle_wakes_a_halt_after_two_and_the_cpu_after_three() {
        let mut interrupts = Interrupts::default();
        interrupts.write_ie(Irq::HBLANK.bits(), 0xFFFF, 0);
        interrupts.write_ime(1, 0xFFFF, 0);
        interrupts.elapse(LINE_DELAY);
        interrupts.raise(Irq::HBLANK, 3);
        assert_eq!(interrupts.if_at(3), 0);
        assert_eq!(interrupts.if_at(4), Irq::HBLANK.bits());
        interrupts.elapse(4);
        assert!(!interrupts.wakes());
        interrupts.elapse(1);
        assert!(interrupts.wakes());
        assert!(!interrupts.pending());
        interrupts.elapse(1);
        assert!(interrupts.pending());
    }

    #[test]
    fn an_acknowledge_right_after_a_raise_still_lets_the_line_pulse() {
        let mut interrupts = Interrupts::default();
        interrupts.write_ie(Irq::TIMER0.bits(), 0xFFFF, 0);
        interrupts.write_ime(1, 0xFFFF, 0);
        interrupts.elapse(LINE_DELAY);
        interrupts.raise(Irq::TIMER0, 0);
        interrupts.acknowledge(Irq::TIMER0.bits(), 0xFFFF, 1);
        interrupts.elapse(LINE_DELAY);
        assert!(interrupts.pending());
        interrupts.elapse(1);
        assert!(!interrupts.pending());
    }

    fn enabled(irq: Irq) -> Interrupts {
        let mut interrupts = Interrupts::default();
        interrupts.write_ie(irq.bits(), 0xFFFF, 0);
        interrupts.write_ime(1, 0xFFFF, 0);
        interrupts.elapse(LINE_DELAY);
        interrupts
    }

    #[test]
    fn a_dma_stall_holds_the_last_two_line_stages() {
        let mut interrupts = enabled(Irq::TIMER0);
        interrupts.raise(Irq::TIMER0, 10);
        interrupts.stall(11, 20);
        interrupts.elapse(21);
        assert!(!interrupts.pending());
        interrupts.elapse(1);
        assert!(interrupts.pending());
    }

    #[test]
    fn a_raise_inside_a_stall_waits_for_its_end() {
        let mut interrupts = enabled(Irq::TIMER0);
        interrupts.stall(5, 20);
        interrupts.raise(Irq::TIMER0, 15);
        interrupts.elapse(21);
        assert!(!interrupts.pending());
        interrupts.elapse(1);
        assert!(interrupts.pending());
    }

    #[test]
    fn a_stall_after_the_second_stage_holds_only_the_last() {
        let mut interrupts = enabled(Irq::TIMER0);
        interrupts.raise(Irq::TIMER0, 10);
        interrupts.stall(12, 20);
        interrupts.elapse(20);
        assert!(!interrupts.pending());
        interrupts.elapse(1);
        assert!(interrupts.pending());
    }

    #[test]
    fn a_stall_that_ends_before_the_raise_changes_nothing() {
        let mut interrupts = enabled(Irq::TIMER0);
        interrupts.stall(2, 8);
        interrupts.raise(Irq::TIMER0, 10);
        interrupts.elapse(LINE_DELAY + 9);
        assert!(!interrupts.pending());
        interrupts.elapse(1);
        assert!(interrupts.pending());
    }

    #[test]
    fn a_stall_leaves_the_wake_chain_alone() {
        let mut interrupts = enabled(Irq::TIMER0);
        interrupts.raise(Irq::TIMER0, 10);
        interrupts.stall(10, 30);
        interrupts.elapse(10 + WAKE_DELAY);
        assert!(interrupts.wakes());
        assert!(!interrupts.pending());
    }

    #[test]
    fn stalls_last_one_step() {
        let mut interrupts = enabled(Irq::TIMER0);
        interrupts.stall(0, 20);
        interrupts.elapse(20);
        interrupts.raise(Irq::TIMER0, 0);
        interrupts.elapse(LINE_DELAY);
        assert!(interrupts.pending());
    }

    #[test]
    fn the_cpu_sees_the_line_as_it_was_when_its_last_access_started() {
        let mut interrupts = enabled(Irq::TIMER0);
        interrupts.raise(Irq::TIMER0, 2);
        interrupts.elapse_sampled(6, 3);
        assert!(!interrupts.pending());
        interrupts.elapse_sampled(1, 0);
        assert!(interrupts.pending());
    }

    #[test]
    fn a_line_up_before_the_last_access_starts_is_seen_at_once() {
        let mut interrupts = enabled(Irq::TIMER0);
        interrupts.raise(Irq::TIMER0, 0);
        interrupts.elapse_sampled(6, 3);
        assert!(interrupts.pending());
    }

    #[test]
    fn a_line_that_drops_during_the_last_access_still_counts_for_one_step() {
        let mut interrupts = enabled(Irq::TIMER0);
        interrupts.raise(Irq::TIMER0, 0);
        interrupts.elapse(LINE_DELAY);
        interrupts.acknowledge(Irq::TIMER0.bits(), 0xFFFF, 1);
        interrupts.elapse_sampled(6, 3);
        assert!(interrupts.pending());
        interrupts.elapse_sampled(1, 0);
        assert!(!interrupts.pending());
    }
}
