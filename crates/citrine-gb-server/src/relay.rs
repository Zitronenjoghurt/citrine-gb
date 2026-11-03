use crate::connections::{Refusal, Slot};
use crate::db::AccountRow;
use crate::hub::{Outbox, Queued};
use crate::limit::{Bucket, ConnLimit};
use crate::stats::Refused;
use crate::storage::Reservation;
use crate::{App, now};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use citrine_gb_net::Sealer;
use citrine_gb_net::{
    ClientMsg, ErrorCode, FileId, GameKey, MAX_FRAME_BYTES, MessageId, Notice, OnlineId,
    PROTOCOL_VERSION, Presence, ServerInfo, ServerMsg, Tag,
};
use futures_util::{SinkExt, StreamExt};
use handshake::handshake;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, broadcast, mpsc, watch};
use tokio::task::JoinHandle;

const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(30);
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const PING_EVERY: Duration = Duration::from_secs(25);
const MAX_UPLOADS: usize = 4;
const MAX_ACKS: usize = 1000;
const MESSAGE_PAGE: u32 = 200;
const SCREEN_EVERY_SECS: f64 = 30.0;
const SEAL_ROOM: usize = 1024;

mod account;
mod files;
mod friends;
mod handshake;
mod sessions;
mod sign_in;

pub async fn upgrade(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    let ip = app.client_ip(&headers, addr);
    let slot = match app.connections.admit(ip) {
        Ok(slot) => slot,
        Err(Refusal::Blocked) => {
            app.stats.refused(Refused::Blocked);
            return (
                StatusCode::FORBIDDEN,
                "This relay doesn't take connections from your address right now.",
            )
                .into_response();
        }
        Err(Refusal::Full) => {
            app.stats.refused(Refused::Connections);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "The relay is full. Try again later.",
            )
                .into_response();
        }
        Err(Refusal::Address) => {
            app.stats.refused(Refused::Connections);
            return (
                StatusCode::TOO_MANY_REQUESTS,
                "Too many connections from your address. Try again later.",
            )
                .into_response();
        }
    };
    ws.max_message_size(MAX_FRAME_BYTES + SEAL_ROOM)
        .max_frame_size(MAX_FRAME_BYTES + SEAL_ROOM)
        .on_upgrade(move |socket| run(app, socket, ip, slot))
}

fn screen_size(png: &[u8]) -> bool {
    let Some(header) = png.get(8..24) else {
        return false;
    };
    if &header[4..8] != b"IHDR" {
        return false;
    }
    let width = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
    let height = u32::from_be_bytes([header[12], header[13], header[14], header[15]]);
    (1..=4).any(|scale| width == 160 * scale && height == 144 * scale)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn file_path(app: &App, id: FileId) -> PathBuf {
    app.files_dir.join(hex(&id.0))
}

fn random_file_id() -> FileId {
    let mut id = [0; 16];
    getrandom::getrandom(&mut id).expect("no system random source");
    FileId(id)
}

pub fn info(app: &App) -> ServerInfo {
    let s = app.settings.get();
    ServerInfo {
        name: s.name,
        version: env!("CARGO_PKG_VERSION").to_string(),
        protocol: PROTOCOL_VERSION,
        password_required: s.password_set,
        sign_ups_open: s.sign_ups,
        invites_required: s.sign_ups && s.invites,
        quota_bytes: s.quota_bytes,
        max_message_bytes: s.max_message_bytes,
        max_file_bytes: s.max_file_bytes,
        game_sharing: s.game_sharing,
        pictures: s.pictures,
    }
}

pub fn quota_of(app: &App, account: &AccountRow) -> u64 {
    account
        .quota_bytes
        .unwrap_or_else(|| app.settings.get().quota_bytes)
}

fn db<T>(result: crate::db::Result<T>) -> Result<T, ErrorCode> {
    result.map_err(|e| {
        log::error!("database: {e}");
        ErrorCode::Server
    })
}

async fn writer(
    app: Arc<App>,
    mut sink: futures_util::stream::SplitSink<WebSocket, Message>,
    mut rx: mpsc::Receiver<Queued>,
    close: watch::Sender<Option<Notice>>,
    mut sealer: Sealer,
) {
    let limit = app.config.limits.send_timeout;
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    loop {
        let (message, room) = tokio::select! {
            queued = rx.recv() => {
                let Some(Queued { bytes, room }) = queued else { break };
                app.stats.sent(bytes.len());
                (Message::Binary(sealer.seal(&bytes).into()), Some(room))
            }
            _ = ping.tick() => (Message::Ping(Default::default()), None),
        };
        if !matches!(
            tokio::time::timeout(limit, sink.send(message)).await,
            Ok(Ok(()))
        ) {
            close.send_replace(Some(Notice::Disconnected));
            return;
        }
        drop(room);
    }
    let _ = tokio::time::timeout(limit, sink.send(Message::Close(None))).await;
}

struct Upload {
    id: FileId,
    size: u64,
    received: u64,
    file: tokio::fs::File,
    path: PathBuf,
    reserved: Reservation,
}

struct PutSave<'a> {
    game: GameKey,
    slot: &'a str,
    base: u64,
    label: &'a [u8],
    data: &'a [u8],
}

