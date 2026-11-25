use crate::App;
use crate::connections::Refusal;
use crate::limit::{Attempts, Bucket};
use crate::settings::Check;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use citrine_gb_net::Notice;
use citrine_gb_surface::pairing::Code;
use citrine_gb_surface::relay::{
    ACCOUNT_DOORS_PER_ADDRESS, BYTES_PER_SEC, Control, DOORS_PER_ADDRESS, DoorKey,
    MAX_MESSAGE_BYTES, WRONG_CODES_PER_MINUTE,
};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;

const QUEUED_FRAMES: usize = 16;
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const FIRST_MESSAGE_TIMEOUT: Duration = Duration::from_secs(10);

enum ToHost {
    Joined(Pipe),
    Gone,
}

struct Pipe {
    to: mpsc::Sender<Vec<u8>>,
    from: mpsc::Receiver<Vec<u8>>,
}

fn pipes() -> (Pipe, Pipe) {
    let (to_host, from_surface) = mpsc::channel(QUEUED_FRAMES);
    let (to_surface, from_host) = mpsc::channel(QUEUED_FRAMES);
    (
        Pipe {
            to: to_surface,
            from: from_surface,
        },
        Pipe {
            to: to_host,
            from: from_host,
        },
    )
}

async fn heard(from: &mut Option<mpsc::Receiver<Vec<u8>>>) -> Option<Vec<u8>> {
    match from {
        Some(from) => from.recv().await,
        None => std::future::pending().await,
    }
}

async fn room(to: Option<mpsc::Sender<Vec<u8>>>) -> Option<mpsc::OwnedPermit<Vec<u8>>> {
    to?.reserve_owned().await.ok()
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Code(Code),
    Account(DoorKey),
}

impl Key {
    fn per_address(self) -> usize {
        match self {
            Self::Code(_) => DOORS_PER_ADDRESS,
            Self::Account(_) => ACCOUNT_DOORS_PER_ADDRESS,
        }
    }

    fn same_kind(self, other: Key) -> bool {
        matches!(
            (self, other),
            (Self::Code(_), Self::Code(_)) | (Self::Account(_), Self::Account(_))
        )
    }
}

struct Door {
    ip: IpAddr,
    host: mpsc::UnboundedSender<ToHost>,
    joined: bool,
    id: u64,
}

pub struct Doors {
    map: Mutex<HashMap<Key, Door>>,
    next: Mutex<u64>,
    tries: Attempts,
}

impl Default for Doors {
    fn default() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            next: Mutex::new(0),
            tries: Attempts::new(WRONG_CODES_PER_MINUTE, Duration::from_secs(60)),
        }
    }
}

impl Doors {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<Key, Door>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn open(
        &self,
        code: Key,
        ip: IpAddr,
        host: mpsc::UnboundedSender<ToHost>,
    ) -> Result<u64, Control> {
        let mut map = self.map();
        let mine = map
            .iter()
            .filter(|(key, d)| d.ip == ip && key.same_kind(code))
            .count();
        if mine >= code.per_address() {
            return Err(Control::Limit);
        }
        if map.contains_key(&code) {
            return Err(Control::Taken);
        }
        let id = {
            let mut next = self.next.lock().unwrap_or_else(|e| e.into_inner());
            *next += 1;
            *next
        };
        map.insert(
            code,
            Door {
                ip,
                host,
                joined: false,
                id,
            },
        );
        Ok(id)
    }

    fn close(&self, code: Key, id: u64) {
        let mut map = self.map();
        if map.get(&code).is_some_and(|d| d.id == id) {
            map.remove(&code);
        }
    }

    fn join(&self, code: Key) -> Result<(Pipe, u64), Control> {
        let mut map = self.map();
        let door = map.get_mut(&code).ok_or(Control::NoCode)?;
        if door.joined {
            return Err(Control::Busy);
        }
        let (host, surface) = pipes();
        if door.host.send(ToHost::Joined(host)).is_err() {
            return Err(Control::NoCode);
        }
        door.joined = true;
        Ok((surface, door.id))
    }

    fn leave(&self, code: Key, id: u64) {
        if let Some(door) = self.map().get_mut(&code).filter(|d| d.id == id) {
            door.joined = false;
            let _ = door.host.send(ToHost::Gone);
        }
    }

    fn look(&self, keys: &[DoorKey]) -> Vec<DoorKey> {
        let map = self.map();
        keys.iter()
            .copied()
            .filter(|key| map.get(&Key::Account(*key)).is_some_and(|d| !d.joined))
            .collect()
    }
}

pub async fn upgrade(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    let ip = app.client_ip(&headers, addr);
    let settings = app.settings.get();
    if settings.kill || !settings.relay {
        return (StatusCode::SERVICE_UNAVAILABLE, "The relay is off.").into_response();
    }
    let slot = match app.connections.admit(ip) {
        Ok(slot) => slot,
        Err(Refusal::Blocked) => {
            return (
                StatusCode::FORBIDDEN,
                "This relay doesn't take connections from your address right now.",
            )
                .into_response();
        }
        Err(Refusal::Full | Refusal::Address) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                "Too many connections. Try again later.",
            )
                .into_response();
        }
    };
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| async move {
            let mut gate = app.hub.gate();
            app.clone().tasks.spawn(async move {
                let shutting_down = async move {
                    while !matches!(
                        gate.recv().await,
                        Ok(Notice::ShuttingDown) | Err(RecvError::Closed)
                    ) {}
                };
                tokio::select! {
                    () = run(app, socket, ip) => {}
                    () = shutting_down => {}
                }
                drop(slot);
            });
        })
}

async fn put(socket: &mut WebSocket, message: Message) -> bool {
    matches!(
        tokio::time::timeout(SEND_TIMEOUT, socket.send(message)).await,
        Ok(Ok(()))
    )
}

