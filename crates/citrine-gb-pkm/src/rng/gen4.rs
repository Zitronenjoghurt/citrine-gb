mod wild;

use super::{Lcrng, nature_of};

use crate::stats::Stats;
pub use wild::{Calls, Lead, LeadOutcome, WildRoll, cute_charm_pid, wild, wild_rolls};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    One,
    J,
    K,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Spread {
    pub pid: u32,
    pub ivs: Stats<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Frame {
    pub seed: u32,
    pub slot_roll: Option<u8>,
    pub spread: Spread,
}

impl Method {
    fn slot_roll(self, out: u16) -> u8 {
        match self {
            Self::J => (out / 656) as u8,
            _ => (out % 100) as u8,
        }
    }

    fn nature(self, out: u16) -> u8 {
        match self {
            Self::J => (out / 0x0A3E) as u8,
            _ => (out % 25) as u8,
        }
    }
}

pub fn generate(method: Method, seed: u32) -> Frame {
    let mut rng = Lcrng(seed);
    let slot_roll = match method {
        Method::One => None,
        Method::J | Method::K => {
            let slot_roll = method.slot_roll(rng.next_u16());
            let nature = method.nature(rng.next_u16());
            loop {
                let pid = pid_from(rng.next_u16(), rng.next_u16());
                if nature_of(pid) == nature {
                    return Frame {
                        seed,
                        slot_roll: Some(slot_roll),
                        spread: Spread {
                            pid,
                            ivs: ivs_from(rng.next_u16(), rng.next_u16()),
                        },
                    };
                }
            }
        }
    };
    let pid = pid_from(rng.next_u16(), rng.next_u16());
    Frame {
        seed,
        slot_roll,
        spread: Spread {
            pid,
            ivs: ivs_from(rng.next_u16(), rng.next_u16()),
        },
    }
}

pub fn seeds(method: Method, spread: Spread) -> Vec<u32> {
    let origins = method1_origins(spread);
    match method {
        Method::One => origins,
        Method::J | Method::K => origins
            .into_iter()
            .flat_map(|origin| wild_origins(method, spread.pid, origin))
            .collect(),
    }
}

fn pid_from(low: u16, high: u16) -> u32 {
    u32::from(high) << 16 | u32::from(low)
}

fn ivs_from(first: u16, second: u16) -> Stats<u8> {
    Stats::unpack_ivs(u32::from(first & 0x7FFF) | u32::from(second & 0x7FFF) << 15)
}

fn iv_words(ivs: Stats<u8>) -> (u16, u16) {
    let packed = ivs.pack_ivs();
    ((packed & 0x7FFF) as u16, (packed >> 15 & 0x7FFF) as u16)
}

fn method1_origins(spread: Spread) -> Vec<u32> {
    let (iv1, iv2) = iv_words(spread.ivs);
    let low = spread.pid & 0xFFFF;
    let high = spread.pid >> 16;
    (0..=0xFFFFu32)
        .filter_map(|bits| {
            let first = low << 16 | bits;
            let second = Lcrng::forward(first);
            if second >> 16 != high {
                return None;
            }
            let third = Lcrng::forward(second);
            let fourth = Lcrng::forward(third);
            ((third >> 16) as u16 & 0x7FFF == iv1 && (fourth >> 16) as u16 & 0x7FFF == iv2)
                .then(|| Lcrng::back(first))
        })
        .collect()
}

fn wild_origins(method: Method, pid: u32, origin: u32) -> Vec<u32> {
    let nature = nature_of(pid);
    let mut found = Vec::new();
    let mut cursor = origin;
    for _ in 0..4096 {
        if method.nature((cursor >> 16) as u16) == nature {
            found.push(Lcrng::back(Lcrng::back(cursor)));
        }
        let low_state = Lcrng::back(cursor);
        let rejected = pid_from((low_state >> 16) as u16, (cursor >> 16) as u16);
        if nature_of(rejected) == nature {
            break;
        }
        cursor = Lcrng::back(low_state);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smogon_spread() -> Spread {
        Spread {
            pid: 0x7B06_01DB,
            ivs: Stats {
                hp: 19,
                atk: 17,
                def: 20,
                spe: 16,
                spa: 3,
                spd: 25,
            },
        }
    }

    #[test]
    fn method1_matches_smogons_worked_example() {
        let frame = generate(Method::One, 0x1A56_B091);
        assert_eq!(frame.spread, smogon_spread());
        assert_eq!(frame.slot_roll, None);
    }

    #[test]
    fn method1_finds_the_published_seed_from_pid_and_ivs() {
        assert!(seeds(Method::One, smogon_spread()).contains(&0x1A56_B091));
    }

    #[test]
    fn method1_finds_no_seed_for_unrelated_ivs() {
        let mut spread = smogon_spread();
        spread.ivs.hp = 18;
        assert!(seeds(Method::One, spread).is_empty());
    }

    #[test]
    fn wild_pids_match_the_nature_call() {
        for method in [Method::J, Method::K] {
            for seed in [0u32, 1, 0x469F_B838, 0x0000_000D, 0xDEAD_BEEF] {
                let frame = generate(method, seed);
                let mut rng = Lcrng(seed);
                rng.next_u16();
                assert_eq!(nature_of(frame.spread.pid), method.nature(rng.next_u16()));
                assert!(frame.slot_roll.unwrap() < 100);
            }
        }
    }

    #[test]
    fn wild_methods_find_their_own_seed_back() {
        for method in [Method::J, Method::K] {
            for seed in [0u32, 1, 0x469F_B838, 0x0000_000D, 0xDEAD_BEEF, 0x1234_5678] {
                let frame = generate(method, seed);
                assert!(
                    seeds(method, frame.spread).contains(&seed),
                    "{method:?} {seed:#x}"
                );
            }
        }
    }

    #[test]
    fn every_seed_found_generates_the_same_spread() {
        let frame = generate(Method::J, 0xC107_0A66);
        for seed in seeds(Method::J, frame.spread) {
            assert_eq!(generate(Method::J, seed).spread, frame.spread);
        }
    }
}