struct Me {
    id: i64,
    online_id: OnlineId,
}

struct Client {
    app: Arc<App>,
    ip: IpAddr,
    slot: Slot,
    conn_id: u64,
    outbox: Outbox,
    close: watch::Sender<Option<Notice>>,
    me: Option<Me>,
    hello: Option<u16>,
    limit: ConnLimit,
    limited_at: Option<Instant>,
    uploads: HashMap<Tag, Upload>,
    downloads: Vec<JoinHandle<()>>,
    download_slots: Arc<Semaphore>,
    screen: Option<String>,
    screen_limit: Bucket,
    device: Option<i64>,
    device_name: String,
    pending_totp: Option<Vec<u8>>,
}

enum Event {
    Frame(Vec<u8>),
    Closed,
    Close(Notice),
    Tick,
}

async fn run(app: Arc<App>, socket: WebSocket, ip: IpAddr, slot: Slot) {
    let (mut sink, mut stream) = socket.split();
    let Some(channel) = handshake(&app, &mut sink, &mut stream).await else {
        return;
    };
    let (sealer, mut opener) = channel.split();
    let (outbox, rx) = Outbox::new(app.config.limits.queue_bytes);
    let (close, mut close_rx) = watch::channel(None);
    let mut writer = tokio::spawn(writer(app.clone(), sink, rx, close.clone(), sealer));
    let mut gate = app.hub.gate();
    let mut client = Client {
        conn_id: app.hub.conn_id(),
        app: app.clone(),
        ip,
        slot,
        outbox,
        close,
        me: None,
        hello: None,
        limit: ConnLimit::default(),
        limited_at: None,
        uploads: HashMap::new(),
        downloads: Vec::new(),
        download_slots: Arc::new(Semaphore::new(app.config.limits.downloads_at_once)),
        screen: None,
        screen_limit: Bucket::new(1.0 / SCREEN_EVERY_SECS, 1.0),
        device: None,
        device_name: String::new(),
        pending_totp: None,
    };
    let settings = app.settings.get();
    let refused = if settings.kill {
        Some(Notice::KillSwitch)
    } else if !settings.relay {
        Some(Notice::RelayOff)
    } else {
        None
    };
    let started = Instant::now();
    let mut heard = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    if let Some(notice) = refused {
        client.send(ServerMsg::Notice(notice)).await;
    } else {
        loop {
            let event = tokio::select! {
                frame = stream.next() => match frame {
                    Some(Ok(Message::Binary(bytes))) => match opener.open(&bytes) {
                        Ok(plain) => Event::Frame(plain),
                        Err(_) => Event::Closed,
                    },
                    Some(Ok(Message::Text(_))) => Event::Frame(Vec::new()),
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => Event::Closed,
                    Some(Ok(_)) => { heard = Instant::now(); continue }
                },
                _ = close_rx.changed() => match *close_rx.borrow() {
                    Some(notice) => Event::Close(notice),
                    None => continue,
                },
                notice = gate.recv() => match notice {
                    Ok(notice) => Event::Close(notice),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => Event::Closed,
                },
                _ = tick.tick() => Event::Tick,
            };
            match event {
                Event::Frame(bytes) => {
                    if client.me.is_none() && bytes.len() > app.config.limits.sign_in_frame_bytes {
                        break;
                    }
                    heard = Instant::now();
                    app.stats.received(bytes.len());
                    client.frame(&bytes).await;
                }
                Event::Closed => break,
                Event::Close(notice) => {
                    client.send(ServerMsg::Notice(notice)).await;
                    break;
                }
                Event::Tick => {
                    let silent = heard.elapsed() > IDLE_TIMEOUT;
                    let anonymous = client.me.is_none() && started.elapsed() > SIGN_IN_TIMEOUT;
                    if silent || anonymous {
                        break;
                    }
                }
            }
        }
    }
    client.finish().await;
    drop(client);
    if tokio::time::timeout(Duration::from_secs(5), &mut writer)
        .await
        .is_err()
    {
        writer.abort();
    }
}

