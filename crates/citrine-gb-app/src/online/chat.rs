use super::{Online, Phase};
use crate::storage::Backend;
use citrine_gb_net::{
    Body, CHUNK_BYTES, ChatMessage, ClientMsg, Envelope, ErrorCode, FileId, FileInfo, FriendState,
    MAX_RECEIPT_IDS, MAX_TEXT_CHARS, MessageId, OnlineId, Receipt, ServerMsg, Share, ShareKind,
    Tag, pack, unpack,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use web_time::Instant;

pub const KEEP_PER_FRIEND: usize = 300;
const NAMESPACE: &str = "chats";
const SAVE_EVERY: Duration = Duration::from_secs(1);
const RECEIPT_EVERY: Duration = Duration::from_secs(1);
const UPLOAD_PER_SECOND: f64 = 3.0 * 1024.0 * 1024.0;
const UPLOAD_BURST: f64 = 3.0 * 1024.0 * 1024.0;
const MAX_HISTORY_BYTES: usize = 32 << 20;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatEntry {
    pub id: u64,
    pub mine: bool,
    pub at: u64,
    pub body: Body,
    pub delivery: Delivery,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub entries: Vec<ChatEntry>,
    pub unread: usize,
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
    sending: HashMap<Tag, (OnlineId, u64)>,
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
}

fn take_first(ids: &mut Vec<u64>) -> Vec<u64> {
    let count = ids.len().min(MAX_RECEIPT_IDS);
    ids.drain(..count).collect()
}

fn history_name(owner: &str) -> String {
    let safe: String = owner
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{safe}.bin")
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

    fn push(&mut self, friend: OnlineId, entry: ChatEntry) {
        let chat = self.chats.entry(friend).or_default();
        chat.entries.push(entry);
        if chat.entries.len() > KEEP_PER_FRIEND {
            let extra = chat.entries.len() - KEEP_PER_FRIEND;
            for old in chat.entries.drain(..extra) {
                self.held.remove(&old.id);
            }
        }
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
            && let Ok(plain) = unpack(&bytes, MAX_HISTORY_BYTES)
            && let Ok(mut chats) = postcard::from_bytes::<BTreeMap<OnlineId, Conversation>>(&plain)
        {
            for entry in chats.values_mut().flat_map(|c| c.entries.iter_mut()) {
                if entry.delivery == Delivery::Sending {
                    entry.delivery = Delivery::Failed(Failure::Gone);
                }
            }
            self.chats = chats;
        }
        self.owner = Some(owner);
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
        let Ok(plain) = postcard::to_allocvec(&self.chats) else {
            return false;
        };
        match store.write(NAMESPACE, &history_name(owner), &pack(&plain, true)) {
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

    fn fail_pending(&mut self) {
        let sending: Vec<(OnlineId, u64)> = self.sending.drain().map(|(_, v)| v).collect();
        let uploads: Vec<(OnlineId, u64, Vec<u8>)> = self
            .uploads
            .drain()
            .map(|(_, u)| (u.to, u.id, u.bytes))
            .collect();
        for (friend, id) in sending {
            self.set_delivery(friend, id, Delivery::Failed(Failure::NotConnected));
        }
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
    pub fn persist_chats(&mut self) {
        self.chat.store = Some(Backend::settings());
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
            ServerMsg::ChatSent { tag, id: _, at } => {
                if let Some((friend, id)) = self.chat.sending.remove(&tag)
                    && let Some(entry) = self.chat.entry_mut(friend, id)
                {
                    entry.delivery = Delivery::Sent;
                    entry.at = at;
                    self.chat.dirty = true;
                    self.chat.held.remove(&id);
                }
            }
            ServerMsg::ChatFailed { tag, error } => {
                if let Some((friend, id)) = self.chat.sending.remove(&tag) {
                    self.chat
                        .set_delivery(friend, id, Delivery::Failed(Failure::Refused(error)));
                }
            }
            ServerMsg::UploadReady(tag) => {
                if let Some(upload) = self.chat.uploads.get_mut(&tag) {
                    upload.ready = true;
                }
            }
            ServerMsg::Uploaded { tag, file } => {
                if let Some(upload) = self.chat.uploads.remove(&tag) {
                    if let Some(entry) = self.chat.entry_mut(upload.to, upload.id)
                        && let Body::Share(share) = &mut entry.body
                    {
                        share.file = file.id;
                        share.size = file.size;
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
        if let Body::Receipt(receipt) = &envelope.body {
            self.chat.take_receipt(from, receipt, at);
            return;
        }
        if let Body::Text(text) = &envelope.body
            && text.chars().count() > MAX_TEXT_CHARS
        {
            return;
        }
        self.chat.owe(from, envelope.id, false);
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
            },
        );
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
                || !self.chat.uploads.is_empty()
                || !self.chat.downloads.is_empty()
            {
                self.chat.fail_pending();
            }
        } else {
            self.pump_uploads(now);
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

    fn send_entry(&mut self, friend: OnlineId, id: u64) {
        let Some(entry) = self.chat.entry_mut(friend, id) else {
            return;
        };
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
            self.chat.sending.insert(tag, (friend, id));
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
            },
        );
        self.start_upload(friend, id, bytes);
        self.chat.sent += 1;
        true
    }

    pub fn retry(&mut self, friend: OnlineId, id: u64) {
        let Some(entry) = self.chat.entry_mut(friend, id) else {
            return;
        };
        if !matches!(entry.delivery, Delivery::Failed(_)) {
            return;
        }
        let needs_upload =
            matches!(&entry.body, Body::Share(share) if share.file == FileId([0; 16]));
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

pub type History = BTreeMap<OnlineId, Conversation>;

fn is_history_name(name: &str) -> bool {
    name.strip_suffix(".bin").is_some_and(|stem| {
        !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

pub fn decode_history(bytes: &[u8]) -> Option<History> {
    let plain = unpack(bytes, MAX_HISTORY_BYTES).ok()?;
    postcard::from_bytes(&plain).ok()
}

pub fn encode_history(history: &History) -> Option<Vec<u8>> {
    let plain = postcard::to_allocvec(history).ok()?;
    Some(pack(&plain, true))
}

pub fn merge_history(into: &mut History, from: &History) -> usize {
    let mut added = 0;
    for (friend, theirs) in from {
        let chat = into.entry(*friend).or_default();
        for entry in &theirs.entries {
            if !chat.entries.iter().any(|e| e.id == entry.id) {
                chat.entries.push(entry.clone());
                added += 1;
            }
        }
        chat.entries.sort_by_key(|e| e.at);
        let extra = chat.entries.len().saturating_sub(KEEP_PER_FRIEND);
        chat.entries.drain(..extra);
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mine(id: u64, delivery: Delivery) -> ChatEntry {
        ChatEntry {
            id,
            mine: true,
            at: 100,
            body: Body::Text("hi".into()),
            delivery,
        }
    }

    #[test]
    fn receipts_only_move_a_message_forward() {
        let friend = OnlineId::new(7).unwrap();
        let mut state = ChatState::default();
        for entry in [
            mine(1, Delivery::Sent),
            mine(2, Delivery::Delivered(150)),
            mine(3, Delivery::Read(160)),
            mine(4, Delivery::Sending),
            mine(5, Delivery::Failed(Failure::NotConnected)),
            mine(6, Delivery::Sent),
        ] {
            state.push(friend, entry);
        }
        let receipt = Receipt {
            delivered: vec![1, 2, 3, 4, 5, 6],
            read: vec![6],
        };
        state.take_receipt(friend, &receipt, 200);
        let got: Vec<Delivery> = state.chats[&friend]
            .entries
            .iter()
            .map(|e| e.delivery)
            .collect();
        assert_eq!(
            got,
            [
                Delivery::Delivered(200),
                Delivery::Delivered(150),
                Delivery::Read(160),
                Delivery::Sending,
                Delivery::Failed(Failure::NotConnected),
                Delivery::Read(200),
            ]
        );
    }

    #[test]
    fn a_receipt_never_touches_received_messages() {
        let friend = OnlineId::new(7).unwrap();
        let mut state = ChatState::default();
        let mut theirs = mine(1, Delivery::Received);
        theirs.mine = false;
        state.push(friend, theirs);
        let receipt = Receipt {
            delivered: vec![1],
            read: vec![1],
        };
        state.take_receipt(friend, &receipt, 200);
        assert_eq!(state.chats[&friend].entries[0].delivery, Delivery::Received);
    }

    #[test]
    fn old_history_still_loads_with_the_new_ticks() {
        #[derive(Serialize)]
        enum OlderDelivery {
            Sending,
            Sent,
            Failed(Failure),
            Received,
        }
        let older = postcard::to_allocvec(&[
            OlderDelivery::Sending,
            OlderDelivery::Sent,
            OlderDelivery::Failed(Failure::Gone),
            OlderDelivery::Received,
        ])
        .unwrap();
        let now: [Delivery; 4] = postcard::from_bytes(&older).unwrap();
        assert_eq!(
            now,
            [
                Delivery::Sending,
                Delivery::Sent,
                Delivery::Failed(Failure::Gone),
                Delivery::Received,
            ]
        );
    }

    #[test]
    fn owed_receipts_go_out_in_batches() {
        let mut ids: Vec<u64> = (0..MAX_RECEIPT_IDS as u64 + 5).collect();
        assert_eq!(take_first(&mut ids).len(), MAX_RECEIPT_IDS);
        assert_eq!(
            ids,
            (MAX_RECEIPT_IDS as u64..MAX_RECEIPT_IDS as u64 + 5).collect::<Vec<_>>()
        );
    }
}
