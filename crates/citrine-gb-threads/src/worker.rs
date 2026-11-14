use crate::gauge::{self, Board, Gauge, Kind, State};
use crate::{Mode, Rng, Scope, Split, Started, spawn};
use std::collections::VecDeque;
use std::sync::atomic::{Ordering, fence};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, OnceLock};
use std::thread::Thread;
use web_time::Instant;

pub trait Job: Send + 'static {
    type Msg: Send + 'static;
    type Reply: Send + 'static;

    fn handle(&mut self, msg: Self::Msg, cx: &mut Cx<'_, Self::Reply>);

    fn stop(&mut self, _cx: &mut Cx<'_, Self::Reply>) {}
}

pub struct Cx<'a, R> {
    replies: &'a Sender<R>,
    pool: Option<&'a rayon::ThreadPool>,
    rng: Option<&'a mut Rng>,
}

impl<R> Cx<'_, R> {
    pub fn reply(&mut self, reply: R) {
        let _ = self.replies.send(reply);
    }

    pub fn scope<'s>(&mut self, name: &'static str, f: impl FnOnce(&Scope<'s>)) {
        self.split().scope(name, f);
    }

    pub fn split(&mut self) -> Split<'_> {
        Split::new(self.pool, self.rng.as_deref_mut())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    Stopped,
    Panicked(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Running,
    Ended(Ended),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Thread,
    Inline,
}

pub struct Replies<'w, R> {
    items: std::collections::vec_deque::Drain<'w, R>,
}

impl<R> Iterator for Replies<'_, R> {
    type Item = R;

    fn next(&mut self) -> Option<R> {
        self.items.next()
    }
}

pub(crate) struct Context {
    pub(crate) mode: Mode,
    pub(crate) pool: Option<Arc<rayon::ThreadPool>>,
    pub(crate) rng: Option<Rng>,
    pub(crate) started: Sender<Started>,
    pub(crate) board: Board,
}

enum Control<J: Job> {
    Msg(J::Msg, Option<Instant>),
    Hand,
    Stop,
}

struct Remote<J: Job> {
    inbox: Inbox<J>,
    back: Receiver<Box<J>>,
    ended: Arc<OnceLock<Ended>>,
}

struct Inbox<J: Job> {
    queue: Sender<Control<J>>,
    wake: Arc<OnceLock<Thread>>,
}

impl<J: Job> Inbox<J> {
    fn send(&self, control: Control<J>) {
        let _ = self.queue.send(control);
        fence(Ordering::SeqCst);
        if let Some(thread) = self.wake.get() {
            thread.unpark();
        }
    }
}

impl<J: Job> Clone for Inbox<J> {
    fn clone(&self) -> Self {
        Self {
            queue: self.queue.clone(),
            wake: Arc::clone(&self.wake),
        }
    }
}

impl<J: Job> Remote<J> {
    #[cfg(not(target_arch = "wasm32"))]
    fn take_back(&self) -> Option<Box<J>> {
        self.back.recv().ok()
    }

    #[cfg(target_arch = "wasm32")]
    fn take_back(&self) -> Option<Box<J>> {
        loop {
            match self.back.try_recv() {
                Ok(job) => return Some(job),
                Err(TryRecvError::Disconnected) => return None,
                Err(TryRecvError::Empty) if self.ended.get().is_some() => return None,
                Err(TryRecvError::Empty) => std::hint::spin_loop(),
            }
        }
    }
}

enum Place<J: Job> {
    Thread(Remote<J>),
    Moving(Remote<J>, Placement),
    Inline(Box<J>, Option<Ended>),
    Gone,
}

pub struct Worker<J: Job> {
    name: String,
    context: Context,
    place: Place<J>,
    queue: Sender<(J::Msg, Option<Instant>)>,
    queued: Receiver<(J::Msg, Option<Instant>)>,
    replies: Sender<J::Reply>,
    arrived: Receiver<J::Reply>,
    ready: VecDeque<J::Reply>,
    held: usize,
    gauge: Arc<Gauge>,
}