impl Client {
    async fn send(&self, msg: ServerMsg) {
        self.outbox.push(msg).await;
    }

    async fn error(&self, code: ErrorCode) {
        self.send(ServerMsg::Error(code)).await;
    }

    fn refused(&self, kind: Refused) {
        self.app.stats.refused(kind);
    }

    async fn room(&self, adding: u64) -> Result<(), ErrorCode> {
        let total = self.app.settings.get().storage_total_bytes;
        let result = self.app.storage.room(&self.app.db, adding, total).await;
        if result == Err(ErrorCode::QuotaFull) {
            self.refused(Refused::RelayStorage);
        }
        result
    }

    async fn rate_limited(&mut self) {
        if self
            .limited_at
            .is_none_or(|at| at.elapsed() > Duration::from_secs(1))
        {
            self.limited_at = Some(Instant::now());
            self.error(ErrorCode::RateLimited).await;
        }
    }

    async fn refuse(&mut self, msg: ClientMsg) {
        self.refused(Refused::Messages);
        let error = ErrorCode::RateLimited;
        let refusal = match msg {
            ClientMsg::Chat { tag, .. } => ServerMsg::ChatFailed { tag, error },
            ClientMsg::Upload { tag, .. } => ServerMsg::UploadFailed { tag, error },
            ClientMsg::Download(file) => ServerMsg::DownloadFailed { file, error },
            ClientMsg::PutSave { tag, .. } | ClientMsg::DeleteSave { tag, .. } => {
                ServerMsg::SaveRefused {
                    tag,
                    error,
                    current: None,
                }
            }
            ClientMsg::Screen(_) => ServerMsg::ScreenRefused(error),
            _ => return self.rate_limited().await,
        };
        self.send(refusal).await;
    }

    async fn frame(&mut self, bytes: &[u8]) {
        if !self.limit.allow(bytes.len()) {
            self.refused(Refused::Messages);
            self.rate_limited().await;
            return;
        }
        let Ok(msg) = ClientMsg::decode(bytes) else {
            self.error(ErrorCode::BadMessage).await;
            return;
        };
        if let Err(code) = self.handle(msg).await {
            self.error(code).await;
        }
    }

    fn me(&self) -> Result<&Me, ErrorCode> {
        self.me.as_ref().ok_or(ErrorCode::NotSignedIn)
    }

    async fn handle(&mut self, msg: ClientMsg) -> Result<(), ErrorCode> {
        match msg {
            ClientMsg::Hello { protocol, .. } => {
                self.hello = Some(protocol);
                self.send(ServerMsg::Info(info(&self.app))).await;
                if protocol != PROTOCOL_VERSION {
                    return Err(ErrorCode::Protocol {
                        server: PROTOCOL_VERSION,
                    });
                }
                Ok(())
            }
            ClientMsg::Ping(n) => {
                self.send(ServerMsg::Pong(n)).await;
                Ok(())
            }
            ClientMsg::SignUp {
                relay_password,
                auth_key,
                public_key,
                display_name,
                invite,
                device_name,
            } => {
                self.device_name = citrine_gb_net::device_name(&device_name);
                self.sign_in(
                    relay_password,
                    auth_key,
                    Some((public_key, display_name, invite)),
                    None,
                )
                .await
            }
            ClientMsg::SignIn {
                relay_password,
                auth_key,
                device,
                code,
                device_name,
            } => {
                self.device_name = citrine_gb_net::device_name(&device_name);
                self.sign_in(relay_password, auth_key, None, Some((device, code)))
                    .await
            }
            other => {
                self.me()?;
                self.signed_in(other).await
            }
        }
    }

