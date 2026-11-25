use std::future::Future;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use tokio::task::{AbortHandle, JoinSet};

pub(crate) struct Tasks(Mutex<Option<JoinSet<()>>>);

impl Default for Tasks {
    fn default() -> Self {
        Self(Mutex::new(Some(JoinSet::new())))
    }
}

impl Tasks {
    fn set(&self) -> MutexGuard<'_, Option<JoinSet<()>>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) -> AbortHandle {
        if let Some(set) = self.set().as_mut() {
            while set.try_join_next().is_some() {}
            return set.spawn(task);
        }
        let late = tokio::spawn(task);
        late.abort();
        late.abort_handle()
    }

    pub(crate) async fn end(&self, grace: Duration) {
        let Some(mut set) = self.set().take() else {
            return;
        };
        let drained =
            tokio::time::timeout(grace, async { while set.join_next().await.is_some() {} }).await;
        if drained.is_err() {
            set.abort_all();
            while set.join_next().await.is_some() {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn end_waits_for_tasks_and_stops_the_ones_past_the_grace() {
        let tasks = Tasks::default();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        tasks.spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = done_tx.send(());
        });
        let (stuck_tx, stuck_rx) = tokio::sync::oneshot::channel::<()>();
        tasks.spawn(async move {
            let _held = stuck_tx;
            std::future::pending::<()>().await;
        });
        tasks.end(Duration::from_millis(200)).await;
        assert!(done_rx.await.is_ok(), "the quick task ran to its end");
        assert!(stuck_rx.await.is_err(), "the stuck task was stopped");
        let (late_tx, late_rx) = tokio::sync::oneshot::channel::<()>();
        tasks.spawn(async move {
            let _ = late_tx.send(());
        });
        assert!(late_rx.await.is_err(), "nothing starts after end");
    }
}
