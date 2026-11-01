use citrine_gb_net::{
    Account, CHUNK_BYTES, ClientMsg, EndReason, ErrorCode, FriendState, GameKey, MAX_SCREEN_BYTES,
    NONCE_BYTES, Notice, OnlineId, PROTOCOL_VERSION, Presence, RelayId, RelayPassword, Sealed,
    ServerInfo, ServerMsg, SessionId,
};
use citrine_gb_server::{Config, ConfigError, Running, StartError, start};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const PASSWORD: &str = "lemon tree";
const ADMIN: &str = "admin pass";

fn config(dir: &Path, vars: &[(&str, &str)]) -> Config {
    let mut map: HashMap<String, String> = HashMap::from([
        ("CITRINE_BIND".into(), "127.0.0.1".into()),
        ("CITRINE_PORT".into(), "0".into()),
        (
            "CITRINE_DATA_DIR".into(),
            dir.join("data").display().to_string(),
        ),
        (
            "CITRINE_STATIC_DIR".into(),
            dir.join("static").display().to_string(),
        ),
        ("CITRINE_RELAY".into(), "on".into()),
        ("CITRINE_RELAY_PASSWORD".into(), PASSWORD.into()),
        ("CITRINE_ADMIN_PASSWORD".into(), ADMIN.into()),
    ]);
    for (k, v) in vars {
        map.insert(k.to_string(), v.to_string());
    }
    Config::from_vars(|k| map.get(k).cloned()).unwrap()
}

async fn server(dir: &Path, vars: &[(&str, &str)]) -> Running {
    start(config(dir, vars)).await.unwrap()
}

struct Client {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl Client {
    async fn connect(addr: SocketAddr) -> Self {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/relay"))
            .await
            .unwrap();
        Self { ws }
    }

    async fn send(&mut self, msg: ClientMsg) {
        self.ws
            .send(Message::Binary(msg.encode().unwrap().into()))
            .await
            .unwrap();
    }

    async fn next(&mut self) -> Option<ServerMsg> {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(5), self.ws.next())
                .await
                .expect("timed out waiting for the server");
            match frame {
                Some(Ok(Message::Binary(bytes))) => {
                    return Some(ServerMsg::decode(&bytes).unwrap());
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return None,
                Some(Ok(_)) => continue,
            }
        }
    }

    async fn recv(&mut self) -> ServerMsg {
        self.next().await.expect("the server closed the connection")
    }

    async fn expect(&mut self, what: &str, pred: impl Fn(&ServerMsg) -> bool) -> ServerMsg {
        loop {
            let msg = self.recv().await;
            if pred(&msg) {
                return msg;
            }
            assert!(
                !matches!(msg, ServerMsg::Notice(_)),
                "got {msg:?} while waiting for {what}"
            );
        }
    }

    async fn closed(&mut self) {
        while let Some(msg) = self.next().await {
            assert!(
                matches!(msg, ServerMsg::Presence { .. } | ServerMsg::Notice(_)),
                "expected the connection to close, got {msg:?}"
            );
        }
    }

    async fn quiet(&mut self) {
        self.send(ClientMsg::Ping(4242)).await;
        let msg = self.recv().await;
        assert_eq!(
            msg,
            ServerMsg::Pong(4242),
            "expected nothing before the pong"
        );
    }

    async fn hello(&mut self) -> ServerInfo {
        self.send(ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            app: "test".into(),
        })
        .await;
        match self.recv().await {
            ServerMsg::Info(info) => info,
            other => panic!("expected info, got {other:?}"),
        }
    }

    async fn sign_up(addr: SocketAddr, id: &RelayId, name: &str) -> (Self, Account) {
        let mut c = Self::connect(addr).await;
        c.hello().await;
        let keys = id.keys();
        c.send(ClientMsg::SignUp {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: keys.auth_key().clone(),
            public_key: keys.public_key(),
            display_name: name.into(),
            invite: None,
        })
        .await;
        let account = c.welcome().await;
        (c, account)
    }

    async fn sign_in(addr: SocketAddr, id: &RelayId) -> (Self, Account) {
        let mut c = Self::connect(addr).await;
        c.hello().await;
        c.send(ClientMsg::SignIn {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: id.keys().auth_key().clone(),
        })
        .await;
        let account = c.welcome().await;
        (c, account)
    }

    async fn welcome(&mut self) -> Account {
        let account = match self.recv().await {
            ServerMsg::Welcome(account) => account,
            other => panic!("expected welcome, got {other:?}"),
        };
        assert!(matches!(self.recv().await, ServerMsg::Friends(_)));
        account
    }
}

async fn friends(addr: SocketAddr) -> ((Client, Account, RelayId), (Client, Account, RelayId)) {
    let a_id = RelayId::random();
    let b_id = RelayId::random();
    let (mut a, a_acc) = Client::sign_up(addr, &a_id, "Ana").await;
    let (mut b, b_acc) = Client::sign_up(addr, &b_id, "Ben").await;
    a.send(ClientMsg::AddFriend(b_acc.online_id)).await;
    b.expect(
        "request",
        |m| matches!(m, ServerMsg::Friend(f) if f.state == FriendState::Incoming),
    )
    .await;
    b.send(ClientMsg::AnswerFriend {
        from: a_acc.online_id,
        accept: true,
    })
    .await;
    a.expect(
        "accept",
        |m| matches!(m, ServerMsg::Friend(f) if f.state == FriendState::Friend),
    )
    .await;
    b.expect(
        "accept",
        |m| matches!(m, ServerMsg::Friend(f) if f.state == FriendState::Friend),
    )
    .await;
    ((a, a_acc, a_id), (b, b_acc, b_id))
}

struct Admin {
    http: reqwest::Client,
    base: String,
    cookie: String,
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

impl Admin {
    async fn login(addr: SocketAddr) -> Self {
        let http = http();
        let base = format!("http://{addr}");
        let r = http
            .post(format!("{base}/admin/login"))
            .form(&[("password", ADMIN)])
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 303);
        let cookie = r.headers()["set-cookie"].to_str().unwrap();
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        let cookie = cookie.split(';').next().unwrap().to_string();
        Self { http, base, cookie }
    }

    async fn post(&self, path: &str, form: &[(&str, &str)]) -> reqwest::Response {
        let r = self
            .http
            .post(format!("{}{path}", self.base))
            .header("cookie", &self.cookie)
            .form(form)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 303, "{path}");
        r
    }

    async fn get(&self, path: &str) -> String {
        let r = self
            .http
            .get(format!("{}{path}", self.base))
            .header("cookie", &self.cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "{path}");
        r.text().await.unwrap()
    }
}

#[tokio::test]
async fn wrong_relay_password_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let keys = RelayId::random().keys();
    for password in [None, Some(RelayPassword("lemon Tree".into()))] {
        let mut c = Client::connect(s.addr).await;
        assert!(c.hello().await.password_required);
        c.send(ClientMsg::SignUp {
            relay_password: password,
            auth_key: keys.auth_key().clone(),
            public_key: keys.public_key(),
            display_name: "Eve".into(),
            invite: None,
        })
        .await;
        assert_eq!(c.recv().await, ServerMsg::Error(ErrorCode::WrongPassword));
        c.send(ClientMsg::ListFiles).await;
        assert_eq!(c.recv().await, ServerMsg::Error(ErrorCode::NotSignedIn));
    }
    s.stop().await;
}

