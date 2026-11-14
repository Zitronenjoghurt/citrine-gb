#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "the test runs its own slow link on plain threads"
)]

use citrine_gb::console::ConsoleKind;
use citrine_gb_app::online::cloud::{LocalSave, SaveStore, SyncState};
use citrine_gb_app::online::transport::{self, Connector, SideLine, Transport, TransportEvent};
use citrine_gb_app::online::{JoinedRelay, Online, OnlineSettings, Phase, ServerAddress, Trust};
use citrine_gb_net::{RelayId, RelayKey};
use citrine_gb_server::{Config, Running, start};
use citrine_gb_threads::{Mode, Threads};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::rc::Rc;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_time::Instant;

const PASSWORD: &str = "lemon tree";
const ROM: &str = "5a17c0de5a17c0de5a17c0de5a17c0de5a17c0de5a17c0de5a17c0de5a17c0de";
const TITLE: &str = "Test Game";
const FRAME: Duration = Duration::from_millis(16);

struct Relay {
    runtime: tokio::runtime::Runtime,
    running: Option<Running>,
    address: ServerAddress,
    _dir: tempfile::TempDir,
}

impl Relay {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let vars: HashMap<&str, String> = HashMap::from([
            ("CITRINE_BIND", "127.0.0.1".into()),
            ("CITRINE_PORT", "0".into()),
            ("CITRINE_DISK_FLOOR_MB", "0".into()),
            ("CITRINE_DISK_FLOOR_PERCENT", "0".into()),
            (
                "CITRINE_DATA_DIR",
                dir.path().join("data").display().to_string(),
            ),
            ("CITRINE_WEB", "false".into()),
            ("CITRINE_RELAY", "true".into()),
            ("CITRINE_RELAY_PASSWORD", PASSWORD.into()),
            ("CITRINE_STORAGE_MB", "200".into()),
        ]);
        let config = Config::from_vars(|k| vars.get(k).cloned()).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let running = runtime.block_on(start(config)).unwrap();
        let address = ServerAddress::parse(&format!("127.0.0.1:{}", running.addr.port())).unwrap();
        Self {
            runtime,
            running: Some(running),
            address,
            _dir: dir,
        }
    }

    fn port(&self) -> u16 {
        self.running.as_ref().unwrap().addr.port()
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            self.runtime.block_on(running.stop());
        }
    }
}

#[derive(Clone, Copy)]
struct Shape {
    up_bytes_per_s: Option<usize>,
    one_way: Duration,
}

const FAST: Shape = Shape {
    up_bytes_per_s: None,
    one_way: Duration::ZERO,
};

struct Link {
    address: ServerAddress,
    shape: Arc<Mutex<Shape>>,
}

impl Link {
    fn start(relay: &Relay) -> Self {
        let listener = {
            let _inside = relay.runtime.enter();
            let socket = tokio::net::TcpSocket::new_v4().unwrap();
            socket.set_recv_buffer_size(16 * 1024).unwrap();
            socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
            let listener = socket.listen(16).unwrap().into_std().unwrap();
            listener.set_nonblocking(false).unwrap();
            listener
        };
        let address = ServerAddress::parse(&format!(
            "127.0.0.1:{}",
            listener.local_addr().unwrap().port()
        ))
        .unwrap();
        let shape = Arc::new(Mutex::new(FAST));
        let port = relay.port();
        let shared = shape.clone();
        std::thread::spawn(move || accept(&listener, port, &shared));
        Self { address, shape }
    }

    fn set(&self, shape: Shape) {
        *self.shape.lock().unwrap() = shape;
    }
}

fn accept(listener: &TcpListener, relay_port: u16, shape: &Arc<Mutex<Shape>>) {
    for client in listener.incoming() {
        let Ok(client) = client else {
            return;
        };
        let Ok(relay) = TcpStream::connect(("127.0.0.1", relay_port)) else {
            continue;
        };
        let _ = client.set_nodelay(true);
        let _ = relay.set_nodelay(true);
        let (Ok(client_in), Ok(relay_in)) = (client.try_clone(), relay.try_clone()) else {
            continue;
        };
        pipe(client_in, relay, shape.clone(), true);
        pipe(relay_in, client, shape.clone(), false);
    }
}