impl<J: Job> Worker<J> {
    pub(crate) fn start(context: Context, name: String, job: J) -> Self {
        let (queue, queued) = mpsc::channel();
        let (replies, arrived) = mpsc::channel();
        let gauge = context.board.gauge(name.clone(), Kind::Worker);
        gauge.set_inline(true);
        gauge.set(State::Parked);
        let mut worker = Self {
            name,
            context,
            place: Place::Inline(Box::new(job), None),
            queue,
            queued,
            replies,
            arrived,
            ready: VecDeque::new(),
            held: 0,
            gauge,
        };
        if worker.context.mode == Mode::Threads {
            worker.move_to(Placement::Thread);
        }
        worker
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn send(&self, msg: J::Msg) {
        let queued_at = self.gauge.clock();
        match &self.place {
            Place::Thread(remote) => remote.inbox.send(Control::Msg(msg, queued_at)),
            Place::Moving(..) | Place::Inline(..) => {
                let _ = self.queue.send((msg, queued_at));
            }
            Place::Gone => return,
        }
        self.gauge.sent();
    }

    pub fn replies(&mut self) -> Replies<'_, J::Reply> {
        self.settle();
        self.ready.extend(self.arrived.try_iter());
        let mut take = self.ready.len();
        if let Some(rng) = self.context.rng.as_mut()
            && take > self.held
            && rng.below(2) == 0
        {
            take = self.held + rng.below(take - self.held);
        }
        self.held = self.ready.len() - take;
        Replies {
            items: self.ready.drain(..take),
        }
    }

    pub fn status(&self) -> Status {
        let ended = match &self.place {
            Place::Thread(remote) | Place::Moving(remote, _) => remote.ended.get(),
            Place::Inline(_, ended) => ended.as_ref(),
            Place::Gone => Some(&Ended::Stopped),
        };
        ended.map_or(Status::Running, |ended| Status::Ended(ended.clone()))
    }

    pub fn has_started(&self) -> bool {
        match &self.place {
            Place::Thread(remote) | Place::Moving(remote, _) => remote.inbox.wake.get().is_some(),
            Place::Inline(..) | Place::Gone => true,
        }
    }

    pub fn placement(&self) -> Placement {
        match self.place {
            Place::Inline(..) => Placement::Inline,
            _ => Placement::Thread,
        }
    }

    pub fn move_to(&mut self, placement: Placement) {
        if self.context.mode != Mode::Threads {
            return;
        }
        self.place = match (std::mem::replace(&mut self.place, Place::Gone), placement) {
            (Place::Thread(remote), Placement::Inline) => {
                remote.inbox.send(Control::Hand);
                Place::Moving(remote, Placement::Inline)
            }
            (Place::Moving(remote, _), target) => Place::Moving(remote, target),
            (Place::Inline(job, None), Placement::Thread) => self.spawn(job),
            (place, _) => place,
        };
        self.settle();
    }

    pub fn inline(&mut self) -> Option<&mut J> {
        self.settle();
        match &mut self.place {
            Place::Inline(job, None) => Some(job),
            _ => None,
        }
    }

    fn settle(&mut self) {
        if let Place::Moving(remote, target) = &self.place {
            let target = *target;
            let arrived = remote.back.try_recv();
            if let Ok(job) = arrived {
                self.place = Place::Inline(job, None);
                if target == Placement::Thread {
                    self.move_to(Placement::Thread);
                    return;
                }
            } else if remote.ended.get().is_some() {
                let Place::Moving(remote, _) = std::mem::replace(&mut self.place, Place::Gone)
                else {
                    unreachable!("checked above")
                };
                self.place = Place::Thread(remote);
            }
        }
        if let Place::Inline(job, ended @ None) = &mut self.place {
            self.gauge.set_inline(true);
            for (msg, queued_at) in self.queued.try_iter() {
                let mut cx = Cx {
                    replies: &self.replies,
                    pool: None,
                    rng: self.context.rng.as_mut(),
                };
                let start = self.gauge.clock();
                let outcome = if start.is_some() {
                    gauge::wear(&self.gauge, || {
                        guarded(&self.name, || job.handle(msg, &mut cx))
                    })
                } else {
                    guarded(&self.name, || job.handle(msg, &mut cx))
                };
                self.gauge.handled(start, queued_at);
                if let Err(panicked) = outcome {
                    *ended = Some(panicked);
                    self.gauge.set(State::Ended);
                    break;
                }
            }
        }
    }

