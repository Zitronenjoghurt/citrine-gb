use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Code(u32);

impl Code {
    pub const DIGITS: usize = 6;

    pub fn random() -> Self {
        let mut bytes = [0; 4];
        getrandom::getrandom(&mut bytes).expect("the system has randomness");
        Self(u32::from_le_bytes(bytes) % 1_000_000)
    }

    pub fn parse(text: &str) -> Option<Self> {
        let digits: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if digits.len() != Self::DIGITS || !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok().map(Self)
    }

    pub fn plain(self) -> String {
        format!("{:06}", self.0)
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plain = self.plain();
        write!(f, "{} {}", &plain[..3], &plain[3..])
    }
}

pub const CODE_LIFE_SECS: f64 = 600.0;
pub const DISCOVERY_PORT: u16 = 47813;

#[cfg(not(target_arch = "wasm32"))]
pub use lan::{
    ANSWERS_PER_SEC, Discovery, Listener, Nearby, NearbyHost, Presence, SCREEN_SERVICE, Sweep,
};

pub type Tag = [u8; 8];

#[cfg(not(target_arch = "wasm32"))]
mod lan {
    use super::{DISCOVERY_PORT, Tag};
    use crate::link::Tcp;
    use crate::proof::{Nonce, TagKey};
    use crate::proto::MAX_NAME_CHARS;
    use citrine_gb_threads::Spawner;
    use std::io::ErrorKind;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, UdpSocket};
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    const SWEEP_THREADS: u8 = 32;
    const SWEEP_WAIT: Duration = Duration::from_millis(300);
    const PROBES_KEPT: usize = 4;

    const PROBE: &str = "CITRINE-SCREEN";
    const ANSWER: &str = "CITRINE-HOST";

    const NEARBY_PROBE: &str = "CITRINE-NEARBY";
    const NEARBY_ANSWER: &str = "CITRINE-HERE";
    pub const SCREEN_SERVICE: &str = "screen";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn parse_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
        if text.len() != N * 2 || !text.is_ascii() {
            return None;
        }
        let mut bytes = [0; N];
        for (at, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[at * 2..at * 2 + 2], 16).ok()?;
        }
        Some(bytes)
    }

    fn tag_text(tag: Option<Tag>) -> String {
        tag.map_or_else(|| "-".into(), |tag| hex(&tag))
    }

    fn parse_tag(text: &str) -> Option<Option<Tag>> {
        if text == "-" {
            return Some(None);
        }
        parse_hex(text).map(Some)
    }

    fn nearby_probe(nonce: &Nonce) -> String {
        format!("{NEARBY_PROBE} {}", hex(nonce))
    }

    fn parse_nearby_probe(bytes: &[u8]) -> Option<Option<Nonce>> {
        let text = std::str::from_utf8(bytes).ok()?;
        if text == NEARBY_PROBE {
            return Some(None);
        }
        let nonce = text.strip_prefix(NEARBY_PROBE)?.strip_prefix(' ')?;
        parse_hex(nonce).map(Some)
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct NearbyHost {
        pub addr: SocketAddr,
        pub instance: u64,
        pub tag: Option<Tag>,
        pub services: Vec<String>,
        pub name: String,
    }

    impl NearbyHost {
        pub fn offers(&self, service: &str) -> bool {
            self.services.iter().any(|s| s == service)
        }

        pub(super) fn text(&self) -> String {
            format!(
                "{NEARBY_ANSWER} {} {:016x} {} {} {}",
                self.addr.port(),
                self.instance,
                tag_text(self.tag),
                self.services.join(","),
                self.name.chars().take(MAX_NAME_CHARS).collect::<String>()
            )
        }

        pub(super) fn parse(text: &str, from: SocketAddr) -> Option<Self> {
            let mut words = text.splitn(6, ' ');
            if words.next() != Some(NEARBY_ANSWER) {
                return None;
            }
            let port = words.next()?.parse().ok()?;
            let instance = u64::from_str_radix(words.next()?, 16).ok()?;
            let tag = parse_tag(words.next()?)?;
            let services: Vec<String> = words
                .next()?
                .split(',')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
            if services.is_empty() {
                return None;
            }
            let name: String = words.next()?.chars().take(MAX_NAME_CHARS).collect();
            Some(Self {
                addr: SocketAddr::new(from.ip(), port),
                instance,
                tag,
                services,
                name,
            })
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Presence {
        pub name: String,
        pub tag_key: Option<TagKey>,
        pub services: Vec<String>,
    }

    pub const ANSWERS_PER_SEC: u32 = 32;
    const READS_PER_ANSWER: usize = 256;

    pub struct Listener {
        tcp: TcpListener,
        udp: Option<UdpSocket>,
        instance: u64,
        presence: Option<Presence>,
        answers: u32,
        answers_since: Instant,
    }

    impl Listener {
        pub fn open() -> std::io::Result<Self> {
            Self::on(DISCOVERY_PORT)
        }

        pub fn on(discovery_port: u16) -> std::io::Result<Self> {
            let tcp = TcpListener::bind((Ipv4Addr::UNSPECIFIED, discovery_port))
                .or_else(|_| TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)))?;
            tcp.set_nonblocking(true)?;
            let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, discovery_port))
                .and_then(|udp| udp.set_nonblocking(true).map(|()| udp))
                .ok();
            let mut instance = [0; 8];
            let _ = getrandom::getrandom(&mut instance);
            Ok(Self {
                tcp,
                udp,
                instance: u64::from_le_bytes(instance),
                presence: None,
                answers: 0,
                answers_since: Instant::now(),
            })
        }

        pub fn instance(&self) -> u64 {
            self.instance
        }

        pub fn set_presence(&mut self, presence: Option<Presence>) {
            self.presence = presence.filter(|p| !p.services.is_empty());
        }

        pub fn port(&self) -> u16 {
            self.tcp.local_addr().map(|a| a.port()).unwrap_or(0)
        }

        pub fn discovery_port(&self) -> Option<u16> {
            self.udp
                .as_ref()
                .and_then(|u| u.local_addr().ok())
                .map(|a| a.port())
        }

        pub fn accept(&mut self) -> Option<Tcp> {
            let (stream, _) = self.tcp.accept().ok()?;
            Tcp::new(stream).ok()
        }

        pub fn answer(&mut self, showing_code: bool) {
            let Some(udp) = &self.udp else {
                return;
            };
            let now = Instant::now();
            if now.duration_since(self.answers_since) >= Duration::from_secs(1) {
                self.answers_since = now;
                self.answers = 0;
            }
            let mut buf = [0; 64];
            for _ in 0..READS_PER_ANSWER {
                let Ok((n, from)) = udp.recv_from(&mut buf) else {
                    break;
                };
                if self.answers >= ANSWERS_PER_SEC {
                    continue;
                }
                self.answers += 1;
                if let Some(nonce) = parse_nearby_probe(&buf[..n]) {
                    if let Some(presence) = &self.presence {
                        let here = NearbyHost {
                            addr: SocketAddr::from((Ipv4Addr::UNSPECIFIED, self.port())),
                            instance: self.instance,
                            tag: presence.tag_key.zip(nonce).map(|(key, n)| key.tag(&n)),
                            services: presence.services.clone(),
                            name: presence.name.clone(),
                        };
                        let _ = udp.send_to(here.text().as_bytes(), from);
                    }
                    continue;
                }
                if showing_code && buf[..n] == *PROBE.as_bytes() {
                    let answer = format!("{ANSWER} {}", self.port());
                    let _ = udp.send_to(answer.as_bytes(), from);
                }
            }
        }

        pub fn may_send(&self) -> bool {
            let Some(udp) = &self.udp else {
                return true;
            };
            let _ = udp.set_broadcast(true);
            let to = SocketAddr::from((Ipv4Addr::BROADCAST, DISCOVERY_PORT));
            !matches!(udp.send_to(&[], to), Err(e) if e.kind() == ErrorKind::HostUnreachable)
        }
    }

    pub fn local_ipv4() -> Option<Ipv4Addr> {
        let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        udp.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
        match udp.local_addr().ok()?.ip() {
            IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Some(ip),
            _ => None,
        }
    }

    fn neighbours(own: Ipv4Addr) -> Vec<Ipv4Addr> {
        let [a, b, c, d] = own.octets();
        (1..=254)
            .filter(|&host| host != d)
            .map(|host| Ipv4Addr::new(a, b, c, host))
            .collect()
    }

    fn directed_broadcast(own: Ipv4Addr) -> Ipv4Addr {
        let [a, b, c, _] = own.octets();
        Ipv4Addr::new(a, b, c, 255)
    }

    pub struct Sweep {
        found: Receiver<Tcp>,
    }

    impl Sweep {
        pub fn start(port: u16, spawner: &Spawner) -> Option<Self> {
            let own = local_ipv4()?;
            let targets = neighbours(own)
                .into_iter()
                .map(|ip| SocketAddr::from((ip, port)))
                .collect();
            Some(Self::toward(targets, spawner))
        }

        pub fn toward(targets: Vec<SocketAddr>, spawner: &Spawner) -> Self {
            let (tx, found) = channel();
            for lane in 0..SWEEP_THREADS {
                let mine: Vec<SocketAddr> = targets
                    .iter()
                    .skip(usize::from(lane))
                    .step_by(usize::from(SWEEP_THREADS))
                    .copied()
                    .collect();
                if mine.is_empty() {
                    continue;
                }
                let tx = tx.clone();
                spawner.run("screen sweep", move || {
                    for addr in mine {
                        if let Ok(tcp) = Tcp::dial(addr, SWEEP_WAIT)
                            && tx.send(tcp).is_err()
                        {
                            return;
                        }
                    }
                });
            }
            Self { found }
        }

        pub fn poll(&mut self) -> Option<Tcp> {
            self.found.try_recv().ok()
        }
    }

    pub struct Nearby {
        udp: Option<UdpSocket>,
        targets: Vec<SocketAddr>,
        skip: Option<u64>,
        nonces: Vec<Nonce>,
    }

    impl Nearby {
        pub fn start(skip: Option<u64>) -> Self {
            Self::toward(broadcast_targets(), skip)
        }

        pub fn toward(targets: Vec<SocketAddr>, skip: Option<u64>) -> Self {
            let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
                .and_then(|udp| {
                    udp.set_broadcast(true)?;
                    udp.set_nonblocking(true)?;
                    Ok(udp)
                })
                .ok();
            let mut nearby = Self {
                udp,
                targets,
                skip,
                nonces: Vec::new(),
            };
            nearby.probe();
            nearby
        }

        pub fn probe(&mut self) {
            let nonce = crate::proof::nonce();
            if self.nonces.len() >= PROBES_KEPT {
                self.nonces.remove(0);
            }
            self.nonces.push(nonce);
            if let Some(udp) = &self.udp {
                for target in &self.targets {
                    let _ = udp.send_to(nearby_probe(&nonce).as_bytes(), target);
                }
            }
        }

        pub fn tagged_by(&self, host: &NearbyHost, key: &TagKey) -> bool {
            host.tag
                .is_some_and(|tag| self.nonces.iter().any(|nonce| key.tags(nonce, &tag)))
        }

        pub fn poll(&mut self) -> Vec<NearbyHost> {
            let mut heard = Vec::new();
            let Some(udp) = &self.udp else {
                return heard;
            };
            let mut buf = [0; 256];
            while let Ok((n, from)) = udp.recv_from(&mut buf) {
                let text = std::str::from_utf8(&buf[..n]).unwrap_or_default();
                if let Some(host) = NearbyHost::parse(text, from)
                    && Some(host.instance) != self.skip
                    && !heard
                        .iter()
                        .any(|h: &NearbyHost| h.instance == host.instance)
                {
                    heard.push(host);
                }
            }
            heard
        }
    }

    fn broadcast_targets() -> Vec<SocketAddr> {
        let mut targets = vec![SocketAddr::from((Ipv4Addr::BROADCAST, DISCOVERY_PORT))];
        if let Some(own) = local_ipv4() {
            targets.push(SocketAddr::from((directed_broadcast(own), DISCOVERY_PORT)));
        }
        targets
    }

    pub struct Discovery {
        udp: Option<UdpSocket>,
        targets: Vec<SocketAddr>,
        heard: Vec<SocketAddr>,
    }

    impl Discovery {
        pub fn start() -> Self {
            Self::toward(&broadcast_targets())
        }

        pub fn toward(targets: &[SocketAddr]) -> Self {
            let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
                .and_then(|udp| {
                    udp.set_broadcast(true)?;
                    udp.set_nonblocking(true)?;
                    Ok(udp)
                })
                .ok();
            let discovery = Self {
                udp,
                targets: targets.to_vec(),
                heard: Vec::new(),
            };
            discovery.probe();
            discovery
        }

        pub fn probe(&self) {
            if let Some(udp) = &self.udp {
                for target in &self.targets {
                    let _ = udp.send_to(PROBE.as_bytes(), target);
                }
            }
        }

        pub fn poll(&mut self) -> Option<SocketAddr> {
            let udp = self.udp.as_ref()?;
            let mut buf = [0; 64];
            while let Ok((n, from)) = udp.recv_from(&mut buf) {
                let text = std::str::from_utf8(&buf[..n]).unwrap_or_default();
                let Some(port) = text
                    .strip_prefix(ANSWER)
                    .and_then(|rest| rest.strip_prefix(' '))
                    .and_then(|port| port.parse().ok())
                else {
                    continue;
                };
                let host = SocketAddr::new(from.ip(), port);
                if !self.heard.contains(&host) {
                    self.heard.push(host);
                    return Some(host);
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_read_with_or_without_the_space() {
        let code = Code::parse("482 913").unwrap();
        assert_eq!(code.to_string(), "482 913");
        assert_eq!(Code::parse("482913"), Some(code));
        assert_eq!(Code::parse("007 001").unwrap().to_string(), "007 001");
        assert_eq!(Code::parse("48291"), None);
        assert_eq!(Code::parse("4829134"), None);
        assert_eq!(Code::parse("48291a"), None);
    }

    #[test]
    fn random_codes_have_six_digits() {
        for _ in 0..50 {
            assert_eq!(Code::random().plain().len(), 6);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_surface_finds_each_host_showing_a_code_once() {
        use std::net::{Ipv4Addr, SocketAddr};
        let mut showing = Listener::on(0).unwrap();
        let mut idle = Listener::on(0).unwrap();
        let at =
            |l: &Listener| SocketAddr::from((Ipv4Addr::LOCALHOST, l.discovery_port().unwrap()));
        let mut phone = Discovery::toward(&[at(&showing), at(&idle)]);
        let mut found = Vec::new();
        for round in 0..2000 {
            if round % 20 == 0 {
                phone.probe();
            }
            showing.answer(true);
            idle.answer(false);
            found.extend(phone.poll());
            if round > 100 && !found.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let ports: Vec<u16> = found.iter().map(|a| a.port()).collect();
        assert_eq!(ports, vec![showing.port()]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn holds_the_code(bytes: &[u8], code: Code) -> bool {
        use sha2::{Digest, Sha256};
        let plain = code.plain();
        let hashed = Sha256::digest(plain.as_bytes());
        let text = String::from_utf8_lossy(bytes);
        text.contains(&plain)
            || text.contains(&code.to_string())
            || bytes.windows(8).any(|w| w == &hashed[..8])
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_sniffer_reads_no_code_from_discovery() {
        use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
        let code = Code::parse("482 913").unwrap();
        let sniffer = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        sniffer.set_nonblocking(true).unwrap();
        let at = [sniffer.local_addr().unwrap()];
        let _phone = Discovery::toward(&at);
        let mut buf = [0; 256];
        let mut probe = None;
        for _ in 0..2000 {
            if let Ok((n, _)) = sniffer.recv_from(&mut buf) {
                probe = Some(buf[..n].to_vec());
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let probe = probe.expect("the phone probes");
        assert!(
            !holds_the_code(&probe, code),
            "{:?}",
            String::from_utf8_lossy(&probe)
        );

        let mut host = Listener::on(0).unwrap();
        let to = SocketAddr::from((Ipv4Addr::LOCALHOST, host.discovery_port().unwrap()));
        sniffer.send_to(&probe, to).unwrap();
        let mut answer = None;
        for _ in 0..2000 {
            host.answer(true);
            if let Ok((n, _)) = sniffer.recv_from(&mut buf) {
                answer = Some(buf[..n].to_vec());
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let answer = answer.expect("the host showing a code answers");
        assert!(
            !holds_the_code(&answer, code),
            "{:?}",
            String::from_utf8_lossy(&answer)
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_sweep_reaches_a_host_without_its_discovery_answer() {
        use std::net::{Ipv4Addr, SocketAddr};
        let mut listener = Listener::on(0).unwrap();
        let closed = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let closed_port = closed.local_addr().unwrap().port();
        drop(closed);
        let targets = vec![
            SocketAddr::from((Ipv4Addr::LOCALHOST, closed_port)),
            SocketAddr::from((Ipv4Addr::LOCALHOST, listener.port())),
        ];
        let threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
        let mut sweep = Sweep::toward(targets, &threads.spawner());
        let mut found = None;
        let mut accepted = None;
        for _ in 0..2000 {
            found = found.or_else(|| sweep.poll());
            accepted = accepted.or_else(|| listener.accept());
            if found.is_some() && accepted.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let found = found.expect("the sweep finds the listener");
        assert_eq!(found.peer().map(|a| a.port()), Some(listener.port()));
        assert!(accepted.is_some());
        assert!(sweep.poll().is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_nearby_probe_lists_hosts_by_name_and_skips_its_own() {
        use std::net::{Ipv4Addr, SocketAddr};
        let key = crate::proof::TagKey([1; 32]);
        let mut mac = Listener::on(0).unwrap();
        mac.set_presence(Some(Presence {
            name: "Clara's MacBook".into(),
            tag_key: Some(key),
            services: vec![SCREEN_SERVICE.into()],
        }));
        let mut quiet = Listener::on(0).unwrap();
        let mut idle = Listener::on(0).unwrap();
        idle.set_presence(Some(Presence {
            name: "Idle PC".into(),
            tag_key: None,
            services: Vec::new(),
        }));
        let mut own = Listener::on(0).unwrap();
        own.set_presence(Some(Presence {
            name: "Pixel 8".into(),
            tag_key: None,
            services: vec![SCREEN_SERVICE.into()],
        }));
        let at =
            |l: &Listener| SocketAddr::from((Ipv4Addr::LOCALHOST, l.discovery_port().unwrap()));
        let mut nearby = Nearby::toward(
            vec![at(&mac), at(&quiet), at(&idle), at(&own)],
            Some(own.instance()),
        );
        let mut heard = Vec::new();
        for _ in 0..2000 {
            mac.answer(false);
            quiet.answer(false);
            idle.answer(false);
            own.answer(false);
            heard.extend(nearby.poll());
            if !heard.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        for _ in 0..50 {
            mac.answer(false);
            idle.answer(false);
            own.answer(false);
            heard.extend(nearby.poll());
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(heard.len(), 1, "{heard:?}");
        assert_eq!(heard[0].name, "Clara's MacBook");
        assert!(nearby.tagged_by(&heard[0], &key));
        assert!(!nearby.tagged_by(&heard[0], &crate::proof::TagKey([2; 32])));
        assert_eq!(heard[0].addr.port(), mac.port());
        assert_eq!(heard[0].instance, mac.instance());
        assert!(heard[0].offers(SCREEN_SERVICE));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_nearby_answer_survives_a_round_trip() {
        use std::net::{Ipv4Addr, SocketAddr};
        let host = NearbyHost {
            addr: SocketAddr::from((Ipv4Addr::new(10, 0, 0, 7), 47813)),
            instance: 0xfeed,
            tag: None,
            services: vec![SCREEN_SERVICE.into(), "walker".into()],
            name: "Living room PC".into(),
        };
        assert_eq!(
            host.text(),
            "CITRINE-HERE 47813 000000000000feed - screen,walker Living room PC"
        );
        assert_eq!(
            NearbyHost::parse(&host.text(), host.addr),
            Some(host.clone())
        );
        let later = NearbyHost::parse(
            "CITRINE-HERE 1 0000000000000001 - mirror,screen Den",
            host.addr,
        )
        .expect("unknown services are kept");
        assert!(later.offers(SCREEN_SERVICE) && !later.offers("walker"));
        assert_eq!(
            NearbyHost::parse("CITRINE-HERE 1 zz - screen x", host.addr),
            None
        );
        assert_eq!(
            NearbyHost::parse("CITRINE-HERE 1 0000000000000001 - , x", host.addr),
            None
        );
        assert_eq!(
            NearbyHost::parse("CITRINE-HOST 123456 47813", host.addr),
            None
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn two_probes_hear_different_tags_and_the_owner_knows_both() {
        use crate::proof::TagKey;
        use std::net::{Ipv4Addr, SocketAddr};
        let key = TagKey([3; 32]);
        let mut mac = Listener::on(0).unwrap();
        mac.set_presence(Some(Presence {
            name: "Clara's MacBook".into(),
            tag_key: Some(key),
            services: vec![SCREEN_SERVICE.into()],
        }));
        let at = SocketAddr::from((Ipv4Addr::LOCALHOST, mac.discovery_port().unwrap()));
        let mut heard = Vec::new();
        for _ in 0..2 {
            let mut nearby = Nearby::toward(vec![at], None);
            let mut first = None;
            for _ in 0..2000 {
                mac.answer(false);
                first = first.or_else(|| nearby.poll().into_iter().next());
                if first.is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let host = first.expect("the mac answers");
            assert!(nearby.tagged_by(&host, &key));
            assert!(!nearby.tagged_by(&host, &TagKey([4; 32])));
            heard.push(host);
        }
        assert!(heard[0].tag.is_some());
        assert_ne!(heard[0].tag, heard[1].tag);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_flood_of_probes_hears_a_bounded_number_of_answers() {
        use std::net::{Ipv4Addr, UdpSocket};
        let mut mac = Listener::on(0).unwrap();
        mac.set_presence(Some(Presence {
            name: "Mac".into(),
            tag_key: None,
            services: vec![SCREEN_SERVICE.into()],
        }));
        let udp = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        udp.set_nonblocking(true).unwrap();
        let at = (Ipv4Addr::LOCALHOST, mac.discovery_port().unwrap());
        for _ in 0..100 {
            udp.send_to(b"CITRINE-NEARBY", at).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        mac.answer(false);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let mut buf = [0; 256];
        let mut heard = 0;
        while udp.recv_from(&mut buf).is_ok() {
            heard += 1;
        }
        assert!(heard > 0);
        assert!(heard <= ANSWERS_PER_SEC, "{heard} answers");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_probe_without_a_nonce_hears_no_tag() {
        use std::net::{Ipv4Addr, UdpSocket};
        let mut mac = Listener::on(0).unwrap();
        mac.set_presence(Some(Presence {
            name: "Mac".into(),
            tag_key: Some(crate::proof::TagKey([3; 32])),
            services: vec![SCREEN_SERVICE.into()],
        }));
        let udp = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        udp.set_nonblocking(true).unwrap();
        let at = (Ipv4Addr::LOCALHOST, mac.discovery_port().unwrap());
        udp.send_to(b"CITRINE-NEARBY", at).unwrap();
        let mut buf = [0; 256];
        let mut answer = None;
        for _ in 0..2000 {
            mac.answer(false);
            if let Ok((n, from)) = udp.recv_from(&mut buf) {
                answer = NearbyHost::parse(std::str::from_utf8(&buf[..n]).unwrap(), from);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(answer.expect("the mac answers").tag, None);
    }
}
