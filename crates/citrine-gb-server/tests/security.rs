use citrine_gb_net::{
    Account, Channel, ClientHandshake, ClientMsg, DeviceKey, ErrorCode, OneTimeCode,
    PROTOCOL_VERSION, RelayId, RelayKey, RelayPassword, ServerMsg,
};
use citrine_gb_server::{Config, Running, start};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const PASSWORD: &str = "lemon tree";

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn server_with(dir: &Path, vars: &[(&str, &str)]) -> Running {
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
        ("CITRINE_WRONG_TRIES_PER_MINUTE".into(), "100".into()),
        (
            "CITRINE_DATA_DIR".into(),
            dir.join("data").display().to_string(),
        ),
        ("CITRINE_WEB".into(), "off".into()),
        ("CITRINE_RELAY".into(), "on".into()),
        ("CITRINE_RELAY_PASSWORD".into(), PASSWORD.into()),
    ]);
    for (k, v) in vars {
        map.insert(k.to_string(), v.to_string());
    }
    start(Config::from_vars(|k| map.get(k).cloned()).unwrap())
        .await
        .unwrap()
}

async fn server(dir: &Path) -> Running {
    server_with(dir, &[]).await
}

async fn open(addr: SocketAddr) -> Ws {
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/relay"))
        .await
        .unwrap();
    ws
}

async fn next_binary(ws: &mut Ws) -> Option<Vec<u8>> {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timed out waiting for the server");
        match frame {
            Some(Ok(Message::Binary(bytes))) => return Some(bytes.to_vec()),
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return None,
            Some(Ok(_)) => continue,
        }
    }
}

struct Client {
    ws: Ws,
    channel: Channel,
    key: RelayKey,
}

impl Client {
    async fn connect(addr: SocketAddr) -> Self {
        let mut ws = open(addr).await;
        let (handshake, first) = ClientHandshake::start();
        ws.send(Message::Binary(first.into())).await.unwrap();
        let reply = next_binary(&mut ws).await.expect("no handshake reply");
        let (key, channel) = handshake.finish(&reply).unwrap();
        Self { ws, channel, key }
    }

    async fn hello(addr: SocketAddr) -> Self {
        let mut c = Self::connect(addr).await;
        c.send(ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            app: "test".into(),
        })
        .await;
        match c.recv().await {
            ServerMsg::Info(_) => c,
            other => panic!("expected info, got {other:?}"),
        }
    }

    async fn send(&mut self, msg: ClientMsg) {
        let sealed = self.channel.seal(&msg.encode().unwrap());
        self.ws.send(Message::Binary(sealed.into())).await.unwrap();
    }

    async fn next(&mut self) -> Option<ServerMsg> {
        loop {
            let bytes = next_binary(&mut self.ws).await?;
            match ServerMsg::decode(&self.channel.open(&bytes).unwrap()).unwrap() {
                ServerMsg::Devices { .. } => continue,
                msg => return Some(msg),
            }
        }
    }

    async fn recv(&mut self) -> ServerMsg {
        self.next().await.expect("the server closed the connection")
    }

    async fn until(&mut self, what: &str, pred: impl Fn(&ServerMsg) -> bool) -> ServerMsg {
        loop {
            let msg = self.recv().await;
            if pred(&msg) {
                return msg;
            }
            if let ServerMsg::Error(e) = msg {
                panic!("expected {what}, got error {e:?}");
            }
        }
    }

    async fn error(&mut self) -> ErrorCode {
        loop {
            if let ServerMsg::Error(e) = self.recv().await {
                return e;
            }
        }
    }

    async fn account(&mut self) -> Account {
        match self
            .until("an account", |m| matches!(m, ServerMsg::Account(_)))
            .await
        {
            ServerMsg::Account(a) => a,
            _ => unreachable!(),
        }
    }
}

