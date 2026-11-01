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
    Box::new(|url| Box::new(WebSocket::open(url)))
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
