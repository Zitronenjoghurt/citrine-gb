#![expect(
    clippy::disallowed_types,
    reason = "compile workers take the hottest queued block, which a channel cannot pick, and citrine-gb-threads has no queue ordered by heat"
)]

use super::arena::{Linked, relocate};
use super::emit::{Emitted, Emitter};
use super::scan::{Op, Plan};
use super::{CodeRam, CodeSpan};
use crate::jit_cache::{JitCache, Record};
use citrine_gb_threads::Spawner;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

pub type Emit = fn(&mut Emitter, &Plan) -> Option<Emitted>;

pub struct Scan {
    pub pc: u32,
    pub thumb: bool,
    pub words: Box<[u32]>,
    pub spans: Box<[CodeSpan]>,
    pub ops: Box<[Op]>,
    pub fetch: Box<[[u32; 2]]>,
    pub pages: Box<[(CodeRam, usize)]>,
    pub gens: Box<[u32]>,
}

pub enum Outcome {
    Code(Linked),
    Refused,
    Broken,
}

pub struct Job {
    pub scan: Scan,
    pub tag: u32,
    pub emit: Emit,
    pub table: Box<[usize]>,
    pub disk: Option<(Arc<JitCache>, u64)>,
    pub cached: Option<Arc<Record>>,
}

pub struct Done {
    pub scan: Scan,
    pub tag: u32,
    pub outcome: Outcome,
    pub nanos: u64,
    pub loaded: bool,
    pub mismatched: bool,
}

pub type Heat = Arc<AtomicU32>;

struct Sent {
    job: Job,
    reply: Sender<Done>,
}

struct Pending<T> {
    sent: Vec<(Heat, T)>,
    lanes: usize,
}

struct Queue<T> {
    pending: Mutex<Pending<T>>,
    wake: Condvar,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            pending: Mutex::new(Pending {
                sent: Vec::new(),
                lanes: 0,
            }),
            wake: Condvar::new(),
        }
    }
}

impl<T> Queue<T> {
    fn push(&self, heat: Heat, item: T) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.sent.push((heat, item));
            self.wake.notify_one();
        }
    }

    fn hottest(&self) -> Option<T> {
        let mut pending = self.pending.lock().ok()?;
        loop {
            let hottest = pending
                .sent
                .iter()
                .enumerate()
                .max_by_key(|(at, (heat, _))| (heat.load(Ordering::Relaxed), usize::MAX - at))
                .map(|(at, _)| at);
            if let Some(at) = hottest {
                return Some(pending.sent.remove(at).1);
            }
            if pending.lanes == 0 {
                return None;
            }
            pending = self.wake.wait(pending).ok()?;
        }
    }

    fn join(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.lanes += 1;
        }
    }

    fn leave(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.lanes -= 1;
            if pending.lanes == 0 {
                pending.sent.clear();
                self.wake.notify_all();
            }
        }
    }
}

pub struct Lane {
    workers: usize,
    queue: Arc<Queue<Sent>>,
    reply: Sender<Done>,
    done: Receiver<Done>,
}

impl Lane {
    pub fn start(spawner: &Spawner, workers: usize) -> Self {
        let workers = workers.max(1);
        let lane = Self::over(Arc::new(Queue::default()), workers);
        for _ in 0..workers {
            let queue = Arc::clone(&lane.queue);
            spawner.thread("jit", move || {
                #[cfg(target_vendor = "apple")]
                unsafe {
                    libc::pthread_set_qos_class_self_np(
                        libc::qos_class_t::QOS_CLASS_USER_INITIATED,
                        0,
                    );
                }
                #[cfg(any(target_os = "linux", target_os = "android"))]
                unsafe {
                    libc::setpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t, 10);
                }
                #[cfg(windows)]
                unsafe {
                    use windows_sys::Win32::System::Threading::{
                        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
                    };
                    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
                }
                serve(&queue)
            });
        }
        lane
    }

    fn over(queue: Arc<Queue<Sent>>, workers: usize) -> Self {
        queue.join();
        let (reply, done) = channel();
        Self {
            workers,
            queue,
            reply,
            done,
        }
    }

    pub fn fork(&self) -> Self {
        Self::over(Arc::clone(&self.queue), self.workers)
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    pub fn send(&self, job: Job, heat: Heat) {
        let reply = self.reply.clone();
        self.queue.push(heat, Sent { job, reply });
    }

    pub fn try_take(&self) -> Option<Done> {
        self.done.try_recv().ok()
    }

    pub fn take(&self) -> Option<Done> {
        self.done.recv().ok()
    }
}

