use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use citrine_gb_net::{
    Account, Channel, ClientHandshake, ClientMsg, DeviceKey, EndReason, ErrorCode, FriendState,
    NONCE_BYTES, PROTOCOL_VERSION, Presence, PushKinds, RelayId, RelayPassword, Sealed, ServerMsg,
};
use citrine_gb_server::{Config, Running, start};
use futures_util::{SinkExt, StreamExt};
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::pkcs8::{EncodePrivateKey, LineEnding};
use rsa::sha2::Sha256;
use rsa::signature::Verifier;
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const PASSWORD: &str = "lemon tree";
const PROJECT: &str = "citrine-test";
const ACCESS: &str = "fake-access-token";
const GONE: &str = "uninstalled-phone";
const CLIENT_EMAIL: &str = "push@citrine-test.iam.gserviceaccount.com";

fn key() -> &'static RsaPrivateKey {
    static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    KEY.get_or_init(|| RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap())
}

struct Sent {
    auth: String,
    body: Value,
}

struct Fake {
    token_uri: String,
    sent: mpsc::UnboundedSender<Sent>,
    assertions: Mutex<Vec<String>>,
}

async fn token(State(fake): State<Arc<Fake>>, body: String) -> (StatusCode, String) {
    let form: HashMap<String, String> = body
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    assert_eq!(
        form.get("grant_type").map(String::as_str),
        Some("urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer")
    );
    let assertion = form.get("assertion").cloned().unwrap_or_default();
    let reply = json!({ "access_token": ACCESS, "expires_in": 3600, "token_type": "Bearer" });
    fake.assertions.lock().await.push(assertion);
    (StatusCode::OK, reply.to_string())
}

async fn send(
    State(fake): State<Arc<Fake>>,
    headers: HeaderMap,
    body: String,
) -> (StatusCode, String) {
    let body: Value = serde_json::from_str(&body).unwrap();
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let gone = body["message"]["token"] == GONE;
    let _ = fake.sent.send(Sent { auth, body });
    if gone {
        let error = json!({ "error": { "code": 404, "status": "NOT_FOUND", "details": [
            { "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": "UNREGISTERED" }
        ]}});
        return (StatusCode::NOT_FOUND, error.to_string());
    }
    (
        StatusCode::OK,
        json!({ "name": "projects/x/messages/1" }).to_string(),
    )
}

struct Firebase {
    url: String,
    fake: Arc<Fake>,
    sent: mpsc::UnboundedReceiver<Sent>,
}

impl Firebase {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, sent) = mpsc::unbounded_channel();
        let fake = Arc::new(Fake {
            token_uri: format!("{url}/token"),
            sent: tx,
            assertions: Mutex::new(Vec::new()),
        });
        let router = Router::new()
            .route("/token", post(token))
            .route(&format!("/v1/projects/{PROJECT}/messages:send"), post(send))
            .with_state(fake.clone());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, fake, sent }
    }

    fn key_file(&self, dir: &Path) -> String {
        let pem = key().to_pkcs8_pem(LineEnding::LF).unwrap();
        let account = json!({
            "type": "service_account",
            "project_id": PROJECT,
            "private_key_id": "test",
            "private_key": pem.as_str(),
            "client_email": CLIENT_EMAIL,
            "token_uri": self.fake.token_uri,
        });
        let path = dir.join("service-account.json");
        std::fs::write(&path, account.to_string()).unwrap();
        path.display().to_string()
    }

    async fn next(&mut self) -> Sent {
        tokio::time::timeout(Duration::from_secs(10), self.sent.recv())
            .await
            .expect("no push reached the fake Firebase")
            .unwrap()
    }
}

