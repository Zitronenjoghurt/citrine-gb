use crate::online::Online;
use citrine_gb_nds_net::relay::{NetIn, NetOut, RelayEnd};
use citrine_gb_net::{ClientMsg, DS_NET_PROTOCOL, ErrorCode, NET_WINDOW_BYTES, ServerMsg};
use std::net::Ipv4Addr;

const _: () = assert!(citrine_gb_nds_net::relay::WINDOW_BYTES == NET_WINDOW_BYTES);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    SignedOut,
    RelayOff,
}

pub struct NetPort {
    end: RelayEnd,
    base: u32,
    top: u32,
    streams: Vec<(u32, bool)>,
    refused: Option<Refusal>,
    told: bool,
    lease: Lease,
    was_up: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lease {
    Unasked,
    Wanted,
    Asked,
    Held(Ipv4Addr),
}

impl NetPort {
    pub fn new(end: RelayEnd, base: u32) -> Self {
        Self {
            end,
            base,
            top: base,
            streams: Vec::new(),
            refused: None,
            told: false,
            lease: Lease::Unasked,
            was_up: false,
        }
    }

    pub fn retire(self) -> (u32, Vec<u32>) {
        let open = self.streams.into_iter().map(|(id, _)| id).collect();
        (self.top, open)
    }

    pub fn take_refusal(&mut self) -> Option<Refusal> {
        let refused = self.refused.filter(|_| !self.told);
        self.told |= refused.is_some();
        refused
    }

