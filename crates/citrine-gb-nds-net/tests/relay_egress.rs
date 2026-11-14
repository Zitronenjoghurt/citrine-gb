mod common;

use std::cell::Cell;
use std::net::{Ipv4Addr, SocketAddrV4};

use citrine_gb_nds_net::gateway::{Config, Gateway, TcpPeer, Trace};
use citrine_gb_nds_net::relay::{self, NetIn, NetOut, RelayEnd, WINDOW_BYTES};
use citrine_gb_threads::Threads;
use common::Client;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::{tcp, udp};
use smoltcp::wire::{IpAddress, IpEndpoint};

const DS_PORT: u16 = 40_000;
const FAR: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 9);
const STEPS: usize = 2_000;

struct Rig {
    gateway: Gateway,
    client: Client,
    end: RelayEnd,
    taken: usize,
    _threads: Threads,
}

impl Rig {
    fn new() -> Self {
        let threads = Threads::default();
        let (relay, end) = relay::relay();
        let config = Config::default();
        let gateway = Gateway::new(config, threads.spawner()).with_relay(relay);
        Self {
            gateway,
            client: Client::new(config.address),
            end,
            taken: 0,
            _threads: threads,
        }
    }

    fn until<T>(&mut self, mut found: impl FnMut(&mut Self) -> Option<T>) -> Option<T> {
        for _ in 0..STEPS {
            self.client.step(&mut self.gateway);
            if let Some(value) = found(self) {
                return Some(value);
            }
        }
        None
    }

    fn sent(&mut self) -> Option<NetOut> {
        loop {
            match self.end.out.try_recv().ok()? {
                NetOut::Taken { total, .. } => self.taken = total as usize,
                out => return Some(out),
            }
        }
    }

    fn answer(&mut self, heard: NetIn) {
        self.end.back.send(heard).unwrap();
    }

    fn udp(&mut self) -> SocketHandle {
        let mut socket = udp::Socket::new(
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 4096]),
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 4096]),
        );
        socket.bind(DS_PORT).unwrap();
        self.client.sockets.add(socket)
    }

    fn heard(&mut self, ds: SocketHandle) -> Option<(Vec<u8>, IpEndpoint)> {
        self.client
            .sockets
            .get_mut::<udp::Socket>(ds)
            .recv()
            .ok()
            .map(|(data, meta)| (data.to_vec(), meta.endpoint))
    }

    fn connect(&mut self, port: u16) -> (SocketHandle, u32) {
        let socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; 4096]),
            tcp::SocketBuffer::new(vec![0; 4096]),
        );
        let ds = self.client.sockets.add(socket);
        let context = self.client.iface.context();
        self.client
            .sockets
            .get_mut::<tcp::Socket>(ds)
            .connect(context, IpEndpoint::new(IpAddress::Ipv4(FAR), port), 41_000)
            .unwrap();
        match self.until(Rig::sent) {
            Some(NetOut::Open { stream, to }) => {
                assert_eq!(to, SocketAddrV4::new(FAR, port));
                (ds, stream)
            }
            other => panic!("the relay should get an open, got {other:?}"),
        }
    }

    fn tcp(&mut self, ds: SocketHandle) -> &mut tcp::Socket<'static> {
        self.client.sockets.get_mut::<tcp::Socket>(ds)
    }
}