fn config(dir: &Path, vars: &[(&str, String)]) -> Config {
    let mut map: HashMap<String, String> = HashMap::from([
        ("CITRINE_BIND".into(), "127.0.0.1".into()),
        ("CITRINE_PORT".into(), "0".into()),
        ("CITRINE_DISK_FLOOR_MB".into(), "0".into()),
        ("CITRINE_DISK_FLOOR_PERCENT".into(), "0".into()),
        ("CITRINE_IP_CONNECTIONS".into(), "100000".into()),
        ("CITRINE_IP_UNSIGNED".into(), "100000".into()),
        ("CITRINE_IP_CONNECTS_PER_MINUTE".into(), "100000".into()),
        ("CITRINE_SIGN_UPS_PER_IP".into(), "100000".into()),
        ("CITRINE_SIGN_UPS_PER_HOUR".into(), "100000".into()),
        ("CITRINE_MESSAGES_PER_SECOND".into(), "100000".into()),
        ("CITRINE_MESSAGE_BURST".into(), "100000".into()),
        ("CITRINE_CHATS_PER_SECOND".into(), "100000".into()),
        ("CITRINE_CHAT_BURST".into(), "100000".into()),
        ("CITRINE_CHAT_KB_PER_SECOND".into(), "100000".into()),
        ("CITRINE_FRIEND_REQUESTS_PER_HOUR".into(), "100000".into()),
        ("CITRINE_FRIEND_REQUEST_BURST".into(), "100000".into()),
        (
            "CITRINE_DATA_DIR".into(),
            dir.join("data").display().to_string(),
        ),
        (
            "CITRINE_STATIC_DIR".into(),
            dir.join("static").display().to_string(),
        ),
        ("CITRINE_WEB".into(), "off".into()),
        ("CITRINE_RELAY".into(), "on".into()),
        ("CITRINE_RELAY_PASSWORD".into(), PASSWORD.into()),
    ]);
    for (k, v) in vars {
        map.insert(k.to_string(), v.clone());
    }
    Config::from_vars(|k| map.get(k).cloned()).unwrap()
}

async fn server_with_firebase(dir: &Path) -> (Running, Firebase) {
    server_with_firebase_and(dir, &[]).await
}

async fn server_with_firebase_and(dir: &Path, more: &[(&str, String)]) -> (Running, Firebase) {
    let firebase = Firebase::start().await;
    let mut vars = vec![
        ("CITRINE_FCM_KEY_FILE", firebase.key_file(dir)),
        ("CITRINE_FCM_URL", firebase.url.clone()),
    ];
    vars.extend(more.iter().cloned());
    (start(config(dir, &vars)).await.unwrap(), firebase)
}

async fn closes_the_app(ana: &mut Client, ben: Client) {
    drop(ben);
    ana.expect("offline", |m| {
        matches!(
            m,
            ServerMsg::Presence {
                presence: Presence::Offline { .. },
                ..
            }
        )
    })
    .await;
}

async fn invite(ana: &mut Client, to: &Account) -> u64 {
    ana.send(ClientMsg::Invite {
        to: to.online_id,
        sealed: sealed(),
    })
    .await;
    let ServerMsg::Invited { session, .. } = ana
        .expect("invited", |m| matches!(m, ServerMsg::Invited { .. }))
        .await
    else {
        unreachable!()
    };
    session
}

struct Client {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    channel: Channel,
    device: Option<DeviceKey>,
    this: Option<u64>,
}

impl Client {
    async fn connect(addr: SocketAddr) -> Self {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/relay"))
            .await
            .unwrap();
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
        Self {
            ws,
            channel,
            device: None,
            this: None,
        }
    }

    async fn send(&mut self, msg: ClientMsg) {
        let sealed = self.channel.seal(&msg.encode().unwrap());
        self.ws.send(Message::Binary(sealed.into())).await.unwrap();
    }

    async fn next(&mut self) -> Option<ServerMsg> {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(5), self.ws.next())
                .await
                .expect("timed out waiting for the server");
            match frame {
                Some(Ok(Message::Binary(bytes))) => {
                    match ServerMsg::decode(&self.channel.open(&bytes).unwrap()).unwrap() {
                        ServerMsg::Device(key) => self.device = Some(key),
                        ServerMsg::Devices { this, .. } => self.this = Some(this),
                        msg => return Some(msg),
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return None,
                Some(Ok(_)) => continue,
            }
        }
    }

    async fn expect(&mut self, what: &str, pred: impl Fn(&ServerMsg) -> bool) -> ServerMsg {
        loop {
            let msg = self.next().await.expect("the server closed the connection");
            if pred(&msg) {
                return msg;
            }
            assert!(
                !matches!(msg, ServerMsg::Notice(_) | ServerMsg::Error(_)),
                "got {msg:?} while waiting for {what}"
            );
        }
    }

    async fn quiet(&mut self) {
        self.send(ClientMsg::Ping(4242)).await;
        self.expect("pong", |m| *m == ServerMsg::Pong(4242)).await;
    }

