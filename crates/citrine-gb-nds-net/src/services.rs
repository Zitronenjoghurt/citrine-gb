use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use md5::{Digest, Md5};

use crate::http::{self, Parsed, Request};
use crate::list_cipher::{CHALLENGE_LEN, ListCipher};
use crate::server_list::Host;

pub const ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
pub const GTS_HOST: &str = "gamestats2.gs.nintendowifi.net";
pub const VALIDATION_HOST: &str = "pkvldtprod.nintendo.co.jp";
pub const HTTP_PORT: u16 = 80;
pub const GPCM_PORT: u16 = 29900;
pub const GPSP_PORT: u16 = 29901;
pub const QR_PORT: u16 = 27900;
pub const SB_PORT: u16 = 28910;
pub const NATNEG_PORT: u16 = 27901;

const DOMAIN: &str = "nintendowifi.net";
const SERVED: &[&str] = &[
    "conntest.nintendowifi.net",
    "nas.nintendowifi.net",
    "gpcm.gs.nintendowifi.net",
    "gpsp.gs.nintendowifi.net",
    GTS_HOST,
    VALIDATION_HOST,
];
const SERVED_SUFFIXES: &[&str] = &[
    ".available.gs.nintendowifi.net",
    ".master.gs.nintendowifi.net",
    "natneg1.gs.nintendowifi.net",
    "natneg2.gs.nintendowifi.net",
    "natneg3.gs.nintendowifi.net",
];
const GAMESPY_SUFFIX: &str = ".gs.nintendowifi.net";
const LIST_REQUEST: u8 = 0;
const SEND_MESSAGE: u8 = 2;
const NO_SERVER_LIST: u32 = 0x02;
const ALTERNATE_SOURCE_IP: u32 = 0x08;
const LIMIT_RESULT_COUNT: u32 = 0x80;
const LIST_SECRETS: &[(&str, &[u8])] = &[("mariokartds", b"yeJ3x8")];
const LIST_HEADER_RANDOM: usize = 5;
const LIST_KEY_LEN: usize = 13;
const LIST_QUERY_PORT: u16 = 6500;
const LIST_END: [u8; 5] = [0, 0xFF, 0xFF, 0xFF, 0xFF];
const MAX_HOSTS: usize = 8;
const QR_MAGIC: [u8; 2] = [0xFE, 0xFD];
const CHALLENGE: u8 = 0x01;
const HEARTBEAT: u8 = 0x03;
const AVAILABLE: u8 = 0x09;
const REGISTERED: u8 = 0x0A;
const CLIENT_MESSAGE: u8 = 0x06;
const CLIENT_MESSAGE_ACK: u8 = 0x07;
const MAX_OUTGOING: usize = 32;
const NATNEG_MAGIC: [u8; 6] = [0xFD, 0xFC, 0x1E, 0x66, 0x6A, 0xB2];
const NN_INIT: u8 = 0;
const NN_INIT_ACK: u8 = 1;
const NN_CONNECT: u8 = 5;
const NN_CONNECT_ACK: u8 = 6;
const NN_ADDRESS_CHECK: u8 = 10;
const NN_ADDRESS_REPLY: u8 = 11;
const NN_REPORT: u8 = 13;
const NN_REPORT_ACK: u8 = 14;
const NN_GAME_PORT: u8 = 0;
const NN_FIRST_PORT: u8 = 1;
const NN_ACK_LEN: usize = 21;
const NN_ACK_FILL: [u8; 7] = [0xFF, 0xFF, 0x6D, 0x16, 0xB5, 0x7D, 0xEA];
const NN_GOT_YOUR_DATA: u8 = 0x42;
const MAX_NEGOTIATIONS: usize = 16;
const NN_RESEND_US: u64 = 500_000;
const NN_CONNECT_TRIES: u8 = 12;
const NN_RETELL_US: u64 = 1_000_000;
const NN_PARTNER_WAIT_US: u64 = 60_000_000;
const AVAILABLE_REPLY: [u8; 7] = [0xFE, 0xFD, 0x09, 0, 0, 0, 0];
const NAS_HEADERS: &[(&str, &str)] = &[
    ("Content-Type", "text/plain"),
    ("NODE", "wifiappe1"),
    ("Server", "Nintendo Wii (http)"),
];
const ALPHANUMERIC: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const UPPERCASE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const NICK_DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuv";
const PROOF_PAD: &str = "                                                ";
const LOG_LIMIT: usize = 256;
const VALIDATED_BYTES: usize = 220;
const UNKNOWN_KIND_RESULTS: usize = 6;
const SIGNATURE_BYTES: usize = 128;

pub trait GtsLink: Send {
    fn call(&mut self, id: u64, target: &str);
    fn answer(&mut self) -> Option<(u64, Option<Vec<u8>>)>;
    fn claim(&mut self, id: u64, profile: u32);
    fn claimed(&mut self) -> Option<(u64, Option<bool>)>;
    fn host(&mut self, game: &str, port: u16, keys: &[(String, String)]);
    fn unhost(&mut self);
    fn list(&mut self, id: u64, game: &str);
    fn listed(&mut self) -> Option<(u64, Option<Vec<Host>>)>;
    fn message(&mut self, to: SocketAddrV4, data: &[u8]);
    fn messages(&mut self) -> Option<(Ipv4Addr, u16, Vec<u8>)>;
    fn natneg(&mut self, cookie: u32, index: u8, port: u16);
    fn natnegged(&mut self) -> Option<(u32, SocketAddrV4)>;
}

pub fn profile_id(seed: u64, gsbrcd: &str) -> u32 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for byte in seed.to_le_bytes().iter().chain(gsbrcd.as_bytes()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    u32::try_from(hash % 0x7FFF_FFFE).unwrap_or_default() + 1
}

pub fn is_served(name: &str) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    SERVED.contains(&name.as_str())
        || SERVED_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
        || is_server_browser(&name)
}

fn is_server_browser(name: &str) -> bool {
    let Some(rest) = name.strip_suffix(GAMESPY_SUFFIX) else {
        return false;
    };
    let Some((game, master)) = rest.rsplit_once('.') else {
        return false;
    };
    LIST_SECRETS.iter().any(|(known, _)| *known == game)
        && master
            .strip_prefix("ms")
            .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

pub fn is_nintendo(name: &str) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    name == DOMAIN || name.ends_with(&format!(".{DOMAIN}")) || name == VALIDATION_HOST
}

pub fn validation(body: &[u8]) -> Vec<u8> {
    let count = body
        .iter()
        .position(|&byte| byte == 0)
        .and_then(|end| body.get(end + 1..end + 3).map(|kind| (end + 3, kind)))
        .map_or(UNKNOWN_KIND_RESULTS, |(start, kind)| match kind {
            [0x00, 0x00 | 0x01] => (body.len() - start) / VALIDATED_BYTES,
            _ => UNKNOWN_KIND_RESULTS,
        });
    vec![0; 1 + 4 * count + SIGNATURE_BYTES]
}

struct NasLogin {
    token: String,
    challenge: String,
    userid: String,
    gsbrcd: String,
    served: Option<u32>,
}

struct GpLogin {
    token: String,
    client: String,
    id: String,
    claim: Option<Claim>,
}

struct Claim {
    call: u64,
    profile: u32,
    saved: bool,
}

struct Flow {
    id: u64,
    port: u16,
    input: Vec<u8>,
    output: Vec<u8>,
    closing: bool,
    waiting: Option<u64>,
    challenge: String,
    profile: String,
    login: Option<GpLogin>,
    peer: SocketAddrV4,
    listing: Option<ListRequest>,
}

struct Negotiation {
    cookie: u32,
    index: u8,
    version: u8,
    game: Option<SocketAddrV4>,
    negotiator: Option<SocketAddrV4>,
    peer: Option<SocketAddrV4>,
    started: u64,
    told: Option<u64>,
    sent: Option<u64>,
    tries: u8,
    acked: bool,
}

pub struct Services {
    seed: u64,
    link: Box<dyn GtsLink>,
    logins: Vec<NasLogin>,
    names: HashMap<(String, String), String>,
    flows: Vec<Flow>,
    hosts: Vec<Host>,
    outgoing: Vec<(u16, SocketAddrV4, Vec<u8>)>,
    negotiations: Vec<Negotiation>,
    next: u64,
    random: u64,
    log: Vec<String>,
    clock: fn() -> u64,
    now: u64,
}

impl Services {
    pub fn new(seed: u64, link: Box<dyn GtsLink>) -> Self {
        Self {
            seed,
            link,
            logins: Vec::new(),
            names: HashMap::new(),
            flows: Vec::new(),
            hosts: Vec::new(),
            outgoing: Vec::new(),
            negotiations: Vec::new(),
            next: 1,
            random: seed ^ 0x9E37_79B9_7F4A_7C15,
            log: Vec::new(),
            clock: unix_now,
            now: 0,
        }
    }

