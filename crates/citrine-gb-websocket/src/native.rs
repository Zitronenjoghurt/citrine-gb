#![expect(
    clippy::disallowed_types,
    clippy::disallowed_methods,
    reason = "the reader and writer threads block on the network and share one lock on the socket, which the caller's thread never takes"
)]

use crate::{Event, Message};
use std::io::ErrorKind;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tungstenite::WebSocket;
use tungstenite::client::{IntoClientRequest, uri_mode};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::stream::{MaybeTlsStream, Mode};

const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;
type Handler = Box<dyn Fn(Event) + Send + Sync>;

enum State {
    Connecting,
    Open(Box<Socket>),
    Closed,
}

enum Out {
    Message(Message),
    Close,
}

struct Shared {
    state: Mutex<State>,
    on_event: Handler,
    dropped: AtomicBool,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn dropped(&self) -> bool {
        self.dropped.load(Ordering::Acquire)
    }

    fn tell(&self, event: Event) {
        if !self.dropped() {
            (self.on_event)(event);
        }
    }
}

pub struct Sender {
    outbox: mpsc::Sender<Out>,
    shared: Arc<Shared>,
}

impl Sender {
    pub fn send(&mut self, message: Message) {
        let _ = self.outbox.send(Out::Message(message));
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        self.shared.dropped.store(true, Ordering::Release);
        let _ = self.outbox.send(Out::Close);
    }
}

pub fn connect(
    url: &str,
    max_frame_bytes: usize,
    on_event: impl Fn(Event) + Send + Sync + 'static,
) -> Result<Sender, String> {
    let shared = Arc::new(Shared {
        state: Mutex::new(State::Connecting),
        on_event: Box::new(on_event),
        dropped: AtomicBool::new(false),
    });
    let (outbox, queued) = mpsc::channel();
    let reader = shared.clone();
    let url = url.to_owned();
    spawn("citrine-websocket", move || {
        run(&url, max_frame_bytes, &reader, queued);
    })?;
    Ok(Sender { outbox, shared })
}

fn spawn(name: &str, body: impl FnOnce() + Send + 'static) -> Result<(), String> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)
        .map(drop)
        .map_err(|err| format!("Failed to spawn thread: {err}"))
}

fn run(url: &str, max_frame_bytes: usize, shared: &Arc<Shared>, queued: Receiver<Out>) {
    let (mut socket, wake) = match open(url, max_frame_bytes) {
        Ok(opened) => opened,
        Err(err) => {
            shared.tell(Event::Failed(err));
            return;
        }
    };
    {
        let mut state = shared.state();
        if shared.dropped() {
            close(&mut socket);
            *state = State::Closed;
            return;
        }
        *state = State::Open(Box::new(socket));
    }
    shared.tell(Event::Opened);
    let writer = shared.clone();
    if let Err(err) = spawn("citrine-websocket-writer", move || write(&writer, &queued)) {
        end(shared, Event::Failed(err));
        return;
    }
    let mut peeked = Ok(1);
    loop {
        let mut state = shared.state();
        let State::Open(socket) = &mut *state else {
            return;
        };
        let ended = match (drain(socket, &shared.on_event), peeked) {
            (Err(event), _) => event,
            (Ok(()), Ok(0)) => Event::Closed,
            (Ok(()), Err(err)) => Event::Failed(format!("read: {err}")),
            (Ok(()), Ok(_)) => {
                drop(state);
                peeked = peek(&wake);
                continue;
            }
        };
        close(socket);
        *state = State::Closed;
        drop(state);
        shared.tell(ended);
        return;
    }
}

fn write(shared: &Shared, queued: &Receiver<Out>) {
    for out in queued {
        let mut state = shared.state();
        let State::Open(socket) = &mut *state else {
            return;
        };
        let Out::Message(message) = out else {
            close(socket);
            *state = State::Closed;
            return;
        };
        if let Err(err) = socket.send(frame(message)) {
            close(socket);
            *state = State::Closed;
            drop(state);
            shared.tell(Event::Failed(format!("send: {err}")));
            return;
        }
    }
}

fn end(shared: &Shared, event: Event) {
    let mut state = shared.state();
    if let State::Open(socket) = &mut *state {
        close(socket);
    }
    *state = State::Closed;
    drop(state);
    shared.tell(event);
}

fn peek(wake: &TcpStream) -> std::io::Result<usize> {
    loop {
        match wake.peek(&mut [0]) {
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            peeked => return peeked,
        }
    }
}

fn open(url: &str, max_frame_bytes: usize) -> Result<(Socket, TcpStream), String> {
    let failed = |err: &dyn std::fmt::Display| format!("Connect: {err}");
    let request = url.into_client_request().map_err(|err| failed(&err))?;
    let uri = request.uri();
    let port = uri
        .port_u16()
        .unwrap_or(match uri_mode(uri).map_err(|err| failed(&err))? {
            Mode::Plain => 80,
            Mode::Tls => 443,
        });
    let host = uri
        .host()
        .ok_or_else(|| failed(&"no host"))?
        .trim_start_matches('[')
        .trim_end_matches(']');
    let tcp = TcpStream::connect((host, port)).map_err(|err| failed(&err))?;
    tcp.set_nodelay(true).map_err(|err| failed(&err))?;
    tcp.set_write_timeout(Some(WRITE_TIMEOUT))
        .map_err(|err| failed(&err))?;
    let wake = tcp.try_clone().map_err(|err| failed(&err))?;
    let config = WebSocketConfig {
        max_frame_size: Some(max_frame_bytes),
        max_message_size: Some(max_frame_bytes),
        ..Default::default()
    };
    let (socket, _) = tungstenite::client_tls_with_config(request, tcp, Some(config), None)
        .map_err(|err| failed(&err))?;
    Ok((socket, wake))
}

fn drain(socket: &mut Socket, on_event: &Handler) -> Result<(), Event> {
    let tcp = tcp_of(socket).ok_or_else(|| Event::Failed("read: unknown stream".to_owned()))?;
    tcp.set_nonblocking(true)
        .map_err(|err| Event::Failed(format!("read: {err}")))?;
    let drained = loop {
        match socket.read() {
            Ok(tungstenite::Message::Binary(bytes)) => {
                on_event(Event::Message(Message::Binary(bytes)));
            }
            Ok(tungstenite::Message::Text(text)) => on_event(Event::Message(Message::Text(text))),
            Ok(tungstenite::Message::Close(_)) => break Err(Event::Closed),
            Ok(_) => {}
            Err(tungstenite::Error::Io(err)) if err.kind() == ErrorKind::WouldBlock => break Ok(()),
            Err(err) => break Err(Event::Failed(format!("read: {err}"))),
        }
    };
    if let Some(tcp) = tcp_of(socket) {
        tcp.set_nonblocking(false)
            .map_err(|err| Event::Failed(format!("read: {err}")))?;
    }
    drained
}

fn close(socket: &mut Socket) {
    let _ = socket.close(None);
    let _ = socket.flush();
    if let Some(tcp) = tcp_of(socket) {
        let _ = tcp.shutdown(Shutdown::Both);
    }
}

fn tcp_of(socket: &Socket) -> Option<&TcpStream> {
    match socket.get_ref() {
        MaybeTlsStream::Plain(tcp) => Some(tcp),
        MaybeTlsStream::Rustls(tls) => Some(tls.get_ref()),
        _ => None,
    }
}

fn frame(message: Message) -> tungstenite::Message {
    match message {
        Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
        Message::Text(text) => tungstenite::Message::Text(text),
    }
}