#[test]
fn a_relayed_ds_port_that_sent_out_hears_a_new_port_and_a_new_address() {
    let mut rig = Rig::new();
    let ds = rig.udp();
    let far = IpEndpoint::new(IpAddress::Ipv4(FAR), 27_900);
    rig.client
        .sockets
        .get_mut::<udp::Socket>(ds)
        .send_slice(b"hello", far)
        .unwrap();
    assert_eq!(
        rig.until(Rig::sent),
        Some(NetOut::Send {
            port: DS_PORT,
            to: SocketAddrV4::new(FAR, 27_900),
            data: b"hello".to_vec(),
        })
    );
    let new_address = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 7), 27_900);
    let new_port = SocketAddrV4::new(FAR, 27_901);
    for (from, data) in [
        (new_address, b"new address".to_vec()),
        (new_port, b"new port".to_vec()),
    ] {
        rig.answer(NetIn::Heard {
            port: DS_PORT,
            from,
            data,
        });
    }
    let mut heard = Vec::new();
    while heard.len() < 2 {
        heard.push(
            rig.until(|rig| rig.heard(ds))
                .expect("the ds hears every answer"),
        );
    }
    heard.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        heard,
        [
            (
                b"new address".to_vec(),
                IpEndpoint::new(IpAddress::Ipv4(*new_address.ip()), 27_900)
            ),
            (
                b"new port".to_vec(),
                IpEndpoint::new(IpAddress::Ipv4(FAR), 27_901)
            ),
        ]
    );
    let peers = rig.gateway.udp_peers();
    let tally = |at: SocketAddrV4| {
        peers
            .iter()
            .find(|peer| peer.peer == at)
            .map(|peer| (peer.ds_port, peer.up, peer.down))
    };
    assert_eq!(tally(SocketAddrV4::new(FAR, 27_900)), Some((DS_PORT, 1, 0)));
    assert_eq!(tally(new_address), Some((DS_PORT, 0, 1)));
    assert_eq!(tally(new_port), Some((DS_PORT, 0, 1)));
}

#[test]
fn a_relayed_stream_waits_for_the_open_then_carries_both_ways() {
    let mut rig = Rig::new();
    let (ds, stream) = rig.connect(29_900);
    rig.until(|rig| rig.tcp(ds).may_send().then_some(()))
        .expect("the ds connects");
    rig.tcp(ds).send_slice(b"\\login\\").unwrap();
    assert_eq!(rig.until(Rig::sent), None, "nothing goes before the open");
    rig.answer(NetIn::Opened { stream, ok: true });
    assert_eq!(
        rig.until(Rig::sent),
        Some(NetOut::Write {
            stream,
            data: b"\\login\\".to_vec(),
        })
    );
    rig.answer(NetIn::Read {
        stream,
        data: b"\\lc\\1".to_vec(),
    });
    let mut got = Vec::new();
    rig.until(|rig| {
        rig.tcp(ds)
            .recv(|data| {
                got.extend_from_slice(data);
                (data.len(), ())
            })
            .ok();
        (got.len() == 5).then_some(())
    })
    .expect("the ds reads the far end");
    assert_eq!(got, b"\\lc\\1");
    rig.answer(NetIn::Closed { stream });
    rig.until(|rig| (!rig.tcp(ds).may_recv()).then_some(()))
        .expect("the ds sees the far end close");
    rig.tcp(ds).close();
    rig.until(|rig| {
        matches!(
            rig.tcp(ds).state(),
            tcp::State::Closed | tcp::State::TimeWait
        )
        .then_some(())
    })
    .expect("the stream ends");
    assert_eq!(
        rig.until(Rig::sent),
        None,
        "a stream the relay closed needs no close"
    );
}

#[test]
fn a_stream_the_relay_could_not_open_resets_the_ds() {
    let mut rig = Rig::new();
    let (ds, stream) = rig.connect(29_900);
    rig.answer(NetIn::Opened { stream, ok: false });
    rig.until(|rig| (rig.tcp(ds).state() == tcp::State::Closed).then_some(()))
        .expect("the ds is reset");
}

#[test]
fn a_ds_that_hangs_up_closes_its_relayed_stream() {
    let mut rig = Rig::new();
    let (ds, stream) = rig.connect(80);
    rig.answer(NetIn::Opened { stream, ok: true });
    rig.until(|rig| rig.tcp(ds).may_send().then_some(()))
        .expect("the ds connects");
    rig.tcp(ds).abort();
    assert_eq!(rig.until(Rig::sent), Some(NetOut::Close { stream }));
}