    pub fn with_clock(mut self, clock: fn() -> u64) -> Self {
        self.clock = clock;
        self
    }

    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }

    pub fn note(&mut self, line: String) {
        if self.log.len() < LOG_LIMIT {
            self.log.push(line);
        }
    }

    pub fn open(&mut self, port: u16) -> Option<u64> {
        if !matches!(port, HTTP_PORT | GPCM_PORT | GPSP_PORT | SB_PORT) {
            self.note(format!("refused port {port}"));
            return None;
        }
        let id = self.next;
        self.next += 1;
        let mut flow = Flow {
            id,
            port,
            input: Vec::new(),
            output: Vec::new(),
            closing: false,
            waiting: None,
            challenge: String::new(),
            profile: String::new(),
            login: None,
            peer: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0),
            listing: None,
        };
        if port == GPCM_PORT {
            flow.challenge = self.random_text(UPPERCASE, 10);
            flow.output = gp_message(&[("lc", "1"), ("challenge", &flow.challenge), ("id", "1")]);
        }
        self.flows.push(flow);
        Some(id)
    }

    pub fn peer(&mut self, id: u64, peer: SocketAddrV4) {
        if let Some(flow) = self.flows.iter_mut().find(|flow| flow.id == id) {
            flow.peer = peer;
        }
    }

    pub fn input(&mut self, id: u64, data: &[u8]) {
        let Some(at) = self.flows.iter().position(|flow| flow.id == id) else {
            return;
        };
        self.flows[at].input.extend_from_slice(data);
        match self.flows[at].port {
            HTTP_PORT => self.serve_http(at),
            SB_PORT => self.serve_browser(at),
            _ => self.serve_gamespy(at),
        }
    }

    pub fn take_output(&mut self, id: u64, room: usize) -> Vec<u8> {
        let Some(flow) = self.flows.iter_mut().find(|flow| flow.id == id) else {
            return Vec::new();
        };
        let taken = room.min(flow.output.len());
        flow.output.drain(..taken).collect()
    }

    pub fn finished(&self, id: u64) -> bool {
        self.flows
            .iter()
            .find(|flow| flow.id == id)
            .is_none_or(|flow| flow.closing && flow.output.is_empty() && flow.waiting.is_none())
    }

    pub fn close(&mut self, id: u64) {
        self.flows.retain(|flow| flow.id != id);
    }

    pub fn take_datagrams(&mut self, port: u16) -> Vec<(SocketAddrV4, Vec<u8>)> {
        let (taken, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.outgoing)
            .into_iter()
            .partition(|(from, _, _)| *from == port);
        self.outgoing = kept;
        taken.into_iter().map(|(_, to, data)| (to, data)).collect()
    }

    pub fn datagram(&mut self, port: u16, from: SocketAddrV4, data: &[u8]) -> Option<Vec<u8>> {
        if port == NATNEG_PORT {
            return self.negotiate(from, data);
        }
        if port != QR_PORT {
            return None;
        }
        match data.first().copied() {
            Some(AVAILABLE) => Some(self.qr_available(data)),
            Some(HEARTBEAT) => {
                let session = data.get(1..5)?;
                let text = format!(
                    "{}00{}{:04X}",
                    self.random_text(UPPERCASE, 6),
                    from.ip()
                        .octets()
                        .iter()
                        .map(|octet| format!("{octet:02X}"))
                        .collect::<String>(),
                    from.port()
                );
                self.note("qr heartbeat".to_owned());
                self.keep_host(from, data);
                Some(qr_message(CHALLENGE, session, text.as_bytes()))
            }
            Some(CHALLENGE) => {
                let session = data.get(1..5)?;
                self.note("qr registered".to_owned());
                Some(qr_message(REGISTERED, session, &[]))
            }
            Some(CLIENT_MESSAGE_ACK) => {
                self.note("qr message taken".to_owned());
                None
            }
            _ => None,
        }
    }

    fn negotiate(&mut self, from: SocketAddrV4, data: &[u8]) -> Option<Vec<u8>> {
        if data.get(..6)? != NATNEG_MAGIC || data.len() < 12 {
            return None;
        }
        let version = data[6];
        let kind = data[7];
        let cookie = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let ack = |kind: u8| {
            let mut reply = data[..data.len().min(NN_ACK_LEN)].to_vec();
            reply.resize(NN_ACK_LEN, 0);
            reply[7] = kind;
            reply
        };
        match kind {
            NN_INIT if data.len() >= NN_ACK_LEN => {
                let (port_type, index, game_port) = (data[12], data[13], data[14] != 0);
                self.note(format!(
                    "nn init {cookie:08x} port {port_type} index {index} from {from}"
                ));
                self.init_seen(cookie, index, version, port_type, game_port, from);
                let mut reply = data[..14].to_vec();
                reply[7] = NN_INIT_ACK;
                reply.extend_from_slice(&NN_ACK_FILL);
                Some(reply)
            }
            NN_CONNECT_ACK => {
                let index = data.get(13).copied();
                for negotiation in &mut self.negotiations {
                    if negotiation.cookie == cookie && Some(negotiation.index) == index {
                        negotiation.acked = true;
                    }
                }
                self.note(format!("nn connect taken {cookie:08x}"));
                None
            }
            NN_REPORT => {
                self.note(format!(
                    "nn report {cookie:08x} result {}",
                    data.get(14).copied().unwrap_or_default()
                ));
                let mut reply = ack(NN_REPORT_ACK);
                reply[14] = 0;
                Some(reply)
            }
            NN_ADDRESS_CHECK => {
                let mut reply = ack(NN_ADDRESS_REPLY);
                reply[15..19].copy_from_slice(&from.ip().octets());
                reply[19..21].copy_from_slice(&from.port().to_be_bytes());
                Some(reply)
            }
            kind => {
                self.note(format!("nn kind {kind} {cookie:08x}"));
                None
            }
        }
    }

    fn init_seen(
        &mut self,
        cookie: u32,
        index: u8,
        version: u8,
        port_type: u8,
        game_port: bool,
        from: SocketAddrV4,
    ) {
        let at = match self
            .negotiations
            .iter()
            .position(|kept| kept.cookie == cookie && kept.index == index)
        {
            Some(at) => at,
            None => {
                if self.negotiations.len() == MAX_NEGOTIATIONS {
                    self.negotiations.remove(0);
                }
                self.negotiations.push(Negotiation {
                    cookie,
                    index,
                    version,
                    game: None,
                    negotiator: None,
                    peer: None,
                    started: self.now,
                    told: None,
                    sent: None,
                    tries: 0,
                    acked: false,
                });
                self.negotiations.len() - 1
            }
        };
        let negotiation = &mut self.negotiations[at];
        if port_type == NN_FIRST_PORT {
            negotiation.negotiator = Some(from);
        }
        if port_type == NN_GAME_PORT || (port_type == NN_FIRST_PORT && !game_port) {
            negotiation.game = Some(from);
        }
        self.tell(at);
        self.connect(at);
    }

    fn tell(&mut self, at: usize) {
        let now = self.now;
        let negotiation = &mut self.negotiations[at];
        let (Some(game), None) = (negotiation.game, negotiation.peer) else {
            return;
        };
        let due = negotiation
            .told
            .is_none_or(|told| now.saturating_sub(told) >= NN_RETELL_US);
        if !due || now.saturating_sub(negotiation.started) > NN_PARTNER_WAIT_US {
            return;
        }
        negotiation.told = Some(now);
        let (cookie, index) = (negotiation.cookie, negotiation.index);
        self.link.natneg(cookie, index, game.port());
    }

    fn connect(&mut self, at: usize) {
        let now = self.now;
        let negotiation = &mut self.negotiations[at];
        let (Some(to), Some(peer)) = (negotiation.negotiator, negotiation.peer) else {
            return;
        };
        let due = negotiation
            .sent
            .is_none_or(|sent| now.saturating_sub(sent) >= NN_RESEND_US);
        if negotiation.acked || negotiation.tries >= NN_CONNECT_TRIES || !due {
            return;
        }
        negotiation.sent = Some(now);
        negotiation.tries += 1;
        let mut packet = NATNEG_MAGIC.to_vec();
        packet.push(negotiation.version);
        packet.push(NN_CONNECT);
        packet.extend_from_slice(&negotiation.cookie.to_be_bytes());
        packet.extend_from_slice(&peer.ip().octets());
        packet.extend_from_slice(&peer.port().to_be_bytes());
        packet.extend_from_slice(&[NN_GOT_YOUR_DATA, 0]);
        let cookie = negotiation.cookie;
        if self.outgoing.len() == MAX_OUTGOING {
            self.outgoing.remove(0);
        }
        self.outgoing.push((NATNEG_PORT, to, packet));
        self.note(format!("nn connect {cookie:08x} to {to} peer {peer}"));
    }

    fn send_message(&mut self, request: &[u8]) {
        let (Some(&[a, b, c, d, high, low]), Some(data)) = (request.get(..6), request.get(6..))
        else {
            self.note("sb message too short".to_owned());
            return;
        };
        let to = SocketAddrV4::new(Ipv4Addr::new(a, b, c, d), u16::from_be_bytes([high, low]));
        self.note(format!(
            "sb message to {to} {} bytes {:02x?}",
            data.len(),
            &data[..data.len().min(16)]
        ));
        if self.hosts.iter().any(|host| host.public == to) {
            self.deliver(to.port(), data);
        } else {
            self.link.message(to, data);
        }
    }

    fn deliver(&mut self, port: u16, data: &[u8]) {
        let Some(host) = self
            .hosts
            .iter()
            .rev()
            .find(|host| host.public.port() == port)
        else {
            self.note(format!("qr message for {port} has no host"));
            return;
        };
        let mut packet = QR_MAGIC.to_vec();
        packet.push(CLIENT_MESSAGE);
        packet.extend_from_slice(&host.session);
        packet.extend_from_slice(&(self.next as u32).to_be_bytes());
        packet.extend_from_slice(data);
        let to = host.public;
        self.next += 1;
        if self.outgoing.len() == MAX_OUTGOING {
            self.outgoing.remove(0);
        }
        self.outgoing.push((QR_PORT, to, packet));
        self.note(format!("qr message to {to}"));
    }

    fn keep_host(&mut self, from: SocketAddrV4, data: &[u8]) {
        let Some(host) = Host::from_heartbeat(from, data) else {
            return;
        };
        self.hosts.retain(|kept| kept.public != from);
        let shared = LIST_SECRETS.iter().any(|(game, _)| *game == host.game());
        if host.is_leaving() {
            if shared {
                self.link.unhost();
            }
            return;
        }
        if shared {
            self.link.host(host.game(), from.port(), host.keys());
        }
        if self.hosts.len() == MAX_HOSTS {
            self.hosts.remove(0);
        }
        self.hosts.push(host);
    }

    fn qr_available(&mut self, data: &[u8]) -> Vec<u8> {
        let game = data
            .get(5..)
            .and_then(|rest| rest.split(|&byte| byte == 0).next())
            .map(String::from_utf8_lossy)
            .unwrap_or_default()
            .into_owned();
        self.note(format!("available {game}"));
        AVAILABLE_REPLY.to_vec()
    }

    pub fn profile_wanted(&self) -> Option<u64> {
        self.flows
            .iter()
            .filter_map(|flow| flow.login.as_ref())
            .find(|login| login.claim.is_none())
            .and_then(|login| self.login_of(&login.token))
            .map(|login| login.userid.trim().parse().unwrap_or_default())
    }

    pub fn saved_profile(&mut self, saved: Option<u32>) {
        for at in 0..self.flows.len() {
            let Some(token) = self.flows[at]
                .login
                .as_ref()
                .filter(|login| login.claim.is_none())
                .map(|login| login.token.clone())
            else {
                continue;
            };
            let Some(login) = self.login_of(&token) else {
                continue;
            };
            let made = profile_id(self.seed, &login.gsbrcd);
            match saved.filter(|&saved| saved != 0 && saved != made) {
                Some(saved) => self.claim(at, saved, true),
                None => self.claim(at, made, false),
            }
        }
    }

    fn claim(&mut self, at: usize, profile: u32, saved: bool) {
        let call = self.next;
        self.next += 1;
        self.link.claim(call, profile);
        if let Some(login) = self.flows[at].login.as_mut() {
            login.claim = Some(Claim {
                call,
                profile,
                saved,
            });
        }
    }

    fn login_of(&self, token: &str) -> Option<&NasLogin> {
        self.logins.iter().find(|login| login.token == token)
    }

    fn claim_answered(&mut self, call: u64, ours: Option<bool>) {
        let Some(at) = self.flows.iter().position(|flow| {
            flow.login
                .as_ref()
                .and_then(|login| login.claim.as_ref())
                .is_some_and(|claim| claim.call == call)
        }) else {
            return;
        };
        let Some(Claim { profile, saved, .. }) = self.flows[at]
            .login
            .as_mut()
            .and_then(|login| login.claim.take())
        else {
            return;
        };
        let made = self.flows[at]
            .login
            .as_ref()
            .and_then(|login| self.login_of(&login.token))
            .map(|login| profile_id(self.seed, &login.gsbrcd))
            .unwrap_or_default();
        match (ours, saved) {
            (Some(true), _) => self.finish_login(at, profile),
            (Some(false), true) => {
                self.note(format!(
                    "gp profile {profile} belongs to another account, offering {made}"
                ));
                self.claim(at, made, false);
            }
            (Some(false), false) => {
                self.note(format!("gp profile {profile} belongs to another account"));
                let id = self.flows[at]
                    .login
                    .take()
                    .map(|login| login.id)
                    .unwrap_or_default();
                self.flows[at].closing = true;
                self.flows[at].output = gp_message(&[
                    ("error", ""),
                    ("err", "256"),
                    ("fatal", ""),
                    ("errmsg", "This profile belongs to another account."),
                    ("id", &id),
                ]);
            }
            (None, _) => {
                self.note(format!(
                    "gp profile {profile} not bound, the relay gave no answer"
                ));
                self.finish_login(at, made);
            }
        }
    }

    pub fn poll(&mut self, us: u64) {
        self.now = us;
        while let Some((call, ours)) = self.link.claimed() {
            self.claim_answered(call, ours);
        }
        while let Some((from, port, data)) = self.link.messages() {
            self.note(format!("relay message from {from} for {port}"));
            self.deliver(port, &data);
        }
        while let Some((cookie, peer)) = self.link.natnegged() {
            self.note(format!("relay natneg {cookie:08x} peer {peer}"));
            for negotiation in &mut self.negotiations {
                if negotiation.cookie == cookie && negotiation.peer != Some(peer) {
                    negotiation.peer = Some(peer);
                    negotiation.sent = None;
                    negotiation.tries = 0;
                    negotiation.acked = false;
                }
            }
        }
        for at in 0..self.negotiations.len() {
            self.tell(at);
            self.connect(at);
        }
        while let Some((call, hosts)) = self.link.listed() {
            let Some(at) = self
                .flows
                .iter()
                .position(|flow| flow.waiting == Some(call) && flow.listing.is_some())
            else {
                continue;
            };
            self.flows[at].waiting = None;
            if let Some(request) = self.flows[at].listing.take() {
                self.write_list(at, &request, &hosts.unwrap_or_default());
            }
            self.serve_browser(at);
        }
        while let Some((call, body)) = self.link.answer() {
            let Some(flow) = self
                .flows
                .iter_mut()
                .find(|flow| flow.waiting == Some(call))
            else {
                continue;
            };
            flow.waiting = None;
            flow.closing = true;
            flow.output = match body {
                Some(body) => http::response(200, &[("Content-Type", "text/html")], &body),
                None => http::response(502, &[], b""),
            };
        }
    }

    fn serve_http(&mut self, at: usize) {
        if self.flows[at].waiting.is_some() || self.flows[at].closing {
            return;
        }
        let request = match http::parse(&self.flows[at].input) {
            Parsed::Incomplete => return,
            Parsed::Broken => {
                self.flows[at].output = http::response(400, &[], b"");
                self.flows[at].closing = true;
                return;
            }
            Parsed::Done(request, used) => {
                self.flows[at].input.drain(..used);
                request
            }
        };
        self.note(format!(
            "http {} {}{}",
            request.method, request.host, request.target
        ));
        let reply = match request.host.as_str() {
            "conntest.nintendowifi.net" => Some(http::response(
                200,
                &[
                    ("Content-Type", "text/html"),
                    ("X-Organization", "Nintendo"),
                    ("Server", "BigIP"),
                ],
                b"ok",
            )),
            "nas.nintendowifi.net" => Some(self.serve_nas(&request)),
            GTS_HOST if request.method == "GET" => {
                let call = self.next;
                self.next += 1;
                self.link.call(call, &request.target);
                self.flows[at].waiting = Some(call);
                None
            }
            VALIDATION_HOST if request.method == "POST" => Some(http::response(
                200,
                &[("Content-Type", "text/plain")],
                &validation(&request.body),
            )),
            _ => Some(http::response(404, &[], b"")),
        };
        if let Some(reply) = reply {
            self.flows[at].output = reply;
            self.flows[at].closing = true;
        }
    }

    fn serve_nas(&mut self, request: &Request) -> Vec<u8> {
        if request.method != "POST" || !matches!(request.path(), "/ac" | "/pr") {
            return http::response(404, &[], b"");
        }
        let fields: Vec<(String, String)> = http::form(&request.body)
            .into_iter()
            .map(|(name, value)| (name, nas_decode(&value)))
            .collect();
        let field = |name: &str| {
            fields
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
        };
        let action = field("action").to_ascii_lowercase();
        self.note(format!("nas {action} {}", field("gsbrcd")));
        let mut reply: Vec<(&str, String)> = Vec::new();
        if request.path() == "/pr" {
            let words = field("words").split('\t').count();
            let zeros = "0".repeat(words);
            for name in [
                "prwords", "prwordsA", "prwordsC", "prwordsE", "prwordsJ", "prwordsK", "prwordsP",
            ] {
                reply.push((name, zeros.clone()));
            }
            reply.push(("returncd", "000".into()));
        } else {
            match action.as_str() {
                "acctcreate" => {
                    let userid = match field("userid") {
                        userid if !userid.is_empty() => userid,
                        _ => format!("{:013}", self.seed % 10_000_000_000_000),
                    };
                    reply.push(("retry", "0".into()));
                    reply.push(("returncd", "002".into()));
                    reply.push(("userid", userid));
                }
                "login" => {
                    let challenge = self.random_text(ALPHANUMERIC, 8);
                    let token = format!("NDS{}", self.random_text(ALPHANUMERIC, 80));
                    self.logins.push(NasLogin {
                        token: token.clone(),
                        challenge: challenge.clone(),
                        userid: field("userid"),
                        gsbrcd: field("gsbrcd"),
                        served: None,
                    });
                    reply.push(("retry", "0".into()));
                    reply.push(("returncd", "001".into()));
                    reply.push(("locator", "gamespy.com".into()));
                    reply.push(("challenge", challenge));
                    reply.push(("token", token));
                }
                "svcloc" => {
                    reply.push(("retry", "0".into()));
                    reply.push(("returncd", "007".into()));
                    reply.push(("statusdata", "Y".into()));
                    if !field("svc").is_empty() {
                        let token = format!("NDS{}", self.random_text(ALPHANUMERIC, 80));
                        reply.push(("servicetoken", token));
                        reply.push(("svchost", "n/a".into()));
                    }
                }
                _ => {}
            }
        }
        reply.push(("datetime", nas_datetime((self.clock)())));
        let body = reply
            .iter()
            .map(|(name, value)| format!("{name}={}", STANDARD.encode(value).replace('=', "*")))
            .collect::<Vec<_>>()
            .join("&")
            + "\r\n";
        http::response(200, NAS_HEADERS, body.as_bytes())
    }

    fn serve_gamespy(&mut self, at: usize) {
        while let Some(end) = find(&self.flows[at].input, b"\\final\\") {
            let message: Vec<u8> = self.flows[at].input.drain(..end + 7).collect();
            let text = String::from_utf8_lossy(&message[..end]).into_owned();
            let pairs = gp_pairs(&text);
            let Some((command, _)) = pairs.first().cloned() else {
                continue;
            };
            self.note(format!("gp {command}"));
            let value = |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.as_str())
                    .unwrap_or_default()
            };
            let id = value("id").to_owned();
            let reply = match command.as_str() {
                "login" => self.gp_login(at, value("authtoken"), value("challenge"), &id),
                "getprofile" => self.gp_profile(value("profileid"), &id),
                "updatepro" => {
                    self.gp_update(at, &pairs);
                    Vec::new()
                }
                "ka" => gp_message(&[("ka", "")]),
                "logout" => {
                    self.flows[at].closing = true;
                    Vec::new()
                }
                "otherslist" => gp_message(&[("otherslist", ""), ("oldone", "")]),
                _ => Vec::new(),
            };
            self.flows[at].output.extend_from_slice(&reply);
        }
    }

    fn serve_browser(&mut self, at: usize) {
        while !self.flows[at].closing && self.flows[at].waiting.is_none() {
            let input = &self.flows[at].input;
            let Some(&[high, low]) = input.get(..2) else {
                return;
            };
            let available = input.len();
            let length = usize::from(u16::from_be_bytes([high, low]));
            if length < 3 {
                self.browser_broken(at);
                return;
            }
            if available < length {
                return;
            }
            let message: Vec<u8> = self.flows[at].input.drain(..length).collect();
            match message[2] {
                LIST_REQUEST => match ListRequest::parse(&message[3..]) {
                    Some(request) => self.server_list(at, request),
                    None => self.browser_broken(at),
                },
                SEND_MESSAGE => self.send_message(&message[3..]),
                command => self.note(format!("sb command {command}")),
            }
        }
    }

    fn server_list(&mut self, at: usize, request: ListRequest) {
        self.note(format!(
            "sb list {} filter {} fields {}",
            request.game, request.filter, request.fields
        ));
        if !LIST_SECRETS.iter().any(|(game, _)| *game == request.game) {
            self.note(format!("sb no secret for {}", request.game));
            self.flows[at].closing = true;
            return;
        }
        if request.options & NO_SERVER_LIST != 0 {
            self.write_list(at, &request, &[]);
            return;
        }
        let call = self.next;
        self.next += 1;
        self.link.list(call, &request.game);
        self.flows[at].waiting = Some(call);
        self.flows[at].listing = Some(request);
    }

    fn write_list(&mut self, at: usize, request: &ListRequest, remote: &[Host]) {
        let Some(&(_, secret)) = LIST_SECRETS.iter().find(|(game, _)| *game == request.game) else {
            return;
        };
        let key = self.random_text(ALPHANUMERIC, LIST_KEY_LEN).into_bytes();
        let mut reply = vec![LIST_HEADER_RANDOM as u8 ^ 0xEC];
        reply.extend_from_slice(&[0; LIST_HEADER_RANDOM]);
        reply.push(LIST_KEY_LEN as u8 ^ 0xEA);
        reply.extend_from_slice(&key);
        let mut body = self.flows[at].peer.ip().octets().to_vec();
        body.extend_from_slice(&LIST_QUERY_PORT.to_be_bytes());
        if request.options & NO_SERVER_LIST == 0 {
            let keys: Vec<&str> = request
                .fields
                .split('\\')
                .filter(|key| !key.is_empty())
                .take(usize::from(u8::MAX))
                .collect();
            body.push(keys.len() as u8);
            for key in &keys {
                body.push(0);
                body.extend_from_slice(key.as_bytes());
                body.push(0);
            }
            body.push(0);
            if let Some(limit) = request.limit {
                self.note(format!("sb list limit {limit}"));
            }
            let room = request.limit.unwrap_or(usize::MAX);
            let mut sent = 0;
            let remote = remote
                .iter()
                .filter(|host| !self.hosts.iter().any(|own| own.public == host.public));
            for host in self.hosts.iter().chain(remote) {
                if sent < room && host.game() == request.game && host.matches(&request.filter) {
                    host.write_entry(&keys, &mut body);
                    sent += 1;
                }
            }
            if sent > 0 {
                self.note(format!("sb list sends {sent} hosts"));
            }
            body.extend_from_slice(&LIST_END);
        }
        ListCipher::for_list(secret, request.challenge, &key).encrypt(&mut body);
        reply.extend_from_slice(&body);
        self.flows[at].output.extend_from_slice(&reply);
    }

    fn browser_broken(&mut self, at: usize) {
        self.note("sb broken request".to_owned());
        self.flows[at].closing = true;
    }

    fn gp_login(&mut self, at: usize, token: &str, client: &str, id: &str) -> Vec<u8> {
        if self.login_of(token).is_none() {
            self.flows[at].closing = true;
            return gp_message(&[
                ("error", ""),
                ("err", "266"),
                ("fatal", ""),
                (
                    "errmsg",
                    "There was an error validating the pre-authentication.",
                ),
                ("id", id),
            ]);
        }
        self.flows[at].login = Some(GpLogin {
            token: token.to_owned(),
            client: client.to_owned(),
            id: id.to_owned(),
            claim: None,
        });
        Vec::new()
    }

    fn finish_login(&mut self, at: usize, profile: u32) {
        let Some(GpLogin {
            token, client, id, ..
        }) = self.flows[at].login.take()
        else {
            return;
        };
        let Some(login) = self.logins.iter_mut().find(|login| login.token == token) else {
            return;
        };
        login.served = Some(profile);
        let half = md5_hex(login.challenge.as_bytes());
        let nick = unique_nick(&login.userid, &login.gsbrcd);
        let server = self.flows[at].challenge.clone();
        let proof = md5_hex(format!("{half}{PROOF_PAD}{token}{server}{client}{half}").as_bytes());
        let sesskey = format!("{}", 10_000_000 + self.next_random() % 89_999_999);
        let lt = STANDARD
            .encode(self.random_text(ALPHANUMERIC, 16))
            .replace('+', "[")
            .replace('/', "]")
            .replace('=', "_");
        let profile = profile.to_string();
        self.note(format!("gp login as profile {profile}"));
        self.flows[at].profile.clone_from(&profile);
        let reply = gp_message(&[
            ("lc", "2"),
            ("sesskey", &sesskey),
            ("proof", &proof),
            ("userid", &profile),
            ("profileid", &profile),
            ("uniquenick", &nick),
            ("lt", &lt),
            ("id", &id),
        ]);
        self.flows[at].output.extend_from_slice(&reply);
    }

    fn gp_update(&mut self, at: usize, pairs: &[(String, String)]) {
        let profile = self.flows[at].profile.clone();
        if profile.is_empty() {
            return;
        }
        for (key, value) in pairs {
            if key == "firstname" || key == "lastname" {
                self.names
                    .insert((profile.clone(), key.clone()), value.clone());
            }
        }
    }

    fn gp_profile(&mut self, profile: &str, id: &str) -> Vec<u8> {
        let nick = self
            .logins
            .iter()
            .find(|login| {
                login
                    .served
                    .is_some_and(|served| served.to_string() == profile)
            })
            .map(|login| unique_nick(&login.userid, &login.gsbrcd))
            .unwrap_or_default();
        let email = format!("{nick}@nds");
        let sig = format!("{:016x}{:016x}", self.next_random(), self.next_random());
        let mut pairs = vec![
            ("pi", ""),
            ("profileid", profile),
            ("nick", nick.as_str()),
            ("userid", profile),
            ("email", email.as_str()),
            ("sig", sig.as_str()),
            ("uniquenick", nick.as_str()),
            ("pid", "11"),
        ];
        for key in ["firstname", "lastname"] {
            if let Some(value) = self.names.get(&(profile.to_owned(), key.to_owned())) {
                pairs.push((key, value.as_str()));
            }
        }
        pairs.extend([
            ("lon", "0.000000"),
            ("lat", "0.000000"),
            ("loc", ""),
            ("id", id),
        ]);
        gp_message(&pairs)
    }

    fn next_random(&mut self) -> u64 {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        self.random
    }

    fn random_text(&mut self, alphabet: &[u8], length: usize) -> String {
        (0..length)
            .map(|_| {
                let at = usize::try_from(self.next_random() % alphabet.len() as u64).unwrap_or(0);
                char::from(alphabet[at])
            })
            .collect()
    }
}