#[tokio::test]
async fn sign_up_then_sign_in_with_the_same_relay_id() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let id = RelayId::random();
    let (c, first) = Client::sign_up(s.addr, &id, "  Clara  ").await;
    assert_eq!(first.display_name, "Clara");
    drop(c);
    let pasted: RelayId = id.to_string().to_uppercase().parse().unwrap();
    let (mut c, again) = Client::sign_in(s.addr, &pasted).await;
    assert_eq!(again.online_id, first.online_id);
    assert_eq!(again.display_name, "Clara");
    c.send(ClientMsg::SetDisplayName("Zitrone 🍋".into())).await;
    assert!(matches!(c.recv().await, ServerMsg::Account(a) if a.display_name == "Zitrone 🍋"));
    c.send(ClientMsg::SetDisplayName("\u{7}".into())).await;
    assert_eq!(c.recv().await, ServerMsg::Error(ErrorCode::BadName));

    let mut dup = Client::connect(s.addr).await;
    dup.hello().await;
    dup.send(ClientMsg::SignUp {
        relay_password: Some(RelayPassword(PASSWORD.into())),
        auth_key: id.keys().auth_key().clone(),
        public_key: id.keys().public_key(),
        display_name: "Again".into(),
        invite: None,
    })
    .await;
    assert_eq!(dup.recv().await, ServerMsg::Error(ErrorCode::AccountExists));

    let mut unknown = Client::connect(s.addr).await;
    unknown.hello().await;
    unknown
        .send(ClientMsg::SignIn {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: RelayId::random().keys().auth_key().clone(),
        })
        .await;
    assert_eq!(
        unknown.recv().await,
        ServerMsg::Error(ErrorCode::UnknownAccount)
    );

    let mut elsewhere = Client::connect(s.addr).await;
    elsewhere.hello().await;
    elsewhere
        .send(ClientMsg::SignIn {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: id.keys().auth_key().clone(),
        })
        .await;
    elsewhere.welcome().await;
    c.expect("replaced", |m| {
        *m == ServerMsg::Notice(Notice::SignedInElsewhere)
    })
    .await;
    c.closed().await;
    s.stop().await;
}

#[tokio::test]
async fn sign_in_needs_hello_first() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let mut c = Client::connect(s.addr).await;
    c.send(ClientMsg::SignIn {
        relay_password: Some(RelayPassword(PASSWORD.into())),
        auth_key: RelayId::random().keys().auth_key().clone(),
    })
    .await;
    assert_eq!(
        c.recv().await,
        ServerMsg::Error(ErrorCode::Protocol {
            server: PROTOCOL_VERSION
        })
    );
    s.stop().await;
}

#[tokio::test]
async fn friend_request_accept_and_presence() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let a_id = RelayId::random();
    let (mut a, a_acc) = Client::sign_up(s.addr, &a_id, "Ana").await;
    let (mut b, b_acc) = Client::sign_up(s.addr, &RelayId::random(), "Ben").await;

    a.send(ClientMsg::AddFriend(b_acc.online_id)).await;
    let ServerMsg::Friend(out) = a.recv().await else {
        panic!("expected the outgoing request")
    };
    assert_eq!(out.state, FriendState::Outgoing);
    assert_eq!(out.presence, None);
    let ServerMsg::Friend(incoming) = b.recv().await else {
        panic!("expected the incoming request")
    };
    assert_eq!(incoming.state, FriendState::Incoming);
    assert_eq!(incoming.online_id, a_acc.online_id);
    assert_eq!(incoming.display_name, "Ana");
    assert_eq!(incoming.public_key, a_id.keys().public_key());

    b.send(ClientMsg::AnswerFriend {
        from: a_acc.online_id,
        accept: true,
    })
    .await;
    let ServerMsg::Friend(seen_by_b) = b.recv().await else {
        panic!("expected the friend")
    };
    assert_eq!(seen_by_b.state, FriendState::Friend);
    assert_eq!(seen_by_b.presence, Some(Presence::Online { away: false }));
    let ServerMsg::Friend(seen_by_a) = a.recv().await else {
        panic!("expected the friend")
    };
    assert_eq!(seen_by_a.state, FriendState::Friend);

    b.send(ClientMsg::SetStatus { away: true }).await;
    assert_eq!(
        a.recv().await,
        ServerMsg::Presence {
            who: b_acc.online_id,
            presence: Presence::Online { away: true }
        }
    );
    drop(b);
    let ServerMsg::Presence { who, presence } = a.recv().await else {
        panic!("expected presence")
    };
    assert_eq!(who, b_acc.online_id);
    assert!(matches!(presence, Presence::Offline { last_seen } if last_seen > 0));

    drop(a);
    let (_a, _) = Client::sign_in(s.addr, &a_id).await;
    s.stop().await;
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[tokio::test]
async fn what_a_friend_plays_reaches_only_them_sealed() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let ((mut a, a_acc, a_id), (mut b, b_acc, b_id)) = friends(s.addr).await;
    let (mut c, c_acc) = Client::sign_up(s.addr, &RelayId::random(), "Cem").await;
    let game = [
        0xC3, 0x7E, 0x19, 0x55, 0x01, 0xA2, 0x44, 0x9B, 0x10, 0x6D, 0xE8, 0x3F, 0x72, 0x0B, 0xD4,
        0x88, 0x5C, 0x21, 0xF0, 0x96,
    ];

    b.send(ClientMsg::SetStatus { away: false }).await;
    let presence = a
        .expect("presence", |m| matches!(m, ServerMsg::Presence { .. }))
        .await;
    assert!(!contains(&presence.encode().unwrap(), &game));

    let sealed = b_id
        .keys()
        .seal_playing(&a_id.keys().public_key(), Some(game));
    b.send(ClientMsg::Playing {
        to: a_acc.online_id,
        sealed: sealed.clone(),
    })
    .await;
    let relayed = a
        .expect("playing", |m| matches!(m, ServerMsg::Playing { .. }))
        .await;
    assert!(!contains(&relayed.encode().unwrap(), &game));
    let ServerMsg::Playing { from, sealed: got } = relayed else {
        unreachable!()
    };
    assert_eq!(from, b_acc.online_id);
    assert_eq!(
        a_id.keys().open_playing(&b_id.keys().public_key(), &got),
        Ok(Some(game))
    );

    b.send(ClientMsg::Playing {
        to: c_acc.online_id,
        sealed,
    })
    .await;
    b.quiet().await;
    c.quiet().await;

    s.stop().await;
    let mut stored = Vec::new();
    for entry in std::fs::read_dir(dir.path().join("data"))
        .unwrap()
        .flatten()
    {
        if entry.path().is_file() {
            stored.extend(std::fs::read(entry.path()).unwrap());
        }
    }
    assert!(!stored.is_empty());
    assert!(!contains(&stored, &game));
}

