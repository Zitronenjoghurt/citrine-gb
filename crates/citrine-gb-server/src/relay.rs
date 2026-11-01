use crate::db::{AccountRow, InviteRefusal, NewSave, SaveWrite};
use crate::hub::{Conn, QUEUE};
use crate::limit::{Bucket, ConnLimit};
use crate::screens::PNG_SIGNATURE;
use crate::{App, now};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::response::Response;
use citrine_gb_net::{
    Account, AuthKey, CHUNK_BYTES, ChatMessage, ClientMsg, ErrorCode, FileId, FileInfo, Friend,
    FriendState, GameKey, InviteCode, MAX_FRAME_BYTES, MAX_LABEL_BYTES, MAX_SCREEN_BYTES,
    MessageId, Notice, OnlineId, PROTOCOL_VERSION, Presence, PublicKey, RelayPassword, SaveInfo,
    Sealed, ServerInfo, ServerMsg, SessionId, Tag, display_name, valid_slot,
};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;

const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(30);
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const PING_EVERY: Duration = Duration::from_secs(25);
const MAX_UPLOADS: usize = 4;
const MAX_ACKS: usize = 1000;
const SCREEN_EVERY_SECS: f64 = 30.0;

pub async fn upgrade(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    let ip = app.client_ip(&headers, addr);
    ws.max_message_size(MAX_FRAME_BYTES)
        .max_frame_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| run(app, socket, ip))
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
        game_sharing: app.config.game_sharing,
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
    mut rx: mpsc::Receiver<ServerMsg>,
) {
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    loop {
        tokio::select! {
            msg = rx.recv() => {
                let Some(msg) = msg else { break };
                let Ok(bytes) = msg.encode() else { continue };
                app.stats.sent(bytes.len());
                if sink.send(Message::Binary(bytes.into())).await.is_err() {
                    return;
                }
            }
            _ = ping.tick() => {
                if sink.send(Message::Ping(Default::default())).await.is_err() {
                    return;
                }
            }
        }
    }
    let _ = sink.send(Message::Close(None)).await;
}

struct Upload {
    id: FileId,
    size: u64,
    received: u64,
    file: tokio::fs::File,
    path: PathBuf,
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
    conn_id: u64,
    tx: mpsc::Sender<ServerMsg>,
    close: watch::Sender<Option<Notice>>,
    me: Option<Me>,
    hello: Option<u16>,
    limit: ConnLimit,
    limited_at: Option<Instant>,
    uploads: HashMap<Tag, Upload>,
    downloads: Vec<JoinHandle<()>>,
    screen: Option<String>,
    screen_limit: Bucket,
}

enum Event {
    Frame(Vec<u8>),
    Closed,
    Close(Notice),
    Tick,
}