    pub fn pump(&mut self, online: &mut Online) {
        let mut off = false;
        for msg in online.inbox() {
            match msg {
                ServerMsg::NetHeard { port, from, data } => self.back(NetIn::Heard {
                    port: *port,
                    from: from.socket(),
                    data: data.clone(),
                }),
                ServerMsg::NetOpened { stream, ok } => {
                    match self.streams.iter_mut().find(|(id, _)| id == stream) {
                        Some(entry) if *ok => entry.1 = true,
                        Some(_) => self.forget(*stream),
                        None => continue,
                    }
                    self.back(NetIn::Opened {
                        stream: self.local(*stream),
                        ok: *ok,
                    });
                }
                ServerMsg::NetRead { stream, data } if self.tracks(*stream) => {
                    self.back(NetIn::Read {
                        stream: self.local(*stream),
                        data: data.clone(),
                    })
                }
                ServerMsg::NetWritten { stream, total } if self.tracks(*stream) => {
                    self.back(NetIn::Written {
                        stream: self.local(*stream),
                        total: *total,
                    })
                }
                ServerMsg::NetClosed { stream } if self.tracks(*stream) => {
                    self.forget(*stream);
                    self.back(NetIn::Closed {
                        stream: self.local(*stream),
                    });
                }
                ServerMsg::NetLeased { ip } if self.lease == Lease::Asked => {
                    let address = ip.map(Ipv4Addr::from);
                    self.lease = address.map_or(Lease::Unasked, Lease::Held);
                    self.back(NetIn::Leased { address });
                }
                ServerMsg::Error(ErrorCode::DsNetOff) => off = true,
                _ => {}
            }
        }
        let signed_out = (!online.is_connected() && !online.comes_back())
            || (online.is_connected() && online.account().is_none());
        let old = online
            .info()
            .is_some_and(|info| info.protocol < DS_NET_PROTOCOL);
        if off || old {
            self.refused = Some(Refusal::RelayOff);
        } else if signed_out && self.refused == Some(Refusal::RelayOff) {
            self.refused = None;
            self.told = false;
        }
        let refusal = if signed_out {
            Some(Refusal::SignedOut)
        } else {
            self.refused.filter(|refused| *refused == Refusal::RelayOff)
        };
        let down = refusal.is_some() || !online.is_connected();
        if down {
            for (stream, open) in std::mem::take(&mut self.streams) {
                let stream = self.local(stream);
                self.back(match open {
                    true => NetIn::Closed { stream },
                    false => NetIn::Opened { stream, ok: false },
                });
            }
        }
        if refusal.is_none() {
            self.refused = None;
            self.told = false;
        }
        while let Ok(out) = self.end.out.try_recv() {
            if out == NetOut::Lease {
                if refusal.is_some() {
                    self.back(NetIn::Leased { address: None });
                } else {
                    self.lease = Lease::Wanted;
                }
                continue;
            }
            if down {
                self.refuse(out, refusal);
                continue;
            }
            match out {
                NetOut::Send { port, to, data } => {
                    online.send(ClientMsg::NetSend {
                        port,
                        to: to.into(),
                        data,
                    });
                }
                NetOut::Open { stream, to } => {
                    let wire = self.wire(stream);
                    if online.send(ClientMsg::NetOpen {
                        stream: wire,
                        to: to.into(),
                    }) {
                        self.streams.push((wire, false));
                        self.top = wire.wrapping_add(1);
                    } else {
                        self.back(NetIn::Opened { stream, ok: false });
                    }
                }
                NetOut::Write { stream, data } => {
                    let wire = self.wire(stream);
                    if !self.tracks(wire) {
                        self.back(NetIn::Closed { stream });
                    } else if !online.send(ClientMsg::NetWrite { stream: wire, data }) {
                        self.forget(wire);
                        self.back(NetIn::Closed { stream });
                    }
                }
                NetOut::Close { stream } => {
                    let wire = self.wire(stream);
                    if self.tracks(wire) {
                        self.forget(wire);
                        online.send(ClientMsg::NetClose { stream: wire });
                    }
                }
                NetOut::Taken { stream, total } => {
                    let wire = self.wire(stream);
                    if self.tracks(wire) {
                        online.send(ClientMsg::NetTaken {
                            stream: wire,
                            total,
                        });
                    }
                }
                NetOut::Lease => {}
            }
        }
        let up = online.is_connected() && refusal.is_none();
        if refusal.is_some() && matches!(self.lease, Lease::Wanted | Lease::Asked) {
            self.lease = Lease::Unasked;
            self.back(NetIn::Leased { address: None });
        }
        if up {
            let want = match self.lease {
                Lease::Wanted => Some(None),
                Lease::Asked if !self.was_up => Some(None),
                Lease::Held(address) if !self.was_up => Some(Some(address.octets())),
                _ => None,
            };
            if let Some(want) = want
                && online.send(ClientMsg::NetLease { want })
                && self.lease == Lease::Wanted
            {
                self.lease = Lease::Asked;
            }
        }
        self.was_up = up;
    }

    fn refuse(&mut self, out: NetOut, refusal: Option<Refusal>) {
        match out {
            NetOut::Send { .. } | NetOut::Close { .. } | NetOut::Taken { .. } => return,
            NetOut::Open { stream, .. } => self.back(NetIn::Opened { stream, ok: false }),
            NetOut::Write { stream, .. } => self.back(NetIn::Closed { stream }),
            NetOut::Lease => return,
        }
        if refusal.is_some() {
            self.refused = refusal;
        }
    }

    fn wire(&self, stream: u32) -> u32 {
        self.base.wrapping_add(stream)
    }

    fn local(&self, wire: u32) -> u32 {
        wire.wrapping_sub(self.base)
    }

    fn tracks(&self, wire: u32) -> bool {
        self.streams.iter().any(|(id, _)| *id == wire)
    }

    fn forget(&mut self, stream: u32) {
        self.streams.retain(|(id, _)| *id != stream);
    }

    fn back(&self, heard: NetIn) {
        self.end.back.send(heard).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::tests::{deliver, sent, signed_in};
    use citrine_gb_net::RelayId;
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::sync::mpsc::{Receiver, Sender, channel};

    const FAR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 9), 28_910);

    fn port() -> (NetPort, Sender<NetOut>, Receiver<NetIn>) {
        let (out, taken) = channel();
        let (back, heard) = channel();
        (NetPort::new(RelayEnd { out: taken, back }, 0), out, heard)
    }