    fn spawn(&self, job: Box<J>) -> Place<J> {
        let (queue, control) = mpsc::channel();
        let (hand_back, back) = mpsc::channel();
        let ended = Arc::new(OnceLock::new());
        let inbox = Inbox {
            queue,
            wake: Arc::new(OnceLock::new()),
        };
        for (msg, queued_at) in self.queued.try_iter() {
            inbox.send(Control::Msg(msg, queued_at));
        }
        self.gauge.set_inline(false);
        let run = Run {
            name: self.name.clone(),
            job,
            control,
            wake: Arc::clone(&inbox.wake),
            replies: self.replies.clone(),
            hand_back,
            ended: Arc::clone(&ended),
            pool: self.context.pool.clone(),
            gauge: Arc::clone(&self.gauge),
        };
        let lost = Arc::clone(&ended);
        let name = self.name.clone();
        let thread = spawn::thread(
            self.name.clone(),
            move || run.run(),
            move |text| {
                let _ = lost.set(Ended::Panicked(format!("{name}: {text}")));
            },
        );
        let stop = inbox.clone();
        let _ = self.context.started.send(Started {
            name: self.name.clone(),
            stop: Box::new(move || stop.send(Control::Stop)),
            thread,
        });
        Place::Thread(Remote { inbox, back, ended })
    }
}

impl<J: Job> Drop for Worker<J> {
    fn drop(&mut self) {
        match std::mem::replace(&mut self.place, Place::Gone) {
            Place::Thread(remote) => remote.inbox.send(Control::Stop),
            Place::Moving(remote, _) => {
                if let Some(job) = remote.take_back() {
                    self.place = Place::Inline(job, None);
                    self.drop_inline();
                }
            }
            place @ Place::Inline(..) => {
                self.place = place;
                self.drop_inline();
            }
            Place::Gone => {}
        }
    }
}

impl<J: Job> Worker<J> {
    fn drop_inline(&mut self) {
        self.settle();
        if let Place::Inline(job, None) = &mut self.place {
            let mut cx = Cx {
                replies: &self.replies,
                pool: None,
                rng: self.context.rng.as_mut(),
            };
            let _ = guarded(&self.name, || job.stop(&mut cx));
        }
    }
}

struct Run<J: Job> {
    name: String,
    job: Box<J>,
    control: Receiver<Control<J>>,
    wake: Arc<OnceLock<Thread>>,
    replies: Sender<J::Reply>,
    hand_back: Sender<Box<J>>,
    ended: Arc<OnceLock<Ended>>,
    pool: Option<Arc<rayon::ThreadPool>>,
    gauge: Arc<Gauge>,
}

impl<J: Job> Run<J> {
    fn next(&self) -> Option<Control<J>> {
        loop {
            match self.control.try_recv() {
                Ok(control) => return Some(control),
                Err(TryRecvError::Empty) => {
                    self.gauge.parked();
                    std::thread::park();
                }
                Err(TryRecvError::Disconnected) => return None,
            }
        }
    }

    fn run(self) {
        let gauge = Arc::clone(&self.gauge);
        gauge::wear(&gauge, || self.serve());
    }

    fn serve(mut self) {
        let _ = self.wake.set(std::thread::current());
        fence(Ordering::SeqCst);
        loop {
            let mut cx = Cx {
                replies: &self.replies,
                pool: self.pool.as_deref(),
                rng: None,
            };
            let control = self.next();
            let job = &mut self.job;
            let outcome = match control {
                Some(Control::Msg(msg, queued_at)) => {
                    self.gauge.set(State::Working);
                    let start = self.gauge.clock();
                    let outcome = guarded(&self.name, || job.handle(msg, &mut cx));
                    self.gauge.handled(start, queued_at);
                    self.gauge.set(State::Working);
                    outcome
                }
                Some(Control::Hand) => {
                    self.gauge.set(State::Parked);
                    self.gauge.set_inline(true);
                    if let Err(mpsc::SendError(mut job)) = self.hand_back.send(self.job) {
                        let _ = guarded(&self.name, || job.stop(&mut cx));
                        let _ = self.ended.set(Ended::Stopped);
                    }
                    return;
                }
                Some(Control::Stop) | None => {
                    let ended = match guarded(&self.name, || job.stop(&mut cx)) {
                        Ok(()) => Ended::Stopped,
                        Err(panicked) => panicked,
                    };
                    let _ = self.ended.set(ended);
                    self.gauge.set(State::Ended);
                    return;
                }
            };
            if let Err(panicked) = outcome {
                let _ = self.ended.set(panicked);
                self.gauge.set(State::Ended);
                return;
            }
        }
    }
}

