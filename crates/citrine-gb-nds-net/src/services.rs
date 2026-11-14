use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use md5::{Digest, Md5};

use crate::http::{self, Parsed, Request};

pub const ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
pub const GTS_HOST: &str = "gamestats2.gs.nintendowifi.net";
pub const VALIDATION_HOST: &str = "pkvldtprod.nintendo.co.jp";
pub const HTTP_PORT: u16 = 80;
pub const GPCM_PORT: u16 = 29900;
pub const GPSP_PORT: u16 = 29901;
pub const QR_PORT: u16 = 27900;

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
];
const QR_MAGIC: [u8; 2] = [0xFE, 0xFD];
const CHALLENGE: u8 = 0x01;
const HEARTBEAT: u8 = 0x03;
const AVAILABLE: u8 = 0x09;
const REGISTERED: u8 = 0x0A;
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
    SERVED.contains(&name.as_str()) || SERVED_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
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
}

pub struct Services {
    seed: u64,
    link: Box<dyn GtsLink>,
    logins: Vec<NasLogin>,
    names: HashMap<(String, String), String>,
    flows: Vec<Flow>,
    next: u64,
    random: u64,
    log: Vec<String>,
    clock: fn() -> u64,
}

impl Services {
    pub fn new(seed: u64, link: Box<dyn GtsLink>) -> Self {
        Self {
            seed,
            link,
            logins: Vec::new(),
            names: HashMap::new(),
            flows: Vec::new(),
            next: 1,
            random: seed ^ 0x9E37_79B9_7F4A_7C15,
            log: Vec::new(),
            clock: unix_now,
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
        if !matches!(port, HTTP_PORT | GPCM_PORT | GPSP_PORT) {
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
        };
        if port == GPCM_PORT {
            flow.challenge = self.random_text(UPPERCASE, 10);
            flow.output = gp_message(&[("lc", "1"), ("challenge", &flow.challenge), ("id", "1")]);
        }
        self.flows.push(flow);
        Some(id)
    }

    pub fn input(&mut self, id: u64, data: &[u8]) {
        let Some(at) = self.flows.iter().position(|flow| flow.id == id) else {
            return;
        };
        self.flows[at].input.extend_from_slice(data);
        match self.flows[at].port {
            HTTP_PORT => self.serve_http(at),
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

    pub fn datagram(&mut self, port: u16, from: SocketAddrV4, data: &[u8]) -> Option<Vec<u8>> {
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
                Some(qr_message(CHALLENGE, session, text.as_bytes()))
            }
            Some(CHALLENGE) => {
                let session = data.get(1..5)?;
                self.note("qr registered".to_owned());
                Some(qr_message(REGISTERED, session, &[]))
            }
            _ => None,
        }
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

    pub fn poll(&mut self) {
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

    fn gp_login(&mut self, at: usize, token: &str, client: &str, id: &str) -> Vec<u8> {
        let Some(login) = self.logins.iter().find(|login| login.token == token) else {
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
        };
        let server = self.flows[at].challenge.clone();
        let half = md5_hex(login.challenge.as_bytes());
        let proof = md5_hex(format!("{half}{PROOF_PAD}{token}{server}{client}{half}").as_bytes());
        let profile = profile_id(self.seed, &login.gsbrcd);
        let nick = unique_nick(&login.userid, &login.gsbrcd);
        let sesskey = format!("{}", 10_000_000 + self.next_random() % 89_999_999);
        let lt = STANDARD
            .encode(self.random_text(ALPHANUMERIC, 16))
            .replace('+', "[")
            .replace('/', "]")
            .replace('=', "_");
        let profile = profile.to_string();
        self.flows[at].profile.clone_from(&profile);
        gp_message(&[
            ("lc", "2"),
            ("sesskey", &sesskey),
            ("proof", &proof),
            ("userid", &profile),
            ("profileid", &profile),
            ("uniquenick", &nick),
            ("lt", &lt),
            ("id", id),
        ])
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
            .find(|login| profile_id(self.seed, &login.gsbrcd).to_string() == profile)
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

    struct Recorder {
        calls: Sender<(u64, String)>,
        answers: Receiver<(u64, Option<Vec<u8>>)>,
    }

    impl GtsLink for Recorder {
        fn call(&mut self, id: u64, target: &str) {
            self.calls.send((id, target.to_owned())).ok();
        }

        fn answer(&mut self) -> Option<(u64, Option<Vec<u8>>)> {
            self.answers.try_recv().ok()
        }
    }

    struct Far {
        calls: Receiver<(u64, String)>,
        answers: Sender<(u64, Option<Vec<u8>>)>,
    }

    fn services() -> (Services, Far) {
        let (calls, called) = mpsc::channel();
        let (answer, answers) = mpsc::channel();
        let services =
            Services::new(42, Box::new(Recorder { calls, answers })).with_clock(|| 1_762_430_400);
        (
            services,
            Far {
                calls: called,
                answers: answer,
            },
        )
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
        let (mut services, _) = services();
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
        let (mut services, _) = services();
        let reply = exchange(
            &mut services,
            HTTP_PORT,
            &nas_post(&[
                ("action", "login"),
                ("userid", "1234567890123"),
                ("gsbrcd", "IPKDabcd"),
            ]),
        );
        let token = nas_field(&reply, "token");
        let id = services.open(GPCM_PORT).unwrap();
        services.take_output(id, usize::MAX);
        services.input(
            id,
            format!("\\login\\\\challenge\\c\\authtoken\\{token}\\id\\1\\final\\").as_bytes(),
        );
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
        services.poll();
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
