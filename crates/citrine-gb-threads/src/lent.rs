use crate::{Cx, Ended, Job, Split, Status, Threads, Worker};
use std::cell::{OnceCell, RefCell};
#[cfg(target_arch = "wasm32")]
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

const LOOK_EVERY: Duration = Duration::from_millis(5);

pub trait Lendable: 'static {
    type Plan: 'static;

    fn work(&mut self, plan: Self::Plan, split: &mut Split<'_>);
}

pub struct Lent<T: Lendable> {
    home: OnceCell<Box<T>>,
    away: Option<RefCell<Box<dyn Away<T>>>>,
}

trait Away<T: Lendable> {
    fn send(&self, value: Box<T>, plan: T::Plan);
    fn recv(&mut self, timeout: Duration) -> Result<Box<T>, RecvTimeoutError>;
    fn try_recv(&mut self) -> Option<Box<T>>;
    fn running(&self) -> bool;
    fn started(&self) -> bool;
    fn ended(&self) -> String;
}

struct Errands<T: Lendable + Send>
where
    T::Plan: Send,
{
    worker: Worker<Errand<T>>,
    back: Receiver<Box<T>>,
}

struct Errand<T> {
    back: Sender<Box<T>>,
}

impl<T: Lendable + Send> Job for Errand<T>
where
    T::Plan: Send,
{
    type Msg = (Box<T>, T::Plan);
    type Reply = ();

    fn handle(&mut self, (mut value, plan): Self::Msg, cx: &mut Cx<'_, ()>) {
        value.work(plan, &mut cx.split());
        let _ = self.back.send(value);
    }
}