fn pipe(mut from: TcpStream, mut to: TcpStream, shape: Arc<Mutex<Shape>>, up: bool) {
    let (tx, rx) = channel::<(Instant, Vec<u8>)>();
    std::thread::spawn(move || {
        for (due, bytes) in rx {
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
            if to.write_all(&bytes).is_err() {
                break;
            }
        }
        let _ = to.shutdown(Shutdown::Write);
    });
    std::thread::spawn(move || {
        let tick = Duration::from_millis(10);
        let mut buf = vec![0; 64 * 1024];
        loop {
            let shape = *shape.lock().unwrap();
            let rate = shape.up_bytes_per_s.filter(|_| up);
            let room = rate.map_or(buf.len(), |r| (r / 100).clamp(1, buf.len()));
            let started = Instant::now();
            match from.read(&mut buf[..room]) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx
                        .send((Instant::now() + shape.one_way, buf[..n].to_vec()))
                        .is_err()
                    {
                        break;
                    }
                }
            }
            if rate.is_some() {
                let spent = started.elapsed();
                if spent < tick {
                    std::thread::sleep(tick - spent);
                }
            }
        }
    });
}

#[derive(Default, Clone, Copy)]
struct Clock {
    send: Duration,
    worst_send: Duration,
    sent: usize,
    poll: Duration,
}

struct Timed {
    inner: Box<dyn Transport>,
    clock: Rc<RefCell<Clock>>,
}

impl Transport for Timed {
    fn send(&mut self, bytes: Vec<u8>) {
        let len = bytes.len();
        let started = Instant::now();
        self.inner.send(bytes);
        let spent = started.elapsed();
        let mut clock = self.clock.borrow_mut();
        clock.send += spent;
        clock.worst_send = clock.worst_send.max(spent);
        clock.sent += len;
    }

    fn poll(&mut self) -> Option<TransportEvent> {
        let started = Instant::now();
        let event = self.inner.poll();
        self.clock.borrow_mut().poll += started.elapsed();
        event
    }

    fn arrived(&self) -> Option<Instant> {
        self.inner.arrived()
    }

    fn clock(&self) -> Option<Instant> {
        self.inner.clock()
    }

    fn relay_key(&self) -> Option<RelayKey> {
        self.inner.relay_key()
    }

    fn side_line(&self) -> Option<SideLine> {
        self.inner.side_line()
    }
}

fn timed(clock: &Rc<RefCell<Clock>>) -> Connector {
    let mut real = transport::websocket();
    let clock = clock.clone();
    Box::new(move |url| {
        Box::new(Timed {
            inner: real(url),
            clock: clock.clone(),
        })
    })
}

#[derive(Default)]
struct Frames {
    count: usize,
    over_100: usize,
    over_50: usize,
    worst: Duration,
    worst_send: Duration,
    send_total: Duration,
    worst_local: Duration,
    sent: usize,
    missed: Option<String>,
}

impl Frames {
    fn line(&self, name: &str) -> String {
        format!(
            "LAG {name}: missed={:?} frames={} over50={} over100={} worst_ms={} worst_send_ms={} send_total_ms={} worst_local_ms={} sent_kb={}",
            self.missed,
            self.count,
            self.over_50,
            self.over_100,
            self.worst.as_millis(),
            self.worst_send.as_millis(),
            self.send_total.as_millis(),
            self.worst_local.as_millis(),
            self.sent / 1024
        )
    }
}

struct Device {
    online: Online,
    store: SaveStore,
    clock: Rc<RefCell<Clock>>,
    _threads: Threads,
    _dir: tempfile::TempDir,
}

