use citrine_gb_net::{
    Account, CHUNK_BYTES, Channel, ClientHandshake, ClientMsg, ErrorCode, FileId, FriendState,
    GameKey, NONCE_BYTES, PROTOCOL_VERSION, RelayId, RelayPassword, Sealed, ServerMsg,
};
use citrine_gb_server::{Config, Running, start};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tokio::net::{TcpSocket, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

fn config(dir: &Path, vars: &[(&str, &str)]) -> Config {
    let mut map: HashMap<String, String> = HashMap::from([
        ("CITRINE_BIND".into(), "127.0.0.1".into()),
        ("CITRINE_PORT".into(), "0".into()),
        ("CITRINE_DISK_FLOOR_MB".into(), "0".into()),
        ("CITRINE_DISK_FLOOR_PERCENT".into(), "0".into()),
        (
            "CITRINE_DATA_DIR".into(),
            dir.join("data").display().to_string(),
        ),
        ("CITRINE_WEB".into(), "off".into()),
        ("CITRINE_RELAY".into(), "on".into()),
        ("CITRINE_RELAY_OPEN".into(), "true".into()),
        ("CITRINE_SIGN_UPS_PER_IP".into(), "100000".into()),
        ("CITRINE_SIGN_UPS_PER_HOUR".into(), "100000".into()),
    ]);
    for (k, v) in vars {
        map.insert(k.to_string(), v.to_string());
    }
    Config::from_vars(|k| map.get(k).cloned()).unwrap()
}

const LOOSE_RATES: &[(&str, &str)] = &[
    ("CITRINE_MESSAGES_PER_SECOND", "100000"),
    ("CITRINE_MESSAGE_BURST", "100000"),
    ("CITRINE_CHATS_PER_SECOND", "100000"),
    ("CITRINE_CHAT_BURST", "100000"),
    ("CITRINE_CHAT_KB_PER_SECOND", "1000000"),
];

async fn server(dir: &Path, vars: &[(&str, &str)]) -> Running {
    start(config(dir, vars)).await.unwrap()
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Client {
    ws: Ws,
    channel: Channel,
}

impl Client {
    async fn secure(mut ws: Ws) -> Self {
        let (handshake, first) = ClientHandshake::start();
        ws.send(Message::Binary(first.into())).await.unwrap();
        let reply = loop {
            match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
                Ok(Some(Ok(Message::Binary(bytes)))) => break bytes,
                Ok(Some(Ok(_))) => continue,
                other => panic!("no handshake reply: {other:?}"),
            }
        };
        let (_, channel) = handshake.finish(&reply).unwrap();
        Self { ws, channel }
    }

    async fn send_raw(&mut self, bytes: Vec<u8>) {
        let sealed = self.channel.seal(&bytes);
        self.ws.send(Message::Binary(sealed.into())).await.unwrap();
    }

    async fn connect(addr: SocketAddr) -> Self {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/relay"))
            .await
            .unwrap();
        Self::secure(ws).await
    }

    async fn slow_reader(addr: SocketAddr) -> Self {
        let socket = TcpSocket::new_v4().unwrap();
        socket.set_recv_buffer_size(4096).unwrap();
        let stream = socket.connect(addr).await.unwrap();
        let (ws, _) = tokio_tungstenite::client_async(
            format!("ws://{addr}/relay"),
            MaybeTlsStream::Plain(stream),
        )
        .await
        .unwrap();
        Self::secure(ws).await
    }

    async fn send(&mut self, msg: ClientMsg) {
        self.ws
            .send(Message::Binary(
                self.channel.seal(&msg.encode().unwrap()).into(),
            ))
            .await
            .unwrap();
    }

    async fn try_send(&mut self, msg: ClientMsg) -> bool {
        self.ws
            .send(Message::Binary(
                self.channel.seal(&msg.encode().unwrap()).into(),
            ))
            .await
            .is_ok()
    }

    async fn next(&mut self) -> Option<ServerMsg> {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(10), self.ws.next())
                .await
                .expect("timed out waiting for the server");
            match frame {
                Some(Ok(Message::Binary(bytes))) => {
                    match ServerMsg::decode(&self.channel.open(&bytes).unwrap()).unwrap() {
                        ServerMsg::Device(_) | ServerMsg::Devices { .. } => continue,
                        msg => return Some(msg),
                    }
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

    async fn pong(&mut self) {
        self.send(ClientMsg::Ping(4242)).await;
        self.expect("pong", |m| *m == ServerMsg::Pong(4242)).await;
    }

    async fn hello(&mut self) {
        self.send(ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            app: "test".into(),
        })
        .await;
        assert!(matches!(self.recv().await, ServerMsg::Info(_)));
    }

    async fn sign_up_on(mut self, id: &RelayId, name: &str) -> (Self, Account) {
        self.hello().await;
        let keys = id.keys();
        self.send(ClientMsg::SignUp {
            relay_password: None,
            auth_key: keys.auth_key().clone(),
            public_key: keys.public_key(),
            display_name: name.into(),
            invite: None,
            device_name: "Test".into(),
        })
        .await;
        let account = self.welcome().await;
        (self, account)
    }

    async fn sign_up(addr: SocketAddr, name: &str) -> (Self, Account, RelayId) {
        let id = RelayId::random();
        let (c, account) = Self::connect(addr).await.sign_up_on(&id, name).await;
        (c, account, id)
    }

    async fn sign_in_on(mut self, id: &RelayId) -> Result<(Self, Account), ErrorCode> {
        self.hello().await;
        self.send(ClientMsg::SignIn {
            relay_password: None,
            auth_key: id.keys().auth_key().clone(),
            device: None,
            code: None,
            device_name: "Test".into(),
        })
        .await;
        match self.recv().await {
            ServerMsg::Welcome(account) => {
                assert!(matches!(self.recv().await, ServerMsg::Friends(_)));
                Ok((self, account))
            }
            ServerMsg::Error(code) => Err(code),
            other => panic!("expected welcome, got {other:?}"),
        }
    }

    async fn welcome(&mut self) -> Account {
        let account = match self.recv().await {
            ServerMsg::Welcome(account) => account,
            other => panic!("expected welcome, got {other:?}"),
        };
        assert!(matches!(self.recv().await, ServerMsg::Friends(_)));
        account
    }

    async fn upload(&mut self, tag: u32, data: &[u8]) -> ServerMsg {
        self.send(ClientMsg::Upload {
            tag,
            size: data.len() as u64,
        })
        .await;
        loop {
            match self.recv().await {
                ServerMsg::UploadReady(t) => {
                    assert_eq!(t, tag);
                    break;
                }
                ServerMsg::Account(_) => continue,
                other => return other,
            }
        }
        for chunk in data.chunks(CHUNK_BYTES) {
            self.send(ClientMsg::UploadChunk {
                tag,
                data: chunk.to_vec(),
            })
            .await;
        }
        loop {
            match self.recv().await {
                ServerMsg::Account(_) => continue,
                other => return other,
            }
        }
    }

    async fn uploaded(&mut self, tag: u32, data: &[u8]) -> FileId {
        let ServerMsg::Uploaded { file, .. } = self.upload(tag, data).await else {
            panic!("expected the upload to finish")
        };
        assert!(matches!(self.recv().await, ServerMsg::Account(_)));
        file.id
    }
}

async fn befriend(a: &mut Client, a_acc: &Account, b: &mut Client, b_acc: &Account) {
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
}

fn sealed(byte: u8, len: usize) -> Sealed {
    Sealed {
        nonce: [byte; NONCE_BYTES],
        ciphertext: vec![byte; len],
    }
}

#[tokio::test]
async fn a_client_that_stops_reading_is_closed_and_others_carry_on() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[
            ("CITRINE_SEND_TIMEOUT_SECS", "1"),
            ("CITRINE_QUEUE_MB", "1"),
        ],
    )
    .await;
    let id = RelayId::random();
    let (mut stuck, _) = Client::slow_reader(s.addr)
        .await
        .sign_up_on(&id, "Sam")
        .await;
    let file = stuck.uploaded(1, &vec![5; 6 << 20]).await;
    let (mut other, _, _) = Client::sign_up(s.addr, "Ola").await;
    for _ in 0..4 {
        stuck.send(ClientMsg::Download(file)).await;
    }
    let mut closed = false;
    for _ in 0..80 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if !stuck.try_send(ClientMsg::Ping(1)).await {
            closed = true;
            break;
        }
    }
    assert!(closed, "the server kept a connection that stopped reading");
    other.pong().await;
    s.stop().await;
}

