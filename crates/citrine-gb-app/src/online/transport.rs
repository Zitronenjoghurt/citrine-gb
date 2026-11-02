use citrine_gb_net::{Channel, ClientHandshake, RelayKey};
use std::ops::ControlFlow;
use std::sync::mpsc::{Receiver, channel};
use web_time::Instant;

pub enum TransportEvent {
    Opened,
    Message(Vec<u8>),
    Closed,
    Failed(String),
}

pub trait Transport {
    fn send(&mut self, bytes: Vec<u8>);
    fn poll(&mut self) -> Option<TransportEvent>;
    fn arrived(&self) -> Option<Instant> {
        None
    }
    fn clock(&self) -> Option<Instant> {
        None
    }
    fn relay_key(&self) -> Option<RelayKey> {
        None
    }
}

pub type Connector = Box<dyn FnMut(&str) -> Box<dyn Transport>>;

pub struct Unplugged;

impl Transport for Unplugged {
    fn send(&mut self, _bytes: Vec<u8>) {}

    fn poll(&mut self) -> Option<TransportEvent> {
        Some(TransportEvent::Failed(String::new()))
    }
}

pub fn websocket() -> Connector {
    Box::new(|url| Box::new(Secure::new(WebSocket::open(url))))
}

enum Stage {
    Waiting,
    Shaking(Box<ClientHandshake>),
    Open(Channel),
    Broken,
}

pub struct Secure<T> {
    inner: T,
    stage: Stage,
    key: Option<RelayKey>,
}

impl<T: Transport> Secure<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            stage: Stage::Waiting,
            key: None,
        }
    }
}

impl<T: Transport> Transport for Secure<T> {
    fn send(&mut self, bytes: Vec<u8>) {
        if let Stage::Open(channel) = &mut self.stage {
            let sealed = channel.seal(&bytes);
            self.inner.send(sealed);
        }
    }

    fn poll(&mut self) -> Option<TransportEvent> {
        loop {
            let event = self.inner.poll()?;
            let stage = std::mem::replace(&mut self.stage, Stage::Broken);
            match (stage, event) {
                (Stage::Waiting, TransportEvent::Opened) => {
                    let (handshake, first) = ClientHandshake::start();
                    self.inner.send(first);
                    self.stage = Stage::Shaking(Box::new(handshake));
                }
                (Stage::Shaking(handshake), TransportEvent::Message(reply)) => {
                    match (*handshake).finish(&reply) {
                        Ok((key, channel)) => {
                            self.key = Some(key);
                            self.stage = Stage::Open(channel);
                            return Some(TransportEvent::Opened);
                        }
                        Err(err) => return Some(TransportEvent::Failed(err.to_string())),
                    }
                }
                (Stage::Open(mut channel), TransportEvent::Message(sealed)) => {
                    let opened = channel.open(&sealed);
                    self.stage = Stage::Open(channel);
                    return Some(match opened {
                        Ok(plain) => TransportEvent::Message(plain),
                        Err(err) => {
                            self.stage = Stage::Broken;
                            TransportEvent::Failed(err.to_string())
                        }
                    });
                }
                (_, TransportEvent::Closed) => return Some(TransportEvent::Closed),
                (_, TransportEvent::Failed(err)) => return Some(TransportEvent::Failed(err)),
                (Stage::Broken, _) => {}
                _ => return Some(TransportEvent::Failed("unexpected frame".into())),
            }
        }
    }

    fn arrived(&self) -> Option<Instant> {
        self.inner.arrived()
    }

    fn clock(&self) -> Option<Instant> {
        self.inner.clock()
    }

    fn relay_key(&self) -> Option<RelayKey> {
        self.key
    }
}

struct WebSocket {
    sender: Option<ewebsock::WsSender>,
    events: Option<Receiver<(Instant, ewebsock::WsEvent)>>,
    failed: Option<String>,
    arrived: Option<Instant>,
}

impl WebSocket {
    fn open(url: &str) -> Self {
        let options = ewebsock::Options {
            max_incoming_frame_size: citrine_gb_net::MAX_FRAME_BYTES + 1024,
            ..Default::default()
        };
        let (tx, rx) = channel();
        let on_event = Box::new(move |event| {
            let _ = tx.send((Instant::now(), event));
            ControlFlow::Continue(())
        });
        match ewebsock::ws_connect(url.to_owned(), options, on_event) {
            Ok(sender) => Self {
                sender: Some(sender),
                events: Some(rx),
                failed: None,
                arrived: None,
            },
            Err(err) => Self {
                sender: None,
                events: None,
                failed: Some(err),
                arrived: None,
            },
        }
    }
}

impl Transport for WebSocket {
    fn send(&mut self, bytes: Vec<u8>) {
        if let Some(sender) = &mut self.sender {
            sender.send(ewebsock::WsMessage::Binary(bytes));
        }
    }

    fn poll(&mut self) -> Option<TransportEvent> {
        if let Some(err) = self.failed.take() {
            return Some(TransportEvent::Failed(err));
        }
        loop {
            let (at, event) = self.events.as_ref()?.try_recv().ok()?;
            self.arrived = Some(at);
            return Some(match event {
                ewebsock::WsEvent::Opened => TransportEvent::Opened,
                ewebsock::WsEvent::Message(ewebsock::WsMessage::Binary(bytes)) => {
                    TransportEvent::Message(bytes)
                }
                ewebsock::WsEvent::Message(_) => continue,
                ewebsock::WsEvent::Error(err) => TransportEvent::Failed(err),
                ewebsock::WsEvent::Closed => TransportEvent::Closed,
            });
        }
    }

    fn arrived(&self) -> Option<Instant> {
        self.arrived
    }

    fn clock(&self) -> Option<Instant> {
        Some(Instant::now())
    }
}