async fn sign_up(addr: SocketAddr, id: &RelayId) -> (Client, DeviceKey) {
    let mut c = Client::hello(addr).await;
    let keys = id.keys();
    c.send(ClientMsg::SignUp {
        relay_password: Some(RelayPassword(PASSWORD.into())),
        auth_key: keys.auth_key().clone(),
        public_key: keys.public_key(),
        display_name: "Ada".into(),
        invite: None,
        device_name: "Test".into(),
    })
    .await;
    let device = match c.recv().await {
        ServerMsg::Device(d) => d,
        other => panic!("expected a device key, got {other:?}"),
    };
    match c.recv().await {
        ServerMsg::Welcome(_) => {}
        other => panic!("expected welcome, got {other:?}"),
    }
    (c, device)
}

enum SignIn {
    New(Client, DeviceKey, Account),
    Known(Client, Account),
    Refused(ErrorCode),
}

impl SignIn {
    fn refused(self) -> ErrorCode {
        match self {
            SignIn::Refused(e) => e,
            _ => panic!("expected a refusal"),
        }
    }

    fn device(self) -> (Client, DeviceKey, Account) {
        match self {
            SignIn::New(c, d, a) => (c, d, a),
            SignIn::Known(..) => panic!("expected a new device key, got none"),
            SignIn::Refused(e) => panic!("expected a sign-in, got {e:?}"),
        }
    }

    fn known(self) -> (Client, Account) {
        match self {
            SignIn::Known(c, a) => (c, a),
            SignIn::New(..) => panic!("expected a known device, got a new key"),
            SignIn::Refused(e) => panic!("expected a sign-in, got {e:?}"),
        }
    }
}