#[tokio::test]
async fn downloads_past_the_waiting_line_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[
            ("CITRINE_QUEUE_MB", "1"),
            ("CITRINE_DOWNLOADS_AT_ONCE", "1"),
            ("CITRINE_DOWNLOADS_WAITING", "1"),
        ],
    )
    .await;
    let id = RelayId::random();
    let (mut c, _) = Client::slow_reader(s.addr)
        .await
        .sign_up_on(&id, "Dee")
        .await;
    let file = c.uploaded(1, &vec![9; 6 << 20]).await;
    for _ in 0..3 {
        c.send(ClientMsg::Download(file)).await;
    }
    let (mut starts, mut bytes, mut refused) = (0, 0, 0);
    while starts < 2 || bytes < 12 << 20 || refused < 1 {
        match c.recv().await {
            ServerMsg::FileStart { .. } => starts += 1,
            ServerMsg::FileChunk { data, .. } => bytes += data.len(),
            ServerMsg::DownloadFailed {
                error: ErrorCode::RateLimited,
                ..
            } => refused += 1,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!((starts, bytes, refused), (2, 12 << 20, 1));
    c.send(ClientMsg::Download(file)).await;
    assert!(matches!(c.recv().await, ServerMsg::FileStart { .. }));
    s.stop().await;
}

#[tokio::test]
async fn waiting_messages_arrive_in_pages_and_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), LOOSE_RATES).await;
    let (mut a, a_acc, _) = Client::sign_up(s.addr, "Ana").await;
    let (mut b, b_acc, b_id) = Client::sign_up(s.addr, "Ben").await;
    befriend(&mut a, &a_acc, &mut b, &b_acc).await;
    drop(b);
    for i in 0..450u32 {
        a.send(ClientMsg::Chat {
            tag: i,
            to: b_acc.online_id,
            sealed: sealed(1, 16),
        })
        .await;
        a.expect(
            "sent",
            |m| matches!(m, ServerMsg::ChatSent { tag, .. } if *tag == i),
        )
        .await;
    }
    let (mut b, _) = Client::connect(s.addr)
        .await
        .sign_in_on(&b_id)
        .await
        .unwrap();
    let mut last = 0;
    for _ in 0..450 {
        let ServerMsg::Chat(msg) = b.expect("chat", |m| matches!(m, ServerMsg::Chat(_))).await
        else {
            unreachable!()
        };
        assert!(msg.id > last);
        last = msg.id;
    }
    b.pong().await;
    s.stop().await;
}

