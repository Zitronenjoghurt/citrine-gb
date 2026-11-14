use crate::codec::Decoder;
use crate::link::{Link, Poll, Route};
use crate::mode::{Mode, View};
use crate::pairing::Code;
use crate::proof::ProofKey;
use crate::proto::{
    Action, Bye, GameCard, HostMsg, Resume, SURFACE_VERSION, SurfaceId, SurfaceKind, SurfaceMsg,
    Wire,
};
use crate::touch::ScreenTouch;

pub const GIVE_UP_SECS: f64 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Joining,
    Asking,
    Joined,
    Lost,
    Ended(Option<Bye>),
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
    route: Route,
    name: String,
    kind: SurfaceKind,
    code: Option<Code>,
    resume: Option<Resume>,
    proof_key: Option<ProofKey>,
    id: Option<SurfaceId>,
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
        code: Option<Code>,
        resume: Option<Resume>,
    ) -> Self {
        let mut remote = Self {
            route: link.route(),
            link: None,
            name: name.into(),
            kind,
            code,
            resume,
            proof_key: None,
            id: None,
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
        link.send(
            SurfaceMsg::Hello {
                version: SURFACE_VERSION,
                name: self.name.clone(),
                kind: self.kind,
                code: self.code,
                resume: self.resume,
            }
            .encode(),
        );
        self.route = link.route();
        self.link = Some(link);
        self.sent_touch = None;
        self.sent_buttons = None;
        if self.phase != Phase::Lost {
            self.phase = Phase::Joining;
        }
    }

    pub fn prove_with(&mut self, key: ProofKey) {
        self.proof_key = Some(key);
    }

    pub fn resume_over(&mut self, link: Box<dyn Link>) {
        if matches!(self.phase, Phase::Ended(_)) {
            return;
        }
        self.dial(link);
    }

    pub fn cut(&mut self) {
        self.link = None;
        if self.phase == Phase::Joined {
            self.lost_at = self.now;
            self.phase = Phase::Lost;
        }
    }

    pub fn leave(&mut self) {
        self.send(SurfaceMsg::Leave);
        self.link = None;
        self.phase = Phase::Ended(None);
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn route(&self) -> Route {
        self.route
    }

    pub fn code(&self) -> Option<Code> {
        self.code
    }

    pub fn resume(&self) -> Option<Resume> {
        self.resume
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
        let mut changed = false;
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
                    changed = true;
                    if matches!(self.phase, Phase::Joined | Phase::Lost) && self.resume.is_some() {
                        if self.phase == Phase::Joined {
                            self.lost_at = now;
                        }
                        self.phase = Phase::Lost;
                    } else if !matches!(self.phase, Phase::Ended(_)) {
                        self.phase = Phase::Ended(None);
                    }
                    break;
                }
            }
        }
        if self.phase == Phase::Lost && now - self.lost_at > GIVE_UP_SECS {
            self.link = None;
            self.phase = Phase::Ended(None);
            changed = true;
        }
        self.flush_input();
        changed
    }

    fn receive(&mut self, msg: HostMsg) {
        match msg {
            HostMsg::Welcome { id, resume, host } => {
                self.id = Some(id);
                self.resume = Some(resume);
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
                self.phase = Phase::Ended(Some(bye));
            }
            HostMsg::Asking => {
                if self.phase == Phase::Joining {
                    self.phase = Phase::Asking;
                }
            }
            HostMsg::Challenge { nonce, host } => {
                let proof = self.proof_key.map(|key| key.prove(&nonce, host));
                self.send(SurfaceMsg::Proof(proof));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::in_app;

    #[test]
    fn input_waits_for_the_welcome_and_sends_only_changes() {
        let (mut host, b) = in_app();
        let mut remote = Remote::join(Box::new(b), "Phone", SurfaceKind::Device, None, None);
        assert!(matches!(host.poll(), Poll::Message(_)));
        remote.buttons(1);
        assert_eq!(host.poll(), Poll::Idle);
        host.send(
            HostMsg::Welcome {
                id: SurfaceId(1),
                resume: Resume([1; 16]),
                host: "PC".into(),
            }
            .encode(),
        );
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
        let mut remote = Remote::join(Box::new(b), "Phone", SurfaceKind::Device, None, None);
        drop(host);
        remote.step(0.0);
        assert_eq!(remote.phase(), Phase::Ended(None));
    }

    #[test]
    fn a_lost_host_is_given_up_after_a_minute() {
        let (mut host, b) = in_app();
        let mut remote = Remote::join(Box::new(b), "Phone", SurfaceKind::Device, None, None);
        host.send(
            HostMsg::Welcome {
                id: SurfaceId(1),
                resume: Resume([1; 16]),
                host: "PC".into(),
            }
            .encode(),
        );
        remote.step(0.0);
        drop(host);
        remote.step(1.0);
        assert_eq!(remote.phase(), Phase::Lost);
        assert_eq!(remote.resume(), Some(Resume([1; 16])));
        remote.step(30.0);
        assert_eq!(remote.phase(), Phase::Lost);
        assert!((remote.lost_for() - 29.0).abs() < 1e-9);
        remote.step(61.5);
        assert_eq!(remote.phase(), Phase::Ended(None));
    }
}
