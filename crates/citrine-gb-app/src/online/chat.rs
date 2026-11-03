use super::{Online, Phase};
use crate::storage::Backend;
use citrine_gb_net::{
    Body, CHUNK_BYTES, ChatMessage, ClientMsg, Edit, Envelope, ErrorCode, FileId, FileInfo,
    FriendState, MAX_PALETTE_BYTES, MAX_RECEIPT_IDS, MAX_TEXT_CHARS, MessageId, OnlineId, Receipt,
    STORAGE_BLOCK, ServerMsg, Share, ShareKind, Tag,
};
use history::{decode_history, encode_history, history_name, is_history_name};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use web_time::Instant;

pub mod history;

const NAMESPACE: &str = "chats";
const SAVE_EVERY: Duration = Duration::from_secs(1);
const RECEIPT_EVERY: Duration = Duration::from_secs(1);
const UPLOAD_PER_SECOND: f64 = 3.0 * 1024.0 * 1024.0;
const UPLOAD_BURST: f64 = 3.0 * 1024.0 * 1024.0;
const CHATS_PER_SECOND: f64 = 4.0;
const CHAT_BURST: f64 = 20.0;
const RATE_LIMITED_PAUSE: Duration = Duration::from_secs(2);
const NO_FILE: FileId = FileId([0; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Failure {
    NotConnected,
    Refused(ErrorCode),
    Gone,
}

impl Failure {
    pub fn text(self) -> &'static str {
        match self {
            Failure::NotConnected => "Not sent, you're offline",
            Failure::Refused(ErrorCode::QuotaFull) => "Not sent, your storage is full",
            Failure::Refused(ErrorCode::TooLarge) => "Not sent, too large for this server",
            Failure::Refused(ErrorCode::SharingOff) => "Not sent, this server doesn't keep files",
            Failure::Refused(ErrorCode::NotFriends) => "Not sent, you aren't friends anymore",
            Failure::Refused(ErrorCode::RateLimited) => "Not sent, sending too fast",
            Failure::Refused(ErrorCode::InboxFull) => "Not sent, their inbox is full",
            Failure::Refused(_) => "Not sent, the server refused it",
            Failure::Gone => "Not sent. Share it again.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Delivery {
    Sending,
    Sent,
    Failed(Failure),
    Received,
    Delivered(u64),
    Read(u64),
    Seen,
}

impl Delivery {
    fn rank(self) -> u8 {
        match self {
            Delivery::Sent => 1,
            Delivery::Delivered(_) => 2,
            Delivery::Read(_) => 3,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Change {
    #[default]
    Kept,
    Edited {
        told: bool,
    },
    Deleted {
        told: bool,
    },
}

impl Change {
    pub fn untold(self) -> bool {
        matches!(
            self,
            Change::Edited { told: false } | Change::Deleted { told: false }
        )
    }

    fn told(self) -> Self {
        match self {
            Change::Edited { .. } => Change::Edited { told: true },
            Change::Deleted { .. } => Change::Deleted { told: true },
            Change::Kept => Change::Kept,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatEntry {
    pub id: u64,
    pub mine: bool,
    pub at: u64,
    pub body: Body,
    pub delivery: Delivery,
    pub change: Change,
}

impl ChatEntry {
    pub fn is_deleted(&self) -> bool {
        matches!(self.change, Change::Deleted { .. })
    }

    pub fn is_edited(&self) -> bool {
        matches!(self.change, Change::Edited { .. })
    }

    fn reached_relay(&self, sending: bool) -> bool {
        sending || self.delivery.rank() > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Out {
    Entry(OnlineId, u64),
    Change(OnlineId, u64),
}

impl Out {
    fn friend(self) -> OnlineId {
        match self {
            Self::Entry(friend, _) | Self::Change(friend, _) => friend,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub entries: Vec<ChatEntry>,
    pub unread: usize,
    pub relayed: BTreeMap<u64, MessageId>,
}

#[derive(Debug, Clone)]
pub struct NewShare {
    pub kind: ShareKind,
    pub data: Vec<u8>,
    pub game: String,
    pub rom_sha256: Option<String>,
    pub created_at: u64,
    pub width: u32,
    pub height: u32,
    pub length_ms: Option<u64>,
    pub preview: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    Loading { done: u64, total: u64 },
    Ready(Vec<u8>),
    Failed(&'static str),
}

struct Upload {
    to: OnlineId,
    id: u64,
    bytes: Vec<u8>,
    sent: usize,
    ready: bool,
}

struct Download {
    peer: OnlineId,
    size: u64,
    data: Vec<u8>,
}

#[derive(Default)]
pub struct ChatState {
    chats: BTreeMap<OnlineId, Conversation>,
    owner: Option<String>,
    next_tag: Tag,
    sending: HashMap<Tag, Out>,
    uploads: HashMap<Tag, Upload>,
    held: HashMap<u64, Vec<u8>>,
    downloads: HashMap<FileId, Download>,
    fetched: HashMap<FileId, Fetch>,
    acks: Vec<MessageId>,
    receipts: BTreeMap<OnlineId, Receipt>,
    receipts_at: Option<Instant>,
    files: Option<Vec<FileInfo>>,
    store: Option<Backend>,
    sent: u32,
    dirty: bool,
    saved_at: Option<Instant>,
    budget: f64,
    budget_at: Option<Instant>,
    queued: BTreeMap<i64, Out>,
    change_failures: HashMap<u64, Failure>,
    changes_due: bool,
    paced: bool,
    chat_tokens: f64,
    chat_tokens_at: Option<Instant>,
    paused_until: Option<Instant>,
}

fn take_first(ids: &mut Vec<u64>) -> Vec<u64> {
    let count = ids.len().min(MAX_RECEIPT_IDS);
    ids.drain(..count).collect()
}

impl ChatState {
    fn tag(&mut self) -> Tag {
        self.next_tag = self.next_tag.wrapping_add(1);
        self.next_tag
    }

    fn entry_mut(&mut self, friend: OnlineId, id: u64) -> Option<&mut ChatEntry> {
        self.chats
            .get_mut(&friend)?
            .entries
            .iter_mut()
            .find(|e| e.id == id)
    }

    fn set_delivery(&mut self, friend: OnlineId, id: u64, delivery: Delivery) {
        if let Some(entry) = self.entry_mut(friend, id) {
            entry.delivery = delivery;
            self.dirty = true;
        }
    }

    fn take_receipt(&mut self, friend: OnlineId, receipt: &Receipt, at: u64) {
        let Some(chat) = self.chats.get_mut(&friend) else {
            return;
        };
        let delivered = receipt.delivered.iter().take(MAX_RECEIPT_IDS);
        let read = receipt.read.iter().take(MAX_RECEIPT_IDS);
        let steps = delivered
            .map(|id| (*id, Delivery::Delivered(at)))
            .chain(read.map(|id| (*id, Delivery::Read(at))));
        for (id, step) in steps {
            if let Some(entry) = chat.entries.iter_mut().find(|e| e.mine && e.id == id)
                && entry.delivery.rank() > 0
                && step.rank() > entry.delivery.rank()
            {
                entry.delivery = step;
                self.dirty = true;
            }
        }
    }

    fn owe(&mut self, friend: OnlineId, id: u64, read: bool) {
        let receipt = self.receipts.entry(friend).or_default();
        let list = if read {
            &mut receipt.read
        } else {
            &mut receipt.delivered
        };
        if !list.contains(&id) {
            list.push(id);
        }
    }

    fn take_edit(&mut self, friend: OnlineId, edit: &Edit) {
        if edit.text.trim().is_empty() || edit.text.chars().count() > MAX_TEXT_CHARS {
            return;
        }
        let Some(entry) = self.chats.get_mut(&friend).and_then(|chat| {
            chat.entries
                .iter_mut()
                .find(|e| !e.mine && e.id == edit.id && !e.is_deleted())
        }) else {
            return;
        };
        if let Body::Text(text) = &mut entry.body {
            text.clone_from(&edit.text);
            entry.change = Change::Edited { told: true };
            self.dirty = true;
        }
    }

    fn take_own_receipt(&mut self, friend: OnlineId, receipt: &Receipt) {
        let Some(chat) = self.chats.get_mut(&friend) else {
            return;
        };
        for id in receipt.read.iter().take(MAX_RECEIPT_IDS) {
            if let Some(entry) = chat.entries.iter_mut().find(|e| !e.mine && e.id == *id)
                && entry.delivery == Delivery::Received
            {
                entry.delivery = Delivery::Seen;
                chat.unread = chat.unread.saturating_sub(1);
                self.dirty = true;
            }
        }
        if chat.unread > 0
            && chat
                .entries
                .iter()
                .rev()
                .find(|e| !e.mine)
                .is_some_and(|e| e.delivery == Delivery::Seen)
        {
            chat.unread = 0;
            self.dirty = true;
        }
        if let Some(owed) = self.receipts.get_mut(&friend) {
            owed.delivered.retain(|id| !receipt.read.contains(id));
            owed.read.retain(|id| !receipt.read.contains(id));
        }
    }

    fn take_own_change(&mut self, friend: OnlineId, body: &Body) {
        let (id, text) = match body {
            Body::Edit(edit) if edit.text.chars().count() <= MAX_TEXT_CHARS => {
                (edit.id, Some(edit.text.clone()))
            }
            Body::Delete(id) => (*id, None),
            _ => return,
        };
        let Some(entry) = self.chats.get_mut(&friend).and_then(|chat| {
            chat.entries
                .iter_mut()
                .find(|e| e.mine && e.id == id && !e.is_deleted())
        }) else {
            return;
        };
        match text {
            Some(text) => {
                entry.body = Body::Text(text);
                entry.change = Change::Edited { told: true };
            }
            None => {
                entry.body = Body::Text(String::new());
                entry.change = Change::Deleted { told: true };
                self.held.remove(&id);
            }
        }
        self.dirty = true;
    }

    fn take_delete(&mut self, friend: OnlineId, id: u64) {
        let Some(chat) = self.chats.get_mut(&friend) else {
            return;
        };
        let Some(entry) = chat
            .entries
            .iter_mut()
            .find(|e| !e.mine && e.id == id && !e.is_deleted())
        else {
            return;
        };
        if entry.delivery == Delivery::Received {
            chat.unread = chat.unread.saturating_sub(1);
        }
        entry.body = Body::Text(String::new());
        entry.change = Change::Deleted { told: true };
        self.dirty = true;
    }

    fn push(&mut self, friend: OnlineId, entry: ChatEntry) {
        let chat = self.chats.entry(friend).or_default();
        chat.entries.push(entry);
        self.dirty = true;
    }

    fn load(&mut self, owner: String) {
        if self.owner.as_ref() == Some(&owner) {
            return;
        }
        self.chats.clear();
        self.held.clear();
        self.files = None;
        if let Some(store) = &self.store
            && let Some(bytes) = store.read(NAMESPACE, &history_name(&owner))
            && let Some(mut chats) = decode_history(&bytes)
        {
            for entry in chats.values_mut().flat_map(|c| c.entries.iter_mut()) {
                if entry.delivery == Delivery::Sending {
                    entry.delivery = Delivery::Failed(Failure::Gone);
                }
            }
            self.chats = chats;
        }
        self.owner = Some(owner);
        self.changes_due = true;
    }

    fn save(&mut self, now: Instant, force: bool) -> bool {
        if !self.dirty {
            return true;
        }
        let Some(store) = &self.store else {
            self.dirty = false;
            return true;
        };
        if !force
            && self
                .saved_at
                .is_some_and(|at| now.duration_since(at) < SAVE_EVERY)
        {
            return false;
        }
        let Some(owner) = &self.owner else {
            return false;
        };
        self.saved_at = Some(now);
        let Some(bytes) = encode_history(&self.chats) else {
            return false;
        };
        match store.write(NAMESPACE, &history_name(owner), &bytes) {
            Ok(()) => {
                self.dirty = false;
                true
            }
            Err(err) => {
                log::error!("Could not save the chats: {err}");
                false
            }
        }
    }

    fn in_flight_to(&self, friend: OnlineId) -> bool {
        self.sending.values().any(|out| out.friend() == friend)
    }

    fn next_ready(&self) -> Option<i64> {
        self.queued
            .iter()
            .find(|(_, out)| !self.in_flight_to(out.friend()))
            .map(|(place, _)| *place)
    }

    fn queue_back(&mut self, out: Out) {
        let place = self.queued.last_key_value().map_or(0, |(last, _)| last + 1);
        self.queued.insert(place, out);
    }

    fn queue_front(&mut self, out: Out) {
        let place = self
            .queued
            .first_key_value()
            .map_or(0, |(first, _)| first - 1);
        self.queued.insert(place, out);
    }

    fn fail_pending(&mut self) {
        let sending: Vec<Out> = self.sending.drain().map(|(_, v)| v).collect();
        let uploads: Vec<(OnlineId, u64, Vec<u8>)> = self
            .uploads
            .drain()
            .map(|(_, u)| (u.to, u.id, u.bytes))
            .collect();
        let queued: Vec<Out> = std::mem::take(&mut self.queued).into_values().collect();
        for out in sending.into_iter().chain(queued) {
            if let Out::Entry(friend, id) = out {
                self.set_delivery(friend, id, Delivery::Failed(Failure::NotConnected));
            }
        }
        self.changes_due = true;
        for (friend, id, bytes) in uploads {
            self.held.insert(id, bytes);
            self.set_delivery(friend, id, Delivery::Failed(Failure::NotConnected));
        }
        let files: Vec<FileId> = self.downloads.drain().map(|(file, _)| file).collect();
        for file in files {
            self.fetched
                .insert(file, Fetch::Failed("The connection dropped. Try again."));
        }
    }

    fn take_chat(&mut self, now: Instant) -> bool {
        if self.paused_until.is_some_and(|until| now < until) {
            return false;
        }
        if !self.paced {
            return true;
        }
        let earned = self.chat_tokens_at.map_or(CHAT_BURST, |at| {
            now.duration_since(at).as_secs_f64() * CHATS_PER_SECOND
        });
        self.chat_tokens_at = Some(now);
        self.chat_tokens = (self.chat_tokens + earned).min(CHAT_BURST);
        if self.chat_tokens >= 1.0 {
            self.chat_tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn rate_limited(&mut self, now: Instant) {
        self.paused_until = Some(now + RATE_LIMITED_PAUSE);
        self.paced = true;
        self.chat_tokens = 0.0;
        self.chat_tokens_at = Some(now);
    }

    fn take_budget(&mut self, now: Instant, bytes: usize) -> bool {
        let elapsed = self.budget_at.map_or(UPLOAD_BURST, |at| {
            now.duration_since(at).as_secs_f64() * UPLOAD_PER_SECOND
        });
        self.budget_at = Some(now);
        self.budget = (self.budget + elapsed).min(UPLOAD_BURST);
        if self.budget >= bytes as f64 {
            self.budget -= bytes as f64;
            true
        } else {
            false
        }
    }
}

impl Online {
    pub fn share_blocked(&self) -> Option<&'static str> {
        let account = self.account()?;
        if account.quota_bytes == 0 {
            return Some(if self.info().is_some_and(|i| i.quota_bytes > 0) {
                "Your account on this relay has no storage"
            } else {
                "This relay doesn't keep files"
            });
        }
        (account.storage_used + STORAGE_BLOCK > account.quota_bytes)
            .then_some("Your storage is full")
    }

    pub fn persist_chats(&mut self) {
        self.chat.store = Some(Backend::settings());
    }

    pub(super) fn chat_changes_due(&mut self) {
        self.chat.changes_due = true;
    }

    pub(super) fn chat_reset(&mut self) {
        self.chat.fail_pending();
        self.chat.save(Instant::now(), true);
        self.chat.fetched.clear();
        self.chat.files = None;
    }

    pub(super) fn chat_receive(&mut self, msg: ServerMsg) -> Option<ServerMsg> {
        if let Some(owner) = self.chat_owner() {
            self.chat.load(owner);
        }
        match msg {
            ServerMsg::Chat(message) => self.chat_message(message),
            ServerMsg::ChatCopy { to, message } => self.chat_copy(to, message),
            ServerMsg::ChatSent { tag, id: relay, at } => match self.chat.sending.remove(&tag) {
                Some(Out::Entry(friend, id)) => {
                    if let Some(entry) = self.chat.entry_mut(friend, id) {
                        entry.delivery = Delivery::Sent;
                        entry.at = at;
                        self.chat.dirty = true;
                        self.chat.held.remove(&id);
                        if let Some(chat) = self.chat.chats.get_mut(&friend) {
                            chat.relayed.insert(id, relay);
                        }
                    }
                }
                Some(Out::Change(friend, id)) => {
                    if let Some(entry) = self.chat.entry_mut(friend, id) {
                        entry.change = entry.change.told();
                        self.chat.dirty = true;
                    }
                    self.restate(friend, id);
                }
                None => {}
            },
            ServerMsg::ChatFailed {
                tag,
                error: ErrorCode::RateLimited,
            } => {
                self.chat.rate_limited(Instant::now());
                if let Some(sent) = self.chat.sending.remove(&tag) {
                    self.chat.queue_front(sent);
                }
            }
            ServerMsg::ChatFailed { tag, error } => match self.chat.sending.remove(&tag) {
                Some(Out::Entry(friend, id)) => {
                    self.chat
                        .set_delivery(friend, id, Delivery::Failed(Failure::Refused(error)));
                }
                Some(Out::Change(_, id)) => {
                    self.chat
                        .change_failures
                        .insert(id, Failure::Refused(error));
                }
                None => {}
            },
            ServerMsg::UploadReady(tag) => {
                if let Some(upload) = self.chat.uploads.get_mut(&tag) {
                    upload.ready = true;
                }
            }
            ServerMsg::Uploaded { tag, file } => {
                if let Some(upload) = self.chat.uploads.remove(&tag) {
                    let kept = match self.chat.entry_mut(upload.to, upload.id) {
                        Some(entry) if !entry.is_deleted() => {
                            if let Body::Share(share) = &mut entry.body {
                                share.file = file.id;
                                share.size = file.size;
                            }
                            true
                        }
                        _ => false,
                    };
                    if !kept {
                        self.send(ClientMsg::DeleteFile(file.id));
                        return None;
                    }
                    if let Some(files) = &mut self.chat.files {
                        files.push(file);
                    }
                    self.send_entry(upload.to, upload.id);
                }
            }
            ServerMsg::UploadFailed { tag, error } => {
                if let Some(upload) = self.chat.uploads.remove(&tag) {
                    self.chat.held.insert(upload.id, upload.bytes);
                    self.chat.set_delivery(
                        upload.to,
                        upload.id,
                        Delivery::Failed(Failure::Refused(error)),
                    );
                }
            }
            ServerMsg::FileStart { file, size } => {
                if let Some(download) = self.chat.downloads.get_mut(&file) {
                    download.size = size;
                    download.data = Vec::with_capacity(size as usize);
                    self.chat.fetched.insert(
                        file,
                        Fetch::Loading {
                            done: 0,
                            total: size,
                        },
                    );
                }
            }
            ServerMsg::FileChunk { file, data } => self.chat_chunk(file, data),
            ServerMsg::DownloadFailed { file, error } => {
                self.chat.downloads.remove(&file);
                let text = match error {
                    ErrorCode::UnknownFile => "This file is no longer on the server",
                    _ => "The server couldn't send the file",
                };
                self.chat.fetched.insert(file, Fetch::Failed(text));
            }
            ServerMsg::FileDeleted(file) => {
                if let Some(files) = &mut self.chat.files {
                    files.retain(|f| f.id != file);
                }
            }
            ServerMsg::Files(files) => self.chat.files = Some(files),
            other => return Some(other),
        }
        None
    }

    fn chat_message(&mut self, message: ChatMessage) {
        let ChatMessage {
            id: server_id,
            from,
            at,
            sealed,
        } = message;
        self.chat.acks.push(server_id);
        let (Some(keys), Some(friend)) = (&self.keys, self.friends.get(&from)) else {
            return;
        };
        let Ok(envelope) = keys.open_envelope(&friend.public_key, &sealed) else {
            return;
        };
        match &envelope.body {
            Body::Receipt(receipt) => {
                self.chat.take_receipt(from, receipt, at);
                return;
            }
            Body::Edit(edit) => {
                self.chat.take_edit(from, edit);
                return;
            }
            Body::Delete(id) => {
                self.chat.take_delete(from, *id);
                return;
            }
            Body::Text(text) if text.chars().count() > MAX_TEXT_CHARS => return,
            Body::Palette(palette) if palette.len() > MAX_PALETTE_BYTES => return,
            _ => {}
        }
        self.chat.owe(from, envelope.id, false);
        self.typing.gone(from);
        let chat = self.chat.chats.entry(from).or_default();
        if chat.entries.iter().any(|e| e.id == envelope.id) {
            return;
        }
        chat.unread += 1;
        self.chat.push(
            from,
            ChatEntry {
                id: envelope.id,
                mine: false,
                at,
                body: envelope.body,
                delivery: Delivery::Received,
                change: Change::Kept,
            },
        );
    }

    fn chat_copy(&mut self, to: OnlineId, message: ChatMessage) {
        let ChatMessage {
            id: relay,
            at,
            sealed,
            ..
        } = message;
        self.chat.acks.push(relay);
        let (Some(keys), Some(friend)) = (&self.keys, self.friends.get(&to)) else {
            return;
        };
        let Ok(envelope) = keys.open_envelope(&friend.public_key, &sealed) else {
            return;
        };
        match &envelope.body {
            Body::Receipt(receipt) => return self.chat.take_own_receipt(to, receipt),
            Body::Edit(_) | Body::Delete(_) => {
                return self.chat.take_own_change(to, &envelope.body);
            }
            Body::Text(text) if text.chars().count() > MAX_TEXT_CHARS => return,
            Body::Palette(palette) if palette.len() > MAX_PALETTE_BYTES => return,
            _ => {}
        }
        let chat = self.chat.chats.entry(to).or_default();
        chat.relayed.insert(envelope.id, relay);
        if chat.entries.iter().any(|e| e.id == envelope.id) {
            return;
        }
        self.chat.push(
            to,
            ChatEntry {
                id: envelope.id,
                mine: true,
                at,
                body: envelope.body,
                delivery: Delivery::Sent,
                change: Change::Kept,
            },
        );
    }

    fn restate(&mut self, friend: OnlineId, id: u64) {
        let Some(&relay) = self
            .chat
            .chats
            .get(&friend)
            .and_then(|chat| chat.relayed.get(&id))
        else {
            return;
        };
        let Some(entry) = self.chat.entry_mut(friend, id) else {
            return;
        };
        let body = match (&entry.change, &entry.body) {
            (Change::Deleted { .. }, _) => None,
            (Change::Edited { .. }, Body::Text(text)) => Some(Body::Text(text.clone())),
            _ => return,
        };
        let sealed = match (body, &self.keys, self.friends.get(&friend)) {
            (None, ..) => None,
            (Some(body), Some(keys), Some(person)) => {
                Some(keys.seal_envelope(&person.public_key, &Envelope { id, body }))
            }
            _ => return,
        };
        if sealed.is_none()
            && let Some(chat) = self.chat.chats.get_mut(&friend)
        {
            chat.relayed.remove(&id);
            self.chat.dirty = true;
        }
        self.send(ClientMsg::Restate { id: relay, sealed });
    }

    pub(super) fn chat_wipe(&mut self) {
        if let Some(owner) = self.chat_owner() {
            self.chat.load(owner.clone());
            self.chat.fail_pending();
            self.chat.chats.clear();
            self.chat.held.clear();
            self.chat.receipts.clear();
            self.chat.queued.clear();
            self.chat.dirty = false;
            if let Some(store) = &self.chat.store {
                let _ = store.delete(NAMESPACE, &history_name(&owner));
            }
        }
    }

    pub fn forget_chats(&mut self, owner: &str) {
        if self.chat.owner.as_deref() == Some(owner) {
            self.chat.owner = None;
            self.chat.chats.clear();
            self.chat.held.clear();
            self.chat.dirty = false;
        }
        if let Some(store) = &self.chat.store {
            let _ = store.delete(NAMESPACE, &history_name(owner));
        }
    }

    fn chat_chunk(&mut self, file: FileId, data: Vec<u8>) {
        let Some(download) = self.chat.downloads.get_mut(&file) else {
            return;
        };
        download.data.extend_from_slice(&data);
        let (done, total) = (download.data.len() as u64, download.size);
        if done < total {
            self.chat
                .fetched
                .insert(file, Fetch::Loading { done, total });
            return;
        }
        let Some(download) = self.chat.downloads.remove(&file) else {
            return;
        };
        let opened = match (&self.keys, self.friends.get(&download.peer)) {
            (Some(keys), Some(friend)) => keys.open_file(&friend.public_key, &download.data).ok(),
            _ => None,
        };
        let fetch = match opened {
            Some(bytes) => Fetch::Ready(bytes),
            None => Fetch::Failed("The file can't be opened"),
        };
        self.chat.fetched.insert(file, fetch);
    }

    pub(super) fn chat_tick(&mut self, now: Instant) {
        if let Some(owner) = self.chat_owner() {
            self.chat.load(owner);
        }
        if self.phase != Phase::Connected {
            if !self.chat.sending.is_empty()
                || !self.chat.queued.is_empty()
                || !self.chat.uploads.is_empty()
                || !self.chat.downloads.is_empty()
            {
                self.chat.fail_pending();
            }
        } else {
            self.pump_uploads(now);
            self.pump_queued(now);
            self.send_receipts(now);
        }
        if self.chat.save(now, false) && !self.chat.acks.is_empty() && self.is_connected() {
            let acks = std::mem::take(&mut self.chat.acks);
            self.send(ClientMsg::ChatAck(acks));
        }
    }

    fn send_receipts(&mut self, now: Instant) {
        if self.chat.receipts.is_empty()
            || self
                .chat
                .receipts_at
                .is_some_and(|at| now.duration_since(at) < RECEIPT_EVERY)
        {
            return;
        }
        self.chat.receipts_at = Some(now);
        let friends: Vec<OnlineId> = self.chat.receipts.keys().copied().collect();
        for friend in friends {
            if !self.chat.receipts.contains_key(&friend) {
                continue;
            }
            if !self.chat.take_chat(now) {
                return;
            }
            let Some(owed) = self.chat.receipts.get_mut(&friend) else {
                continue;
            };
            let batch = Receipt {
                delivered: take_first(&mut owed.delivered),
                read: take_first(&mut owed.read),
            };
            if owed.is_empty() {
                self.chat.receipts.remove(&friend);
            }
            let sealed = match (&self.keys, self.friends.get(&friend)) {
                (Some(keys), Some(person)) if person.state == FriendState::Friend => keys
                    .seal_envelope(
                        &person.public_key,
                        &Envelope::new(Body::Receipt(batch.clone())),
                    ),
                _ => continue,
            };
            let tag = self.chat.tag();
            if !self.send(ClientMsg::Chat {
                tag,
                to: friend,
                sealed,
            }) {
                for id in batch.delivered {
                    self.chat.owe(friend, id, false);
                }
                for id in batch.read {
                    self.chat.owe(friend, id, true);
                }
                return;
            }
        }
    }

    fn chat_owner(&self) -> Option<String> {
        let account = self.account.as_ref()?;
        let address = &self.target.as_ref()?.address;
        Some(format!("{}@{}", account.online_id, address.key()))
    }

    fn pump_uploads(&mut self, now: Instant) {
        let mut tags: Vec<Tag> = self
            .chat
            .uploads
            .iter()
            .filter(|(_, u)| u.ready && u.sent < u.bytes.len())
            .map(|(tag, _)| *tag)
            .collect();
        tags.sort_unstable();
        for tag in tags {
            while let Some((start, total)) =
                self.chat.uploads.get(&tag).map(|u| (u.sent, u.bytes.len()))
            {
                if start >= total {
                    break;
                }
                let end = (start + CHUNK_BYTES).min(total);
                if !self.chat.take_budget(now, end - start) {
                    return;
                }
                let data = self.chat.uploads[&tag].bytes[start..end].to_vec();
                if !self.send(ClientMsg::UploadChunk { tag, data }) {
                    return;
                }
                if let Some(upload) = self.chat.uploads.get_mut(&tag) {
                    upload.sent = end;
                }
            }
        }
    }

    fn pump_queued(&mut self, now: Instant) {
        if std::mem::take(&mut self.chat.changes_due) {
            self.queue_untold_changes();
        }
        while let Some(place) = self.chat.next_ready() {
            if !self.chat.take_chat(now) {
                break;
            }
            match self.chat.queued.remove(&place) {
                Some(Out::Entry(friend, id)) => self.transmit_entry(friend, id),
                Some(Out::Change(friend, id)) => self.transmit_change(friend, id),
                None => {}
            }
        }
    }

    fn queue_untold_changes(&mut self) {
        let waiting: Vec<Out> = self
            .chat
            .sending
            .values()
            .chain(self.chat.queued.values())
            .copied()
            .collect();
        let untold: Vec<Out> = self
            .chat
            .chats
            .iter()
            .filter(|(friend, _)| self.can_chat(**friend))
            .flat_map(|(friend, chat)| {
                chat.entries
                    .iter()
                    .filter(|e| e.mine && e.change.untold())
                    .map(|e| Out::Change(*friend, e.id))
            })
            .filter(|out| !waiting.contains(out))
            .collect();
        for out in untold {
            if let Out::Change(_, id) = out
                && self.chat.change_failures.contains_key(&id)
            {
                continue;
            }
            self.chat.queue_back(out);
        }
    }

    fn send_entry(&mut self, friend: OnlineId, id: u64) {
        let now = Instant::now();
        if self.phase != Phase::Connected
            || (self.chat.queued.is_empty()
                && !self.chat.in_flight_to(friend)
                && self.chat.take_chat(now))
        {
            self.transmit_entry(friend, id);
            return;
        }
        if let Some(entry) = self.chat.entry_mut(friend, id) {
            entry.delivery = Delivery::Sending;
            self.chat.dirty = true;
            self.chat.queue_back(Out::Entry(friend, id));
        }
    }

    fn send_change(&mut self, friend: OnlineId, id: u64) {
        self.chat.change_failures.remove(&id);
        if self.phase != Phase::Connected {
            return;
        }
        let out = Out::Change(friend, id);
        if self.chat.queued.values().any(|o| *o == out) {
            return;
        }
        if self.chat.queued.is_empty()
            && !self.chat.in_flight_to(friend)
            && self.chat.take_chat(Instant::now())
        {
            self.transmit_change(friend, id);
        } else {
            self.chat.queue_back(out);
        }
    }

    fn transmit_change(&mut self, friend: OnlineId, id: u64) {
        let Some(entry) = self.chat.entry_mut(friend, id) else {
            return;
        };
        let body = match (&entry.change, &entry.body) {
            (Change::Deleted { told: false }, _) => Body::Delete(id),
            (Change::Edited { told: false }, Body::Text(text)) => Body::Edit(Edit {
                id,
                text: text.clone(),
            }),
            _ => return,
        };
        let sealed = match (&self.keys, self.friends.get(&friend)) {
            (Some(keys), Some(person)) if self.phase == Phase::Connected => {
                keys.seal_envelope(&person.public_key, &Envelope::new(body))
            }
            _ => return,
        };
        let tag = self.chat.tag();
        if self.send(ClientMsg::Chat {
            tag,
            to: friend,
            sealed,
        }) {
            self.chat.sending.insert(tag, Out::Change(friend, id));
        }
    }

    fn transmit_entry(&mut self, friend: OnlineId, id: u64) {
        let Some(entry) = self.chat.entry_mut(friend, id) else {
            return;
        };
        if entry.is_deleted() {
            return;
        }
        let envelope = Envelope {
            id,
            body: entry.body.clone(),
        };
        entry.delivery = Delivery::Sending;
        self.chat.dirty = true;
        let sealed = match (&self.keys, self.friends.get(&friend)) {
            (Some(keys), Some(person)) if self.phase == Phase::Connected => {
                keys.seal_envelope(&person.public_key, &envelope)
            }
            _ => {
                self.chat
                    .set_delivery(friend, id, Delivery::Failed(Failure::NotConnected));
                return;
            }
        };
        let tag = self.chat.tag();
        if self.send(ClientMsg::Chat {
            tag,
            to: friend,
            sealed,
        }) {
            self.chat.sending.insert(tag, Out::Entry(friend, id));
        } else {
            self.chat
                .set_delivery(friend, id, Delivery::Failed(Failure::NotConnected));
        }
    }

    fn start_upload(&mut self, friend: OnlineId, id: u64, bytes: Vec<u8>) {
        if self.phase != Phase::Connected {
            self.chat.held.insert(id, bytes);
            self.chat
                .set_delivery(friend, id, Delivery::Failed(Failure::NotConnected));
            return;
        }
        let tag = self.chat.tag();
        let size = bytes.len() as u64;
        self.chat.set_delivery(friend, id, Delivery::Sending);
        self.chat.uploads.insert(
            tag,
            Upload {
                to: friend,
                id,
                bytes,
                sent: 0,
                ready: false,
            },
        );
        self.send(ClientMsg::Upload { tag, size });
    }

    pub fn can_chat(&self, friend: OnlineId) -> bool {
        self.friends
            .get(&friend)
            .is_some_and(|f| f.state == FriendState::Friend)
    }

    pub fn send_text(&mut self, friend: OnlineId, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS || !self.can_chat(friend) {
            return false;
        }
        let envelope = Envelope::new(Body::Text(text.to_owned()));
        let id = envelope.id;
        self.chat.push(
            friend,
            ChatEntry {
                id,
                mine: true,
                at: crate::storage::now_unix(),
                body: envelope.body,
                delivery: Delivery::Sending,
                change: Change::Kept,
            },
        );
        self.send_entry(friend, id);
        self.chat.sent += 1;
        true
    }

    pub fn take_sent_messages(&mut self) -> u32 {
        std::mem::take(&mut self.chat.sent)
    }

    pub fn share(&mut self, friend: OnlineId, new: NewShare) -> bool {
        if !self.can_chat(friend) {
            return false;
        }
        let (Some(keys), Some(person)) = (&self.keys, self.friends.get(&friend)) else {
            return false;
        };
        let compress = new.kind == ShareKind::Replay;
        let bytes = keys.seal_file(&person.public_key, &new.data, compress);
        let envelope = Envelope::new(Body::Share(Share {
            file: FileId([0; 16]),
            kind: new.kind,
            size: bytes.len() as u64,
            game: new.game,
            rom_sha256: new.rom_sha256,
            created_at: new.created_at,
            width: new.width,
            height: new.height,
            length_ms: new.length_ms,
            preview: new.preview,
        }));
        let id = envelope.id;
        self.chat.push(
            friend,
            ChatEntry {
                id,
                mine: true,
                at: crate::storage::now_unix(),
                body: envelope.body,
                delivery: Delivery::Sending,
                change: Change::Kept,
            },
        );
        self.start_upload(friend, id, bytes);
        self.chat.sent += 1;
        true
    }

    pub fn send_palette(&mut self, friend: OnlineId, palette: String) -> bool {
        if palette.len() > MAX_PALETTE_BYTES || !self.can_chat(friend) {
            return false;
        }
        let envelope = Envelope::new(Body::Palette(palette));
        let id = envelope.id;
        self.chat.push(
            friend,
            ChatEntry {
                id,
                mine: true,
                at: crate::storage::now_unix(),
                body: envelope.body,
                delivery: Delivery::Sending,
                change: Change::Kept,
            },
        );
        self.send_entry(friend, id);
        self.chat.sent += 1;
        true
    }

    fn in_flight(&self, friend: OnlineId, id: u64) -> bool {
        self.chat
            .sending
            .values()
            .any(|out| *out == Out::Entry(friend, id))
    }

    pub fn can_edit(&self, friend: OnlineId, id: u64) -> bool {
        self.conversation(friend)
            .and_then(|c| c.entries.iter().find(|e| e.id == id))
            .is_some_and(|e| e.mine && !e.is_deleted() && matches!(e.body, Body::Text(_)))
    }

    pub fn edit_text(&mut self, friend: OnlineId, id: u64, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS || !self.can_edit(friend, id) {
            return false;
        }
        let reached = {
            let sending = self.in_flight(friend, id);
            let Some(entry) = self.chat.entry_mut(friend, id) else {
                return false;
            };
            if entry.body == Body::Text(text.to_owned()) {
                return true;
            }
            entry.body = Body::Text(text.to_owned());
            let reached = entry.reached_relay(sending);
            if reached {
                entry.change = Change::Edited { told: false };
            }
            reached
        };
        self.chat.dirty = true;
        if reached {
            self.send_change(friend, id);
        }
        true
    }

    pub fn delete_message(&mut self, friend: OnlineId, id: u64) {
        let sending = self.in_flight(friend, id);
        let Some(entry) = self.chat.entry_mut(friend, id) else {
            return;
        };
        if !entry.mine || entry.is_deleted() {
            return;
        }
        let file = match &entry.body {
            Body::Share(share) if share.file != NO_FILE => Some(share.file),
            _ => None,
        };
        if !entry.reached_relay(sending) {
            self.chat
                .queued
                .retain(|_, out| *out != Out::Entry(friend, id));
            self.forget(friend, id);
            return;
        }
        entry.body = Body::Text(String::new());
        entry.change = Change::Deleted { told: false };
        self.chat.dirty = true;
        self.chat.held.remove(&id);
        if let Some(file) = file {
            self.delete_file(file);
        }
        self.send_change(friend, id);
    }

    pub fn change_failure(&self, id: u64) -> Option<Failure> {
        self.chat.change_failures.get(&id).copied()
    }

    pub fn retry(&mut self, friend: OnlineId, id: u64) {
        if self.chat.change_failures.contains_key(&id) {
            self.send_change(friend, id);
            return;
        }
        let Some(entry) = self.chat.entry_mut(friend, id) else {
            return;
        };
        if !matches!(entry.delivery, Delivery::Failed(_)) {
            return;
        }
        let needs_upload = matches!(&entry.body, Body::Share(share) if share.file == NO_FILE);
        if !needs_upload {
            self.send_entry(friend, id);
            return;
        }
        match self.chat.held.remove(&id) {
            Some(bytes) => self.start_upload(friend, id, bytes),
            None => self
                .chat
                .set_delivery(friend, id, Delivery::Failed(Failure::Gone)),
        }
    }

    pub fn forget(&mut self, friend: OnlineId, id: u64) {
        if let Some(chat) = self.chat.chats.get_mut(&friend) {
            chat.entries.retain(|e| e.id != id);
            self.chat.held.remove(&id);
            self.chat.dirty = true;
        }
    }

    pub fn conversation(&self, friend: OnlineId) -> Option<&Conversation> {
        self.chat.chats.get(&friend)
    }

    pub fn unread(&self, friend: OnlineId) -> usize {
        self.chat.chats.get(&friend).map_or(0, |c| c.unread)
    }

    pub fn total_unread(&self) -> usize {
        self.chat
            .chats
            .iter()
            .filter(|(id, _)| self.can_chat(**id))
            .map(|(_, c)| c.unread)
            .sum()
    }

    pub fn mark_read(&mut self, friend: OnlineId) {
        if let Some(chat) = self.chat.chats.get_mut(&friend)
            && chat.unread > 0
        {
            chat.unread = 0;
            self.chat.dirty = true;
        }
    }

    pub fn mark_seen(&mut self, friend: OnlineId, ids: &[u64]) {
        let Some(chat) = self.chat.chats.get_mut(&friend) else {
            return;
        };
        let mut seen = Vec::new();
        for entry in &mut chat.entries {
            if entry.delivery == Delivery::Received && ids.contains(&entry.id) {
                entry.delivery = Delivery::Seen;
                seen.push(entry.id);
            }
        }
        if seen.is_empty() {
            return;
        }
        self.chat.dirty = true;
        for id in seen {
            self.chat.owe(friend, id, true);
        }
    }

    pub fn needs_attention(&self) -> bool {
        self.waiting_requests() > 0 || self.total_unread() > 0
    }

    pub fn fetch(&mut self, peer: OnlineId, file: FileId) {
        if let Some(Fetch::Loading { .. } | Fetch::Ready(_)) = self.chat.fetched.get(&file) {
            return;
        }
        if !self.is_connected() {
            self.chat
                .fetched
                .insert(file, Fetch::Failed("You're offline"));
            return;
        }
        self.chat.downloads.insert(
            file,
            Download {
                peer,
                size: 0,
                data: Vec::new(),
            },
        );
        self.chat
            .fetched
            .insert(file, Fetch::Loading { done: 0, total: 0 });
        self.send(ClientMsg::Download(file));
    }

    pub fn fetched(&self, file: FileId) -> Option<&Fetch> {
        self.chat.fetched.get(&file)
    }

    pub fn take_fetched(&mut self, file: FileId) -> Option<Fetch> {
        match self.chat.fetched.get(&file)? {
            Fetch::Loading { .. } => None,
            _ => self.chat.fetched.remove(&file),
        }
    }

    pub fn list_files(&mut self) -> bool {
        self.send(ClientMsg::ListFiles)
    }

    pub fn shared_files(&self) -> Option<&[FileInfo]> {
        self.chat.files.as_deref()
    }

    pub fn delete_file(&mut self, file: FileId) -> bool {
        self.send(ClientMsg::DeleteFile(file))
    }

    pub fn share_of(&self, file: FileId) -> Option<(OnlineId, &Share)> {
        self.chat.chats.iter().find_map(|(friend, chat)| {
            chat.entries.iter().find_map(|e| match &e.body {
                Body::Share(share) if e.mine && share.file == file => Some((*friend, share)),
                _ => None,
            })
        })
    }

    pub fn uploading(&self, friend: OnlineId, id: u64) -> Option<(usize, usize)> {
        self.chat
            .uploads
            .values()
            .find(|u| u.to == friend && u.id == id)
            .map(|u| (u.sent, u.bytes.len()))
    }

    pub fn flush_chats(&mut self) {
        self.chat.save(Instant::now(), true);
    }

    pub fn chat_files(&mut self) -> Vec<(String, Vec<u8>)> {
        self.flush_chats();
        let Some(store) = &self.chat.store else {
            return Vec::new();
        };
        let mut files: Vec<(String, Vec<u8>)> = store
            .list(NAMESPACE)
            .into_iter()
            .filter(|name| is_history_name(name))
            .filter_map(|name| {
                let bytes = store.read(NAMESPACE, &name)?;
                Some((name, bytes))
            })
            .collect();
        files.sort();
        files
    }

    pub fn chat_restore(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        if !is_history_name(name) {
            return Err(std::io::Error::other("not a chat history"));
        }
        self.flush_chats();
        if let Some(store) = &self.chat.store {
            store.write(NAMESPACE, name, bytes)?;
        }
        self.chat.owner = None;
        Ok(())
    }

    pub fn chat_delete_all(&mut self) -> std::io::Result<()> {
        self.chat.chats.clear();
        self.chat.held.clear();
        self.chat.dirty = false;
        self.chat.owner = None;
        match &self.chat.store {
            Some(store) => store.clear(NAMESPACE),
            None => Ok(()),
        }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn persist_chats_in(&mut self, root: std::path::PathBuf) {
        self.chat.store = Some(Backend::at(root));
    }
}

#[cfg(test)]
mod tests;