fn put(tag: u32, slot: &str, data: &[u8]) -> ClientMsg {
    ClientMsg::PutSave {
        tag,
        game: GameKey([4; 32]),
        slot: slot.into(),
        base: 0,
        label: b"label".to_vec(),
        data: data.to_vec(),
    }
}

async fn put_answer(c: &mut Client) -> Result<(), ErrorCode> {
    loop {
        match c.recv().await {
            ServerMsg::SaveStored { .. } => return Ok(()),
            ServerMsg::SaveRefused { error, .. } => return Err(error),
            ServerMsg::Account(_) => continue,
            other => panic!("expected a save answer, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn an_open_relay_starts_at_20_mb_unless_storage_is_set() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let (_, account, _) = Client::sign_up(s.addr, "Ana").await;
    assert_eq!(account.quota_bytes, 20 << 20);
    s.stop().await;
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_STORAGE_MB", "100")]).await;
    let (_, account, _) = Client::sign_up(s.addr, "Ana").await;
    assert_eq!(account.quota_bytes, 100 << 20);
    s.stop().await;
}

#[tokio::test]
async fn a_tiny_file_costs_a_disk_block() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let (mut a, _, _) = Client::sign_up(s.addr, "Ana").await;
    assert!(matches!(
        a.upload(1, b"x").await,
        ServerMsg::Uploaded { .. }
    ));
    assert!(matches!(a.recv().await, ServerMsg::Account(acc) if acc.storage_used == 4096));
    s.stop().await;
}

#[tokio::test]
async fn the_total_storage_cap_holds_across_accounts() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_STORAGE_TOTAL_MB", "3")]).await;
    let (mut a, _, _) = Client::sign_up(s.addr, "Ana").await;
    let (mut b, _, _) = Client::sign_up(s.addr, "Ben").await;
    a.uploaded(1, &vec![1; 5 << 19]).await;
    assert_eq!(
        b.upload(2, &vec![2; 1 << 20]).await,
        ServerMsg::UploadFailed {
            tag: 2,
            error: ErrorCode::QuotaFull
        }
    );
    b.send(put(3, "battery", &vec![2; 1 << 19])).await;
    assert_eq!(put_answer(&mut b).await, Err(ErrorCode::QuotaFull));
    b.uploaded(4, &vec![3; 64 << 10]).await;
    s.stop().await;
}

#[tokio::test]
async fn too_little_free_disk_refuses_writes_but_not_reads() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_DISK_FLOOR_MB", "1000000000")]).await;
    let (mut a, a_acc, _) = Client::sign_up(s.addr, "Ana").await;
    let (mut b, b_acc, _) = Client::sign_up(s.addr, "Ben").await;
    befriend(&mut a, &a_acc, &mut b, &b_acc).await;
    a.send(put(1, "battery", b"x")).await;
    assert_eq!(put_answer(&mut a).await, Err(ErrorCode::QuotaFull));
    assert_eq!(
        a.upload(2, b"x").await,
        ServerMsg::UploadFailed {
            tag: 2,
            error: ErrorCode::QuotaFull
        }
    );
    a.send(ClientMsg::Chat {
        tag: 3,
        to: b_acc.online_id,
        sealed: sealed(1, 16),
    })
    .await;
    assert_eq!(
        a.recv().await,
        ServerMsg::ChatFailed {
            tag: 3,
            error: ErrorCode::QuotaFull
        }
    );
    a.send(ClientMsg::ListSaves).await;
    assert_eq!(a.recv().await, ServerMsg::Saves(vec![]));
    s.stop().await;
}

#[tokio::test]
async fn save_slots_and_files_have_a_count_cap() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[("CITRINE_MAX_SAVES", "3"), ("CITRINE_MAX_FILES", "2")],
    )
    .await;
    let (mut a, _, _) = Client::sign_up(s.addr, "Ana").await;
    for (tag, slot) in [(1, "a"), (2, "b"), (3, "c")] {
        a.send(put(tag, slot, b"x")).await;
        assert_eq!(put_answer(&mut a).await, Ok(()));
    }
    a.send(put(4, "d", b"x")).await;
    assert_eq!(put_answer(&mut a).await, Err(ErrorCode::QuotaFull));
    a.send(ClientMsg::PutSave {
        tag: 5,
        game: GameKey([4; 32]),
        slot: "a".into(),
        base: 1,
        label: b"label".to_vec(),
        data: b"y".to_vec(),
    })
    .await;
    assert_eq!(put_answer(&mut a).await, Ok(()));
    let first = a.uploaded(6, b"one").await;
    a.uploaded(7, b"two").await;
    assert_eq!(
        a.upload(8, b"three").await,
        ServerMsg::UploadFailed {
            tag: 8,
            error: ErrorCode::QuotaFull
        }
    );
    a.send(ClientMsg::DeleteFile(first)).await;
    a.expect("deleted", |m| matches!(m, ServerMsg::Account(_)))
        .await;
    a.uploaded(9, b"three").await;
    let (mut b, _, _) = Client::sign_up(s.addr, "Ben").await;
    b.uploaded(1, b"own cap").await;
    s.stop().await;
}