#[test]
fn a_ds_that_reads_nothing_holds_the_relay_to_its_window() {
    let mut rig = Rig::new();
    let (ds, stream) = rig.connect(29_900);
    rig.answer(NetIn::Opened { stream, ok: true });
    rig.until(|rig| rig.tcp(ds).may_send().then_some(()))
        .expect("the ds connects");
    let piece = vec![0x5C; 16 * 1024];
    let mut read = 0;
    loop {
        while read + piece.len() <= rig.taken + WINDOW_BYTES {
            rig.answer(NetIn::Read {
                stream,
                data: piece.clone(),
            });
            read += piece.len();
        }
        let taken = rig.taken;
        match rig.until(|rig| {
            let out = rig.sent();
            (out.is_some() || rig.taken > taken).then_some(out)
        }) {
            None => break,
            Some(None) => {}
            Some(Some(other)) => panic!("a relay inside its window got {other:?}"),
        }
    }
    assert!(rig.taken >= WINDOW_BYTES, "took {}", rig.taken);
    assert!(rig.taken < 2 * WINDOW_BYTES, "took {}", rig.taken);
    for _ in 0..2 {
        rig.answer(NetIn::Read {
            stream,
            data: piece.clone(),
        });
    }
    assert_eq!(rig.until(Rig::sent), Some(NetOut::Close { stream }));
    rig.until(|rig| (rig.tcp(ds).state() == tcp::State::Closed).then_some(()))
        .expect("the ds is reset");
}

#[test]
fn a_ds_upload_waits_for_the_relay_to_write_its_window() {
    let mut rig = Rig::new();
    let (ds, stream) = rig.connect(29_900);
    rig.answer(NetIn::Opened { stream, ok: true });
    rig.until(|rig| rig.tcp(ds).may_send().then_some(()))
        .expect("the ds connects");
    let written = Cell::new(0);
    let upload = |rig: &mut Rig| {
        rig.tcp(ds).send_slice(&[0x5C; 4096]).ok();
        while let Some(out) = rig.sent() {
            match out {
                NetOut::Write { data, .. } => written.set(written.get() + data.len()),
                other => panic!("an upload sends only writes, got {other:?}"),
            }
        }
        None::<()>
    };
    rig.until(upload);
    rig.until(upload);
    assert_eq!(written.get(), WINDOW_BYTES);
    rig.answer(NetIn::Written {
        stream,
        total: WINDOW_BYTES as u64,
    });
    rig.until(upload);
    rig.until(upload);
    assert_eq!(written.get(), 2 * WINDOW_BYTES);
}

#[test]
fn a_traced_gateway_keeps_datagrams_both_ways_and_each_stream_with_its_bytes() {
    let mut rig = Rig::new();
    rig.gateway.trace();
    let ds = rig.udp();
    let far = SocketAddrV4::new(FAR, 27_900);
    rig.client
        .sockets
        .get_mut::<udp::Socket>(ds)
        .send_slice(b"\x03beat", IpEndpoint::new(IpAddress::Ipv4(FAR), 27_900))
        .unwrap();
    rig.until(Rig::sent).expect("the datagram goes out");
    rig.answer(NetIn::Heard {
        port: DS_PORT,
        from: far,
        data: b"\xfe\xfd\x0a".to_vec(),
    });
    rig.until(|rig| rig.heard(ds)).expect("the ds hears it");
    let (tcp, stream) = rig.connect(28_910);
    rig.answer(NetIn::Opened { stream, ok: true });
    rig.until(|rig| rig.tcp(tcp).may_send().then_some(()))
        .expect("the ds connects");
    rig.tcp(tcp).send_slice(b"list").unwrap();
    rig.until(Rig::sent).expect("the request goes out");
    rig.answer(NetIn::Read {
        stream,
        data: b"servers".to_vec(),
    });
    rig.until(|rig| {
        let peers = rig.gateway.tcp_peers();
        (peers.first().map(|peer| peer.down) == Some(7)).then_some(())
    })
    .expect("the ds gets the answer");
    let browser = SocketAddrV4::new(FAR, 28_910);
    let seen: Vec<_> = rig
        .gateway
        .tcp_peers()
        .into_iter()
        .map(|peer: TcpPeer| {
            (
                peer.ds_port,
                peer.to,
                peer.stream,
                peer.state,
                peer.up,
                peer.relayed,
                peer.down,
            )
        })
        .collect();
    assert_eq!(
        seen,
        [(
            41_000,
            browser,
            Some(stream),
            tcp::State::Established,
            4,
            7,
            7
        )]
    );
    rig.tcp(tcp).abort();
    rig.until(Rig::sent).expect("the stream closes");
    rig.until(|rig| rig.gateway.tcp_peers().is_empty().then_some(()))
        .expect("the flow ends");
    let (states, trace): (Vec<_>, Vec<_>) = rig
        .gateway
        .take_trace()
        .into_iter()
        .partition(|item| matches!(item, Trace::TcpState { .. }));
    let states: Vec<_> = states
        .into_iter()
        .filter_map(|item| match item {
            Trace::TcpState { state, .. } => Some(state),
            _ => None,
        })
        .collect();
    assert!(
        states.starts_with(&[tcp::State::SynReceived, tcp::State::Established]),
        "{states:?}"
    );
    assert_eq!(
        trace,
        [
            Trace::Datagram {
                ds_port: DS_PORT,
                peer: far,
                up: true,
                data: b"\x03beat".to_vec(),
            },
            Trace::Datagram {
                ds_port: DS_PORT,
                peer: far,
                up: false,
                data: b"\xfe\xfd\x0a".to_vec(),
            },
            Trace::TcpOpen {
                ds_port: 41_000,
                to: browser,
            },
            Trace::TcpOpened {
                ds_port: 41_000,
                to: browser,
                ok: true,
            },
            Trace::TcpClosed {
                ds_port: 41_000,
                to: browser,
                up: 4,
                relayed: 7,
                down: 7,
            },
        ]
    );
    assert!(rig.gateway.take_trace().is_empty());
}

