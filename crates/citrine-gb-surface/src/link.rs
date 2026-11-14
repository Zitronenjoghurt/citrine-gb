use std::net::IpAddr;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

#[derive(Debug, PartialEq, Eq)]
pub enum Poll {
    Message(Vec<u8>),
    Idle,
    Closed,
}

pub trait Link {
    fn send(&mut self, bytes: Vec<u8>);
    fn poll(&mut self) -> Poll;
    fn route(&self) -> Route;

    fn address(&self) -> Option<IpAddr> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Route {
    InApp,
    Network,
    Relay,
}

impl Route {
    pub fn label(self) -> &'static str {
        match self {
            Self::InApp => "On this device",
            Self::Network => "Same network",
            Self::Relay => "Over the internet",
        }
    }
}

pub struct InApp {
    to: Option<Sender<Vec<u8>>>,
    from: Receiver<Vec<u8>>,
    route: Route,
}

pub fn in_app() -> (InApp, InApp) {
    in_app_as(Route::InApp)
}

pub fn in_app_as(route: Route) -> (InApp, InApp) {
    let (a_to, b_from) = channel();
    let (b_to, a_from) = channel();
    (
        InApp {
            to: Some(a_to),
            from: a_from,
            route,
        },
        InApp {
            to: Some(b_to),
            from: b_from,
            route,
        },
    )
}

impl InApp {
    pub fn close(&mut self) {
        self.to = None;
    }
}

impl Link for InApp {
    fn send(&mut self, bytes: Vec<u8>) {
        if let Some(to) = &self.to
            && to.send(bytes).is_err()
        {
            self.to = None;
        }
    }

    fn poll(&mut self) -> Poll {
        if self.to.is_none() {
            return Poll::Closed;
        }
        match self.from.try_recv() {
            Ok(bytes) => Poll::Message(bytes),
            Err(TryRecvError::Empty) => Poll::Idle,
            Err(TryRecvError::Disconnected) => Poll::Closed,
        }
    }

    fn route(&self) -> Route {
        self.route
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use tcp::Tcp;

#[cfg(not(target_arch = "wasm32"))]
mod tcp {
    use super::{Link, Poll, Route};
    use std::io::{ErrorKind, Read, Write};
    use std::net::{IpAddr, SocketAddr, TcpStream};
    use std::time::Duration;

    pub const MAX_MESSAGE: usize = 1 << 20;

    pub struct Tcp {
        stream: Option<TcpStream>,
        inbox: Vec<u8>,
        outbox: Vec<u8>,
    }

    impl Tcp {
        pub fn new(stream: TcpStream) -> std::io::Result<Self> {
            stream.set_nodelay(true)?;
            stream.set_nonblocking(true)?;
            Ok(Self {
                stream: Some(stream),
                inbox: Vec::new(),
                outbox: Vec::new(),
            })
        }

        pub fn dial(addr: SocketAddr, wait: Duration) -> std::io::Result<Self> {
            Self::new(TcpStream::connect_timeout(&addr, wait)?)
        }

        pub fn peer(&self) -> Option<SocketAddr> {
            self.stream.as_ref().and_then(|s| s.peer_addr().ok())
        }

        fn flush(&mut self) {
            let Some(stream) = &mut self.stream else {
                return;
            };
            while !self.outbox.is_empty() {
                match stream.write(&self.outbox) {
                    Ok(0) => {
                        self.stream = None;
                        return;
                    }
                    Ok(n) => {
                        self.outbox.drain(..n);
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => {
                        self.stream = None;
                        return;
                    }
                }
            }
        }

        fn fill(&mut self) {
            let Some(stream) = &mut self.stream else {
                return;
            };
            let mut buf = [0; 16 * 1024];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => {
                        self.stream = None;
                        return;
                    }
                    Ok(n) => self.inbox.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => {
                        self.stream = None;
                        return;
                    }
                }
            }
        }

        fn next(&mut self) -> Option<Result<Vec<u8>, ()>> {
            let head: [u8; 4] = self.inbox.get(..4)?.try_into().ok()?;
            let size = u32::from_le_bytes(head) as usize;
            if size > MAX_MESSAGE {
                return Some(Err(()));
            }
            if self.inbox.len() < 4 + size {
                return None;
            }
            let message = self.inbox[4..4 + size].to_vec();
            self.inbox.drain(..4 + size);
            Some(Ok(message))
        }
    }

    impl Link for Tcp {
        fn send(&mut self, bytes: Vec<u8>) {
            let Ok(size) = u32::try_from(bytes.len()) else {
                return;
            };
            self.outbox.extend_from_slice(&size.to_le_bytes());
            self.outbox.extend_from_slice(&bytes);
            self.flush();
        }

        fn poll(&mut self) -> Poll {
            self.flush();
            if let Some(next) = self.next() {
                return match next {
                    Ok(message) => Poll::Message(message),
                    Err(()) => {
                        self.stream = None;
                        Poll::Closed
                    }
                };
            }
            self.fill();
            match self.next() {
                Some(Ok(message)) => Poll::Message(message),
                Some(Err(())) => {
                    self.stream = None;
                    Poll::Closed
                }
                None if self.stream.is_none() => Poll::Closed,
                None => Poll::Idle,
            }
        }

        fn route(&self) -> Route {
            Route::Network
        }

        fn address(&self) -> Option<IpAddr> {
            self.peer().map(|addr| addr.ip())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(link: &mut dyn Link) -> Poll {
        for _ in 0..2000 {
            match link.poll() {
                Poll::Idle => std::thread::sleep(std::time::Duration::from_millis(1)),
                other => return other,
            }
        }
        Poll::Idle
    }

    #[test]
    fn in_app_links_carry_both_ways_and_close() {
        let (mut a, mut b) = in_app();
        a.send(vec![1, 2]);
        b.send(vec![3]);
        assert_eq!(b.poll(), Poll::Message(vec![1, 2]));
        assert_eq!(a.poll(), Poll::Message(vec![3]));
        assert_eq!(a.poll(), Poll::Idle);
        drop(b);
        assert_eq!(a.poll(), Poll::Closed);
    }

    #[test]
    fn tcp_frames_messages_of_any_size() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut dialed = Tcp::dial(addr, std::time::Duration::from_secs(2)).unwrap();
        let mut accepted = Tcp::new(listener.accept().unwrap().0).unwrap();
        let big = vec![7; 300_000];
        dialed.send(big.clone());
        dialed.send(vec![]);
        dialed.send(vec![1]);
        for want in [big, vec![], vec![1]] {
            loop {
                dialed.poll();
                match accepted.poll() {
                    Poll::Message(got) => {
                        assert_eq!(got, want);
                        break;
                    }
                    Poll::Idle => std::thread::sleep(std::time::Duration::from_millis(1)),
                    Poll::Closed => panic!("closed"),
                }
            }
        }
        accepted.send(vec![9, 9]);
        assert_eq!(wait(&mut dialed), Poll::Message(vec![9, 9]));
        drop(accepted);
        assert_eq!(wait(&mut dialed), Poll::Closed);
    }

    #[test]
    fn tcp_refuses_an_oversized_frame() {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut raw = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let mut accepted = Tcp::new(listener.accept().unwrap().0).unwrap();
        raw.write_all(&u32::MAX.to_le_bytes()).unwrap();
        assert_eq!(wait(&mut accepted), Poll::Closed);
    }
}