impl Device {
    fn new(settings: &OnlineSettings) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SaveStore::in_dir(dir.path().to_path_buf());
        let clock = Rc::new(RefCell::new(Clock::default()));
        let threads = Threads::new(Mode::Threads);
        let mut online = Online::new(timed(&clock));
        online.cloud_attach(store.clone());
        online.cloud_spawner(threads.spawner());
        online.set_cloud(settings.cloud_owner(), true);
        Self {
            online,
            store,
            clock,
            _threads: threads,
            _dir: dir,
        }
    }

    fn frame(&mut self, frames: &mut Frames) {
        *self.clock.borrow_mut() = Clock::default();
        let started = Instant::now();
        self.online.update(Instant::now());
        let spent = started.elapsed();
        let clock = *self.clock.borrow();
        frames.count += 1;
        frames.worst = frames.worst.max(spent);
        frames.over_50 += usize::from(spent > Duration::from_millis(50));
        frames.over_100 += usize::from(spent > Duration::from_millis(100));
        frames.worst_send = frames.worst_send.max(clock.worst_send);
        frames.send_total += clock.send;
        frames.sent += clock.sent;
        frames.worst_local = frames
            .worst_local
            .max(spent.saturating_sub(clock.send + clock.poll));
        if let Some(rest) = FRAME.checked_sub(spent) {
            std::thread::sleep(rest);
        }
    }

    fn run_until(&mut self, what: &str, limit: Duration, done: impl Fn(&Device) -> bool) -> Frames {
        let mut frames = Frames::default();
        let until = Instant::now() + limit;
        loop {
            self.frame(&mut frames);
            if done(self) {
                return frames;
            }
            if Instant::now() >= until {
                frames.missed = Some(what.to_owned());
                return frames;
            }
        }
    }

    fn run_for(&mut self, limit: Duration) -> Frames {
        let mut frames = Frames::default();
        let until = Instant::now() + limit;
        while Instant::now() < until {
            self.frame(&mut frames);
        }
        frames
    }

    fn sign_in(&mut self, relay: JoinedRelay) -> Frames {
        self.online.sign_in(relay, Instant::now());
        self.run_until("signing in", Duration::from_secs(60), |d| {
            d.online.is_connected()
        })
    }

    fn synced(&self, saves: &[LocalSave]) -> bool {
        saves
            .iter()
            .all(|s| self.online.cloud_state(ROM, s) == Some(SyncState::Synced))
    }
}

fn account(relay: &Relay) -> OnlineSettings {
    let mut online = Online::default();
    online.probe(
        relay.address.clone(),
        PASSWORD.into(),
        Trust::default(),
        Instant::now(),
    );
    let limit = Instant::now() + Duration::from_secs(20);
    while online.phase() != Phase::Probed {
        online.update(Instant::now());
        assert!(Instant::now() < limit, "no server info");
        std::thread::sleep(Duration::from_millis(5));
    }
    online.create_account("Clara".into(), Instant::now());
    while !online.is_connected() {
        online.update(Instant::now());
        assert!(Instant::now() < limit, "no welcome");
        std::thread::sleep(Duration::from_millis(5));
    }
    let joined = online.take_joined().unwrap();
    let mut settings = OnlineSettings::default();
    settings.remember(
        &joined.address,
        &joined.relay_id,
        &joined.password,
        &joined.account,
        &joined.trust,
    );
    online.leave();
    settings
}

fn through(settings: &OnlineSettings, link: &Link) -> JoinedRelay {
    let mut joined = settings.joined_relay().unwrap();
    joined.address = link.address.clone();
    joined
}

fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u8
        })
        .collect()
}

fn picture() -> Vec<u8> {
    vec![90; 160 * 144 * 4]
}

const STATE: usize = 3 << 20;

const SLOW: Shape = Shape {
    up_bytes_per_s: Some(1 << 20),
    one_way: Duration::from_millis(50),
};

const LATE: Shape = Shape {
    up_bytes_per_s: Some(512 << 10),
    one_way: Duration::from_millis(1000),
};

fn scenario(name: &str, shape: Shape) -> Vec<String> {
    let relay = Relay::start();
    let settings = &account(&relay);
    let link = Link::start(&relay);
    let mut lines = Vec::new();
    let mut device = Device::new(settings);
    device
        .store
        .store_snapshot(ROM, 0, &noise(STATE, 1), &picture(), ConsoleKind::Gbc)
        .unwrap();
    let auto = device
        .store
        .store_autosave(ROM, &noise(STATE, 2), &picture(), ConsoleKind::Gbc, 5)
        .unwrap();
    link.set(shape);
    let connect = device.sign_in(through(settings, &link));
    lines.push(connect.line(&format!("{name} connect")));
    device.online.cloud_game(Some((ROM.into(), TITLE.into())));
    device.online.cloud_hold(ROM);
    let sync = device.run_until("the first sync", Duration::from_secs(120), |d| {
        d.synced(&[LocalSave::Snapshot(0), LocalSave::Autosave(auto)])
    });
    lines.push(sync.line(&format!("{name} first sync")));
    lines.push(format!(
        "LAG {name} states: {:?} {:?} phase {:?}",
        device.online.cloud_state(ROM, &LocalSave::Snapshot(0)),
        device.online.cloud_state(ROM, &LocalSave::Autosave(auto)),
        device.online.phase()
    ));
    lines.push(
        device
            .run_for(Duration::from_secs(5))
            .line(&format!("{name} idle")),
    );
    device
        .store
        .store_snapshot(ROM, 1, &noise(STATE, 3), &picture(), ConsoleKind::Gbc)
        .unwrap();
    let play = device.run_until("the new snapshot to sync", Duration::from_secs(120), |d| {
        d.synced(&[LocalSave::Snapshot(1)])
    });
    lines.push(play.line(&format!("{name} save while playing")));
    let mut other = Device::new(settings);
    let connect = other.sign_in(through(settings, &link));
    lines.push(connect.line(&format!("{name} second device connect")));
    other.online.cloud_game(Some((ROM.into(), TITLE.into())));
    other.online.cloud_hold(ROM);
    let fetch = other.run_until("the download", Duration::from_secs(120), |d| {
        d.synced(&[LocalSave::Snapshot(0), LocalSave::Snapshot(1)])
    });
    lines.push(fetch.line(&format!("{name} download")));
    other.online.leave();
    other.online.update(Instant::now());
    device.online.leave();
    device.online.update(Instant::now());
    lines
}