fn guarded(name: &str, f: impl FnOnce()) -> Result<(), Ended> {
    crate::guarded(name, f).map_err(Ended::Panicked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Threads;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::ThreadId;
    use std::time::{Duration, Instant};

    const MODES: [Mode; 4] = [
        Mode::Threads,
        Mode::Inline,
        Mode::Shuffled(1),
        Mode::Shuffled(7),
    ];
    const BOOM: u64 = u64::MAX;

    struct Counter {
        total: u64,
    }

    impl Job for Counter {
        type Msg = u64;
        type Reply = u64;

        fn handle(&mut self, msg: u64, cx: &mut Cx<'_, u64>) {
            assert_ne!(msg, BOOM, "boom");
            self.total += msg;
            cx.reply(self.total);
        }
    }

    struct Where;

    impl Job for Where {
        type Msg = u32;
        type Reply = (u32, ThreadId);

        fn handle(&mut self, msg: u32, cx: &mut Cx<'_, (u32, ThreadId)>) {
            cx.reply((msg, std::thread::current().id()));
        }
    }

    struct Consoles {
        states: Vec<u64>,
    }

    impl Job for Consoles {
        type Msg = u32;
        type Reply = Vec<u64>;

        fn handle(&mut self, frames: u32, cx: &mut Cx<'_, Vec<u64>>) {
            cx.scope("test", |scope| {
                for state in self.states.iter_mut() {
                    scope.spawn(move || {
                        for _ in 0..frames {
                            *state = state.wrapping_mul(6_364_136_223_846_793_005) ^ (*state >> 29);
                        }
                    });
                }
            });
            cx.reply(self.states.clone());
        }
    }

    struct Stops(Arc<AtomicBool>);

    impl Job for Stops {
        type Msg = ();
        type Reply = &'static str;

        fn handle(&mut self, _: (), _: &mut Cx<'_, &'static str>) {}

        fn stop(&mut self, cx: &mut Cx<'_, &'static str>) {
            self.0.store(true, Ordering::SeqCst);
            cx.reply("last battery bytes");
        }
    }

    fn within(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::yield_now();
        }
    }

    fn collect<J: Job>(worker: &mut Worker<J>, count: usize) -> Vec<J::Reply> {
        let mut replies = Vec::new();
        within("replies", || {
            replies.extend(worker.replies());
            replies.len() >= count
        });
        replies
    }

    #[test]
    fn replies_arrive_in_send_order_in_every_mode() {
        for mode in MODES {
            let threads = Threads::new(mode);
            let mut worker = threads.start("counter", Counter { total: 0 });
            for n in 1..=50 {
                worker.send(n);
            }
            let expected: Vec<u64> = (1..=50).map(|n| n * (n + 1) / 2).collect();
            assert_eq!(collect(&mut worker, 50), expected, "{mode:?}");
        }
    }

    #[test]
    fn a_worker_says_it_has_started_once_its_thread_runs_and_at_once_inline() {
        for mode in MODES {
            let threads = Threads::new(mode);
            let worker = threads.start("counter", Counter { total: 0 });
            if mode == Mode::Threads {
                within("the thread start", || worker.has_started());
            } else {
                assert!(worker.has_started(), "{mode:?}");
            }
        }
    }

    #[test]
    fn a_panicking_job_ends_with_panicked_while_the_others_keep_running() {
        for mode in MODES {
            let threads = Threads::new(mode);
            let mut broken = threads.start("console: a", Counter { total: 0 });
            let mut fine = threads.start("console: b", Counter { total: 0 });
            broken.send(BOOM);
            within("the panic", || {
                broken.replies().for_each(drop);
                broken.status() != Status::Running
            });
            let Status::Ended(Ended::Panicked(text)) = broken.status() else {
                panic!("{mode:?}: {:?}", broken.status());
            };
            assert!(text.starts_with("console: a: "), "{text}");
            fine.send(5);
            assert_eq!(collect(&mut fine, 1), [5], "{mode:?}");
            assert_eq!(fine.status(), Status::Running);
        }
    }

    #[test]
    fn parallel_steps_give_the_same_states_in_every_mode() {
        let run = |mode| {
            let threads = Threads::new(mode);
            let states = (1..=6).map(|n| n * 0x9E37_79B9).collect();
            let mut worker = threads.start("session", Consoles { states });
            for frames in [10, 200, 3] {
                worker.send(frames);
            }
            collect(&mut worker, 3)
        };
        let inline = run(Mode::Inline);
        for mode in MODES {
            assert_eq!(run(mode), inline, "{mode:?}");
        }
    }

    #[test]
    fn a_threaded_worker_moved_inline_keeps_its_state_and_runs_on_the_caller() {
        let threads = Threads::new(Mode::Threads);
        let mut worker = threads.start("console", Counter { total: 0 });
        worker.send(1);
        worker.send(2);
        worker.move_to(Placement::Inline);
        worker.send(3);
        within("the hand-back", || worker.inline().is_some());
        assert_eq!(worker.placement(), Placement::Inline);
        assert_eq!(worker.inline().unwrap().total, 6);
        assert_eq!(collect(&mut worker, 3), [1, 3, 6]);
        worker.move_to(Placement::Thread);
        assert_eq!(worker.placement(), Placement::Thread);
        worker.send(4);
        assert_eq!(collect(&mut worker, 1), [10]);
    }

    #[test]
    fn a_worker_runs_on_its_own_named_thread_and_inline_on_the_caller() {
        let here = std::thread::current().id();
        let threads = Threads::new(Mode::Threads);
        let mut worker = threads.start("console: tobu", Where);
        worker.send(1);
        let [(_, there)] = collect(&mut worker, 1)[..] else {
            panic!("one reply")
        };
        assert_ne!(there, here);
        worker.move_to(Placement::Inline);
        worker.send(2);
        assert_eq!(collect(&mut worker, 1), [(2, here)]);
        worker.move_to(Placement::Thread);
        worker.send(3);
        let [(3, again)] = collect(&mut worker, 1)[..] else {
            panic!("one reply")
        };
        assert_ne!(again, here);
        for mode in [Mode::Inline, Mode::Shuffled(2)] {
            let threads = Threads::new(mode);
            let mut worker = threads.start("console: tobu", Where);
            worker.move_to(Placement::Thread);
            assert_eq!(worker.placement(), Placement::Inline);
            worker.send(1);
            assert_eq!(collect(&mut worker, 1), [(1, here)], "{mode:?}");
        }
    }

    #[test]
    fn dropping_a_worker_runs_its_stop_in_every_mode() {
        for mode in MODES {
            let threads = Threads::new(mode);
            let stopped = Arc::new(AtomicBool::new(false));
            drop(threads.start("saves", Stops(Arc::clone(&stopped))));
            within("the stop", || stopped.load(Ordering::SeqCst));
        }
    }

    #[test]
    fn shuffled_replies_come_in_order_and_at_most_one_poll_late() {
        let mut held_back = 0;
        for seed in 1..=10 {
            let threads = Threads::new(Mode::Shuffled(seed));
            let mut worker = threads.start("counter", Counter { total: 0 });
            for n in 1..=20 {
                worker.send(n);
            }
            let first: Vec<u64> = worker.replies().collect();
            let second: Vec<u64> = worker.replies().collect();
            if first.len() < 20 {
                held_back += 1;
            }
            let all: Vec<u64> = first.into_iter().chain(second).collect();
            let expected: Vec<u64> = (1..=20).map(|n| n * (n + 1) / 2).collect();
            assert_eq!(all, expected, "seed {seed}");
        }
        assert!(held_back > 0, "no seed held a reply back");
    }
}
