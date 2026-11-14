use crate::{Rng, Scope, Turnout, rounds, scope};

pub type Side<'s> = Box<dyn FnOnce() + Send + 's>;

pub struct Split<'a> {
    pool: Option<&'a rayon::ThreadPool>,
    rng: Option<&'a mut Rng>,
    starved: bool,
}

impl<'a> Split<'a> {
    pub fn inline() -> Self {
        Self {
            pool: None,
            rng: None,
            starved: false,
        }
    }

    pub(crate) fn new(pool: Option<&'a rayon::ThreadPool>, rng: Option<&'a mut Rng>) -> Self {
        Self {
            pool,
            rng,
            starved: false,
        }
    }

    #[cfg(feature = "testing")]
    pub fn starved() -> Self {
        Self {
            starved: true,
            ..Self::inline()
        }
    }

    pub fn lanes(&self) -> usize {
        self.pool.map_or(1, rayon::ThreadPool::current_num_threads)
    }

    pub fn has_pool(&self) -> bool {
        self.pool.is_some()
    }

    pub fn beside<R>(
        &mut self,
        sides: Vec<(&'static str, Side<'_>)>,
        here: impl FnOnce() -> R,
    ) -> R {
        self.beside_with(sides, |_| here())
    }

    pub fn beside_with<R>(
        &mut self,
        sides: Vec<(&'static str, Side<'_>)>,
        here: impl FnOnce(&mut Split<'_>) -> R,
    ) -> R {
        match self.pool {
            Some(pool) => pool.in_place_scope(|scope| {
                for (name, side) in sides {
                    scope.spawn(move |_| crate::gauge::timed_on_pool(name, side));
                }
                here(&mut Split {
                    pool: Some(pool),
                    rng: self.rng.as_deref_mut(),
                    starved: self.starved,
                })
            }),
            None => {
                let out = here(self);
                for (_, side) in sides {
                    side();
                }
                out
            }
        }
    }

    pub fn scope<'s>(&mut self, name: &'static str, f: impl FnOnce(&Scope<'s>)) {
        scope::run(name, self.pool, self.rng.as_deref_mut(), f);
    }

    pub fn rounds<T: Send>(
        &mut self,
        name: &'static str,
        items: &mut [T],
        work: impl Fn(&mut T) + Sync,
        between: impl FnMut(&mut [&mut T]) -> bool + Send,
    ) -> Turnout {
        rounds::run(
            name,
            self.pool,
            self.rng.as_deref_mut(),
            self.starved,
            items,
            work,
            between,
        )
    }
}

#[cfg(feature = "testing")]
impl crate::Mode {
    pub fn from_env() -> Self {
        let mode = std::env::var("CITRINE_THREADS").unwrap_or_default();
        match mode.split_once(':') {
            _ if mode == "threads" => Self::Threads,
            Some(("shuffled", seed)) => Self::Shuffled(seed.parse().expect("a shuffle seed")),
            _ => Self::Inline,
        }
    }
}

#[cfg(feature = "testing")]
impl crate::Threads {
    pub fn split<T>(&self, f: impl FnOnce(&mut Split<'_>) -> T) -> T {
        let mut rng = self.next_rng();
        f(&mut Split::new(self.pool.as_deref(), rng.as_mut()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cx, Job, Mode, Threads};
    use std::sync::atomic::Ordering;

    struct Lanes;

    impl Job for Lanes {
        type Msg = ();
        type Reply = usize;

        fn handle(&mut self, _: (), cx: &mut Cx<'_, usize>) {
            let lanes = cx.split().lanes();
            cx.reply(lanes);
        }
    }

    fn lanes_on(threads: &Threads) -> usize {
        let mut worker = threads.start("lanes", Lanes);
        worker.send(());
        loop {
            if let Some(lanes) = worker.replies().next() {
                return lanes;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_split_has_one_lane_per_pool_thread_and_one_without_a_pool() {
        assert_eq!(Split::inline().lanes(), 1);
        let threads = Threads::new(Mode::Threads);
        assert_eq!(lanes_on(&threads), threads.pool_size());
        assert_eq!(lanes_on(&Threads::new(Mode::Inline)), 1);
        assert_eq!(lanes_on(&Threads::new(Mode::Shuffled(5))), 1);
    }

    #[test]
    fn beside_runs_the_side_task_on_the_pool_while_the_caller_waits_for_it() {
        let threads = Threads::new(Mode::Threads);
        let (tx, rx) = std::sync::mpsc::channel();
        let mut split = Split::new(threads.pool.as_deref(), None);
        assert!(split.has_pool());
        let name = split.beside(
            vec![(
                "test",
                Box::new(move || {
                    tx.send(std::thread::current().name().map(str::to_owned))
                        .unwrap()
                }),
            )],
            || rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap(),
        );
        assert!(name.is_some_and(|name| name.starts_with("pool ")));
    }

    #[test]
    fn beside_without_a_pool_runs_the_caller_first_and_the_side_task_after() {
        let ran = std::sync::atomic::AtomicBool::new(false);
        let mut split = Split::inline();
        assert!(!split.has_pool());
        let side_saw = std::sync::atomic::AtomicBool::new(false);
        split.beside(
            vec![(
                "test",
                Box::new(|| side_saw.store(ran.load(Ordering::SeqCst), Ordering::SeqCst)),
            )],
            || ran.store(true, Ordering::SeqCst),
        );
        assert!(side_saw.load(Ordering::SeqCst));
    }

    #[test]
    fn side_tasks_beside_run_at_the_same_time_when_the_pool_has_room() {
        let threads = Threads::new(Mode::Threads);
        if threads.pool_size() < 2 {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (back, done) = std::sync::mpsc::channel();
        let mut split = Split::new(threads.pool.as_deref(), None);
        let sides: Vec<(&str, Side<'_>)> = vec![
            (
                "test",
                Box::new(move || {
                    tx.send(()).unwrap();
                }),
            ),
            (
                "test",
                Box::new(move || {
                    let seen = rx.recv_timeout(std::time::Duration::from_secs(10)).is_ok();
                    back.send(seen).unwrap();
                }),
            ),
        ];
        let seen = split.beside(sides, || {
            done.recv_timeout(std::time::Duration::from_secs(10))
                .unwrap()
        });
        assert!(seen);
    }

    #[test]
    fn beside_with_hands_the_caller_a_split_on_the_same_pool() {
        let threads = Threads::new(Mode::Threads);
        let (tx, rx) = std::sync::mpsc::channel();
        let mut split = Split::new(threads.pool.as_deref(), None);
        let lanes = split.beside_with(
            vec![("test", Box::new(move || tx.send(()).unwrap()))],
            |inner| {
                rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
                (inner.has_pool(), inner.lanes())
            },
        );
        assert_eq!(lanes, (true, threads.pool_size()));
    }

    #[test]
    fn beside_with_and_no_pool_runs_the_caller_inline_first() {
        let ran = std::sync::atomic::AtomicBool::new(false);
        let side_saw = std::sync::atomic::AtomicBool::new(false);
        let mut split = Split::inline();
        let lanes = split.beside_with(
            vec![(
                "test",
                Box::new(|| side_saw.store(ran.load(Ordering::SeqCst), Ordering::SeqCst)),
            )],
            |inner| {
                ran.store(true, Ordering::SeqCst);
                inner.lanes()
            },
        );
        assert_eq!(lanes, 1);
        assert!(side_saw.load(Ordering::SeqCst));
    }
}
