use crate::Turnout;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{
    AtomicBool, AtomicU8, AtomicU64,
    Ordering::{Acquire, Relaxed, Release},
};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use web_time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Worker,
    Pool,
    Background,
    Side,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Starting,
    Working,
    Parked,
    Spinning,
    Ended,
}

impl State {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Working,
            2 => Self::Parked,
            3 => Self::Spinning,
            4 => Self::Ended,
            _ => Self::Starting,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub id: u64,
    pub tasks: Vec<(&'static str, Duration)>,
    pub name: String,
    pub kind: Kind,
    pub state: State,
    pub inline: bool,
    pub sent: u64,
    pub handled: u64,
    pub parks: u64,
    pub busy: Duration,
    pub waited: Duration,
    pub waits: u64,
    pub spun: Duration,
    pub rounds: u64,
    pub missed: u64,
}

impl Reading {
    pub fn queued(&self) -> u64 {
        self.sent.saturating_sub(self.handled)
    }
}

pub(crate) struct Gauge {
    id: u64,
    name: String,
    kind: Kind,
    watching: Arc<AtomicBool>,
    #[cfg(test)]
    clock_reads: Arc<AtomicU64>,
    state: AtomicU8,
    inline: AtomicBool,
    sent: AtomicU64,
    handled: AtomicU64,
    parks: AtomicU64,
    busy_ns: AtomicU64,
    waited_ns: AtomicU64,
    waits: AtomicU64,
    spun_ns: AtomicU64,
    rounds: AtomicU64,
    missed: AtomicU64,
    pieces: AtomicBool,
    tasks: [TaskSlot; TASK_SLOTS],
}

const TASK_SLOTS: usize = 8;

#[derive(Default)]
struct TaskSlot {
    name: OnceLock<&'static str>,
    busy_ns: AtomicU64,
}

impl Gauge {
    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }

    pub(crate) fn clock(&self) -> Option<Instant> {
        if !self.watching.load(Relaxed) {
            return None;
        }
        #[cfg(test)]
        self.clock_reads.fetch_add(1, Relaxed);
        Some(Instant::now())
    }

