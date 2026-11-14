use citrine_gb_surface::link::{InApp, Link, Poll, Route, in_app_as};
use citrine_gb_surface::pairing::Code;
use citrine_gb_surface::relay::{Control, DoorKey, MAX_MESSAGE_BYTES};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, channel};

struct Socket {
    sender: citrine_gb_websocket::Sender,
    events: Receiver<citrine_gb_websocket::Event>,
}

enum Heard {
    Opened,
    Control(Control),
    Data(Vec<u8>),
    Closed,
}

impl Socket {
    fn open(url: &str) -> Option<Self> {
        let (tx, rx) = channel();
        let on_event = move |event| {
            let _ = tx.send(event);
        };
        let sender = citrine_gb_websocket::connect(url, MAX_MESSAGE_BYTES + 1024, on_event).ok()?;
        Some(Self { sender, events: rx })
    }

    fn say(&mut self, control: &Control) {
        self.sender
            .send(citrine_gb_websocket::Message::Text(control.text()));
    }

    fn data(&mut self, bytes: Vec<u8>) {
        self.sender
            .send(citrine_gb_websocket::Message::Binary(bytes));
    }

    fn hear(&self) -> Option<Heard> {
        loop {
            let event = match self.events.try_recv() {
                Ok(event) => event,
                Err(std::sync::mpsc::TryRecvError::Empty) => return None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return Some(Heard::Closed),
            };
            return Some(match event {
                citrine_gb_websocket::Event::Opened => Heard::Opened,
                citrine_gb_websocket::Event::Message(citrine_gb_websocket::Message::Binary(
                    bytes,
                )) => Heard::Data(bytes),
                citrine_gb_websocket::Event::Message(citrine_gb_websocket::Message::Text(text)) => {
                    match Control::parse(&text) {
                        Some(control) => Heard::Control(control),
                        None => continue,
                    }
                }
                citrine_gb_websocket::Event::Failed(_) | citrine_gb_websocket::Event::Closed => {
                    Heard::Closed
                }
            });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorState {
    Opening,
    Open,
    Taken,
    Refused,
    Closed,
}

pub struct Door {
    code: Option<Code>,
    opening: Control,
    socket: Option<Socket>,
    state: DoorState,
    near: Option<InApp>,
    used: bool,
    idle_since: f64,
}

impl Door {
    pub fn open(url: &str, code: Code, password: Option<String>, now: f64) -> Self {
        Self::knock(url, Some(code), Control::Open { code, password }, now)
    }

    pub fn account(url: &str, key: DoorKey, password: Option<String>, now: f64) -> Self {
        Self::knock(url, None, Control::Account { key, password }, now)
    }

    fn knock(url: &str, code: Option<Code>, opening: Control, now: f64) -> Self {
        let socket = Socket::open(url);
        Self {
            code,
            opening,
            state: if socket.is_some() {
                DoorState::Opening
            } else {
                DoorState::Closed
            },
            socket,
            near: None,
            used: false,
            idle_since: now,
        }
    }

    pub fn code(&self) -> Option<Code> {
        self.code
    }

    pub fn state(&self) -> DoorState {
        self.state
    }

    pub fn used(&self) -> bool {
        self.used
    }

    pub fn idle_for(&self, now: f64) -> f64 {
        if self.near.is_some() {
            0.0
        } else {
            now - self.idle_since
        }
    }

    pub fn step(&mut self, now: f64) -> Option<InApp> {
        let mut joined = None;
        while let Some(heard) = self.socket.as_ref().and_then(Socket::hear) {
            match heard {
                Heard::Opened => {
                    if let Some(socket) = &mut self.socket {
                        socket.say(&self.opening);
                    }
                }
                Heard::Control(Control::Opened) => self.state = DoorState::Open,
                Heard::Control(Control::Taken) => {
                    self.state = DoorState::Taken;
                    self.socket = None;
                }
                Heard::Control(Control::Limit) => {
                    self.state = DoorState::Refused;
                    self.socket = None;
                }
                Heard::Control(Control::Joined) => {
                    let (near, far) = in_app_as(Route::Relay);
                    self.near = Some(near);
                    self.used = true;
                    joined = Some(far);
                }
                Heard::Control(Control::Gone) => {
                    self.near = None;
                    self.idle_since = now;
                }
                Heard::Control(_) => {}
                Heard::Data(bytes) => {
                    if let Some(near) = &mut self.near {
                        near.send(bytes);
                    }
                }
                Heard::Closed => {
                    self.state = DoorState::Closed;
                    self.socket = None;
                    self.near = None;
                    self.idle_since = now;
                }
            }
        }
        while let Some(near) = &mut self.near {
            match near.poll() {
                Poll::Message(bytes) => {
                    if let Some(socket) = &mut self.socket {
                        socket.data(bytes);
                    }
                }
                Poll::Idle => break,
                Poll::Closed => {
                    self.near = None;
                    self.idle_since = now;
                }
            }
        }
        joined
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NoCode,
    Busy,
    Limit,
}

pub struct Relayed {
    socket: Option<Socket>,
    knock: Control,
    joined: bool,
    pending: Vec<Vec<u8>>,
    refusal: Rc<Cell<Option<Refusal>>>,
}

impl Relayed {
    pub fn open(url: &str, code: Code) -> (Self, Rc<Cell<Option<Refusal>>>) {
        Self::knock(url, Control::Join(code))
    }

    pub fn dial(url: &str, key: DoorKey) -> (Self, Rc<Cell<Option<Refusal>>>) {
        Self::knock(url, Control::Dial(key))
    }

    fn knock(url: &str, knock: Control) -> (Self, Rc<Cell<Option<Refusal>>>) {
        let refusal = Rc::new(Cell::new(None));
        let link = Self {
            socket: Socket::open(url),
            knock,
            joined: false,
            pending: Vec::new(),
            refusal: refusal.clone(),
        };
        (link, refusal)
    }
}

impl Link for Relayed {
    fn send(&mut self, bytes: Vec<u8>) {
        match &mut self.socket {
            Some(socket) if self.joined => socket.data(bytes),
            Some(_) => self.pending.push(bytes),
            None => {}
        }
    }

    fn poll(&mut self) -> Poll {
        loop {
            let Some(socket) = &mut self.socket else {
                return Poll::Closed;
            };
            let Some(heard) = socket.hear() else {
                return Poll::Idle;
            };
            match heard {
                Heard::Opened => socket.say(&self.knock),
                Heard::Control(Control::Joined) => {
                    self.joined = true;
                    for bytes in std::mem::take(&mut self.pending) {
                        socket.data(bytes);
                    }
                }
                Heard::Control(refused @ (Control::NoCode | Control::Busy | Control::Limit)) => {
                    self.refusal.set(Some(match refused {
                        Control::NoCode => Refusal::NoCode,
                        Control::Busy => Refusal::Busy,
                        _ => Refusal::Limit,
                    }));
                    self.socket = None;
                }
                Heard::Control(Control::Gone) | Heard::Closed => self.socket = None,
                Heard::Control(_) => {}
                Heard::Data(bytes) => return Poll::Message(bytes),
            }
        }
    }

    fn route(&self) -> Route {
        Route::Relay
    }
}

pub struct Look {
    socket: Option<Socket>,
    keys: Vec<DoorKey>,
}

impl Look {
    pub fn start(url: &str, keys: Vec<DoorKey>) -> Self {
        Self {
            socket: Socket::open(url),
            keys,
        }
    }

    pub fn step(&mut self) -> Option<Option<Vec<DoorKey>>> {
        loop {
            let Some(socket) = &mut self.socket else {
                return Some(None);
            };
            match socket.hear()? {
                Heard::Opened => socket.say(&Control::Look(self.keys.clone())),
                Heard::Control(Control::Here(keys)) => {
                    self.socket = None;
                    return Some(Some(keys));
                }
                Heard::Closed => {
                    self.socket = None;
                    return Some(None);
                }
                Heard::Control(_) | Heard::Data(_) => {}
            }
        }
    }
}