#[test]
fn an_untraced_gateway_keeps_nothing() {
    let mut rig = Rig::new();
    let ds = rig.udp();
    rig.client
        .sockets
        .get_mut::<udp::Socket>(ds)
        .send_slice(b"beat", IpEndpoint::new(IpAddress::Ipv4(FAR), 27_900))
        .unwrap();
    rig.until(Rig::sent).expect("the datagram goes out");
    assert!(rig.gateway.take_trace().is_empty());
}

#[test]
fn a_ds_that_closes_its_side_closes_its_relayed_stream() {
    let mut rig = Rig::new();
    let (ds, stream) = rig.connect(28_910);
    rig.answer(NetIn::Opened { stream, ok: true });
    rig.until(|rig| rig.tcp(ds).may_send().then_some(()))
        .expect("the ds connects");
    rig.tcp(ds).send_slice(b"list").unwrap();
    rig.until(Rig::sent).expect("the request goes out");
    rig.answer(NetIn::Read {
        stream,
        data: b"servers".to_vec(),
    });
    let mut got = Vec::new();
    rig.until(|rig| {
        rig.tcp(ds)
            .recv(|data| {
                got.extend_from_slice(data);
                (data.len(), ())
            })
            .ok();
        (got.len() == 7).then_some(())
    })
    .expect("the ds reads the list");
    rig.tcp(ds).close();
    assert_eq!(rig.until(Rig::sent), Some(NetOut::Close { stream }));
    rig.until(|rig| {
        matches!(
            rig.tcp(ds).state(),
            tcp::State::Closed | tcp::State::TimeWait
        )
        .then_some(())
    })
    .expect("the stream ends for the ds too");
    rig.until(|rig| rig.gateway.tcp_peers().is_empty().then_some(()))
        .expect("the gateway lets the flow go");
    assert_eq!(rig.until(Rig::sent), None, "one close is enough");
}

#[test]
fn a_ds_that_resets_during_the_handshake_frees_its_relayed_stream() {
    let mut rig = Rig::new();
    let socket = tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; 4096]),
        tcp::SocketBuffer::new(vec![0; 4096]),
    );
    let ds = rig.client.sockets.add(socket);
    let context = rig.client.iface.context();
    rig.client
        .sockets
        .get_mut::<tcp::Socket>(ds)
        .connect(
            context,
            IpEndpoint::new(IpAddress::Ipv4(FAR), 29_900),
            41_002,
        )
        .unwrap();
    rig.client.step(&mut rig.gateway);
    let open = (0..STEPS).find_map(|_| {
        rig.client.step_deaf(&mut rig.gateway);
        rig.sent()
    });
    let Some(NetOut::Open { stream, .. }) = open else {
        panic!("the relay should get an open");
    };
    rig.client.sockets.remove(ds);
    rig.answer(NetIn::Opened { stream, ok: true });
    rig.answer(NetIn::Read {
        stream,
        data: vec![0x5C; 38],
    });
    assert_eq!(rig.until(Rig::sent), Some(NetOut::Close { stream }));
    assert!(rig.gateway.tcp_peers().is_empty());
}
