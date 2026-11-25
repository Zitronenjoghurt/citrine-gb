use citrine_gb_net::{PeerHello, peer_hello};
use citrine_gb_surface::link::{Link, Poll, Route, Tcp};
use std::net::IpAddr;

const SORT_SECS: f64 = 5.0;
const MAX_ARRIVING: usize = 32;
const MAX_ARRIVING_PER_ADDRESS: usize = 4;

#[cfg_attr(not(feature = "nds"), allow(dead_code))]
pub struct PeerArrival {
    pub hello: PeerHello,
    pub first: Vec<u8>,
    pub link: Tcp,
}

pub(super) enum Sorted {
    Screen(Box<dyn Link>),
    Peer(PeerArrival),
}

#[derive(Default)]
pub(super) struct Doorway {
    arriving: Vec<(Tcp, f64)>,
}

impl Doorway {
    pub fn arrive(&mut self, link: Tcp, now: f64) {
        let address = link.address();
        let same = self
            .arriving
            .iter()
            .filter(|(other, _)| address.is_some() && other.address() == address)
            .count();
        if self.arriving.len() >= MAX_ARRIVING || same >= MAX_ARRIVING_PER_ADDRESS {
            return;
        }
        self.arriving.push((link, now));
    }

    pub fn sort(&mut self, now: f64) -> Vec<Sorted> {
        let mut sorted = Vec::new();
        let mut waiting = Vec::new();
        for (mut link, at) in self.arriving.drain(..) {
            match link.poll() {
                Poll::Message(first) => sorted.push(sort(first, link)),
                Poll::Idle if now - at < SORT_SECS => waiting.push((link, at)),
                Poll::Idle => sorted.push(Sorted::Screen(Box::new(link))),
                Poll::Closed => {}
            }
        }
        self.arriving = waiting;
        sorted
    }
}

fn sort(first: Vec<u8>, link: Tcp) -> Sorted {
    match peer_hello(&first) {
        Some(hello) => Sorted::Peer(PeerArrival { hello, first, link }),
        None => Sorted::Screen(Box::new(Replayed {
            first: Some(first),
            link,
        })),
    }
}

struct Replayed {
    first: Option<Vec<u8>>,
    link: Tcp,
}

impl Link for Replayed {
    fn send(&mut self, bytes: Vec<u8>) {
        self.link.send(bytes);
    }

    fn poll(&mut self) -> Poll {
        match self.first.take() {
            Some(first) => Poll::Message(first),
            None => self.link.poll(),
        }
    }

    fn route(&self) -> Route {
        self.link.route()
    }

    fn address(&self) -> Option<IpAddr> {
        self.link.address()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    #[test]
    fn one_address_cant_crowd_the_doorway() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut doorway = Doorway::default();
        let mut far = Vec::new();
        for _ in 0..10 {
            far.push(TcpStream::connect(address).unwrap());
            let (near, _) = listener.accept().unwrap();
            doorway.arrive(Tcp::new(near).unwrap(), 0.0);
        }
        assert_eq!(doorway.arriving.len(), MAX_ARRIVING_PER_ADDRESS);
    }
}
