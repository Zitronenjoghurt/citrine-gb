#[cfg(not(target_arch = "wasm32"))]
mod doorway;
mod relay;
mod suggest;

use citrine_gb::console::{Touch, VideoSpec};
use citrine_gb_net::CheckCode;
use citrine_gb_surface::host::{Host, ScreenFrame};
#[cfg(not(target_arch = "wasm32"))]
use citrine_gb_surface::link::Link;
use citrine_gb_surface::mode::{Mode, View};
use citrine_gb_surface::pairing::Code;
use citrine_gb_surface::proto::{Bye, GameCard, SurfaceKind};
use citrine_gb_surface::remote::{Phase, Remote};
use citrine_gb_surface::touch::ScreenTouch;
use relay::{Door, DoorState, Refusal, Relayed};
use std::cell::Cell;
use std::rc::Rc;

pub use citrine_gb_surface::host::Asker;
pub use citrine_gb_surface::host::{Presence, SurfaceRow};
pub use citrine_gb_surface::proto::{Action, SurfaceId};
#[cfg(not(target_arch = "wasm32"))]
pub use doorway::PeerArrival;
#[cfg(not(target_arch = "wasm32"))]
pub use suggest::Reach;
pub use suggest::{Account, DeviceKeys, OwnDevice, Suggestion};

#[cfg(not(target_arch = "wasm32"))]
const LOOK_SECS: f64 = 3.0;
#[cfg(not(target_arch = "wasm32"))]
const PROBE_EVERY_SECS: f64 = 0.25;
#[cfg(target_os = "macos")]
const LAN_CHECK_SECS: f64 = 2.0;
const RETRY_SECS: f64 = 2.0;
const ACCOUNT_RETRY_SECS: f64 = 30.0;
const CARD_EVERY_SECS: f64 = 0.5;
#[cfg(not(target_arch = "wasm32"))]
const MAX_PEERS_WAITING: usize = 8;
#[cfg(not(target_arch = "wasm32"))]
const MAX_PEERS_PER_ADDRESS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relay {
    pub url: String,
    pub web: String,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    #[cfg(not(target_arch = "wasm32"))]
    Lan(std::net::SocketAddr),
    Relay,
}