#[tokio::test]
async fn friend_list_on_sign_in_and_decline_and_remove() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let ((mut a, a_acc, a_id), (mut b, b_acc, _)) = friends(s.addr).await;
    let (mut c, c_acc) = Client::sign_up(s.addr, &RelayId::random(), "Cat").await;
    c.send(ClientMsg::AddFriend(a_acc.online_id)).await;
    a.expect(
        "request",
        |m| matches!(m, ServerMsg::Friend(f) if f.online_id == c_acc.online_id),
    )
    .await;
    drop(a);
    b.expect("offline", |m| matches!(m, ServerMsg::Presence { .. }))
        .await;

    let mut again = Client::connect(s.addr).await;
    again.hello().await;
    again
        .send(ClientMsg::SignIn {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: a_id.keys().auth_key().clone(),
        })
        .await;
    assert!(matches!(again.recv().await, ServerMsg::Welcome(_)));
    let ServerMsg::Friends(list) = again.recv().await else {
        panic!("expected the friend list")
    };
    let state = |id: OnlineId| list.iter().find(|f| f.online_id == id).map(|f| f.state);
    assert_eq!(list.len(), 2);
    assert_eq!(state(b_acc.online_id), Some(FriendState::Friend));
    assert_eq!(state(c_acc.online_id), Some(FriendState::Incoming));

    again
        .send(ClientMsg::AnswerFriend {
            from: c_acc.online_id,
            accept: false,
        })
        .await;
    assert_eq!(again.recv().await, ServerMsg::FriendGone(c_acc.online_id));
    c.expect("declined", |m| *m == ServerMsg::FriendGone(a_acc.online_id))
        .await;

    again.send(ClientMsg::RemoveFriend(b_acc.online_id)).await;
    assert_eq!(again.recv().await, ServerMsg::FriendGone(b_acc.online_id));
    b.expect("removed", |m| *m == ServerMsg::FriendGone(a_acc.online_id))
        .await;

    again
        .send(ClientMsg::AddFriend(OnlineId::new(1).unwrap()))
        .await;
    assert_eq!(again.recv().await, ServerMsg::Error(ErrorCode::UnknownUser));
    again.send(ClientMsg::AddFriend(a_acc.online_id)).await;
    assert_eq!(again.recv().await, ServerMsg::Error(ErrorCode::UnknownUser));
    s.stop().await;
}

#[tokio::test]
async fn an_offline_message_arrives_on_next_connect() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let ((mut a, a_acc, a_id), (b, b_acc, b_id)) = friends(s.addr).await;
    drop(b);
    a.expect("offline", |m| {
        matches!(
            m,
            ServerMsg::Presence {
                presence: Presence::Offline { .. },
                ..
            }
        )
    })
    .await;

    let sealed = a_id.keys().seal(
        &b_id.keys().public_key(),
        "see you at the trade 🍋".as_bytes(),
    );
    a.send(ClientMsg::Chat {
        tag: 7,
        to: b_acc.online_id,
        sealed: sealed.clone(),
    })
    .await;
    let ServerMsg::ChatSent { tag, id, .. } = a.recv().await else {
        panic!("expected chat sent")
    };
    assert_eq!(tag, 7);

    let mut b = Client::connect(s.addr).await;
    b.hello().await;
    b.send(ClientMsg::SignIn {
        relay_password: Some(RelayPassword(PASSWORD.into())),
        auth_key: b_id.keys().auth_key().clone(),
    })
    .await;
    b.welcome().await;
    let ServerMsg::Chat(msg) = b.recv().await else {
        panic!("expected the stored message")
    };
    assert_eq!(msg.id, id);
    assert_eq!(msg.from, a_acc.online_id);
    assert_eq!(msg.sealed, sealed);
    let plain = b_id
        .keys()
        .open(&a_id.keys().public_key(), &msg.sealed)
        .unwrap();
    assert_eq!(plain, "see you at the trade 🍋".as_bytes());

    b.send(ClientMsg::ChatAck(vec![id])).await;
    b.quiet().await;
    drop(b);
    let (mut b, _) = Client::sign_in(s.addr, &b_id).await;
    b.quiet().await;

    let live = a_id.keys().seal(&b_id.keys().public_key(), b"live");
    a.send(ClientMsg::Chat {
        tag: 8,
        to: b_acc.online_id,
        sealed: live.clone(),
    })
    .await;
    let ServerMsg::Chat(msg) = b
        .expect("live chat", |m| matches!(m, ServerMsg::Chat(_)))
        .await
    else {
        unreachable!()
    };
    assert_eq!(msg.sealed, live);
    s.stop().await;
}

#[tokio::test]
async fn chat_needs_friends_and_respects_the_size_cap() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let ((mut a, _, _), (_b, b_acc, _)) = friends(s.addr).await;
    let (_c, c_acc) = Client::sign_up(s.addr, &RelayId::random(), "Cat").await;
    let small = Sealed {
        nonce: [1; 24],
        ciphertext: vec![0; 32],
    };
    a.send(ClientMsg::Chat {
        tag: 1,
        to: c_acc.online_id,
        sealed: small,
    })
    .await;
    assert_eq!(
        a.recv().await,
        ServerMsg::ChatFailed {
            tag: 1,
            error: ErrorCode::NotFriends
        }
    );
    let big = Sealed {
        nonce: [1; 24],
        ciphertext: vec![0; 64 * 1024 + 1],
    };
    a.send(ClientMsg::Chat {
        tag: 2,
        to: b_acc.online_id,
        sealed: big,
    })
    .await;
    assert_eq!(
        a.recv().await,
        ServerMsg::ChatFailed {
            tag: 2,
            error: ErrorCode::TooLarge
        }
    );
    s.stop().await;
}

async fn upload(c: &mut Client, tag: u32, data: &[u8]) -> ServerMsg {
    c.send(ClientMsg::Upload {
        tag,
        size: data.len() as u64,
    })
    .await;
    match c.recv().await {
        ServerMsg::UploadReady(t) => assert_eq!(t, tag),
        other => return other,
    }
    for chunk in data.chunks(CHUNK_BYTES) {
        c.send(ClientMsg::UploadChunk {
            tag,
            data: chunk.to_vec(),
        })
        .await;
    }
    c.recv().await
}

#[tokio::test]
async fn quota_refuses_an_upload_over_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_STORAGE_MB", "1")]).await;
    let ((mut a, _, _), (mut b, _, _)) = friends(s.addr).await;

    assert_eq!(
        upload(&mut a, 1, &vec![1; 1024 * 1024 + 1]).await,
        ServerMsg::UploadFailed {
            tag: 1,
            error: ErrorCode::QuotaFull
        }
    );
    let data: Vec<u8> = (0..600 * 1024).map(|i| (i % 251) as u8).collect();
    let ServerMsg::Uploaded { tag, file } = upload(&mut a, 2, &data).await else {
        panic!("expected the upload to finish")
    };
    assert_eq!(tag, 2);
    assert_eq!(file.size, data.len() as u64);
    assert!(
        matches!(a.recv().await, ServerMsg::Account(acc) if acc.storage_used == data.len() as u64)
    );
    assert_eq!(
        upload(&mut a, 3, &data).await,
        ServerMsg::UploadFailed {
            tag: 3,
            error: ErrorCode::QuotaFull
        }
    );

    b.send(ClientMsg::Download(file.id)).await;
    assert_eq!(
        b.recv().await,
        ServerMsg::FileStart {
            file: file.id,
            size: data.len() as u64
        }
    );
    let mut got = Vec::new();
    while got.len() < data.len() {
        let ServerMsg::FileChunk { data: chunk, .. } = b.recv().await else {
            panic!("expected a chunk")
        };
        got.extend(chunk);
    }
    assert_eq!(got, data);

    let (mut stranger, _) = Client::sign_up(s.addr, &RelayId::random(), "Sam").await;
    stranger.send(ClientMsg::Download(file.id)).await;
    assert_eq!(
        stranger.recv().await,
        ServerMsg::DownloadFailed {
            file: file.id,
            error: ErrorCode::UnknownFile
        }
    );

    a.send(ClientMsg::DeleteFile(file.id)).await;
    assert_eq!(a.recv().await, ServerMsg::FileDeleted(file.id));
    assert!(matches!(a.recv().await, ServerMsg::Account(acc) if acc.storage_used == 0));
    a.send(ClientMsg::ListFiles).await;
    assert_eq!(a.recv().await, ServerMsg::Files(vec![]));
    s.stop().await;
}

