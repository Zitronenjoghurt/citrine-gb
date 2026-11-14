use crate::codec::Encoder;
use crate::link::{Link, Poll, Route};
use crate::mode::{Mode, ScreenShape};
use crate::pairing::{CODE_LIFE_SECS, Code};
use crate::proof::{Nonce, ProofKey};
use crate::proto::{
    Action, Bye, GameCard, HostMsg, MAX_NAME_CHARS, Resume, SURFACE_VERSION, SurfaceId,
    SurfaceKind, SurfaceMsg, Wire,
};
use crate::touch::{Pointer, ScreenTouch, Stylus, clamped};
use std::net::IpAddr;

pub const KEEP_LOST_SECS: f64 = 60.0;
pub const HELLO_WAIT_SECS: f64 = 10.0;
pub const FRAMES_IN_FLIGHT: usize = 2;
pub const MAX_ASKS: usize = 3;
pub const ASK_SECS: f64 = 60.0;
pub const NEEDS_UPDATE_EVERY_SECS: f64 = 60.0;

#[derive(Debug, Clone, Copy)]
pub struct ScreenFrame<'a> {
    pub rgba: &'a [u8],
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Connected,
    Reconnecting,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceRow {
    pub id: SurfaceId,
    pub name: String,
    pub kind: SurfaceKind,
    pub mode: Mode,
    pub presence: Presence,
    pub route: Route,
    pub ping_ms: Option<u32>,
}

struct Waiting {
    link: Box<dyn Link>,
    since: f64,
    trusted: bool,
}

#[derive(PartialEq, Eq)]
enum Sender {
    Address(IpAddr),
    Name(String),
}

struct Asking {
    id: u32,
    link: Box<dyn Link>,
    name: String,
    kind: SurfaceKind,
    sender: Sender,
    nonce: Nonce,
    since: f64,
    shown: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asker {
    pub id: u32,
    pub name: String,
    pub kind: SurfaceKind,
}

struct Slot {
    id: SurfaceId,
    name: String,
    kind: SurfaceKind,
    link: Option<Box<dyn Link>>,
    route: Route,
    trusted: bool,
    resume: Resume,
    mode: Mode,
    encoders: Vec<Encoder>,
    seq: u32,
    in_flight: Vec<(u32, f64)>,
    ping: Option<f64>,
    touch: Option<ScreenTouch>,
    buttons: u16,
    lost_at: Option<f64>,
}

impl Slot {
    fn send(&mut self, msg: &HostMsg) {
        if let Some(link) = &mut self.link {
            link.send(msg.encode());
        }
    }

    fn restart(&mut self) {
        self.encoders.iter_mut().for_each(Encoder::restart);
        self.in_flight.clear();
    }