#[cfg(not(target_arch = "wasm32"))]
struct Looking {
    discovery: citrine_gb_surface::pairing::Discovery,
    sweep: Option<citrine_gb_surface::pairing::Sweep>,
    since: f64,
    probed_at: f64,
    answered: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Knock {
    Code(Code),
    Own(DeviceKeys),
    Asked,
}

impl Knock {
    fn surface(self) -> citrine_gb_surface::remote::Knock {
        use citrine_gb_surface::remote::Knock as Surface;
        match self {
            Self::Code(code) => Surface::Code(code),
            Self::Own(keys) => Surface::Own(keys.proof),
            Self::Asked => Surface::Asked,
        }
    }
}

pub struct Joining {
    knock: Knock,
    host_name: Option<String>,
    remote: Option<Remote>,
    #[cfg(not(target_arch = "wasm32"))]
    looking: Option<Looking>,
    way: Option<Way>,
    refusal: Option<Rc<Cell<Option<Refusal>>>>,
    retry_at: f64,
    custom: Vec<View>,
    unreachable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinState {
    Looking,
    Asking,
    Joined,
    Reconnecting,
    Failed(&'static str),
}

impl Joining {
    fn new(knock: Knock, host_name: Option<String>, now: f64) -> Self {
        Self {
            knock,
            host_name,
            remote: None,
            #[cfg(not(target_arch = "wasm32"))]
            looking: None,
            way: None,
            refusal: None,
            retry_at: now,
            custom: Vec::new(),
            unreachable: false,
        }
    }

    pub fn host_name(&self) -> Option<&str> {
        self.host_name.as_deref()
    }

    #[cfg(all(test, feature = "nds", not(target_arch = "wasm32")))]
    pub fn remote(&self) -> Option<&Remote> {
        self.remote.as_ref()
    }

    pub fn custom(&self) -> &[View] {
        &self.custom
    }

    pub fn check_code(&self) -> Option<CheckCode> {
        self.remote
            .as_ref()
            .filter(|remote| remote.phase() == Phase::Asking)
            .and_then(Remote::check_code)
    }

    pub fn state(&self) -> JoinState {
        let Some(remote) = &self.remote else {
            return if self.unreachable {
                JoinState::Failed("Couldn't reach the other device")
            } else {
                JoinState::Looking
            };
        };
        match remote.phase() {
            Phase::Joining => JoinState::Looking,
            Phase::Asking => JoinState::Asking,
            Phase::Joined => JoinState::Joined,
            Phase::Lost => JoinState::Reconnecting,
            Phase::Ended(Some(bye)) => JoinState::Failed(bye.label()),
            Phase::Ended(None) => {
                JoinState::Failed(match self.refusal.as_ref().and_then(|r| r.get()) {
                    Some(Refusal::NoCode) => match self.knock {
                        Knock::Own(_) => "The other device isn't reachable now",
                        Knock::Code(_) | Knock::Asked => Bye::WrongCode.label(),
                    },
                    Some(Refusal::Busy) => "Another device uses this code",
                    Some(Refusal::Limit) => "Too many tries. Wait a minute",
                    None => "Couldn't reach the other device",
                })
            }
        }
    }

    pub fn remember(&mut self, mode: &Mode) {
        if let Mode::Custom(views) = mode {
            self.custom = views.clone();
        }
    }

    #[cfg(all(test, feature = "nds", not(target_arch = "wasm32")))]
    pub fn remote_mut(&mut self) -> Option<&mut Remote> {
        self.remote.as_mut()
    }

    pub fn take_remote(&mut self) -> Option<Remote> {
        self.remote.take()
    }

    pub fn put_remote(&mut self, remote: Remote) {
        self.remote = Some(remote);
    }
}

pub struct SecondScreens {
    host: Host,
    clock: web_time::Instant,
    #[cfg(not(target_arch = "wasm32"))]
    lan: Option<citrine_gb_surface::pairing::Listener>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) discovery_port: u16,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) discovery_targets: Option<Vec<std::net::SocketAddr>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) sweep_targets: Option<Vec<std::net::SocketAddr>>,
    #[cfg(not(target_arch = "wasm32"))]
    spawner: Option<citrine_gb_threads::Spawner>,
    #[cfg(target_os = "macos")]
    lan_checked_at: f64,
    lan_blocked: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) advertise: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) nearby_targets: Option<Vec<std::net::SocketAddr>>,
    #[cfg(not(target_arch = "wasm32"))]
    doorway: doorway::Doorway,
    #[cfg(not(target_arch = "wasm32"))]
    peers: Vec<PeerArrival>,
    #[cfg(not(target_arch = "wasm32"))]
    services: Vec<String>,
    account: Option<Account>,
    account_door: Option<Door>,
    account_door_at: f64,
    suggest: suggest::Suggest,
    doors: Vec<Door>,
    relay: Option<Relay>,
    joining: Option<Joining>,
    applied_buttons: u16,
    applied_touch: Option<Touch>,
    published: Option<u128>,
    card_at: f64,
    name: String,
}

impl Default for SecondScreens {
    fn default() -> Self {
        let name = crate::online::cloud::device_name();
        Self {
            host: Host::new(name.clone()),
            clock: web_time::Instant::now(),
            #[cfg(not(target_arch = "wasm32"))]
            lan: None,
            #[cfg(not(target_arch = "wasm32"))]
            discovery_port: citrine_gb_surface::pairing::DISCOVERY_PORT,
            #[cfg(not(target_arch = "wasm32"))]
            discovery_targets: None,
            #[cfg(not(target_arch = "wasm32"))]
            sweep_targets: None,
            #[cfg(not(target_arch = "wasm32"))]
            spawner: None,
            #[cfg(target_os = "macos")]
            lan_checked_at: f64::NEG_INFINITY,
            lan_blocked: false,
            #[cfg(not(target_arch = "wasm32"))]
            advertise: !cfg!(test),
            #[cfg(not(target_arch = "wasm32"))]
            nearby_targets: None,
            #[cfg(not(target_arch = "wasm32"))]
            doorway: doorway::Doorway::default(),
            #[cfg(not(target_arch = "wasm32"))]
            peers: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            services: Vec::new(),
            account: None,
            account_door: None,
            account_door_at: 0.0,
            suggest: suggest::Suggest::default(),
            doors: Vec::new(),
            relay: None,
            joining: None,
            applied_buttons: 0,
            applied_touch: None,
            published: None,
            card_at: f64::NEG_INFINITY,
            name,
        }
    }
}

