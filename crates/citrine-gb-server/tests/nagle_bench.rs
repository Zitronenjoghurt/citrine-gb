#![expect(
    clippy::disallowed_types,
    clippy::disallowed_methods,
    reason = "the bench runs each player's pacing and reading on its own thread"
)]

use citrine_gb_net::{
    Account, ClientHandshake, ClientMsg, FriendState, NONCE_BYTES, Opener, PROTOCOL_VERSION,
    RelayId, RelayPassword, Sealed, Sealer, ServerMsg, SessionId,
};
use citrine_gb_server::{Config, Running, start};
use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite;
use tungstenite::Message;
use tungstenite::client::IntoClientRequest;

const PASSWORD: &str = "lemon tree";
const FRAME: Duration = Duration::from_nanos(16_742_706);
const WAIT: Duration = Duration::from_secs(5);

fn config(dir: &Path) -> Config {
    let map: HashMap<String, String> = [
        ("CITRINE_BIND", "127.0.0.1".to_string()),
        ("CITRINE_PORT", "0".into()),
        ("CITRINE_STUN_PORT", "0".into()),
        ("CITRINE_DISK_FLOOR_MB", "0".into()),
        ("CITRINE_DISK_FLOOR_PERCENT", "0".into()),
        ("CITRINE_IP_CONNECTIONS", "100000".into()),
        ("CITRINE_IP_UNSIGNED", "100000".into()),
        ("CITRINE_IP_CONNECTS_PER_MINUTE", "100000".into()),
        ("CITRINE_SIGN_UPS_PER_IP", "100000".into()),
        ("CITRINE_SIGN_UPS_PER_HOUR", "100000".into()),
        ("CITRINE_MESSAGES_PER_SECOND", "100000".into()),
        ("CITRINE_MESSAGE_BURST", "100000".into()),
        ("CITRINE_FRIEND_REQUESTS_PER_HOUR", "100000".into()),
        ("CITRINE_FRIEND_REQUEST_BURST", "100000".into()),
        ("CITRINE_DATA_DIR", dir.join("data").display().to_string()),
        (
            "CITRINE_STATIC_DIR",
            dir.join("static").display().to_string(),
        ),
        ("CITRINE_RELAY", "on".into()),
        ("CITRINE_RELAY_PASSWORD", PASSWORD.into()),
        ("CITRINE_WEB", "false".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    Config::from_vars(|k| map.get(k).cloned()).unwrap()
}

#[derive(Clone, Copy)]
enum Client {
    PollLoop { nodelay: bool },
    Citrine,
}

type Out = Box<dyn Fn(Vec<u8>) + Send + Sync>;
type Inbox = Receiver<(Instant, Vec<u8>)>;

fn poll_loop(
    mut ws: tungstenite::WebSocket<TcpStream>,
    rx: Receiver<Vec<u8>>,
    inbox: Sender<(Instant, Vec<u8>)>,
) {
    ws.get_mut()
        .set_read_timeout(Some(Duration::from_millis(10)))
        .unwrap();
    loop {
        match rx.try_recv() {
            Ok(bytes) => ws.send(Message::Binary(bytes.into())).unwrap(),
            Err(TryRecvError::Disconnected) => {
                let _ = ws.close(None);
                let _ = ws.flush();
                return;
            }
            Err(TryRecvError::Empty) => {}
        }
        match ws.read() {
            Ok(Message::Binary(bytes)) => {
                let _ = inbox.send((Instant::now(), bytes.into()));
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => panic!("read: {e}"),
        }
        std::thread::yield_now();
    }
}

fn pipe(addr: SocketAddr, client: Client) -> (Out, Inbox) {
    let url = format!("ws://{addr}/relay");
    let (inbox_tx, inbox) = channel();
    match client {
        Client::PollLoop { nodelay } => {
            let tcp = TcpStream::connect(addr).unwrap();
            tcp.set_nodelay(nodelay).unwrap();
            let (ws, _) =
                tungstenite::client::client(url.into_client_request().unwrap(), tcp).unwrap();
            let (tx, rx) = channel::<Vec<u8>>();
            std::thread::spawn(move || poll_loop(ws, rx, inbox_tx));
            let tx = Mutex::new(tx);
            (
                Box::new(move |bytes| tx.lock().unwrap().send(bytes).unwrap()),
                inbox,
            )
        }
        Client::Citrine => {
            let (opened_tx, opened) = channel();
            let sender = citrine_gb_websocket::connect(&url, 1 << 20, move |event| match event {
                citrine_gb_websocket::Event::Opened => {
                    let _ = opened_tx.send(());
                }
                citrine_gb_websocket::Event::Message(citrine_gb_websocket::Message::Binary(
                    bytes,
                )) => {
                    let _ = inbox_tx.send((Instant::now(), bytes));
                }
                citrine_gb_websocket::Event::Message(_) => {}
                other => panic!("{other:?}"),
            })
            .unwrap();
            opened.recv_timeout(WAIT).unwrap();
            let sender = Mutex::new(sender);
            (
                Box::new(move |bytes| {
                    sender
                        .lock()
                        .unwrap()
                        .send(citrine_gb_websocket::Message::Binary(bytes))
                }),
                inbox,
            )
        }
    }
}

struct Writer {
    out: Out,
    sealer: Sealer,
}

impl Writer {
    fn send(&mut self, msg: &ClientMsg) {
        (self.out)(self.sealer.seal(&msg.encode().unwrap()));
    }
}

struct Reader {
    inbox: Inbox,
    opener: Opener,
}

impl Reader {
    fn recv_timeout(&mut self, wait: Duration) -> Option<(Instant, ServerMsg)> {
        loop {
            let (at, bytes) = self.inbox.recv_timeout(wait).ok()?;
            match ServerMsg::decode(&self.opener.open(&bytes).unwrap()).unwrap() {
                ServerMsg::Device(_) | ServerMsg::Devices { .. } => continue,
                msg => return Some((at, msg)),
            }
        }
    }

    fn expect(&mut self, pred: impl Fn(&ServerMsg) -> bool) -> ServerMsg {
        loop {
            let (_, msg) = self.recv_timeout(WAIT).unwrap();
            if pred(&msg) {
                return msg;
            }
        }
    }
}

struct Conn {
    writer: Writer,
    reader: Reader,
}

impl Conn {
    fn open(addr: SocketAddr, client: Client) -> Self {
        let (out, inbox) = pipe(addr, client);
        let (handshake, first) = ClientHandshake::start();
        out(first);
        let (_, reply) = inbox.recv_timeout(WAIT).unwrap();
        let (_, channel) = handshake.finish(&reply).unwrap();
        let (sealer, opener) = channel.split();
        Self {
            writer: Writer { out, sealer },
            reader: Reader { inbox, opener },
        }
    }

    fn send(&mut self, msg: &ClientMsg) {
        self.writer.send(msg);
    }

    fn expect(&mut self, pred: impl Fn(&ServerMsg) -> bool) -> ServerMsg {
        self.reader.expect(pred)
    }

    fn sign_up(addr: SocketAddr, client: Client, name: &str) -> (Self, Account) {
        let mut c = Self::open(addr, client);
        c.send(&ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            app: "bench".into(),
        });
        c.expect(|m| matches!(m, ServerMsg::Info(_)));
        let id = RelayId::random();
        let keys = id.keys();
        c.send(&ClientMsg::SignUp {
            relay_password: Some(RelayPassword(PASSWORD.into())),
            auth_key: keys.auth_key().clone(),
            public_key: keys.public_key(),
            display_name: name.into(),
            invite: None,
            device_name: "Bench".into(),
        });
        let ServerMsg::Welcome(account) = c.expect(|m| matches!(m, ServerMsg::Welcome(_))) else {
            unreachable!()
        };
        c.expect(|m| matches!(m, ServerMsg::Friends(_)));
        (c, account)
    }
}

fn session(addr: SocketAddr, client: Client) -> (Conn, Conn, SessionId) {
    let (mut a, a_acc) = Conn::sign_up(addr, client, "Ana");
    let (mut b, b_acc) = Conn::sign_up(addr, client, "Ben");
    a.send(&ClientMsg::AddFriend(b_acc.online_id));
    b.expect(|m| matches!(m, ServerMsg::Friend(f) if f.state == FriendState::Incoming));
    b.send(&ClientMsg::AnswerFriend {
        from: a_acc.online_id,
        accept: true,
    });
    a.expect(|m| matches!(m, ServerMsg::Friend(f) if f.state == FriendState::Friend));
    b.expect(|m| matches!(m, ServerMsg::Friend(f) if f.state == FriendState::Friend));
    a.send(&ClientMsg::Invite {
        to: b_acc.online_id,
        sealed: Sealed {
            nonce: [1; NONCE_BYTES],
            ciphertext: vec![1; 40],
        },
    });
    let ServerMsg::Invited { session, .. } = a.expect(|m| matches!(m, ServerMsg::Invited { .. }))
    else {
        unreachable!()
    };
    b.expect(|m| matches!(m, ServerMsg::Invite { .. }));
    b.send(&ClientMsg::AnswerInvite {
        session,
        accept: true,
    });
    b.expect(|m| matches!(m, ServerMsg::SessionStarted { .. }));
    a.expect(|m| matches!(m, ServerMsg::SessionStarted { .. }));
    (a, b, session)
}

const PING: u8 = 1;
const ECHO: u8 = 2;
const FRAME_MSG: u8 = 3;

fn play(session: SessionId, kind: u8, seq: u64, sent: u64) -> ClientMsg {
    let mut ciphertext = vec![0u8; 40];
    ciphertext[0] = kind;
    ciphertext[1..9].copy_from_slice(&seq.to_le_bytes());
    ciphertext[9..17].copy_from_slice(&sent.to_le_bytes());
    let mut nonce = [kind; NONCE_BYTES];
    nonce[..8].copy_from_slice(&seq.to_le_bytes());
    ClientMsg::Play {
        session,
        sealed: Sealed { nonce, ciphertext },
    }
}

fn parse(msg: &ServerMsg) -> Option<(u8, u64, u64)> {
    let ServerMsg::Play { sealed, .. } = msg else {
        return None;
    };
    let c = &sealed.ciphertext;
    Some((
        c[0],
        u64::from_le_bytes(c[1..9].try_into().unwrap()),
        u64::from_le_bytes(c[9..17].try_into().unwrap()),
    ))
}

struct Samples {
    rtt: Vec<u64>,
    one_way: Vec<u64>,
}

fn listen(
    mut reader: Reader,
    writer: Arc<Mutex<Writer>>,
    session: SessionId,
    base: Instant,
    echo: bool,
    stop: Arc<AtomicBool>,
) -> Samples {
    let mut samples = Samples {
        rtt: Vec::new(),
        one_way: Vec::new(),
    };
    while !stop.load(Ordering::Relaxed) {
        let Some((at, msg)) = reader.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };
        let now = (at - base).as_nanos() as u64;
        match parse(&msg) {
            Some((PING, seq, sent)) if echo => {
                samples.one_way.push(now - sent);
                writer.lock().unwrap().send(&play(session, ECHO, seq, sent));
            }
            Some((ECHO, _, sent)) => samples.rtt.push(now - sent),
            Some((FRAME_MSG, _, sent)) => samples.one_way.push(now - sent),
            _ => {}
        }
    }
    samples
}

fn pace(
    session: SessionId,
    base: Instant,
    kind: u8,
    offset: Duration,
    secs: u64,
    writer: Arc<Mutex<Writer>>,
) {
    let frames = secs * 60;
    let start = Instant::now() + offset;
    for seq in 0..frames {
        let due = start + FRAME * seq as u32;
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        let mut writer = writer.lock().unwrap();
        let sent = base.elapsed().as_nanos() as u64;
        writer.send(&play(session, kind, seq, sent));
    }
}

struct Stats {
    p50: f64,
    p95: f64,
    max: f64,
    n: usize,
}

fn stats(mut v: Vec<u64>) -> Stats {
    v.sort_unstable();
    let ms = |x: u64| x as f64 / 1e6;
    Stats {
        p50: ms(v[v.len() / 2]),
        p95: ms(v[v.len() * 95 / 100]),
        max: ms(*v.last().unwrap()),
        n: v.len(),
    }
}

fn run_case(client: Client, secs: u64) -> (Stats, Stats) {
    let dir = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let running: Running = runtime.block_on(start(config(dir.path()))).unwrap();
    let (a, b, session) = session(running.addr, client);
    let base = Instant::now();
    let stop = Arc::new(AtomicBool::new(false));
    let a_writer = Arc::new(Mutex::new(a.writer));
    let b_writer = Arc::new(Mutex::new(b.writer));
    let a_thread = {
        let (writer, stop) = (a_writer.clone(), stop.clone());
        std::thread::spawn(move || listen(a.reader, writer, session, base, false, stop))
    };
    let b_thread = {
        let (writer, stop) = (b_writer.clone(), stop.clone());
        std::thread::spawn(move || listen(b.reader, writer, session, base, true, stop))
    };
    let a_pace = {
        let writer = a_writer.clone();
        std::thread::spawn(move || pace(session, base, PING, Duration::ZERO, secs, writer))
    };
    let b_pace = {
        let writer = b_writer.clone();
        std::thread::spawn(move || pace(session, base, FRAME_MSG, FRAME * 37 / 100, secs, writer))
    };
    a_pace.join().unwrap();
    b_pace.join().unwrap();
    std::thread::sleep(Duration::from_secs(1));
    stop.store(true, Ordering::Relaxed);
    let a = a_thread.join().unwrap();
    let b = b_thread.join().unwrap();
    drop((a_writer, b_writer));
    runtime.block_on(running.stop());
    let mut one_way = a.one_way;
    one_way.extend(b.one_way);
    (stats(a.rtt), stats(one_way))
}

#[test]
#[ignore]
fn nagle_relay_round_trip() {
    let secs: u64 = std::env::var("NAGLE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let runs: usize = std::env::var("NAGLE_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let cases = [
        (
            "ewebsock loop, Nagle on",
            Client::PollLoop { nodelay: false },
        ),
        (
            "ewebsock loop, Nagle off",
            Client::PollLoop { nodelay: true },
        ),
        ("citrine-gb-websocket", Client::Citrine),
    ];
    let mut results: Vec<Vec<(Stats, Stats)>> = cases.iter().map(|_| Vec::new()).collect();
    for run in 0..runs {
        for (i, (name, client)) in cases.iter().enumerate() {
            let r = run_case(*client, secs);
            println!(
                "run {run} {name}: rtt p50 {:.2} p95 {:.2} max {:.2} n {} | one-way p50 {:.2} p95 {:.2} max {:.2} n {}",
                r.0.p50, r.0.p95, r.0.max, r.0.n, r.1.p50, r.1.p95, r.1.max, r.1.n
            );
            results[i].push(r);
        }
    }
    for (i, (name, ..)) in cases.iter().enumerate() {
        println!("\n{name} ({secs} s per run, ms)");
        println!("| run | rtt p50 | rtt p95 | rtt max | one-way p50 | one-way p95 | one-way max |");
        println!("|---|---|---|---|---|---|---|");
        for (run, (rtt, ow)) in results[i].iter().enumerate() {
            println!(
                "| {run} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
                rtt.p50, rtt.p95, rtt.max, ow.p50, ow.p95, ow.max
            );
        }
    }
}