#[test]
#[ignore = "prints frame times for a report, takes about a minute"]
fn lag_numbers() {
    let relay = Relay::start();
    let settings = account(&relay);
    let mut lines = Vec::new();
    let mut off = Device::new(&settings);
    lines.push(off.run_for(Duration::from_secs(3)).line("relay off"));
    lines.extend(scenario("fast", FAST));
    lines.extend(scenario("slow 1MB/s 50ms", SLOW));
    lines.extend(scenario("late 512KB/s 1s", LATE));
    for line in lines {
        println!("{line}");
    }
}

#[test]
fn a_relay_that_answers_two_seconds_late_holds_no_frame() {
    let relay = Relay::start();
    let settings = account(&relay);
    let link = Link::start(&relay);
    let mut device = Device::new(&settings);
    let connect = device.sign_in(through(&settings, &link));
    assert!(connect.missed.is_none(), "{}", connect.line("connect"));
    device.online.cloud_game(Some((ROM.into(), TITLE.into())));
    link.set(LATE);
    device
        .store
        .store_snapshot(ROM, 0, &noise(STATE, 1), &picture(), ConsoleKind::Gbc)
        .unwrap();
    device.online.cloud_hold(ROM);
    let sync = device.run_until("the snapshot to sync", Duration::from_secs(120), |d| {
        d.synced(&[LocalSave::Snapshot(0)])
    });
    let line = sync.line("late upload");
    assert!(sync.missed.is_none(), "{line}");
    assert!(sync.worst < Duration::from_millis(50), "{line}");
}

fn sparse(len: usize, seed: u64) -> Vec<u8> {
    let mut bytes = noise(len, seed);
    for (at, byte) in bytes.iter_mut().enumerate() {
        if at % 8 != 0 {
            *byte = 0;
        }
    }
    bytes
}

fn timed_ms<T>(work: impl FnOnce() -> T) -> (T, f64) {
    let started = Instant::now();
    let out = work();
    (out, started.elapsed().as_secs_f64() * 1000.0)
}

#[test]
#[ignore = "prints what each upload step costs for a report"]
fn cost_numbers() {
    let keys = RelayId::random().keys();
    for len in [64 << 10, 512 << 10, 4 << 20, 12 << 20] {
        for (kind, state) in [("noise", noise(len, 7)), ("sparse", sparse(len, 7))] {
            let dir = tempfile::tempdir().unwrap();
            let store = SaveStore::in_dir(dir.path().to_path_buf());
            for slot in 0..2 {
                store
                    .store_snapshot(ROM, slot, &state, &picture(), ConsoleKind::Gbc)
                    .unwrap();
            }
            let (items, read) = timed_ms(|| store.local_items(ROM, "lag"));
            let payload = &items[0].payload;
            let (_, hash) = timed_ms(|| payload.hash());
            let (sealed, seal) = timed_ms(|| keys.seal_save(payload));
            let (opened, open) = timed_ms(|| keys.open_save(&sealed));
            assert!(opened.is_ok());
            println!(
                "COST {} KB {kind}: local_items({} saves)={read:.1}ms hash={hash:.1}ms seal_save={seal:.1}ms open_save={open:.1}ms sealed_kb={}",
                len >> 10,
                items.len(),
                sealed.len() >> 10
            );
        }
    }
}
