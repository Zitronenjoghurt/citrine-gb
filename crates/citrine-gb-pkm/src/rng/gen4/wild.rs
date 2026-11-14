use super::{Method, Spread, iv_words, ivs_from, method1_origins, pid_from};
use crate::rng::{Lcrng, nature_of};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lead {
    None,
    Synchronize(u8),
    CuteCharm { female: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeadOutcome {
    None,
    SyncPass,
    SyncFail,
    CuteCharmPass,
    CuteCharmFail,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Calls {
    pub before_slot: u8,
    pub level: bool,
    pub until_perfect_iv: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WildRoll {
    pub seed: u32,
    pub before_slot: [u16; 2],
    pub slot: u16,
    pub level: Option<u16>,
    pub lead: LeadOutcome,
    pub spread: Spread,
}

const LOOP_LIMIT: usize = 4096;
const ATTEMPTS: usize = 4;
const PERFECT_IV: u8 = 31;

impl Method {
    fn sync_passes(self, out: u16) -> bool {
        match self {
            Self::J => out >> 15 == 0,
            _ => out & 1 == 0,
        }
    }

    fn cute_charm_passes(self, out: u16) -> bool {
        match self {
            Self::J => out / 0x5556 != 0,
            _ => !out.is_multiple_of(3),
        }
    }
}

impl LeadOutcome {
    fn same_lead(self, other: Self) -> bool {
        use LeadOutcome::*;
        matches!(
            (self, other),
            (None, None)
                | (SyncPass | SyncFail, SyncPass | SyncFail)
                | (CuteCharmPass | CuteCharmFail, CuteCharmPass | CuteCharmFail)
        )
    }
}

fn has_perfect_iv(spread: Spread) -> bool {
    spread.ivs.to_array().contains(&PERFECT_IV)
}

pub fn cute_charm_pid(female: bool, gender_ratio: u8, nature: u8) -> u32 {
    let nature = u32::from(nature);
    if female {
        nature
    } else {
        25 * (u32::from(gender_ratio) / 25 + 1) + nature
    }
}

fn cute_charm_target(pid: u32, gender_ratio: u8) -> Option<bool> {
    if matches!(gender_ratio, 0 | 254 | 255) {
        return None;
    }
    let nature = nature_of(pid);
    [true, false]
        .into_iter()
        .find(|&female| cute_charm_pid(female, gender_ratio, nature) == pid)
}

fn attempt(method: Method, rng: &mut Lcrng, lead: Lead, gender_ratio: u8) -> (Spread, LeadOutcome) {
    let (target, outcome) = match lead {
        Lead::None => (method.nature(rng.next_u16()), LeadOutcome::None),
        Lead::Synchronize(nature) => {
            if method.sync_passes(rng.next_u16()) {
                (nature, LeadOutcome::SyncPass)
            } else {
                (method.nature(rng.next_u16()), LeadOutcome::SyncFail)
            }
        }
        Lead::CuteCharm { female } => {
            if method.cute_charm_passes(rng.next_u16()) {
                let nature = method.nature(rng.next_u16());
                let pid = cute_charm_pid(female, gender_ratio, nature);
                let ivs = ivs_from(rng.next_u16(), rng.next_u16());
                return (Spread { pid, ivs }, LeadOutcome::CuteCharmPass);
            }
            (method.nature(rng.next_u16()), LeadOutcome::CuteCharmFail)
        }
    };
    let pid = loop {
        let pid = pid_from(rng.next_u16(), rng.next_u16());
        if nature_of(pid) == target {
            break pid;
        }
    };
    let ivs = ivs_from(rng.next_u16(), rng.next_u16());
    (Spread { pid, ivs }, outcome)
}

pub fn wild(method: Method, seed: u32, calls: Calls, lead: Lead, gender_ratio: u8) -> WildRoll {
    let mut rng = Lcrng(seed);
    let mut before_slot = [0; 2];
    for out in before_slot.iter_mut().take(usize::from(calls.before_slot)) {
        *out = rng.next_u16();
    }
    let slot = rng.next_u16();
    let level = calls.level.then(|| rng.next_u16());
    let attempts = if calls.until_perfect_iv { ATTEMPTS } else { 1 };
    let mut made = attempt(method, &mut rng, lead, gender_ratio);
    for _ in 1..attempts {
        if has_perfect_iv(made.0) {
            break;
        }
        made = attempt(method, &mut rng, lead, gender_ratio);
    }
    WildRoll {
        seed,
        before_slot,
        slot,
        level,
        lead: made.1,
        spread: made.0,
    }
}

fn high(state: u32) -> u16 {
    (state >> 16) as u16
}

fn blocks_before_pid(method: Method, origin: u32, nature: u8, out: &mut Vec<(u32, LeadOutcome)>) {
    let mut cursor = origin;
    for _ in 0..LOOP_LIMIT {
        let before = Lcrng::back(cursor);
        if method.nature(high(cursor)) == nature {
            out.push((before, LeadOutcome::None));
            if !method.sync_passes(high(before)) {
                out.push((Lcrng::back(before), LeadOutcome::SyncFail));
            }
            if !method.cute_charm_passes(high(before)) {
                out.push((Lcrng::back(before), LeadOutcome::CuteCharmFail));
            }
        }
        if method.sync_passes(high(cursor)) {
            out.push((before, LeadOutcome::SyncPass));
        }
        let rejected = pid_from(high(before), high(cursor));
        if nature_of(rejected) == nature {
            break;
        }
        cursor = Lcrng::back(before);
    }
}

fn cute_charm_block(method: Method, before_ivs: u32, nature: Option<u8>) -> Option<u32> {
    let charm = Lcrng::back(before_ivs);
    let nature_ok = nature.is_none_or(|n| method.nature(high(before_ivs)) == n);
    (nature_ok && method.cute_charm_passes(high(charm))).then(|| Lcrng::back(charm))
}

fn last_attempts(method: Method, spread: Spread, gender_ratio: u8) -> Vec<(u32, LeadOutcome)> {
    let nature = nature_of(spread.pid);
    let mut found = Vec::new();
    if cute_charm_target(spread.pid, gender_ratio).is_some() {
        for before_ivs in iv_origins(spread) {
            if let Some(block) = cute_charm_block(method, before_ivs, Some(nature)) {
                found.push((block, LeadOutcome::CuteCharmPass));
            }
        }
    }
    for origin in method1_origins(spread) {
        blocks_before_pid(method, origin, nature, &mut found);
    }
    found
}

fn earlier_attempts(method: Method, end: u32) -> Vec<(u32, LeadOutcome, Spread)> {
    let second = high(end);
    let first_state = Lcrng::back(end);
    let ivs = ivs_from(high(first_state), second);
    let before_ivs = Lcrng::back(first_state);
    let low_state = Lcrng::back(before_ivs);
    let pid = pid_from(high(low_state), high(before_ivs));
    let spread = Spread { pid, ivs };
    let mut blocks = Vec::new();
    blocks_before_pid(method, Lcrng::back(low_state), nature_of(pid), &mut blocks);
    let mut found: Vec<(u32, LeadOutcome, Spread)> =
        blocks.into_iter().map(|(b, o)| (b, o, spread)).collect();
    if let Some(block) = cute_charm_block(method, before_ivs, None) {
        found.push((block, LeadOutcome::CuteCharmPass, Spread { pid: 0, ivs }));
    }
    found
}

pub fn wild_rolls(method: Method, spread: Spread, calls: Calls, gender_ratio: u8) -> Vec<WildRoll> {
    let mut found = Vec::new();
    for (block, outcome) in last_attempts(method, spread, gender_ratio) {
        if !calls.until_perfect_iv {
            found.push(prefix(block, calls, outcome, spread));
            continue;
        }
        let perfect = has_perfect_iv(spread);
        let mut frontier = vec![block];
        for count in 1..=ATTEMPTS {
            if perfect || count == ATTEMPTS {
                found.extend(frontier.iter().map(|&b| prefix(b, calls, outcome, spread)));
            }
            if count == ATTEMPTS {
                break;
            }
            frontier = frontier
                .iter()
                .flat_map(|&end| earlier_attempts(method, end))
                .filter(|(_, o, s)| o.same_lead(outcome) && !has_perfect_iv(*s))
                .map(|(b, _, _)| b)
                .collect();
        }
    }
    found
}

fn prefix(mut state: u32, calls: Calls, lead: LeadOutcome, spread: Spread) -> WildRoll {
    let mut take = || {
        let out = high(state);
        state = Lcrng::back(state);
        out
    };
    let level = calls.level.then(&mut take);
    let slot = take();
    let mut before_slot = [0; 2];
    for out in before_slot
        .iter_mut()
        .take(usize::from(calls.before_slot))
        .rev()
    {
        *out = take();
    }
    WildRoll {
        seed: state,
        before_slot,
        slot,
        level,
        lead,
        spread,
    }
}

fn iv_origins(spread: Spread) -> Vec<u32> {
    let (iv1, iv2) = iv_words(spread.ivs);
    (0..0x2_0000u32)
        .filter_map(|bits| {
            let first = (bits >> 16 << 15 | u32::from(iv1)) << 16 | (bits & 0xFFFF);
            let second = Lcrng::forward(first);
            ((second >> 16) as u16 & 0x7FFF == iv2).then(|| Lcrng::back(first))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trips(method: Method, seed: u32, calls: Calls, lead: Lead, ratio: u8) -> WildRoll {
        let roll = wild(method, seed, calls, lead, ratio);
        let back = wild_rolls(method, roll.spread, calls, ratio);
        assert!(
            back.contains(&roll),
            "{method:?} {lead:?} {calls:?} {roll:?}"
        );
        roll
    }

    #[test]
    fn every_lead_outcome_is_found_again() {
        let mut outcomes = std::collections::HashSet::new();
        for method in [Method::J, Method::K] {
            for seed in 0..40u32 {
                let seed = seed.wrapping_mul(0x9E37_79B9);
                for calls in [
                    Calls::default(),
                    Calls {
                        before_slot: 1,
                        level: true,
                        until_perfect_iv: false,
                    },
                    Calls {
                        before_slot: 2,
                        level: true,
                        until_perfect_iv: true,
                    },
                ] {
                    for lead in [
                        Lead::None,
                        Lead::Synchronize(seed as u8 % 25),
                        Lead::CuteCharm {
                            female: seed & 1 == 0,
                        },
                    ] {
                        outcomes.insert(round_trips(method, seed, calls, lead, 127).lead);
                    }
                }
            }
        }
        assert_eq!(outcomes.len(), 5);
    }

    #[test]
    fn rerolls_stop_at_a_perfect_iv_or_after_four_tries() {
        let calls = Calls {
            before_slot: 0,
            level: false,
            until_perfect_iv: true,
        };
        let mut rerolled = 0;
        for seed in 0..200u32 {
            let seed = seed.wrapping_mul(0x2545_F491);
            let once = wild(Method::K, seed, Calls::default(), Lead::None, 127);
            let roll = wild(Method::K, seed, calls, Lead::None, 127);
            if has_perfect_iv(once.spread) {
                assert_eq!(roll.spread, once.spread);
            } else {
                rerolled += 1;
                assert_ne!(roll.spread, once.spread);
            }
        }
        assert!(rerolled > 100);
    }

    #[test]
    fn synchronize_passes_on_its_nature() {
        let roll = (0..100u32)
            .map(|seed| {
                wild(
                    Method::J,
                    seed << 8,
                    Calls::default(),
                    Lead::Synchronize(3),
                    127,
                )
            })
            .find(|r| r.lead == LeadOutcome::SyncPass)
            .unwrap();
        assert_eq!(nature_of(roll.spread.pid), 3);
    }

    #[test]
    fn cute_charm_makes_small_pids_of_the_other_gender() {
        assert_eq!(cute_charm_pid(true, 127, 7), 7);
        assert_eq!(cute_charm_pid(false, 127, 7), 157);
        assert_eq!(cute_charm_pid(false, 191, 0), 200);
        assert_eq!(cute_charm_target(157, 127), Some(false));
        assert_eq!(cute_charm_target(157, 255), None);
    }
}