    fn since(&self, start: Instant) -> u64 {
        #[cfg(test)]
        self.clock_reads.fetch_add(1, Relaxed);
        u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    pub(crate) fn set(&self, state: State) {
        self.state.store(state as u8, Relaxed);
    }

    pub(crate) fn set_inline(&self, inline: bool) {
        self.inline.store(inline, Relaxed);
    }

    pub(crate) fn sent(&self) {
        self.sent.fetch_add(1, Relaxed);
    }

    pub(crate) fn parked(&self) {
        self.parks.fetch_add(1, Relaxed);
        self.set(State::Parked);
    }

    pub(crate) fn handled(&self, start: Option<Instant>, queued_at: Option<Instant>) {
        if let Some(start) = start {
            self.busy_ns.fetch_add(self.since(start), Relaxed);
        }
        if let (Some(start), Some(queued_at)) = (start, queued_at) {
            let waited = start.saturating_duration_since(queued_at).as_nanos();
            self.waited_ns
                .fetch_add(u64::try_from(waited).unwrap_or(u64::MAX), Relaxed);
            self.waits.fetch_add(1, Relaxed);
        }
        self.handled.fetch_add(1, Release);
    }

    pub(crate) fn worked_on(&self, task: &'static str, start: Option<Instant>) {
        let Some(start) = start else {
            return;
        };
        let spent = self.since(start);
        self.busy_ns.fetch_add(spent, Relaxed);
        let slot = self
            .tasks
            .iter()
            .find(|slot| *slot.name.get_or_init(|| task) == task);
        if let Some(slot) = slot {
            slot.busy_ns.fetch_add(spent, Relaxed);
        }
    }

    pub(crate) fn timed_in_pieces(&self) -> bool {
        self.pieces.load(Relaxed)
    }

    pub(crate) fn spun(&self, start: Option<Instant>) {
        if let Some(start) = start {
            self.spun_ns.fetch_add(self.since(start), Relaxed);
        }
    }

    pub(crate) fn add_turnout(&self, turnout: Turnout) {
        self.rounds.fetch_add(turnout.rounds, Relaxed);
        self.missed.fetch_add(turnout.missing(), Relaxed);
    }

    fn reading(&self) -> Reading {
        Reading {
            id: self.id,
            tasks: self
                .tasks
                .iter()
                .filter_map(|slot| {
                    let name = slot.name.get()?;
                    Some((*name, Duration::from_nanos(slot.busy_ns.load(Relaxed))))
                })
                .collect(),
            name: self.name.clone(),
            kind: self.kind,
            state: State::from_u8(self.state.load(Relaxed)),
            inline: self.inline.load(Relaxed),
            sent: self.sent.load(Relaxed),
            handled: self.handled.load(Acquire),
            parks: self.parks.load(Relaxed),
            busy: Duration::from_nanos(self.busy_ns.load(Relaxed)),
            waited: Duration::from_nanos(self.waited_ns.load(Relaxed)),
            waits: self.waits.load(Relaxed),
            spun: Duration::from_nanos(self.spun_ns.load(Relaxed)),
            rounds: self.rounds.load(Relaxed),
            missed: self.missed.load(Relaxed),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Board {
    watching: Arc<AtomicBool>,
    ids: Arc<AtomicU64>,
    made: Sender<Arc<Gauge>>,
    #[cfg(test)]
    clock_reads: Arc<AtomicU64>,
}

impl Board {
    pub(crate) fn gauge(&self, name: impl Into<String>, kind: Kind) -> Arc<Gauge> {
        let gauge = Arc::new(Gauge {
            id: self.ids.fetch_add(1, Relaxed),
            name: name.into(),
            kind,
            watching: Arc::clone(&self.watching),
            #[cfg(test)]
            clock_reads: Arc::clone(&self.clock_reads),
            state: AtomicU8::new(State::Starting as u8),
            inline: AtomicBool::new(false),
            sent: AtomicU64::new(0),
            handled: AtomicU64::new(0),
            parks: AtomicU64::new(0),
            busy_ns: AtomicU64::new(0),
            waited_ns: AtomicU64::new(0),
            waits: AtomicU64::new(0),
            spun_ns: AtomicU64::new(0),
            rounds: AtomicU64::new(0),
            missed: AtomicU64::new(0),
            pieces: AtomicBool::new(false),
            tasks: Default::default(),
        });
        let _ = self.made.send(Arc::clone(&gauge));
        gauge
    }
}

pub(crate) struct Gauges {
    board: Board,
    made: Receiver<Arc<Gauge>>,
    live: RefCell<Vec<Arc<Gauge>>>,
}

impl Gauges {
    pub(crate) fn new() -> Self {
        let (made, arrived) = mpsc::channel();
        Self {
            board: Board {
                watching: Arc::new(AtomicBool::new(false)),
                ids: Arc::new(AtomicU64::new(0)),
                made,
                #[cfg(test)]
                clock_reads: Arc::new(AtomicU64::new(0)),
            },
            made: arrived,
            live: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn board(&self) -> &Board {
        &self.board
    }

    pub(crate) fn watch(&self, on: bool) {
        self.board.watching.store(on, Relaxed);
    }

    pub(crate) fn watching(&self) -> bool {
        self.board.watching.load(Relaxed)
    }

    pub(crate) fn readings(&self) -> Vec<Reading> {
        let mut live = self.live.borrow_mut();
        live.extend(self.made.try_iter());
        live.retain(|gauge| Arc::strong_count(gauge) > 1);
        live.iter().map(|gauge| gauge.reading()).collect()
    }

    #[cfg(test)]
    pub(crate) fn clock_reads(&self) -> u64 {
        self.board.clock_reads.load(Relaxed)
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Gauge>>> = const { RefCell::new(None) };
    static PIECES: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn current() -> Option<Arc<Gauge>> {
    CURRENT.with(|current| current.borrow().clone())
}

pub(crate) fn with_current(f: impl FnOnce(&Gauge)) {
    CURRENT.with(|current| {
        if let Some(gauge) = current.borrow().as_deref() {
            f(gauge);
        }
    });
}

pub(crate) fn wear<T>(gauge: &Arc<Gauge>, f: impl FnOnce() -> T) -> T {
    let before = CURRENT.with(|current| current.replace(Some(Arc::clone(gauge))));
    let out = f();
    CURRENT.with(|current| *current.borrow_mut() = before);
    out
}

pub(crate) fn timed_on_pool(name: &'static str, task: impl FnOnce()) {
    let before = PIECES.replace(false);
    let watched = start(&[Kind::Pool]);
    task();
    let pieces = PIECES.replace(before);
    finish(watched, name, !pieces);
}

pub fn work<T>(name: &'static str, task: impl FnOnce() -> T) -> T {
    PIECES.set(true);
    let watched = start(&[Kind::Side, Kind::Pool]);
    let out = task();
    finish(watched, name, true);
    out
}

fn start(kinds: &[Kind]) -> Option<(Arc<Gauge>, Instant)> {
    CURRENT.with(|current| {
        let current = current.borrow();
        let gauge = current
            .as_ref()
            .filter(|gauge| kinds.contains(&gauge.kind()))?;
        gauge.pieces.store(true, Relaxed);
        let start = gauge.clock()?;
        gauge.set(State::Working);
        Some((Arc::clone(gauge), start))
    })
}

fn finish(watched: Option<(Arc<Gauge>, Instant)>, name: &'static str, charge: bool) {
    if let Some((gauge, start)) = watched {
        if charge {
            gauge.worked_on(name, Some(start));
        }
        gauge.set(State::Parked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cx, Job, Mode, Threads, Unthreaded};
    use std::time::Duration;

    struct Nap(Duration);

    impl Job for Nap {
        type Msg = ();
        type Reply = ();

        fn handle(&mut self, _: (), cx: &mut Cx<'_, ()>) {
            std::thread::sleep(self.0);
            cx.reply(());
        }
    }

    struct Rounds {
        ran: Arc<AtomicU64>,
        wait_for_pool: bool,
    }

    impl Job for Rounds {
        type Msg = ();
        type Reply = ();

        fn handle(&mut self, _: (), cx: &mut Cx<'_, ()>) {
            let mut items = [0u64; 4];
            let pooled = AtomicBool::new(false);
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut left = 200;
            let ran = &self.ran;
            let wait_for_pool = self.wait_for_pool;
            cx.split().rounds(
                "test",
                &mut items,
                |item| {
                    *item = item.wrapping_add(1);
                    if rayon::current_thread_index().is_some() {
                        pooled.store(true, Relaxed);
                    }
                },
                |_| {
                    ran.fetch_add(1, Relaxed);
                    left -= 1;
                    left > 0
                        || (wait_for_pool && !pooled.load(Relaxed) && Instant::now() < deadline)
                },
            );
            cx.reply(());
        }
    }

    struct Sides;

    impl Job for Sides {
        type Msg = ();
        type Reply = ();

        fn handle(&mut self, _: (), cx: &mut Cx<'_, ()>) {
            let nap = || std::thread::sleep(Duration::from_millis(3));
            cx.split().beside(
                vec![("3d", Box::new(nap)), ("2d lines", Box::new(nap))],
                || {},
            );
            cx.reply(());
        }
    }

    struct PiecedSide;

    impl Job for PiecedSide {
        type Msg = ();
        type Reply = ();

        fn handle(&mut self, _: (), cx: &mut Cx<'_, ()>) {
            let side = || {
                work("3d", || std::thread::sleep(Duration::from_millis(2)));
                std::thread::sleep(Duration::from_millis(60));
            };
            cx.split().beside(vec![("3d", Box::new(side))], || {});
            cx.reply(());
        }
    }

    fn pool_tasks(threads: &Threads) -> Vec<(&'static str, Duration)> {
        threads
            .readings()
            .into_iter()
            .filter(|reading| reading.kind == Kind::Pool)
            .flat_map(|reading| reading.tasks)
            .filter(|(_, busy)| !busy.is_zero())
            .collect()
    }

    fn reading(threads: &Threads, name: &str) -> Reading {
        threads
            .readings()
            .into_iter()
            .find(|reading| reading.name == name)
            .expect("a reading for the worker")
    }

    fn settle<J: Job<Msg = ()>>(threads: &Threads, worker: &mut crate::Worker<J>, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut got = 0;
        while got < count {
            got += worker.replies().count();
            assert!(Instant::now() < deadline, "timed out waiting for replies");
            std::thread::yield_now();
        }
        while threads
            .readings()
            .iter()
            .any(|reading| reading.kind == Kind::Worker && reading.queued() > 0)
        {
            assert!(
                Instant::now() < deadline,
                "a worker never counted its message"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_closed_pane_counts_messages_and_reads_no_clock() {
        let threads = Threads::new(Mode::Threads);
        let mut worker = threads.start("console", Nap(Duration::ZERO));
        for _ in 0..100 {
            worker.send(());
        }
        settle(&threads, &mut worker, 100);
        let reading = reading(&threads, "console");
        assert_eq!(
            (reading.sent, reading.handled, reading.queued()),
            (100, 100, 0)
        );
        assert_eq!(reading.busy, Duration::ZERO);
        assert_eq!(reading.waits, 0);
        assert_eq!(reading.kind, Kind::Worker);
        assert!(!reading.inline);
        assert_eq!(threads.gauges.clock_reads(), 0);
    }

    #[test]
    fn an_open_pane_times_work_and_the_wait_before_it() {
        let threads = Threads::new(Mode::Threads);
        threads.watch(true);
        let mut worker = threads.start("saves", Nap(Duration::from_millis(2)));
        for _ in 0..5 {
            worker.send(());
        }
        settle(&threads, &mut worker, 5);
        let reading = reading(&threads, "saves");
        assert_eq!(reading.handled, 5);
        assert!(reading.busy >= Duration::from_millis(10), "{reading:?}");
        assert_eq!(reading.waits, 5);
        assert!(reading.waited >= Duration::from_millis(2), "{reading:?}");
        assert!(threads.gauges.clock_reads() >= 15);
    }

    #[test]
    fn closing_the_pane_stops_the_clock_again() {
        let threads = Threads::new(Mode::Threads);
        threads.watch(true);
        let mut worker = threads.start("console", Nap(Duration::ZERO));
        worker.send(());
        settle(&threads, &mut worker, 1);
        threads.watch(false);
        let reads = threads.gauges.clock_reads();
        for _ in 0..50 {
            worker.send(());
        }
        settle(&threads, &mut worker, 50);
        assert_eq!(threads.gauges.clock_reads(), reads);
        assert_eq!(reading(&threads, "console").handled, 51);
    }

    #[test]
    fn an_idle_worker_reads_as_parked() {
        let threads = Threads::new(Mode::Threads);
        let mut worker = threads.start("console", Nap(Duration::ZERO));
        worker.send(());
        settle(&threads, &mut worker, 1);
        let deadline = Instant::now() + Duration::from_secs(10);
        while reading(&threads, "console").state != State::Parked {
            assert!(Instant::now() < deadline, "the worker never parked");
            std::thread::yield_now();
        }
        assert!(reading(&threads, "console").parks >= 1);
    }

    #[test]
    fn inline_workers_say_so_and_still_count() {
        let threads = Threads::new(Mode::Inline);
        let mut worker = threads.start("console", Nap(Duration::ZERO));
        worker.send(());
        settle(&threads, &mut worker, 1);
        let reading = reading(&threads, "console");
        assert!(reading.inline);
        assert_eq!(reading.handled, 1);
        assert_eq!(threads.unthreaded(), Some(Unthreaded::Chosen));
    }

    #[test]
    fn pool_threads_have_readings_and_rounds_count_on_the_caller() {
        let threads = Threads::new(Mode::Threads);
        assert_eq!(threads.unthreaded(), None);
        let pool = threads
            .readings()
            .into_iter()
            .filter(|reading| reading.kind == Kind::Pool)
            .count();
        assert_eq!(pool, threads.pool_size());
        threads.watch(true);
        let ran = Arc::new(AtomicU64::new(0));
        let mut worker = threads.start(
            "session",
            Rounds {
                ran: ran.clone(),
                wait_for_pool: threads.pool_size() > 1,
            },
        );
        let worked = || -> Duration {
            threads
                .readings()
                .iter()
                .filter(|reading| reading.kind == Kind::Pool)
                .map(|reading| reading.busy + reading.spun)
                .sum()
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            worker.send(());
            settle(&threads, &mut worker, 1);
            if threads.pool_size() <= 1 || worked() > Duration::ZERO {
                break;
            }
            assert!(Instant::now() < deadline, "no pool thread took a round");
        }
        let reading = reading(&threads, "session");
        assert_eq!(reading.rounds, ran.load(Relaxed));
    }

    #[test]
    fn pool_threads_time_each_named_task_only_while_watched() {
        let threads = Threads::new(Mode::Threads);
        let mut worker = threads.start("console", Sides);
        worker.send(());
        settle(&threads, &mut worker, 1);
        assert_eq!(pool_tasks(&threads), []);
        if threads.pool_size() == 0 {
            return;
        }
        threads.watch(true);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen: Vec<&str> = Vec::new();
        while seen.len() < 2 {
            worker.send(());
            settle(&threads, &mut worker, 1);
            let tasks = pool_tasks(&threads);
            for (name, busy) in &tasks {
                assert!(*busy >= Duration::from_millis(3), "{name}: {tasks:?}");
            }
            seen = ["3d", "2d lines"]
                .into_iter()
                .filter(|name| tasks.iter().any(|(task, _)| task == name))
                .collect();
            assert!(Instant::now() < deadline, "pool threads ran only {seen:?}");
        }
    }

    #[test]
    fn a_pool_side_task_timed_in_pieces_counts_only_its_pieces() {
        let threads = Threads::new(Mode::Threads);
        if threads.pool_size() == 0 {
            return;
        }
        threads.watch(true);
        let mut worker = threads.start("console", PiecedSide);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut frames = 0;
        let busy = loop {
            worker.send(());
            settle(&threads, &mut worker, 1);
            frames += 1;
            if let Some((_, busy)) = pool_tasks(&threads)
                .into_iter()
                .find(|(task, _)| *task == "3d")
            {
                break busy;
            }
            assert!(
                Instant::now() < deadline,
                "no pool thread ran the side task"
            );
        };
        assert!(busy >= Duration::from_millis(2), "{busy:?}");
        assert!(
            busy < Duration::from_millis(30) * frames,
            "{busy:?} in {frames} frames"
        );
    }

    #[test]
    fn a_dropped_worker_leaves_the_readings() {
        let threads = Threads::new(Mode::Threads);
        let mut worker = threads.start("watch", Nap(Duration::ZERO));
        worker.send(());
        settle(&threads, &mut worker, 1);
        drop(worker);
        let deadline = Instant::now() + Duration::from_secs(10);
        while threads
            .readings()
            .iter()
            .any(|reading| reading.name == "watch")
        {
            assert!(Instant::now() < deadline, "the reading stayed");
            std::thread::yield_now();
        }
    }

    #[test]
    fn background_tasks_show_while_they_run() {
        let threads = Threads::new(Mode::Threads);
        let (go, wait) = mpsc::channel::<()>();
        let mut pending = threads.background("engine build", move || wait.recv().is_ok());
        let running = reading(&threads, "engine build");
        assert_eq!(
            (running.kind, running.state),
            (Kind::Background, State::Working)
        );
        go.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(result) = pending.try_take() {
                assert_eq!(result, Ok(true));
                break;
            }
            assert!(Instant::now() < deadline, "the task never ended");
            std::thread::yield_now();
        }
        while threads
            .readings()
            .iter()
            .any(|reading| reading.name == "engine build")
        {
            assert!(Instant::now() < deadline, "the reading stayed");
            std::thread::yield_now();
        }
    }
}