impl Drop for Lane {
    fn drop(&mut self) {
        self.queue.leave();
    }
}

fn serve(queue: &Queue<Sent>) {
    let mut emitter = None;
    while let Some(sent) = queue.hottest() {
        let done = citrine_gb_threads::work("compile", || compile(&mut emitter, sent.job));
        let _ = sent.reply.send(done);
    }
}

pub fn compile(emitter: &mut Option<Emitter>, job: Job) -> Done {
    let began = Instant::now();
    let (outcome, loaded) = emit(emitter, &job);
    Done {
        mismatched: job.cached.is_some() && !loaded,
        scan: job.scan,
        tag: job.tag,
        outcome,
        nanos: began.elapsed().as_nanos() as u64,
        loaded,
    }
}

fn emit(emitter: &mut Option<Emitter>, job: &Job) -> (Outcome, bool) {
    if emitter.is_none() {
        *emitter = Emitter::new();
    }
    let Some(built) = emitter.as_mut() else {
        return (Outcome::Broken, false);
    };
    let scan = &job.scan;
    let plan = Plan {
        pc: scan.pc,
        thumb: scan.thumb,
        words: &scan.words,
        ops: &scan.ops,
        fetch: &scan.fetch,
    };
    let emitted = match catch_unwind(AssertUnwindSafe(|| (job.emit)(built, &plan))) {
        Ok(Some(emitted)) => emitted,
        Ok(None) => return (Outcome::Refused, false),
        Err(_) => {
            *emitter = None;
            return (Outcome::Refused, false);
        }
    };
    let same = job
        .cached
        .as_ref()
        .is_some_and(|record| *emitted.code == *record.code && *emitted.relocs == *record.relocs);
    if !same
        && emitted.portable
        && let Some((disk, cpu)) = &job.disk
    {
        disk.keep(Record::new(
            *cpu,
            super::key(scan.pc, scan.thumb),
            &scan.words,
            &scan.fetch,
            &emitted.code,
            &emitted.relocs,
        ));
    }
    match relocate(&emitted.code, &emitted.relocs, &job.table) {
        Some(bytes) => (Outcome::Code(bytes), same),
        None => (Outcome::Refused, false),
    }
}

#[cfg(test)]
mod tests {
    use super::{Heat, Queue};
    use std::sync::atomic::Ordering;

    #[test]
    fn the_most_wanted_block_compiles_first_and_ties_go_in_order() {
        let queue = Queue::default();
        queue.join();
        let heats: Vec<Heat> = (0..4).map(|_| Heat::default()).collect();
        for (at, heat) in heats.iter().enumerate() {
            queue.push(heat.clone(), at);
        }
        heats[2].fetch_add(5, Ordering::Relaxed);
        heats[3].fetch_add(1, Ordering::Relaxed);
        let order: Vec<usize> = (0..4).filter_map(|_| queue.hottest()).collect();
        assert_eq!(order, [2, 3, 0, 1]);
    }

    #[test]
    fn workers_stop_once_every_lane_is_gone() {
        let queue = Queue::<u32>::default();
        queue.join();
        queue.push(Heat::default(), 7);
        queue.leave();
        assert_eq!(queue.hottest(), None);
    }
}