    async fn hello(&mut self) {
        self.send(ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            app: "test".into(),
        })
        .await;
        self.expect("info", |m| matches!(m, ServerMsg::Info(_)))
            .await;
    }

    async fn welcome(&mut self) -> Account {
        let ServerMsg::Welcome(account) = self
            .expect("welcome", |m| matches!(m, ServerMsg::Welcome(_)))
            .await
        else {
            unreachable!()
        };
        self.expect("friends", |m| matches!(m, ServerMsg::Friends(_)))
            .await;
        account
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
            device_name: "Pixel".into(),
        })
        .await;
        let account = c.welcome().await;
        (c, account)
    }

    async fn sign_in(addr: SocketAddr, id: &RelayId) -> Self {
        let mut c = Self::connect(addr).await;
        c.hello().await;
        c.send(ClientMsg::SignIn {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: id.keys().auth_key().clone(),
            device: None,
            code: None,
            device_name: "Tablet".into(),
        })
        .await;
        c.welcome().await;
        c
    }

    async fn push(&mut self, token: &str, kinds: PushKinds) {
        self.send(ClientMsg::SetPush {
            token: Some(token.into()),
            kinds,
        })
        .await;
        self.quiet().await;
    }

    async fn away(&mut self, away: bool) {
        self.send(ClientMsg::SetStatus { away }).await;
        self.quiet().await;
    }

    async fn chat(&mut self, to: &Account) {
        self.send(ClientMsg::Chat {
            tag: 1,
            to: to.online_id,
            sealed: sealed(),
        })
        .await;
        self.expect("sent", |m| matches!(m, ServerMsg::ChatSent { .. }))
            .await;
    }
}

fn sealed() -> Sealed {
    Sealed {
        nonce: [1; NONCE_BYTES],
        ciphertext: vec![1, 2, 3],
    }
}

async fn friends(addr: SocketAddr) -> ((Client, Account), (Client, Account, RelayId)) {
    let ben_id = RelayId::random();
    let (mut ana, ana_acc) = Client::sign_up(addr, &RelayId::random(), "Ana").await;
    let (mut ben, ben_acc) = Client::sign_up(addr, &ben_id, "Ben").await;
    ana.send(ClientMsg::AddFriend(ben_acc.online_id)).await;
    ben.expect(
        "request",
        |m| matches!(m, ServerMsg::Friend(f) if f.state == FriendState::Incoming),
    )
    .await;
    ben.send(ClientMsg::AnswerFriend {
        from: ana_acc.online_id,
        accept: true,
    })
    .await;
    for c in [&mut ana, &mut ben] {
        c.expect(
            "friends",
            |m| matches!(m, ServerMsg::Friend(f) if f.state == FriendState::Friend),
        )
        .await;
    }
    ((ana, ana_acc), (ben, ben_acc, ben_id))
}

fn no(friend_requests: bool, invites: bool, chats: bool) -> PushKinds {
    PushKinds {
        friend_requests: !friend_requests,
        invites: !invites,
        chats: !chats,
    }
}

#[tokio::test]
async fn a_chat_in_the_background_sends_one_v1_message_per_chat() {
    let dir = tempfile::tempdir().unwrap();
    let (running, mut firebase) = server_with_firebase(dir.path()).await;
    let ((mut ana, ana_acc), (mut ben, ben_acc, _)) = friends(running.addr).await;
    ben.push("ben-phone", PushKinds::ALL).await;
    ben.away(true).await;
    ana.chat(&ben_acc).await;
    let first = firebase.next().await;
    let from = ana_acc.online_id.to_string();
    assert_eq!(first.auth, format!("Bearer {ACCESS}"));
    assert_eq!(
        first.body,
        json!({ "message": {
            "token": "ben-phone",
            "data": { "kind": "chat", "from": from, "name": "Ana", "count": "1" },
            "android": { "priority": "HIGH", "collapse_key": format!("chat-{from}") },
        }})
    );
    ana.chat(&ben_acc).await;
    let second = firebase.next().await;
    assert_eq!(second.body["message"]["data"]["count"], "2");
    assert_eq!(
        second.body["message"]["android"]["collapse_key"],
        first.body["message"]["android"]["collapse_key"]
    );
    running.stop().await;
}

