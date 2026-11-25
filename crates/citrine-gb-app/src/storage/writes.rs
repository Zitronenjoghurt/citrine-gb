use super::SaveStore;
use citrine_gb::persistence::battery_save::BatterySave;
use citrine_gb::persistence::state::StateBody;
use citrine_gb_threads::{Cx, Job, Mode, Status, Threads, Worker};

#[cfg(not(target_arch = "wasm32"))]
const SETTLE_FOR: std::time::Duration = std::time::Duration::from_secs(5);

pub enum Write {
    Battery {
        store: SaveStore,
        key: String,
        save: BatterySave,
    },
    BatteryFile {
        store: SaveStore,
        key: String,
        ram: Vec<u8>,
    },
    Autosave {
        store: SaveStore,
        key: String,
        state: StateBody,
        frame: Vec<u8>,
        keep: usize,
    },
    Snapshot {
        store: SaveStore,
        key: String,
        slot: usize,
        state: StateBody,
        frame: Vec<u8>,
        progress: Option<Vec<u8>>,
    },
    #[cfg(feature = "nds")]
    JitCache(std::sync::Arc<citrine_gb::nds::machine::JitCache>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Written {
    Battery,
    Autosave(usize),
    Snapshot(usize),
    Cache,
}

pub type Outcome = Result<Written, String>;

impl Write {
    fn run(self) -> Outcome {
        match self {
            Self::Battery { store, key, save } => store
                .store_battery_save(&key, &save)
                .map(|()| Written::Battery)
                .map_err(|err| format!("Saving the game failed: {err}")),
            Self::BatteryFile { store, key, ram } => store
                .store_battery(&key, &ram)
                .map(|()| Written::Battery)
                .map_err(|err| format!("Saving the game failed: {err}")),
            Self::Autosave {
                store,
                key,
                state,
                frame,
                keep,
            } => pack(&state)
                .and_then(|packed| {
                    let keep = crate::autosave::keep_for(state.kind, keep);
                    store.store_autosave(&key, &packed, &frame, state.kind, keep)
                })
                .map(Written::Autosave)
                .map_err(|err| format!("Autosave failed: {err}")),
            Self::Snapshot {
                store,
                key,
                slot,
                state,
                frame,
                progress,
            } => pack(&state)
                .and_then(|packed| {
                    store.store_snapshot(&key, slot, &packed, &frame, state.kind)?;
                    store.store_snapshot_progress(&key, slot, &packed, progress.as_deref())
                })
                .map(|()| Written::Snapshot(slot))
                .map_err(|err| format!("Saving the state failed: {err}")),
            #[cfg(feature = "nds")]
            Self::JitCache(cache) => {
                crate::emulator::jit_cache::store(&cache);
                Ok(Written::Cache)
            }
        }
    }
}

fn pack(state: &StateBody) -> std::io::Result<Vec<u8>> {
    state
        .pack(state.quality(!cfg!(target_arch = "wasm32")))
        .map_err(|err| std::io::Error::other(format!("{err:?}")))
}

pub struct SaveJob;

impl Job for SaveJob {
    type Msg = Write;
    type Reply = Outcome;

    fn handle(&mut self, write: Write, cx: &mut Cx<'_, Outcome>) {
        cx.reply(write.run());
    }
}

pub struct Writes {
    worker: Worker<SaveJob>,
    queued: usize,
    news: Vec<Outcome>,
}

impl Default for Writes {
    fn default() -> Self {
        Self::start(&Threads::new(Mode::Inline))
    }
}

impl Writes {
    pub fn start(threads: &Threads) -> Self {
        let worker = if cfg!(all(threaded, target_arch = "wasm32")) {
            Threads::new(Mode::Inline).start("saves", SaveJob)
        } else {
            threads.start("saves", SaveJob)
        };
        Self {
            worker,
            queued: 0,
            news: Vec::new(),
        }
    }

    pub fn send(&mut self, write: Write) {
        if self.worker.status() != Status::Running {
            log::error!("the saves worker ended, writing on this thread");
            self.news.push(write.run());
            return;
        }
        self.queued += 1;
        self.worker.send(write);
        self.collect();
    }

    pub fn take_news(&mut self) -> Vec<Outcome> {
        self.collect();
        std::mem::take(&mut self.news)
    }

    pub fn settle(&mut self) {
        self.collect();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let until = web_time::Instant::now() + SETTLE_FOR;
            while self.queued > 0
                && self.worker.status() == Status::Running
                && web_time::Instant::now() < until
            {
                std::thread::sleep(std::time::Duration::from_millis(1));
                self.collect();
            }
        }
    }

    pub fn is_idle(&mut self) -> bool {
        self.collect();
        self.queued == 0
    }

    fn collect(&mut self) {
        let arrived: Vec<Outcome> = self.worker.replies().collect();
        self.queued = self.queued.saturating_sub(arrived.len());
        self.news.extend(arrived);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use citrine_gb::console::ConsoleKind;

    const KEY: &str = "GAME";

    fn root(name: &str) -> std::path::PathBuf {
        let root = crate::storage::scratch_dir()
            .join(format!("citrine-writes-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&root);
        root
    }

    fn state() -> StateBody {
        StateBody {
            kind: ConsoleKind::Gb,
            data: b"state".to_vec(),
        }
    }

    fn snapshot(store: &SaveStore, slot: usize) -> Write {
        Write::Snapshot {
            store: store.clone(),
            key: KEY.into(),
            slot,
            state: state(),
            frame: vec![0; 160 * 144 * 4],
            progress: None,
        }
    }

    #[test]
    fn every_mode_writes_the_same_files_and_reports_them_in_order() {
        for (n, mode) in [Mode::Inline, Mode::Threads, Mode::Shuffled(7)]
            .into_iter()
            .enumerate()
        {
            let root = root(&format!("modes-{n}"));
            let store = SaveStore::at(root.clone());
            let threads = Threads::new(mode);
            let mut writes = Writes::start(&threads);
            writes.send(Write::BatteryFile {
                store: store.clone(),
                key: KEY.into(),
                ram: vec![1; 64],
            });
            writes.send(snapshot(&store, 1));
            writes.send(Write::Autosave {
                store: store.clone(),
                key: KEY.into(),
                state: state(),
                frame: vec![0; 160 * 144 * 4],
                keep: 3,
            });
            writes.settle();

            assert!(writes.is_idle(), "{mode:?}");
            assert_eq!(
                writes.take_news(),
                [
                    Ok(Written::Battery),
                    Ok(Written::Snapshot(1)),
                    Ok(Written::Autosave(0))
                ],
                "{mode:?}"
            );
            assert_eq!(store.load_battery(KEY), Some(vec![1; 64]));
            let packed = store.load_snapshot(KEY, 1).expect("snapshot");
            assert_eq!(
                super::super::decompress(&packed).as_deref(),
                Some(&b"state"[..])
            );
            assert_eq!(store.autosave_ids(KEY), vec![0]);
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn a_failed_write_comes_back_as_news_naming_what_failed() {
        let root = root("failed");
        std::fs::write(&root, b"a file where the folder goes").expect("file");
        let store = SaveStore::at(root.clone());
        let threads = Threads::new(Mode::Threads);
        let mut writes = Writes::start(&threads);
        writes.send(snapshot(&store, 0));
        writes.settle();
        let news = writes.take_news();
        assert!(
            matches!(&news[..], [Err(err)] if err.starts_with("Saving the state failed")),
            "{news:?}"
        );
        let _ = std::fs::remove_file(root);
    }
}