    fn lose(&mut self, now: f64) {
        self.link = None;
        self.lost_at = Some(now);
        self.touch = None;
        self.buttons = 0;
        self.in_flight.clear();
    }
}

pub struct Host {
    name: String,
    code: Option<(Code, f64)>,
    waiting: Vec<Waiting>,
    asking: Vec<Asking>,
    next_ask: u32,
    instance: u64,
    proof_key: Option<ProofKey>,
    outdated: Vec<String>,
    outdated_at: Vec<(String, f64)>,
    slots: Vec<Slot>,
    next_id: u32,
    game: Option<GameCard>,
    stylus: Stylus<SurfaceId>,
    actions: Vec<Action>,
    now: f64,
}

impl Host {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            code: None,
            waiting: Vec::new(),
            asking: Vec::new(),
            next_ask: 1,
            instance: crate::proof::instance(),
            proof_key: None,
            outdated: Vec::new(),
            outdated_at: Vec::new(),
            slots: Vec::new(),
            next_id: 1,
            game: None,
            stylus: Stylus::default(),
            actions: Vec::new(),
            now: 0.0,
        }
    }

    pub fn open_code(&mut self, now: f64) -> Code {
        let code = Code::random();
        self.code = Some((code, now));
        code
    }

    pub fn open_code_as(&mut self, code: Code, now: f64) {
        self.code = Some((code, now));
    }

    pub fn close_code(&mut self) {
        self.code = None;
    }

    pub fn code(&self) -> Option<(Code, f64)> {
        let (code, opened) = self.code?;
        let left = CODE_LIFE_SECS - (self.now - opened);
        (left > 0.0).then_some((code, left))
    }

    pub fn accept(&mut self, link: Box<dyn Link>) {
        self.waiting.push(Waiting {
            link,
            since: self.now,
            trusted: false,
        });
    }

    pub fn accept_trusted(&mut self, link: Box<dyn Link>) {
        self.waiting.push(Waiting {
            link,
            since: self.now,
            trusted: true,
        });
    }

    pub fn set_proof_key(&mut self, key: Option<ProofKey>) {
        self.proof_key = key;
    }

    pub fn askers(&self) -> Vec<Asker> {
        self.asking
            .iter()
            .filter(|ask| ask.shown)
            .map(|ask| Asker {
                id: ask.id,
                name: ask.name.clone(),
                kind: ask.kind,
            })
            .collect()
    }

    pub fn allow(&mut self, id: u32) {
        if let Some(at) = self.asking.iter().position(|ask| ask.id == id && ask.shown) {
            let ask = self.asking.remove(at);
            self.admit(ask.link, ask.name, ask.kind, false);
        }
    }

    pub fn decline(&mut self, id: u32) {
        if let Some(at) = self.asking.iter().position(|ask| ask.id == id && ask.shown) {
            let mut ask = self.asking.remove(at);
            ask.link.send(HostMsg::Bye(Bye::Declined).encode());
        }
    }

    pub fn take_outdated(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outdated)
    }

    pub fn game(&self) -> Option<&GameCard> {
        self.game.as_ref()
    }

    pub fn screens(&self) -> &[ScreenShape] {
        self.game
            .as_ref()
            .map(|g| g.screens.as_slice())
            .unwrap_or(&[])
    }

    pub fn set_game(&mut self, game: Option<GameCard>) {
        if self.game == game {
            return;
        }
        let new_screens =
            game.as_ref().map(|g| &g.screens) != self.game.as_ref().map(|g| &g.screens);
        self.game = game;
        if self.game.is_none() {
            for mut ask in self.asking.drain(..) {
                ask.link.send(HostMsg::Bye(Bye::NoGame).encode());
            }
        }
        let msg = HostMsg::Game(self.game.clone());
        for slot in &mut self.slots {
            slot.send(&msg);
            if new_screens {
                slot.restart();
            }
        }
    }

    pub fn step(&mut self, now: f64) {
        self.now = now;
        self.greet();
        self.hear_askers();
        for at in 0..self.slots.len() {
            self.listen(at);
        }
        let kept = |slot: &Slot| match slot.lost_at {
            Some(lost) => !slot.trusted && now - lost < KEEP_LOST_SECS,
            None => true,
        };
        self.slots.retain(kept);
    }

    fn greet(&mut self) {
        let mut waiting = std::mem::take(&mut self.waiting);
        let mut still = Vec::new();
        for mut wait in waiting.drain(..) {
            match wait.link.poll() {
                Poll::Message(bytes) => {
                    if let Some(SurfaceMsg::Hello {
                        version,
                        name,
                        kind,
                        code,
                        resume,
                    }) = SurfaceMsg::decode(&bytes)
                    {
                        self.hello(wait, version, name, kind, code, resume);
                    }
                }
                Poll::Idle if self.now - wait.since < HELLO_WAIT_SECS => still.push(wait),
                Poll::Idle | Poll::Closed => {}
            }
        }
        still.append(&mut self.waiting);
        self.waiting = still;
    }

    fn hello(
        &mut self,
        mut wait: Waiting,
        version: u16,
        name: String,
        kind: SurfaceKind,
        code: Option<Code>,
        resume: Option<Resume>,
    ) {
        let name: String = name.chars().take(MAX_NAME_CHARS).collect();
        if version != SURFACE_VERSION {
            wait.link.send(HostMsg::Bye(Bye::NeedsUpdate).encode());
            self.name_outdated(name);
            return;
        }
        let route = wait.link.route();
        if let Some(slot) = resume.and_then(|r| self.slots.iter_mut().find(|s| s.resume == r)) {
            slot.link = Some(wait.link);
            slot.route = route;
            slot.lost_at = None;
            slot.restart();
            let welcome = HostMsg::Welcome {
                id: slot.id,
                resume: slot.resume,
                host: self.name.clone(),
            };
            let mode = HostMsg::Mode(slot.mode.clone());
            slot.send(&welcome);
            slot.send(&HostMsg::Game(self.game.clone()));
            slot.send(&mode);
            return;
        }
        if !wait.trusted && code.is_none() {
            self.ask(wait.link, name, kind);
            return;
        }
        let open = self.code().map(|(c, _)| c);
        if !wait.trusted && code != open {
            wait.link.send(HostMsg::Bye(Bye::WrongCode).encode());
            return;
        }
        if !wait.trusted {
            self.code = None;
        }
        self.admit(wait.link, name, kind, wait.trusted);
    }

    fn name_outdated(&mut self, name: String) {
        let now = self.now;
        self.outdated_at
            .retain(|(_, at)| now - at < NEEDS_UPDATE_EVERY_SECS);
        if self.outdated_at.iter().any(|(named, _)| *named == name) {
            return;
        }
        self.outdated_at.push((name.clone(), now));
        self.outdated.push(name);
    }

    fn ask(&mut self, mut link: Box<dyn Link>, name: String, kind: SurfaceKind) {
        if self.game.is_none() {
            link.send(HostMsg::Bye(Bye::NoGame).encode());
            return;
        }
        let sender = link
            .address()
            .map_or_else(|| Sender::Name(name.clone()), Sender::Address);
        self.asking.retain(|ask| ask.sender != sender);
        let nonce = crate::proof::nonce();
        link.send(
            HostMsg::Challenge {
                nonce,
                host: self.instance,
            }
            .encode(),
        );
        let id = self.next_ask;
        self.next_ask += 1;
        self.asking.push(Asking {
            id,
            link,
            name,
            kind,
            sender,
            nonce,
            since: self.now,
            shown: false,
        });
    }

    fn hear_askers(&mut self) {
        let now = self.now;
        let (key, host) = (self.proof_key, self.instance);
        let mut shown = self.asking.iter().filter(|ask| ask.shown).count();
        let mut proven = Vec::new();
        self.asking.retain_mut(|ask| {
            loop {
                match ask.link.poll() {
                    Poll::Message(bytes) => match SurfaceMsg::decode(&bytes) {
                        Some(SurfaceMsg::Proof(proof)) if !ask.shown => {
                            let own = key
                                .zip(proof)
                                .is_some_and(|(key, proof)| key.checks(&ask.nonce, host, &proof));
                            if own {
                                proven.push(ask.id);
                                return true;
                            }
                            if shown >= MAX_ASKS {
                                ask.link.send(HostMsg::Bye(Bye::Declined).encode());
                                return false;
                            }
                            ask.link.send(HostMsg::Asking.encode());
                            ask.shown = true;
                            ask.since = now;
                            shown += 1;
                        }
                        Some(SurfaceMsg::Leave) => return false,
                        _ => {}
                    },
                    Poll::Idle => break,
                    Poll::Closed => return false,
                }
            }
            if ask.shown && now - ask.since >= ASK_SECS {
                ask.link.send(HostMsg::Bye(Bye::NoAnswer).encode());
                return false;
            }
            ask.shown || now - ask.since < HELLO_WAIT_SECS
        });
        for id in proven {
            if let Some(at) = self.asking.iter().position(|ask| ask.id == id) {
                let ask = self.asking.remove(at);
                self.admit(ask.link, ask.name, ask.kind, false);
            }
        }
    }

    fn admit(&mut self, link: Box<dyn Link>, name: String, kind: SurfaceKind, trusted: bool) {
        let route = link.route();
        let id = SurfaceId(self.next_id);
        self.next_id += 1;
        let mut slot = Slot {
            id,
            name,
            kind,
            link: Some(link),
            route,
            trusted,
            resume: Resume::random(),
            mode: Mode::default_for(self.screens()),
            encoders: Vec::new(),
            seq: 0,
            in_flight: Vec::new(),
            ping: None,
            touch: None,
            buttons: 0,
            lost_at: None,
        };
        slot.send(&HostMsg::Welcome {
            id,
            resume: slot.resume,
            host: self.name.clone(),
        });
        slot.send(&HostMsg::Game(self.game.clone()));
        slot.send(&HostMsg::Mode(slot.mode.clone()));
        self.slots.push(slot);
    }

    fn listen(&mut self, at: usize) {
        let now = self.now;
        let screens = self.screens().to_vec();
        let slot = &mut self.slots[at];
        loop {
            let Some(link) = &mut slot.link else {
                return;
            };
            let bytes = match link.poll() {
                Poll::Message(bytes) => bytes,
                Poll::Idle => return,
                Poll::Closed => {
                    slot.lose(now);
                    return;
                }
            };
            match SurfaceMsg::decode(&bytes) {
                Some(SurfaceMsg::SetMode(mode)) => {
                    slot.mode = mode;
                    slot.restart();
                    let echo = HostMsg::Mode(slot.mode.clone());
                    slot.send(&echo);
                }
                Some(SurfaceMsg::Touch(touch)) => {
                    slot.touch = touch.and_then(|t| clamped(t, &screens));
                }
                Some(SurfaceMsg::Buttons(buttons)) => slot.buttons = buttons,
                Some(SurfaceMsg::Ack { seq, .. }) => {
                    if let Some(&(_, sent)) = slot.in_flight.iter().find(|(s, _)| *s == seq) {
                        slot.ping = Some(now - sent);
                    }
                    slot.in_flight.retain(|(s, _)| *s > seq);
                }
                Some(SurfaceMsg::Action(action)) => self.actions.push(action),
                Some(SurfaceMsg::Leave) => {
                    slot.lose(now);
                    slot.lost_at = Some(now - KEEP_LOST_SECS);
                    return;
                }
                Some(SurfaceMsg::Hello { .. } | SurfaceMsg::Proof(_)) | None => {}
            }
        }
    }

    pub fn publish(&mut self, frames: &[ScreenFrame<'_>]) {
        let screens = self.screens().to_vec();
        let now = self.now;
        for slot in &mut self.slots {
            if slot.link.is_none() || slot.kind == SurfaceKind::Window {
                continue;
            }
            let shown: Vec<u8> = (0..frames.len())
                .filter_map(|i| u8::try_from(i).ok())
                .filter(|i| slot.mode.shows_screen(&screens, *i))
                .collect();
            if shown.is_empty() || slot.in_flight.len() >= FRAMES_IN_FLIGHT * shown.len() {
                continue;
            }
            for index in shown {
                let frame = frames[usize::from(index)];
                if slot.encoders.len() <= usize::from(index) {
                    slot.encoders
                        .resize_with(usize::from(index) + 1, Encoder::default);
                }
                slot.seq = slot.seq.wrapping_add(1);
                let seq = slot.seq;
                let encoded = slot.encoders[usize::from(index)].encode(
                    index,
                    seq,
                    frame.rgba,
                    frame.width,
                    frame.height,
                );
                slot.in_flight.push((seq, now));
                slot.send(&HostMsg::Frame(encoded));
            }
        }
    }

    pub fn buttons(&self) -> u16 {
        self.slots
            .iter()
            .filter(|s| s.link.is_some())
            .fold(0, |held, s| held | s.buttons)
    }

    pub fn touch(&mut self, local: Option<ScreenTouch>) -> Option<ScreenTouch> {
        let pressing = std::iter::once((Pointer::Local, local)).chain(
            self.slots
                .iter()
                .filter(|s| s.link.is_some())
                .map(|s| (Pointer::Surface(s.id), s.touch)),
        );
        self.stylus.pick(pressing.collect::<Vec<_>>())
    }

    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }

    pub fn surfaces(&self) -> Vec<SurfaceRow> {
        self.slots
            .iter()
            .map(|s| SurfaceRow {
                id: s.id,
                name: s.name.clone(),
                kind: s.kind,
                mode: s.mode.clone(),
                presence: if s.link.is_some() {
                    Presence::Connected
                } else {
                    Presence::Reconnecting
                },
                route: s.route,
                ping_ms: s.ping.map(|p| (p * 1000.0).round() as u32),
            })
            .collect()
    }

    pub fn wants_frames(&self) -> bool {
        let screens = self.screens();
        self.slots.iter().any(|s| {
            s.link.is_some()
                && s.kind != SurfaceKind::Window
                && (0..screens.len())
                    .any(|i| u8::try_from(i).is_ok_and(|i| s.mode.shows_screen(screens, i)))
        })
    }

    pub fn shows_screen(&self, index: u8) -> bool {
        self.slots
            .iter()
            .any(|s| s.link.is_some() && s.mode.shows_screen(self.screens(), index))
    }

    pub fn set_mode(&mut self, id: SurfaceId, mode: Mode) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.id == id) {
            slot.mode = mode;
            slot.restart();
            let msg = HostMsg::Mode(slot.mode.clone());
            slot.send(&msg);
        }
    }

    pub fn remove(&mut self, id: SurfaceId) {
        if let Some(at) = self.slots.iter().position(|s| s.id == id) {
            let mut slot = self.slots.remove(at);
            slot.send(&HostMsg::Bye(Bye::Removed));
        }
    }

    pub fn close(&mut self) {
        for slot in &mut self.slots {
            slot.send(&HostMsg::Bye(Bye::HostClosed));
        }
        for ask in &mut self.asking {
            ask.link.send(HostMsg::Bye(Bye::HostClosed).encode());
        }
        self.slots.clear();
        self.waiting.clear();
        self.asking.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::in_app;
    use crate::mode::{Preset, View};
    use crate::remote::{Phase, Remote};

    fn ds() -> GameCard {
        GameCard {
            title: "Pong-NDS".into(),
            system: "Nintendo DS".into(),
            icon: None,
            screens: vec![
                ScreenShape {
                    width: 256,
                    height: 192,
                    touch: false,
                },
                ScreenShape {
                    width: 256,
                    height: 192,
                    touch: true,
                },
            ],
            played_secs: 0,
            paused: false,
            saved_secs_ago: None,
        }
    }

    fn pair(host: &mut Host, code: Option<Code>, now: f64) -> Remote {
        let (a, b) = in_app();
        host.accept(Box::new(a));
        let mut remote = Remote::join(Box::new(b), "Pixel 8", SurfaceKind::Device, code, None);
        settle(host, &mut remote, now);
        remote
    }

    fn settle(host: &mut Host, remote: &mut Remote, now: f64) {
        for _ in 0..4 {
            host.step(now);
            remote.step(now);
        }
    }

    fn frames(fill: u8) -> [Vec<u8>; 2] {
        let solid = |v: u8| [v, v, v, 255].repeat(256 * 192);
        [solid(fill), solid(fill.wrapping_add(1))]
    }

    fn shows(remote: &Remote, screen: u8, rgba: &[u8]) -> bool {
        remote.picture(screen).is_some_and(|p| p.rgba == rgba)
    }

    fn publish(host: &mut Host, pictures: &[Vec<u8>; 2]) {
        let frames: Vec<_> = pictures
            .iter()
            .map(|rgba| ScreenFrame {
                rgba,
                width: 256,
                height: 192,
            })
            .collect();
        host.publish(&frames);
    }

    #[test]
    fn the_right_code_pairs_once() {
        let mut host = Host::new("Clara's PC");
        host.set_game(Some(ds()));
        let code = host.open_code(0.0);
        let remote = pair(&mut host, Some(code), 1.0);
        assert_eq!(remote.phase(), Phase::Joined);
        assert_eq!(remote.host_name(), "Clara's PC");
        assert_eq!(remote.mode(), &Mode::Preset(Preset::BottomScreen));
        assert_eq!(host.code(), None);
        let second = pair(&mut host, Some(code), 2.0);
        assert_eq!(second.phase(), Phase::Ended(Some(Bye::WrongCode)));
        assert_eq!(host.surfaces().len(), 1);
    }

    #[test]
    fn a_wrong_or_old_code_is_refused() {
        let mut host = Host::new("PC");
        let code = host.open_code(0.0);
        let wrong = Code::parse(if code.plain() == "111111" {
            "222222"
        } else {
            "111111"
        });
        assert_eq!(
            pair(&mut host, wrong, 1.0).phase(),
            Phase::Ended(Some(Bye::WrongCode))
        );
        assert_eq!(
            pair(&mut host, Some(code), CODE_LIFE_SECS + 1.0).phase(),
            Phase::Ended(Some(Bye::WrongCode))
        );
        assert!(host.surfaces().is_empty());
    }

    #[test]
    fn a_surface_sees_the_frames_its_mode_shows() {
        let mut host = Host::new("PC");
        host.set_game(Some(ds()));
        let code = host.open_code(0.0);
        let mut remote = pair(&mut host, Some(code), 0.0);
        let pictures = frames(10);
        publish(&mut host, &pictures);
        settle(&mut host, &mut remote, 0.1);
        assert!(shows(&remote, 1, &pictures[1]));
        assert!(remote.picture(0).is_none());
        assert!(host.shows_screen(1));
        assert!(!host.shows_screen(0));
        remote.set_mode(Mode::Custom(vec![View::Screen(0)]));
        settle(&mut host, &mut remote, 0.2);
        publish(&mut host, &pictures);
        settle(&mut host, &mut remote, 0.3);
        assert!(shows(&remote, 0, &pictures[0]));
        assert!(host.shows_screen(0));
        assert_eq!(host.surfaces()[0].mode, Mode::Custom(vec![View::Screen(0)]));
    }

    #[test]
    fn the_host_changes_the_mode_too() {
        let mut host = Host::new("PC");
        host.set_game(Some(ds()));
        let code = host.open_code(0.0);
        let mut remote = pair(&mut host, Some(code), 0.0);
        let id = host.surfaces()[0].id;
        host.set_mode(id, Mode::Preset(Preset::GameInfo));
        settle(&mut host, &mut remote, 0.1);
        assert_eq!(remote.mode(), &Mode::Preset(Preset::GameInfo));
        assert_eq!(remote.views(), vec![View::GameInfo]);
        assert_eq!(remote.game().map(|g| g.title.as_str()), Some("Pong-NDS"));
    }

    #[test]
    fn a_slow_surface_drops_frames_instead_of_queueing() {
        let mut host = Host::new("PC");
        host.set_game(Some(ds()));
        let code = host.open_code(0.0);
        let mut remote = pair(&mut host, Some(code), 0.0);
        for fill in 0..10 {
            publish(&mut host, &frames(fill));
        }
        assert_eq!(host.slots[0].in_flight.len(), FRAMES_IN_FLIGHT);
        settle(&mut host, &mut remote, 0.5);
        assert!(host.slots[0].in_flight.is_empty());
        assert!(host.surfaces()[0].ping_ms.is_some());
        publish(&mut host, &frames(20));
        settle(&mut host, &mut remote, 0.6);
        assert_eq!(remote.picture(1).map(|p| p.rgba[0]), Some(21));
    }

    #[test]
    fn touch_and_buttons_reach_the_host_and_clear_on_disconnect() {
        let mut host = Host::new("PC");
        host.set_game(Some(ds()));
        let code = host.open_code(0.0);
        let mut remote = pair(&mut host, Some(code), 0.0);
        let press = ScreenTouch {
            screen: 1,
            x: 40,
            y: 50,
        };
        remote.touch(Some(press));
        remote.buttons(0b11);
        settle(&mut host, &mut remote, 0.1);
        assert_eq!(host.touch(None), Some(press));
        assert_eq!(host.buttons(), 0b11);
        drop(remote);
        host.step(0.2);
        assert_eq!(host.touch(None), None);
        assert_eq!(host.buttons(), 0);
        assert_eq!(host.surfaces()[0].presence, Presence::Reconnecting);
        host.step(0.2 + KEEP_LOST_SECS);
        assert!(host.surfaces().is_empty());
    }

    #[test]
    fn a_touch_on_the_top_screen_is_ignored() {
        let mut host = Host::new("PC");
        host.set_game(Some(ds()));
        let code = host.open_code(0.0);
        let mut remote = pair(&mut host, Some(code), 0.0);
        remote.touch(Some(ScreenTouch {
            screen: 0,
            x: 1,
            y: 1,
        }));
        settle(&mut host, &mut remote, 0.1);
        assert_eq!(host.touch(None), None);
    }

    #[test]
    fn a_lost_surface_comes_back_with_its_token_and_mode() {
        let mut host = Host::new("PC");
        host.set_game(Some(ds()));
        let code = host.open_code(0.0);
        let mut remote = pair(&mut host, Some(code), 0.0);
        remote.set_mode(Mode::Preset(Preset::Controller));
        settle(&mut host, &mut remote, 0.1);
        let pictures = frames(3);
        publish(&mut host, &pictures);
        settle(&mut host, &mut remote, 0.1);
        host.slots[0].lose(0.2);
        remote.step(0.2);
        assert_eq!(remote.phase(), Phase::Lost);
        let (a, b) = in_app();
        host.accept(Box::new(a));
        remote.resume_over(Box::new(b));
        settle(&mut host, &mut remote, 30.0);
        assert_eq!(remote.phase(), Phase::Joined);
        assert_eq!(host.surfaces().len(), 1);
        assert_eq!(host.surfaces()[0].presence, Presence::Connected);
        assert_eq!(remote.mode(), &Mode::Preset(Preset::Controller));
        publish(&mut host, &frames(8));
        settle(&mut host, &mut remote, 30.1);
        assert_eq!(remote.picture(1).map(|p| p.rgba[0]), Some(9));
    }

    #[test]
    fn a_surface_that_leaves_is_gone_at_once() {
        let mut host = Host::new("PC");
        host.set_game(Some(ds()));
        let code = host.open_code(0.0);
        let mut remote = pair(&mut host, Some(code), 0.0);
        assert!(host.wants_frames());
        remote.leave();
        host.step(0.1);
        assert!(host.surfaces().is_empty());
        assert!(!host.wants_frames());
    }

    #[test]
    fn windows_draw_their_own_frames() {
        let mut host = Host::new("PC");
        host.set_game(Some(ds()));
        let (a, b) = in_app();
        host.accept_trusted(Box::new(a));
        let mut remote = Remote::join(Box::new(b), "Window", SurfaceKind::Window, None, None);
        settle(&mut host, &mut remote, 0.0);
        assert!(host.shows_screen(1));
        assert!(!host.wants_frames());
        publish(&mut host, &frames(1));
        settle(&mut host, &mut remote, 0.1);
        assert!(remote.picture(1).is_none());
    }

    #[test]
    fn removing_a_surface_says_goodbye() {
        let mut host = Host::new("PC");
        let code = host.open_code(0.0);
        let mut remote = pair(&mut host, Some(code), 0.0);
        host.remove(host.surfaces()[0].id);
        remote.step(0.1);
        assert_eq!(remote.phase(), Phase::Ended(Some(Bye::Removed)));
    }

    #[test]
    fn trusted_surfaces_need_no_code_and_leave_at_once() {
        let mut host = Host::new("PC");
        let (a, b) = in_app();
        host.accept_trusted(Box::new(a));
        let mut remote = Remote::join(Box::new(b), "Window", SurfaceKind::Window, None, None);
        settle(&mut host, &mut remote, 0.0);
        assert_eq!(remote.phase(), Phase::Joined);
        drop(remote);
        host.step(0.1);
        assert!(host.surfaces().is_empty());
    }

    #[test]
    fn an_old_app_is_told_to_update() {
        let mut host = Host::new("PC");
        let (a, mut b) = in_app();
        host.accept(Box::new(a));
        b.send(
            SurfaceMsg::Hello {
                version: SURFACE_VERSION + 1,
                name: "x".into(),
                kind: SurfaceKind::Device,
                code: None,
                resume: None,
            }
            .encode(),
        );
        host.step(0.0);
        assert_eq!(
            b.poll(),
            Poll::Message(HostMsg::Bye(Bye::NeedsUpdate).encode())
        );
    }

    #[test]
    fn a_silent_link_is_dropped() {
        let mut host = Host::new("PC");
        let (a, _b) = in_app();
        host.accept(Box::new(a));
        host.step(0.0);
        assert_eq!(host.waiting.len(), 1);
        host.step(HELLO_WAIT_SECS + 1.0);
        assert!(host.waiting.is_empty());
    }

    #[test]
    fn actions_reach_the_host() {
        let mut host = Host::new("PC");
        let code = host.open_code(0.0);
        let mut remote = pair(&mut host, Some(code), 0.0);
        remote.action(Action::Pause);
        settle(&mut host, &mut remote, 0.1);
        assert_eq!(host.take_actions(), vec![Action::Pause]);
        assert!(host.take_actions().is_empty());
    }

    fn ask_as(host: &mut Host, name: &str, proof: Option<ProofKey>, now: f64) -> Remote {
        let (a, b) = in_app();
        host.accept(Box::new(a));
        let mut remote = Remote::join(Box::new(b), name, SurfaceKind::Device, None, None);
        if let Some(key) = proof {
            remote.prove_with(key);
        }
        settle(host, &mut remote, now);
        remote
    }

    fn ask(host: &mut Host, now: f64) -> Remote {
        ask_as(host, "Pixel 8", None, now)
    }

    fn playing(name: &str) -> Host {
        let mut host = Host::new(name);
        host.set_game(Some(ds()));
        host
    }

    const OWN: ProofKey = ProofKey([9; 32]);

    #[test]
    fn a_hello_without_a_code_waits_for_allow() {
        let mut host = playing("Clara's Mac");
        let mut remote = ask(&mut host, 1.0);
        assert_eq!(remote.phase(), Phase::Asking);
        let askers = host.askers();
        assert_eq!(askers.len(), 1);
        assert_eq!(askers[0].name, "Pixel 8");
        assert!(host.surfaces().is_empty());
        host.allow(askers[0].id);
        settle(&mut host, &mut remote, 2.0);
        assert_eq!(remote.phase(), Phase::Joined);
        assert_eq!(host.surfaces().len(), 1);
        assert!(host.askers().is_empty());
    }

    #[test]
    fn not_now_ends_the_join() {
        let mut host = playing("Mac");
        let mut remote = ask(&mut host, 1.0);
        let id = host.askers()[0].id;
        host.decline(id);
        settle(&mut host, &mut remote, 2.0);
        assert_eq!(remote.phase(), Phase::Ended(Some(Bye::Declined)));
        assert!(host.surfaces().is_empty());
        assert!(host.askers().is_empty());
    }

    #[test]
    fn a_phone_that_leaves_drops_its_ask() {
        let mut host = playing("Mac");
        let mut remote = ask(&mut host, 1.0);
        remote.leave();
        settle(&mut host, &mut remote, 2.0);
        assert!(host.askers().is_empty());
    }

    #[test]
    fn asks_are_capped() {
        let mut host = playing("Mac");
        let remotes: Vec<_> = (0..MAX_ASKS)
            .map(|at| ask_as(&mut host, &format!("Phone {at}"), None, 1.0))
            .collect();
        assert!(remotes.iter().all(|r| r.phase() == Phase::Asking));
        let extra = ask_as(&mut host, "One more", None, 1.0);
        assert_eq!(extra.phase(), Phase::Ended(Some(Bye::Declined)));
        assert_eq!(host.askers().len(), MAX_ASKS);
    }

    #[test]
    fn a_proof_with_the_own_key_skips_the_ask() {
        let mut host = playing("Mac");
        host.set_proof_key(Some(OWN));
        let stranger = ask_as(&mut host, "Stranger", Some(ProofKey([8; 32])), 1.0);
        assert_eq!(stranger.phase(), Phase::Asking);
        let own = ask_as(&mut host, "Pixel 8", Some(OWN), 1.0);
        assert_eq!(own.phase(), Phase::Joined);
        assert_eq!(host.askers().len(), 1);
        assert_eq!(host.surfaces().len(), 1);
    }

    #[test]
    fn a_host_without_a_proof_key_asks_its_own_devices() {
        let mut host = playing("Mac");
        let own = ask_as(&mut host, "Pixel 8", Some(OWN), 1.0);
        assert_eq!(own.phase(), Phase::Asking);
        assert!(host.surfaces().is_empty());
    }

    #[test]
    fn an_own_proof_needs_no_code_and_keeps_the_open_one() {
        let mut host = playing("Mac");
        host.set_proof_key(Some(OWN));
        let code = host.open_code(0.0);
        let own = ask_as(&mut host, "Pixel 8", Some(OWN), 1.0);
        assert_eq!(own.phase(), Phase::Joined);
        assert_eq!(host.code().map(|(c, _)| c), Some(code));
    }

    type Seen = std::rc::Rc<std::cell::RefCell<Vec<Vec<u8>>>>;

    struct Tap {
        inner: crate::link::InApp,
        sent: Seen,
        heard: Seen,
    }

    impl Link for Tap {
        fn send(&mut self, bytes: Vec<u8>) {
            self.sent.borrow_mut().push(bytes.clone());
            self.inner.send(bytes);
        }

        fn poll(&mut self) -> Poll {
            let poll = self.inner.poll();
            if let Poll::Message(bytes) = &poll {
                self.heard.borrow_mut().push(bytes.clone());
            }
            poll
        }

        fn route(&self) -> Route {
            self.inner.route()
        }
    }

    fn tapped_own_join(host: &mut Host, now: f64) -> (Remote, Seen, Seen) {
        let (a, b) = in_app();
        host.accept(Box::new(a));
        let (sent, heard) = (Seen::default(), Seen::default());
        let tap = Tap {
            inner: b,
            sent: Seen::clone(&sent),
            heard: Seen::clone(&heard),
        };
        let mut own = Remote::join(Box::new(tap), "Pixel 8", SurfaceKind::Device, None, None);
        own.prove_with(OWN);
        settle(host, &mut own, now);
        (own, sent, heard)
    }

    #[test]
    fn no_message_on_the_link_carries_the_proof_key() {
        let mut host = playing("Mac");
        host.set_proof_key(Some(OWN));
        let (own, sent, heard) = tapped_own_join(&mut host, 1.0);
        assert_eq!(own.phase(), Phase::Joined);
        let sent = sent.borrow();
        let heard = heard.borrow();
        assert!(
            sent.iter()
                .any(|m| matches!(SurfaceMsg::decode(m), Some(SurfaceMsg::Proof(Some(_)))))
        );
        for message in sent.iter().chain(heard.iter()) {
            assert!(!message.windows(OWN.0.len()).any(|w| w == OWN.0));
        }
    }

    #[test]
    fn a_replayed_own_join_is_asked_not_admitted() {
        let mut host = playing("Mac");
        host.set_proof_key(Some(OWN));
        let (own, sent, _) = tapped_own_join(&mut host, 1.0);
        assert_eq!(own.phase(), Phase::Joined);
        let (a, mut b) = in_app();
        host.accept(Box::new(a));
        for bytes in sent.borrow().iter() {
            b.send(bytes.clone());
        }
        let mut heard = Vec::new();
        for _ in 0..4 {
            host.step(2.0);
            while let Poll::Message(bytes) = b.poll() {
                heard.extend(HostMsg::decode(&bytes));
            }
        }
        assert_eq!(host.surfaces().len(), 1);
        assert_eq!(host.askers().len(), 1);
        assert!(heard.contains(&HostMsg::Asking));
        assert!(!heard.iter().any(|m| matches!(m, HostMsg::Welcome { .. })));
    }

    #[test]
    fn a_proof_for_another_challenge_is_asked_not_admitted() {
        let mut host = playing("Mac");
        host.set_proof_key(Some(OWN));
        let (a, mut b) = in_app();
        host.accept(Box::new(a));
        b.send(
            SurfaceMsg::Hello {
                version: SURFACE_VERSION,
                name: "Forger".into(),
                kind: SurfaceKind::Device,
                code: None,
                resume: None,
            }
            .encode(),
        );
        host.step(1.0);
        let Poll::Message(bytes) = b.poll() else {
            panic!("the host challenges");
        };
        let Some(HostMsg::Challenge { nonce, host: id }) = HostMsg::decode(&bytes) else {
            panic!("a challenge");
        };
        let mut other = nonce;
        other[0] ^= 1;
        b.send(SurfaceMsg::Proof(Some(OWN.prove(&other, id))).encode());
        host.step(1.1);
        assert!(host.surfaces().is_empty());
        assert_eq!(host.askers().len(), 1);
    }

    #[test]
    fn a_silent_link_never_raises_a_prompt() {
        let mut host = playing("Mac");
        let (a, mut b) = in_app();
        host.accept(Box::new(a));
        b.send(
            SurfaceMsg::Hello {
                version: SURFACE_VERSION,
                name: "Quiet".into(),
                kind: SurfaceKind::Device,
                code: None,
                resume: None,
            }
            .encode(),
        );
        host.step(1.0);
        assert!(host.askers().is_empty());
        host.step(1.0 + HELLO_WAIT_SECS);
        let real = ask(&mut host, 1.0 + HELLO_WAIT_SECS);
        assert_eq!(real.phase(), Phase::Asking);
        assert_eq!(host.askers().len(), 1);
    }

    #[test]
    fn an_unanswered_ask_expires_and_tells_the_phone() {
        let mut host = playing("Mac");
        let mut remote = ask(&mut host, 1.0);
        assert_eq!(host.askers().len(), 1);
        settle(&mut host, &mut remote, ASK_SECS);
        assert_eq!(host.askers().len(), 1);
        settle(&mut host, &mut remote, 1.0 + ASK_SECS);
        assert!(host.askers().is_empty());
        assert_eq!(remote.phase(), Phase::Ended(Some(Bye::NoAnswer)));
    }

    #[test]
    fn a_new_ask_replaces_the_senders_old_one() {
        let mut host = playing("Mac");
        let mut first = ask(&mut host, 1.0);
        let second = ask(&mut host, 1.0);
        settle(&mut host, &mut first, 1.0);
        assert_eq!(host.askers().len(), 1);
        assert!(matches!(first.phase(), Phase::Ended(_)));
        assert_eq!(second.phase(), Phase::Asking);
        let others: Vec<_> = (0..MAX_ASKS - 1)
            .map(|at| ask_as(&mut host, &format!("Phone {at}"), None, 1.0))
            .collect();
        assert!(others.iter().all(|r| r.phase() == Phase::Asking));
        let again = ask(&mut host, 1.0);
        assert_eq!(again.phase(), Phase::Asking);
        assert_eq!(host.askers().len(), MAX_ASKS);
    }

    #[test]
    fn no_game_means_no_prompt() {
        let mut host = Host::new("Mac");
        let remote = ask(&mut host, 1.0);
        assert!(host.askers().is_empty());
        assert_eq!(remote.phase(), Phase::Ended(Some(Bye::NoGame)));
        let code = host.open_code(1.0);
        let added = pair(&mut host, Some(code), 1.0);
        assert_eq!(added.phase(), Phase::Joined);
    }

    #[test]
    fn a_game_that_stops_closes_its_prompts() {
        let mut host = playing("Mac");
        let mut remote = ask(&mut host, 1.0);
        assert_eq!(host.askers().len(), 1);
        host.set_game(None);
        settle(&mut host, &mut remote, 2.0);
        assert!(host.askers().is_empty());
        assert_eq!(remote.phase(), Phase::Ended(Some(Bye::NoGame)));
    }

    #[test]
    fn a_long_name_is_cut_before_the_prompt() {
        let mut host = playing("Mac");
        let long = "x".repeat(100);
        ask_as(&mut host, &long, None, 1.0);
        assert_eq!(host.askers()[0].name.chars().count(), MAX_NAME_CHARS);
    }

    fn old_hello(host: &mut Host, name: &str, now: f64) {
        let (a, mut b) = in_app();
        host.accept(Box::new(a));
        b.send(
            SurfaceMsg::Hello {
                version: SURFACE_VERSION - 1,
                name: name.into(),
                kind: SurfaceKind::Device,
                code: None,
                resume: None,
            }
            .encode(),
        );
        host.step(now);
    }

    #[test]
    fn needs_update_is_named_once_a_minute_per_device() {
        let mut host = Host::new("Mac");
        old_hello(&mut host, "Old phone", 1.0);
        old_hello(&mut host, "Old phone", 2.0);
        old_hello(&mut host, "Old tablet", 2.0);
        assert_eq!(
            host.take_outdated(),
            vec!["Old phone".to_string(), "Old tablet".to_string()]
        );
        old_hello(&mut host, "Old phone", 1.0 + NEEDS_UPDATE_EVERY_SECS);
        assert_eq!(host.take_outdated(), vec!["Old phone".to_string()]);
    }

    #[test]
    fn an_old_surface_hears_needs_update_and_the_host_names_it() {
        let mut host = Host::new("Mac");
        let (a, mut b) = in_app();
        host.accept(Box::new(a));
        b.send(
            SurfaceMsg::Hello {
                version: SURFACE_VERSION - 1,
                name: "Old phone".into(),
                kind: SurfaceKind::Device,
                code: None,
                resume: None,
            }
            .encode(),
        );
        host.step(1.0);
        let Poll::Message(bytes) = b.poll() else {
            panic!("the host answers");
        };
        assert_eq!(
            HostMsg::decode(&bytes),
            Some(HostMsg::Bye(Bye::NeedsUpdate))
        );
        assert_eq!(host.take_outdated(), vec!["Old phone".to_string()]);
        assert!(host.take_outdated().is_empty());
    }
}
