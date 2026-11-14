#![expect(
    clippy::disallowed_types,
    reason = "rounds hands items between pool threads that take turns, never at once"
)]

use crate::Rng;
use crate::gauge::{self, Kind, State};
use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const SPINS: u32 = 64;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Turnout {
    pub rounds: u64,
    pub joined: u64,
    pub lanes: u64,
}

impl Turnout {
    pub fn add(&mut self, other: Self) {
        self.rounds += other.rounds;
        self.joined += other.joined;
        self.lanes = self.lanes.max(other.lanes);
    }

    pub fn missing(&self) -> u64 {
        (self.rounds * self.lanes).saturating_sub(self.joined)
    }
}

pub(crate) fn run<T: Send>(
    name: &'static str,
    pool: Option<&rayon::ThreadPool>,
    rng: Option<&mut Rng>,
    starved: bool,
    items: &mut [T],
    work: impl Fn(&mut T) + Sync,
    between: impl FnMut(&mut [&mut T]) -> bool + Send,
) -> Turnout {
    let turnout = split_up(name, pool, rng, starved, items, work, between);
    gauge::with_current(|gauge| gauge.add_turnout(turnout));
    turnout
}

fn split_up<T: Send>(
    name: &'static str,
    pool: Option<&rayon::ThreadPool>,
    rng: Option<&mut Rng>,
    starved: bool,
    items: &mut [T],
    work: impl Fn(&mut T) + Sync,
    between: impl FnMut(&mut [&mut T]) -> bool + Send,
) -> Turnout {
    match pool {
        Some(pool) if !starved && items.len() > 1 && pool.current_num_threads() > 1 => {
            on_pool(name, pool, items, work, between)
        }
        _ => {
            let rounds = one_by_one(rng, items, work, between);
            let lanes = if starved { items.len() as u64 } else { 1 };
            Turnout {
                rounds,
                joined: rounds,
                lanes,
            }
        }
    }
}

fn one_by_one<T>(
    mut rng: Option<&mut Rng>,
    items: &mut [T],
    work: impl Fn(&mut T),
    mut between: impl FnMut(&mut [&mut T]) -> bool,
) -> u64 {
    let mut order: Vec<usize> = (0..items.len()).collect();
    let mut rounds = 0;
    loop {
        if let Some(rng) = rng.as_deref_mut() {
            rng.shuffle(&mut order);
        }
        for &i in &order {
            work(&mut items[i]);
        }
        rounds += 1;
        let mut all: Vec<&mut T> = items.iter_mut().collect();
        if !between(&mut all) {
            return rounds;
        }
    }
}

struct Shared<'a, T, W, B> {
    name: &'static str,
    items: Vec<Mutex<&'a mut T>>,
    work: W,
    between: Mutex<B>,
    next: AtomicUsize,
    finished: AtomicUsize,
    joined: AtomicUsize,
    turnout: AtomicUsize,
    epoch: AtomicUsize,
    round: AtomicUsize,
    stop: AtomicBool,
    panic: Mutex<Option<Box<dyn Any + Send>>>,
}

