use crate::gauge::{self, Board, Kind, State};
use crate::{Mode, Panicked, Rng, guarded, side_threads};
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};

type Task = Box<dyn FnOnce() + Send>;

struct Queued {
    name: &'static str,
    task: Task,
}

#[derive(Clone)]
pub struct Spawner {
    threaded: bool,
    queue: Sender<Queued>,
    panics: Sender<Panicked>,
    board: Board,
}

pub(crate) struct SideJobs {
    queued: Receiver<Queued>,
    panics: Receiver<Panicked>,
    held: RefCell<Vec<Queued>>,
    rng: RefCell<Option<Rng>>,
    spawner: Spawner,
}

impl SideJobs {
    pub(crate) fn new(mode: Mode, board: Board) -> Self {
        let (queue, queued) = mpsc::channel();
        let (panics_tx, panics) = mpsc::channel();
        let rng = match mode {
            Mode::Shuffled(seed) => Some(Rng::new(seed ^ 0x51DE_70B5)),
            _ => None,
        };
        Self {
            queued,
            panics,
            held: RefCell::new(Vec::new()),
            rng: RefCell::new(rng),
            spawner: Spawner {
                threaded: side_threads(mode),
                queue,
                panics: panics_tx,
                board,
            },
        }
    }

    pub(crate) fn spawner(&self) -> Spawner {
        self.spawner.clone()
    }

    pub(crate) fn run(&self) -> Vec<Panicked> {
        let mut due: Vec<Queued> = self.held.take();
        due.extend(self.queued.try_iter());
        let mut rng = self.rng.borrow_mut();
        let mut held = Vec::new();
        let mut panics: Vec<Panicked> = self.panics.try_iter().collect();
        for job in due {
            if let Some(rng) = rng.as_mut()
                && rng.below(2) == 0
            {
                held.push(job);
                continue;
            }
            if let Err(text) = guarded(job.name, job.task) {
                panics.push(Panicked(text));
            }
        }
        *self.held.borrow_mut() = held;
        panics
    }
}

impl Spawner {
    pub fn run(&self, name: &'static str, task: impl FnOnce() + Send + 'static) {
        if self.threaded {
            self.start(name, task);
        } else {
            let _ = self.queue.send(Queued {
                name,
                task: Box::new(task),
            });
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn run_future(
        &self,
        name: &'static str,
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        self.run(name, move || pollster::block_on(future));
    }

    #[cfg(target_arch = "wasm32")]
    pub fn run_future(
        &self,
        _name: &'static str,
        future: impl std::future::Future<Output = ()> + 'static,
    ) {
        crate::spawn_local(future);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn thread(&self, name: &'static str, task: impl FnOnce() + Send + 'static) {
        self.start(name, task);
    }

    fn start(&self, name: &'static str, task: impl FnOnce() + Send + 'static) {
        let panics = self.panics.clone();
        let gauge = self.board.gauge(name, Kind::Side);
        gauge.set(State::Working);
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                let start = gauge.clock();
                if let Err(text) = gauge::wear(&gauge, || guarded(name, task)) {
                    let _ = panics.send(Panicked(text));
                }
                gauge.handled(start.filter(|_| !gauge.timed_in_pieces()), None);
                gauge.set(State::Ended);
            })
            .expect("a side thread starts");
    }
}

#[cfg(test)]
mod tests {
    use crate::{Mode, Panicked, State, Threads};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn ran_within(threads: &Threads, rx: &mpsc::Receiver<u8>) -> (Vec<u8>, Vec<Panicked>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut got = Vec::new();
        let mut panics = Vec::new();
        while got.len() < 2 && Instant::now() < deadline {
            panics.extend(threads.run_side_jobs());
            got.extend(rx.try_iter());
            std::thread::yield_now();
        }
        while panics.is_empty() && Instant::now() < deadline {
            panics.extend(threads.run_side_jobs());
            std::thread::yield_now();
        }
        got.sort();
        (got, panics)
    }

    #[test]
    fn side_jobs_run_once_and_report_panics_in_every_mode() {
        for mode in [Mode::Threads, Mode::Inline, Mode::Shuffled(3)] {
            let threads = Threads::new(mode);
            let spawner = threads.spawner();
            let (tx, rx) = mpsc::channel();
            for n in [1, 2] {
                let tx = tx.clone();
                spawner.run("scan", move || tx.send(n).unwrap());
            }
            spawner.run("import", || panic!("bad zip"));
            let (got, panics) = ran_within(&threads, &rx);
            assert_eq!(got, [1, 2], "{mode:?}");
            assert_eq!(
                panics,
                [Panicked("import: bad zip".to_string())],
                "{mode:?}"
            );
        }
    }

    #[test]
    fn inline_side_jobs_wait_for_the_ui_thread_to_run_them() {
        let threads = Threads::new(Mode::Inline);
        let (tx, rx) = mpsc::channel();
        threads.spawner().run("scan", move || tx.send(7u8).unwrap());
        assert!(rx.try_recv().is_err());
        assert!(threads.run_side_jobs().is_empty());
        assert_eq!(rx.try_recv(), Ok(7));
    }

    #[test]
    fn a_side_thread_runs_on_its_own_thread_even_inline() {
        let threads = Threads::new(Mode::Inline);
        let (tx, rx) = mpsc::channel();
        threads.spawner().thread("webcam", move || {
            tx.send(std::thread::current().name().map(str::to_string))
                .unwrap()
        });
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(10)),
            Ok(Some("webcam".to_string()))
        );
    }

    #[test]
    fn a_long_lived_side_thread_shows_the_work_it_times_and_parks_between() {
        let threads = Threads::new(Mode::Threads);
        threads.watch(true);
        let (worked, ready) = mpsc::channel();
        let (end, ended) = mpsc::channel::<()>();
        threads.spawner().thread("jit", move || {
            crate::work("compile", || std::thread::sleep(Duration::from_millis(20)));
            worked.send(()).unwrap();
            let _ = ended.recv();
        });
        ready.recv_timeout(Duration::from_secs(10)).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        let reading = threads
            .readings()
            .into_iter()
            .find(|reading| reading.name == "jit")
            .unwrap();
        assert!(reading.busy >= Duration::from_millis(20), "{reading:?}");
        assert!(reading.busy < Duration::from_millis(60), "{reading:?}");
        assert_eq!(reading.state, State::Parked);
        assert_eq!(reading.tasks.len(), 1, "{reading:?}");
        assert_eq!(reading.tasks[0].0, "compile");
        drop(end);
    }
}