    async fn signed_in(&mut self, msg: ClientMsg) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let streamed = matches!(
            msg,
            ClientMsg::Play { .. } | ClientMsg::Game { .. } | ClientMsg::UploadChunk { .. }
        );
        if !streamed && !self.app.rates.messages.take(me, 1.0) {
            self.refuse(msg).await;
            return Ok(());
        }
        match msg {
            ClientMsg::SetDisplayName(name) => self.set_name(&name).await,
            ClientMsg::SetStatus { away } => {
                let me = self.me()?.id;
                match self
                    .app
                    .hub
                    .set_presence(me, self.conn_id, Presence::Online { away })
                {
                    Some(presence) => self.tell_friends(presence).await,
                    None => Ok(()),
                }
            }
            ClientMsg::AddFriend(to) => self.add_friend(to).await,
            ClientMsg::AnswerFriend { from, accept } => self.answer_friend(from, accept).await,
            ClientMsg::RemoveFriend(who) => self.remove_friend(who, false).await,
            ClientMsg::Block(who) => self.remove_friend(who, true).await,
            ClientMsg::Unblock(who) => {
                let other = self.user(who).await?;
                db(self.app.db.unblock(self.me()?.id, other.id).await)?;
                self.tell_me(ServerMsg::FriendGone(who)).await;
                Ok(())
            }
            ClientMsg::Chat { tag, to, sealed } => {
                if let Err(error) = self.chat(tag, to, sealed).await {
                    self.send(ServerMsg::ChatFailed { tag, error }).await;
                }
                Ok(())
            }
            ClientMsg::ChatAck(ids) => {
                let ids: Vec<MessageId> = ids.into_iter().take(MAX_ACKS).collect();
                let device = self.device.ok_or(ErrorCode::NotSignedIn)?;
                db(self.app.db.ack_messages(device, &ids).await)
            }
            ClientMsg::Restate { id, sealed } => {
                if let Some(sealed) = &sealed
                    && sealed.ciphertext.len() as u64 > self.app.settings.get().max_message_bytes
                {
                    return Err(ErrorCode::TooLarge);
                }
                let me = self.me()?.id;
                db(self.app.db.restate_message(me, id, sealed.as_ref()).await)?;
                Ok(())
            }
            ClientMsg::SignOutDevice(device) => self.sign_out_device(device).await,
            ClientMsg::Upload { tag, size } => {
                if let Err(error) = self.upload(tag, size).await {
                    self.send(ServerMsg::UploadFailed { tag, error }).await;
                }
                Ok(())
            }
            ClientMsg::UploadChunk { tag, data } => {
                if let Err(error) = self.chunk(tag, &data).await {
                    if let Some(upload) = self.uploads.remove(&tag) {
                        let _ = tokio::fs::remove_file(&upload.path).await;
                    }
                    self.send(ServerMsg::UploadFailed { tag, error }).await;
                }
                Ok(())
            }
            ClientMsg::Download(file) => {
                if let Err(error) = self.download(file).await {
                    self.send(ServerMsg::DownloadFailed { file, error }).await;
                }
                Ok(())
            }
            ClientMsg::DeleteFile(file) => {
                let me = self.me()?.id;
                if !db(self.app.db.delete_file(file, me).await)? {
                    return Err(ErrorCode::UnknownFile);
                }
                let _ = tokio::fs::remove_file(file_path(&self.app, file)).await;
                self.tell_me(ServerMsg::FileDeleted(file)).await;
                self.send_account().await
            }
            ClientMsg::ListFiles => {
                let files = db(self.app.db.files_of(self.me()?.id).await)?;
                self.send(ServerMsg::Files(files)).await;
                Ok(())
            }
            ClientMsg::Invite { to, sealed } => self.invite(to, sealed).await,
            ClientMsg::AnswerInvite { session, accept } => {
                self.answer_invite(session, accept).await
            }
            ClientMsg::Leave(session) => {
                let me = self.me()?.id;
                let other = self
                    .app
                    .hub
                    .end(session, me, self.conn_id)
                    .ok_or(ErrorCode::UnknownSession)?;
                let ended = ServerMsg::SessionEnded {
                    session,
                    reason: citrine_gb_net::EndReason::Left,
                };
                self.app.hub.send_to(other, ended.clone());
                self.app.hub.send_others(me, self.conn_id, ended);
                Ok(())
            }
            ClientMsg::Play { session, sealed } => {
                let other = self.peer(session)?;
                if !self.session_room(session, &sealed) {
                    self.refused(Refused::Play);
                    self.rate_limited().await;
                    return Ok(());
                }
                self.app
                    .hub
                    .send_to(other, ServerMsg::Play { session, sealed });
                Ok(())
            }
            ClientMsg::Game { session, sealed } => {
                if !self.app.settings.get().game_sharing {
                    return Err(ErrorCode::GameSharingOff);
                }
                let other = self.peer(session)?;
                if !self.session_room(session, &sealed) {
                    self.refused(Refused::Play);
                    self.rate_limited().await;
                    return Ok(());
                }
                self.app
                    .hub
                    .send_to(other, ServerMsg::Game { session, sealed });
                Ok(())
            }
            ClientMsg::Playing { to, sealed } => {
                let me = self.me()?;
                let from = me.online_id;
                let me = me.id;
                let other = self.user(to).await?;
                if db(self.app.db.are_friends(me, other.id).await)? {
                    self.app
                        .hub
                        .send(other.id, ServerMsg::Playing { from, sealed });
                }
                Ok(())
            }
            ClientMsg::Typing { to, typing } => self.typing(to, typing).await,
            ClientMsg::ListSaves => {
                let saves = db(self.app.db.saves_of(self.me()?.id).await)?;
                self.send(ServerMsg::Saves(saves)).await;
                Ok(())
            }
            ClientMsg::PutSave {
                tag,
                game,
                slot,
                base,
                label,
                data,
            } => {
                let put = PutSave {
                    game,
                    slot: &slot,
                    base,
                    label: &label,
                    data: &data,
                };
                if let Err((error, current)) = self.put_save(tag, put).await {
                    self.send(ServerMsg::SaveRefused {
                        tag,
                        error,
                        current,
                    })
                    .await;
                }
                Ok(())
            }
            ClientMsg::GetSave { game, slot } => self.get_save(game, slot).await,
            ClientMsg::DeleteSave {
                tag,
                game,
                slot,
                base,
            } => {
                let me = self.me()?.id;
                match db(self.app.db.delete_save(me, game, &slot, base).await)? {
                    Ok(file) => {
                        let _ = tokio::fs::remove_file(file_path(&self.app, file)).await;
                        self.send(ServerMsg::SaveDeleted { tag }).await;
                        self.others(ServerMsg::SavesChanged)?;
                        self.send_account().await
                    }
                    Err(None) => {
                        self.send(ServerMsg::SaveDeleted { tag }).await;
                        Ok(())
                    }
                    Err(current) => {
                        self.send(ServerMsg::SaveRefused {
                            tag,
                            error: ErrorCode::SaveChanged,
                            current,
                        })
                        .await;
                        Ok(())
                    }
                }
            }
            ClientMsg::Screen(png) => {
                self.screen(png).await;
                Ok(())
            }
            ClientMsg::TwoFactorStart => self.two_factor_start().await,
            ClientMsg::TwoFactorConfirm(code) => self.two_factor_confirm(code).await,
            ClientMsg::TwoFactorOff(code) => self.two_factor_off(code).await,
            ClientMsg::NewRecoveryCodes(code) => self.new_recovery_codes(code).await,
            ClientMsg::DeleteAccount { code } => self.delete_account(code).await,
            ClientMsg::Hello { .. }
            | ClientMsg::Ping(_)
            | ClientMsg::SignUp { .. }
            | ClientMsg::SignIn { .. } => Err(ErrorCode::AlreadySignedIn),
        }
    }

    async fn tell_me(&self, msg: ServerMsg) {
        if let Some(me) = &self.me {
            self.app.hub.send_others(me.id, self.conn_id, msg.clone());
        }
        self.send(msg).await;
    }

    fn others(&self, msg: ServerMsg) -> Result<(), ErrorCode> {
        self.app.hub.send_others(self.me()?.id, self.conn_id, msg);
        Ok(())
    }

    async fn finish(&mut self) {
        if let Some(token) = self.screen.take() {
            self.app.screens.remove(&token);
        }
        for download in self.downloads.drain(..) {
            download.abort();
        }
        for (_, upload) in self.uploads.drain() {
            drop(upload.file);
            let _ = tokio::fs::remove_file(&upload.path).await;
        }
        let Some(me) = self.me.as_ref().map(|me| me.id) else {
            return;
        };
        let before = self.app.hub.presence(me);
        let Some(left) = self.app.hub.leave(me, self.conn_id) else {
            return;
        };
        let last_seen = now();
        if let Some(device) = self.device {
            let _ = db(self.app.db.device_seen(device, last_seen).await);
        }
        if left.last {
            let _ = db(self.app.db.set_last_seen(me, last_seen).await);
            let _ = self.tell_friends(Presence::Offline { last_seen }).await;
        } else {
            if let Some(after) = self.app.hub.presence(me)
                && Some(after) != before
            {
                let _ = self.tell_friends(after).await;
            }
            let _ = self.send_devices().await;
        }
        for (session, other) in left.ended {
            self.app.hub.send_to(
                other,
                ServerMsg::SessionEnded {
                    session,
                    reason: citrine_gb_net::EndReason::Disconnected,
                },
            );
        }
    }
}
