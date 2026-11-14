use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::mpsc::{self, Receiver, Sender};

pub const WINDOW_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetOut {
    Send {
        port: u16,
        to: SocketAddrV4,
        data: Vec<u8>,
    },
    Open {
        stream: u32,
        to: SocketAddrV4,
    },
    Write {
        stream: u32,
        data: Vec<u8>,
    },
    Close {
        stream: u32,
    },
    Lease,
    Taken {
        stream: u32,
        total: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetIn {
    Heard {
        port: u16,
        from: SocketAddrV4,
        data: Vec<u8>,
    },
    Opened {
        stream: u32,
        ok: bool,
    },
    Read {
        stream: u32,
        data: Vec<u8>,
    },
    Closed {
        stream: u32,
    },
    Leased {
        address: Option<Ipv4Addr>,
    },
    Written {
        stream: u32,
        total: u64,
    },
}

pub struct Relay {
    pub(crate) out: Sender<NetOut>,
    pub(crate) back: Receiver<NetIn>,
}

pub struct RelayEnd {
    pub out: Receiver<NetOut>,
    pub back: Sender<NetIn>,
}

pub fn relay() -> (Relay, RelayEnd) {
    let (out, taken) = mpsc::channel();
    let (back, heard) = mpsc::channel();
    (Relay { out, back: heard }, RelayEnd { out: taken, back })
}
