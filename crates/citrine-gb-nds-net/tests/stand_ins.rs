mod common;

use std::sync::mpsc::{self, Receiver, Sender};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use citrine_gb_nds_net::gateway::{Config, Gateway};
use citrine_gb_nds_net::services::{self, GtsLink, Services};
use citrine_gb_threads::Threads;
use common::Client;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::dns::{self, GetQueryResultError};
use smoltcp::socket::{tcp, udp};
use smoltcp::wire::{DnsQueryType, IpAddress, IpEndpoint};

struct Relay {
    calls: Sender<(u64, String)>,
    answers: Receiver<(u64, Option<Vec<u8>>)>,
}

impl GtsLink for Relay {
    fn call(&mut self, id: u64, target: &str) {
        self.calls.send((id, target.to_owned())).ok();
    }

    fn answer(&mut self) -> Option<(u64, Option<Vec<u8>>)> {
        self.answers.try_recv().ok()
    }
}

struct Setup {
    gateway: Gateway,
    client: Client,
    calls: Receiver<(u64, String)>,
    answers: Sender<(u64, Option<Vec<u8>>)>,
    _threads: Threads,
}

fn setup() -> Setup {
    let threads = Threads::default();
    let (calls, called) = mpsc::channel();
    let (answer, answers) = mpsc::channel();
    let config = Config::default();
    let gateway = Gateway::new(config, threads.spawner())
        .with_resolver(std::sync::Arc::new(|_: &str| {
            panic!("no lookup leaves the gateway")
        }))
        .with_services(Services::new(77, Box::new(Relay { calls, answers })));
    Setup {
        gateway,
        client: Client::new(config.address),
        calls: called,
        answers: answer,
        _threads: threads,
    }
}

fn resolve(setup: &mut Setup, name: &str) -> Vec<IpAddress> {
    let server = IpAddress::Ipv4(setup.gateway.config().address);
    let mut resolver = dns::Socket::new(&[server], vec![]);
    let query = resolver
        .start_query(setup.client.iface.context(), name, DnsQueryType::A)
        .unwrap();
    let handle = setup.client.sockets.add(resolver);
    let mut found = Vec::new();
    for _ in 0..2000 {
        setup.client.step(&mut setup.gateway);
        match setup
            .client
            .sockets
            .get_mut::<dns::Socket>(handle)
            .get_query_result(query)
        {
            Ok(addresses) => {
                found = addresses.to_vec();
                break;
            }
            Err(GetQueryResultError::Pending) => {}
            Err(_) => break,
        }
    }
    setup.client.sockets.remove(handle);
    found
}

fn connect(setup: &mut Setup, port: u16, local: u16) -> SocketHandle {
    let mut socket = tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; 8192]),
        tcp::SocketBuffer::new(vec![0; 8192]),
    );
    socket
        .connect(
            setup.client.iface.context(),
            IpEndpoint::new(IpAddress::Ipv4(services::ADDRESS), port),
            local,
        )
        .unwrap();
    setup.client.sockets.add(socket)
}

fn talk(
    setup: &mut Setup,
    handle: SocketHandle,
    send: &[u8],
    until: impl Fn(&[u8]) -> bool,
) -> Vec<u8> {
    let mut sent = false;
    let mut heard = Vec::new();
    for _ in 0..4000 {
        setup.client.step(&mut setup.gateway);
        let socket = setup.client.sockets.get_mut::<tcp::Socket>(handle);
        if !sent && socket.can_send() {
            socket.send_slice(send).unwrap();
            sent = true;
        }
        while socket.can_recv() {
            socket
                .recv(|data| {
                    heard.extend_from_slice(data);
                    (data.len(), ())
                })
                .unwrap();
        }
        if sent && until(&heard) {
            break;
        }
    }
    heard
}

fn ends_closed(setup: &mut Setup, handle: SocketHandle) -> bool {
    for _ in 0..200 {
        setup.client.step(&mut setup.gateway);
        if !setup.client.sockets.get::<tcp::Socket>(handle).may_recv() {
            return true;
        }
    }
    false
}

fn field(body: &str, name: &str) -> String {
    let value = body
        .trim_end()
        .split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
        .unwrap_or_default();
    String::from_utf8(STANDARD.decode(value.replace('*', "=")).unwrap()).unwrap()
}