async fn chat_answer(c: &mut Client, tag: u32) -> Result<(), ErrorCode> {
    match c
        .expect("chat answer", |m| {
            matches!(m, ServerMsg::ChatSent { tag: t, .. } | ServerMsg::ChatFailed { tag: t, .. } if *t == tag)
        })
        .await
    {
        ServerMsg::ChatFailed { error, .. } => Err(error),
        _ => Ok(()),
    }
}

#[tokio::test]
async fn one_sender_cant_fill_an_offline_friends_inbox_for_others() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[("CITRINE_INBOX_MESSAGES", "5"), ("CITRINE_INBOX_MB", "1")],
    )
    .await;
    let (mut a, a_acc, _) = Client::sign_up(s.addr, "Ana").await;
    let (mut b, b_acc, b_id) = Client::sign_up(s.addr, "Ben").await;
    let (mut c, c_acc, _) = Client::sign_up(s.addr, "Cy").await;
    befriend(&mut a, &a_acc, &mut b, &b_acc).await;
    befriend(&mut c, &c_acc, &mut b, &b_acc).await;
    drop(b);
    for tag in 0..5 {
        a.send(ClientMsg::Chat {
            tag,
            to: b_acc.online_id,
            sealed: sealed(1, 16),
        })
        .await;
        assert_eq!(chat_answer(&mut a, tag).await, Ok(()));
    }
    a.send(ClientMsg::Chat {
        tag: 5,
        to: b_acc.online_id,
        sealed: sealed(1, 16),
    })
    .await;
    assert_eq!(chat_answer(&mut a, 5).await, Err(ErrorCode::InboxFull));
    for tag in 0..2 {
        c.send(ClientMsg::Chat {
            tag,
            to: b_acc.online_id,
            sealed: sealed(2, 60_000),
        })
        .await;
        assert_eq!(chat_answer(&mut c, tag).await, Ok(()));
    }
    let (mut b, _) = Client::connect(s.addr)
        .await
        .sign_in_on(&b_id)
        .await
        .unwrap();
    for _ in 0..7 {
        b.expect("chat", |m| matches!(m, ServerMsg::Chat(_))).await;
    }
    b.pong().await;
    s.stop().await;
}

#[tokio::test]
async fn the_inbox_also_caps_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let vars = [LOOSE_RATES, &[("CITRINE_INBOX_MB", "1")]].concat();
    let s = server(dir.path(), &vars).await;
    let (mut a, a_acc, _) = Client::sign_up(s.addr, "Ana").await;
    let (mut b, b_acc, _) = Client::sign_up(s.addr, "Ben").await;
    befriend(&mut a, &a_acc, &mut b, &b_acc).await;
    drop(b);
    for tag in 0..17 {
        a.send(ClientMsg::Chat {
            tag,
            to: b_acc.online_id,
            sealed: sealed(1, 60_000),
        })
        .await;
        assert_eq!(chat_answer(&mut a, tag).await, Ok(()));
    }
    a.send(ClientMsg::Chat {
        tag: 17,
        to: b_acc.online_id,
        sealed: sealed(1, 60_000),
    })
    .await;
    assert_eq!(chat_answer(&mut a, 17).await, Err(ErrorCode::InboxFull));
    s.stop().await;
}

async fn try_connect(addr: SocketAddr, forwarded: Option<&str>) -> Result<Client, u16> {
    let mut request = format!("ws://{addr}/relay").into_client_request().unwrap();
    if let Some(ip) = forwarded {
        request
            .headers_mut()
            .insert("x-forwarded-for", ip.parse().unwrap());
    }
    match tokio_tungstenite::connect_async(request).await {
        Ok((ws, _)) => Ok(Client::secure(ws).await),
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            Err(response.status().as_u16())
        }
        Err(e) => panic!("connect: {e}"),
    }
}

#[tokio::test]
async fn one_address_gets_ten_connections_and_others_still_get_in() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_TRUSTED_PROXY_HOPS", "1")]).await;
    let mut held = Vec::new();
    for i in 0..10 {
        let c = try_connect(s.addr, Some("10.0.0.1")).await.unwrap();
        let (c, _) = c.sign_up_on(&RelayId::random(), &format!("P{i}")).await;
        held.push(c);
    }
    assert_eq!(try_connect(s.addr, Some("10.0.0.1")).await.err(), Some(429));
    let other = try_connect(s.addr, Some("10.0.0.2")).await.unwrap();
    let (mut other, _) = other.sign_up_on(&RelayId::random(), "Ola").await;
    other.pong().await;
    held.pop();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(try_connect(s.addr, Some("10.0.0.1")).await.is_ok());
    s.stop().await;
}

#[tokio::test]
async fn only_three_connections_per_address_may_wait_to_sign_in() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let mut waiting = Vec::new();
    for _ in 0..3 {
        waiting.push(try_connect(s.addr, None).await.unwrap());
    }
    assert_eq!(try_connect(s.addr, None).await.err(), Some(429));
    let first = waiting.remove(0);
    let (mut signed, _) = first.sign_up_on(&RelayId::random(), "Sid").await;
    assert!(try_connect(s.addr, None).await.is_ok());
    signed.pong().await;
    s.stop().await;
}