impl<T: Lendable + Send> Away<T> for Errands<T>
where
    T::Plan: Send,
{
    fn send(&self, value: Box<T>, plan: T::Plan) {
        self.worker.send((value, plan));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn recv(&mut self, timeout: Duration) -> Result<Box<T>, RecvTimeoutError> {
        self.worker.replies().for_each(drop);
        self.back.recv_timeout(timeout)
    }

    #[cfg(target_arch = "wasm32")]
    fn recv(&mut self, _timeout: Duration) -> Result<Box<T>, RecvTimeoutError> {
        self.worker.replies().for_each(drop);
        match self.back.try_recv() {
            Ok(value) => Ok(value),
            Err(TryRecvError::Empty) if self.running() => Err(RecvTimeoutError::Timeout),
            Err(_) => Err(RecvTimeoutError::Disconnected),
        }
    }

    fn try_recv(&mut self) -> Option<Box<T>> {
        self.back.try_recv().ok()
    }

    fn running(&self) -> bool {
        self.worker.status() == Status::Running
    }

    fn started(&self) -> bool {
        self.worker.has_started()
    }

    fn ended(&self) -> String {
        match self.worker.status() {
            Status::Ended(Ended::Panicked(text)) => text,
            _ => format!("the {} worker stopped", self.worker.name()),
        }
    }
}

impl<T: Lendable> Lent<T> {
    pub fn new(value: Box<T>) -> Self {
        Self {
            home: OnceCell::from(value),
            away: None,
        }
    }

    pub fn run_on(&mut self, threads: &Threads, name: impl Into<String>)
    where
        T: Send,
        T::Plan: Send,
    {
        self.get();
        let (back, home) = mpsc::channel();
        let worker = threads.start(name, Errand { back });
        self.away = Some(RefCell::new(Box::new(Errands { worker, back: home })));
    }

    pub fn park(&mut self) {
        self.get();
        self.away = None;
    }

    pub fn has_worker(&self) -> bool {
        self.away.is_some()
    }

    pub fn has_started(&self) -> bool {
        self.away
            .as_ref()
            .is_some_and(|away| away.borrow().started())
    }

    pub fn is_home(&self) -> bool {
        self.home.get().is_some()
    }

    pub fn home(&self) -> Option<&T> {
        self.home.get().map(|value| &**value)
    }

    pub fn get(&self) -> &T {
        self.home.get_or_init(|| self.fetch())
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.get();
        self.home.get_mut().expect("the value is home")
    }

    pub fn into_inner(mut self) -> Box<T> {
        self.get();
        self.away = None;
        self.home.take().expect("the value is home")
    }

    pub fn lend(&mut self, plan: T::Plan) {
        self.get();
        let Some(away) = &self.away else {
            self.get_mut().work(plan, &mut Split::inline());
            return;
        };
        let value = self.home.take().expect("the value is home");
        away.borrow().send(value, plan);
    }

    pub fn wait(&self, timeout: Duration) -> bool {
        if self.is_home() {
            return true;
        }
        let Some(away) = &self.away else {
            return true;
        };
        let mut away = away.borrow_mut();
        match away.recv(timeout) {
            Ok(value) => {
                drop(away);
                let _ = self.home.set(value);
                true
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => panic!("{}", away.ended()),
        }
    }

    fn fetch(&self) -> Box<T> {
        let away = self
            .away
            .as_ref()
            .expect("the value is always home without a worker");
        let mut away = away.borrow_mut();
        loop {
            match away.recv(LOOK_EVERY) {
                Ok(value) => return value,
                Err(RecvTimeoutError::Timeout) if away.running() => {}
                Err(_) => match away.try_recv() {
                    Some(value) => return value,
                    None => panic!("{}", away.ended()),
                },
            }
        }
    }
}

impl<T: Lendable + Default> Default for Lent<T> {
    fn default() -> Self {
        Self::new(Box::default())
    }
}

const _: fn() = || {
    fn send<T: Send>() {}
    send::<Errand<Vec<u8>>>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mode;
    use std::thread::ThreadId;

    const MODES: [Mode; 4] = [
        Mode::Threads,
        Mode::Inline,
        Mode::Shuffled(3),
        Mode::Shuffled(11),
    ];

    #[derive(Default)]
    struct Tally {
        total: u64,
        seen: Vec<ThreadId>,
    }

    impl Lendable for Tally {
        type Plan = u64;

        fn work(&mut self, add: u64, _split: &mut Split<'_>) {
            assert!(add != 0, "nothing to add");
            self.total = self.total.wrapping_mul(31).wrapping_add(add);
            self.seen.push(std::thread::current().id());
        }
    }

    fn tallied(threads: Option<&Threads>) -> (u64, Vec<ThreadId>) {
        let mut lent = Lent::<Tally>::default();
        if let Some(threads) = threads {
            lent.run_on(threads, "tally");
        }
        for add in 1..=20 {
            lent.lend(add);
            if add % 3 == 0 {
                assert!(lent.wait(Duration::from_secs(5)));
            }
            if add % 7 == 0 {
                lent.get_mut().total += 1;
            }
        }
        let tally = lent.into_inner();
        (tally.total, tally.seen)
    }

    #[test]
    fn lent_work_gives_the_same_value_in_every_mode_and_without_a_worker() {
        let (alone, _) = tallied(None);
        for mode in MODES {
            let threads = Threads::new(mode);
            assert_eq!(tallied(Some(&threads)).0, alone, "{mode:?}");
        }
    }

    #[test]
    fn lent_work_runs_on_the_worker_with_threads_and_on_the_caller_inline() {
        let me = std::thread::current().id();
        let threads = Threads::new(Mode::Threads);
        let (_, seen) = tallied(Some(&threads));
        assert!(seen.iter().all(|id| *id != me));
        for mode in [Mode::Inline, Mode::Shuffled(5)] {
            let threads = Threads::new(mode);
            let (_, seen) = tallied(Some(&threads));
            assert!(seen.iter().all(|id| *id == me), "{mode:?}");
        }
    }

    #[test]
    fn a_parked_value_comes_home_and_runs_on_the_caller() {
        let threads = Threads::new(Mode::Threads);
        let mut lent = Lent::<Tally>::default();
        lent.run_on(&threads, "tally");
        lent.lend(4);
        lent.park();
        assert!(lent.is_home() && !lent.has_worker());
        lent.lend(2);
        assert_eq!(lent.home().map(|t| t.total), Some(4 * 31 + 2));
        assert_eq!(lent.get().seen[1], std::thread::current().id());
    }

    #[test]
    fn a_lent_value_has_started_once_its_worker_runs_and_never_without_one() {
        for mode in MODES {
            let threads = Threads::new(mode);
            let mut lent = Lent::<Tally>::default();
            assert!(!lent.has_started(), "{mode:?}");
            lent.run_on(&threads, "tally");
            let until = std::time::Instant::now() + Duration::from_secs(5);
            while !lent.has_started() && std::time::Instant::now() < until {
                std::thread::yield_now();
            }
            assert!(lent.has_started(), "{mode:?}");
            lent.park();
            assert!(!lent.has_started(), "{mode:?}");
        }
    }

    #[test]
    fn a_panic_in_lent_work_names_the_worker_when_the_value_is_fetched() {
        for mode in MODES {
            let threads = Threads::new(mode);
            let mut lent = Lent::<Tally>::default();
            lent.run_on(&threads, "tally");
            lent.lend(0);
            let fetched =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lent.get().total));
            let text = fetched.expect_err("the fetch panics");
            let text = text.downcast_ref::<String>().cloned().unwrap_or_default();
            assert!(
                text.starts_with("tally: nothing to add"),
                "{mode:?}: {text}"
            );
        }
    }
}