#[tokio::test]
async fn no_storage_means_no_file_sharing() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_STORAGE_MB", "0")]).await;
    let (mut a, _) = Client::sign_up(s.addr, &RelayId::random(), "Ana").await;
    assert_eq!(
        upload(&mut a, 1, b"tiny").await,
        ServerMsg::UploadFailed {
            tag: 1,
            error: ErrorCode::SharingOff
        }
    );
    s.stop().await;
}

fn put(tag: u32, slot: &str, base: u64, data: &[u8]) -> ClientMsg {
    ClientMsg::PutSave {
        tag,
        game: GameKey([4; 32]),
        slot: slot.into(),
        base,
        label: b"sealed label".to_vec(),
        data: data.to_vec(),
    }
}

#[tokio::test]
async fn saves_keep_a_version_and_count_toward_storage() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_STORAGE_MB", "1")]).await;
    let id = RelayId::random();
    let (mut a, _) = Client::sign_up(s.addr, &id, "Ana").await;

    a.send(put(1, "battery", 0, &[7; 1000])).await;
    let ServerMsg::SaveStored { tag: 1, info } = a.recv().await else {
        panic!("expected the save to be stored")
    };
    assert_eq!((info.version, info.size), (1, 1000));
    assert!(matches!(a.recv().await, ServerMsg::Account(acc) if acc.storage_used == 1000));

    a.send(put(2, "battery", 0, &[8; 500])).await;
    let ServerMsg::SaveRefused {
        tag: 2,
        error: ErrorCode::SaveChanged,
        current: Some(current),
    } = a.recv().await
    else {
        panic!("expected a stale base to be refused")
    };
    assert_eq!(current.version, 1);

    a.send(put(3, "battery", 1, &[9; 600])).await;
    assert!(matches!(a.recv().await, ServerMsg::SaveStored { tag: 3, info } if info.version == 2));
    assert!(matches!(a.recv().await, ServerMsg::Account(acc) if acc.storage_used == 600));

    a.send(put(4, "slot-0", 0, &vec![1; 1_048_000])).await;
    assert!(matches!(
        a.recv().await,
        ServerMsg::SaveRefused {
            tag: 4,
            error: ErrorCode::QuotaFull,
            current: None
        }
    ));
    a.send(put(5, "../escape", 0, b"x")).await;
    assert!(matches!(
        a.recv().await,
        ServerMsg::SaveRefused {
            error: ErrorCode::BadMessage,
            ..
        }
    ));

    a.send(ClientMsg::ListFiles).await;
    assert_eq!(a.recv().await, ServerMsg::Files(vec![]));
    drop(a);

    let (mut b, account) = Client::sign_in(s.addr, &id).await;
    assert_eq!(account.storage_used, 600);
    b.send(ClientMsg::ListSaves).await;
    let ServerMsg::Saves(saves) = b.recv().await else {
        panic!("expected the list")
    };
    assert_eq!(saves.len(), 1);
    assert_eq!(saves[0].label, b"sealed label");
    b.send(ClientMsg::GetSave {
        game: GameKey([4; 32]),
        slot: "battery".into(),
    })
    .await;
    let ServerMsg::Save { info, data } = b.recv().await else {
        panic!("expected the save")
    };
    assert_eq!((info.version, data), (2, vec![9; 600]));
    b.send(ClientMsg::GetSave {
        game: GameKey([4; 32]),
        slot: "slot-9".into(),
    })
    .await;
    assert!(matches!(b.recv().await, ServerMsg::SaveMissing { .. }));

    b.send(ClientMsg::DeleteSave {
        tag: 6,
        game: GameKey([4; 32]),
        slot: "battery".into(),
        base: 1,
    })
    .await;
    assert!(matches!(
        b.recv().await,
        ServerMsg::SaveRefused {
            tag: 6,
            error: ErrorCode::SaveChanged,
            ..
        }
    ));
    b.send(ClientMsg::DeleteSave {
        tag: 7,
        game: GameKey([4; 32]),
        slot: "battery".into(),
        base: 2,
    })
    .await;
    assert_eq!(b.recv().await, ServerMsg::SaveDeleted { tag: 7 });
    assert!(matches!(b.recv().await, ServerMsg::Account(acc) if acc.storage_used == 0));
    let files = std::fs::read_dir(dir.path().join("data").join("files"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(files, 0);
    s.stop().await;
}

fn sealed(byte: u8) -> Sealed {
    Sealed {
        nonce: [byte; NONCE_BYTES],
        ciphertext: vec![byte; 40],
    }
}

async fn start_session(
    a: &mut Client,
    b: &mut Client,
    a_acc: &Account,
    b_acc: &Account,
) -> SessionId {
    a.send(ClientMsg::Invite {
        to: b_acc.online_id,
        sealed: sealed(1),
    })
    .await;
    let ServerMsg::Invited { session, to } = a.recv().await else {
        panic!("expected invited")
    };
    assert_eq!(to, b_acc.online_id);
    assert_eq!(
        b.recv().await,
        ServerMsg::Invite {
            session,
            from: a_acc.online_id,
            sealed: sealed(1),
        }
    );
    a.send(ClientMsg::Play {
        session,
        sealed: sealed(2),
    })
    .await;
    assert_eq!(a.recv().await, ServerMsg::Error(ErrorCode::UnknownSession));
    b.send(ClientMsg::AnswerInvite {
        session,
        accept: true,
    })
    .await;
    assert_eq!(
        b.recv().await,
        ServerMsg::SessionStarted {
            session,
            with: a_acc.online_id
        }
    );
    assert_eq!(
        a.recv().await,
        ServerMsg::SessionStarted {
            session,
            with: b_acc.online_id
        }
    );
    session
}

#[tokio::test]
async fn sessions_forward_sealed_play_messages() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let ((mut a, a_acc, _), (mut b, b_acc, _)) = friends(s.addr).await;
    let session = start_session(&mut a, &mut b, &a_acc, &b_acc).await;

    for byte in [3u8, 4, 5] {
        a.send(ClientMsg::Play {
            session,
            sealed: sealed(byte),
        })
        .await;
        assert_eq!(
            b.recv().await,
            ServerMsg::Play {
                session,
                sealed: sealed(byte)
            }
        );
        b.send(ClientMsg::Play {
            session,
            sealed: sealed(!byte),
        })
        .await;
        assert_eq!(
            a.recv().await,
            ServerMsg::Play {
                session,
                sealed: sealed(!byte)
            }
        );
    }

    let (mut c, _) = Client::sign_up(s.addr, &RelayId::random(), "Cat").await;
    c.send(ClientMsg::Play {
        session,
        sealed: sealed(9),
    })
    .await;
    assert_eq!(c.recv().await, ServerMsg::Error(ErrorCode::UnknownSession));

    drop(a);
    b.expect("session end", |m| {
        *m == ServerMsg::SessionEnded {
            session,
            reason: EndReason::Disconnected,
        }
    })
    .await;
    s.stop().await;
}