#[tokio::test]
async fn new_connections_per_minute_are_capped_per_address() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[
            ("CITRINE_TRUSTED_PROXY_HOPS", "1"),
            ("CITRINE_IP_CONNECTS_PER_MINUTE", "5"),
        ],
    )
    .await;
    for _ in 0..5 {
        let mut c = try_connect(s.addr, Some("10.0.0.1")).await.unwrap();
        c.hello().await;
    }
    assert_eq!(try_connect(s.addr, Some("10.0.0.1")).await.err(), Some(429));
    assert!(try_connect(s.addr, Some("10.0.0.2")).await.is_ok());
    s.stop().await;
}

#[tokio::test]
async fn a_full_relay_answers_503() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[
            ("CITRINE_TRUSTED_PROXY_HOPS", "1"),
            ("CITRINE_MAX_CONNECTIONS", "2"),
        ],
    )
    .await;
    let _a = try_connect(s.addr, Some("10.0.0.1")).await.unwrap();
    let _b = try_connect(s.addr, Some("10.0.0.2")).await.unwrap();
    assert_eq!(try_connect(s.addr, Some("10.0.0.3")).await.err(), Some(503));
    s.stop().await;
}

#[tokio::test]
async fn without_a_trusted_proxy_the_forwarded_header_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let mut held = Vec::new();
    for i in 1..=3 {
        held.push(
            try_connect(s.addr, Some(&format!("10.0.0.{i}")))
                .await
                .unwrap(),
        );
    }
    assert_eq!(try_connect(s.addr, Some("10.0.0.4")).await.err(), Some(429));
    drop(held);
    s.stop().await;
}

#[tokio::test]
async fn a_big_frame_before_sign_in_closes_the_socket() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let mut c = Client::connect(s.addr).await;
    c.send_raw(vec![0; 8 << 10]).await;
    assert_eq!(c.next().await, None);
    let (mut signed, _, _) = Client::sign_up(s.addr, "Sue").await;
    signed.send_raw(vec![0; 8 << 10]).await;
    assert_eq!(signed.recv().await, ServerMsg::Error(ErrorCode::BadMessage));
    signed.pong().await;
    s.stop().await;
}

fn sign_in_with(id: &RelayId, password: Option<&str>) -> ClientMsg {
    ClientMsg::SignIn {
        relay_password: password.map(|p| RelayPassword(p.into())),
        auth_key: id.keys().auth_key().clone(),
        device: None,
        code: None,
        device_name: "Test".into(),
    }
}

fn sign_up_with(id: &RelayId, name: &str, password: Option<&str>) -> ClientMsg {
    let keys = id.keys();
    ClientMsg::SignUp {
        relay_password: password.map(|p| RelayPassword(p.into())),
        auth_key: keys.auth_key().clone(),
        public_key: keys.public_key(),
        display_name: name.into(),
        invite: None,
        device_name: "Test".into(),
    }
}

async fn entry(c: &mut Client) -> Result<(), ErrorCode> {
    match c.recv().await {
        ServerMsg::Welcome(_) => {
            assert!(matches!(c.recv().await, ServerMsg::Friends(_)));
            Ok(())
        }
        ServerMsg::Error(code) => Err(code),
        other => panic!("expected welcome or an error, got {other:?}"),
    }
}

async fn signed_in(addr: SocketAddr, msg: ClientMsg) -> Result<Client, ErrorCode> {
    let mut c = Client::connect(addr).await;
    c.hello().await;
    c.send(msg).await;
    entry(&mut c).await.map(|()| c)
}

#[tokio::test]
async fn random_keys_from_an_address_dont_lock_out_its_accounts() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let ana = RelayId::random();
    drop(
        signed_in(s.addr, sign_up_with(&ana, "Ana", None))
            .await
            .unwrap(),
    );
    let mut bot = Client::connect(s.addr).await;
    bot.hello().await;
    let mut limited = 0;
    for _ in 0..80 {
        bot.send(sign_in_with(&RelayId::random(), None)).await;
        match entry(&mut bot).await {
            Err(ErrorCode::UnknownAccount) => assert_eq!(limited, 0),
            Err(ErrorCode::TooManyTries) => limited += 1,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert_eq!(limited, 20);
    let mut c = signed_in(s.addr, sign_in_with(&ana, None)).await.unwrap();
    c.pong().await;
    assert_eq!(
        signed_in(s.addr, sign_up_with(&RelayId::random(), "Cy", None))
            .await
            .err(),
        Some(ErrorCode::TooManyTries)
    );
    s.stop().await;
}

#[tokio::test]
async fn wrong_passwords_count_per_account_and_per_address() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[
            ("CITRINE_RELAY_PASSWORD", "pw"),
            ("CITRINE_RELAY_OPEN", "false"),
        ],
    )
    .await;
    let ana = RelayId::random();
    let ben = RelayId::random();
    drop(
        signed_in(s.addr, sign_up_with(&ana, "Ana", Some("pw")))
            .await
            .unwrap(),
    );
    drop(
        signed_in(s.addr, sign_up_with(&ben, "Ben", Some("pw")))
            .await
            .unwrap(),
    );
    let mut bot = Client::connect(s.addr).await;
    bot.hello().await;
    for _ in 0..10 {
        bot.send(sign_in_with(&ana, Some("guess"))).await;
        assert_eq!(entry(&mut bot).await, Err(ErrorCode::WrongPassword));
    }
    bot.send(sign_in_with(&ana, Some("pw"))).await;
    assert_eq!(entry(&mut bot).await, Err(ErrorCode::TooManyTries));
    drop(
        signed_in(s.addr, sign_in_with(&ben, Some("pw")))
            .await
            .unwrap(),
    );
    for _ in 0..10 {
        bot.send(sign_up_with(&RelayId::random(), "Guess", Some("guess")))
            .await;
        assert_eq!(entry(&mut bot).await, Err(ErrorCode::WrongPassword));
    }
    bot.send(sign_up_with(&RelayId::random(), "Cy", Some("pw")))
        .await;
    assert_eq!(entry(&mut bot).await, Err(ErrorCode::TooManyTries));
    let mut b = signed_in(s.addr, sign_in_with(&ben, Some("pw")))
        .await
        .unwrap();
    b.pong().await;
    s.stop().await;
}

