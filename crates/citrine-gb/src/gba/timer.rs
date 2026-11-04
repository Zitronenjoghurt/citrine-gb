use super::interrupt::Irq;

pub const TIMERS: usize = 4;
const CONTROL_MASK: u16 = 0x00C7;
const CASCADE: u16 = 1 << 2;
const IRQ_ENABLE: u16 = 1 << 6;
const START: u16 = 1 << 7;
const PRESCALER_SHIFTS: [u32; 4] = [0, 6, 8, 10];
const COUNTER_RANGE: u32 = 0x1_0000;
#[cfg(test)]
pub(crate) const START_DELAY: u32 = 2;
const WRITE_DELAY: u32 = 1;
const PENDING_RELOAD: u8 = 1;
const PENDING_CONTROL: u8 = 2;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Timer {
    reload: u16,
    counter: u16,
    control: u16,
    ticks: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    delay: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    pending: u8,
    #[cfg_attr(feature = "serde", serde(default))]
    apply_in: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    next_reload: u16,
    #[cfg_attr(feature = "serde", serde(default))]
    next_control: u16,
}

impl Timer {
    fn started(self) -> bool {
        self.control & START != 0
    }

    fn shift(self) -> u32 {
        PRESCALER_SHIFTS[usize::from(self.control & 0b11)]
    }

    fn apply(&mut self, cascade_allowed: bool, phase: u32) {
        if self.pending & PENDING_RELOAD != 0 {
            self.reload = self.next_reload;
        }
        if self.pending & PENDING_CONTROL != 0 {
            let was_started = self.started();
            self.control = self.next_control;
            if self.started() {
                let offset = phase & ((1 << self.shift()) - 1);
                self.start(was_started, cascade_allowed, offset);
            }
        }
        self.pending = 0;
    }

    fn start(&mut self, was_started: bool, cascade_allowed: bool, offset: u32) {
        let cascade = cascade_allowed && self.control & CASCADE != 0;
        if was_started {
            if !cascade {
                self.ticks = offset;
                self.delay = 0;
            }
        } else if cascade {
            self.counter = self.reload;
        } else if self.counter == 0xFFFF && offset == 0 {
            self.ticks = 0;
            self.delay = 0;
        } else {
            self.ticks = offset;
            self.delay = 1;
        }
    }

    fn prescale(&mut self, cycles: u32) -> u32 {
        let waited = cycles.min(self.delay);
        self.delay -= waited;
        if waited > 0 && self.delay == 0 {
            self.counter = self.reload;
        }
        let cycles = cycles - waited;
        let shift = self.shift();
        let total = u64::from(self.ticks) + u64::from(cycles);
        self.ticks = (total & ((1 << shift) - 1)) as u32;
        (total >> shift) as u32
    }

    fn first_overflow(self) -> u32 {
        let counter = if self.delay > 0 {
            self.reload
        } else {
            self.counter
        };
        let to_overflow = COUNTER_RANGE - u32::from(counter);
        self.delay + (to_overflow << self.shift()) - self.ticks
    }

    fn period(self) -> u32 {
        (COUNTER_RANGE - u32::from(self.reload)) << self.shift()
    }