#[tokio::test]
async fn game_files_cross_only_when_the_relay_shares_games() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let ((mut a, a_acc, _), (mut b, b_acc, _)) = friends(s.addr).await;
    assert!(!a.hello().await.game_sharing);
    let session = start_session(&mut a, &mut b, &a_acc, &b_acc).await;
    a.send(ClientMsg::Game {
        session,
        sealed: sealed(7),
    })
    .await;
    assert_eq!(a.recv().await, ServerMsg::Error(ErrorCode::GameSharingOff));
    s.stop().await;

    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_GAME_SHARING", "true")]).await;
    let ((mut a, a_acc, _), (mut b, b_acc, _)) = friends(s.addr).await;
    assert!(a.hello().await.game_sharing);
    let session = start_session(&mut a, &mut b, &a_acc, &b_acc).await;
    let part = Sealed {
        nonce: [7; NONCE_BYTES],
        ciphertext: vec![0x5A; 192 * 1024 + 64],
    };
    a.send(ClientMsg::Game {
        session,
        sealed: part.clone(),
    })
    .await;
    assert_eq!(
        b.recv().await,
        ServerMsg::Game {
            session,
            sealed: part
        }
    );
    s.stop().await;
}

#[tokio::test]
async fn invites_need_an_online_friend_and_can_be_declined() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let ((mut a, _, _), (mut b, b_acc, _)) = friends(s.addr).await;
    a.send(ClientMsg::Invite {
        to: b_acc.online_id,
        sealed: sealed(1),
    })
    .await;
    let ServerMsg::Invited { session, .. } = a.recv().await else {
        panic!("expected invited")
    };
    b.recv().await;
    b.send(ClientMsg::AnswerInvite {
        session,
        accept: false,
    })
    .await;
    assert_eq!(
        a.recv().await,
        ServerMsg::SessionEnded {
            session,
            reason: EndReason::Declined
        }
    );
    drop(b);
    a.expect("offline", |m| matches!(m, ServerMsg::Presence { .. }))
        .await;
    a.send(ClientMsg::Invite {
        to: b_acc.online_id,
        sealed: sealed(1),
    })
    .await;
    assert_eq!(a.recv().await, ServerMsg::Error(ErrorCode::Offline));
    s.stop().await;
}

#[tokio::test]
async fn a_flood_is_rate_limited() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let mut c = Client::connect(s.addr).await;
    for i in 0..1500 {
        c.send(ClientMsg::Ping(i)).await;
    }
    let mut pongs = 0;
    let mut limited = 0;
    loop {
        match c.recv().await {
            ServerMsg::Pong(1_000_000) => break,
            ServerMsg::Pong(_) => pongs += 1,
            ServerMsg::Error(ErrorCode::RateLimited) => {
                limited += 1;
                tokio::time::sleep(Duration::from_millis(1100)).await;
                c.send(ClientMsg::Ping(1_000_000)).await;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(limited >= 1);
    assert!(pongs < 1500, "{pongs}");
    s.stop().await;
}

#[tokio::test]
async fn kill_switch_disconnects_and_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let id = RelayId::random();
    let (mut c, _) = Client::sign_up(s.addr, &id, "Ana").await;
    let mut anonymous = Client::connect(s.addr).await;
    let admin = Admin::login(s.addr).await;

    admin.post("/admin/kill", &[("on", "on")]).await;
    assert_eq!(c.recv().await, ServerMsg::Notice(Notice::KillSwitch));
    c.closed().await;
    assert_eq!(
        anonymous.recv().await,
        ServerMsg::Notice(Notice::KillSwitch)
    );
    anonymous.closed().await;

    let mut refused = Client::connect(s.addr).await;
    assert_eq!(refused.recv().await, ServerMsg::Notice(Notice::KillSwitch));
    refused.closed().await;
    assert!(admin.get("/admin").await.contains("Kill switch is on"));

    admin.post("/admin/kill", &[("on", "off")]).await;
    let (_c, _) = Client::sign_in(s.addr, &id).await;
    s.stop().await;
}

#[tokio::test]
async fn relay_off_in_the_panel_disconnects_with_a_notice() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let (mut c, _) = Client::sign_up(s.addr, &RelayId::random(), "Ana").await;
    let admin = Admin::login(s.addr).await;
    admin.post("/admin/relay", &[("on", "off")]).await;
    assert_eq!(c.recv().await, ServerMsg::Notice(Notice::RelayOff));
    c.closed().await;
    let mut refused = Client::connect(s.addr).await;
    assert_eq!(refused.recv().await, ServerMsg::Notice(Notice::RelayOff));
    admin.post("/admin/relay", &[("on", "on")]).await;
    let mut back = Client::connect(s.addr).await;
    back.hello().await;
    s.stop().await;
}

#[tokio::test]
async fn ban_disconnects_and_blocks_sign_in() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let id = RelayId::random();
    let (mut c, _) = Client::sign_up(s.addr, &id, "Spam <b>bot</b>").await;
    let admin = Admin::login(s.addr).await;
    let users = admin.get("/admin/users").await;
    assert!(users.contains("Spam &lt;b&gt;bot&lt;/b&gt;"));
    assert!(!users.contains("<b>bot</b>"));
    admin
        .post("/admin/users/1/ban", &[("back", "/admin/users")])
        .await;
    assert_eq!(c.recv().await, ServerMsg::Notice(Notice::Banned));
    c.closed().await;
    let mut again = Client::connect(s.addr).await;
    again.hello().await;
    again
        .send(ClientMsg::SignIn {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: id.keys().auth_key().clone(),
        })
        .await;
    assert_eq!(again.recv().await, ServerMsg::Error(ErrorCode::Banned));
    s.stop().await;
}

#[tokio::test]
async fn password_change_can_sign_everyone_out() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let id = RelayId::random();
    let (mut c, _) = Client::sign_up(s.addr, &id, "Ana").await;
    let admin = Admin::login(s.addr).await;
    admin
        .post(
            "/admin/password",
            &[
                ("password", "new one"),
                ("repeat", "new one"),
                ("sign_out", "yes"),
            ],
        )
        .await;
    assert_eq!(c.recv().await, ServerMsg::Notice(Notice::PasswordChanged));
    c.closed().await;
    let mut old = Client::connect(s.addr).await;
    old.hello().await;
    old.send(ClientMsg::SignIn {
        relay_password: Some(RelayPassword(PASSWORD.into())),
        auth_key: id.keys().auth_key().clone(),
    })
    .await;
    assert_eq!(old.recv().await, ServerMsg::Error(ErrorCode::WrongPassword));
    old.send(ClientMsg::SignIn {
        relay_password: Some(RelayPassword("new one".into())),
        auth_key: id.keys().auth_key().clone(),
    })
    .await;
    old.welcome().await;
    s.stop().await;
}

#[tokio::test]
async fn relay_off_with_web_on_still_serves_static_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("static")).unwrap();
    std::fs::write(dir.path().join("static/index.html"), "<h1>citrine web</h1>").unwrap();
    let s = server(
        dir.path(),
        &[("CITRINE_RELAY", "false"), ("CITRINE_WEB", "true")],
    )
    .await;
    let r = http()
        .get(format!("http://{}/", s.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.text().await.unwrap().contains("citrine web"));
    let mut c = Client::connect(s.addr).await;
    assert_eq!(c.recv().await, ServerMsg::Notice(Notice::RelayOff));
    c.closed().await;
    s.stop().await;
}

