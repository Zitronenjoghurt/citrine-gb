use super::Cpu;
use super::interrupt::Irq;
use crate::gba::timer::Timers as Counters;

pub const FIRST: usize = 0x100;
pub const LAST: usize = 0x10F;
pub const TIMERS: usize = crate::gba::timer::TIMERS;

const LAZY_LIMIT: u32 = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename = "Timers"))]
struct Settled {
    counters: Counters,
    divider: u32,
    carry: u32,
}

impl Settled {
    fn advance(&mut self, cycles: u32) -> Irq {
        let total = self.carry + cycles;
        self.carry = total % self.divider;
        let overflows = self.counters.step(total / self.divider);
        Irq::from_bits_truncate(u32::from(overflows.irq().bits()))
    }

    fn until_event(&self) -> Option<u32> {
        self.counters
            .until_overflow()
            .map(|bus| (bus * self.divider).saturating_sub(self.carry).max(1))
    }

    fn write(&mut self, offset: usize, value: u32, mask: u32) {
        let offset = offset & !3;
        for half in 0..2 {
            let shift = 16 * half;
            let mask = (mask >> shift) as u16;
            if mask != 0 {
                self.counters
                    .write(offset + 2 * half, (value >> shift) as u16, mask);
            }
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "Settled", into = "Settled"))]
pub struct Timers {
    settled: Settled,
    owed: u32,
    next: Option<u32>,
    lazy: u32,
}

impl From<Settled> for Timers {
    fn from(settled: Settled) -> Self {
        let mut timers = Self {
            settled,
            owed: 0,
            next: None,
            lazy: 0,
        };
        timers.plan();
        timers
    }
}

impl From<Timers> for Settled {
    fn from(timers: Timers) -> Self {
        timers.settled()
    }
}

impl PartialEq for Timers {
    fn eq(&self, other: &Self) -> bool {
        self.settled() == other.settled()
    }
}

impl Eq for Timers {}

impl std::hash::Hash for Timers {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.settled().hash(state);
    }
}

impl Timers {
    pub fn new(cpu: Cpu) -> Self {
        Settled {
            counters: Counters::default(),
            divider: match cpu {
                Cpu::Arm9 => 2,
                Cpu::Arm7 => 1,
            },
            carry: 0,
        }
        .into()
    }

    fn settled(&self) -> Settled {
        let mut settled = self.settled;
        if self.owed > 0 {
            settled.advance(self.owed);
        }
        settled
    }

    fn settle(&mut self) {
        let owed = std::mem::take(&mut self.owed);
        if owed > 0 {
            self.settled.advance(owed);
        }
    }

    fn plan(&mut self) {
        self.next = self.settled.until_event();
        self.lazy = self.next.map_or(LAZY_LIMIT, |next| next.min(LAZY_LIMIT));
    }

    pub fn counter(&self, timer: usize) -> u16 {
        self.settled().counters.counter(timer)
    }

    pub fn reload(&self, timer: usize) -> u16 {
        self.settled().counters.reload(timer)
    }

    pub fn control(&self, timer: usize) -> u16 {
        self.settled().counters.control(timer)
    }

    pub fn read(&self, offset: usize) -> u32 {
        let offset = offset & !3;
        let counters = self.settled().counters;
        u32::from(counters.read(offset)) | (u32::from(counters.read(offset + 2)) << 16)
    }

    pub fn write(&mut self, offset: usize, value: u32, mask: u32) {
        self.settle();
        self.settled.write(offset, value, mask);
        self.plan();
    }

    pub fn step(&mut self, cycles: u32) -> Irq {
        let owed = self.owed + cycles;
        if owed < self.lazy {
            self.owed = owed;
            return Irq::empty();
        }
        self.owed = 0;
        let irq = self.settled.advance(owed);
        self.plan();
        irq
    }

