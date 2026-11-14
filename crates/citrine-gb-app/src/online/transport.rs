use citrine_gb_net::{ClientHandshake, Opener, RelayKey, Sealer};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
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
    fn side_line(&self) -> Option<SideLine> {
        None
    }
}

pub type SideLine = Arc<dyn Fn(Vec<u8>) + Send + Sync>;

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
    Open(Opener),
    Broken,
}

pub struct Secure<T> {
    inner: T,
    stage: Stage,
    sealer: Option<Arc<Mutex<Sealer>>>,
    key: Option<RelayKey>,
}

impl<T: Transport> Secure<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            stage: Stage::Waiting,
            sealer: None,
            key: None,
        }
    }
}

impl<T: Transport> Transport for Secure<T> {
    fn send(&mut self, bytes: Vec<u8>) {
        if let Some(sealer) = &self.sealer
            && let Ok(mut sealer) = sealer.lock()
        {
            let sealed = sealer.seal(&bytes);
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
                            let (sealer, opener) = channel.split();
                            self.key = Some(key);
                            self.sealer = Some(Arc::new(Mutex::new(sealer)));
                            self.stage = Stage::Open(opener);
                            return Some(TransportEvent::Opened);
                        }
                        Err(err) => return Some(TransportEvent::Failed(err.to_string())),
                    }
                }
                (Stage::Open(mut opener), TransportEvent::Message(sealed)) => {
                    let opened = opener.open(&sealed);
                    self.stage = Stage::Open(opener);
                    return Some(match opened {
                        Ok(plain) => TransportEvent::Message(plain),
                        Err(err) => {
                            self.stage = Stage::Broken;
                            self.sealer = None;
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

    fn side_line(&self) -> Option<SideLine> {
        let sealer = Arc::downgrade(self.sealer.as_ref()?);
        let raw = self.inner.side_line()?;
        Some(Arc::new(move |plain: Vec<u8>| {
            if let Some(sealer) = sealer.upgrade()
                && let Ok(mut sealer) = sealer.lock()
            {
                raw(sealer.seal(&plain));
            }
        }))
    }
}

struct WebSocket {
    sender: Option<Arc<Mutex<citrine_gb_websocket::Sender>>>,
    events: Option<Receiver<(Instant, citrine_gb_websocket::Event)>>,
    failed: Option<String>,
    arrived: Option<Instant>,
}

impl WebSocket {
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            clippy::arc_with_non_send_sync,
            reason = "the web has one thread and no side line"
        )
    )]
    fn open(url: &str) -> Self {
        let (tx, rx) = channel();
        let on_event = move |event| {
            let _ = tx.send((Instant::now(), event));
        };
        match citrine_gb_websocket::connect(url, citrine_gb_net::MAX_FRAME_BYTES + 1024, on_event) {
            Ok(sender) => Self {
                sender: Some(Arc::new(Mutex::new(sender))),
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
        if let Some(sender) = &self.sender
            && let Ok(mut sender) = sender.lock()
        {
            sender.send(citrine_gb_websocket::Message::Binary(bytes));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn side_line(&self) -> Option<SideLine> {
        let sender = Arc::downgrade(self.sender.as_ref()?);
        Some(Arc::new(move |bytes: Vec<u8>| {
            if let Some(sender) = sender.upgrade()
                && let Ok(mut sender) = sender.lock()
            {
                sender.send(citrine_gb_websocket::Message::Binary(bytes));
            }
        }))
    }

    fn poll(&mut self) -> Option<TransportEvent> {
        if let Some(err) = self.failed.take() {
            return Some(TransportEvent::Failed(err));
        }
        loop {
            let (at, event) = self.events.as_ref()?.try_recv().ok()?;
            self.arrived = Some(at);
            return Some(match event {
                citrine_gb_websocket::Event::Opened => TransportEvent::Opened,
                citrine_gb_websocket::Event::Message(citrine_gb_websocket::Message::Binary(
                    bytes,
                )) => TransportEvent::Message(bytes),
                citrine_gb_websocket::Event::Message(_) => continue,
                citrine_gb_websocket::Event::Failed(err) => TransportEvent::Failed(err),
                citrine_gb_websocket::Event::Closed => TransportEvent::Closed,
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
