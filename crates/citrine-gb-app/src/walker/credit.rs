#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Counter {
    #[default]
    None,
    HealthConnect,
    StepSensor,
}

impl Counter {
    fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::HealthConnect => 1,
            Self::StepSensor => 2,
        }
    }

    fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::None),
            1 => Some(Self::HealthConnect),
            2 => Some(Self::StepSensor),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Credit {
    pub from: u64,
    pub to: u64,
    pub steps: u32,
}

pub trait StepSource {
    fn counter(&self) -> Counter;
    fn steps(&mut self, from: u64, to: u64) -> Option<u32>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Credited {
    pub counter: Counter,
    pub until: Option<u64>,
}

impl Credited {
    pub const SIZE: usize = 9;

    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[0] = self.counter.tag();
        bytes[1..].copy_from_slice(&self.until.unwrap_or(0).to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; Self::SIZE] = bytes.try_into().ok()?;
        let until = u64::from_le_bytes(bytes[1..].try_into().ok()?);
        Some(Self {
            counter: Counter::from_tag(bytes[0])?,
            until: (until != 0).then_some(until),
        })
    }

    pub fn next(&mut self, source: &mut dyn StepSource, now: u64) -> Option<Credit> {
        let Some(from) = self.until else {
            self.until = Some(now);
            self.counter = source.counter();
            return None;
        };
        if now <= from {
            return None;
        }
        let steps = source.steps(from, now)?;
        self.until = Some(now);
        self.counter = source.counter();
        Some(Credit {
            from,
            to: now,
            steps,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[derive(Default)]
    pub(crate) struct Pedometer {
        pub(crate) taken: Vec<(u64, u32)>,
        pub(crate) asked: Vec<(u64, u64)>,
        pub(crate) offline: bool,
    }

    impl StepSource for Pedometer {
        fn counter(&self) -> Counter {
            Counter::HealthConnect
        }

        fn steps(&mut self, from: u64, to: u64) -> Option<u32> {
            self.asked.push((from, to));
            if self.offline {
                return None;
            }
            Some(
                self.taken
                    .iter()
                    .filter(|(at, _)| *at >= from && *at < to)
                    .map(|(_, steps)| steps)
                    .sum(),
            )
        }
    }

    #[test]
    fn the_first_credit_starts_the_window_and_counts_nothing_before_it() {
        let mut source = Pedometer {
            taken: vec![(50, 900)],
            ..Pedometer::default()
        };
        let mut credited = Credited::default();
        assert_eq!(credited.next(&mut source, 100), None);
        assert_eq!(credited.until, Some(100));
        assert_eq!(credited.counter, Counter::HealthConnect);
        assert!(source.asked.is_empty());
    }

    #[test]
    fn each_window_starts_where_the_last_one_ended() {
        let mut source = Pedometer {
            taken: vec![(150, 300), (199, 20), (200, 7), (260, 40)],
            ..Pedometer::default()
        };
        let mut credited = Credited {
            counter: Counter::None,
            until: Some(100),
        };
        let first = credited.next(&mut source, 200).unwrap();
        let second = credited.next(&mut source, 300).unwrap();
        assert_eq!(
            (first, second),
            (
                Credit {
                    from: 100,
                    to: 200,
                    steps: 320
                },
                Credit {
                    from: 200,
                    to: 300,
                    steps: 47
                }
            )
        );
        assert_eq!(credited.next(&mut source, 300), None);
    }

    #[test]
    fn a_source_without_an_answer_keeps_the_window_open() {
        let mut source = Pedometer {
            taken: vec![(150, 300)],
            offline: true,
            ..Pedometer::default()
        };
        let mut credited = Credited {
            counter: Counter::None,
            until: Some(100),
        };
        assert_eq!(credited.next(&mut source, 200), None);
        source.offline = false;
        assert_eq!(credited.next(&mut source, 250).unwrap().steps, 300);
        assert_eq!(source.asked, vec![(100, 200), (100, 250)]);
    }

    #[test]
    fn the_record_round_trips_and_refuses_an_unknown_counter() {
        let credited = Credited {
            counter: Counter::StepSensor,
            until: Some(1_790_000_000),
        };
        assert_eq!(Credited::from_bytes(&credited.to_bytes()), Some(credited));
        assert_eq!(
            Credited::from_bytes(&Credited::default().to_bytes()),
            Some(Credited::default())
        );
        let mut bytes = credited.to_bytes();
        bytes[0] = 9;
        assert_eq!(Credited::from_bytes(&bytes), None);
    }
}
