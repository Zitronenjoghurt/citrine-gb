#![expect(
    clippy::disallowed_methods,
    reason = "citrine-gb-threads is the one place that starts threads"
)]

mod background;
mod both;
mod claim;
mod gauge;
mod latest;
mod lent;
mod rng;
mod rounds;
mod scope;
mod side;
mod spawn;
mod split;
mod worker;

pub use background::{Panicked, Pending};
pub use claim::Claim;
pub use gauge::{Kind, Reading, State, work};
pub use latest::{Publisher, Reader, latest};
pub use lent::{Lendable, Lent};
pub use rounds::Turnout;
pub use scope::Scope;
pub use side::Spawner;
pub use split::{Side, Split};
pub use worker::{Cx, Ended, Job, Placement, Replies, Status, Worker};

use rng::Rng;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

const MAX_POOL: usize = 7;
const SHUTDOWN_TICK: Duration = Duration::from_millis(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unthreaded {
    Chosen,
    NoThreadBuild,
    NotIsolated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Threads,
    Inline,
    Shuffled(u64),
}

pub struct Threads {
    mode: Mode,
    pool: Option<Arc<rayon::ThreadPool>>,
    started: Sender<Started>,
    registry: Receiver<Started>,
    running: RefCell<Vec<Started>>,
    seeds: Cell<u64>,
    side: side::SideJobs,
    gauges: gauge::Gauges,
}

pub(crate) struct Started {
    name: String,
    stop: Box<dyn Fn() + Send>,
    thread: spawn::Handle,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Shutdown {
    pub unfinished: Vec<String>,
}

impl Default for Threads {
    fn default() -> Self {
        Self::new(Mode::Inline)
    }
}

impl Threads {
    pub fn new(mode: Mode) -> Self {
        let mode = match mode {
            Mode::Threads if !spawn::supported() => Mode::Inline,
            mode => mode,
        };
        let gauges = gauge::Gauges::new();
        let board = gauges.board().clone();
        let pool = (mode == Mode::Threads).then(|| {
            let threads = pool_threads(spawn::cores(), fast_cores());
            Arc::new(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .spawn_handler(|thread| {
                        let name = format!("pool {}", thread.index());
                        let gauge = board.gauge(name.clone(), Kind::Pool);
                        gauge.set(State::Parked);
                        spawn::thread(
                            name,
                            move || {
                                gauge::wear(&gauge, || thread.run());
                                gauge.set(State::Ended);
                            },
                            |_| {},
                        );
                        Ok(())
                    })
                    .build()
                    .expect("the pool starts"),
            )
        });
        let (started, registry) = mpsc::channel();
        Self {
            mode,
            pool,
            started,
            registry,
            running: RefCell::new(Vec::new()),
            seeds: Cell::new(0),
            side: side::SideJobs::new(mode, gauges.board().clone()),
            gauges,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn unthreaded(&self) -> Option<Unthreaded> {
        (self.mode != Mode::Threads).then(|| spawn::missing().unwrap_or(Unthreaded::Chosen))
    }

    pub fn cores(&self) -> usize {
        spawn::cores()
    }

    pub fn watch(&self, on: bool) {
        self.gauges.watch(on);
    }

    pub fn watching(&self) -> bool {
        self.gauges.watching()
    }

    pub fn readings(&self) -> Vec<Reading> {
        self.gauges.readings()
    }

    pub fn pool_size(&self) -> usize {
        self.pool
            .as_ref()
            .map_or(1, |pool| pool.current_num_threads())
    }

    pub fn start<J: Job>(&self, name: impl Into<String>, job: J) -> Worker<J> {
        self.prune();
        Worker::start(self.context(), name.into(), job)
    }

    pub fn background<T: Send + 'static>(
        &self,
        name: &'static str,
        task: impl FnOnce() -> T + Send + 'static,
    ) -> Pending<T> {
        Pending::start(self.mode, self.next_rng(), self.gauges.board(), name, task)
    }

    pub fn spawner(&self) -> Spawner {
        self.side.spawner()
    }

    pub fn run_side_jobs(&self) -> Vec<Panicked> {
        self.prune();
        self.side.run()
    }

    pub fn shutdown(self, deadline: Duration) -> Shutdown {
        self.prune();
        let started = self.running.take();
        for worker in &started {
            (worker.stop)();
        }
        let mut left = if cfg!(target_arch = "wasm32") {
            Duration::ZERO
        } else {
            deadline
        };
        let mut unfinished = Vec::new();
        for worker in started {
            while !worker.thread.is_finished() && !left.is_zero() {
                std::thread::sleep(SHUTDOWN_TICK);
                left = left.saturating_sub(SHUTDOWN_TICK);
            }
            if worker.thread.is_finished() {
                worker.thread.join();
            } else {
                unfinished.push(worker.name);
            }
        }
        Shutdown { unfinished }
    }

    fn prune(&self) {
        let mut running = self.running.borrow_mut();
        running.extend(self.registry.try_iter());
        running.retain(|worker| !worker.thread.is_finished());
    }

    #[cfg(test)]
    fn registered(&self) -> usize {
        self.prune();
        self.running.borrow().len()
    }

    fn next_rng(&self) -> Option<Rng> {
        let Mode::Shuffled(seed) = self.mode else {
            return None;
        };
        let n = self.seeds.get();
        self.seeds.set(n + 1);
        Some(Rng::new(seed ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15)))
    }

    fn context(&self) -> worker::Context {
        worker::Context {
            mode: self.mode,
            pool: self.pool.clone(),
            rng: self.next_rng(),
            started: self.started.clone(),
            board: self.gauges.board().clone(),
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn spawn_local(future: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(future);
}

fn side_threads(mode: Mode) -> bool {
    mode == Mode::Threads && cfg!(not(target_arch = "wasm32"))
}

fn pool_threads(cores: usize, fast: Option<usize>) -> usize {
    fast.unwrap_or(cores)
        .min(cores)
        .saturating_sub(1)
        .clamp(1, MAX_POOL)
}

#[cfg(target_os = "android")]
fn fast_cores() -> Option<usize> {
    let tops: Vec<u64> = (0..64)
        .map_while(|cpu| {
            std::fs::read_to_string(format!(
                "/sys/devices/system/cpu/cpu{cpu}/cpufreq/cpuinfo_max_freq"
            ))
            .ok()
        })
        .filter_map(|text| text.trim().parse().ok())
        .collect();
    fast_count(&tops)
}

#[cfg(not(target_os = "android"))]
fn fast_cores() -> Option<usize> {
    None
}

#[cfg(any(target_os = "android", test))]
fn fast_count(tops: &[u64]) -> Option<usize> {
    let fastest = tops.iter().copied().max().filter(|top| *top > 0)?;
    Some(tops.iter().filter(|top| **top * 10 >= fastest * 7).count())
}

pub(crate) fn guarded<T>(name: &str, f: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|payload| {
        let text = payload
            .downcast_ref::<&str>()
            .map(|text| text.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panicked".to_string());
        format!("{name}: {text}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    struct Slow;

    impl Job for Slow {
        type Msg = Duration;
        type Reply = &'static str;

        fn handle(&mut self, wait: Duration, _: &mut Cx<'_, &'static str>) {
            std::thread::sleep(wait);
        }

        fn stop(&mut self, cx: &mut Cx<'_, &'static str>) {
            cx.reply("flushed");
        }
    }

    #[test]
    fn shutdown_stops_every_worker_and_names_the_ones_past_the_deadline() {
        let threads = Threads::new(Mode::Threads);
        let mut saves = threads.start("saves", Slow);
        let stuck = threads.start("console: stuck", Slow);
        stuck.send(Duration::from_secs(3));
        saves.send(Duration::from_millis(1));
        let report = threads.shutdown(Duration::from_millis(300));
        assert_eq!(report.unfinished, ["console: stuck"]);
        assert_eq!(saves.replies().collect::<Vec<_>>(), ["flushed"]);
        assert_eq!(saves.status(), Status::Ended(Ended::Stopped));
    }

    #[test]
    fn finished_workers_leave_the_registry_while_running_ones_stay() {
        let threads = Threads::new(Mode::Threads);
        let saves = threads.start("saves", Slow);
        for _ in 0..50 {
            let mut console = threads.start("console", Slow);
            console.move_to(Placement::Inline);
            console.move_to(Placement::Thread);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while threads.registered() > 1 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(threads.registered(), 1);
        drop(saves);
        assert_eq!(
            threads.shutdown(Duration::from_secs(5)),
            Shutdown::default()
        );
    }

    #[test]
    fn inline_modes_start_no_thread_and_shut_down_at_once() {
        for mode in [Mode::Inline, Mode::Shuffled(9)] {
            let threads = Threads::new(mode);
            let worker = threads.start("console", Slow);
            assert_eq!(worker.placement(), Placement::Inline);
            assert_eq!(threads.pool_size(), 1);
            assert_eq!(threads.shutdown(Duration::ZERO), Shutdown::default());
        }
    }

    #[test]
    fn the_pool_leaves_a_core_for_the_ui_and_has_at_most_seven_threads() {
        let threads = Threads::new(Mode::Threads);
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        assert_eq!(threads.pool_size(), (cores - 1).clamp(1, MAX_POOL));
    }

    #[test]
    fn a_phone_pool_counts_only_the_fast_cores() {
        let phone = [
            1_800_000, 1_800_000, 1_800_000, 1_800_000, 2_400_000, 2_400_000, 2_400_000, 3_000_000,
        ];
        assert_eq!(fast_count(&phone), Some(4));
        assert_eq!(pool_threads(phone.len(), fast_count(&phone)), 3);
        assert_eq!(fast_count(&[2_000_000; 4]), Some(4));
        assert_eq!(fast_count(&[]), None);
        assert_eq!(fast_count(&[0, 0]), None);
    }

    #[test]
    fn the_pool_has_one_thread_on_two_slow_cores_and_seven_on_many() {
        assert_eq!(pool_threads(2, None), 1);
        assert_eq!(pool_threads(1, None), 1);
        assert_eq!(pool_threads(8, Some(1)), 1);
        assert_eq!(pool_threads(32, None), MAX_POOL);
        assert_eq!(pool_threads(4, Some(9)), 3);
    }
}