async fn say(socket: &mut WebSocket, control: Control) -> bool {
    put(socket, Message::Text(control.text().into())).await
}

async fn first_message(socket: &mut WebSocket) -> Option<Option<Control>> {
    loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => return Some(Control::parse(&text)),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            _ => return None,
        }
    }
}

async fn run(app: Arc<App>, mut socket: WebSocket, ip: IpAddr) {
    let Ok(Some(first)) =
        tokio::time::timeout(FIRST_MESSAGE_TIMEOUT, first_message(&mut socket)).await
    else {
        return;
    };
    match first {
        Some(Control::Open { code, password }) => {
            host(app, socket, ip, Key::Code(code), password).await;
        }
        Some(Control::Account { key, password }) => {
            host(app, socket, ip, Key::Account(key), password).await;
        }
        Some(Control::Join(code)) => surface(app, socket, ip, Key::Code(code)).await,
        Some(Control::Dial(key)) => surface(app, socket, ip, Key::Account(key)).await,
        Some(Control::Look(keys)) => {
            let here = app.doors.look(&keys);
            say(&mut socket, Control::Here(here)).await;
        }
        _ => {}
    }
}

async fn host(
    app: Arc<App>,
    mut socket: WebSocket,
    ip: IpAddr,
    code: Key,
    password: Option<String>,
) {
    let tries = &app.wrong_tries;
    if tries.blocked(ip) {
        say(&mut socket, Control::Limit).await;
        return;
    }
    match app.settings.check_password(password).await {
        Check::Right => {}
        Check::Wrong | Check::Busy => {
            tries.fail(ip);
            say(&mut socket, Control::Limit).await;
            return;
        }
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    let id = match app.doors.open(code, ip, tx) {
        Ok(id) => id,
        Err(refusal) => {
            say(&mut socket, refusal).await;
            return;
        }
    };
    if !say(&mut socket, Control::Opened).await {
        app.doors.close(code, id);
        return;
    }
    let mut to_surface: Option<mpsc::Sender<Vec<u8>>> = None;
    let mut from_surface: Option<mpsc::Receiver<Vec<u8>>> = None;
    let mut held: Option<Vec<u8>> = None;
    let mut budget = Bucket::new(BYTES_PER_SEC, 2.0 * BYTES_PER_SEC);
    loop {
        let pause = budget.wait(MAX_MESSAGE_BYTES as f64);
        tokio::select! {
            () = tokio::time::sleep(pause), if held.is_none() && !pause.is_zero() => {}
            frame = socket.recv(), if held.is_none() && pause.is_zero() => match frame {
                Some(Ok(Message::Binary(bytes))) => {
                    budget.take(bytes.len() as f64);
                    if to_surface.is_some() {
                        held = Some(bytes.to_vec());
                    }
                }
                Some(Ok(Message::Text(_) | Message::Ping(_) | Message::Pong(_))) => {}
                _ => break,
            },
            permit = room(to_surface.clone()), if held.is_some() => {
                if let (Some(permit), Some(bytes)) = (permit, held.take()) {
                    permit.send(bytes);
                }
            },
            event = rx.recv() => match event {
                Some(ToHost::Joined(pipe)) => {
                    to_surface = Some(pipe.to);
                    from_surface = Some(pipe.from);
                    if !say(&mut socket, Control::Joined).await {
                        break;
                    }
                }
                Some(ToHost::Gone) => {
                    (to_surface, from_surface, held) = (None, None, None);
                    if !say(&mut socket, Control::Gone).await {
                        break;
                    }
                }
                None => break,
            },
            data = heard(&mut from_surface) => match data {
                Some(bytes) => {
                    if !put(&mut socket, Message::Binary(bytes.into())).await {
                        break;
                    }
                }
                None => from_surface = None,
            },
        }
    }
    app.doors.close(code, id);
}

async fn surface(app: Arc<App>, mut socket: WebSocket, ip: IpAddr, code: Key) {
    let tries = &app.doors.tries;
    if tries.blocked(ip) {
        say(&mut socket, Control::Limit).await;
        return;
    }
    let (mut host, id) = match app.doors.join(code) {
        Ok(joined) => joined,
        Err(refusal) => {
            if refusal == Control::NoCode {
                tries.fail(ip);
            }
            say(&mut socket, refusal).await;
            return;
        }
    };
    let mut held: Option<Vec<u8>> = None;
    let mut budget = Bucket::new(BYTES_PER_SEC, 2.0 * BYTES_PER_SEC);
    if say(&mut socket, Control::Joined).await {
        loop {
            let pause = budget.wait(MAX_MESSAGE_BYTES as f64);
            tokio::select! {
                () = tokio::time::sleep(pause), if held.is_none() && !pause.is_zero() => {}
                frame = socket.recv(), if held.is_none() && pause.is_zero() => match frame {
                    Some(Ok(Message::Binary(bytes))) => {
                        budget.take(bytes.len() as f64);
                        held = Some(bytes.to_vec());
                    }
                    Some(Ok(Message::Text(_) | Message::Ping(_) | Message::Pong(_))) => {}
                    _ => break,
                },
                permit = host.to.reserve(), if held.is_some() => {
                    if let (Ok(permit), Some(bytes)) = (permit, held.take()) {
                        permit.send(bytes);
                    }
                },
                data = host.from.recv() => match data {
                    Some(bytes) => {
                        if !put(&mut socket, Message::Binary(bytes.into())).await {
                            break;
                        }
                    }
                    None => {
                        say(&mut socket, Control::Gone).await;
                        break;
                    }
                },
            }
        }
    }
    app.doors.leave(code, id);
}