async fn invite(host: &mut Client, guest: &mut Client, to: &Account) -> u64 {
    host.send(ClientMsg::Invite {
        to: to.online_id,
        sealed: sealed(1, 16),
    })
    .await;
    let ServerMsg::Invited { session, .. } = host
        .expect("invited", |m| matches!(m, ServerMsg::Invited { .. }))
        .await
    else {
        unreachable!()
    };
    guest
        .expect(
            "invite",
            |m| matches!(m, ServerMsg::Invite { session: s, .. } if *s == session),
        )
        .await;
    session
}

#[tokio::test]
async fn a_burst_of_messages_is_refused_but_other_accounts_carry_on() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[
            ("CITRINE_MESSAGE_BURST", "20"),
            ("CITRINE_MESSAGES_PER_SECOND", "1"),
        ],
    )
    .await;
    let (mut a, _, _) = Client::sign_up(s.addr, "Ana").await;
    let (mut b, _, _) = Client::sign_up(s.addr, "Ben").await;
    for _ in 0..40 {
        a.send(ClientMsg::ListFiles).await;
    }
    a.send(ClientMsg::Ping(7)).await;
    let (mut files, mut limited) = (0, 0);
    loop {
        match a.recv().await {
            ServerMsg::Files(_) => files += 1,
            ServerMsg::Error(ErrorCode::RateLimited) => limited += 1,
            ServerMsg::Pong(7) => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!((20..=23).contains(&files), "{files} answered");
    assert_eq!(limited, 1);
    b.send(ClientMsg::ListFiles).await;
    assert!(matches!(b.recv().await, ServerMsg::Files(_)));
    s.stop().await;
}

#[tokio::test]
async fn a_chat_burst_is_refused_with_chat_failed() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[
            ("CITRINE_CHAT_BURST", "5"),
            ("CITRINE_CHATS_PER_SECOND", "1"),
        ],
    )
    .await;
    let (mut a, a_acc, _) = Client::sign_up(s.addr, "Ana").await;
    let (mut b, b_acc, _) = Client::sign_up(s.addr, "Ben").await;
    befriend(&mut a, &a_acc, &mut b, &b_acc).await;
    for tag in 0..8 {
        a.send(ClientMsg::Chat {
            tag,
            to: b_acc.online_id,
            sealed: sealed(1, 16),
        })
        .await;
    }
    let mut sent = 0;
    for tag in 0..8 {
        match chat_answer(&mut a, tag).await {
            Ok(()) => sent += 1,
            Err(error) => assert_eq!(error, ErrorCode::RateLimited),
        }
    }
    assert!((5..=6).contains(&sent), "{sent} sent");
    b.pong().await;
    s.stop().await;
}

#[tokio::test]
async fn chat_bytes_have_their_own_rate() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_CHAT_KB_PER_SECOND", "16")]).await;
    let (mut a, a_acc, _) = Client::sign_up(s.addr, "Ana").await;
    let (mut b, b_acc, _) = Client::sign_up(s.addr, "Ben").await;
    befriend(&mut a, &a_acc, &mut b, &b_acc).await;
    for tag in 0..3 {
        a.send(ClientMsg::Chat {
            tag,
            to: b_acc.online_id,
            sealed: sealed(1, 30_000),
        })
        .await;
    }
    assert_eq!(chat_answer(&mut a, 0).await, Ok(()));
    assert_eq!(chat_answer(&mut a, 1).await, Ok(()));
    assert_eq!(chat_answer(&mut a, 2).await, Err(ErrorCode::RateLimited));
    s.stop().await;
}

#[tokio::test]
async fn friend_requests_have_a_rate_and_repeats_are_free() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_FRIEND_REQUEST_BURST", "2")]).await;
    let (mut a, _, _) = Client::sign_up(s.addr, "Ana").await;
    let mut targets = Vec::new();
    for name in ["Ben", "Cy", "Dee"] {
        let (c, acc, _) = Client::sign_up(s.addr, name).await;
        targets.push((c, acc));
    }
    for to in [0, 0, 1] {
        a.send(ClientMsg::AddFriend(targets[to].1.online_id)).await;
        assert!(matches!(
            a.recv().await,
            ServerMsg::Friend(f) if f.state == FriendState::Outgoing
        ));
    }
    a.send(ClientMsg::AddFriend(targets[2].1.online_id)).await;
    assert_eq!(a.recv().await, ServerMsg::Error(ErrorCode::RateLimited));
    s.stop().await;
}