async fn sign_in(
    addr: SocketAddr,
    id: &RelayId,
    device: Option<&DeviceKey>,
    code: Option<&str>,
) -> SignIn {
    let mut c = Client::hello(addr).await;
    c.send(ClientMsg::SignIn {
        relay_password: Some(RelayPassword(PASSWORD.into())),
        auth_key: id.keys().auth_key().clone(),
        device: device.cloned(),
        code: code.map(|c| OneTimeCode(c.into())),
        device_name: "Test".into(),
    })
    .await;
    match c.recv().await {
        ServerMsg::Error(e) => SignIn::Refused(e),
        ServerMsg::Device(d) => match c.recv().await {
            ServerMsg::Welcome(a) => SignIn::New(c, d, a),
            other => panic!("expected welcome, got {other:?}"),
        },
        ServerMsg::Welcome(a) => SignIn::Known(c, a),
        other => panic!("expected a sign-in answer, got {other:?}"),
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn code_at(secret: &[u8], time: u64) -> String {
    totp_rs::Builder::new()
        .with_secret(secret.to_vec())
        .build_noncompliant()
        .generate(time)
        .to_string()
}

fn wrong(code: &str) -> String {
    code.chars()
        .map(|c| match c.to_digit(10) {
            Some(d) => char::from_digit((d + 5) % 10, 10).unwrap(),
            None => c,
        })
        .collect()
}

async fn turn_on(c: &mut Client) -> (Vec<u8>, u64, Vec<String>) {
    c.send(ClientMsg::TwoFactorStart).await;
    let secret = match c
        .until("a setup secret", |m| {
            matches!(m, ServerMsg::TwoFactorSetup { .. })
        })
        .await
    {
        ServerMsg::TwoFactorSetup { secret } => secret,
        _ => unreachable!(),
    };
    let base = unix_now();
    c.send(ClientMsg::TwoFactorConfirm(OneTimeCode(code_at(
        &secret, base,
    ))))
    .await;
    let codes = match c
        .until("recovery codes", |m| {
            matches!(m, ServerMsg::RecoveryCodes(_))
        })
        .await
    {
        ServerMsg::RecoveryCodes(codes) => codes,
        _ => unreachable!(),
    };
    let account = c.account().await;
    assert!(account.two_factor);
    assert_eq!(account.codes_left as usize, codes.len());
    (secret, base, codes)
}

#[tokio::test]
async fn two_factor_asks_new_devices_for_a_code() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let id = RelayId::random();
    let (mut first, kept) = sign_up(s.addr, &id).await;
    let (mut c, _, _) = sign_in(s.addr, &id, None, None).await.device();

    c.send(ClientMsg::TwoFactorStart).await;
    let secret = match c
        .until("a setup secret", |m| {
            matches!(m, ServerMsg::TwoFactorSetup { .. })
        })
        .await
    {
        ServerMsg::TwoFactorSetup { secret } => secret,
        _ => unreachable!(),
    };
    c.send(ClientMsg::TwoFactorConfirm(OneTimeCode(wrong(&code_at(
        &secret,
        unix_now(),
    )))))
    .await;
    assert_eq!(c.error().await, ErrorCode::CodeWrong);
    let base = unix_now();
    c.send(ClientMsg::TwoFactorConfirm(OneTimeCode(code_at(
        &secret, base,
    ))))
    .await;
    let codes = match c
        .until("recovery codes", |m| {
            matches!(m, ServerMsg::RecoveryCodes(_))
        })
        .await
    {
        ServerMsg::RecoveryCodes(codes) => codes,
        _ => unreachable!(),
    };
    assert_eq!(codes.len(), 10);
    assert_eq!(c.account().await.codes_left, 10);

    let needed = sign_in(s.addr, &id, None, None).await.refused();
    assert_eq!(needed, ErrorCode::CodeNeeded);
    first.send(ClientMsg::Ping(7)).await;
    first
        .until("the first device still online", |m| {
            matches!(m, ServerMsg::Pong(7))
        })
        .await;
    let (_kept, _) = sign_in(s.addr, &id, Some(&kept), None).await.known();
    let next = code_at(&secret, base + 30);
    let refused = sign_in(s.addr, &id, None, Some(&wrong(&next)))
        .await
        .refused();
    assert_eq!(refused, ErrorCode::CodeWrong);

    let (_phone, phone, account) = sign_in(s.addr, &id, None, Some(&next)).await.device();
    assert!(account.two_factor);
    let replayed = sign_in(s.addr, &id, None, Some(&next)).await.refused();
    assert_eq!(replayed, ErrorCode::CodeWrong);

    let (_laptop, _, account) = sign_in(s.addr, &id, None, Some(&codes[0])).await.device();
    assert_eq!(account.codes_left, 9);
    let again = sign_in(s.addr, &id, None, Some(&codes[0])).await.refused();
    assert_eq!(again, ErrorCode::CodeWrong);

    let (mut c, account) = sign_in(s.addr, &id, Some(&phone), None).await.known();
    assert_eq!(account.codes_left, 9);

    c.send(ClientMsg::TwoFactorOff(OneTimeCode(wrong(&codes[1]))))
        .await;
    assert_eq!(c.error().await, ErrorCode::CodeWrong);
    c.send(ClientMsg::TwoFactorOff(OneTimeCode(codes[1].clone())))
        .await;
    let account = c.account().await;
    assert!(!account.two_factor);
    assert_eq!(account.codes_left, 0);
    let (_free, _, _) = sign_in(s.addr, &id, None, None).await.device();
    s.stop().await;
}

#[tokio::test]
async fn new_recovery_codes_replace_the_old_ones() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let id = RelayId::random();
    let (mut c, _) = sign_up(s.addr, &id).await;
    let (_, _, old) = turn_on(&mut c).await;
    c.send(ClientMsg::NewRecoveryCodes(OneTimeCode(old[0].clone())))
        .await;
    let fresh = match c
        .until("recovery codes", |m| {
            matches!(m, ServerMsg::RecoveryCodes(_))
        })
        .await
    {
        ServerMsg::RecoveryCodes(codes) => codes,
        _ => unreachable!(),
    };
    assert_eq!(c.account().await.codes_left, 10);
    let stale = sign_in(s.addr, &id, None, Some(&old[1])).await.refused();
    assert_eq!(stale, ErrorCode::CodeWrong);
    sign_in(s.addr, &id, None, Some(&fresh[0])).await.device();
    s.stop().await;
}