#[tokio::test]
async fn web_off_returns_404_for_the_root() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("static")).unwrap();
    std::fs::write(dir.path().join("static/index.html"), "<h1>citrine web</h1>").unwrap();
    let s = server(dir.path(), &[("CITRINE_WEB", "false")]).await;
    let r = http()
        .get(format!("http://{}/", s.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
    assert!(!r.text().await.unwrap().contains("citrine web"));
    let mut c = Client::connect(s.addr).await;
    assert!(c.hello().await.password_required);
    s.stop().await;

    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[("CITRINE_WEB", "false"), ("CITRINE_RELAY", "false")],
    )
    .await;
    let r = http()
        .get(format!("http://{}/", s.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
    let text = r.text().await.unwrap();
    assert!(text.contains("CITRINE_WEB=true"));
    assert!(text.contains("/admin"));
    let r = http()
        .get(format!("http://{}/admin", s.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    s.stop().await;
}

#[tokio::test]
async fn no_admin_password_means_no_admin_panel() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[("CITRINE_ADMIN_PASSWORD", ""), ("CITRINE_WEB", "false")],
    )
    .await;
    let r = http()
        .get(format!("http://{}/admin", s.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
    s.stop().await;
}

#[tokio::test]
async fn admin_login_wrong_then_right() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let base = format!("http://{}", s.addr);
    let page = http().get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(page.status(), 200);
    assert!(page.text().await.unwrap().contains("Admin password"));
    let r = http()
        .get(format!("{base}/admin/users"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 303);
    let r = http()
        .get(format!("{base}/admin/stats"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    let wrong = http()
        .post(format!("{base}/admin/login"))
        .form(&[("password", "admin Pass")])
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);
    assert!(wrong.headers().get("set-cookie").is_none());
    assert!(wrong.text().await.unwrap().contains("Wrong password"));

    let forged = http()
        .get(format!("{base}/admin/users"))
        .header("cookie", "citrine_admin=00")
        .send()
        .await
        .unwrap();
    assert_eq!(forged.status(), 303);

    let admin = Admin::login(s.addr).await;
    let dash = admin.get("/admin").await;
    assert!(dash.contains("Online now"));
    assert!(!dash.contains(ADMIN));
    assert!(!dash.contains(PASSWORD));
    assert!(admin.get("/admin/stats").await.contains("\"online\""));
    admin.get("/admin/users").await;
    let settings = admin.get("/admin/settings").await;
    assert!(!settings.contains(PASSWORD));

    let r = admin
        .http
        .post(format!("{base}/admin/logout"))
        .header("cookie", &admin.cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 303);
    let r = admin
        .http
        .get(format!("{base}/admin/users"))
        .header("cookie", &admin.cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 303);
    s.stop().await;
}

#[tokio::test]
async fn admin_logins_are_rate_limited() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let base = format!("http://{}", s.addr);
    for _ in 0..5 {
        let r = http()
            .post(format!("{base}/admin/login"))
            .form(&[("password", "guess")])
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
    }
    let r = http()
        .post(format!("{base}/admin/login"))
        .form(&[("password", ADMIN)])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 429);
    s.stop().await;
}

#[tokio::test]
async fn behind_a_proxy_one_visitors_guesses_do_not_lock_out_another() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_TRUSTED_PROXY_HOPS", "1")]).await;
    let base = format!("http://{}", s.addr);
    let login = |from: &'static str, password: &'static str| {
        http()
            .post(format!("{base}/admin/login"))
            .header("x-forwarded-for", from)
            .form(&[("password", password)])
            .send()
    };
    for _ in 0..5 {
        assert_eq!(login("203.0.113.7", "guess").await.unwrap().status(), 401);
    }
    assert_eq!(login("203.0.113.7", ADMIN).await.unwrap().status(), 429);
    assert_eq!(login("203.0.113.8", ADMIN).await.unwrap().status(), 303);
    s.stop().await;
}

#[tokio::test]
async fn without_a_proxy_a_forwarded_header_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let base = format!("http://{}", s.addr);
    for n in 0..5 {
        let r = http()
            .post(format!("{base}/admin/login"))
            .header("x-forwarded-for", format!("203.0.113.{n}"))
            .form(&[("password", "guess")])
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
    }
    let r = http()
        .post(format!("{base}/admin/login"))
        .header("x-forwarded-for", "203.0.113.99")
        .form(&[("password", ADMIN)])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 429);
    s.stop().await;
}

#[tokio::test]
async fn the_database_is_copied_once_a_day_and_old_copies_go() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let first_day_of_2052 = 29_950 * 86_400;
    for day in 0..16 {
        s.back_up(first_day_of_2052 + day * 86_400 + 3_600).await;
    }
    s.back_up(first_day_of_2052 + 15 * 86_400 + 7_200).await;
    let backups = dir.path().join("data").join("backups");
    let mut names: Vec<String> = std::fs::read_dir(&backups)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("citrine-2052"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 14);
    assert_eq!(names.first().unwrap(), "citrine-2052-01-03.db");
    assert_eq!(names.last().unwrap(), "citrine-2052-01-16.db");
    let copy = std::fs::read(backups.join("citrine-2052-01-16.db")).unwrap();
    assert!(copy.starts_with(b"SQLite format 3\0"));
    s.stop().await;
}

#[tokio::test]
async fn settings_changed_in_the_panel_persist_across_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let mut c = Client::connect(s.addr).await;
    let before = c.hello().await;
    assert_eq!(before.quota_bytes, 100 << 20);
    assert!(before.sign_ups_open);
    drop(c);
    let admin = Admin::login(s.addr).await;
    admin
        .post(
            "/admin/settings",
            &[
                ("name", "Lemon Grove"),
                ("quota_mb", "7"),
                ("max_message_kb", "16"),
                ("max_file_mb", "3"),
            ],
        )
        .await;
    s.stop().await;

    let s = server(dir.path(), &[]).await;
    let mut c = Client::connect(s.addr).await;
    let after = c.hello().await;
    assert_eq!(after.name, "Lemon Grove");
    assert_eq!(after.quota_bytes, 7 << 20);
    assert_eq!(after.max_message_bytes, 16 << 10);
    assert_eq!(after.max_file_bytes, 3 << 20);
    assert!(!after.sign_ups_open);
    let keys = RelayId::random().keys();
    c.send(ClientMsg::SignUp {
        relay_password: Some(RelayPassword(PASSWORD.into())),
        auth_key: keys.auth_key().clone(),
        public_key: keys.public_key(),
        display_name: "Late".into(),
        invite: None,
    })
    .await;
    assert_eq!(c.recv().await, ServerMsg::Error(ErrorCode::SignUpsClosed));

    let admin = Admin::login(s.addr).await;
    let page = admin.get("/admin/settings").await;
    assert!(page.contains("Set here"));
    admin
        .post("/admin/settings/reset", &[("key", "default_quota_mb")])
        .await;
    let mut c = Client::connect(s.addr).await;
    assert_eq!(c.hello().await.quota_bytes, 100 << 20);
    s.stop().await;
}