#[tokio::test]
async fn downloads_have_a_daily_budget() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_DOWNLOAD_MB_PER_DAY", "1")]).await;
    let (mut a, _, _) = Client::sign_up(s.addr, "Ana").await;
    let (mut b, _, _) = Client::sign_up(s.addr, "Ben").await;
    let file = a.uploaded(1, &vec![1; 600 << 10]).await;
    a.send(ClientMsg::Download(file)).await;
    a.send(ClientMsg::Download(file)).await;
    a.expect("refusal", |m| {
        matches!(m, ServerMsg::DownloadFailed { file: f, error: ErrorCode::RateLimited } if *f == file)
    })
    .await;
    let small = b.uploaded(1, b"tiny").await;
    b.send(ClientMsg::Download(small)).await;
    b.expect("start", |m| matches!(m, ServerMsg::FileStart { .. }))
        .await;
    s.stop().await;
}

#[tokio::test]
async fn a_session_has_a_bandwidth_budget_per_sender() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[
            ("CITRINE_SESSION_BURST_MB", "1"),
            ("CITRINE_SESSION_KB_PER_SECOND", "1"),
        ],
    )
    .await;
    let (mut a, a_acc, _) = Client::sign_up(s.addr, "Ana").await;
    let (mut b, b_acc, _) = Client::sign_up(s.addr, "Ben").await;
    befriend(&mut a, &a_acc, &mut b, &b_acc).await;
    let session = invite(&mut a, &mut b, &b_acc).await;
    b.send(ClientMsg::AnswerInvite {
        session,
        accept: true,
    })
    .await;
    a.expect("start", |m| matches!(m, ServerMsg::SessionStarted { .. }))
        .await;
    b.expect("start", |m| matches!(m, ServerMsg::SessionStarted { .. }))
        .await;
    for _ in 0..5 {
        a.send(ClientMsg::Play {
            session,
            sealed: sealed(2, 300_000),
        })
        .await;
    }
    a.expect("limit", |m| *m == ServerMsg::Error(ErrorCode::RateLimited))
        .await;
    a.pong().await;
    b.send(ClientMsg::Ping(9)).await;
    let mut plays = 0;
    loop {
        match b.recv().await {
            ServerMsg::Play { .. } => plays += 1,
            ServerMsg::Pong(9) => break,
            _ => {}
        }
    }
    assert_eq!(plays, 3);
    b.send(ClientMsg::Play {
        session,
        sealed: sealed(3, 16),
    })
    .await;
    a.expect("play", |m| matches!(m, ServerMsg::Play { .. }))
        .await;
    s.stop().await;
}

#[tokio::test]
async fn a_host_has_a_few_open_invites_and_a_new_one_replaces_the_old() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_MAX_INVITES", "2")]).await;
    let (mut h, h_acc, _) = Client::sign_up(s.addr, "Host").await;
    let mut guests = Vec::new();
    for name in ["Ben", "Cy", "Dee"] {
        let (mut c, acc, _) = Client::sign_up(s.addr, name).await;
        befriend(&mut h, &h_acc, &mut c, &acc).await;
        guests.push((c, acc));
    }
    let (g, g_acc) = &mut guests[0];
    let first = invite(&mut h, g, g_acc).await;
    h.send(ClientMsg::Invite {
        to: g_acc.online_id,
        sealed: sealed(1, 16),
    })
    .await;
    let ServerMsg::Invited {
        session: second, ..
    } = h
        .expect("invited", |m| matches!(m, ServerMsg::Invited { .. }))
        .await
    else {
        unreachable!()
    };
    assert_eq!(
        g.recv().await,
        ServerMsg::SessionEnded {
            session: first,
            reason: citrine_gb_net::EndReason::Left,
        }
    );
    assert!(matches!(g.recv().await, ServerMsg::Invite { session, .. } if session == second));
    let (g, g_acc) = &mut guests[1];
    invite(&mut h, g, g_acc).await;
    h.send(ClientMsg::Invite {
        to: guests[2].1.online_id,
        sealed: sealed(1, 16),
    })
    .await;
    h.expect("refusal", |m| {
        *m == ServerMsg::Error(ErrorCode::RateLimited)
    })
    .await;
    s.stop().await;
}

fn screen(width: u32, height: u32) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend_from_slice(&width.to_be_bytes());
    png.extend_from_slice(&height.to_be_bytes());
    png.resize(200, 0);
    png
}

#[tokio::test]
async fn a_screen_must_have_the_game_boy_shape() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[]).await;
    let (mut a, _, _) = Client::sign_up(s.addr, "Ana").await;
    a.send(ClientMsg::Screen(screen(1000, 1000))).await;
    assert_eq!(
        a.recv().await,
        ServerMsg::ScreenRefused(ErrorCode::BadMessage)
    );
    a.send(ClientMsg::Screen(screen(480, 432))).await;
    assert!(matches!(a.recv().await, ServerMsg::ScreenStored { .. }));
    s.stop().await;
}