async fn run(app: Arc<App>, socket: WebSocket, ip: IpAddr) {
    let (sink, mut stream) = socket.split();
    let (tx, rx) = mpsc::channel(QUEUE);
    let writer = tokio::spawn(writer(app.clone(), sink, rx));
    let (close, mut close_rx) = watch::channel(None);
    let mut gate = app.hub.gate();
    let mut client = Client {
        conn_id: app.hub.conn_id(),
        app: app.clone(),
        ip,
        tx,
        close,
        me: None,
        hello: None,
        limit: ConnLimit::default(),
        limited_at: None,
        uploads: HashMap::new(),
        downloads: Vec::new(),
        screen: None,
        screen_limit: Bucket::new(1.0 / SCREEN_EVERY_SECS, 1.0),
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
                    Some(Ok(Message::Binary(bytes))) => Event::Frame(bytes.to_vec()),
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
    let _ = tokio::time::timeout(Duration::from_secs(5), writer).await;
}

impl Client {
    async fn send(&self, msg: ServerMsg) {
        let _ = self.tx.send(msg).await;
    }

    async fn error(&self, code: ErrorCode) {
        self.send(ServerMsg::Error(code)).await;
    }

    async fn frame(&mut self, bytes: &[u8]) {
        if !self.limit.allow(bytes.len()) {
            if self
                .limited_at
                .is_none_or(|at| at.elapsed() > Duration::from_secs(1))
            {
                self.limited_at = Some(Instant::now());
                self.error(ErrorCode::RateLimited).await;
            }
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
            } => {
                self.sign_in(
                    relay_password,
                    auth_key,
                    Some((public_key, display_name, invite)),
                )
                .await
            }
            ClientMsg::SignIn {
                relay_password,
                auth_key,
            } => self.sign_in(relay_password, auth_key, None).await,
            other => {
                self.me()?;
                self.signed_in(other).await
            }
        }
    }

    async fn signed_in(&mut self, msg: ClientMsg) -> Result<(), ErrorCode> {
        match msg {
            ClientMsg::SetDisplayName(name) => self.set_name(&name).await,
            ClientMsg::SetStatus { away } => {
                let presence = Presence::Online { away };
                let me = self.me()?;
                self.app.hub.set_presence(me.id, presence);
                self.tell_friends(presence).await
            }
            ClientMsg::AddFriend(to) => self.add_friend(to).await,
            ClientMsg::AnswerFriend { from, accept } => self.answer_friend(from, accept).await,
            ClientMsg::RemoveFriend(who) => self.remove_friend(who, false).await,
            ClientMsg::Block(who) => self.remove_friend(who, true).await,
            ClientMsg::Unblock(who) => {
                let other = self.user(who).await?;
                db(self.app.db.unblock(self.me()?.id, other.id).await)?;
                self.send(ServerMsg::FriendGone(who)).await;
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
                db(self.app.db.ack_messages(self.me()?.id, &ids).await)
            }
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
                self.send(ServerMsg::FileDeleted(file)).await;
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
                let other = self
                    .app
                    .hub
                    .end(session, self.me()?.id)
                    .ok_or(ErrorCode::UnknownSession)?;
                self.app.hub.send(
                    other,
                    ServerMsg::SessionEnded {
                        session,
                        reason: citrine_gb_net::EndReason::Left,
                    },
                );
                Ok(())
            }
            ClientMsg::Play { session, sealed } => {
                let other = self.peer(session)?;
                self.app
                    .hub
                    .send(other, ServerMsg::Play { session, sealed });
                Ok(())
            }
            ClientMsg::Game { session, sealed } => {
                if !self.app.config.game_sharing {
                    return Err(ErrorCode::GameSharingOff);
                }
                let other = self.peer(session)?;
                self.app
                    .hub
                    .send(other, ServerMsg::Game { session, sealed });
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
            ClientMsg::Hello { .. }
            | ClientMsg::Ping(_)
            | ClientMsg::SignUp { .. }
            | ClientMsg::SignIn { .. } => Err(ErrorCode::AlreadySignedIn),
        }
    }

    async fn sign_in(
        &mut self,
        password: Option<RelayPassword>,
        auth_key: AuthKey,
        new: Option<(PublicKey, String, Option<InviteCode>)>,
    ) -> Result<(), ErrorCode> {
        if self.me.is_some() {
            return Err(ErrorCode::AlreadySignedIn);
        }
        if self.hello != Some(PROTOCOL_VERSION) {
            return Err(ErrorCode::Protocol {
                server: PROTOCOL_VERSION,
            });
        }
        let attempts = &self.app.relay_attempts;
        if attempts.blocked(self.ip) {
            return Err(ErrorCode::TooManyTries);
        }
        let ok = self
            .app
            .settings
            .check_password(password.map(|p| p.0))
            .await;
        if !ok {
            attempts.fail(self.ip);
            return Err(ErrorCode::WrongPassword);
        }
        let hash = auth_key.hash();
        let existing = db(self.app.db.account_by_auth(&hash).await)?;
        let account = match (new, existing) {
            (Some(_), Some(_)) => return Err(ErrorCode::AccountExists),
            (Some((public_key, name, invite)), None) => {
                let settings = self.app.settings.get();
                if !settings.sign_ups {
                    return Err(ErrorCode::SignUpsClosed);
                }
                let name = display_name(&name).map_err(|_| ErrorCode::BadName)?;
                if !settings.invites {
                    db(self
                        .app
                        .db
                        .create_account(&hash, &public_key, &name, now())
                        .await)?
                } else {
                    let invite = invite.ok_or(ErrorCode::InviteNeeded)?;
                    let made = self
                        .app
                        .db
                        .create_account_with_invite(&hash, &public_key, &name, now(), invite)
                        .await;
                    match db(made)? {
                        Ok(account) => account,
                        Err(refusal) => {
                            attempts.fail(self.ip);
                            return Err(match refusal {
                                InviteRefusal::Wrong => ErrorCode::InviteWrong,
                                InviteRefusal::Used => ErrorCode::InviteUsed,
                            });
                        }
                    }
                }
            }
            (None, Some(account)) => account,
            (None, None) => {
                attempts.fail(self.ip);
                return Err(ErrorCode::UnknownAccount);
            }
        };
        if account.banned {
            return Err(ErrorCode::Banned);
        }
        self.enter(account).await
    }

    async fn enter(&mut self, account: AccountRow) -> Result<(), ErrorCode> {
        let presence = Presence::Online { away: false };
        self.me = Some(Me {
            id: account.id,
            online_id: account.online_id,
        });
        let online = self.app.hub.join(
            account.id,
            Conn {
                conn_id: self.conn_id,
                tx: self.tx.clone(),
                close: self.close.clone(),
                presence,
            },
        );
        self.app.stats.connected(online);
        db(self.app.db.set_last_seen(account.id, now()).await)?;
        let welcome = self.account(&account).await?;
        self.send(ServerMsg::Welcome(welcome)).await;
        let friends = db(self.app.db.relations(account.id).await)?
            .into_iter()
            .map(|(row, state)| self.friend(&row, state))
            .collect();
        self.send(ServerMsg::Friends(friends)).await;
        for (id, from, at, sealed) in db(self.app.db.pending_messages(account.id).await)? {
            self.send(ServerMsg::Chat(ChatMessage {
                id,
                from,
                at,
                sealed,
            }))
            .await;
        }
        self.tell_friends(presence).await
    }

    async fn account(&self, row: &AccountRow) -> Result<Account, ErrorCode> {
        Ok(Account {
            online_id: row.online_id,
            display_name: row.display_name.clone(),
            storage_used: db(self.app.db.storage_used(row.id).await)?,
            quota_bytes: quota_of(&self.app, row),
        })
    }

    async fn my_row(&self) -> Result<AccountRow, ErrorCode> {
        db(self.app.db.account(self.me()?.id).await)?.ok_or(ErrorCode::Server)
    }

    async fn send_account(&self) -> Result<(), ErrorCode> {
        let account = self.account(&self.my_row().await?).await?;
        self.send(ServerMsg::Account(account)).await;
        Ok(())
    }

    fn friend(&self, row: &AccountRow, state: FriendState) -> Friend {
        let presence = (state == FriendState::Friend).then(|| {
            self.app.hub.presence(row.id).unwrap_or(Presence::Offline {
                last_seen: row.last_seen,
            })
        });
        Friend {
            online_id: row.online_id,
            display_name: row.display_name.clone(),
            public_key: row.public_key,
            state,
            presence,
        }
    }

    async fn user(&self, online: OnlineId) -> Result<AccountRow, ErrorCode> {
        let me = self.me()?;
        match db(self.app.db.account_by_online(online).await)? {
            Some(row) if row.id != me.id => Ok(row),
            _ => Err(ErrorCode::UnknownUser),
        }
    }

    async fn tell_friends(&self, presence: Presence) -> Result<(), ErrorCode> {
        let me = self.me()?;
        for friend in db(self.app.db.friend_ids(me.id).await)? {
            self.app.hub.send(
                friend,
                ServerMsg::Presence {
                    who: me.online_id,
                    presence,
                },
            );
        }
        Ok(())
    }

    async fn set_name(&mut self, name: &str) -> Result<(), ErrorCode> {
        let name = display_name(name).map_err(|_| ErrorCode::BadName)?;
        let me = self.me()?.id;
        db(self.app.db.set_display_name(me, &name).await)?;
        let row = self.my_row().await?;
        self.send(ServerMsg::Account(self.account(&row).await?))
            .await;
        for (other, state) in db(self.app.db.relations(me).await)? {
            let seen_as = match state {
                FriendState::Friend => FriendState::Friend,
                FriendState::Incoming => FriendState::Outgoing,
                FriendState::Outgoing => FriendState::Incoming,
                FriendState::Blocked => continue,
            };
            if seen_as == FriendState::Outgoing && db(self.app.db.is_blocked(other.id, me).await)? {
                continue;
            }
            self.app
                .hub
                .send(other.id, ServerMsg::Friend(self.friend(&row, seen_as)));
        }
        Ok(())
    }

    async fn add_friend(&mut self, to: OnlineId) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let other = self.user(to).await?;
        let app = self.app.clone();
        if db(app.db.are_friends(me, other.id).await)? {
            self.send(ServerMsg::Friend(self.friend(&other, FriendState::Friend)))
                .await;
            return Ok(());
        }
        if db(app.db.has_request(other.id, me).await)? {
            return self.answer_friend(to, true).await;
        }
        db(app.db.unblock(me, other.id).await)?;
        db(app.db.add_request(me, other.id, now()).await)?;
        self.send(ServerMsg::Friend(
            self.friend(&other, FriendState::Outgoing),
        ))
        .await;
        if !db(app.db.is_blocked(other.id, me).await)? {
            let row = self.my_row().await?;
            app.hub.send(
                other.id,
                ServerMsg::Friend(self.friend(&row, FriendState::Incoming)),
            );
        }
        Ok(())
    }

    async fn answer_friend(&mut self, from: OnlineId, accept: bool) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let my_online = self.me()?.online_id;
        let other = self.user(from).await?;
        let app = self.app.clone();
        if !db(app.db.has_request(other.id, me).await)? {
            return Err(ErrorCode::UnknownUser);
        }
        if accept {
            db(app.db.befriend(me, other.id, now()).await)?;
            self.send(ServerMsg::Friend(self.friend(&other, FriendState::Friend)))
                .await;
            let row = self.my_row().await?;
            app.hub.send(
                other.id,
                ServerMsg::Friend(self.friend(&row, FriendState::Friend)),
            );
        } else {
            db(app.db.unfriend(me, other.id).await)?;
            self.send(ServerMsg::FriendGone(from)).await;
            app.hub.send(other.id, ServerMsg::FriendGone(my_online));
        }
        Ok(())
    }

    async fn remove_friend(&mut self, who: OnlineId, block: bool) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let my_online = self.me()?.online_id;
        let other = self.user(who).await?;
        let app = self.app.clone();
        if block {
            db(app.db.block(me, other.id).await)?;
            self.send(ServerMsg::Friend(self.friend(&other, FriendState::Blocked)))
                .await;
        } else {
            db(app.db.unfriend(me, other.id).await)?;
            self.send(ServerMsg::FriendGone(who)).await;
        }
        app.hub.send(other.id, ServerMsg::FriendGone(my_online));
        for session in app.hub.end_between(me, other.id) {
            let ended = ServerMsg::SessionEnded {
                session,
                reason: citrine_gb_net::EndReason::Left,
            };
            app.hub.send(other.id, ended.clone());
            self.send(ended).await;
        }
        Ok(())
    }

    async fn chat(&mut self, tag: Tag, to: OnlineId, sealed: Sealed) -> Result<(), ErrorCode> {
        let me = self.me()?;
        let (me_id, me_online) = (me.id, me.online_id);
        let other = self.user(to).await?;
        if !db(self.app.db.are_friends(me_id, other.id).await)? {
            return Err(ErrorCode::NotFriends);
        }
        if sealed.ciphertext.len() as u64 > self.app.settings.get().max_message_bytes {
            return Err(ErrorCode::TooLarge);
        }
        let at = now();
        let id = db(self
            .app
            .db
            .insert_message(me_id, other.id, at, &sealed)
            .await)?;
        self.app.stats.message();
        self.send(ServerMsg::ChatSent { tag, id, at }).await;
        self.app.hub.send(
            other.id,
            ServerMsg::Chat(ChatMessage {
                id,
                from: me_online,
                at,
                sealed,
            }),
        );
        Ok(())
    }

    async fn upload(&mut self, tag: Tag, size: u64) -> Result<(), ErrorCode> {
        if self.uploads.contains_key(&tag) {
            return Err(ErrorCode::BadMessage);
        }
        if self.uploads.len() >= MAX_UPLOADS {
            return Err(ErrorCode::RateLimited);
        }
        let row = self.my_row().await?;
        let quota = quota_of(&self.app, &row);
        if quota == 0 {
            return Err(ErrorCode::SharingOff);
        }
        if size == 0 || size > self.app.settings.get().max_file_bytes {
            return Err(ErrorCode::TooLarge);
        }
        let reserved: u64 = self.uploads.values().map(|u| u.size).sum();
        let used = db(self.app.db.storage_used(row.id).await)? + reserved;
        if used.saturating_add(size) > quota {
            return Err(ErrorCode::QuotaFull);
        }
        let id = random_file_id();
        let path = self.app.files_dir.join(format!("{}.part", hex(&id.0)));
        let file = tokio::fs::File::create(&path).await.map_err(|e| {
            log::error!("upload: {e}");
            ErrorCode::Server
        })?;
        self.uploads.insert(
            tag,
            Upload {
                id,
                size,
                received: 0,
                file,
                path,
            },
        );
        self.send(ServerMsg::UploadReady(tag)).await;
        Ok(())
    }

    async fn chunk(&mut self, tag: Tag, data: &[u8]) -> Result<(), ErrorCode> {
        let upload = self.uploads.get_mut(&tag).ok_or(ErrorCode::UnknownFile)?;
        if upload.received + data.len() as u64 > upload.size {
            return Err(ErrorCode::TooLarge);
        }
        let io = |e: std::io::Error| {
            log::error!("upload: {e}");
            ErrorCode::Server
        };
        upload.file.write_all(data).await.map_err(io)?;
        upload.received += data.len() as u64;
        if upload.received < upload.size {
            return Ok(());
        }
        let mut upload = self.uploads.remove(&tag).ok_or(ErrorCode::Server)?;
        upload.file.flush().await.map_err(io)?;
        upload.file.sync_all().await.map_err(io)?;
        let path = file_path(&self.app, upload.id);
        tokio::fs::rename(&upload.path, &path).await.map_err(io)?;
        let created = now();
        let me = self.me()?.id;
        if let Err(e) = db(self
            .app
            .db
            .insert_file(upload.id, me, upload.size, created)
            .await)
        {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(e);
        }
        self.send(ServerMsg::Uploaded {
            tag,
            file: FileInfo {
                id: upload.id,
                size: upload.size,
                created,
            },
        })
        .await;
        self.send_account().await
    }

    async fn download(&mut self, file: FileId) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let (owner, info) = db(self.app.db.file(file).await)?.ok_or(ErrorCode::UnknownFile)?;
        if owner != me && !db(self.app.db.are_friends(owner, me).await)? {
            return Err(ErrorCode::UnknownFile);
        }
        let path = file_path(&self.app, file);
        let tx = self.tx.clone();
        self.downloads.retain(|d| !d.is_finished());
        self.downloads.push(tokio::spawn(async move {
            let Ok(mut f) = tokio::fs::File::open(&path).await else {
                let _ = tx
                    .send(ServerMsg::DownloadFailed {
                        file,
                        error: ErrorCode::UnknownFile,
                    })
                    .await;
                return;
            };
            if tx
                .send(ServerMsg::FileStart {
                    file,
                    size: info.size,
                })
                .await
                .is_err()
            {
                return;
            }
            let mut left = info.size;
            while left > 0 {
                let mut data = vec![0; CHUNK_BYTES.min(left as usize)];
                if f.read_exact(&mut data).await.is_err() {
                    let _ = tx
                        .send(ServerMsg::DownloadFailed {
                            file,
                            error: ErrorCode::Server,
                        })
                        .await;
                    return;
                }
                left -= data.len() as u64;
                if tx.send(ServerMsg::FileChunk { file, data }).await.is_err() {
                    return;
                }
            }
        }));
        Ok(())
    }

    async fn put_save(
        &mut self,
        tag: Tag,
        put: PutSave<'_>,
    ) -> Result<(), (ErrorCode, Option<SaveInfo>)> {
        let plain = |code: ErrorCode| (code, None);
        if !valid_slot(put.slot) || put.label.len() > MAX_LABEL_BYTES {
            return Err(plain(ErrorCode::BadMessage));
        }
        let row = self.my_row().await.map_err(plain)?;
        let quota = quota_of(&self.app, &row);
        if quota == 0 {
            return Err(plain(ErrorCode::SharingOff));
        }
        let size = put.data.len() as u64;
        if size == 0 || size > self.app.settings.get().max_file_bytes {
            return Err(plain(ErrorCode::TooLarge));
        }
        let current = db(self.app.db.save(row.id, put.game, put.slot).await).map_err(plain)?;
        let version = current.as_ref().map_or(0, |(info, _)| info.version);
        if version != put.base {
            return Err((ErrorCode::SaveChanged, current.map(|(info, _)| info)));
        }
        let replacing = current.as_ref().map_or(0, |(info, _)| info.size);
        let reserved: u64 = self.uploads.values().map(|u| u.size).sum();
        let used = db(self.app.db.storage_used(row.id).await).map_err(plain)? + reserved;
        if used.saturating_sub(replacing).saturating_add(size) > quota {
            return Err(plain(ErrorCode::QuotaFull));
        }
        let id = random_file_id();
        let path = file_path(&self.app, id);
        let part = self.app.files_dir.join(format!("{}.part", hex(&id.0)));
        let io = |e: std::io::Error| {
            log::error!("save: {e}");
            plain(ErrorCode::Server)
        };
        let mut file = tokio::fs::File::create(&part).await.map_err(io)?;
        file.write_all(put.data).await.map_err(io)?;
        file.sync_all().await.map_err(io)?;
        drop(file);
        tokio::fs::rename(&part, &path).await.map_err(io)?;
        let new = NewSave {
            game: put.game,
            slot: put.slot,
            base: put.base,
            label: put.label,
            file: id,
            size,
        };
        let written = match db(self.app.db.put_save(row.id, new, now()).await) {
            Ok(written) => written,
            Err(code) => {
                let _ = tokio::fs::remove_file(&path).await;
                return Err(plain(code));
            }
        };
        match written {
            SaveWrite::Stored { info, replaced } => {
                if let Some(old) = replaced {
                    let _ = tokio::fs::remove_file(file_path(&self.app, old)).await;
                }
                self.send(ServerMsg::SaveStored { tag, info }).await;
                self.send_account().await.map_err(plain)
            }
            SaveWrite::Changed(current) => {
                let _ = tokio::fs::remove_file(&path).await;
                Err((ErrorCode::SaveChanged, current))
            }
        }
    }

    async fn get_save(&mut self, game: GameKey, slot: String) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let Some((info, file)) = db(self.app.db.save(me, game, &slot).await)? else {
            self.send(ServerMsg::SaveMissing { game, slot }).await;
            return Ok(());
        };
        match tokio::fs::read(file_path(&self.app, file)).await {
            Ok(data) => self.send(ServerMsg::Save { info, data }).await,
            Err(e) => {
                log::error!("save {}: {e}", hex(&file.0));
                self.send(ServerMsg::SaveMissing { game, slot }).await;
            }
        }
        Ok(())
    }

    fn peer(&self, session: SessionId) -> Result<i64, ErrorCode> {
        self.app
            .hub
            .peer(session, self.me()?.id)
            .ok_or(ErrorCode::UnknownSession)
    }

    async fn invite(&mut self, to: OnlineId, sealed: Sealed) -> Result<(), ErrorCode> {
        let me = self.me()?;
        let (me_id, me_online) = (me.id, me.online_id);
        let other = self.user(to).await?;
        if !db(self.app.db.are_friends(me_id, other.id).await)? {
            return Err(ErrorCode::NotFriends);
        }
        if !self.app.hub.is_online(other.id) {
            return Err(ErrorCode::Offline);
        }
        let session = self.app.hub.invite(me_id, other.id);
        self.send(ServerMsg::Invited { session, to }).await;
        self.app.hub.send(
            other.id,
            ServerMsg::Invite {
                session,
                from: me_online,
                sealed,
            },
        );
        Ok(())
    }

    async fn answer_invite(&mut self, session: SessionId, accept: bool) -> Result<(), ErrorCode> {
        let me = self.me()?;
        let (me_id, me_online) = (me.id, me.online_id);
        let found = self
            .app
            .hub
            .answer(session, me_id, accept)
            .ok_or(ErrorCode::UnknownSession)?;
        if !accept {
            self.app.hub.send(
                found.host,
                ServerMsg::SessionEnded {
                    session,
                    reason: citrine_gb_net::EndReason::Declined,
                },
            );
            return Ok(());
        }
        let host = db(self.app.db.account(found.host).await)?.ok_or(ErrorCode::Server)?;
        self.send(ServerMsg::SessionStarted {
            session,
            with: host.online_id,
        })
        .await;
        self.app.hub.send(
            found.host,
            ServerMsg::SessionStarted {
                session,
                with: me_online,
            },
        );
        Ok(())
    }

    async fn screen(&mut self, png: Vec<u8>) {
        let refused = if !self.app.settings.get().pictures {
            Some(ErrorCode::PicturesOff)
        } else if png.len() > MAX_SCREEN_BYTES {
            Some(ErrorCode::TooLarge)
        } else if !png.starts_with(PNG_SIGNATURE) {
            Some(ErrorCode::BadMessage)
        } else if !self.screen_limit.take(1.0) {
            Some(ErrorCode::RateLimited)
        } else {
            None
        };
        if let Some(error) = refused {
            self.send(ServerMsg::ScreenRefused(error)).await;
            return;
        }
        let token = self
            .screen
            .get_or_insert_with(crate::screens::token)
            .clone();
        self.app.screens.put(&token, png);
        self.send(ServerMsg::ScreenStored {
            path: format!("/f/{token}"),
        })
        .await;
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
        let Some(me) = &self.me else { return };
        let Some(ended) = self.app.hub.leave(me.id, self.conn_id) else {
            return;
        };
        let last_seen = now();
        let _ = db(self.app.db.set_last_seen(me.id, last_seen).await);
        let _ = self.tell_friends(Presence::Offline { last_seen }).await;
        for (session, other) in ended {
            self.app.hub.send(
                other,
                ServerMsg::SessionEnded {
                    session,
                    reason: citrine_gb_net::EndReason::Disconnected,
                },
            );
        }
    }
}
