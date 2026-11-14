use crate::nds::rtc::CYCLES_PER_SECOND;

const US_PER_SECOND: u64 = 1_000_000;
const HZ: u64 = CYCLES_PER_SECOND as u64;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MicroClock {
    remainder: u64,
}

impl MicroClock {
    pub fn advance(&mut self, cycles: u32) -> u64 {
        let total = self.remainder + u64::from(cycles) * US_PER_SECOND;
        self.remainder = total % HZ;
        total / HZ
    }

    pub fn cycles_until(&self, us: u64) -> u32 {
        let needed = (us * HZ).saturating_sub(self.remainder);
        needed.div_ceil(US_PER_SECOND).clamp(1, u64::from(u32::MAX)) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_of_cycles_is_a_million_microseconds() {
        let mut clock = MicroClock::default();
        let total: u64 = (0..1000).map(|_| clock.advance(33_513)).sum();
        let tail = clock.advance(982);
        assert_eq!(total + tail, 1_000_000);
    }

    #[test]
    fn cycles_until_reaches_exactly_the_next_microsecond() {
        let mut clock = MicroClock::default();
        for _ in 0..50 {
            let cycles = clock.cycles_until(1);
            assert!(cycles == 33 || cycles == 34);
            assert_eq!(clock.advance(cycles - 1), 0);
            assert_eq!(clock.advance(1), 1);
        }
    }

    #[test]
    fn cycles_until_many_microseconds_needs_no_more_than_that() {
        let mut clock = MicroClock::default();
        clock.advance(17);
        let cycles = clock.cycles_until(1024);
        assert_eq!(clock.clone().advance(cycles - 1), 1023);
        assert_eq!(clock.advance(cycles), 1024);
    }
}
