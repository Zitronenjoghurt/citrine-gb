use crate::Threads;

impl Threads {
    pub fn both<A: Send, B>(
        &self,
        name: &'static str,
        side: impl FnOnce() -> A + Send,
        here: impl FnOnce() -> B,
    ) -> (A, B) {
        if let Some(pool) = &self.pool {
            let mut done = None;
            let here = pool.in_place_scope(|scope| {
                scope.spawn(|_| crate::gauge::timed_on_pool(name, || done = Some(side())));
                here()
            });
            return (done.expect("the side half ran"), here);
        }
        if self.next_rng().is_some_and(|mut rng| rng.below(2) == 0) {
            let here = here();
            (side(), here)
        } else {
            (side(), here())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Mode, Threads};
    use std::sync::atomic::{AtomicU8, Ordering};

    const MODES: [Mode; 4] = [
        Mode::Threads,
        Mode::Inline,
        Mode::Shuffled(1),
        Mode::Shuffled(2),
    ];

    #[test]
    fn both_halves_run_and_return_their_results_in_every_mode() {
        let data = [1u8, 2, 3];
        for mode in MODES {
            let threads = Threads::new(mode);
            assert_eq!(
                threads.both(
                    "test",
                    || data.iter().map(|&b| u32::from(b)).sum::<u32>(),
                    || data.len()
                ),
                (6, 3),
                "{mode:?}"
            );
        }
    }

    #[test]
    fn threads_run_the_side_half_on_the_pool() {
        let threads = Threads::new(Mode::Threads);
        let (side, _) = threads.both(
            "test",
            || std::thread::current().name().map(str::to_owned),
            || (),
        );
        assert!(side.is_some_and(|name| name.starts_with("pool ")));
    }

    #[test]
    fn inline_runs_the_side_half_first_and_shuffled_swaps_the_turn() {
        let turns = |mode| {
            let threads = Threads::new(mode);
            (0..16)
                .map(|_| {
                    let next = AtomicU8::new(0);
                    let turn = || next.fetch_add(1, Ordering::SeqCst);
                    threads.both("test", turn, turn)
                })
                .collect::<Vec<_>>()
        };
        assert!(turns(Mode::Inline).iter().all(|&turn| turn == (0, 1)));
        let shuffled = turns(Mode::Shuffled(3));
        assert!(shuffled.contains(&(0, 1)));
        assert!(shuffled.contains(&(1, 0)));
        assert_eq!(shuffled, turns(Mode::Shuffled(3)));
    }
}