struct ListRequest {
    game: String,
    challenge: [u8; CHALLENGE_LEN],
    filter: String,
    fields: String,
    options: u32,
    limit: Option<usize>,
}

impl ListRequest {
    fn parse(body: &[u8]) -> Option<Self> {
        let mut rest = body.get(6..)?;
        let _query = take_text(&mut rest)?;
        let game = take_text(&mut rest)?;
        let challenge = rest.get(..CHALLENGE_LEN)?.try_into().ok()?;
        rest = &rest[CHALLENGE_LEN..];
        let filter = take_text(&mut rest)?;
        let fields = take_text(&mut rest)?;
        let options = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?);
        rest = &rest[4..];
        if options & ALTERNATE_SOURCE_IP != 0 {
            rest = rest.get(4..).unwrap_or_default();
        }
        let limit = rest
            .get(..4)
            .filter(|_| options & LIMIT_RESULT_COUNT != 0)
            .map(|count| u32::from_le_bytes(count.try_into().unwrap_or_default()) as usize)
            .filter(|&count| count > 0);
        Some(Self {
            game,
            challenge,
            filter,
            fields,
            options,
            limit,
        })
    }
}

fn take_text(rest: &mut &[u8]) -> Option<String> {
    let end = rest.iter().position(|&byte| byte == 0)?;
    let value = String::from_utf8_lossy(&rest[..end]).into_owned();
    *rest = &rest[end + 1..];
    Some(value)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn gp_pairs(text: &str) -> Vec<(String, String)> {
    let mut parts = text.split('\\');
    parts.next();
    let mut pairs = Vec::new();
    while let Some(key) = parts.next() {
        let value = parts.next().unwrap_or_default();
        pairs.push((key.to_owned(), value.to_owned()));
    }
    pairs
}

fn qr_message(kind: u8, session: &[u8], text: &[u8]) -> Vec<u8> {
    let mut message = QR_MAGIC.to_vec();
    message.push(kind);
    message.extend_from_slice(session);
    if !text.is_empty() {
        message.extend_from_slice(text);
        message.push(0);
    }
    message
}

fn gp_message(pairs: &[(&str, &str)]) -> Vec<u8> {
    let mut text = String::new();
    for (key, value) in pairs {
        text.push('\\');
        text.push_str(key);
        text.push('\\');
        text.push_str(value);
    }
    text.push_str("\\final\\");
    text.into_bytes()
}

fn md5_hex(bytes: &[u8]) -> String {
    Md5::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn unique_nick(userid: &str, gsbrcd: &str) -> String {
    let mut number: u64 = userid.parse().unwrap_or_default();
    let mut digits = Vec::new();
    loop {
        digits.push(NICK_DIGITS[usize::try_from(number % 32).unwrap_or(0)]);
        number /= 32;
        if number == 0 {
            break;
        }
    }
    digits.reverse();
    format!("{}{gsbrcd}", String::from_utf8_lossy(&digits))
}

fn nas_decode(value: &str) -> String {
    let standard: String = value
        .chars()
        .map(|c| match c {
            '*' => '=',
            '?' | '-' => '/',
            '>' | '.' => '+',
            c => c,
        })
        .collect();
    STANDARD
        .decode(standard.as_bytes())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|_| value.to_owned())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn nas_datetime(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or_default();
    let rest = seconds % 86_400;
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let of_era = shifted.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let day_of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}{:02}{:02}{:02}",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Receiver, Sender};

    type Listed = (u64, Option<Vec<Host>>);

    struct Recorder {
        calls: Sender<(u64, String)>,
        answers: Receiver<(u64, Option<Vec<u8>>)>,
        claims: Sender<(u64, u32)>,
        claimed: Receiver<(u64, Option<bool>)>,
        hosting: Sender<Option<(String, u16)>>,
        lists: Sender<(u64, String)>,
        listed: Receiver<Listed>,
        sent: Sender<(SocketAddrV4, Vec<u8>)>,
        heard: Receiver<(Ipv4Addr, u16, Vec<u8>)>,
        sides: Sender<(u32, u8, u16)>,
        peers: Receiver<(u32, SocketAddrV4)>,
    }

    impl GtsLink for Recorder {
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

        fn host(&mut self, game: &str, port: u16, _: &[(String, String)]) {
            self.hosting.send(Some((game.to_owned(), port))).ok();
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
            self.sent.send((to, data.to_vec())).ok();
        }

        fn messages(&mut self) -> Option<(Ipv4Addr, u16, Vec<u8>)> {
            self.heard.try_recv().ok()
        }

        fn natneg(&mut self, cookie: u32, index: u8, port: u16) {
            self.sides.send((cookie, index, port)).ok();
        }

        fn natnegged(&mut self) -> Option<(u32, SocketAddrV4)> {
            self.peers.try_recv().ok()
        }
    }

    struct Far {
        calls: Receiver<(u64, String)>,
        answers: Sender<(u64, Option<Vec<u8>>)>,
        claims: Receiver<(u64, u32)>,
        claimed: Sender<(u64, Option<bool>)>,
        hosting: Receiver<Option<(String, u16)>>,
        lists: Receiver<(u64, String)>,
        listed: Sender<Listed>,
        sent: Receiver<(SocketAddrV4, Vec<u8>)>,
        heard: Sender<(Ipv4Addr, u16, Vec<u8>)>,
        sides: Receiver<(u32, u8, u16)>,
        peers: Sender<(u32, SocketAddrV4)>,
    }

    impl Far {
        fn bind(&self, services: &mut Services, ours: Option<bool>) -> u32 {
            let (call, profile) = self.claims.try_recv().expect("a claim reaches the relay");
            self.claimed.send((call, ours)).unwrap();
            services.poll(0);
            profile
        }
    }

    fn services() -> (Services, Far) {
        let (calls, called) = mpsc::channel();
        let (answer, answers) = mpsc::channel();
        let (claims, claim_rx) = mpsc::channel();
        let (claimed_tx, claimed) = mpsc::channel();
        let (hosting, hosted) = mpsc::channel();
        let (lists, asked) = mpsc::channel();
        let (listed_tx, listed) = mpsc::channel();
        let (sent, sent_rx) = mpsc::channel();
        let (heard_tx, heard) = mpsc::channel();
        let (sides, sides_rx) = mpsc::channel();
        let (peers_tx, peers) = mpsc::channel();
        let services = Services::new(
            42,
            Box::new(Recorder {
                calls,
                answers,
                claims,
                claimed,
                hosting,
                lists,
                listed,
                sent,
                heard,
                sides,
                peers,
            }),
        )
        .with_clock(|| 1_762_430_400);
        (
            services,
            Far {
                calls: called,
                answers: answer,
                claims: claim_rx,
                claimed: claimed_tx,
                hosting: hosted,
                lists: asked,
                listed: listed_tx,
                sent: sent_rx,
                heard: heard_tx,
                sides: sides_rx,
                peers: peers_tx,
            },
        )
    }

    fn gp_login(services: &mut Services, gsbrcd: &str) -> u64 {
        let reply = exchange(
            services,
            HTTP_PORT,
            &nas_post(&[
                ("action", "login"),
                ("userid", "1234567890123"),
                ("gsbrcd", gsbrcd),
            ]),
        );
        let token = nas_field(&reply, "token");
        let id = services.open(GPCM_PORT).unwrap();
        services.take_output(id, usize::MAX);
        services.input(
            id,
            format!("\\login\\\\challenge\\c\\authtoken\\{token}\\id\\1\\final\\").as_bytes(),
        );
        assert!(services.take_output(id, usize::MAX).is_empty());
        assert_eq!(services.profile_wanted(), Some(1_234_567_890_123));
        id
    }

    fn served(services: &mut Services, id: u64) -> Option<String> {
        let answer = String::from_utf8(services.take_output(id, usize::MAX)).unwrap();
        let pairs = gp_pairs(answer.trim_end_matches("\\final\\"));
        pairs
            .iter()
            .find(|(key, _)| key == "profileid")
            .map(|(_, value)| value.clone())
    }

    fn exchange(services: &mut Services, port: u16, request: &[u8]) -> String {
        let id = services.open(port).unwrap();
        services.take_output(id, usize::MAX);
        services.input(id, request);
        String::from_utf8(services.take_output(id, usize::MAX)).unwrap()
    }

    fn nas_field(reply: &str, name: &str) -> String {
        let body = reply.split("\r\n\r\n").nth(1).unwrap().trim_end();
        let value = body
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
            .unwrap();
        String::from_utf8(STANDARD.decode(value.replace('*', "=")).unwrap()).unwrap()
    }

    fn nas_post(fields: &[(&str, &str)]) -> Vec<u8> {
        let body = fields
            .iter()
            .map(|(name, value)| format!("{name}={}", STANDARD.encode(value).replace('=', "*")))
            .collect::<Vec<_>>()
            .join("&");
        format!(
            "POST /ac HTTP/1.1\r\nHost: nas.nintendowifi.net\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn only_the_hosts_the_stand_ins_answer_are_served() {
        assert!(is_served("nas.nintendowifi.net"));
        assert!(is_served("GPCM.gs.nintendowifi.net."));
        assert!(is_served("pokemondpds.available.gs.nintendowifi.net"));
        assert!(is_served(GTS_HOST));
        assert!(is_served("pokemondpds.master.gs.nintendowifi.net"));
        assert!(!is_served("pokemondpds.ms4.gs.nintendowifi.net"));
        assert!(is_served("MarioKartDS.ms17.gs.nintendowifi.net."));
        assert!(!is_served("mariokartds.msx.gs.nintendowifi.net"));
        assert!(!is_served("mariokartds.ms.gs.nintendowifi.net"));
        assert!(!is_served("ms17.gs.nintendowifi.net"));
        assert!(is_served("mariokartds.natneg1.gs.nintendowifi.net"));
        assert!(is_served("natneg3.gs.nintendowifi.net"));
        assert!(!is_served("natneg4.gs.nintendowifi.net"));
        assert!(is_nintendo("pokemondpds.ms4.gs.nintendowifi.net"));
        assert!(!is_nintendo("example.net"));
        assert!(is_served(VALIDATION_HOST));
        assert!(is_nintendo(VALIDATION_HOST));
        assert!(!is_nintendo("www.nintendo.co.jp"));
    }

    #[test]
    fn the_validation_stand_in_passes_every_pokemon_with_a_blank_signature() {
        let mut gts = b"NDStoken\0\x00\x01".to_vec();
        gts.extend_from_slice(&[7; 2 * VALIDATED_BYTES]);
        let reply = validation(&gts);
        assert_eq!(reply.len(), 1 + 2 * 4 + SIGNATURE_BYTES);
        assert!(reply.iter().all(|&byte| byte == 0));
        assert_eq!(
            validation(b"t\0\x00\x04").len(),
            1 + 6 * 4 + SIGNATURE_BYTES
        );
        assert_eq!(validation(b"").len(), 1 + 6 * 4 + SIGNATURE_BYTES);

        let (mut services, _) = services();
        let mut post = format!(
            "POST /pokemon/validate HTTP/1.1\r\nHost: {VALIDATION_HOST}\r\nContent-Length: {}\r\n\r\n",
            gts.len()
        )
        .into_bytes();
        post.extend_from_slice(&gts);
        let reply = exchange(&mut services, HTTP_PORT, &post);
        assert!(reply.starts_with("HTTP/1.1 200 OK"));
        assert!(reply.contains("Content-Length: 137\r\n"));
    }

    #[test]
    fn conntest_answers_ok_as_nintendo() {
        let (mut services, _) = services();
        let reply = exchange(
            &mut services,
            HTTP_PORT,
            b"GET / HTTP/1.1\r\nHost: conntest.nintendowifi.net\r\n\r\n",
        );
        assert!(reply.starts_with("HTTP/1.1 200 OK"));
        assert!(reply.contains("X-Organization: Nintendo"));
        assert!(reply.ends_with("\r\n\r\nok"));
    }

    #[test]
    fn a_nas_login_gives_a_token_the_profile_login_accepts_with_a_correct_proof() {
        let (mut services, far) = services();
        let reply = exchange(
            &mut services,
            HTTP_PORT,
            &nas_post(&[
                ("action", "login"),
                ("userid", "1234567890123"),
                ("gsbrcd", "CPUJabcd"),
            ]),
        );
        assert_eq!(nas_field(&reply, "returncd"), "001");
        assert_eq!(nas_field(&reply, "datetime"), "20251106120000");
        let token = nas_field(&reply, "token");
        let challenge = nas_field(&reply, "challenge");
        assert!(token.starts_with("NDS") && token.len() == 83);
        assert_eq!(challenge.len(), 8);

        let id = services.open(GPCM_PORT).unwrap();
        let greeting = String::from_utf8(services.take_output(id, usize::MAX)).unwrap();
        let server = gp_pairs(greeting.trim_end_matches("\\final\\"))[1]
            .1
            .clone();
        assert_eq!(server.len(), 10);
        let client = "c".repeat(32);
        services.input(
            id,
            format!("\\login\\\\challenge\\{client}\\authtoken\\{token}\\id\\1\\final\\")
                .as_bytes(),
        );
        services.saved_profile(None);
        assert_eq!(
            far.bind(&mut services, Some(true)),
            profile_id(42, "CPUJabcd")
        );
        let answer = String::from_utf8(services.take_output(id, usize::MAX)).unwrap();
        let pairs = gp_pairs(answer.trim_end_matches("\\final\\"));
        let get = |name: &str| pairs.iter().find(|(key, _)| key == name).unwrap().1.clone();
        let half = md5_hex(challenge.as_bytes());
        assert_eq!(
            get("proof"),
            md5_hex(format!("{half}{PROOF_PAD}{token}{server}{client}{half}").as_bytes())
        );
        assert_eq!(get("profileid"), profile_id(42, "CPUJabcd").to_string());
        assert_eq!(
            get("uniquenick"),
            format!("{}CPUJabcd", unique_nick("1234567890123", ""))
        );
    }

    #[test]
    fn a_lastname_the_game_saves_comes_back_in_its_profile() {
        let (mut services, far) = services();
        let id = gp_login(&mut services, "IPKDabcd");
        services.saved_profile(None);
        far.bind(&mut services, Some(true));
        services.take_output(id, usize::MAX);
        let profile = profile_id(42, "IPKDabcd");
        let get = format!("\\getprofile\\\\sesskey\\1\\profileid\\{profile}\\id\\2\\final\\");
        services.input(id, get.as_bytes());
        let before = String::from_utf8(services.take_output(id, usize::MAX)).unwrap();
        assert!(!before.contains("\\lastname\\"));

        services.input(
            id,
            b"\\updatepro\\\\sesskey\\1\\lastname\\5fq09csjnADAJ0i0fdc0\\final\\",
        );
        assert!(services.take_output(id, usize::MAX).is_empty());
        services.input(id, get.as_bytes());
        let after = String::from_utf8(services.take_output(id, usize::MAX)).unwrap();
        assert!(after.contains("\\pid\\11\\lastname\\5fq09csjnADAJ0i0fdc0\\lon\\"));
        assert!(after.ends_with("\\id\\2\\final\\"));
    }

    #[test]
    fn the_profile_the_game_holds_is_served_once_the_relay_binds_it() {
        let (mut services, far) = services();
        let id = gp_login(&mut services, "AMCEabcd");
        services.saved_profile(Some(777));
        assert_eq!(services.profile_wanted(), None);
        assert_eq!(far.bind(&mut services, Some(true)), 777);
        assert_eq!(served(&mut services, id).as_deref(), Some("777"));
        assert!(
            services
                .take_log()
                .contains(&"gp login as profile 777".to_owned())
        );
        let get = "\\getprofile\\\\sesskey\\1\\profileid\\777\\id\\2\\final\\";
        services.input(id, get.as_bytes());
        let profile = String::from_utf8(services.take_output(id, usize::MAX)).unwrap();
        assert!(profile.contains("\\uniquenick\\"));
        assert!(!profile.contains("\\uniquenick\\\\"));
    }

    #[test]
    fn a_held_profile_of_another_account_gets_the_made_one_and_a_log_line() {
        let (mut services, far) = services();
        let id = gp_login(&mut services, "AMCEabcd");
        services.saved_profile(Some(777));
        assert_eq!(far.bind(&mut services, Some(false)), 777);
        assert_eq!(services.take_output(id, usize::MAX), Vec::<u8>::new());
        let made = profile_id(42, "AMCEabcd");
        assert_eq!(far.bind(&mut services, Some(true)), made);
        assert_eq!(served(&mut services, id), Some(made.to_string()));
        assert!(services.take_log().contains(&format!(
            "gp profile 777 belongs to another account, offering {made}"
        )));
    }

    #[test]
    fn a_made_profile_of_another_account_refuses_the_login() {
        let (mut services, far) = services();
        let id = gp_login(&mut services, "AMCEabcd");
        services.saved_profile(Some(0));
        assert_eq!(
            far.bind(&mut services, Some(false)),
            profile_id(42, "AMCEabcd")
        );
        let answer = String::from_utf8(services.take_output(id, usize::MAX)).unwrap();
        assert!(answer.starts_with("\\error\\\\err\\256\\fatal\\"));
        assert!(answer.ends_with("\\id\\1\\final\\"));
        assert!(services.finished(id));
        assert!(far.claims.try_recv().is_err());
    }

    #[test]
    fn a_relay_that_binds_nothing_gets_the_made_profile() {
        let (mut services, far) = services();
        let id = gp_login(&mut services, "AMCEabcd");
        services.saved_profile(Some(777));
        far.bind(&mut services, None);
        assert_eq!(
            served(&mut services, id),
            Some(profile_id(42, "AMCEabcd").to_string())
        );
        assert!(far.claims.try_recv().is_err());
    }

    #[test]
    fn an_unknown_token_is_refused_and_closes() {
        let (mut services, _) = services();
        let id = services.open(GPCM_PORT).unwrap();
        services.take_output(id, usize::MAX);
        services.input(
            id,
            b"\\login\\\\challenge\\x\\authtoken\\NDSnope\\id\\1\\final\\",
        );
        let answer = String::from_utf8(services.take_output(id, usize::MAX)).unwrap();
        assert!(answer.starts_with("\\error\\\\err\\266"));
        assert!(services.finished(id));
    }

    #[test]
    fn a_profile_is_one_number_per_account_and_cartridge() {
        assert_eq!(profile_id(1, "CPUJ"), profile_id(1, "CPUJ"));
        assert_ne!(profile_id(1, "CPUJ"), profile_id(2, "CPUJ"));
        assert_ne!(profile_id(1, "CPUJ"), profile_id(1, "IPKJ"));
        for seed in 0..1000 {
            let id = profile_id(seed, "IPKE");
            assert!(id > 0 && id < 0x7FFF_FFFF);
        }
    }

    #[test]
    fn a_heartbeat_gets_a_challenge_and_its_answer_registers_the_game() {
        let (mut services, _) = services();
        let from = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 0xC350);
        let mut beat = vec![HEARTBEAT, 1, 2, 3, 4];
        beat.extend_from_slice(b"localip0\x0010.0.0.2\0gamename\0pokemondpds\0\0");
        let challenge = services.datagram(QR_PORT, from, &beat).unwrap();
        assert_eq!(challenge[..7], [0xFE, 0xFD, CHALLENGE, 1, 2, 3, 4]);
        let text = &challenge[7..];
        assert_eq!(text.len(), 6 + 2 + 8 + 4 + 1);
        assert_eq!(&text[6..], b"000A000002C350\0");

        let mut answer = vec![CHALLENGE, 1, 2, 3, 4];
        answer.extend_from_slice(b"c29tZQA=\0");
        assert_eq!(
            services.datagram(QR_PORT, from, &answer),
            Some(vec![0xFE, 0xFD, REGISTERED, 1, 2, 3, 4])
        );
        assert_eq!(
            services.take_log(),
            vec!["qr heartbeat".to_owned(), "qr registered".to_owned()]
        );
    }

    #[test]
    fn the_available_check_says_the_game_is_up() {
        let (mut services, _) = services();
        let mut asked = vec![AVAILABLE, 0, 0, 0, 0];
        asked.extend_from_slice(b"pokemondpds\0");
        let from = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 5000);
        assert_eq!(
            services.datagram(QR_PORT, from, &asked),
            Some(AVAILABLE_REPLY.to_vec())
        );
        assert_eq!(services.datagram(QR_PORT, from, &[3, 0]), None);
        assert_eq!(services.datagram(GPCM_PORT, from, &asked), None);
        assert_eq!(
            services.take_log(),
            vec!["available pokemondpds".to_owned()]
        );
    }

    fn list_request(game: &str, filter: &str, fields: &str, options: u32) -> Vec<u8> {
        let mut body = vec![0, 1, 3, 0, 0, 0, 0];
        for text in [game, game] {
            body.extend_from_slice(text.as_bytes());
            body.push(0);
        }
        body.extend_from_slice(b"Abc12345");
        for text in [filter, fields] {
            body.extend_from_slice(text.as_bytes());
            body.push(0);
        }
        body.extend_from_slice(&options.to_be_bytes());
        let mut request = u16::try_from(body.len() + 2)
            .unwrap()
            .to_be_bytes()
            .to_vec();
        request.extend_from_slice(&body);
        request
    }

    #[test]
    fn the_server_browser_takes_a_list_request_in_pieces() {
        let (mut services, _) = services();
        let id = services.open(SB_PORT).unwrap();
        let request = list_request(
            "mariokartds",
            "dwc_mver = 90 and dwc_pid != 603819417",
            "\\dwc_pid\\dwc_mtype",
            0,
        );
        let (head, tail) = request.split_at(9);
        services.input(id, head);
        assert!(services.take_log().is_empty());
        services.input(id, tail);
        assert_eq!(
            services.take_log(),
            vec![
                "sb list mariokartds filter dwc_mver = 90 and dwc_pid != 603819417 \
                 fields \\dwc_pid\\dwc_mtype"
                    .to_owned()
            ]
        );
        assert!(!services.finished(id));
    }

    fn read_list(reply: &[u8], challenge: [u8; CHALLENGE_LEN]) -> Vec<u8> {
        let random = usize::from(reply[0] ^ 0xEC);
        let key_len = usize::from(reply[random + 1] ^ 0xEA);
        let key = &reply[random + 2..random + 2 + key_len];
        let mut body = reply[random + 2 + key_len..].to_vec();
        ListCipher::for_list(b"yeJ3x8", challenge, key).decrypt(&mut body);
        body
    }

    #[test]
    fn mario_kart_gets_an_empty_list_it_can_decrypt() {
        let (mut services, far) = services();
        let id = services.open(SB_PORT).unwrap();
        services.peer(id, SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 50000));
        services.input(
            id,
            &list_request("mariokartds", "", "\\dwc_pid\\dwc_mtype", 0),
        );
        assert!(services.take_output(id, 4096).is_empty());
        let (call, game) = far
            .lists
            .try_recv()
            .expect("the list request reaches the relay");
        assert_eq!(game, "mariokartds");
        far.listed.send((call, Some(Vec::new()))).unwrap();
        services.poll(0);
        let reply = services.take_output(id, 4096);
        assert_eq!(reply[0], 5 ^ 0xEC);
        assert_eq!(reply[6], 13 ^ 0xEA);
        let mut list = vec![10, 77, 1, 1, 0x19, 0x64, 2];
        list.extend_from_slice(b"\0dwc_pid\0\0dwc_mtype\0\0");
        list.extend_from_slice(&LIST_END);
        assert_eq!(read_list(&reply, *b"Abc12345"), list);
        assert!(!services.finished(id));
    }

    fn mario_kart_heartbeat(state: &str) -> Vec<u8> {
        let mut beat = vec![HEARTBEAT, 9, 8, 7, 6];
        for (key, value) in [
            ("localip0", "10.77.1.1"),
            ("localport", "56400"),
            ("natneg", "1"),
            ("statechanged", state),
            ("gamename", "mariokartds"),
            ("numplayers", "0"),
            ("dwc_pid", "603819417"),
        ] {
            beat.extend_from_slice(key.as_bytes());
            beat.push(0);
            beat.extend_from_slice(value.as_bytes());
            beat.push(0);
        }
        beat.push(0);
        beat
    }

    fn list_with(
        services: &mut Services,
        far: &Far,
        filter: &str,
        remote: Option<Vec<Host>>,
    ) -> Vec<u8> {
        let id = services.open(SB_PORT).unwrap();
        services.peer(id, SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 50000));
        services.input(
            id,
            &list_request("mariokartds", filter, "\\dwc_pid\\numplayers", 0),
        );
        let (call, _) = far
            .lists
            .try_recv()
            .expect("the list request reaches the relay");
        far.listed.send((call, remote)).unwrap();
        services.poll(0);
        let reply = services.take_output(id, 4096);
        read_list(&reply, *b"Abc12345")
    }

    fn list_for(services: &mut Services, far: &Far, filter: &str) -> Vec<u8> {
        list_with(services, far, filter, Some(Vec::new()))
    }

    fn list_head() -> Vec<u8> {
        let mut list = vec![10, 77, 1, 1, 0x19, 0x64, 2];
        list.extend_from_slice(b"\0dwc_pid\0\0numplayers\0\0");
        list
    }

    fn other_host(port: u16, pid: &str) -> Host {
        Host::listed(
            SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 2), port),
            vec![
                ("gamename".to_owned(), "mariokartds".to_owned()),
                ("dwc_pid".to_owned(), pid.to_owned()),
                ("numplayers".to_owned(), "1".to_owned()),
            ],
        )
    }

    #[test]
    fn a_heartbeat_shares_its_host_through_the_link_until_it_leaves() {
        let (mut services, far) = services();
        let ds = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 56400);
        services.datagram(QR_PORT, ds, &mario_kart_heartbeat("1"));
        assert_eq!(
            far.hosting.try_recv(),
            Ok(Some(("mariokartds".to_owned(), 56400)))
        );
        services.datagram(QR_PORT, ds, &mario_kart_heartbeat("2"));
        assert_eq!(far.hosting.try_recv(), Ok(None));
        assert!(far.hosting.try_recv().is_err());
    }

    #[test]
    fn a_list_adds_the_relay_hosts_the_filter_lets_through() {
        let (mut services, far) = services();
        let ds = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 56400);
        services.datagram(QR_PORT, ds, &mario_kart_heartbeat("1"));
        let mut ours_again = other_host(56400, "603819417");
        ours_again.public = ds;
        let remote = vec![other_host(56401, "5"), other_host(56402, "6"), ours_again];
        let mut list = list_head();
        list.extend_from_slice(&[0x72, 10, 77, 1, 2, 0xDC, 0x51, 10, 77, 1, 2, 0xDC, 0x51]);
        list.extend_from_slice(b"\xFF5\0\xFF1\0");
        list.extend_from_slice(&LIST_END);
        assert_eq!(
            list_with(
                &mut services,
                &far,
                "dwc_pid != 603819417 and dwc_pid != 6",
                Some(remote.clone())
            ),
            list
        );
        let everyone = list_with(&mut services, &far, "", Some(remote));
        assert_eq!(
            everyone.len(),
            list_head().len() + 27 + 2 * 19 + LIST_END.len()
        );
    }

    #[test]
    fn a_list_stops_at_the_result_count_the_browser_asks_for() {
        let (mut services, far) = services();
        let ds = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 56400);
        services.datagram(QR_PORT, ds, &mario_kart_heartbeat("1"));
        services.take_log();
        let id = services.open(SB_PORT).unwrap();
        services.peer(id, SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 50000));
        let mut request = list_request(
            "mariokartds",
            "",
            "\\dwc_pid\\numplayers",
            LIMIT_RESULT_COUNT | ALTERNATE_SOURCE_IP,
        );
        request.extend_from_slice(&[10, 77, 1, 1]);
        request.extend_from_slice(&1u32.to_le_bytes());
        let length = u16::try_from(request.len()).unwrap().to_be_bytes();
        request[..2].copy_from_slice(&length);
        services.input(id, &request);
        let (call, _) = far.lists.try_recv().unwrap();
        let remote = vec![other_host(56401, "5"), other_host(56402, "6")];
        far.listed.send((call, Some(remote))).unwrap();
        services.poll(0);
        let list = read_list(&services.take_output(id, 4096), *b"Abc12345");
        assert_eq!(list.len(), list_head().len() + 27 + LIST_END.len());
        assert!(services.take_log().contains(&"sb list limit 1".to_owned()));
    }

    fn browser_message(to: SocketAddrV4, data: &[u8]) -> Vec<u8> {
        let mut request = vec![0, 0, SEND_MESSAGE];
        request.extend_from_slice(&to.ip().octets());
        request.extend_from_slice(&to.port().to_be_bytes());
        request.extend_from_slice(data);
        let length = u16::try_from(request.len()).unwrap().to_be_bytes();
        request[..2].copy_from_slice(&length);
        request
    }

    fn client_message_parts(packet: &[u8]) -> (&[u8], &[u8]) {
        assert_eq!(packet[..3], [0xFE, 0xFD, CLIENT_MESSAGE]);
        (&packet[3..7], &packet[11..])
    }

    #[test]
    fn a_browser_message_to_a_far_host_goes_through_the_link() {
        let (mut services, far) = services();
        let id = services.open(SB_PORT).unwrap();
        let host = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 2), 56401);
        services.input(id, &browser_message(host, b"\xFD\xFCcookie"));
        assert_eq!(far.sent.try_recv(), Ok((host, b"\xFD\xFCcookie".to_vec())));
        assert!(services.take_datagrams(QR_PORT).is_empty());
        assert!(services.take_output(id, 64).is_empty());
        assert!(!services.finished(id));
    }

    #[test]
    fn a_browser_message_to_our_own_host_reaches_the_ds_at_once() {
        let (mut services, far) = services();
        let ds = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 56400);
        services.datagram(QR_PORT, ds, &mario_kart_heartbeat("1"));
        let id = services.open(SB_PORT).unwrap();
        services.input(id, &browser_message(ds, b"hello"));
        assert!(far.sent.try_recv().is_err());
        let sent = services.take_datagrams(QR_PORT);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, ds);
        assert_eq!(
            client_message_parts(&sent[0].1),
            (&[9, 8, 7, 6][..], &b"hello"[..])
        );
        assert!(services.take_datagrams(QR_PORT).is_empty());
    }

    #[test]
    fn a_relay_message_reaches_the_host_on_its_port_and_the_ack_is_taken() {
        let (mut services, far) = services();
        let ds = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 56400);
        services.datagram(QR_PORT, ds, &mario_kart_heartbeat("1"));
        let other = Ipv4Addr::new(10, 77, 1, 2);
        far.heard.send((other, 56400, b"SBCM".to_vec())).unwrap();
        far.heard.send((other, 56499, b"lost".to_vec())).unwrap();
        services.poll(0);
        let sent = services.take_datagrams(QR_PORT);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, ds);
        let (session, data) = client_message_parts(&sent[0].1);
        assert_eq!((session, data), (&[9, 8, 7, 6][..], &b"SBCM"[..]));
        let mut ack = vec![CLIENT_MESSAGE_ACK];
        ack.extend_from_slice(&sent[0].1[3..11]);
        assert_eq!(services.datagram(QR_PORT, ds, &ack), None);
        assert!(
            services
                .take_log()
                .iter()
                .any(|line| line == "qr message taken")
        );
    }

    #[test]
    fn a_short_browser_message_is_noted_and_the_browser_stays_open() {
        let (mut services, far) = services();
        let id = services.open(SB_PORT).unwrap();
        services.input(id, &[0, 6, SEND_MESSAGE, 10, 77, 1]);
        assert!(far.sent.try_recv().is_err());
        assert!(!services.finished(id));
        assert!(
            services
                .take_log()
                .iter()
                .any(|line| line == "sb message too short")
        );
    }

    fn nn_packet(kind: u8, rest: &[u8]) -> Vec<u8> {
        let mut packet = NATNEG_MAGIC.to_vec();
        packet.extend_from_slice(&[2, kind, 0x12, 0x34, 0x56, 0x78]);
        packet.extend_from_slice(rest);
        packet
    }

    fn nn_init(port_type: u8, index: u8, game_port: bool) -> Vec<u8> {
        let mut rest = vec![port_type, index, u8::from(game_port), 10, 77, 1, 1, 0, 0];
        rest.extend_from_slice(b"mariokartds\0");
        nn_packet(NN_INIT, &rest)
    }

    #[test]
    fn natneg_inits_are_acked_and_the_game_port_goes_to_the_relay_once() {
        let (mut services, far) = services();
        let game = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 56400);
        let negotiator = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 50123);
        let ack = services
            .datagram(NATNEG_PORT, game, &nn_init(0, 1, true))
            .unwrap();
        let mut expected = nn_packet(NN_INIT_ACK, &[0, 1]);
        expected.extend_from_slice(&NN_ACK_FILL);
        assert_eq!(ack, expected);
        for port_type in 1..=3 {
            let ack = services
                .datagram(NATNEG_PORT, negotiator, &nn_init(port_type, 1, true))
                .unwrap();
            assert_eq!(
                (ack.len(), ack[7], ack[12]),
                (NN_ACK_LEN, NN_INIT_ACK, port_type)
            );
        }
        services.datagram(NATNEG_PORT, game, &nn_init(0, 1, true));
        assert_eq!(far.sides.try_recv(), Ok((0x1234_5678, 1, 56400)));
        assert!(far.sides.try_recv().is_err());
        assert!(services.take_datagrams(NATNEG_PORT).is_empty());

        let peer = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 2), 50000);
        far.peers.send((0x1234_5678, peer)).unwrap();
        far.peers.send((0x1234_5678, peer)).unwrap();
        services.poll(0);
        let connect = nn_packet(NN_CONNECT, &[10, 77, 1, 2, 0xC3, 0x50, NN_GOT_YOUR_DATA, 0]);
        assert_eq!(
            services.take_datagrams(NATNEG_PORT),
            vec![(negotiator, connect)]
        );
        assert!(services.take_datagrams(QR_PORT).is_empty());
        assert_eq!(
            services.datagram(NATNEG_PORT, negotiator, &nn_packet(NN_CONNECT_ACK, &[0; 9])),
            None
        );
    }

    #[test]
    fn a_peer_heard_before_the_negotiator_socket_connects_once_it_speaks() {
        let (mut services, far) = services();
        let negotiator = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 50123);
        let peer = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 2), 56400);
        services.datagram(NATNEG_PORT, negotiator, &nn_init(2, 0, false));
        assert!(far.sides.try_recv().is_err());
        services.datagram(NATNEG_PORT, negotiator, &nn_init(1, 0, false));
        assert_eq!(far.sides.try_recv(), Ok((0x1234_5678, 0, 50123)));
        far.peers.send((0x1234_5678, peer)).unwrap();
        far.peers.send((0x0BAD_C0DE, peer)).unwrap();
        services.poll(0);
        let sent = services.take_datagrams(NATNEG_PORT);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, negotiator);
        assert_eq!(sent[0].1[12..18], [10, 77, 1, 2, 0xDC, 0x50]);
    }

    fn negotiating(services: &mut Services) -> SocketAddrV4 {
        let game = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 56400);
        let negotiator = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 50123);
        services.datagram(NATNEG_PORT, game, &nn_init(0, 1, true));
        for port_type in 1..=3 {
            services.datagram(NATNEG_PORT, negotiator, &nn_init(port_type, 1, true));
        }
        negotiator
    }

    fn connect_ack(index: u8) -> Vec<u8> {
        let mut ack = nn_packet(NN_CONNECT_ACK, &[0, index]);
        ack.resize(NN_ACK_LEN, 0);
        ack
    }

    #[test]
    fn a_lost_connect_goes_again_until_the_ds_acks_it() {
        let (mut services, far) = services();
        let negotiator = negotiating(&mut services);
        let peer = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 2), 50000);
        far.peers.send((0x1234_5678, peer)).unwrap();
        services.poll(1_000);
        let first = services.take_datagrams(NATNEG_PORT);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].0, negotiator);
        services.poll(1_000 + NN_RESEND_US / 2);
        assert!(services.take_datagrams(NATNEG_PORT).is_empty());
        services.poll(1_000 + NN_RESEND_US);
        assert_eq!(services.take_datagrams(NATNEG_PORT), first);
        services.datagram(NATNEG_PORT, negotiator, &connect_ack(0));
        services.poll(1_000 + 2 * NN_RESEND_US);
        assert_eq!(services.take_datagrams(NATNEG_PORT), first);
        services.datagram(NATNEG_PORT, negotiator, &connect_ack(1));
        for round in 3..10 {
            services.poll(1_000 + round * NN_RESEND_US);
        }
        assert!(services.take_datagrams(NATNEG_PORT).is_empty());
    }

    #[test]
    fn a_connect_nobody_acks_stops_after_its_tries() {
        let (mut services, far) = services();
        negotiating(&mut services);
        let peer = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 2), 50000);
        far.peers.send((0x1234_5678, peer)).unwrap();
        let mut sent = 0;
        for round in 0..100 {
            services.poll(round * NN_RESEND_US);
            sent += services.take_datagrams(NATNEG_PORT).len();
        }
        assert_eq!(sent, usize::from(NN_CONNECT_TRIES));
    }

    #[test]
    fn the_relay_hears_a_negotiation_again_until_its_peer_comes() {
        let (mut services, far) = services();
        negotiating(&mut services);
        assert_eq!(far.sides.try_recv(), Ok((0x1234_5678, 1, 56400)));
        services.poll(NN_RETELL_US / 2);
        assert!(far.sides.try_recv().is_err());
        services.poll(NN_RETELL_US);
        assert_eq!(far.sides.try_recv(), Ok((0x1234_5678, 1, 56400)));
        assert!(far.sides.try_recv().is_err());
        let peer = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 2), 50000);
        far.peers.send((0x1234_5678, peer)).unwrap();
        for round in 2..10 {
            services.poll(round * NN_RETELL_US);
        }
        assert!(far.sides.try_recv().is_err());
    }

    #[test]
    fn a_negotiation_with_no_peer_stops_telling_the_relay_after_the_partner_wait() {
        let (mut services, far) = services();
        negotiating(&mut services);
        let mut told = 0;
        let mut round = 0;
        while round * NN_RETELL_US <= 2 * NN_PARTNER_WAIT_US {
            services.poll(round * NN_RETELL_US);
            told += far.sides.try_iter().count();
            round += 1;
        }
        assert_eq!(told as u64, 1 + NN_PARTNER_WAIT_US / NN_RETELL_US);
    }

    #[test]
    fn a_natneg_report_gets_its_ack_and_strangers_get_nothing() {
        let (mut services, _far) = services();
        let from = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 50123);
        let mut report = vec![0, 1, 1];
        report.extend_from_slice(&[0; 60]);
        let ack = services
            .datagram(NATNEG_PORT, from, &nn_packet(NN_REPORT, &report))
            .unwrap();
        assert_eq!(ack.len(), NN_ACK_LEN);
        assert_eq!((ack[7], ack[13], ack[14]), (NN_REPORT_ACK, 1, 0));
        assert_eq!(ack[8..12], [0x12, 0x34, 0x56, 0x78]);
        assert_eq!(
            services.datagram(NATNEG_PORT, from, b"hello there, friend"),
            None
        );
        assert_eq!(services.datagram(NATNEG_PORT, from, &NATNEG_MAGIC), None);
    }

    #[test]
    fn a_list_the_relay_cannot_answer_has_only_our_hosts() {
        let (mut services, far) = services();
        let ds = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 56400);
        services.datagram(QR_PORT, ds, &mario_kart_heartbeat("1"));
        let ours = list_with(&mut services, &far, "", None);
        assert_eq!(ours.len(), list_head().len() + 27 + LIST_END.len());
    }

    #[test]
    fn mario_kart_finds_its_own_heartbeat_in_the_list() {
        let (mut services, far) = services();
        let ds = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 56400);
        assert!(
            services
                .datagram(QR_PORT, ds, &mario_kart_heartbeat("1"))
                .is_some()
        );
        services.take_log();
        let mut list = vec![10, 77, 1, 1, 0x19, 0x64, 2];
        list.extend_from_slice(b"\0dwc_pid\0\0numplayers\0\0");
        list.extend_from_slice(&[0x76, 10, 77, 1, 1, 0xDC, 0x50, 10, 77, 1, 1, 0xDC, 0x50]);
        list.extend_from_slice(b"\xFF603819417\0\xFF0\0");
        list.extend_from_slice(&LIST_END);
        assert_eq!(list_for(&mut services, &far, "dwc_pid = 603819417"), list);
        assert_eq!(
            services.take_log(),
            vec![
                "sb list mariokartds filter dwc_pid = 603819417 fields \\dwc_pid\\numplayers"
                    .to_owned(),
                "sb list sends 1 hosts".to_owned()
            ]
        );
    }

    #[test]
    fn a_list_leaves_out_hosts_the_filter_rules_out_or_that_left() {
        let (mut services, far) = services();
        let ds = SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 56400);
        let mut empty = vec![10, 77, 1, 1, 0x19, 0x64, 2];
        empty.extend_from_slice(b"\0dwc_pid\0\0numplayers\0\0");
        empty.extend_from_slice(&LIST_END);
        services.datagram(QR_PORT, ds, &mario_kart_heartbeat("1"));
        services.datagram(QR_PORT, ds, &mario_kart_heartbeat("1"));
        assert_eq!(list_for(&mut services, &far, "dwc_pid != 603819417"), empty);
        assert_eq!(list_for(&mut services, &far, "").len(), empty.len() + 27);
        services.datagram(QR_PORT, ds, &mario_kart_heartbeat("2"));
        assert_eq!(list_for(&mut services, &far, "dwc_pid = 603819417"), empty);
    }

    #[test]
    fn a_list_request_without_servers_gets_only_the_address() {
        let (mut services, _) = services();
        let id = services.open(SB_PORT).unwrap();
        services.input(
            id,
            &list_request("mariokartds", "", "\\dwc_pid", NO_SERVER_LIST),
        );
        let reply = services.take_output(id, 4096);
        assert_eq!(
            read_list(&reply, *b"Abc12345"),
            vec![0, 0, 0, 0, 0x19, 0x64]
        );
    }

    #[test]
    fn a_list_for_a_game_without_a_secret_closes() {
        let (mut services, _) = services();
        let id = services.open(SB_PORT).unwrap();
        services.input(id, &list_request("pokemondpds", "", "", 0));
        assert!(services.finished(id));
        assert_eq!(
            services.take_log(),
            vec![
                "sb list pokemondpds filter  fields ".to_owned(),
                "sb no secret for pokemondpds".to_owned()
            ]
        );
    }

    #[test]
    fn a_broken_list_request_closes_the_browser() {
        let (mut services, _) = services();
        let id = services.open(SB_PORT).unwrap();
        services.input(id, &[0, 5, 0, b'x', 0]);
        assert!(services.finished(id));
        assert_eq!(services.take_log(), vec!["sb broken request".to_owned()]);
    }

    #[test]
    fn a_gts_request_waits_for_the_link_and_answers_with_its_body() {
        let (mut services, far) = services();
        let id = services.open(HTTP_PORT).unwrap();
        services.input(
            id,
            format!(
                "GET /pokemondpds/worldexchange/info.asp?pid=5 HTTP/1.1\r\nHost: {GTS_HOST}\r\n\r\n"
            )
            .as_bytes(),
        );
        assert!(services.take_output(id, usize::MAX).is_empty());
        assert!(!services.finished(id));
        let (call, target) = far.calls.try_recv().unwrap();
        assert_eq!(target, "/pokemondpds/worldexchange/info.asp?pid=5");
        far.answers.send((call, Some(vec![1, 0]))).unwrap();
        services.poll(0);
        let reply = services.take_output(id, usize::MAX);
        assert!(reply.ends_with(b"Content-Length: 2\r\nConnection: close\r\n\r\n\x01\x00"));
        assert!(services.finished(id));
    }

    #[test]
    fn the_nas_date_is_utc() {
        assert_eq!(nas_datetime(0), "19700101000000");
        assert_eq!(nas_datetime(951_782_400), "20000229000000");
        assert_eq!(nas_datetime(1_762_430_400 + 3661), "20251106130101");
    }
}