    #[test]
    fn a_stream_opened_while_signed_out_is_refused_and_told_once() {
        let (mut port, out, heard) = port();
        out.send(NetOut::Open { stream: 7, to: FAR }).unwrap();
        out.send(NetOut::Open { stream: 8, to: FAR }).unwrap();
        port.pump(&mut Online::offline());
        assert_eq!(
            heard.try_iter().collect::<Vec<_>>(),
            vec![
                NetIn::Opened {
                    stream: 7,
                    ok: false
                },
                NetIn::Opened {
                    stream: 8,
                    ok: false
                },
            ]
        );
        assert_eq!(port.take_refusal(), Some(Refusal::SignedOut));
        assert_eq!(port.take_refusal(), None);
    }

    #[test]
    fn a_datagram_while_signed_out_is_dropped_without_a_notice() {
        let (mut port, out, heard) = port();
        out.send(NetOut::Send {
            port: 40_000,
            to: FAR,
            data: b"hi".to_vec(),
        })
        .unwrap();
        port.pump(&mut Online::offline());
        assert_eq!(heard.try_iter().count(), 0);
        assert_eq!(port.take_refusal(), None);
    }

    #[test]
    fn a_lease_goes_to_the_server_and_its_answer_back_to_the_gateway() {
        let (mut online, wire, _) = signed_in(&RelayId::random());
        let (mut port, out, heard) = port();
        out.send(NetOut::Lease).unwrap();
        port.pump(&mut online);
        assert_eq!(sent(&wire), vec![ClientMsg::NetLease { want: None }]);
        deliver(
            &mut online,
            ServerMsg::NetLeased {
                ip: Some([10, 77, 1, 2]),
            },
        );
        port.pump(&mut online);
        assert_eq!(
            heard.try_iter().collect::<Vec<_>>(),
            vec![NetIn::Leased {
                address: Some(Ipv4Addr::new(10, 77, 1, 2))
            }]
        );
        port.pump(&mut online);
        assert_eq!(sent(&wire), vec![]);
    }

    #[test]
    fn a_lease_asked_while_signed_out_falls_back_at_once() {
        let (mut port, out, heard) = port();
        out.send(NetOut::Lease).unwrap();
        port.pump(&mut Online::offline());
        assert_eq!(
            heard.try_iter().collect::<Vec<_>>(),
            vec![NetIn::Leased { address: None }]
        );
        assert_eq!(port.take_refusal(), None);
    }

    #[test]
    fn a_rebuilt_port_opens_new_wire_ids_and_ignores_the_old_streams() {
        let (mut online, wire, _) = signed_in(&RelayId::random());
        let (mut first, out, _) = port();
        out.send(NetOut::Open { stream: 0, to: FAR }).unwrap();
        first.pump(&mut online);
        assert_eq!(
            sent(&wire),
            vec![ClientMsg::NetOpen {
                stream: 0,
                to: FAR.into()
            }]
        );
        let (next, open) = first.retire();
        assert_eq!((next, open), (1, vec![0]));
        let (out, taken) = channel();
        let (back, heard) = channel();
        let mut second = NetPort::new(RelayEnd { out: taken, back }, next);
        out.send(NetOut::Open { stream: 0, to: FAR }).unwrap();
        second.pump(&mut online);
        assert_eq!(
            sent(&wire),
            vec![ClientMsg::NetOpen {
                stream: 1,
                to: FAR.into()
            }]
        );
        deliver(
            &mut online,
            ServerMsg::NetRead {
                stream: 0,
                data: b"old".to_vec(),
            },
        );
        deliver(
            &mut online,
            ServerMsg::NetOpened {
                stream: 1,
                ok: true,
            },
        );
        second.pump(&mut online);
        assert_eq!(
            heard.try_iter().collect::<Vec<_>>(),
            vec![NetIn::Opened {
                stream: 0,
                ok: true
            }]
        );
    }
}