#[tokio::test]
async fn an_open_relay_must_be_chosen() {
    let dir = tempfile::tempdir().unwrap();
    let refused = start(config(dir.path(), &[("CITRINE_RELAY_PASSWORD", "")])).await;
    assert!(matches!(
        refused,
        Err(StartError::Config(ConfigError::OpenRelay))
    ));
    let s = server(
        dir.path(),
        &[
            ("CITRINE_RELAY_PASSWORD", ""),
            ("CITRINE_RELAY_OPEN", "true"),
        ],
    )
    .await;
    let mut c = Client::connect(s.addr).await;
    assert!(!c.hello().await.password_required);
    let keys = RelayId::random().keys();
    c.send(ClientMsg::SignUp {
        relay_password: None,
        auth_key: keys.auth_key().clone(),
        public_key: keys.public_key(),
        display_name: "Open".into(),
        invite: None,
    })
    .await;
    c.welcome().await;
    s.stop().await;
}

#[tokio::test]
async fn garbage_frames_get_an_error_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let mut c = Client::connect(s.addr).await;
    c.ws.send(Message::Binary(vec![0xFF, 0x00, 0x13].into()))
        .await
        .unwrap();
    assert_eq!(c.recv().await, ServerMsg::Error(ErrorCode::BadMessage));
    c.hello().await;
    s.stop().await;
}

fn png(fill: u8, len: usize) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.resize(len, fill);
    png
}

#[tokio::test]
async fn a_screen_is_public_until_its_connection_ends() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let base = format!("http://{}", s.addr);
    let http = http();

    let mut stranger = Client::connect(s.addr).await;
    stranger.hello().await;
    stranger.send(ClientMsg::Screen(png(1, 100))).await;
    assert_eq!(
        stranger.recv().await,
        ServerMsg::Error(ErrorCode::NotSignedIn)
    );

    let (mut c, _) = Client::sign_up(s.addr, &RelayId::random(), "Ana").await;
    c.send(ClientMsg::Screen(png(1, 100))).await;
    let path = match c.recv().await {
        ServerMsg::ScreenStored { path } => path,
        other => panic!("expected the screen to be stored, got {other:?}"),
    };
    assert!(path.starts_with("/f/"));
    assert_eq!(path.len(), 35);
    let r = http.get(format!("{base}{path}")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "image/png");
    assert_eq!(r.headers()["cache-control"], "max-age=60");
    assert_eq!(r.bytes().await.unwrap().as_ref(), png(1, 100).as_slice());

    c.send(ClientMsg::Screen(png(2, MAX_SCREEN_BYTES + 1)))
        .await;
    assert_eq!(
        c.recv().await,
        ServerMsg::ScreenRefused(ErrorCode::TooLarge)
    );
    c.send(ClientMsg::Screen(vec![2; 100])).await;
    assert_eq!(
        c.recv().await,
        ServerMsg::ScreenRefused(ErrorCode::BadMessage)
    );
    c.send(ClientMsg::Screen(png(2, 100))).await;
    assert_eq!(
        c.recv().await,
        ServerMsg::ScreenRefused(ErrorCode::RateLimited)
    );
    let r = http.get(format!("{base}{path}")).send().await.unwrap();
    assert_eq!(r.bytes().await.unwrap().as_ref(), png(1, 100).as_slice());

    let unknown = http
        .get(format!("{base}/f/{}", "0".repeat(32)))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 404);

    drop(c);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let r = http.get(format!("{base}{path}")).send().await.unwrap();
        if r.status() == 404 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the screen outlived its connection"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    s.stop().await;
}