    fn count(&mut self, increments: u32) -> u32 {
        let to_overflow = COUNTER_RANGE - u32::from(self.counter);
        if increments < to_overflow {
            self.counter += increments as u16;
            return 0;
        }
        let period = COUNTER_RANGE - u32::from(self.reload);
        let past = increments - to_overflow;
        self.counter = (u32::from(self.reload) + past % period) as u16;
        1 + past / period
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Overflows {
    counts: [u32; TIMERS],
    irq: Irq,
    #[cfg_attr(feature = "serde", serde(default))]
    irq_at: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    first_at: [u32; TIMERS],
}

impl Overflows {
    pub fn count(self, timer: usize) -> u32 {
        self.counts[timer]
    }

    pub fn irq(self) -> Irq {
        self.irq
    }

    pub fn irq_at(self) -> u32 {
        self.irq_at
    }

    pub fn first_at(self, timer: usize) -> u32 {
        self.first_at[timer]
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.counts.iter().fold(0, |any, count| any | count) == 0
    }

    pub fn starting_at(mut self, start: u32) -> Self {
        if self.is_empty() {
            return self;
        }
        for at in &mut self.first_at {
            *at += start;
        }
        self
    }

    pub fn add(&mut self, other: Self) {
        self.irq |= other.irq;
        if other.is_empty() {
            return;
        }
        for index in 0..TIMERS {
            let earlier = self.counts[index] == 0 || other.first_at[index] < self.first_at[index];
            if other.counts[index] > 0 && earlier {
                self.first_at[index] = other.first_at[index];
            }
            self.counts[index] += other.counts[index];
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Timers {
    timers: [Timer; TIMERS],
    #[cfg_attr(feature = "serde", serde(default))]
    phase: u32,
}

impl Timers {
    pub fn counter(&self, timer: usize) -> u16 {
        self.timers[timer].counter
    }

    pub fn reload(&self, timer: usize) -> u16 {
        self.timers[timer].reload
    }

    pub fn control(&self, timer: usize) -> u16 {
        self.timers[timer].control
    }

    pub fn period(&self, index: usize) -> u32 {
        self.timers[index].period()
    }

    pub fn until_overflow(&self) -> Option<u32> {
        self.timers
            .iter()
            .enumerate()
            .filter(|(index, timer)| {
                timer.started() && (*index == 0 || timer.control & CASCADE == 0)
            })
            .map(|(_, timer)| timer.first_overflow())
            .chain(self.next_apply())
            .min()
    }

    pub fn running(&self) -> bool {
        self.timers.iter().any(|timer| timer.started())
    }

    fn next_apply(&self) -> Option<u32> {
        self.timers
            .iter()
            .filter(|timer| timer.pending != 0)
            .map(|timer| timer.apply_in)
            .min()
    }

    pub fn read(&self, offset: usize) -> u16 {
        let timer = self.timers[offset / 4];
        if offset & 2 == 0 {
            timer.counter
        } else {
            timer.control
        }
    }

    pub fn write(&mut self, offset: usize, value: u16, mask: u16) {
        let timer = &mut self.timers[offset / 4];
        if offset & 2 == 0 {
            let reload = if timer.pending & PENDING_RELOAD != 0 {
                timer.next_reload
            } else {
                timer.reload
            };
            timer.next_reload = (reload & !mask) | (value & mask);
            timer.pending |= PENDING_RELOAD;
        } else {
            let control = if timer.pending & PENDING_CONTROL != 0 {
                timer.next_control
            } else {
                timer.control
            };
            timer.next_control = ((control & !mask) | (value & mask)) & CONTROL_MASK;
            timer.pending |= PENDING_CONTROL;
        }
        timer.apply_in = WRITE_DELAY;
    }

    #[inline(always)]
    pub fn step(&mut self, cycles: u32) -> Overflows {
        let start = self.phase;
        self.phase = start.wrapping_add(cycles);
        let (started, pending) = self
            .timers
            .iter()
            .fold((0, 0), |(started, pending), timer| {
                (started | timer.control & START, pending | timer.pending)
            });
        if started == 0 && pending == 0 {
            return Overflows::default();
        }
        self.step_active(start, cycles, pending != 0)
    }

    #[inline(never)]
    fn step_active(&mut self, start: u32, cycles: u32, pending: bool) -> Overflows {
        let mut overflows = Overflows::default();
        let mut done = 0;
        if pending {
            while let Some(at) = self.next_apply().filter(|&at| at <= cycles - done) {
                self.elapse_pending(at);
                self.run(at, done, &mut overflows);
                done += at;
                let phase = start.wrapping_add(done);
                for (index, timer) in self.timers.iter_mut().enumerate() {
                    if timer.pending != 0 && timer.apply_in == 0 {
                        timer.apply(index > 0, phase);
                    }
                }
            }
            self.elapse_pending(cycles - done);
        }
        self.run(cycles - done, done, &mut overflows);
        overflows
    }

    fn elapse_pending(&mut self, cycles: u32) {
        for timer in &mut self.timers {
            if timer.pending != 0 {
                timer.apply_in = timer.apply_in.saturating_sub(cycles);
            }
        }
    }

    fn run(&mut self, cycles: u32, since: u32, overflows: &mut Overflows) {
        if cycles == 0 || !self.running() {
            return;
        }
        let mut carry = 0;
        let mut parent = (0, 0);
        for (index, timer) in self.timers.iter_mut().enumerate() {
            if !timer.started() {
                carry = 0;
                continue;
            }
            let cascade = index > 0 && timer.control & CASCADE != 0;
            let before = *timer;
            let increments = if cascade {
                carry
            } else {
                timer.prescale(cycles)
            };
            carry = timer.count(increments);
            if carry == 0 {
                continue;
            }
            let first = overflows.counts[index] == 0;
            overflows.counts[index] += carry;
            let timing = if cascade {
                let to_overflow = COUNTER_RANGE - u32::from(before.counter);
                let (first, period) = parent;
                (
                    first + (to_overflow - 1).saturating_mul(period),
                    period.saturating_mul(COUNTER_RANGE - u32::from(before.reload)),
                )
            } else {
                (before.first_overflow(), before.period())
            };
            parent = timing;
            if first {
                overflows.first_at[index] = since + timing.0;
            }
            if timer.control & IRQ_ENABLE != 0 {
                let at = since + timing.0;
                if overflows.irq.is_empty() || at < overflows.irq_at {
                    overflows.irq_at = at;
                }
                overflows.irq |= Irq::timer(index);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTROL: usize = 2;

    fn offset(timer: usize) -> usize {
        4 * timer
    }

    fn start(timers: &mut Timers, timer: usize, reload: u16, control: u16) {
        timers.write(offset(timer), reload, 0xFFFF);
        timers.write(offset(timer) + CONTROL, control | START, 0xFFFF);
        timers.step(START_DELAY);
    }

    #[test]
    fn a_started_timer_waits_two_cycles_before_counting() {
        let mut timers = Timers::default();
        timers.write(CONTROL, START, 0xFFFF);
        timers.step(2);
        assert_eq!(timers.read(0), 0);
        timers.step(1);
        assert_eq!(timers.read(0), 1);
    }

    #[test]
    fn reload_write_leaves_the_counter_alone() {
        let mut timers = Timers::default();
        timers.write(0, 0x1234, 0xFFFF);
        timers.step(1);
        assert_eq!(timers.read(0), 0);
        assert_eq!(timers.reload(0), 0x1234);
    }

    #[test]
    fn start_copies_the_reload_into_the_counter() {
        let mut timers = Timers::default();
        start(&mut timers, 1, 0xABCD, 0);
        assert_eq!(timers.read(offset(1)), 0xABCD);
    }

    #[test]
    fn writing_start_again_while_running_does_not_reload() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0x1000, 0);
        timers.step(5);
        timers.write(offset(0) + CONTROL, START, 0xFFFF);
        assert_eq!(timers.read(0), 0x1005);
    }

    #[test]
    fn control_keeps_only_its_defined_bits() {
        let mut timers = Timers::default();
        timers.write(offset(2) + CONTROL, 0xFFFF, 0xFFFF);
        timers.step(1);
        assert_eq!(timers.read(offset(2) + CONTROL), 0x00C7);
        timers.write(offset(2) + CONTROL, 0x0000, 0xFF00);
        timers.step(1);
        assert_eq!(timers.read(offset(2) + CONTROL), 0x00C7);
    }

    #[test]
    fn each_prescaler_divides_the_clock() {
        for (prescaler, divider) in [(0, 1), (1, 64), (2, 256), (3, 1024)] {
            let mut timers = Timers::default();
            start(&mut timers, 0, 0, prescaler);
            let first = if divider == 1 { 1 } else { divider - 1 };
            timers.step(first - 1);
            assert_eq!(timers.counter(0), 0, "prescaler {prescaler}");
            timers.step(1);
            assert_eq!(timers.counter(0), 1, "prescaler {prescaler}");
            timers.step(divider * 10);
            assert_eq!(timers.counter(0), 11, "prescaler {prescaler}");
        }
    }

    #[test]
    fn a_prescaled_timer_ticks_on_the_free_running_prescaler() {
        for divider in [64u32, 256, 1024] {
            let prescaler = divider.trailing_zeros() / 2 - 2;
            let mut timers = Timers::default();
            timers.step(5);
            timers.write(offset(0) + CONTROL, START | prescaler as u16, 0xFFFF);
            timers.step(1);
            timers.step(divider - 6);
            assert_eq!(timers.counter(0), 0, "divider {divider}");
            timers.step(1);
            assert_eq!(timers.counter(0), 1, "divider {divider}");
        }
    }

    #[test]
    fn writes_land_one_cycle_late() {
        let mut timers = Timers::default();
        timers.write(0, 0x1234, 0xFFFF);
        timers.write(offset(0) + CONTROL, START, 0xFFFF);
        assert_eq!(timers.reload(0), 0);
        assert_eq!(timers.control(0), 0);
        timers.step(1);
        assert_eq!(timers.reload(0), 0x1234);
        assert_eq!(timers.control(0), START);
    }

    #[test]
    fn a_start_with_the_counter_at_ffff_overflows_before_the_reload() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0xFFFD, 0);
        timers.step(1);
        timers.write(offset(0) + CONTROL, 0, 0xFFFF);
        timers.step(1);
        assert_eq!(timers.counter(0), 0xFFFF);
        timers.write(0, 0, 0xFFFF);
        timers.write(offset(0) + CONTROL, START | IRQ_ENABLE, 0xFFFF);
        let overflows = timers.step(2);
        assert_eq!(overflows.irq(), Irq::TIMER0);
        assert_eq!(timers.counter(0), 0);
    }

    #[test]
    fn prescaler_remainder_carries_across_steps() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0, 1);
        for _ in 0..4 {
            timers.step(16);
        }
        assert_eq!(timers.counter(0), 1);
    }

    #[test]
    fn stopped_counter_reads_frozen() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0x0100, 0);
        timers.step(0x20);
        timers.write(offset(0) + CONTROL, 0, 0xFFFF);
        timers.step(0x1000);
        assert_eq!(timers.read(0), 0x0121);
    }

