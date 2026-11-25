use citrine_gb_net::{Channel, PeerAnswer, PeerDial, PeerKey, PeerSecret};
use citrine_gb_surface::link::{Link, Poll, Tcp};
use citrine_gb_surface::pairing::NearbyHost;
use citrine_gb_surface::proof::TagKey;
use citrine_gb_threads::Spawner;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

pub const SERVICE: &str = "lan";
const NAME_CHARS: usize = 40;
const HANDSHAKE_SECS: f64 = 10.0;
const MAX_WAITING: usize = 8;
const MAX_STRANGERS: usize = 16;
const MAX_READS: usize = 64;
const KEYRING: &[u8; 4] = b"CWKR";
const PROBE_SECS: f64 = 5.0;
const REDIAL_SECS: f64 = 10.0;
const DIAL_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Walker,
    Saves,
}

impl Service {
    const ALL: [Service; 2] = [Service::Walker, Service::Saves];

    fn tag(self) -> &'static str {
        match self {
            Service::Walker => "walker",
            Service::Saves => "saves",
        }
    }

    fn tagged(tag: &str) -> Option<Service> {
        Service::ALL
            .into_iter()
            .find(|service| service.tag() == tag)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum Wire {
    Hello { name: String, instance: Option<u64> },
    Part { service: String, body: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Joined(PeerKey),
    Part {
        from: PeerKey,
        service: Service,
        body: Vec<u8>,
    },
    Left(PeerKey),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Known {
    pub key: PeerKey,
    pub name: String,
}

#[derive(Serialize, Deserialize)]
struct Keyring {
    secret: [u8; 32],
    known: Vec<Known>,
}

enum Shake {
    Dialing(PeerDial),
    Answering(PeerAnswer),
}

struct Pending {
    shake: Shake,
    link: Box<dyn Link>,
    account: bool,
    instance: Option<u64>,
    at: f64,
}

struct Session {
    key: PeerKey,
    name: Option<String>,
    instance: Option<u64>,
    link: Box<dyn Link>,
    channel: Channel,
    account: bool,
    dialed: bool,
    closed: bool,
    at: f64,
}

impl Session {
    fn send(&mut self, wire: &Wire) {
        let Ok(plain) = postcard::to_allocvec(wire) else {
            return;
        };
        let sealed = self.channel.seal(&plain);
        self.link.send(sealed);
    }

    fn dialer(&self, me: PeerKey) -> PeerKey {
        if self.dialed { me } else { self.key }
    }
}

pub struct Lan {
    secret: PeerSecret,
    known: Vec<Known>,
    pending: Vec<Pending>,
    sessions: Vec<Session>,
    name: String,
    instance: Option<u64>,
    changed: bool,
    events: Vec<Event>,
}

impl Lan {
    pub fn new(keyring: Option<&[u8]>) -> Self {
        let keyring = keyring.and_then(read_keyring);
        let changed = keyring.is_none();
        let (secret, known) = match keyring {
            Some(keyring) => (PeerSecret::from_bytes(keyring.secret), keyring.known),
            None => (PeerSecret::random(), Vec::new()),
        };
        Self {
            secret,
            known,
            pending: Vec::new(),
            sessions: Vec::new(),
            name: String::new(),
            instance: None,
            changed,
            events: Vec::new(),
        }
    }

    pub fn key(&self) -> PeerKey {
        self.secret.public()
    }

    pub fn set_name(&mut self, name: &str, instance: Option<u64>) {
        self.name = cut(name);
        self.instance = instance;
    }

    pub fn take_keyring(&mut self) -> Option<Vec<u8>> {
        if !std::mem::take(&mut self.changed) {
            return None;
        }
        let keyring = Keyring {
            secret: *self.secret.as_bytes(),
            known: self.known.clone(),
        };
        let json = serde_json::to_vec(&keyring).ok()?;
        Some([&KEYRING[..], &json].concat())
    }

    pub fn known(&self) -> &[Known] {
        &self.known
    }

    pub fn name(&self, key: PeerKey) -> Option<&str> {
        self.joined(key)?.name.as_deref()
    }

    pub fn trusted(&self, key: PeerKey) -> bool {
        self.joined(key).is_some_and(|session| session.account)
            || self.known.iter().any(|known| known.key == key)
    }

    pub fn remember(&mut self, key: PeerKey) {
        let Some(name) = self.name(key).map(str::to_owned) else {
            return;
        };
        match self.known.iter_mut().find(|known| known.key == key) {
            Some(known) if known.name == name => {}
            Some(known) => {
                known.name = name;
                self.changed = true;
            }
            None => {
                self.known.push(Known { key, name });
                self.changed = true;
            }
        }
    }

    pub fn forget(&mut self, key: PeerKey) {
        let before = self.known.len();
        self.known.retain(|known| known.key != key);
        self.changed |= self.known.len() != before;
        for session in &mut self.sessions {
            if session.key == key && !session.account {
                session.closed = true;
            }
        }
        self.drop_closed();
    }

    pub fn end_account(&mut self) {
        self.pending.retain(|pending| !pending.account);
        for session in &mut self.sessions {
            if session.account {
                session.closed = true;
            }
        }
        self.drop_closed();
    }

    pub fn send(&mut self, to: PeerKey, service: Service, body: Vec<u8>) {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.key == to && session.name.is_some())
        {
            session.send(&Wire::Part {
                service: service.tag().to_owned(),
                body,
            });
        }
    }

    pub fn wants(&self, instance: u64) -> bool {
        Some(instance) != self.instance
            && !self
                .pending
                .iter()
                .any(|pending| pending.instance == Some(instance))
            && !self
                .sessions
                .iter()
                .any(|session| session.instance == Some(instance))
    }

    pub fn dialed(
        &mut self,
        instance: u64,
        mut link: Box<dyn Link>,
        psk: Option<[u8; 32]>,
        now: f64,
    ) {
        let Ok((dial, first)) = PeerDial::start(SERVICE, &self.secret, psk.as_ref()) else {
            return;
        };
        link.send(first);
        self.pending.push(Pending {
            shake: Shake::Dialing(dial),
            link,
            account: psk.is_some(),
            instance: Some(instance),
            at: now,
        });
    }

    pub fn arrived(
        &mut self,
        first: &[u8],
        mut link: Box<dyn Link>,
        psk: Option<[u8; 32]>,
        now: f64,
    ) {
        let waiting = self
            .pending
            .iter()
            .filter(|pending| matches!(pending.shake, Shake::Answering(_)))
            .count();
        if waiting >= MAX_WAITING {
            return;
        }
        let Ok((answer, reply)) = PeerAnswer::accept(first, &self.secret, psk.as_ref()) else {
            return;
        };
        link.send(reply);
        self.pending.push(Pending {
            account: answer.hello().account,
            shake: Shake::Answering(answer),
            link,
            instance: None,
            at: now,
        });
    }

    pub fn step(&mut self, now: f64) -> Vec<Event> {
        self.finish_shakes(now);
        for at in 0..self.sessions.len() {
            self.read(at);
        }
        for session in &mut self.sessions {
            if session.name.is_none() && now - session.at >= HANDSHAKE_SECS {
                session.closed = true;
            }
        }
        self.drop_closed();
        std::mem::take(&mut self.events)
    }

    pub fn trusted_peers(&self) -> impl Iterator<Item = (PeerKey, &str, bool)> {
        self.sessions.iter().filter_map(|session| {
            let name = session.name.as_deref()?;
            let known = self.known.iter().any(|known| known.key == session.key);
            (session.account || known).then_some((session.key, name, session.account))
        })
    }

    fn joined(&self, key: PeerKey) -> Option<&Session> {
        self.sessions
            .iter()
            .find(|session| session.key == key && session.name.is_some())
    }

    fn drop_closed(&mut self) {
        for session in &self.sessions {
            if session.closed && session.name.is_some() {
                self.events.push(Event::Left(session.key));
            }
        }
        self.sessions.retain(|session| !session.closed);
    }

    fn finish_shakes(&mut self, now: f64) {
        let me = self.key();
        let mut done = Vec::new();
        for mut pending in std::mem::take(&mut self.pending) {
            let first = match pending.link.poll() {
                Poll::Message(first) => first,
                Poll::Idle if now - pending.at < HANDSHAKE_SECS => {
                    self.pending.push(pending);
                    continue;
                }
                Poll::Idle | Poll::Closed => continue,
            };
            let (key, channel, dialed) = match pending.shake {
                Shake::Dialing(dial) => {
                    let Ok((last, key, channel)) = dial.finish(&first) else {
                        continue;
                    };
                    pending.link.send(last);
                    (key, channel, true)
                }
                Shake::Answering(answer) => {
                    let Ok((key, channel)) = answer.finish(&first) else {
                        continue;
                    };
                    (key, channel, false)
                }
            };
            if key == me {
                continue;
            }
            let mut session = Session {
                key,
                name: None,
                instance: pending.instance,
                link: pending.link,
                channel,
                account: pending.account,
                dialed,
                closed: false,
                at: now,
            };
            session.send(&Wire::Hello {
                name: self.name.clone(),
                instance: self.instance,
            });
            done.push(session);
        }
        for session in done {
            let twin = self.sessions.iter().position(|old| old.key == session.key);
            match twin {
                Some(at) if self.sessions[at].dialer(me).0 <= session.dialer(me).0 => {}
                Some(at) => {
                    if self.sessions[at].name.is_some() {
                        self.events.push(Event::Left(session.key));
                    }
                    self.sessions[at] = session;
                }
                None if !session.dialed
                    && self.stranger(&session)
                    && self.strangers() >= MAX_STRANGERS => {}
                None => self.sessions.push(session),
            }
        }
    }

    fn stranger(&self, session: &Session) -> bool {
        !session.account && self.known.iter().all(|known| known.key != session.key)
    }

    fn strangers(&self) -> usize {
        self.sessions
            .iter()
            .filter(|session| self.stranger(session))
            .count()
    }

    fn read(&mut self, at: usize) {
        let session = &mut self.sessions[at];
        for _ in 0..MAX_READS {
            let sealed = match session.link.poll() {
                Poll::Message(sealed) => sealed,
                Poll::Idle => return,
                Poll::Closed => {
                    session.closed = true;
                    return;
                }
            };
            let wire = session
                .channel
                .open(&sealed)
                .ok()
                .and_then(|plain| postcard::from_bytes::<Wire>(&plain).ok());
            match wire {
                Some(Wire::Hello { name, instance }) => {
                    let joined = session.name.is_none();
                    session.name = Some(cut(&name));
                    session.instance = session.instance.or(instance);
                    if joined {
                        self.events.push(Event::Joined(session.key));
                    }
                }
                Some(Wire::Part { service, body }) if session.name.is_some() => {
                    if let Some(service) = Service::tagged(&service) {
                        self.events.push(Event::Part {
                            from: session.key,
                            service,
                            body,
                        });
                    }
                }
                Some(Wire::Part { .. }) | None => {
                    session.closed = true;
                    return;
                }
            }
        }
    }
}

fn read_keyring(bytes: &[u8]) -> Option<Keyring> {
    serde_json::from_slice(bytes.strip_prefix(&KEYRING[..])?).ok()
}

fn cut(name: &str) -> String {
    name.chars().take(NAME_CHARS).collect()
}

pub struct LanAccount {
    pub own_psk: [u8; 32],
    pub devices: Vec<(TagKey, [u8; 32])>,
}

type Dialed = (u64, Tcp, Option<[u8; 32]>);

pub struct Finder {
    nearby: Option<(Option<u64>, citrine_gb_surface::pairing::Nearby)>,
    probed_at: f64,
    tried: Vec<(u64, f64)>,
    tx: Sender<Dialed>,
    found: Receiver<Dialed>,
}

impl Default for Finder {
    fn default() -> Self {
        let (tx, found) = channel();
        Self {
            nearby: None,
            probed_at: f64::NEG_INFINITY,
            tried: Vec::new(),
            tx,
            found,
        }
    }
}

impl Finder {
    pub fn step(
        &mut self,
        lan: &mut Lan,
        account: Option<&LanAccount>,
        spawner: &Spawner,
        now: f64,
    ) {
        let skip = lan.instance;
        if self.nearby.as_ref().is_none_or(|(at, _)| *at != skip) {
            self.nearby = Some((skip, citrine_gb_surface::pairing::Nearby::start(skip)));
            self.probed_at = now;
        }
        let Some((_, nearby)) = self.nearby.as_mut() else {
            return;
        };
        if now - self.probed_at >= PROBE_SECS {
            nearby.probe();
            self.probed_at = now;
        }
        self.tried.retain(|(_, at)| now - at < REDIAL_SECS);
        for host in nearby.poll() {
            if !host.offers(SERVICE)
                || !lan.wants(host.instance)
                || self
                    .tried
                    .iter()
                    .any(|(instance, _)| *instance == host.instance)
            {
                continue;
            }
            self.tried.push((host.instance, now));
            let psk = account.and_then(|account| psk_for(nearby, &host, account));
            let tx = self.tx.clone();
            let NearbyHost { addr, instance, .. } = host;
            spawner.run("lan dial", move || {
                if let Ok(tcp) = Tcp::dial(addr, DIAL_WAIT) {
                    let _ = tx.send((instance, tcp, psk));
                }
            });
        }
        while let Ok((instance, tcp, psk)) = self.found.try_recv() {
            lan.dialed(instance, Box::new(tcp), psk, now);
        }
    }
}

fn psk_for(
    nearby: &citrine_gb_surface::pairing::Nearby,
    host: &NearbyHost,
    account: &LanAccount,
) -> Option<[u8; 32]> {
    account
        .devices
        .iter()
        .find(|(tag, _)| nearby.tagged_by(host, tag))
        .map(|(_, psk)| *psk)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use citrine_gb_surface::link::{InApp, in_app};

    const PSK: [u8; 32] = [9; 32];

    pub(crate) fn lan(name: &str, instance: u64) -> Lan {
        let mut lan = Lan::new(None);
        lan.set_name(name, Some(instance));
        lan
    }

    pub(crate) fn connect(from: &mut Lan, to: &mut Lan, psk: Option<[u8; 32]>) {
        let (dial_end, mut answer_end) = in_app();
        let instance = to.instance.expect("an instance");
        from.dialed(instance, Box::new(dial_end), psk, 0.0);
        let Poll::Message(first) = answer_end.poll() else {
            panic!("the dialer said hello");
        };
        to.arrived(&first, Box::new(answer_end), psk, 0.0);
    }

    fn settle(lans: &mut [&mut Lan]) -> Vec<Vec<Event>> {
        let mut seen = vec![Vec::new(); lans.len()];
        for _ in 0..6 {
            for (at, lan) in lans.iter_mut().enumerate() {
                seen[at].extend(lan.step(0.0));
            }
        }
        seen
    }

    #[test]
    fn a_hello_joins_and_parts_reach_the_other_side() {
        let mut mac = lan("Mac", 1);
        let mut pixel = lan("Pixel", 2);
        connect(&mut pixel, &mut mac, None);
        let seen = settle(&mut [&mut mac, &mut pixel]);
        assert_eq!(seen[0], [Event::Joined(pixel.key())]);
        assert_eq!(seen[1], [Event::Joined(mac.key())]);
        assert_eq!(mac.name(pixel.key()), Some("Pixel"));
        pixel.send(mac.key(), Service::Walker, vec![1, 2, 3]);
        let seen = settle(&mut [&mut mac, &mut pixel]);
        assert_eq!(
            seen[0],
            [Event::Part {
                from: pixel.key(),
                service: Service::Walker,
                body: vec![1, 2, 3],
            }]
        );
    }

    #[test]
    fn only_the_account_and_remembered_devices_are_trusted() {
        let mut mac = lan("Mac", 1);
        let mut pixel = lan("Pixel", 2);
        let mut ipad = lan("iPad", 3);
        connect(&mut pixel, &mut mac, None);
        connect(&mut ipad, &mut mac, Some(PSK));
        settle(&mut [&mut mac, &mut pixel, &mut ipad]);
        assert!(!mac.trusted(pixel.key()));
        assert!(mac.trusted(ipad.key()));
        mac.remember(pixel.key());
        assert!(mac.trusted(pixel.key()));
        assert_eq!(mac.known()[0].name, "Pixel");
        assert!(mac.known().iter().all(|known| known.key != ipad.key()));
    }

    #[test]
    fn forgetting_a_device_ends_its_session_but_not_the_accounts() {
        let mut mac = lan("Mac", 1);
        let mut pixel = lan("Pixel", 2);
        let mut ipad = lan("iPad", 3);
        connect(&mut pixel, &mut mac, None);
        connect(&mut ipad, &mut mac, Some(PSK));
        settle(&mut [&mut mac, &mut pixel, &mut ipad]);
        mac.remember(pixel.key());
        mac.forget(pixel.key());
        mac.forget(ipad.key());
        let seen = settle(&mut [&mut mac, &mut pixel, &mut ipad]);
        assert_eq!(seen[0], [Event::Left(pixel.key())]);
        assert_eq!(seen[1], [Event::Left(mac.key())]);
        assert!(seen[2].is_empty());
        assert!(mac.known().is_empty());
        assert_eq!(mac.name(ipad.key()), Some("iPad"));
    }

    #[test]
    fn signing_out_ends_the_account_sessions_but_keeps_paired_ones() {
        let mut mac = lan("Mac", 1);
        let mut pixel = lan("Pixel", 2);
        let mut ipad = lan("iPad", 3);
        connect(&mut pixel, &mut mac, None);
        connect(&mut ipad, &mut mac, Some(PSK));
        settle(&mut [&mut mac, &mut pixel, &mut ipad]);
        mac.remember(pixel.key());
        mac.end_account();
        let seen = settle(&mut [&mut mac, &mut pixel, &mut ipad]);
        assert_eq!(seen[0], [Event::Left(ipad.key())]);
        assert_eq!(seen[2], [Event::Left(mac.key())]);
        let trusted: Vec<_> = mac.trusted_peers().collect();
        assert_eq!(trusted, [(pixel.key(), "Pixel", false)]);
    }

    #[test]
    fn a_part_for_a_service_this_build_lacks_is_skipped() {
        let mut mac = lan("Mac", 1);
        let mut pixel = lan("Pixel", 2);
        connect(&mut pixel, &mut mac, None);
        settle(&mut [&mut mac, &mut pixel]);
        let session = &mut pixel.sessions[0];
        session.send(&Wire::Part {
            service: "later".into(),
            body: vec![1],
        });
        pixel.send(mac.key(), Service::Walker, vec![2]);
        let seen = settle(&mut [&mut mac, &mut pixel]);
        assert_eq!(
            seen[0],
            [Event::Part {
                from: pixel.key(),
                service: Service::Walker,
                body: vec![2],
            }]
        );
    }

    #[test]
    fn two_links_between_the_same_devices_end_as_one() {
        let mut mac = lan("Mac", 1);
        let mut pixel = lan("Pixel", 2);
        connect(&mut pixel, &mut mac, None);
        connect(&mut mac, &mut pixel, None);
        settle(&mut [&mut mac, &mut pixel]);
        assert_eq!(mac.sessions.len(), 1);
        assert_eq!(pixel.sessions.len(), 1);
        pixel.send(mac.key(), Service::Walker, vec![7]);
        let seen = settle(&mut [&mut mac, &mut pixel]);
        assert!(seen[0].contains(&Event::Part {
            from: pixel.key(),
            service: Service::Walker,
            body: vec![7],
        }));
    }

    #[test]
    fn the_keyring_keeps_the_secret_and_remembered_devices() {
        let mut mac = lan("Mac", 1);
        let mut pixel = lan("Pixel", 2);
        connect(&mut pixel, &mut mac, None);
        settle(&mut [&mut mac, &mut pixel]);
        assert!(mac.take_keyring().is_some());
        mac.remember(pixel.key());
        let bytes = mac.take_keyring().expect("a changed keyring");
        assert!(mac.take_keyring().is_none());
        let mut again = Lan::new(Some(&bytes));
        assert_eq!(again.key(), mac.key());
        assert_eq!(again.known(), mac.known());
        again.forget(pixel.key());
        assert!(again.known().is_empty());
        assert!(again.take_keyring().is_some());
    }

    fn answered(to: &mut Lan, now: f64) -> (InApp, PeerDial) {
        let (dial_end, answer_end) = in_app();
        let (dial, first) = PeerDial::start(SERVICE, &PeerSecret::random(), None).unwrap();
        to.arrived(&first, Box::new(answer_end), None, now);
        (dial_end, dial)
    }

    fn silent(to: &mut Lan, now: f64) -> InApp {
        let (mut link, dial) = answered(to, now);
        let Poll::Message(reply) = link.poll() else {
            panic!("the answer");
        };
        let (last, _, _) = dial.finish(&reply).unwrap();
        link.send(last);
        link
    }

    #[test]
    fn at_most_eight_handshakes_wait_and_they_end_after_ten_seconds() {
        let mut mac = lan("Mac", 1);
        let _links: Vec<_> = (0..12).map(|_| answered(&mut mac, 0.0)).collect();
        assert_eq!(mac.pending.len(), 8);
        mac.step(10.0);
        assert!(mac.pending.is_empty());
        let mut pixel = lan("Pixel", 2);
        connect(&mut pixel, &mut mac, None);
        let seen = settle(&mut [&mut mac, &mut pixel]);
        assert_eq!(seen[0], [Event::Joined(pixel.key())]);
    }

    #[test]
    fn a_session_without_a_hello_ends_after_ten_seconds() {
        let mut mac = lan("Mac", 1);
        let _link = silent(&mut mac, 0.0);
        mac.step(1.0);
        assert_eq!(mac.sessions.len(), 1);
        mac.step(11.0);
        assert!(mac.sessions.is_empty());
    }

    #[test]
    fn strangers_past_sixteen_are_turned_away_but_the_account_joins() {
        let mut mac = lan("Mac", 1);
        let mut others: Vec<Lan> = (0..18).map(|n| lan(&n.to_string(), 10 + n)).collect();
        for other in &mut others {
            connect(other, &mut mac, None);
            settle(&mut [&mut mac, &mut *other]);
        }
        assert_eq!(mac.sessions.len(), 16);
        let mut ipad = lan("iPad", 3);
        connect(&mut ipad, &mut mac, Some(PSK));
        settle(&mut [&mut mac, &mut ipad]);
        assert_eq!(mac.name(ipad.key()), Some("iPad"));
    }

    #[test]
    fn one_step_reads_at_most_sixty_four_parts_from_a_device() {
        let mut mac = lan("Mac", 1);
        let mut pixel = lan("Pixel", 2);
        connect(&mut pixel, &mut mac, None);
        settle(&mut [&mut mac, &mut pixel]);
        for n in 0..100u8 {
            pixel.send(mac.key(), Service::Walker, vec![n]);
        }
        assert_eq!(mac.step(0.0).len(), 64);
        assert_eq!(mac.step(0.0).len(), 36);
    }
}
