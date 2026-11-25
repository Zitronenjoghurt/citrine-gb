use crate::online::Online;
use citrine_gb_nds_net::server_list::Host;
use citrine_gb_nds_net::services::GtsLink;
use citrine_gb_net::{ClientMsg, ErrorCode, GtsReply, NetAddr, ServerMsg};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};

type Answer = (u64, Option<Vec<u8>>);
type Claimed = (u64, Option<bool>);
type Hosting = Option<(String, u16, Vec<(String, String)>)>;
type Listed = (u64, Option<Vec<Host>>);
type Message = (SocketAddrV4, Vec<u8>);
type Heard = (Ipv4Addr, u16, Vec<u8>);
type Side = (u32, u8, u16);
type Natnegged = (u32, SocketAddrV4);

const RELAYED_KEPT: usize = 64;

pub struct GtsPort {
    calls: Receiver<(u64, String)>,
    answers: Sender<Answer>,
    waiting: Vec<u64>,
    claims: Receiver<(u64, u32)>,
    claimed: Sender<Claimed>,
    claiming: Vec<u64>,
    hosting: Receiver<Hosting>,
    lists: Receiver<(u64, String)>,
    listed: Sender<Listed>,
    listing: Vec<u64>,
    messages: Receiver<Message>,
    heard: SyncSender<Heard>,
    sides: Receiver<Side>,
    natnegged: SyncSender<Natnegged>,
}

pub struct RelayLink {
    calls: Sender<(u64, String)>,
    answers: Receiver<Answer>,
    claims: Sender<(u64, u32)>,
    claimed: Receiver<Claimed>,
    hosting: Sender<Hosting>,
    lists: Sender<(u64, String)>,
    listed: Receiver<Listed>,
    messages: Sender<Message>,
    heard: Receiver<Heard>,
    sides: Sender<Side>,
    natnegged: Receiver<Natnegged>,
}

pub fn pair() -> (GtsPort, RelayLink) {
    let (call_tx, call_rx) = channel();
    let (answer_tx, answer_rx) = channel();
    let (claim_tx, claim_rx) = channel();
    let (claimed_tx, claimed_rx) = channel();
    let (hosting_tx, hosting_rx) = channel();
    let (list_tx, list_rx) = channel();
    let (listed_tx, listed_rx) = channel();
    let (message_tx, message_rx) = channel();
    let (heard_tx, heard_rx) = sync_channel(RELAYED_KEPT);
    let (side_tx, side_rx) = channel();
    let (natnegged_tx, natnegged_rx) = sync_channel(RELAYED_KEPT);
    (
        GtsPort {
            calls: call_rx,
            answers: answer_tx,
            waiting: Vec::new(),
            claims: claim_rx,
            claimed: claimed_tx,
            claiming: Vec::new(),
            hosting: hosting_rx,
            lists: list_rx,
            listed: listed_tx,
            listing: Vec::new(),
            messages: message_rx,
            heard: heard_tx,
            sides: side_rx,
            natnegged: natnegged_tx,
        },
        RelayLink {
            calls: call_tx,
            answers: answer_rx,
            claims: claim_tx,
            claimed: claimed_rx,
            hosting: hosting_tx,
            lists: list_tx,
            listed: listed_rx,
            messages: message_tx,
            heard: heard_rx,
            sides: side_tx,
            natnegged: natnegged_rx,
        },
    )
}

impl GtsLink for RelayLink {
    fn call(&mut self, id: u64, target: &str) {
        self.calls.send((id, target.to_owned())).ok();
    }

    fn answer(&mut self) -> Option<Answer> {
        self.answers.try_recv().ok()
    }

    fn claim(&mut self, id: u64, profile: u32) {
        self.claims.send((id, profile)).ok();
    }

    fn claimed(&mut self) -> Option<Claimed> {
        self.claimed.try_recv().ok()
    }