#[tokio::test]
async fn the_server_signs_in_to_firebase_with_the_service_account() {
    let dir = tempfile::tempdir().unwrap();
    let (running, mut firebase) = server_with_firebase(dir.path()).await;
    let ((mut ana, _), (mut ben, ben_acc, _)) = friends(running.addr).await;
    ben.push("ben-phone", PushKinds::ALL).await;
    ben.away(true).await;
    ana.chat(&ben_acc).await;
    ana.chat(&ben_acc).await;
    firebase.next().await;
    firebase.next().await;
    let assertions = firebase.fake.assertions.lock().await.clone();
    assert_eq!(assertions.len(), 1, "the access token is kept");
    let parts: Vec<&str> = assertions[0].split('.').collect();
    let [header, claims, signature] = parts[..] else {
        panic!("not a JWT: {}", assertions[0]);
    };
    let decode = |part: &str| -> Value {
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
    };
    assert_eq!(decode(header), json!({ "alg": "RS256", "typ": "JWT" }));
    let claims_json = decode(claims);
    assert_eq!(claims_json["iss"], CLIENT_EMAIL);
    assert_eq!(
        claims_json["scope"],
        "https://www.googleapis.com/auth/firebase.messaging"
    );
    assert_eq!(claims_json["aud"], firebase.fake.token_uri);
    assert_eq!(
        claims_json["exp"].as_u64().unwrap() - claims_json["iat"].as_u64().unwrap(),
        3600
    );
    let verifier = VerifyingKey::<Sha256>::new(RsaPublicKey::from(key()));
    let signature = Signature::try_from(&URL_SAFE_NO_PAD.decode(signature).unwrap()[..]).unwrap();
    verifier
        .verify(format!("{header}.{claims}").as_bytes(), &signature)
        .unwrap();
    running.stop().await;
}

#[tokio::test]
async fn nothing_is_pushed_while_the_app_is_in_front() {
    let dir = tempfile::tempdir().unwrap();
    let (running, mut firebase) = server_with_firebase(dir.path()).await;
    let ((mut ana, _), (mut ben, ben_acc, ben_id)) = friends(running.addr).await;
    ben.push("ben-phone", PushKinds::ALL).await;
    ana.chat(&ben_acc).await;
    let mut tablet = Client::sign_in(running.addr, &ben_id).await;
    tablet.away(true).await;
    ana.chat(&ben_acc).await;
    ben.away(true).await;
    ana.chat(&ben_acc).await;
    let sent = firebase.next().await;
    assert_eq!(sent.body["message"]["token"], "ben-phone");
    assert_eq!(
        sent.body["message"]["data"]["count"], "3",
        "the first two chats sent nothing while Ben had the app in front"
    );
    running.stop().await;
}

#[tokio::test]
async fn friend_requests_and_invites_are_pushed_once_each() {
    let dir = tempfile::tempdir().unwrap();
    let (running, mut firebase) = server_with_firebase(dir.path()).await;
    let ((mut ana, ana_acc), (mut ben, ben_acc, _)) = friends(running.addr).await;
    ben.push("ben-phone", PushKinds::ALL).await;
    ben.away(true).await;
    let (mut cat, cat_acc) = Client::sign_up(running.addr, &RelayId::random(), "Cat").await;
    cat.send(ClientMsg::AddFriend(ben_acc.online_id)).await;
    cat.quiet().await;
    cat.send(ClientMsg::AddFriend(ben_acc.online_id)).await;
    cat.quiet().await;
    let request = firebase.next().await;
    assert_eq!(
        request.body["message"]["data"],
        json!({ "kind": "friend_request", "from": cat_acc.online_id.to_string(), "name": "Cat" })
    );
    ana.send(ClientMsg::Invite {
        to: ben_acc.online_id,
        sealed: sealed(),
    })
    .await;
    let ServerMsg::Invited { session, .. } = ana
        .expect("invited", |m| matches!(m, ServerMsg::Invited { .. }))
        .await
    else {
        unreachable!()
    };
    let invite = firebase.next().await;
    assert_eq!(
        invite.body["message"]["data"],
        json!({
            "kind": "invite",
            "from": ana_acc.online_id.to_string(),
            "name": "Ana",
            "session": session.to_string(),
        }),
        "the repeated friend request sent nothing"
    );
    assert_eq!(invite.body["message"]["android"]["ttl"], "300s");
    running.stop().await;
}

