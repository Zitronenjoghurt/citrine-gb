mod common;

use std::sync::mpsc::{self, Receiver, Sender};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use citrine_gb_nds_net::gateway::{Config, Gateway};
use citrine_gb_nds_net::list_cipher::ListCipher;
use citrine_gb_nds_net::server_list::Host;
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
    claims: Sender<(u64, u32)>,
    claimed: Receiver<(u64, Option<bool>)>,
    lists: Vec<u64>,
}

impl GtsLink for Relay {
    fn call(&mut self, id: u64, target: &str) {
        self.calls.send((id, target.to_owned())).ok();
    }

    fn answer(&mut self) -> Option<(u64, Option<Vec<u8>>)> {
        self.answers.try_recv().ok()
    }

    fn claim(&mut self, id: u64, profile: u32) {
        self.claims.send((id, profile)).ok();
    }

    fn claimed(&mut self) -> Option<(u64, Option<bool>)> {
        self.claimed.try_recv().ok()
    }

    fn host(&mut self, _: &str, _: u16, _: &[(String, String)]) {}

    fn unhost(&mut self) {}

    fn list(&mut self, id: u64, _: &str) {
        self.lists.push(id);
    }

    fn listed(&mut self) -> Option<(u64, Option<Vec<Host>>)> {
        self.lists.pop().map(|id| (id, Some(Vec::new())))
    }

    fn message(&mut self, _: std::net::SocketAddrV4, _: &[u8]) {}

    fn messages(&mut self) -> Option<(std::net::Ipv4Addr, u16, Vec<u8>)> {
        None
    }

    fn natneg(&mut self, _: u32, _: u8, _: u16) {}

    fn natnegged(&mut self) -> Option<(u32, std::net::SocketAddrV4)> {
        None
    }
}

struct Setup {
    gateway: Gateway,
    client: Client,
    calls: Receiver<(u64, String)>,
    answers: Sender<(u64, Option<Vec<u8>>)>,
    claims: Receiver<(u64, u32)>,
    claimed: Sender<(u64, Option<bool>)>,
    _threads: Threads,
}

fn setup() -> Setup {
    let threads = Threads::default();
    let (calls, called) = mpsc::channel();
    let (answer, answers) = mpsc::channel();
    let (claims, claim_rx) = mpsc::channel();
    let (claimed_tx, claimed) = mpsc::channel();
    let config = Config::default();
    let gateway = Gateway::new(config, threads.spawner())
        .with_resolver(std::sync::Arc::new(|_: &str| {
            panic!("no lookup leaves the gateway")
        }))
        .with_services(Services::new(
            77,
            Box::new(Relay {
                calls,
                answers,
                claims,
                claimed,
                lists: Vec::new(),
            }),
        ));
    Setup {
        gateway,
        client: Client::new(config.address),
        calls: called,
        answers: answer,
        claims: claim_rx,
        claimed: claimed_tx,
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
fn mario_kart_reaches_the_server_browser_with_its_list_request() {
    let mut setup = setup();
    assert_eq!(
        resolve(&mut setup, "mariokartds.ms17.gs.nintendowifi.net"),
        vec![IpAddress::Ipv4(services::ADDRESS)]
    );
    let mut request = vec![0, 1, 3, 0, 0, 0, 0];
    request.extend_from_slice(b"mariokartds\0mariokartds\0Abc12345\0\\dwc_pid\0");
    request.extend_from_slice(&[0, 0, 0, 0]);
    let mut framed = u16::try_from(request.len() + 2)
        .unwrap()
        .to_be_bytes()
        .to_vec();
    framed.extend_from_slice(&request);
    let browser = connect(&mut setup, services::SB_PORT, 4100);
    let reply = talk(&mut setup, browser, &framed, |heard| heard.len() >= 42);
    let random = usize::from(reply[0] ^ 0xEC);
    let key_len = usize::from(reply[random + 1] ^ 0xEA);
    let key = &reply[random + 2..random + 2 + key_len];
    let mut list = reply[random + 2 + key_len..].to_vec();
    ListCipher::for_list(b"yeJ3x8", *b"Abc12345", key).decrypt(&mut list);
    assert_eq!(list[4..6], 6500u16.to_be_bytes());
    assert_eq!(list[6..], *b"\x01\0dwc_pid\0\0\0\xFF\xFF\xFF\xFF");
    let log = setup.gateway.services_mut().unwrap().take_log();
    assert!(
        log.contains(&"sb list mariokartds filter  fields \\dwc_pid".to_owned()),
        "{log:?}"
    );
}

#[test]
fn a_browser_message_to_our_own_host_comes_back_to_its_query_port() {
    let mut setup = setup();
    let mut udp = udp::Socket::new(
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 512]),
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 512]),
    );
    udp.bind(56400).unwrap();
    let udp = setup.client.sockets.add(udp);
    let mut beat = vec![3, 9, 8, 7, 6];
    beat.extend_from_slice(b"gamename\0mariokartds\0dwc_pid\x00603819417\0\0");
    let qr = IpEndpoint::new(IpAddress::Ipv4(services::ADDRESS), services::QR_PORT);
    setup
        .client
        .sockets
        .get_mut::<udp::Socket>(udp)
        .send_slice(&beat, qr)
        .unwrap();
    let mut heard = Vec::new();
    for _ in 0..200 {
        setup.client.step(&mut setup.gateway);
        while let Ok((data, _)) = setup.client.sockets.get_mut::<udp::Socket>(udp).recv() {
            heard.push(data.to_vec());
        }
        if !heard.is_empty() {
            break;
        }
    }
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0][..3], [0xFE, 0xFD, 1]);

    let mut request = vec![0, 0, 2, 10, 77, 0, 2];
    request.extend_from_slice(&56400u16.to_be_bytes());
    request.extend_from_slice(b"hello");
    let length = u16::try_from(request.len()).unwrap().to_be_bytes();
    request[..2].copy_from_slice(&length);
    let browser = connect(&mut setup, services::SB_PORT, 4101);
    talk(&mut setup, browser, &request, |_| true);
    let mut message = None;
    for _ in 0..200 {
        setup.client.step(&mut setup.gateway);
        if let Ok((data, meta)) = setup.client.sockets.get_mut::<udp::Socket>(udp).recv() {
            message = Some((data.to_vec(), meta.endpoint));
            break;
        }
    }
    let (data, from) = message.expect("the message reaches the query port");
    assert_eq!(from, qr);
    assert_eq!(data[..7], [0xFE, 0xFD, 6, 9, 8, 7, 6]);
    assert_eq!(data[11..], *b"hello");
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
    let early = talk(
        &mut setup,
        gpcm,
        format!(
            "\\login\\\\challenge\\{}\\authtoken\\{token}\\gamename\\pokemondpds\\id\\1\\final\\",
            "x".repeat(32)
        )
        .as_bytes(),
        |_| false,
    );
    assert!(early.is_empty());
    let pid = services::profile_id(77, "CPUDjdnc");
    let stand_ins = setup.gateway.services_mut().unwrap();
    assert_eq!(stand_ins.profile_wanted(), Some(2_000_000_000_001));
    stand_ins.saved_profile(None);
    let (call, claimed) = setup
        .claims
        .try_recv()
        .expect("the login claims its profile");
    assert_eq!(claimed, pid);
    setup.claimed.send((call, Some(true))).unwrap();
    let answer = talk(&mut setup, gpcm, b"", |heard| heard.ends_with(b"\\final\\"));
    let answer = String::from_utf8(answer).unwrap();
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