impl SecondScreens {
    pub fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64()
    }

    pub fn host(&self) -> &Host {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut Host {
        &mut self.host
    }

    pub fn joining(&self) -> Option<&Joining> {
        self.joining.as_ref()
    }

    pub fn joining_mut(&mut self) -> Option<&mut Joining> {
        self.joining.as_mut()
    }

    pub fn set_relay(&mut self, relay: Option<Relay>) {
        if self.relay != relay {
            self.account_door = None;
            self.account_door_at = 0.0;
            self.suggest.forget_own();
        }
        self.relay = relay;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_spawner(&mut self, spawner: citrine_gb_threads::Spawner) {
        self.spawner = Some(spawner);
    }

    pub fn set_account(&mut self, account: Option<Account>) {
        if self.account.as_ref().map(|a| a.keys) != account.as_ref().map(|a| a.keys) {
            self.account_door = None;
            self.account_door_at = 0.0;
            self.suggest.forget_own();
        }
        self.host
            .set_proof_key(account.as_ref().map(|a| a.keys.proof));
        self.account = account;
    }

    pub fn want_suggestions(&mut self) {
        let now = self.now();
        self.suggest.want(now);
    }

    pub fn suggestions(&self) -> Vec<Suggestion> {
        self.suggest.list(self.account.as_ref())
    }

    pub fn askers(&self) -> Vec<Asker> {
        self.host.askers()
    }

    pub fn allow(&mut self, id: u32) {
        self.host.allow(id);
    }

    pub fn decline(&mut self, id: u32) {
        self.host.decline(id);
    }

    pub fn take_outdated(&mut self) -> Vec<String> {
        self.host.take_outdated()
    }

    pub fn code(&self) -> Option<(Code, f64)> {
        self.host.code()
    }

    pub fn code_spent(&self) -> bool {
        self.host.code_spent()
    }

    pub fn lan_blocked(&self) -> bool {
        self.lan_blocked
    }

    pub fn code_link(&self, code: Code) -> Option<String> {
        let relay = self.relay.as_ref()?;
        Some(format!("{}/#screen={}", relay.web, code.plain()))
    }

    pub fn add_device(&mut self) -> Code {
        let now = self.now();
        let code = self.host.open_code(now);
        self.open_lan();
        self.open_door(code, now);
        code
    }

    pub fn stop_adding(&mut self) {
        if let Some((code, _)) = self.host.code() {
            self.doors.retain(|d| d.code() != Some(code) || d.used());
        }
        self.host.close_code();
    }

    fn open_door(&mut self, code: Code, now: f64) {
        if let Some(relay) = &self.relay {
            self.doors
                .push(Door::open(&relay.url, code, relay.password.clone(), now));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_lan(&mut self) {
        if self.lan.is_none() {
            self.lan = citrine_gb_surface::pairing::Listener::on(self.discovery_port).ok();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn open_lan(&mut self) {}

    #[cfg(not(target_arch = "wasm32"))]
    pub fn offer(&mut self, service: &str, on: bool) {
        let offered = self.services.iter().any(|s| s == service);
        if on && !offered {
            self.services.push(service.to_owned());
        } else if !on && offered {
            self.services.retain(|s| s != service);
            self.peers.retain(|p| p.hello.service != service);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn take_peers(&mut self, service: &str) -> Vec<PeerArrival> {
        let (taken, kept) = std::mem::take(&mut self.peers)
            .into_iter()
            .partition(|p| p.hello.service == service);
        self.peers = kept;
        taken
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn instance(&self) -> Option<u64> {
        self.lan.as_ref().map(|lan| lan.instance())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn sort_arrivals(&mut self, now: f64) {
        for sorted in self.doorway.sort(now) {
            match sorted {
                doorway::Sorted::Screen(link) => self.host.accept(link),
                doorway::Sorted::Peer(peer) => {
                    let address = peer.link.address();
                    let same = self
                        .peers
                        .iter()
                        .filter(|p| address.is_some() && p.link.address() == address)
                        .count();
                    if self.services.contains(&peer.hello.service) && same < MAX_PEERS_PER_ADDRESS {
                        self.peers.push(peer);
                    }
                }
            }
        }
        let over = self.peers.len().saturating_sub(MAX_PEERS_WAITING);
        self.peers.drain(..over);
    }

    #[cfg(all(test, feature = "nds", not(target_arch = "wasm32")))]
    pub fn lan_discovery_port(&self) -> Option<u16> {
        self.lan.as_ref().and_then(|lan| lan.discovery_port())
    }

    #[cfg(all(test, feature = "nds", not(target_arch = "wasm32")))]
    pub fn lan_port(&self) -> Option<u16> {
        self.lan.as_ref().map(|lan| lan.port())
    }

    pub fn open_window(&mut self, name: &str, kind: SurfaceKind) -> Remote {
        let (near, far) = citrine_gb_surface::link::in_app();
        self.host.accept_trusted(Box::new(near));
        let knock = citrine_gb_surface::remote::Knock::Asked;
        let mut remote = Remote::join(Box::new(far), name, kind, knock);
        let now = self.now();
        self.host.step(now);
        remote.step(now);
        remote
    }

    pub fn join_suggestion(&mut self, suggestion: &Suggestion) {
        let now = self.now();
        let knock = suggestion.keys.map_or(Knock::Asked, Knock::Own);
        let mut joining = Joining::new(knock, Some(suggestion.name.clone()), now);
        #[cfg(not(target_arch = "wasm32"))]
        if let Reach::Lan(addr) = suggestion.reach {
            Self::dial_lan(&mut joining, addr, &self.name);
        }
        if joining.remote.is_none() {
            self.dial_relay(&mut joining);
        }
        joining.unreachable = joining.remote.is_none();
        self.joining = Some(joining);
    }

    pub fn join(&mut self, code: Code) {
        let now = self.now();
        let mut joining = Joining::new(Knock::Code(code), None, now);
        #[cfg(not(target_arch = "wasm32"))]
        {
            use citrine_gb_surface::pairing::{DISCOVERY_PORT, Discovery, Sweep};
            let spawner = self.spawner.as_ref();
            let (discovery, sweep) = match &self.discovery_targets {
                Some(targets) => (
                    Discovery::toward(targets),
                    spawner
                        .zip(self.sweep_targets.clone())
                        .map(|(spawner, targets)| Sweep::toward(targets, spawner)),
                ),
                None => (
                    Discovery::start(),
                    spawner.and_then(|spawner| Sweep::start(DISCOVERY_PORT, spawner)),
                ),
            };
            joining.looking = Some(Looking {
                discovery,
                sweep,
                since: now,
                probed_at: now,
                answered: false,
            });
        }
        #[cfg(target_arch = "wasm32")]
        self.dial_relay(&mut joining);
        self.joining = Some(joining);
    }

    pub fn leave(&mut self) {
        if let Some(mut joining) = self.joining.take()
            && let Some(remote) = &mut joining.remote
        {
            remote.leave();
        }
    }

    fn dial_relay(&self, joining: &mut Joining) {
        let Some(relay) = &self.relay else {
            return;
        };
        let (link, refusal) = match joining.knock {
            Knock::Code(code) => Relayed::open(&relay.url, code),
            Knock::Own(keys) => Relayed::dial(&relay.url, keys.door),
            Knock::Asked => return,
        };
        joining.refusal = Some(refusal);
        joining.way = Some(Way::Relay);
        Self::link(joining, Box::new(link), &self.name);
    }

    fn link(joining: &mut Joining, link: Box<dyn citrine_gb_surface::link::Link>, name: &str) {
        match &mut joining.remote {
            Some(remote) => remote.resume_over(link),
            None => {
                let knock = joining.knock.surface();
                joining.remote = Some(Remote::join(link, name, SurfaceKind::Device, knock));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn dial_lan(joining: &mut Joining, addr: std::net::SocketAddr, name: &str) -> bool {
        let wait = std::time::Duration::from_millis(400);
        match citrine_gb_surface::link::Tcp::dial(addr, wait) {
            Ok(tcp) => {
                joining.way = Some(Way::Lan(addr));
                Self::link(joining, Box::new(tcp), name);
                true
            }
            Err(_) => false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn look(&self, joining: &mut Joining, now: f64) {
        let Some(looking) = &mut joining.looking else {
            return;
        };
        if now - looking.probed_at >= PROBE_EVERY_SECS {
            looking.discovery.probe();
            looking.probed_at = now;
        }
        let trying = joining
            .remote
            .as_ref()
            .is_some_and(|r| matches!(r.phase(), Phase::Joining | Phase::Asking | Phase::Joined));
        let joined = joining
            .remote
            .as_ref()
            .is_some_and(|r| r.phase() == Phase::Joined);
        if joined {
            joining.looking = None;
            return;
        }
        if trying {
            return;
        }
        if let Some(addr) = looking.discovery.poll() {
            looking.answered = true;
            joining.remote = None;
            if !Self::dial_lan(joining, addr, &self.name) {
                joining.looking = None;
                self.dial_relay(joining);
            }
        } else if looking.answered {
            joining.looking = None;
        } else if let Some(tcp) = looking.sweep.as_mut().and_then(|sweep| sweep.poll()) {
            let Some(addr) = tcp.peer() else {
                return;
            };
            joining.remote = None;
            joining.way = Some(Way::Lan(addr));
            Self::link(joining, Box::new(tcp), &self.name);
        } else if now - looking.since > LOOK_SECS {
            joining.looking = None;
            joining.remote = None;
            self.dial_relay(joining);
        }
    }

    #[cfg(target_os = "macos")]
    fn check_lan(&mut self, now: f64) {
        let Some(lan) = &self.lan else {
            return;
        };
        if self.host.code().is_none() {
            return;
        }
        if now - self.lan_checked_at >= LAN_CHECK_SECS {
            self.lan_checked_at = now;
            self.lan_blocked = !lan.may_send();
        }
    }

    pub fn step(&mut self) {
        let now = self.now();
        #[cfg(not(target_arch = "wasm32"))]
        if self.advertise {
            self.open_lan();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(lan) = &mut self.lan {
            let mut services = Vec::new();
            if self.host.game().is_some() {
                services.push(citrine_gb_surface::pairing::SCREEN_SERVICE.to_owned());
            }
            services.extend(self.services.iter().cloned());
            lan.set_presence(Some(citrine_gb_surface::pairing::Presence {
                name: self.name.clone(),
                tag_key: self.account.as_ref().map(|a| a.keys.tag),
                services,
            }));
            while let Some(tcp) = lan.accept() {
                self.doorway.arrive(tcp, now);
            }
            lan.answer(self.host.code().is_some());
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.sort_arrivals(now);
        #[cfg(target_os = "macos")]
        self.check_lan(now);
        let mut reopen = None;
        for door in &mut self.doors {
            if let Some(link) = door.step(now) {
                self.host.accept(Box::new(link));
            }
            if door.state() == DoorState::Taken
                && self
                    .host
                    .code()
                    .is_some_and(|(c, _)| Some(c) == door.code())
            {
                reopen = door.code();
            }
        }
        if let Some(taken) = reopen {
            self.doors.retain(|d| d.code() != Some(taken));
            let code = self.host.open_code(now);
            self.open_door(code, now);
        }
        let open = self.host.code().map(|(code, _)| code);
        let keep = citrine_gb_surface::host::KEEP_LOST_SECS;
        self.doors.retain(|d| {
            matches!(d.state(), DoorState::Opening | DoorState::Open)
                && (d.code() == open || (d.used() && d.idle_for(now) < keep))
        });
        self.step_account_door(now);
        self.suggest.step(suggest::Around {
            now,
            relay: self.relay.as_ref(),
            account: self.account.as_ref(),
            #[cfg(not(target_arch = "wasm32"))]
            skip: self.lan.as_ref().map(|lan| lan.instance()),
            #[cfg(not(target_arch = "wasm32"))]
            targets: self.nearby_targets.as_deref(),
        });
        self.host.step(now);
        self.step_joining(now);
    }

    fn step_account_door(&mut self, now: f64) {
        let (Some(relay), Some(account), Some(_)) = (&self.relay, &self.account, self.host.game())
        else {
            self.account_door = None;
            return;
        };
        if let Some(door) = &mut self.account_door {
            if let Some(link) = door.step(now) {
                self.host.accept(Box::new(link));
            }
            if matches!(
                door.state(),
                DoorState::Taken | DoorState::Refused | DoorState::Closed
            ) {
                self.account_door = None;
                self.account_door_at = now + ACCOUNT_RETRY_SECS;
            }
        } else if now >= self.account_door_at {
            self.account_door = Some(Door::account(
                &relay.url,
                account.keys.door,
                relay.password.clone(),
                now,
            ));
        }
    }

    fn step_joining(&mut self, now: f64) {
        let Some(mut joining) = self.joining.take() else {
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        if joining.looking.is_some() {
            if let Some(remote) = &mut joining.remote {
                remote.step(now);
            }
            self.look(&mut joining, now);
            joining.unreachable = joining.looking.is_none() && joining.remote.is_none();
        }
        if let Some(remote) = &mut joining.remote {
            remote.step(now);
        }
        let lost = joining
            .remote
            .as_ref()
            .is_some_and(|r| r.phase() == Phase::Lost);
        if lost && now >= joining.retry_at {
            joining.retry_at = now + RETRY_SECS;
            match joining.way {
                #[cfg(not(target_arch = "wasm32"))]
                Some(Way::Lan(addr)) => {
                    Self::dial_lan(&mut joining, addr, &self.name);
                }
                Some(Way::Relay) | None => self.dial_relay(&mut joining),
            }
        }
        self.joining = Some(joining);
    }

    pub fn input(
        &mut self,
        local: Option<Option<Touch>>,
        touch_screen: Option<u8>,
    ) -> (Option<Option<Touch>>, Option<u16>) {
        let local_touch = local
            .flatten()
            .zip(touch_screen)
            .map(|(t, screen)| ScreenTouch {
                screen,
                x: t.x,
                y: t.y,
            });
        let merged = self
            .host
            .touch(local_touch)
            .map(|t| Touch { x: t.x, y: t.y });
        let touch = (local.is_some() || merged != self.applied_touch).then_some(merged);
        self.applied_touch = merged;
        let buttons = self.host.buttons();
        let pressed = (buttons != self.applied_buttons).then_some(buttons);
        self.applied_buttons = buttons;
        (touch, pressed)
    }

    pub fn take_actions(&mut self) -> Vec<Action> {
        self.host.take_actions()
    }

    pub fn wants_frames(&self, frame: u128) -> bool {
        self.published != Some(frame) && self.host.wants_frames()
    }

    pub fn publish(&mut self, frame: u128, video: VideoSpec, rgba: &[u8]) {
        self.published = Some(frame);
        let crops: Vec<Vec<u8>> = video
            .screens
            .iter()
            .map(|s| crop(rgba, video.width, s.x, s.y, s.width, s.height))
            .collect();
        let frames: Vec<ScreenFrame<'_>> = video
            .screens
            .iter()
            .zip(&crops)
            .map(|(s, rgba)| ScreenFrame {
                rgba,
                width: u16::try_from(s.width).unwrap_or(u16::MAX),
                height: u16::try_from(s.height).unwrap_or(u16::MAX),
            })
            .collect();
        self.host.publish(&frames);
    }

    pub fn card_due(&mut self) -> bool {
        let now = self.now();
        if now - self.card_at >= CARD_EVERY_SECS {
            self.card_at = now;
            true
        } else {
            false
        }
    }

    pub fn set_game(&mut self, card: Option<GameCard>) {
        self.host.set_game(card);
    }

    pub fn screen_away(&self, screens: usize) -> Option<usize> {
        if screens < 2 {
            return None;
        }
        let away: Vec<usize> = (0..screens)
            .filter(|&i| u8::try_from(i).is_ok_and(|i| self.host.shows_screen(i)))
            .collect();
        (away.len() == 1).then(|| away[0])
    }

    pub fn surfaces(&self) -> Vec<SurfaceRow> {
        self.host.surfaces()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn close(&mut self) {
        self.leave();
        self.host.close();
    }
}

fn crop(rgba: &[u8], width: usize, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * 4);
    for row in y..y + h {
        let start = (row * width + x) * 4;
        match rgba.get(start..start + w * 4) {
            Some(line) => out.extend_from_slice(line),
            None => out.resize(out.len() + w * 4, 0),
        }
    }
    out
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod doorway_tests {
    use super::*;
    use citrine_gb_surface::link::{Link, Poll, Tcp};
    use citrine_gb_surface::proto::{HostMsg, Wire};
    use std::net::SocketAddr;
    use std::time::Duration;

    const HELLO_0_12: [u8; 13] = [0, 2, 7, b'P', b'i', b'x', b'e', b'l', b' ', b'8', 2, 0, 0];

    fn listening() -> SecondScreens {
        let mut screens = SecondScreens {
            discovery_port: 0,
            advertise: true,
            ..SecondScreens::default()
        };
        screens.step();
        screens
    }

    fn dial(screens: &SecondScreens) -> Tcp {
        let port = screens.lan.as_ref().expect("the lan is open").port();
        Tcp::dial(
            SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_secs(2),
        )
        .unwrap()
    }

    fn step_until(
        screens: &mut SecondScreens,
        mut done: impl FnMut(&mut SecondScreens) -> bool,
    ) -> bool {
        for _ in 0..400 {
            screens.step();
            if done(screens) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn a_screen_hello_from_0_12_reaches_the_host_and_hears_needs_update() {
        let mut screens = listening();
        screens.offer("walker", true);
        let mut phone = dial(&screens);
        phone.send(HELLO_0_12.to_vec());
        let mut reply = None;
        assert!(step_until(&mut screens, |_| {
            if let Poll::Message(bytes) = phone.poll() {
                reply = HostMsg::decode(&bytes);
            }
            reply.is_some()
        }));
        assert_eq!(reply, Some(HostMsg::Bye(Bye::NeedsUpdate)));
        assert_eq!(screens.take_outdated(), vec!["Pixel 8".to_owned()]);
        assert!(screens.take_peers("walker").is_empty());
    }

    #[test]
    fn a_0_12_host_tells_this_phone_to_update() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut phone = SecondScreens::default();
        phone.join_suggestion(&Suggestion {
            name: "Old Mac".into(),
            own: false,
            reach: Reach::Lan(listener.local_addr().unwrap()),
            keys: None,
        });
        let mut old_host = Tcp::new(listener.accept().unwrap().0).unwrap();
        let mut hello = None;
        assert!(step_until(&mut phone, |_| {
            if let Poll::Message(bytes) = old_host.poll() {
                hello = citrine_gb_surface::proto::Hello::front(&bytes);
            }
            hello.is_some()
        }));
        assert_eq!(
            hello.map(|(version, _)| version),
            Some(citrine_gb_surface::proto::SURFACE_VERSION)
        );
        old_host.send(HostMsg::Bye(Bye::NeedsUpdate).encode());
        assert!(step_until(&mut phone, |phone| {
            phone.joining().map(|j| j.state())
                == Some(JoinState::Failed("Update Citrine on both devices"))
        }));
    }

    #[test]
    fn a_walker_hello_never_reaches_the_host() {
        let mut screens = listening();
        screens.offer("walker", true);
        let mut phone = dial(&screens);
        let (_, first) =
            citrine_gb_net::PeerDial::start("walker", &citrine_gb_net::PeerSecret::random(), None)
                .unwrap();
        phone.send(first.clone());
        let mut peers = Vec::new();
        assert!(step_until(&mut screens, |screens| {
            peers.extend(screens.take_peers("walker"));
            !peers.is_empty()
        }));
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].first, first);
        assert_eq!(peers[0].hello.service, "walker");
        for _ in 0..20 {
            screens.step();
        }
        assert_eq!(phone.poll(), Poll::Idle);
    }

    #[test]
    fn a_service_this_device_does_not_offer_is_dropped() {
        let mut screens = listening();
        let mut phone = dial(&screens);
        let (_, first) =
            citrine_gb_net::PeerDial::start("walker", &citrine_gb_net::PeerSecret::random(), None)
                .unwrap();
        phone.send(first);
        assert!(step_until(&mut screens, |_| phone.poll() == Poll::Closed));
        assert!(screens.take_peers("walker").is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_take_one_screen_of_a_stacked_frame() {
        let width = 2;
        let rgba: Vec<u8> = (0..2 * 4 * 4).map(|v| v as u8).collect();
        assert_eq!(crop(&rgba, width, 0, 2, 2, 2), rgba[16..32].to_vec());
        assert_eq!(crop(&rgba, width, 1, 0, 1, 1), rgba[4..8].to_vec());
        assert_eq!(crop(&rgba, width, 0, 3, 2, 2).len(), 16);
    }
}