#[tokio::test]
async fn each_kind_follows_its_switch() {
    let dir = tempfile::tempdir().unwrap();
    let (running, mut firebase) = server_with_firebase(dir.path()).await;
    let ((mut ana, _), (mut ben, ben_acc, _)) = friends(running.addr).await;
    ben.away(true).await;
    ben.push("ben-phone", no(false, true, true)).await;
    ana.chat(&ben_acc).await;
    ana.send(ClientMsg::Invite {
        to: ben_acc.online_id,
        sealed: sealed(),
    })
    .await;
    ana.expect("invited", |m| matches!(m, ServerMsg::Invited { .. }))
        .await;
    let (mut cat, _) = Client::sign_up(running.addr, &RelayId::random(), "Cat").await;
    cat.send(ClientMsg::AddFriend(ben_acc.online_id)).await;
    cat.quiet().await;
    let sent = firebase.next().await;
    assert_eq!(
        sent.body["message"]["data"]["kind"], "friend_request",
        "the switched-off chat and invite sent nothing"
    );
    ben.push("ben-phone", no(true, true, false)).await;
    let (mut dan, _) = Client::sign_up(running.addr, &RelayId::random(), "Dan").await;
    dan.send(ClientMsg::AddFriend(ben_acc.online_id)).await;
    dan.quiet().await;
    ana.chat(&ben_acc).await;
    let sent = firebase.next().await;
    assert_eq!(
        sent.body["message"]["data"]["kind"], "chat",
        "the switched-off friend request sent nothing"
    );
    assert_eq!(sent.body["message"]["data"]["count"], "2");
    running.stop().await;
}

#[tokio::test]
async fn signing_out_removes_the_token() {
    let dir = tempfile::tempdir().unwrap();
    let (running, mut firebase) = server_with_firebase(dir.path()).await;
    let ((mut ana, _), (mut ben, ben_acc, ben_id)) = friends(running.addr).await;
    ben.push("ben-phone", PushKinds::ALL).await;
    ben.quiet().await;
    let this = ben.this.expect("the server names this device");
    ben.send(ClientMsg::SignOutDevice(this)).await;
    while ben.next().await.is_some() {}
    let mut tablet = Client::sign_in(running.addr, &ben_id).await;
    tablet.push("ben-tablet", PushKinds::ALL).await;
    tablet.away(true).await;
    ana.chat(&ben_acc).await;
    let sent = firebase.next().await;
    assert_eq!(sent.body["message"]["token"], "ben-tablet");
    tablet
        .send(ClientMsg::SetPush {
            token: None,
            kinds: PushKinds::ALL,
        })
        .await;
    tablet.quiet().await;
    let mut phone = Client::sign_in(running.addr, &ben_id).await;
    phone.push("ben-phone-2", PushKinds::ALL).await;
    phone.away(true).await;
    ana.chat(&ben_acc).await;
    let sent = firebase.next().await;
    assert_eq!(sent.body["message"]["token"], "ben-phone-2");
    running.stop().await;
}

#[tokio::test]
async fn a_token_firebase_no_longer_knows_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let (running, mut firebase) = server_with_firebase(dir.path()).await;
    let ((mut ana, _), (mut ben, ben_acc, ben_id)) = friends(running.addr).await;
    ben.push(GONE, PushKinds::ALL).await;
    ben.away(true).await;
    ana.chat(&ben_acc).await;
    assert_eq!(firebase.next().await.body["message"]["token"], GONE);
    ana.chat(&ben_acc).await;
    let mut tablet = Client::sign_in(running.addr, &ben_id).await;
    tablet.push("ben-tablet", PushKinds::ALL).await;
    tablet.away(true).await;
    ana.chat(&ben_acc).await;
    assert_eq!(firebase.next().await.body["message"]["token"], "ben-tablet");
    running.stop().await;
}

#[tokio::test]
async fn without_a_key_the_server_takes_tokens_and_sends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("broken.json"), "not a key").unwrap();
    for vars in [
        vec![],
        vec![(
            "CITRINE_FCM_KEY_FILE",
            dir.path().join("broken.json").display().to_string(),
        )],
        vec![(
            "CITRINE_FCM_KEY_FILE",
            dir.path().join("missing.json").display().to_string(),
        )],
    ] {
        let data = tempfile::tempdir().unwrap();
        let running = start(config(data.path(), &vars)).await.unwrap();
        let ((mut ana, _), (mut ben, ben_acc, _)) = friends(running.addr).await;
        ben.push("ben-phone", PushKinds::ALL).await;
        ben.away(true).await;
        ana.chat(&ben_acc).await;
        ben.expect("chat", |m| matches!(m, ServerMsg::Chat(_)))
            .await;
        running.stop().await;
    }
}

