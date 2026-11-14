use super::engine::Engine;
use citrine_gb_threads::Claim;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

const BATCH: usize = 1024;

struct Back {
    engine: Box<Engine>,
    batches: Receiver<Vec<u32>>,
}

impl Back {
    fn catch_up(&mut self) {
        for batch in self.batches.try_iter() {
            self.engine.run(&batch);
        }
    }
}

pub(super) struct Link {
    back: Arc<Claim<Back>>,
    batches: Sender<Vec<u32>>,
    batch: Vec<u32>,
    nudge: Box<dyn Fn() + Send>,
}

pub struct Worker(Arc<Claim<Back>>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Ran,
    Idle,
    Busy,
}

pub(super) fn lend(engine: Box<Engine>, nudge: Box<dyn Fn() + Send>) -> (Link, Worker) {
    let (batches, received) = mpsc::channel();
    let back = Arc::new(Claim::new(Back {
        engine,
        batches: received,
    }));
    let link = Link {
        back: Arc::clone(&back),
        batches,
        batch: Vec::with_capacity(BATCH * 2),
        nudge,
    };
    (link, Worker(back))
}

impl Link {
    pub(super) fn post(&mut self, command: u8, params: &[u32]) {
        self.batch
            .push(u32::from(command) | (params.len() as u32) << 8);
        self.batch.extend_from_slice(params);
        if self.batch.len() >= BATCH {
            self.ship();
            (self.nudge)();
        }
    }

    pub(super) fn with<R>(&mut self, f: impl FnOnce(&mut Engine) -> R) -> R {
        self.ship();
        let mut f = Some(f);
        loop {
            let done = self.back.try_with(|back| {
                back.catch_up();
                f.take().map(|f| f(&mut back.engine))
            });
            if let Some(Some(result)) = done {
                return result;
            }
            std::hint::spin_loop();
        }
    }

    pub(super) fn come_home(mut self) -> Box<Engine> {
        self.ship();
        let Self { back, .. } = self;
        let mut back = Arc::into_inner(back)
            .expect("the geometry worker ends with its frame")
            .into_inner();
        back.catch_up();
        back.engine
    }

    fn ship(&mut self) {
        if !self.batch.is_empty() {
            let batch = std::mem::replace(&mut self.batch, Vec::with_capacity(BATCH * 2));
            let _ = self.batches.send(batch);
        }
    }
}

impl Worker {
    pub fn step(&self) -> Step {
        self.0
            .try_with(|back| match back.batches.try_recv() {
                Ok(batch) => {
                    back.engine.run(&batch);
                    Step::Ran
                }
                Err(_) => Step::Idle,
            })
            .unwrap_or(Step::Busy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MTX_IDENTITY: u8 = 0x15;

    #[test]
    fn the_worker_gets_the_commands_once_a_batch_fills() {
        let (mut link, worker) = lend(Box::default(), Box::new(|| {}));
        for _ in 1..BATCH {
            link.post(MTX_IDENTITY, &[]);
        }
        assert_eq!(worker.step(), Step::Idle);
        link.post(MTX_IDENTITY, &[]);
        assert_eq!(worker.step(), Step::Ran);
        assert_eq!(worker.step(), Step::Idle);
    }
}
