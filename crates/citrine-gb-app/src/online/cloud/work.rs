use super::CloudState;
use super::server::save_frames;
use crate::storage::cloud::LocalItem;
use citrine_gb_net::{GameKey, Keys, SaveInfo, SavePayload, Tag};
use citrine_gb_threads::Spawner;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use web_time::Instant;

pub(super) struct Scanned {
    pub(super) item: LocalItem,
    pub(super) hash: [u8; 32],
}

pub(super) struct Scan {
    rom: String,
    stale: bool,
    done: Receiver<Vec<Scanned>>,
}

pub(super) struct Sealing {
    tag: Tag,
    upload: Upload,
    done: Receiver<Sealed>,
}

pub(super) struct Sealed {
    pub(super) len: usize,
    pub(super) frames: Vec<Vec<u8>>,
}

#[derive(Clone)]
pub(super) struct Upload {
    pub(super) game: GameKey,
    pub(super) slot: String,
    pub(super) base: u64,
    pub(super) label: Vec<u8>,
}

pub(super) type Opened = Option<(SavePayload, [u8; 32])>;

pub(super) struct Opening {
    info: SaveInfo,
    done: Receiver<Opened>,
}

pub(super) enum Ready<T> {
    Waiting,
    Done(T),
    Lost,
}

fn run<T: Send + 'static>(
    spawner: Option<&Spawner>,
    name: &'static str,
    task: impl FnOnce() -> T + Send + 'static,
) -> Receiver<T> {
    let (done, result) = mpsc::channel();
    let finish = move || {
        let _ = done.send(task());
    };
    match spawner {
        Some(spawner) => spawner.run(name, finish),
        None => finish(),
    }
    result
}

fn ready<T>(done: &Receiver<T>) -> Ready<T> {
    match done.try_recv() {
        Ok(value) => Ready::Done(value),
        Err(TryRecvError::Empty) => Ready::Waiting,
        Err(TryRecvError::Disconnected) => Ready::Lost,
    }
}

impl CloudState {
    pub(super) fn scanned(&mut self, rom: &str) -> Option<Vec<Scanned>> {
        if let Some(scan) = self.scan.take() {
            match ready(&scan.done) {
                Ready::Waiting => {
                    self.scan = Some(scan);
                    return None;
                }
                Ready::Done(items) if scan.rom == rom => {
                    if scan.stale {
                        self.rescan.insert(scan.rom);
                    }
                    return Some(items);
                }
                Ready::Done(_) | Ready::Lost => {}
            }
        }
        let store = self.store.clone()?;
        let device = self.device();
        let key = rom.to_string();
        let done = run(self.spawner.as_ref(), "cloud scan", move || {
            store
                .local_items(&key, &device)
                .into_iter()
                .map(|item| Scanned {
                    hash: item.payload.hash(),
                    item,
                })
                .collect()
        });
        match ready(&done) {
            Ready::Done(items) => Some(items),
            Ready::Waiting => {
                self.scan = Some(Scan {
                    rom: rom.to_string(),
                    stale: false,
                    done,
                });
                None
            }
            Ready::Lost => None,
        }
    }

    pub(super) fn rescan_if_stale(&mut self, rom: &str, now: Instant) {
        if self.rescan.remove(rom) {
            self.due.insert(rom.to_string(), now);
        }
    }

    pub(super) fn touched(&mut self, rom: &str) {
        if let Some(scan) = self.scan.as_mut()
            && scan.rom == rom
        {
            scan.stale = true;
        }
    }

    pub(super) fn seal(&mut self, tag: Tag, upload: Upload, keys: &Keys, payload: SavePayload) {
        let keys = keys.clone();
        let job = upload.clone();
        let done = run(self.spawner.as_ref(), "cloud seal", move || {
            let data = keys.seal_save(&payload);
            Sealed {
                len: data.len(),
                frames: save_frames(tag, job, data),
            }
        });
        self.sealing = Some(Sealing { tag, upload, done });
    }

    pub(super) fn sealed(&mut self) -> Option<(Tag, Upload, Option<Sealed>)> {
        let sealing = self.sealing.take()?;
        match ready(&sealing.done) {
            Ready::Waiting => {
                self.sealing = Some(sealing);
                None
            }
            Ready::Done(sealed) => Some((sealing.tag, sealing.upload, Some(sealed))),
            Ready::Lost => Some((sealing.tag, sealing.upload, None)),
        }
    }

    pub(super) fn open(&mut self, info: SaveInfo, keys: Option<&Keys>, data: Vec<u8>) {
        let keys = keys.cloned();
        let done = run(self.spawner.as_ref(), "cloud open", move || {
            let payload = keys?.open_save(&data).ok()?;
            let hash = payload.hash();
            Some((payload, hash))
        });
        self.opening = Some(Opening { info, done });
    }

    pub(super) fn opened(&mut self) -> Option<(SaveInfo, Opened)> {
        let opening = self.opening.take()?;
        match ready(&opening.done) {
            Ready::Waiting => {
                self.opening = Some(opening);
                None
            }
            Ready::Done(opened) => Some((opening.info, opened)),
            Ready::Lost => Some((opening.info, None)),
        }
    }

    pub(super) fn drop_work(&mut self) {
        self.scan = None;
        self.rescan.clear();
        self.sealing = None;
        self.sending = None;
        self.opening = None;
    }
}