    #[test]
    fn restart_after_stop_reloads() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0x0100, 0);
        timers.step(0x20);
        timers.write(offset(0) + CONTROL, 0, 0xFFFF);
        timers.step(1);
        let stopped = timers.read(0);
        timers.write(offset(0) + CONTROL, START, 0xFFFF);
        timers.step(1);
        assert_eq!(timers.read(0), stopped);
        timers.step(1);
        assert_eq!(timers.read(0), 0x0100);
    }

    #[test]
    fn overflow_reloads_and_counts_each_wrap() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0xFFF0, 0);
        let overflows = timers.step(0x10);
        assert_eq!(overflows.count(0), 1);
        assert_eq!(timers.counter(0), 0xFFF0);
        let overflows = timers.step(0x10 * 3 + 5);
        assert_eq!(overflows.count(0), 3);
        assert_eq!(timers.counter(0), 0xFFF5);
    }

    #[test]
    fn reload_of_ffff_overflows_every_tick() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0xFFFF, 0);
        assert_eq!(timers.step(7).count(0), 7);
        assert_eq!(timers.counter(0), 0xFFFF);
    }

    #[test]
    fn new_reload_applies_at_the_next_overflow() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0xFFF0, 0);
        timers.write(0, 0xFF00, 0xFFFF);
        assert_eq!(timers.counter(0), 0xFFF0);
        timers.step(0x10);
        assert_eq!(timers.counter(0), 0xFF00);
    }

    #[test]
    fn overflow_requests_an_irq_only_when_enabled() {
        let mut timers = Timers::default();
        start(&mut timers, 2, 0xFFFF, 0);
        start(&mut timers, 3, 0xFFFF, IRQ_ENABLE);
        let overflows = timers.step(1);
        assert_eq!(overflows.count(2), 1);
        assert_eq!(overflows.irq(), Irq::TIMER3);
    }

    #[test]
    fn count_up_timer_ticks_on_the_previous_overflow() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0xFF00, 0);
        start(&mut timers, 1, 0xFFFE, CASCADE | IRQ_ENABLE | 0b11);
        let overflows = timers.step(0x100);
        assert_eq!(timers.counter(1), 0xFFFF);
        assert_eq!(overflows.irq(), Irq::empty());
        let overflows = timers.step(0x100);
        assert_eq!(overflows.count(1), 1);
        assert_eq!(overflows.irq(), Irq::TIMER1);
        assert_eq!(timers.counter(1), 0xFFFE);
    }

    #[test]
    fn cascade_chains_through_all_four_timers() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0xFFFF, 0);
        for timer in 1..TIMERS {
            start(&mut timers, timer, 0xFFFF, CASCADE);
        }
        let overflows = timers.step(3);
        assert_eq!((1..TIMERS).map(|t| overflows.count(t)).sum::<u32>(), 9);
    }

    #[test]
    fn count_up_does_nothing_while_the_previous_timer_is_stopped() {
        let mut timers = Timers::default();
        start(&mut timers, 1, 0, CASCADE);
        timers.step(0x10000);
        assert_eq!(timers.counter(1), 0);
    }

    #[test]
    fn count_up_bit_is_ignored_on_timer_0() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0, CASCADE);
        timers.step(9);
        assert_eq!(timers.counter(0), 9);
    }

    #[test]
    fn stopped_count_up_timer_breaks_the_chain() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0xFFFF, 0);
        start(&mut timers, 2, 0, CASCADE);
        timers.step(4);
        assert_eq!(timers.counter(2), 0);
    }

    #[test]
    fn step_is_a_no_op_when_every_timer_is_off() {
        let mut timers = Timers::default();
        timers.write(0, 0xFFFF, 0xFFFF);
        assert_eq!(timers.step(1_000_000), Overflows::default());
        assert_eq!(timers.counter(0), 0);
        assert!(!timers.running());
    }

    #[test]
    fn huge_step_on_the_slowest_prescaler_does_not_overflow_the_tick_count() {
        let mut timers = Timers::default();
        start(&mut timers, 0, 0, 3);
        timers.step(u32::MAX);
        timers.step(u32::MAX);
        assert_eq!(timers.counter(0), ((2 * u64::from(u32::MAX)) >> 10) as u16);
    }

    #[test]
    fn a_started_timer_shows_its_old_count_until_the_second_cycle() {
        let mut timers = Timers::default();
        timers.write(0, 0xFFE0, 0xFFFF);
        timers.write(CONTROL, START, 0xFFFF);
        timers.step(1);
        assert_eq!(timers.read(0), 0);
        timers.step(1);
        assert_eq!(timers.read(0), 0xFFE0);
        timers.step(1);
        assert_eq!(timers.read(0), 0xFFE1);
    }

    #[test]
    fn a_prescaled_timer_loads_its_reload_in_the_second_cycle() {
        let mut timers = Timers::default();
        timers.step(5);
        timers.write(0, 0xFFE0, 0xFFFF);
        timers.write(CONTROL, START | 1, 0xFFFF);
        timers.step(1);
        assert_eq!(timers.read(0), 0);
        timers.step(1);
        assert_eq!(timers.read(0), 0xFFE0);
    }
}
