use super::device::device_name;
use super::{Op, Remote};
use crate::online::Online;
use citrine_gb_net::{ClientMsg, GameKey, Keys, SaveInfo, SaveLabel, SavePayload};
use std::collections::BTreeSet;

const GAME: &[u8] = b"pokewalker";
const SLOT: &str = "walker";
const DATA: &str = "walker";

pub fn walker_key(keys: &Keys) -> GameKey {
    keys.game_key(GAME)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkerRemote {
    pub version: u64,
    pub device: String,
    pub hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalkerRequest {
    Fetch,
    Put { base: u64, data: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalkerReply {
    Listed(Option<WalkerRemote>),
    Fetched { remote: WalkerRemote, data: Vec<u8> },
    Unreadable(WalkerRemote),
    Stored(WalkerRemote),
    Refused { changed: bool },
    Lost,
}

#[derive(Default)]
pub(super) struct WalkerSlot {
    active: bool,
    queued: Option<WalkerRequest>,
    out: bool,
    replies: Vec<WalkerReply>,
    listed: Option<Option<WalkerRemote>>,
    unreadable: BTreeSet<u64>,
}

impl WalkerSlot {
    pub(super) fn is_active(&self) -> bool {
        self.active
    }

    pub(super) fn reset(&mut self) {
        if self.out || self.queued.is_some() {
            self.replies.push(WalkerReply::Lost);
        }
        self.out = false;
        self.queued = None;
        self.listed = None;
        self.unreadable.clear();
    }

    fn answered(&mut self, reply: WalkerReply) {
        self.out = false;
        self.replies.push(reply);
    }
}

fn remote_of(remote: &Remote) -> WalkerRemote {
    WalkerRemote {
        version: remote.info.version,
        device: remote
            .label
            .as_ref()
            .map(|label| label.device.clone())
            .unwrap_or_default(),
        hash: remote.label.as_ref().map_or([0; 32], |label| label.hash),
    }
}

impl Online {
    pub fn walker_sync(&mut self, on: bool) {
        let slot = &mut self.cloud.walker;
        if slot.active && !on {
            slot.reset();
            slot.replies.clear();
        }
        slot.active = on;
    }

    pub fn walker_ready(&self) -> bool {
        let slot = &self.cloud.walker;
        slot.active
            && !slot.out
            && slot.queued.is_none()
            && slot.listed.is_some()
            && self.cloud.listing.is_some()
    }

    pub fn walker_send(&mut self, request: WalkerRequest) {
        self.cloud.walker.queued = Some(request);
    }

    pub fn walker_replies(&mut self) -> Vec<WalkerReply> {
        let mut replies = std::mem::take(&mut self.cloud.walker.replies);
        if let (Some(keys), Some(listing)) = (self.keys.as_ref(), self.cloud.listing.as_ref()) {
            let now = listing
                .get(&(walker_key(keys), SLOT.to_string()))
                .map(remote_of);
            let slot = &mut self.cloud.walker;
            if slot.listed.as_ref() != Some(&now) {
                slot.listed = Some(now.clone());
                replies.push(WalkerReply::Listed(now));
            }
        }
        replies
    }

    pub(super) fn walker_pump(&mut self) -> bool {
        if !self.cloud.walker.active {
            return false;
        }
        let Some(keys) = self.keys.as_ref() else {
            return false;
        };
        let Some(request) = self.cloud.walker.queued.take() else {
            return false;
        };
        let game = walker_key(keys);
        match request {
            WalkerRequest::Fetch => {
                let unreadable = self
                    .cloud
                    .listing
                    .as_ref()
                    .and_then(|listing| listing.get(&(game, SLOT.to_string())))
                    .map(remote_of)
                    .filter(|remote| self.cloud.walker.unreadable.contains(&remote.version));
                if let Some(remote) = unreadable {
                    self.cloud
                        .walker
                        .replies
                        .push(WalkerReply::Unreadable(remote));
                    return false;
                }
                self.cloud.parts.clear();
                self.send(ClientMsg::GetSave {
                    game,
                    slot: SLOT.to_string(),
                });
                self.cloud.start(Op::Get {
                    game,
                    slot: SLOT.to_string(),
                });
            }
            WalkerRequest::Put { base, data } => {
                let payload = SavePayload {
                    files: vec![(DATA.to_string(), data)],
                };
                let label = SaveLabel {
                    game: String::new(),
                    name: String::new(),
                    device: device_name(),
                    saved_at: crate::storage::now_unix(),
                    hash: payload.hash(),
                };
                let sealed_label = keys.seal_label(&label);
                let data = keys.seal_save(&payload);
                let remote = self
                    .cloud
                    .listing
                    .as_ref()
                    .and_then(|listing| listing.get(&(game, SLOT.to_string())));
                if !self.fits_cloud(data.len() + sealed_label.len())
                    || self
                        .cloud
                        .doomed(data.len(), sealed_label.len(), remote)
                        .is_some()
                {
                    self.cloud
                        .walker
                        .replies
                        .push(WalkerReply::Refused { changed: false });
                    return false;
                }
                let tag = self.cloud.tag();
                self.put_save(tag, game, SLOT.to_string(), base, sealed_label, data);
                self.cloud.start(Op::WalkerPut { tag, label });
            }
        }
        self.cloud.walker.out = true;
        true
    }

    pub(super) fn walker_stored(&mut self, info: SaveInfo, label: SaveLabel) {
        let remote = Remote {
            info,
            label: Some(label),
        };
        let stored = remote_of(&remote);
        self.cloud_list_insert(remote);
        self.cloud.walker.answered(WalkerReply::Stored(stored));
    }

    pub(super) fn walker_refused(&mut self, changed: bool) {
        self.cloud.walker.answered(WalkerReply::Refused { changed });
    }

    pub(super) fn walker_lost(&mut self) {
        self.cloud.walker.answered(WalkerReply::Lost);
    }

    pub(super) fn walker_fetched(&mut self, remote: Remote, data: &[u8]) {
        let walker = remote_of(&remote);
        self.cloud_list_insert(remote);
        let opened = self.keys.as_ref().map(|keys| keys.open_save(data));
        let data = match opened {
            Some(Ok(payload)) => payload.file(DATA).map(<[u8]>::to_vec),
            _ => None,
        };
        let reply = match data {
            Some(data) => WalkerReply::Fetched {
                remote: walker,
                data,
            },
            None => {
                log::warn!("the cloud walker could not be opened");
                self.cloud.walker.unreadable.insert(walker.version);
                WalkerReply::Unreadable(walker)
            }
        };
        self.cloud.walker.answered(reply);
    }
}
