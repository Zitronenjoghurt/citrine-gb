use crate::gauge::{self, Board, Kind, State as Gauged};
use crate::{Mode, Rng, guarded, side_threads};
use std::sync::mpsc::{self, Receiver, TryRecvError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Panicked(pub String);

type Task<T> = Box<dyn FnOnce() -> T + Send>;

pub struct Pending<T> {
    state: State<T>,
    rng: Option<Rng>,
}

enum State<T> {
    Running(Receiver<Result<T, Panicked>>),
    Later(&'static str, Task<T>),
    Taken,
}

impl<T: Send + 'static> Pending<T> {
    pub(crate) fn start(
        mode: Mode,
        rng: Option<Rng>,
        board: &Board,
        name: &'static str,
        task: impl FnOnce() -> T + Send + 'static,
    ) -> Self {
        let state = if side_threads(mode) {
            let (done, result) = mpsc::channel();
            let gauge = board.gauge(name, Kind::Background);
            gauge.set(Gauged::Working);
            std::thread::Builder::new()
                .name(name.to_string())
                .spawn(move || {
                    let start = gauge.clock();
                    let outcome = gauge::wear(&gauge, || guarded(name, task));
                    gauge.handled(start, None);
                    gauge.set(Gauged::Ended);
                    let _ = done.send(outcome.map_err(Panicked));
                })
                .expect("a background thread starts");
            State::Running(result)
        } else {
            State::Later(name, Box::new(task))
        };
        Self { state, rng }
    }
}

impl<T> Pending<T> {
    pub fn try_take(&mut self) -> Option<Result<T, Panicked>> {
        match std::mem::replace(&mut self.state, State::Taken) {
            State::Running(result) => match result.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => {
                    self.state = State::Running(result);
                    None
                }
                Err(TryRecvError::Disconnected) => None,
            },
            State::Later(name, task) => {
                if let Some(mut rng) = self.rng.take()
                    && rng.below(2) == 0
                {
                    self.state = State::Later(name, task);
                    return None;
                }
                Some(guarded(name, task).map_err(Panicked))
            }
            State::Taken => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Mode, Panicked, Threads};
    use std::time::{Duration, Instant};

    fn taken<T>(pending: &mut crate::Pending<T>) -> Result<T, Panicked> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(result) = pending.try_take() {
                return result;
            }
            assert!(Instant::now() < deadline, "timed out");
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_background_task_hands_its_result_back_once_in_every_mode() {
        for mode in [Mode::Threads, Mode::Inline, Mode::Shuffled(4)] {
            let threads = Threads::new(mode);
            let mut pending = threads.background("folder scan", || 6 * 7);
            assert_eq!(taken(&mut pending), Ok(42), "{mode:?}");
            assert_eq!(pending.try_take(), None);
        }
    }

    #[test]
    fn a_panicking_background_task_reports_panicked_with_its_name() {
        for mode in [Mode::Threads, Mode::Inline] {
            let threads = Threads::new(mode);
            let mut pending = threads.background("update check", || -> u8 { panic!("offline") });
            assert_eq!(
                taken(&mut pending),
                Err(Panicked("update check: offline".to_string())),
                "{mode:?}"
            );
        }
    }
}
