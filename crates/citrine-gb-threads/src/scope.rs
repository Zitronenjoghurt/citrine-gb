use crate::Rng;
use std::cell::RefCell;

type Task<'s> = Box<dyn FnOnce() + Send + 's>;

pub struct Scope<'s> {
    tasks: RefCell<Vec<Task<'s>>>,
}

impl<'s> Scope<'s> {
    pub fn spawn(&self, task: impl FnOnce() + Send + 's) {
        self.tasks.borrow_mut().push(Box::new(task));
    }
}

pub(crate) fn run<'s>(
    name: &'static str,
    pool: Option<&rayon::ThreadPool>,
    rng: Option<&mut Rng>,
    f: impl FnOnce(&Scope<'s>),
) {
    let scope = Scope {
        tasks: RefCell::new(Vec::new()),
    };
    f(&scope);
    let mut tasks = scope.tasks.into_inner();
    match pool {
        Some(pool) if tasks.len() > 1 => {
            let here = tasks.pop();
            pool.in_place_scope(|scope| {
                for task in tasks {
                    scope.spawn(move |_| crate::gauge::timed_on_pool(name, task));
                }
                if let Some(task) = here {
                    task();
                }
            });
        }
        _ => {
            if let Some(rng) = rng {
                rng.shuffle(&mut tasks);
            }
            for task in tasks {
                task();
            }
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_types,
    reason = "the tests gather names from the pool threads"
)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn pool() -> rayon::ThreadPool {
        rayon::ThreadPoolBuilder::new()
            .num_threads(3)
            .thread_name(|i| format!("pool {i}"))
            .build()
            .unwrap()
    }

    fn squares(pool: Option<&rayon::ThreadPool>, rng: Option<&mut Rng>) -> Vec<u64> {
        let mut values: Vec<u64> = (0..64).collect();
        run("test", pool, rng, |scope| {
            for value in values.iter_mut() {
                scope.spawn(move || *value *= *value);
            }
        });
        values
    }

    fn order(rng: Option<&mut Rng>) -> Vec<usize> {
        let seen = Mutex::new(Vec::new());
        run("test", None, rng, |scope| {
            for i in 0..12 {
                let seen = &seen;
                scope.spawn(move || seen.lock().unwrap().push(i));
            }
        });
        seen.into_inner().unwrap()
    }

    #[test]
    fn every_placement_gives_the_same_result() {
        let expected: Vec<u64> = (0..64).map(|v| v * v).collect();
        assert_eq!(squares(None, None), expected);
        assert_eq!(squares(Some(&pool()), None), expected);
        assert_eq!(squares(None, Some(&mut Rng::new(3))), expected);
    }

    #[test]
    fn inline_runs_tasks_in_spawn_order_and_shuffled_in_a_seeded_order() {
        assert_eq!(order(None), (0..12).collect::<Vec<_>>());
        let shuffled = order(Some(&mut Rng::new(5)));
        assert_ne!(shuffled, (0..12).collect::<Vec<_>>());
        assert_eq!(shuffled, order(Some(&mut Rng::new(5))));
    }

    #[test]
    fn the_caller_runs_the_last_task_and_the_pool_runs_the_rest() {
        let names = Mutex::new(Vec::new());
        run("test", Some(&pool()), None, |scope| {
            for i in 0..8 {
                let names = &names;
                scope.spawn(move || {
                    let name = std::thread::current().name().map(str::to_owned);
                    names.lock().unwrap().push((i, name));
                });
            }
        });
        let names = names.into_inner().unwrap();
        assert_eq!(names.len(), 8);
        let pooled =
            |name: &Option<String>| name.as_deref().is_some_and(|n| n.starts_with("pool "));
        assert!(
            names.iter().all(|(i, name)| pooled(name) == (*i != 7)),
            "{names:?}"
        );
    }

    #[test]
    fn a_lone_task_runs_on_the_caller() {
        let mut name = None;
        run("test", Some(&pool()), None, |scope| {
            scope.spawn(|| name = std::thread::current().name().map(str::to_owned));
        });
        assert_eq!(name, std::thread::current().name().map(str::to_owned));
    }

    #[test]
    fn a_panicking_task_reaches_the_caller_after_the_others_ran() {
        let mut values = [0u8; 4];
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run("test", Some(&pool()), None, |scope| {
                for (i, value) in values.iter_mut().enumerate() {
                    scope.spawn(move || {
                        assert_ne!(i, 2, "task two fails");
                        *value = 1;
                    });
                }
            })
        }));
        assert!(caught.is_err());
        assert_eq!(values, [1, 1, 0, 1]);
    }
}