#[tokio::test]
async fn one_address_makes_three_accounts_an_hour() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[
            ("CITRINE_SIGN_UPS_PER_IP", "3"),
            ("CITRINE_TRUSTED_PROXY_HOPS", "1"),
        ],
    )
    .await;
    for i in 0..3 {
        let c = try_connect(s.addr, Some("10.0.0.1")).await.unwrap();
        c.sign_up_on(&RelayId::random(), &format!("P{i}")).await;
    }
    let mut c = try_connect(s.addr, Some("10.0.0.1")).await.unwrap();
    c.hello().await;
    c.send(sign_up_with(&RelayId::random(), "P3", None)).await;
    assert_eq!(entry(&mut c).await, Err(ErrorCode::TooManyTries));
    let mut d = try_connect(s.addr, Some("10.0.0.2")).await.unwrap();
    d.hello().await;
    for _ in 0..3 {
        d.send(sign_up_with(&RelayId::random(), "", None)).await;
        assert_eq!(entry(&mut d).await, Err(ErrorCode::BadName));
    }
    d.send(sign_up_with(&RelayId::random(), "Dee", None)).await;
    assert_eq!(entry(&mut d).await, Ok(()));
    s.stop().await;
}

#[tokio::test]
async fn the_whole_relay_makes_a_set_number_of_accounts_an_hour() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(
        dir.path(),
        &[
            ("CITRINE_SIGN_UPS_PER_HOUR", "2"),
            ("CITRINE_TRUSTED_PROXY_HOPS", "1"),
        ],
    )
    .await;
    let mut held = Vec::new();
    for i in 1..=2 {
        let c = try_connect(s.addr, Some(&format!("10.0.1.{i}")))
            .await
            .unwrap();
        held.push(c.sign_up_on(&RelayId::random(), &format!("P{i}")).await.0);
    }
    let mut c = try_connect(s.addr, Some("10.0.1.3")).await.unwrap();
    c.hello().await;
    c.send(sign_up_with(&RelayId::random(), "P3", None)).await;
    assert_eq!(entry(&mut c).await, Err(ErrorCode::SignUpsClosed));
    s.stop().await;
}

#[tokio::test]
async fn a_burst_of_friend_requests_leaves_the_target_connected_with_one_notice() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), LOOSE_RATES).await;
    let (mut a, _, _) = Client::sign_up(s.addr, "Ana").await;
    let (mut t, t_acc, _) = Client::sign_up(s.addr, "Tia").await;
    for _ in 0..801 {
        a.send(ClientMsg::AddFriend(t_acc.online_id)).await;
    }
    a.pong().await;
    t.send(ClientMsg::Ping(5)).await;
    let mut notices = 0;
    loop {
        match t.recv().await {
            ServerMsg::Friend(f) if f.state == FriendState::Incoming => notices += 1,
            ServerMsg::Pong(5) => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(notices, 1);
    s.stop().await;
}

async fn friends_on_sign_in(addr: SocketAddr, id: &RelayId) -> Vec<citrine_gb_net::Friend> {
    let mut c = Client::connect(addr).await;
    c.hello().await;
    c.send(sign_in_with(id, None)).await;
    assert!(matches!(c.recv().await, ServerMsg::Welcome(_)));
    let ServerMsg::Friends(friends) = c.recv().await else {
        panic!("expected the friend list")
    };
    friends
}

#[tokio::test]
async fn a_full_incoming_list_drops_new_requests_quietly() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_FRIEND_REQUESTS_IN", "2")]).await;
    let (mut t, t_acc, t_id) = Client::sign_up(s.addr, "Tia").await;
    for name in ["Ana", "Ben", "Cy"] {
        let (mut c, _, _) = Client::sign_up(s.addr, name).await;
        c.send(ClientMsg::AddFriend(t_acc.online_id)).await;
        assert!(matches!(
            c.recv().await,
            ServerMsg::Friend(f) if f.state == FriendState::Outgoing
        ));
    }
    t.send(ClientMsg::Ping(5)).await;
    let mut notices = 0;
    loop {
        match t.recv().await {
            ServerMsg::Friend(f) if f.state == FriendState::Incoming => notices += 1,
            ServerMsg::Pong(5) => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(notices, 2);
    assert_eq!(friends_on_sign_in(s.addr, &t_id).await.len(), 2);
    s.stop().await;
}

#[tokio::test]
async fn one_account_has_a_cap_on_open_requests() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path(), &[("CITRINE_FRIEND_REQUESTS_OUT", "2")]).await;
    let (mut a, _, a_id) = Client::sign_up(s.addr, "Ana").await;
    let mut targets = Vec::new();
    for name in ["Ben", "Cy", "Dee"] {
        targets.push(Client::sign_up(s.addr, name).await.1);
    }
    for to in &targets[..2] {
        a.send(ClientMsg::AddFriend(to.online_id)).await;
        assert!(matches!(
            a.recv().await,
            ServerMsg::Friend(f) if f.state == FriendState::Outgoing
        ));
    }
    a.send(ClientMsg::AddFriend(targets[2].online_id)).await;
    assert_eq!(a.recv().await, ServerMsg::Error(ErrorCode::RateLimited));
    assert_eq!(friends_on_sign_in(s.addr, &a_id).await.len(), 2);
    s.stop().await;
}