#[tokio::test]
async fn wrong_codes_lock_the_account() {
    let dir = tempfile::tempdir().unwrap();
    let s = server_with(dir.path(), &[("CITRINE_WRONG_TRIES_PER_MINUTE", "3")]).await;
    let id = RelayId::random();
    let (mut c, _) = sign_up(s.addr, &id).await;
    let (secret, base, _) = turn_on(&mut c).await;
    let next = code_at(&secret, base + 30);
    let mut refusals = Vec::new();
    for _ in 0..4 {
        refusals.push(
            sign_in(s.addr, &id, None, Some(&wrong(&next)))
                .await
                .refused(),
        );
    }
    assert_eq!(refusals[0], ErrorCode::CodeWrong);
    assert_eq!(refusals[3], ErrorCode::TooManyTries);
    let locked = sign_in(s.addr, &id, None, Some(&next)).await.refused();
    assert_eq!(locked, ErrorCode::TooManyTries);
    s.stop().await;
}

#[tokio::test]
async fn deleting_an_account_asks_for_the_code() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let id = RelayId::random();
    let (mut c, _) = sign_up(s.addr, &id).await;
    let (_, _, codes) = turn_on(&mut c).await;
    c.send(ClientMsg::DeleteAccount { code: None }).await;
    assert_eq!(c.error().await, ErrorCode::CodeNeeded);
    c.send(ClientMsg::DeleteAccount {
        code: Some(OneTimeCode(wrong(&codes[0]))),
    })
    .await;
    assert_eq!(c.error().await, ErrorCode::CodeWrong);
    c.send(ClientMsg::DeleteAccount {
        code: Some(OneTimeCode(codes[0].clone())),
    })
    .await;
    c.until("deletion", |m| matches!(m, ServerMsg::AccountDeleted))
        .await;
    while c.next().await.is_some() {}
    let gone = sign_in(s.addr, &id, None, Some(&codes[1])).await.refused();
    assert_eq!(gone, ErrorCode::UnknownAccount);

    let plain = RelayId::random();
    let (mut p, _) = sign_up(s.addr, &plain).await;
    p.send(ClientMsg::DeleteAccount { code: None }).await;
    p.until("deletion", |m| matches!(m, ServerMsg::AccountDeleted))
        .await;
    let gone = sign_in(s.addr, &plain, None, None).await.refused();
    assert_eq!(gone, ErrorCode::UnknownAccount);
    s.stop().await;
}

#[tokio::test]
async fn the_relay_key_stays_the_same_across_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let key = s.relay_key();
    let c = Client::connect(s.addr).await;
    assert_eq!(c.key, key);
    drop(c);
    s.stop().await;

    let s = server(dir.path()).await;
    assert_eq!(s.relay_key(), key);
    let c = Client::connect(s.addr).await;
    assert_eq!(c.key, key);
    drop(c);
    s.stop().await;

    let other = tempfile::tempdir().unwrap();
    let s = server(other.path()).await;
    assert_ne!(s.relay_key(), key);
    s.stop().await;
}

#[tokio::test]
async fn an_old_app_hears_the_new_protocol_version() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let mut ws = open(s.addr).await;
    let hello = ClientMsg::Hello {
        protocol: 1,
        app: "old".into(),
    };
    ws.send(Message::Binary(hello.encode().unwrap().into()))
        .await
        .unwrap();
    let info = ServerMsg::decode(&next_binary(&mut ws).await.unwrap()).unwrap();
    assert!(matches!(info, ServerMsg::Info(_)));
    let error = ServerMsg::decode(&next_binary(&mut ws).await.unwrap()).unwrap();
    assert!(matches!(
        error,
        ServerMsg::Error(ErrorCode::Protocol { server: 2 })
    ));
    assert!(next_binary(&mut ws).await.is_none());

    let mut junk = open(s.addr).await;
    junk.send(Message::Binary(b"CGBNnot a handshake".to_vec().into()))
        .await
        .unwrap();
    assert!(next_binary(&mut junk).await.is_none());
    s.stop().await;
}

#[tokio::test]
async fn a_tampered_frame_closes_the_connection() {
    let dir = tempfile::tempdir().unwrap();
    let s = server(dir.path()).await;
    let mut c = Client::connect(s.addr).await;
    let mut sealed = c.channel.seal(
        &ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            app: "test".into(),
        }
        .encode()
        .unwrap(),
    );
    let last = sealed.len() - 1;
    sealed[last] ^= 1;
    c.ws.send(Message::Binary(sealed.into())).await.unwrap();
    assert!(c.next().await.is_none());
    s.stop().await;
}
