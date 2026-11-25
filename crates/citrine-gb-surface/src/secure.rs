use crate::link::{Link, Poll, Route};
use citrine_gb_net::Channel;
use std::net::IpAddr;

pub struct Sealed {
    link: Option<Box<dyn Link>>,
    channel: Channel,
    route: Route,
    address: Option<IpAddr>,
}

impl Sealed {
    pub(crate) fn new(link: Box<dyn Link>, channel: Channel) -> Self {
        Self {
            route: link.route(),
            address: link.address(),
            link: Some(link),
            channel,
        }
    }
}

impl Link for Sealed {
    fn send(&mut self, bytes: Vec<u8>) {
        if let Some(link) = &mut self.link {
            link.send(self.channel.seal(&bytes));
        }
    }

    fn poll(&mut self) -> Poll {
        let Some(link) = &mut self.link else {
            return Poll::Closed;
        };
        match link.poll() {
            Poll::Message(sealed) => match self.channel.open(&sealed) {
                Ok(plain) => Poll::Message(plain),
                Err(_) => {
                    self.link = None;
                    Poll::Closed
                }
            },
            other => other,
        }
    }

    fn route(&self) -> Route {
        self.route
    }

    fn address(&self) -> Option<IpAddr> {
        self.address
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{InApp, in_app};
    use citrine_gb_net::{KeyedDial, keyed_answer};

    fn sealed() -> (Sealed, InApp, Channel) {
        let (near, far) = in_app();
        let (dial, first) = KeyedDial::start(b"t").unwrap();
        let (reply, answer) = keyed_answer(b"t", &[1; 32], &first).unwrap();
        let dial = dial.finish(&[1; 32], &reply).unwrap();
        (Sealed::new(Box::new(near), dial), far, answer)
    }

    #[test]
    fn sealed_links_carry_plain_messages_both_ways() {
        let (mut sealed, mut far, mut answer) = sealed();
        sealed.send(vec![1, 2, 3]);
        let Poll::Message(bytes) = far.poll() else {
            panic!("a sealed message");
        };
        assert_ne!(bytes, vec![1, 2, 3]);
        assert_eq!(answer.open(&bytes).unwrap(), vec![1, 2, 3]);
        far.send(answer.seal(&[]));
        assert_eq!(sealed.poll(), Poll::Message(vec![]));
        assert_eq!(sealed.poll(), Poll::Idle);
    }

    #[test]
    fn a_plain_replayed_or_foreign_message_closes_the_link() {
        let (mut sealed, mut far, mut foreign) = sealed();
        far.send(vec![3, 15]);
        assert_eq!(sealed.poll(), Poll::Closed);
        sealed.send(vec![1]);
        assert!(!matches!(far.poll(), Poll::Message(_)));

        let (mut sealed, mut far, mut answer) = self::sealed();
        let once = answer.seal(&[4]);
        far.send(once.clone());
        far.send(once);
        assert_eq!(sealed.poll(), Poll::Message(vec![4]));
        assert_eq!(sealed.poll(), Poll::Closed);

        let (mut sealed, mut far, _) = self::sealed();
        far.send(foreign.seal(&[5]));
        assert_eq!(sealed.poll(), Poll::Closed);
    }
}