impl<T: Send, W: Fn(&mut T) + Sync, B: FnMut(&mut [&mut T]) -> bool + Send> Shared<'_, T, W, B> {
    fn lane(&self) {
        let gauge = gauge::current();
        let pool = gauge.as_ref().filter(|gauge| gauge.kind() == Kind::Pool);
        let after = if pool.is_some() {
            State::Parked
        } else {
            State::Working
        };
        while !self.stop.load(Ordering::Acquire) {
            let round = self.round.load(Ordering::Acquire);
            let start = gauge.as_ref().and_then(|gauge| gauge.clock());
            if let (Some(pool), Some(_)) = (pool, start) {
                pool.set(State::Working);
            }
            self.take_turns();
            let spin = gauge.as_ref().zip(start).and_then(|(gauge, _)| {
                if let Some(pool) = pool {
                    pool.worked_on(self.name, start);
                }
                gauge.set(State::Spinning);
                gauge.clock()
            });
            self.wait_past(round);
            if let Some(gauge) = gauge.as_ref().filter(|_| spin.is_some()) {
                gauge.spun(spin);
                gauge.set(after);
            }
        }
    }

    fn take_turns(&self) {
        let count = self.items.len();
        let mut counted = None;
        loop {
            let i = self.next.fetch_add(1, Ordering::AcqRel);
            if i >= count {
                return;
            }
            let epoch = self.epoch.load(Ordering::Acquire);
            if counted != Some(epoch) {
                counted = Some(epoch);
                self.joined.fetch_add(1, Ordering::AcqRel);
            }
            let ran = catch_unwind(AssertUnwindSafe(|| {
                let mut item = self.items[i].lock().unwrap_or_else(|e| e.into_inner());
                (self.work)(&mut item);
            }));
            if let Err(payload) = ran {
                self.fail(payload);
                return;
            }
            if self.finished.fetch_add(1, Ordering::AcqRel) + 1 == count {
                self.close_round();
            }
        }
    }

    fn close_round(&self) {
        let joined = self.joined.swap(0, Ordering::AcqRel);
        self.turnout.fetch_add(joined, Ordering::AcqRel);
        self.epoch.fetch_add(1, Ordering::AcqRel);
        let ran = catch_unwind(AssertUnwindSafe(|| {
            let mut guards: Vec<_> = self
                .items
                .iter()
                .map(|item| item.lock().unwrap_or_else(|e| e.into_inner()))
                .collect();
            let mut all: Vec<&mut T> = guards.iter_mut().map(|guard| &mut ***guard).collect();
            let mut between = self.between.lock().unwrap_or_else(|e| e.into_inner());
            between(&mut all)
        }));
        match ran {
            Ok(true) => {
                self.finished.store(0, Ordering::Release);
                self.next.store(0, Ordering::Release);
            }
            Ok(false) => self.stop.store(true, Ordering::Release),
            Err(payload) => self.fail(payload),
        }
        self.round.fetch_add(1, Ordering::AcqRel);
    }

    fn fail(&self, payload: Box<dyn Any + Send>) {
        self.panic
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert(payload);
        self.stop.store(true, Ordering::Release);
    }

    fn wait_past(&self, round: usize) {
        let mut spins = 0;
        while self.round.load(Ordering::Acquire) == round && !self.stop.load(Ordering::Acquire) {
            if spins < SPINS {
                spins += 1;
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
    }
}

fn on_pool<T: Send>(
    name: &'static str,
    pool: &rayon::ThreadPool,
    items: &mut [T],
    work: impl Fn(&mut T) + Sync,
    between: impl FnMut(&mut [&mut T]) -> bool + Send,
) -> Turnout {
    let lanes = items.len().min(pool.current_num_threads() + 1);
    let shared = Shared {
        name,
        items: items.iter_mut().map(Mutex::new).collect(),
        work,
        between: Mutex::new(between),
        next: AtomicUsize::new(0),
        finished: AtomicUsize::new(0),
        joined: AtomicUsize::new(0),
        turnout: AtomicUsize::new(0),
        epoch: AtomicUsize::new(0),
        round: AtomicUsize::new(0),
        stop: AtomicBool::new(false),
        panic: Mutex::new(None),
    };
    pool.in_place_scope(|scope| {
        for _ in 1..lanes {
            scope.spawn(|_| shared.lane());
        }
        shared.lane();
    });
    let panic = shared.panic.into_inner().unwrap_or_else(|e| e.into_inner());
    if let Some(payload) = panic {
        resume_unwind(payload);
    }
    Turnout {
        rounds: shared.round.into_inner() as u64,
        joined: shared.turnout.into_inner() as u64,
        lanes: lanes as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(threads: usize) -> rayon::ThreadPool {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
    }

    fn relay(pool: Option<&rayon::ThreadPool>, rng: Option<&mut Rng>) -> Vec<Vec<u64>> {
        let mut items: Vec<(u64, Vec<u64>)> = (0..4).map(|i| (i, Vec::new())).collect();
        let mut rounds = 0;
        run(
            "test",
            pool,
            rng,
            false,
            &mut items,
            |(value, seen)| {
                *value = value.wrapping_mul(6364136223846793005).wrapping_add(1);
                seen.push(*value);
            },
            |all| {
                let sum = all.iter().fold(0u64, |sum, item| sum.wrapping_add(item.0));
                for item in all.iter_mut() {
                    item.0 ^= sum;
                }
                rounds += 1;
                rounds < 500
            },
        );
        items.into_iter().map(|(_, seen)| seen).collect()
    }

    #[test]
    fn rounds_give_the_same_result_inline_shuffled_and_on_pools_of_any_size() {
        let inline = relay(None, None);
        assert_eq!(inline[0].len(), 500);
        assert_eq!(relay(None, Some(&mut Rng::new(77))), inline);
        for threads in [1, 2, 3, 7] {
            assert_eq!(
                relay(Some(&pool(threads)), None),
                inline,
                "{threads} threads"
            );
        }
    }

    #[test]
    fn a_busy_pool_still_finishes_the_rounds_on_the_caller() {
        let pool = pool(2);
        let (stuck, release) = std::sync::mpsc::channel::<()>();
        pool.spawn(move || {
            let _ = release.recv();
        });
        let mut count = [0u32; 3];
        let turnout = run(
            "test",
            Some(&pool),
            None,
            false,
            &mut count,
            |n| *n += 1,
            |all| *all[0] < 100,
        );
        drop(stuck);
        assert_eq!(count, [100; 3]);
        assert_eq!(turnout.rounds, 100);
        assert_eq!(turnout.lanes, 3);
        assert!(turnout.missing() >= 100, "{turnout:?}");
    }

    #[test]
    fn one_by_one_rounds_have_one_lane_and_none_missing() {
        let mut count = [0u32; 4];
        let turnout = run(
            "test",
            None,
            None,
            false,
            &mut count,
            |n| *n += 1,
            |all| *all[0] < 30,
        );
        assert_eq!(
            turnout,
            Turnout {
                rounds: 30,
                joined: 30,
                lanes: 1
            }
        );
        assert_eq!(turnout.missing(), 0);
    }

    #[test]
    fn a_starved_run_misses_every_lane_but_one_and_still_gives_the_same_result() {
        let mut count = [0u32; 4];
        let turnout = run(
            "test",
            Some(&pool(3)),
            None,
            true,
            &mut count,
            |n| *n += 1,
            |all| *all[0] < 30,
        );
        assert_eq!(count, [30; 4]);
        assert_eq!(turnout.lanes, 4);
        assert_eq!(turnout.missing(), 90);
    }

    #[test]
    fn turnouts_add_up_their_rounds_and_keep_the_widest_lanes() {
        let mut total = Turnout::default();
        total.add(Turnout {
            rounds: 10,
            joined: 25,
            lanes: 4,
        });
        total.add(Turnout {
            rounds: 5,
            joined: 5,
            lanes: 1,
        });
        assert_eq!(total.rounds, 15);
        assert_eq!(total.joined, 30);
        assert_eq!(total.missing(), 30);
    }

    #[test]
    fn a_panicking_item_reaches_the_caller_and_stops_every_lane() {
        let pool = pool(3);
        let mut items = [0u32; 4];
        let caught = catch_unwind(AssertUnwindSafe(|| {
            run(
                "test",
                Some(&pool),
                None,
                false,
                &mut items,
                |n| {
                    *n += 1;
                    assert!(*n < 10, "item ran too often");
                },
                |_| true,
            );
        }));
        assert!(caught.is_err());
    }
}