    pub fn until_event(&self) -> Option<u32> {
        self.next.map(|next| next - self.owed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: u32 = 0xFFFF_FFFF;
    const START: u32 = 1 << 23;
    const CASCADE: u32 = 1 << 18;
    const IRQ: u32 = 1 << 22;

    fn started(cpu: Cpu, timer: usize, reload: u16, control: u32) -> Timers {
        let mut timers = Timers::new(cpu);
        timers.write(4 * timer, u32::from(reload) | START | control, ALL);
        timers
    }

    fn run_to_irq(timers: &mut Timers) -> (u32, Irq) {
        let mut elapsed = 0;
        loop {
            let at = timers.until_event().expect("a timer runs");
            elapsed += at;
            let irq = timers.step(at);
            if !irq.is_empty() {
                return (elapsed, irq);
            }
        }
    }

    fn period(cpu: Cpu, reload: u16, control: u32) -> u32 {
        let mut timers = started(cpu, 0, reload, IRQ | control);
        run_to_irq(&mut timers);
        run_to_irq(&mut timers).0
    }

    #[test]
    fn prescalers_follow_gbatek() {
        assert_eq!(period(Cpu::Arm7, 0xFFF0, 0), 16);
        assert_eq!(period(Cpu::Arm7, 0xFFF0, 1 << 16), 16 * 64);
        assert_eq!(period(Cpu::Arm7, 0xFFF0, 2 << 16), 16 * 256);
        assert_eq!(period(Cpu::Arm7, 0xFFF0, 3 << 16), 16 * 1024);
    }

    #[test]
    fn reload_zero_counts_the_full_range() {
        assert_eq!(period(Cpu::Arm7, 0, 0), 0x1_0000);
    }

    #[test]
    fn arm9_timers_run_at_half_its_clock() {
        assert_eq!(period(Cpu::Arm9, 0xFFF0, 0), 32);
        assert_eq!(period(Cpu::Arm9, 0xFF00, 1 << 16), 2 * 256 * 64);
    }

    #[test]
    fn arm9_odd_steps_carry_over() {
        let mut timers = started(Cpu::Arm9, 0, 0, 0);
        timers.step(4);
        let before = timers.counter(0);
        for _ in 0..10 {
            timers.step(1);
        }
        assert_eq!(timers.counter(0), before.wrapping_add(5));
    }

    #[test]
    fn each_timer_raises_its_own_irq() {
        let mut timers = started(Cpu::Arm7, 2, 0xFFFF, IRQ);
        assert_eq!(run_to_irq(&mut timers).1, Irq::TIMER2);
    }

    #[test]
    fn no_irq_without_the_enable_bit() {
        let mut timers = started(Cpu::Arm7, 0, 0xFFFE, 0);
        assert!(timers.step(100).is_empty());
    }

    #[test]
    fn cascade_counts_the_previous_overflows() {
        let mut timers = started(Cpu::Arm7, 0, 0xFFFF, 0);
        timers.write(4, 0xFFFE | START | CASCADE | IRQ, ALL);
        let (_, irq) = run_to_irq(&mut timers);
        assert_eq!(irq, Irq::TIMER1);
        assert_eq!(run_to_irq(&mut timers).0, 2);
    }

    #[test]
    fn registers_read_counter_and_control() {
        let mut timers = Timers::new(Cpu::Arm7);
        timers.write(8, 0x1234 | (0xC3 << 16), ALL);
        timers.step(1);
        assert_eq!(timers.reload(2), 0x1234);
        assert_eq!(timers.control(2), 0xC3);
        assert_eq!(timers.read(8) >> 16, 0xC3);
    }

    #[test]
    fn halfword_writes_keep_the_other_half() {
        let mut timers = Timers::new(Cpu::Arm7);
        timers.write(0, 0x0000_5555, 0x0000_FFFF);
        timers.write(0, 0x0041_0000, 0xFFFF_0000);
        timers.step(1);
        assert_eq!(timers.reload(0), 0x5555);
        assert_eq!(timers.control(0), 0x41);
    }

    #[test]
    fn idle_timers_have_no_event() {
        assert_eq!(Timers::new(Cpu::Arm9).until_event(), None);
    }

    #[test]
    fn lazy_steps_match_stepping_every_slice() {
        let mut seed = 0x2545_F491u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for cpu in [Cpu::Arm9, Cpu::Arm7] {
            let mut lazy = Timers::new(cpu);
            let mut eager = lazy.settled();
            for _ in 0..50_000 {
                match next() % 32 {
                    0 => {
                        let timer = (next() % 4) as usize;
                        let reload = 0xFFFF - (next() % 0x400);
                        let control = (next() & 0xC7) << 16;
                        let mask = [ALL, 0xFFFF, 0xFFFF_0000][(next() % 3) as usize];
                        lazy.write(4 * timer, reload | control, mask);
                        eager.write(4 * timer, reload | control, mask);
                    }
                    1 => {
                        for timer in 0..4 {
                            let counters = eager.counters;
                            let word = u32::from(counters.read(4 * timer))
                                | (u32::from(counters.read(4 * timer + 2)) << 16);
                            assert_eq!(lazy.read(4 * timer), word);
                        }
                        assert_eq!(lazy.settled(), eager);
                    }
                    _ => {
                        let cycles = if next() % 64 == 0 {
                            next() % 5000
                        } else {
                            next() % 80
                        };
                        assert_eq!(lazy.step(cycles), eager.advance(cycles));
                        assert_eq!(lazy.until_event(), eager.until_event());
                    }
                }
            }
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn owed_cycles_save_as_if_stepped() {
        let mut lazy = started(Cpu::Arm9, 0, 0x8000, 0);
        lazy.step(1001);
        lazy.step(999);
        assert_eq!(lazy.owed, 999);
        let eager = lazy.settled();
        let saved = serde_json::to_string(&lazy).expect("timers save");
        assert_eq!(saved, serde_json::to_string(&eager).expect("timers save"));
        let loaded: Timers = serde_json::from_str(&saved).expect("timers load");
        assert_eq!(loaded, lazy);
        assert_eq!(loaded.counter(0), lazy.counter(0));
    }
}