    fn host(&mut self, game: &str, port: u16, keys: &[(String, String)]) {
        self.hosting
            .send(Some((game.to_owned(), port, keys.to_vec())))
            .ok();
    }

    fn unhost(&mut self) {
        self.hosting.send(None).ok();
    }

    fn list(&mut self, id: u64, game: &str) {
        self.lists.send((id, game.to_owned())).ok();
    }

    fn listed(&mut self) -> Option<Listed> {
        self.listed.try_recv().ok()
    }

    fn message(&mut self, to: SocketAddrV4, data: &[u8]) {
        self.messages.send((to, data.to_vec())).ok();
    }

    fn messages(&mut self) -> Option<Heard> {
        self.heard.try_recv().ok()
    }

    fn natneg(&mut self, cookie: u32, index: u8, port: u16) {
        self.sides.send((cookie, index, port)).ok();
    }

    fn natnegged(&mut self) -> Option<Natnegged> {
        self.natnegged.try_recv().ok()
    }
}

impl GtsPort {
    pub fn pump(&mut self, online: &mut Online) {
        let mut refused = false;
        let mut unread = false;
        for msg in online.inbox() {
            match msg {
                ServerMsg::Gts { call, reply } => {
                    if let Some(at) = self.waiting.iter().position(|id| id == call) {
                        self.waiting.swap_remove(at);
                        let body = match reply {
                            GtsReply::Body(body) => Some(body.clone()),
                            GtsReply::Status(_) => None,
                        };
                        self.answers.send((*call, body)).ok();
                    }
                }
                ServerMsg::DsProfile { call, ours } => {
                    if let Some(at) = self.claiming.iter().position(|id| id == call) {
                        self.claiming.swap_remove(at);
                        self.claimed.send((*call, Some(*ours))).ok();
                    }
                }
                ServerMsg::DsList { call, hosts } => {
                    if let Some(at) = self.listing.iter().position(|id| id == call) {
                        self.listing.swap_remove(at);
                        let hosts = hosts
                            .iter()
                            .map(|host| Host::listed(host.addr.socket(), host.keys.clone()))
                            .collect();
                        self.listed.send((*call, Some(hosts))).ok();
                    }
                }
                ServerMsg::DsMessage { from, port, data } => {
                    self.heard
                        .try_send((Ipv4Addr::from(*from), *port, data.clone()))
                        .ok();
                }
                ServerMsg::DsNatneg { cookie, peer } => {
                    self.natnegged.try_send((*cookie, peer.socket())).ok();
                }
                ServerMsg::Error(ErrorCode::GtsOff) => refused = true,
                ServerMsg::Error(
                    ErrorCode::NotSignedIn | ErrorCode::BadMessage | ErrorCode::Server,
                ) => unread = true,
                _ => {}
            }
        }
        if unread || !online.is_connected() {
            for id in self.claiming.drain(..) {
                self.claimed.send((id, None)).ok();
            }
            for id in self.listing.drain(..) {
                self.listed.send((id, None)).ok();
            }
        }
        if refused || !online.is_connected() {
            for id in self.waiting.drain(..) {
                self.answers.send((id, None)).ok();
            }
        }
        while let Ok((call, target)) = self.calls.try_recv() {
            if online.is_connected()
                && online.account().is_some()
                && online.send(ClientMsg::Gts { call, target })
            {
                self.waiting.push(call);
            } else {
                self.answers.send((call, None)).ok();
            }
        }
        while let Ok((call, profile)) = self.claims.try_recv() {
            if online.is_connected()
                && online.account().is_some()
                && online.send(ClientMsg::DsProfile { call, profile })
            {
                self.claiming.push(call);
            } else {
                self.claimed.send((call, None)).ok();
            }
        }
        let ready = online.is_connected() && online.account().is_some();
        while let Ok(hosting) = self.hosting.try_recv() {
            if ready {
                online.send(match hosting {
                    Some((game, port, keys)) => ClientMsg::DsHost { game, port, keys },
                    None => ClientMsg::DsUnhost,
                });
            }
        }
        while let Ok((call, game)) = self.lists.try_recv() {
            if ready && online.send(ClientMsg::DsList { call, game }) {
                self.listing.push(call);
            } else {
                self.listed.send((call, None)).ok();
            }
        }
        while let Ok((to, data)) = self.messages.try_recv() {
            if ready {
                let to = NetAddr {
                    ip: to.ip().octets(),
                    port: to.port(),
                };
                online.send(ClientMsg::DsMessage { to, data });
            }
        }
        while let Ok((cookie, index, port)) = self.sides.try_recv() {
            if ready {
                online.send(ClientMsg::DsNatneg {
                    cookie,
                    index,
                    port,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_without_a_relay_is_answered_at_once() {
        let (mut port, mut link) = pair();
        link.call(7, "/pokemondpds/worldexchange/info.asp?pid=1");
        port.pump(&mut Online::offline());
        assert_eq!(link.answer(), Some((7, None)));
        assert_eq!(link.answer(), None);
    }

    #[test]
    fn a_list_without_a_relay_is_answered_at_once_and_a_host_goes_nowhere() {
        let (mut port, mut link) = pair();
        link.host("mariokartds", 56400, &[("dwc_pid".into(), "1".into())]);
        link.list(9, "mariokartds");
        port.pump(&mut Online::offline());
        assert_eq!(link.listed(), Some((9, None)));
        assert_eq!(link.listed(), None);
    }

    #[test]
    fn a_message_without_a_relay_goes_nowhere_and_none_arrive() {
        let (mut port, mut link) = pair();
        let host = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 2), 56400);
        link.message(host, b"SBCM");
        port.pump(&mut Online::offline());
        assert_eq!(link.messages(), None);
        assert!(port.messages.try_recv().is_err());
        link.natneg(7, 1, 56400);
        port.pump(&mut Online::offline());
        assert_eq!(link.natnegged(), None);
        assert!(port.sides.try_recv().is_err());
    }

    #[test]
    fn relay_messages_and_natneg_peers_past_the_bound_are_dropped_until_the_ds_reads_them() {
        let (mut port, mut link) = pair();
        let mut online = Online::offline();
        let peer = NetAddr {
            ip: [10, 77, 1, 2],
            port: 50000,
        };
        for n in 0..1000u32 {
            let data = n.to_be_bytes().to_vec();
            crate::online::tests::deliver(
                &mut online,
                ServerMsg::DsMessage {
                    from: [10, 77, 1, 2],
                    port: 56400,
                    data,
                },
            );
            crate::online::tests::deliver(&mut online, ServerMsg::DsNatneg { cookie: n, peer });
        }
        port.pump(&mut online);
        let heard: Vec<Heard> = std::iter::from_fn(|| link.messages()).collect();
        assert_eq!(heard.len(), 64);
        assert_eq!(heard[63].2, 63u32.to_be_bytes());
        let peers: Vec<Natnegged> = std::iter::from_fn(|| link.natnegged()).collect();
        assert_eq!(peers.len(), 64);
        assert_eq!(peers[63].0, 63);

        let mut online = Online::offline();
        crate::online::tests::deliver(
            &mut online,
            ServerMsg::DsMessage {
                from: [10, 77, 1, 2],
                port: 56400,
                data: b"after".to_vec(),
            },
        );
        port.pump(&mut online);
        assert_eq!(
            link.messages().map(|heard| heard.2),
            Some(b"after".to_vec())
        );
    }

    #[test]
    fn a_claim_without_a_relay_is_answered_unbound_at_once() {
        let (mut port, mut link) = pair();
        link.claim(8, 123_456);
        port.pump(&mut Online::offline());
        assert_eq!(link.claimed(), Some((8, None)));
        assert_eq!(link.claimed(), None);
    }
}