#[test]
fn every_wfc_name_the_stand_ins_answer_resolves_to_them_and_others_to_nothing() {
    let mut setup = setup();
    let ours = vec![IpAddress::Ipv4(services::ADDRESS)];
    assert_eq!(resolve(&mut setup, "conntest.nintendowifi.net"), ours);
    assert_eq!(resolve(&mut setup, "nas.nintendowifi.net"), ours);
    assert_eq!(resolve(&mut setup, "gpcm.gs.nintendowifi.net"), ours);
    assert_eq!(
        resolve(&mut setup, "pokemondpds.available.gs.nintendowifi.net"),
        ours
    );
    assert_eq!(resolve(&mut setup, services::GTS_HOST), ours);
    assert_eq!(
        resolve(&mut setup, "pokemondpds.ms4.gs.nintendowifi.net"),
        vec![]
    );
    let log = setup.gateway.services_mut().unwrap().take_log();
    assert!(log.contains(&"dns pokemondpds.ms4.gs.nintendowifi.net unknown".to_owned()));
}

#[test]
fn a_ds_logs_in_through_nas_and_gamespy_and_reaches_the_gts_over_the_link() {
    let mut setup = setup();
    let conntest = connect(&mut setup, services::HTTP_PORT, 4000);
    let reply = talk(
        &mut setup,
        conntest,
        b"GET / HTTP/1.1\r\nHost: conntest.nintendowifi.net\r\n\r\n",
        |heard| heard.ends_with(b"ok"),
    );
    assert!(reply.starts_with(b"HTTP/1.1 200 OK"));
    assert!(ends_closed(&mut setup, conntest));

    let login = format!(
        "action={}&userid={}&gsbrcd={}",
        STANDARD.encode("LOGIN").replace('=', "*"),
        STANDARD.encode("2000000000001").replace('=', "*"),
        STANDARD.encode("CPUDjdnc").replace('=', "*"),
    );
    let request = format!(
        "POST /ac HTTP/1.1\r\nHost: nas.nintendowifi.net\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{login}",
        login.len()
    );
    let nas = connect(&mut setup, services::HTTP_PORT, 4001);
    let reply = talk(&mut setup, nas, request.as_bytes(), |heard| {
        heard.ends_with(b"\r\n")
    });
    let reply = String::from_utf8(reply).unwrap();
    let body = reply.split("\r\n\r\n").nth(1).unwrap();
    assert_eq!(field(body, "returncd"), "001");
    let token = field(body, "token");

    let gpcm = connect(&mut setup, services::GPCM_PORT, 4002);
    let greeting = talk(&mut setup, gpcm, b"", |heard| heard.ends_with(b"\\final\\"));
    assert!(greeting.starts_with(b"\\lc\\1\\challenge\\"));
    let answer = talk(
        &mut setup,
        gpcm,
        format!(
            "\\login\\\\challenge\\{}\\authtoken\\{token}\\gamename\\pokemondpds\\id\\1\\final\\",
            "x".repeat(32)
        )
        .as_bytes(),
        |heard| heard.ends_with(b"\\final\\"),
    );
    let answer = String::from_utf8(answer).unwrap();
    let pid = services::profile_id(77, "CPUDjdnc");
    assert!(answer.starts_with("\\lc\\2\\sesskey\\"));
    assert!(answer.contains(&format!("\\profileid\\{pid}\\")));

    let mut udp = udp::Socket::new(
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 512]),
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 512]),
    );
    udp.bind(4003).unwrap();
    let udp = setup.client.sockets.add(udp);
    let mut available = vec![9, 0, 0, 0, 0];
    available.extend_from_slice(b"pokemondpds\0");
    setup
        .client
        .sockets
        .get_mut::<udp::Socket>(udp)
        .send_slice(
            &available,
            IpEndpoint::new(IpAddress::Ipv4(services::ADDRESS), services::QR_PORT),
        )
        .unwrap();
    let mut heard = None;
    for _ in 0..200 {
        setup.client.step(&mut setup.gateway);
        if let Ok((data, _)) = setup.client.sockets.get_mut::<udp::Socket>(udp).recv() {
            heard = Some(data.to_vec());
            break;
        }
    }
    assert_eq!(heard, Some(vec![0xFE, 0xFD, 9, 0, 0, 0, 0]));

    let gts = connect(&mut setup, services::HTTP_PORT, 4004);
    let target = format!("/pokemondpds/worldexchange/info.asp?pid={pid}");
    let request = format!(
        "GET {target} HTTP/1.1\r\nHost: {}\r\n\r\n",
        services::GTS_HOST
    );
    let early = talk(&mut setup, gts, request.as_bytes(), |_| false);
    assert!(early.is_empty());
    let called = setup.calls.try_recv().ok();
    let (id, asked) = called.expect("the gts request reaches the link");
    assert_eq!(asked, target);
    setup.answers.send((id, Some(b"token".to_vec()))).unwrap();
    let reply = talk(&mut setup, gts, b"", |heard| heard.ends_with(b"token"));
    assert!(reply.starts_with(b"HTTP/1.1 200 OK"));
    assert!(ends_closed(&mut setup, gts));
}