#[tokio::test]
async fn a_token_that_is_too_long_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let running = start(config(dir.path(), &[])).await.unwrap();
    let (mut ben, _) = Client::sign_up(running.addr, &RelayId::random(), "Ben").await;
    ben.send(ClientMsg::SetPush {
        token: Some("x".repeat(citrine_gb_net::MAX_PUSH_TOKEN_BYTES + 1)),
        kinds: PushKinds::ALL,
    })
    .await;
    let msg = ben.next().await.unwrap();
    assert_eq!(msg, ServerMsg::Error(citrine_gb_net::ErrorCode::TooLarge));
    running.stop().await;
}

#[tokio::test]
async fn an_invite_to_a_closed_app_rings_and_waits_for_it_to_open() {
    let dir = tempfile::tempdir().unwrap();
    let (running, mut firebase) = server_with_firebase(dir.path()).await;
    let ((mut ana, ana_acc), (mut ben, ben_acc, ben_id)) = friends(running.addr).await;
    ben.push("ben-phone", PushKinds::ALL).await;
    ben.quiet().await;
    closes_the_app(&mut ana, ben).await;
    let session = invite(&mut ana, &ben_acc).await;
    let rang = firebase.next().await;
    assert_eq!(rang.body["message"]["token"], "ben-phone");
    assert_eq!(rang.body["message"]["data"]["kind"], "invite");
    assert_eq!(rang.body["message"]["data"]["session"], session.to_string());
    let mut opened = Client::sign_in(running.addr, &ben_id).await;
    let invite = opened
        .expect("the open invite", |m| matches!(m, ServerMsg::Invite { .. }))
        .await;
    assert_eq!(
        invite,
        ServerMsg::Invite {
            session,
            from: ana_acc.online_id,
            sealed: sealed(),
        }
    );
    opened
        .send(ClientMsg::AnswerInvite {
            session,
            accept: true,
        })
        .await;
    ana.expect(
        "the session starts",
        |m| matches!(m, ServerMsg::SessionStarted { session: s, .. } if *s == session),
    )
    .await;
    running.stop().await;
}

#[tokio::test]
async fn an_unanswered_invite_ends_after_its_time() {
    let dir = tempfile::tempdir().unwrap();
    let open = [("CITRINE_INVITE_OPEN_SECS", "1".to_string())];
    let (running, mut firebase) = server_with_firebase_and(dir.path(), &open).await;
    let ((mut ana, _), (mut ben, ben_acc, ben_id)) = friends(running.addr).await;
    ben.push("ben-phone", PushKinds::ALL).await;
    ben.quiet().await;
    closes_the_app(&mut ana, ben).await;
    let session = invite(&mut ana, &ben_acc).await;
    firebase.next().await;
    let ended = ana
        .expect("no answer", |m| matches!(m, ServerMsg::SessionEnded { .. }))
        .await;
    assert_eq!(
        ended,
        ServerMsg::SessionEnded {
            session,
            reason: EndReason::NoAnswer,
        }
    );
    let mut opened = Client::sign_in(running.addr, &ben_id).await;
    opened.quiet().await;
    opened
        .send(ClientMsg::AnswerInvite {
            session,
            accept: true,
        })
        .await;
    assert_eq!(
        opened
            .expect("gone", |m| {
                matches!(m, ServerMsg::Error(_) | ServerMsg::Invite { .. })
            })
            .await,
        ServerMsg::Error(ErrorCode::UnknownSession)
    );
    running.stop().await;
}

#[tokio::test]
async fn a_closed_app_without_invite_pushes_is_offline() {
    let dir = tempfile::tempdir().unwrap();
    let (running, _firebase) = server_with_firebase(dir.path()).await;
    let ((mut ana, _), (mut ben, ben_acc, _)) = friends(running.addr).await;
    ben.push("ben-phone", no(false, true, false)).await;
    ben.quiet().await;
    closes_the_app(&mut ana, ben).await;
    ana.send(ClientMsg::Invite {
        to: ben_acc.online_id,
        sealed: sealed(),
    })
    .await;
    assert_eq!(
        ana.expect("refused", |m| matches!(m, ServerMsg::Error(_)))
            .await,
        ServerMsg::Error(ErrorCode::Offline)
    );
    running.stop().await;
}
