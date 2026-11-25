use crate::codec::{Decoder, MAX_SCREENS};
use crate::link::{Link, Poll, Route};
use crate::mode::{Mode, View};
use crate::pairing::Code;
use crate::proof::{CodeKnock, LinkKey, OPEN_LINK, ProofKey, resume_link};
use crate::proto::{
    Action, Bye, Entry, GameCard, Hello, HostMsg, SurfaceId, SurfaceKind, SurfaceMsg, Wire,
};
use crate::secure::Sealed;
use crate::touch::ScreenTouch;
use citrine_gb_net::{CheckCode, CheckNonce, KeyedDial};

pub const GIVE_UP_SECS: f64 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Joining,
    Asking,
    Joined,
    Lost,
    Ended(Option<Bye>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Knock {
    Own(ProofKey),
    Code(Code),
    Asked,
}

enum Key {
    Known(LinkKey),
    Code(CodeKnock),
}

impl Key {
    fn agree(self, pake: &[u8]) -> Option<LinkKey> {
        match self {
            Self::Known(key) => Some(key),
            Self::Code(knock) => knock.finish(pake),
        }
    }
}

struct Dialing {
    link: Box<dyn Link>,
    dial: KeyedDial,
    key: Key,
    check: Option<CheckNonce>,
}

#[derive(Debug, Default, Clone)]
pub struct Picture {
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    pub seq: u32,
}

pub struct Remote {
    link: Option<Box<dyn Link>>,
    dialing: Option<Dialing>,
    route: Route,
    name: String,
    kind: SurfaceKind,
    knock: Knock,
    resume: Option<LinkKey>,
    binding: Option<[u8; 32]>,
    check_code: Option<CheckCode>,
    resuming: bool,
    id: Option<SurfaceId>,
    knock_again: bool,
    plain_bye: Option<Bye>,
    host: String,
    phase: Phase,
    lost_at: f64,
    now: f64,
    mode: Mode,
    game: Option<GameCard>,
    pictures: Vec<Option<Picture>>,
    decoders: Vec<Decoder>,
    sent_touch: Option<Option<ScreenTouch>>,
    sent_buttons: Option<u16>,
    want_touch: Option<ScreenTouch>,
    want_buttons: u16,
}

impl Remote {
    pub fn join(
        link: Box<dyn Link>,
        name: impl Into<String>,
        kind: SurfaceKind,
        knock: Knock,
    ) -> Self {
        let mut remote = Self {
            route: link.route(),
            link: None,
            dialing: None,
            name: name.into(),
            kind,
            knock,
            resume: None,
            binding: None,
            check_code: None,
            resuming: false,
            id: None,
            knock_again: false,
            plain_bye: None,
            host: String::new(),
            phase: Phase::Joining,
            lost_at: 0.0,
            now: 0.0,
            mode: Mode::default(),
            game: None,
            pictures: Vec::new(),
            decoders: Vec::new(),
            sent_touch: None,
            sent_buttons: None,
            want_touch: None,
            want_buttons: 0,
        };
        remote.dial(link);
        remote
    }

    fn dial(&mut self, mut link: Box<dyn Link>) {
        let mut pake = Vec::new();
        let mut check = None;
        let (entry, key) = match (self.id, self.resume) {
            (Some(id), Some(resume)) if !self.knock_again => {
                (Entry::Resume(id), Key::Known(resume))
            }
            _ => match self.knock {
                Knock::Own(key) => (Entry::Own, Key::Known(key.link())),
                Knock::Code(code) => {
                    let (knock, first) = CodeKnock::start(code);
                    pake = first;
                    (Entry::Code, Key::Code(knock))
                }
                Knock::Asked => {
                    let nonce = CheckNonce::random();
                    check = Some(nonce);
                    (Entry::Asked(nonce.commit()), Key::Known(OPEN_LINK))
                }
            },
        };
        let mut hello = Hello::new(self.name.clone(), self.kind, entry);
        hello.pake = pake;
        self.resuming = matches!(entry, Entry::Resume(_));
        self.binding = None;
        self.check_code = None;
        self.route = link.route();
        self.link = None;
        self.dialing = KeyedDial::start(&hello.prologue())
            .ok()
            .map(|(dial, first)| {
                hello.handshake = first;
                link.send(SurfaceMsg::Hello(hello).encode());
                Dialing {
                    link,
                    dial,
                    key,
                    check,
                }
            });
        self.sent_touch = None;
        self.sent_buttons = None;
        if self.phase != Phase::Lost {
            self.phase = Phase::Joining;
        }
    }

    pub fn resume_over(&mut self, link: Box<dyn Link>) {
        if matches!(self.phase, Phase::Ended(_)) {
            return;
        }
        self.dial(link);
    }

    pub fn cut(&mut self) {
        self.link = None;
        self.dialing = None;
        if self.phase == Phase::Joined {
            self.lost_at = self.now;
            self.phase = Phase::Lost;
        }
    }

    pub fn leave(&mut self) {
        self.send(SurfaceMsg::Leave);
        self.link = None;
        self.dialing = None;
        self.phase = Phase::Ended(None);
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn route(&self) -> Route {
        self.route
    }

    pub fn check_code(&self) -> Option<CheckCode> {
        self.check_code
    }

    pub fn can_resume(&self) -> bool {
        self.resume.is_some()
    }

    pub fn lost_for(&self) -> f64 {
        if self.phase == Phase::Lost {
            self.now - self.lost_at
        } else {
            0.0
        }
    }

    pub fn host_name(&self) -> &str {
        &self.host
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    pub fn game(&self) -> Option<&GameCard> {
        self.game.as_ref()
    }

    pub fn views(&self) -> Vec<View> {
        let screens = self
            .game
            .as_ref()
            .map(|g| g.screens.as_slice())
            .unwrap_or(&[]);
        self.mode
            .views(screens)
            .into_iter()
            .filter(View::travels)
            .collect()
    }

    pub fn picture(&self, screen: u8) -> Option<&Picture> {
        self.pictures.get(usize::from(screen))?.as_ref()
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode.clone();
        self.send(SurfaceMsg::SetMode(mode));
    }

    pub fn touch(&mut self, touch: Option<ScreenTouch>) {
        self.want_touch = touch;
        self.flush_input();
    }

    pub fn buttons(&mut self, buttons: u16) {
        self.want_buttons = buttons;
        self.flush_input();
    }

    pub fn action(&mut self, action: Action) {
        self.send(SurfaceMsg::Action(action));
    }

    fn flush_input(&mut self) {
        if self.phase != Phase::Joined {
            return;
        }
        if self.sent_touch != Some(self.want_touch) {
            self.sent_touch = Some(self.want_touch);
            self.send(SurfaceMsg::Touch(self.want_touch));
        }
        if self.sent_buttons != Some(self.want_buttons) {
            self.sent_buttons = Some(self.want_buttons);
            self.send(SurfaceMsg::Buttons(self.want_buttons));
        }
    }

    fn send(&mut self, msg: SurfaceMsg) {
        if let Some(link) = &mut self.link {
            link.send(msg.encode());
        }
    }

    pub fn step(&mut self, now: f64) -> bool {
        self.now = now;
        let mut changed = self.open(now);
        while let Some(link) = &mut self.link {
            match link.poll() {
                Poll::Message(bytes) => {
                    if let Some(msg) = HostMsg::decode(&bytes) {
                        self.receive(msg);
                        changed = true;
                    }
                }
                Poll::Idle => break,
                Poll::Closed => {
                    self.link = None;
                    self.closed(now);
                    changed = true;
                    break;
                }
            }
        }
        if self.phase == Phase::Lost && now - self.lost_at > GIVE_UP_SECS {
            self.link = None;
            self.dialing = None;
            self.phase = Phase::Ended(self.plain_bye);
            changed = true;
        }
        self.flush_input();
        changed
    }

    fn open(&mut self, now: f64) -> bool {
        let Some(dialing) = &mut self.dialing else {
            return false;
        };
        loop {
            match dialing.link.poll() {
                Poll::Message(bytes) => match HostMsg::decode(&bytes) {
                    Some(HostMsg::Opened { reply, pake }) => {
                        let Some(Dialing {
                            link,
                            dial,
                            key,
                            check,
                        }) = self.dialing.take()
                        else {
                            return false;
                        };
                        let opened = key
                            .agree(&pake)
                            .and_then(|key| dial.finish(&key, &reply).ok());
                        match opened {
                            Some(channel) => {
                                let binding = channel.binding();
                                self.binding = Some(binding);
                                self.check_code =
                                    check.map(|nonce| CheckCode::new(&binding, &nonce));
                                let ready = check.map_or(SurfaceMsg::Ready, SurfaceMsg::Check);
                                let mut sealed = Sealed::new(link, channel);
                                sealed.send(ready.encode());
                                self.link = Some(Box::new(sealed));
                            }
                            None => self.refused(now),
                        }
                        return true;
                    }
                    Some(HostMsg::Bye(bye)) if self.phase == Phase::Lost => {
                        self.dialing = None;
                        self.plain_bye = Some(bye);
                        self.knock_again =
                            bye == Bye::Removed && matches!(self.knock, Knock::Own(_));
                        return true;
                    }
                    Some(HostMsg::Bye(bye)) => {
                        self.dialing = None;
                        self.phase = Phase::Ended(Some(bye));
                        return true;
                    }
                    _ => {}
                },
                Poll::Idle => return false,
                Poll::Closed => {
                    self.dialing = None;
                    self.closed(now);
                    return true;
                }
            }
        }
    }

    fn refused(&mut self, now: f64) {
        if self.phase == Phase::Joining && matches!(self.knock, Knock::Code(_)) {
            self.phase = Phase::Ended(Some(Bye::WrongCode));
        } else {
            self.closed(now);
        }
    }

    fn closed(&mut self, now: f64) {
        if matches!(self.phase, Phase::Joined | Phase::Lost) && self.resume.is_some() {
            if self.phase == Phase::Joined {
                self.lost_at = now;
            }
            self.phase = Phase::Lost;
        } else if !matches!(self.phase, Phase::Ended(_)) {
            self.phase = Phase::Ended(None);
        }
    }

    fn receive(&mut self, msg: HostMsg) {
        match msg {
            HostMsg::Welcome { id, host } => {
                if !self.resuming {
                    self.resume = self.binding.as_ref().map(resume_link);
                }
                self.id = Some(id);
                self.knock_again = false;
                self.plain_bye = None;
                self.host = host;
                self.phase = Phase::Joined;
                self.decoders.clear();
            }
            HostMsg::Game(game) => {
                if game.as_ref().map(|g| &g.screens) != self.game.as_ref().map(|g| &g.screens) {
                    self.pictures.clear();
                    self.decoders.clear();
                }
                self.game = game;
            }
            HostMsg::Mode(mode) => self.mode = mode,
            HostMsg::Frame(frame) => {
                let at = usize::from(frame.screen);
                if at >= MAX_SCREENS {
                    return;
                }
                if self.decoders.len() <= at {
                    self.decoders.resize_with(at + 1, Decoder::default);
                    self.pictures.resize_with(at + 1, || None);
                }
                if let Some(rgba) = self.decoders[at].decode(&frame) {
                    self.pictures[at] = Some(Picture {
                        width: frame.width,
                        height: frame.height,
                        rgba: rgba.to_vec(),
                        seq: frame.seq,
                    });
                }
                self.send(SurfaceMsg::Ack {
                    screen: frame.screen,
                    seq: frame.seq,
                });
            }
            HostMsg::Bye(bye) => {
                self.link = None;
                self.dialing = None;
                self.phase = Phase::Ended(Some(bye));
            }
            HostMsg::Asking => {
                if self.phase == Phase::Joining {
                    self.phase = Phase::Asking;
                }
            }
            HostMsg::Opened { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{InApp, in_app};
    use crate::proof::code_answer;
    use citrine_gb_net::{Channel, keyed_answer};

    fn hello_on(host: &mut InApp) -> Hello {
        let Poll::Message(bytes) = host.poll() else {
            panic!("the remote says hello");
        };
        let Some(SurfaceMsg::Hello(hello)) = SurfaceMsg::decode(&bytes) else {
            panic!("a hello");
        };
        hello
    }

    #[derive(Clone, Copy)]
    enum Answer {
        Key(LinkKey),
        Code(Code),
    }

    fn answer(remote: &mut Remote, host: &mut InApp, with: Answer) -> Option<Channel> {
        let hello = hello_on(host);
        let (key, pake) = match with {
            Answer::Key(key) => (key, Vec::new()),
            Answer::Code(code) => {
                let (pake, key) = code_answer(code, &hello.pake).unwrap();
                (key, pake)
            }
        };
        let (reply, mut channel) = keyed_answer(&hello.prologue(), &key, &hello.handshake).unwrap();
        host.send(HostMsg::Opened { reply, pake }.encode());
        remote.step(0.0);
        let Poll::Message(ready) = host.poll() else {
            return None;
        };
        let proven = channel
            .open(&ready)
            .ok()
            .and_then(|bytes| SurfaceMsg::decode(&bytes));
        matches!(proven, Some(SurfaceMsg::Ready | SurfaceMsg::Check(_))).then_some(channel)
    }

    #[test]
    fn an_asked_phone_shows_the_code_of_the_nonce_it_committed_to() {
        let (mut host, b) = in_app();
        let mut remote = Remote::join(Box::new(b), "Phone", SurfaceKind::Device, Knock::Asked);
        let hello = hello_on(&mut host);
        let Entry::Asked(commit) = hello.entry else {
            panic!("an asked hello");
        };
        assert_eq!(remote.check_code(), None);
        let (reply, mut channel) =
            keyed_answer(&hello.prologue(), &OPEN_LINK, &hello.handshake).unwrap();
        host.send(
            HostMsg::Opened {
                reply,
                pake: Vec::new(),
            }
            .encode(),
        );
        remote.step(0.0);
        let Poll::Message(sealed) = host.poll() else {
            panic!("the phone answers");
        };
        let Some(SurfaceMsg::Check(nonce)) = SurfaceMsg::decode(&channel.open(&sealed).unwrap())
        else {
            panic!("the phone reveals its nonce");
        };
        assert_eq!(nonce.commit(), commit);
        assert_eq!(
            remote.check_code(),
            Some(CheckCode::new(&channel.binding(), &nonce))
        );
    }

    #[test]
    fn each_asked_join_commits_to_a_new_nonce() {
        let (mut host, b) = in_app();
        let _first = Remote::join(Box::new(b), "Phone", SurfaceKind::Device, Knock::Asked);
        let (mut again, c) = in_app();
        let _second = Remote::join(Box::new(c), "Phone", SurfaceKind::Device, Knock::Asked);
        assert_ne!(hello_on(&mut host).entry, hello_on(&mut again).entry);
    }

    fn answered(knock: Knock, with: Answer) -> (Remote, InApp, Option<Channel>) {
        let (mut host, b) = in_app();
        let mut remote = Remote::join(Box::new(b), "Phone", SurfaceKind::Device, knock);
        let channel = answer(&mut remote, &mut host, with);
        (remote, host, channel)
    }

    fn opened(knock: Knock) -> (Remote, Sealed, [u8; 32]) {
        let with = match knock {
            Knock::Own(key) => Answer::Key(key.link()),
            Knock::Code(code) => Answer::Code(code),
            Knock::Asked => Answer::Key(OPEN_LINK),
        };
        let (remote, host, channel) = answered(knock, with);
        let channel = channel.expect("the remote proves its key");
        let binding = channel.binding();
        (remote, Sealed::new(Box::new(host), channel), binding)
    }

    fn welcome(host: &mut dyn Link) {
        host.send(
            HostMsg::Welcome {
                id: SurfaceId(1),
                host: "PC".into(),
            }
            .encode(),
        );
    }

    #[test]
    fn input_waits_for_the_welcome_and_sends_only_changes() {
        let (mut remote, mut host, _) = opened(Knock::Asked);
        remote.buttons(1);
        assert_eq!(host.poll(), Poll::Idle);
        welcome(&mut host);
        remote.step(0.0);
        let mut got = Vec::new();
        while let Poll::Message(bytes) = host.poll() {
            got.push(SurfaceMsg::decode(&bytes).unwrap());
        }
        assert_eq!(got, vec![SurfaceMsg::Touch(None), SurfaceMsg::Buttons(1)]);
        remote.buttons(1);
        remote.step(0.1);
        assert_eq!(host.poll(), Poll::Idle);
    }

    #[test]
    fn a_closed_link_before_the_welcome_ends_the_join() {
        let (host, b) = in_app();
        let mut remote = Remote::join(Box::new(b), "Phone", SurfaceKind::Device, Knock::Asked);
        drop(host);
        remote.step(0.0);
        assert_eq!(remote.phase(), Phase::Ended(None));
    }

    #[test]
    fn a_lost_host_is_given_up_after_a_minute() {
        let (mut remote, mut host, _) = opened(Knock::Asked);
        welcome(&mut host);
        remote.step(0.0);
        drop(host);
        remote.step(1.0);
        assert_eq!(remote.phase(), Phase::Lost);
        assert!(remote.can_resume());
        remote.step(30.0);
        assert_eq!(remote.phase(), Phase::Lost);
        assert!((remote.lost_for() - 29.0).abs() < 1e-9);
        remote.step(61.5);
        assert_eq!(remote.phase(), Phase::Ended(None));
    }

    #[test]
    fn the_phone_refuses_a_host_without_its_key() {
        let own = Knock::Own(ProofKey([1; 32]));
        let (remote, _, proven) = answered(own, Answer::Key(ProofKey([2; 32]).link()));
        assert_eq!(remote.phase(), Phase::Ended(None));
        assert!(proven.is_none());
        let (remote, _, proven) = answered(own, Answer::Key(OPEN_LINK));
        assert_eq!(remote.phase(), Phase::Ended(None));
        assert!(proven.is_none());
        let (remote, _, proven) = answered(own, Answer::Key(ProofKey([1; 32]).link()));
        assert_eq!(remote.phase(), Phase::Joining);
        assert!(proven.is_some());
    }

    #[test]
    fn a_wrong_code_shows_as_a_wrong_code() {
        let code = Knock::Code(Code::parse("111 111").unwrap());
        let other = Answer::Code(Code::parse("222 222").unwrap());
        let (remote, _, proven) = answered(code, other);
        assert_eq!(remote.phase(), Phase::Ended(Some(Bye::WrongCode)));
        assert!(proven.is_none());
    }

    #[test]
    fn a_plain_needs_update_from_an_old_host_ends_the_join() {
        let (mut host, b) = in_app();
        let mut remote = Remote::join(Box::new(b), "Phone", SurfaceKind::Device, Knock::Asked);
        assert_eq!(hello_on(&mut host).version, crate::proto::SURFACE_VERSION);
        host.send(vec![4, 2]);
        remote.step(0.0);
        assert_eq!(remote.phase(), Phase::Ended(Some(Bye::NeedsUpdate)));
        assert_eq!(Bye::NeedsUpdate.label(), "Update Citrine on both devices");
    }

    #[test]
    fn a_plain_message_after_the_handshake_cuts_the_link() {
        let (mut remote, mut host, _) = answered(Knock::Asked, Answer::Key(OPEN_LINK));
        host.send(
            HostMsg::Welcome {
                id: SurfaceId(1),
                host: "Middle".into(),
            }
            .encode(),
        );
        remote.step(0.0);
        assert_eq!(remote.phase(), Phase::Ended(None));
        assert_eq!(remote.host_name(), "");
    }

    #[test]
    fn a_rejoin_after_the_welcome_knocks_with_the_key_its_link_agreed() {
        let (mut remote, mut host, binding) = opened(Knock::Code(Code::parse("123 456").unwrap()));
        welcome(&mut host);
        remote.step(0.0);
        drop(host);
        remote.step(1.0);
        let (mut again, b) = in_app();
        remote.resume_over(Box::new(b));
        let hello = hello_on(&mut again);
        assert_eq!(
            (hello.entry, hello.pake),
            (Entry::Resume(SurfaceId(1)), Vec::new())
        );
        let (mut again, b) = in_app();
        remote.resume_over(Box::new(b));
        let resume = Answer::Key(resume_link(&binding));
        let channel = answer(&mut remote, &mut again, resume).expect("the agreed key");
        let mut again = Sealed::new(Box::new(again), channel);
        welcome(&mut again);
        remote.step(2.0);
        assert_eq!(remote.phase(), Phase::Joined);
        drop(again);
        remote.step(3.0);
        let (mut third, b) = in_app();
        remote.resume_over(Box::new(b));
        assert!(answer(&mut remote, &mut third, resume).is_some());
    }

    #[test]
    fn a_resume_answered_without_the_agreed_key_gets_no_link() {
        let (mut remote, mut host, binding) = opened(Knock::Asked);
        welcome(&mut host);
        remote.step(0.0);
        drop(host);
        remote.step(1.0);
        for wrong in [OPEN_LINK, resume_link(&[1; 32]), binding] {
            let (mut forger, b) = in_app();
            remote.resume_over(Box::new(b));
            assert!(answer(&mut remote, &mut forger, Answer::Key(wrong)).is_none());
            assert_eq!(remote.phase(), Phase::Lost);
        }
    }

    #[test]
    fn a_plain_removed_keeps_a_lost_phones_key() {
        let (mut remote, mut host, _) = opened(Knock::Code(Code::parse("123 456").unwrap()));
        welcome(&mut host);
        remote.step(0.0);
        drop(host);
        remote.step(1.0);
        assert_eq!(remote.phase(), Phase::Lost);
        let (mut forger, b) = in_app();
        remote.resume_over(Box::new(b));
        assert_eq!(hello_on(&mut forger).entry, Entry::Resume(SurfaceId(1)));
        forger.send(HostMsg::Bye(Bye::Removed).encode());
        remote.step(2.0);
        assert_eq!(remote.phase(), Phase::Lost);
        assert!(remote.can_resume());
        let (mut again, b) = in_app();
        remote.resume_over(Box::new(b));
        assert_eq!(hello_on(&mut again).entry, Entry::Resume(SurfaceId(1)));
        remote.step(62.0);
        assert_eq!(remote.phase(), Phase::Ended(Some(Bye::Removed)));
    }
}