#[tokio::test]
async fn discord_pictures_turned_off_are_refused_and_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let base = format!("http://{}", s.addr);
    let http = http();
    let mut probe = Client::connect(s.addr).await;
    assert!(probe.hello().await.pictures);
    drop(probe);

    let (mut c, _) = Client::sign_up(s.addr, &RelayId::random(), "Ana").await;
    c.send(ClientMsg::Screen(png(1, 100))).await;
    let path = match c.recv().await {
        ServerMsg::ScreenStored { path } => path,
        other => panic!("expected the screen to be stored, got {other:?}"),
    };

    let admin = Admin::login(s.addr).await;
    let page = admin.get("/admin/settings").await;
    assert!(page.contains(r#"<label for="pictures">Discord screenshots</label>"#));
    assert!(page.contains(r#"name="pictures" type="checkbox" class="switch" value="on" checked>"#));
    let form = |pictures: bool| {
        let mut fields = vec![
            ("name", "Lemon"),
            ("sign_ups", "on"),
            ("quota_mb", "100"),
            ("max_message_kb", "64"),
            ("max_file_mb", "25"),
        ];
        if pictures {
            fields.push(("pictures", "on"));
        }
        fields
    };
    admin.post("/admin/settings", &form(false)).await;
    let page = admin.get("/admin/settings").await;
    assert!(page.contains(r#"name="pictures" type="checkbox" class="switch" value="on">"#));

    c.send(ClientMsg::Screen(png(2, 100))).await;
    assert_eq!(
        c.recv().await,
        ServerMsg::ScreenRefused(ErrorCode::PicturesOff)
    );
    let r = http.get(format!("{base}{path}")).send().await.unwrap();
    assert_eq!(r.status(), 404);
    let mut late = Client::connect(s.addr).await;
    assert!(!late.hello().await.pictures);
    drop(late);

    admin.post("/admin/settings", &form(true)).await;
    let r = http.get(format!("{base}{path}")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let mut late = Client::connect(s.addr).await;
    assert!(late.hello().await.pictures);
    s.stop().await;
}

fn totp(key: &str) -> totp_rs::Totp {
    totp_rs::Builder::new()
        .with_secret(totp_rs::Secret::try_from_base32(key).unwrap())
        .build_noncompliant()
}

fn unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let from = text.find(start).unwrap() + start.len();
    let len = text[from..].find(end).unwrap();
    &text[from..from + len]
}

async fn sign_in_with(addr: SocketAddr, password: &str, code: Option<&str>) -> reqwest::Response {
    let mut form = vec![("password", password)];
    if let Some(code) = code {
        form.push(("code", code));
    }
    http()
        .post(format!("http://{addr}/admin/login"))
        .form(&form)
        .send()
        .await
        .unwrap()
}

async fn turn_on_two_factor(admin: &Admin) -> (totp_rs::Totp, String, Vec<String>) {
    let off = admin.get("/admin/settings").await;
    assert!(off.contains("Two-factor sign-in"));
    assert!(off.contains("Set up"));
    let setup = admin.get("/admin/settings?setup=two-factor").await;
    assert!(setup.contains("<svg"));
    let key = between(&setup, r#"id="tf-key">"#, "<").replace(' ', "");
    assert_eq!(
        key,
        between(
            &admin.get("/admin/settings?setup=two-factor").await,
            r#"id="tf-key">"#,
            "<"
        )
        .replace(' ', "")
    );
    let totp = totp(&key);
    let wrong = admin
        .post("/admin/two-factor/on", &[("code", "12345")])
        .await;
    assert!(
        wrong.headers()["location"]
            .to_str()
            .unwrap()
            .contains("bad-code")
    );
    let code = totp.generate(unix()).to_string();
    let r = admin
        .http
        .post(format!("{}/admin/two-factor/on", admin.base))
        .header("cookie", &admin.cookie)
        .form(&[("code", code.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["cache-control"], "no-store");
    let page = r.text().await.unwrap();
    let codes: Vec<String> = between(&page, r#"<ol class="codes mono">"#, "</ol>")
        .split("<li>")
        .filter_map(|li| li.strip_suffix("</li>"))
        .map(str::to_string)
        .collect();
    assert_eq!(codes.len(), 10);
    assert!(page.contains("download="));
    let on = admin.get("/admin/settings").await;
    assert!(on.contains("10 recovery codes left"));
    assert!(!on.contains(&key));
    (totp, code, codes)
}

#[tokio::test]
async fn two_factor_setup_sign_in_and_turn_off() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let admin = Admin::login(s.addr).await;
    let (totp, used, codes) = turn_on_two_factor(&admin).await;

    let page = http()
        .get(format!("http://{}/admin", s.addr))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("Code from your app"));

    let r = sign_in_with(s.addr, ADMIN, None).await;
    assert_eq!(r.status(), 401);
    assert!(r.text().await.unwrap().contains("Wrong password or code"));
    let r = sign_in_with(s.addr, ADMIN, Some(&used)).await;
    assert_eq!(r.status(), 401);
    let next = totp.generate(unix() + 30).to_string();
    let r = sign_in_with(s.addr, "admin Pass", Some(&next)).await;
    assert_eq!(r.status(), 401);
    let r = sign_in_with(s.addr, ADMIN, Some(&next)).await;
    assert_eq!(r.status(), 303);
    assert!(r.headers().get("set-cookie").is_some());

    let r = sign_in_with(s.addr, ADMIN, Some(&codes[0])).await;
    assert_eq!(r.status(), 303);
    let r = sign_in_with(s.addr, ADMIN, Some(&codes[0])).await;
    assert_eq!(r.status(), 401);
    assert!(
        admin
            .get("/admin/settings")
            .await
            .contains("9 recovery codes left")
    );

    let wrong = admin
        .post("/admin/two-factor/off", &[("code", "000000-0")])
        .await;
    assert!(
        wrong.headers()["location"]
            .to_str()
            .unwrap()
            .contains("bad-code")
    );
    let off = admin
        .post("/admin/two-factor/off", &[("code", codes[1].as_str())])
        .await;
    assert!(
        off.headers()["location"]
            .to_str()
            .unwrap()
            .contains("2fa-off")
    );
    assert!(admin.get("/admin/settings").await.contains("Set up"));
    let r = sign_in_with(s.addr, ADMIN, None).await;
    assert_eq!(r.status(), 303);
    s.stop().await;
}

#[tokio::test]
async fn new_recovery_codes_replace_the_old_ones() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let admin = Admin::login(s.addr).await;
    let (_, _, old) = turn_on_two_factor(&admin).await;
    let r = admin
        .http
        .post(format!("{}/admin/two-factor/codes", admin.base))
        .header("cookie", &admin.cookie)
        .form(&[("code", old[2].as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let page = r.text().await.unwrap();
    let new = between(&page, r#"<ol class="codes mono">"#, "</ol>")
        .split("<li>")
        .filter_map(|li| li.strip_suffix("</li>"))
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(new.len(), 10);
    assert_eq!(
        sign_in_with(s.addr, ADMIN, Some(&old[3])).await.status(),
        401
    );
    assert_eq!(
        sign_in_with(s.addr, ADMIN, Some(&new[3])).await.status(),
        303
    );
    s.stop().await;
}

#[tokio::test]
async fn wrong_codes_count_toward_the_lockout() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let admin = Admin::login(s.addr).await;
    let (totp, _, codes) = turn_on_two_factor(&admin).await;
    let valid = totp.generate(unix() + 30).to_string();
    let wrong = format!(
        "{:06}",
        (valid.parse::<u32>().unwrap() + 500_000) % 1_000_000
    );
    for _ in 0..5 {
        let r = sign_in_with(s.addr, ADMIN, Some(&wrong)).await;
        assert_eq!(r.status(), 401);
    }
    assert_eq!(
        sign_in_with(s.addr, ADMIN, Some(&valid)).await.status(),
        429
    );
    assert_eq!(
        sign_in_with(s.addr, ADMIN, Some(&codes[0])).await.status(),
        429
    );
    s.stop().await;
}

#[tokio::test]
async fn the_reset_variable_turns_two_factor_off_at_start() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let admin = Admin::login(s.addr).await;
    turn_on_two_factor(&admin).await;
    s.stop().await;

    let s = server(dir.path(), &[]).await;
    assert_eq!(sign_in_with(s.addr, ADMIN, None).await.status(), 401);
    s.stop().await;

    let s = server(dir.path(), &[("CITRINE_ADMIN_2FA_RESET", "true")]).await;
    assert_eq!(sign_in_with(s.addr, ADMIN, None).await.status(), 303);
    let admin = Admin::login(s.addr).await;
    let settings = admin.get("/admin/settings").await;
    assert!(settings.contains("Set up"));
    assert!(settings.contains("CITRINE_ADMIN_2FA_RESET is still set"));
    s.stop().await;
}

#[tokio::test]
async fn a_relay_password_set_in_the_panel_is_stored_as_argon2id() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let admin = Admin::login(s.addr).await;
    admin
        .post(
            "/admin/password",
            &[("password", "new lemon"), ("repeat", "new lemon")],
        )
        .await;
    s.stop().await;
    let mut stored = std::fs::read(dir.path().join("data/citrine.db")).unwrap();
    stored.extend(std::fs::read(dir.path().join("data/citrine.db-wal")).unwrap_or_default());
    let stored = String::from_utf8_lossy(&stored);
    assert!(stored.contains("$argon2id$"));
    assert!(!stored.contains("new lemon"));

    let s = server(dir.path(), &[]).await;
    let mut c = Client::connect(s.addr).await;
    c.hello().await;
    let keys = RelayId::random().keys();
    c.send(ClientMsg::SignUp {
        relay_password: Some(RelayPassword("new lemon".into())),
        auth_key: keys.auth_key().clone(),
        public_key: keys.public_key(),
        display_name: "Lime".into(),
        invite: None,
    })
    .await;
    c.welcome().await;
    s.stop().await;
}

#[tokio::test]
async fn web_on_without_its_files_says_so_and_how_to_build_them() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_WEB", "true")]).await;
    let root = format!("http://{}/", s.addr);
    let r = http().get(&root).send().await.unwrap();
    assert_eq!(r.status(), 404);
    let text = r.text().await.unwrap();
    assert!(text.contains("The web app isn't built yet"), "{text}");
    assert!(text.contains("trunk build --release --dist /"), "{text}");
    assert!(text.contains("static"), "{text}");
    assert!(text.contains("data-copy=\"trunk build"), "{text}");
    let r = http()
        .get(format!("http://{}/admin/icon.png", s.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    let admin = Admin::login(s.addr).await;
    let panel = admin.get("/admin").await;
    assert!(panel.contains("has no web app in it"), "{panel}");
    assert!(panel.contains("data-copy=\"trunk build"), "{panel}");

    std::fs::create_dir_all(dir.path().join("static")).unwrap();
    std::fs::write(dir.path().join("static/index.html"), "<h1>citrine web</h1>").unwrap();
    let r = http().get(&root).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.text().await.unwrap().contains("citrine web"));
    let r = http().get(format!("{root}nope.js")).send().await.unwrap();
    assert_eq!(r.status(), 404);
    assert!(!r.text().await.unwrap().contains("isn't built"));
    let panel = admin.get("/admin").await;
    assert!(!panel.contains("has no web app in it"));
    assert!(panel.contains("Web app on"));
    s.stop().await;
    let bare = tempfile::tempdir().unwrap();
    let s = server(
        bare.path(),
        &[("CITRINE_WEB", "true"), ("CITRINE_ADMIN_PASSWORD", "")],
    )
    .await;
    let r = http()
        .get(format!("http://{}/admin/icon.png", s.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    s.stop().await;
}
