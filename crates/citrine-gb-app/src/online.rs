use citrine_gb_net::{
    Account, ClientMsg, DeviceId, DeviceInfo, ErrorCode, Friend, FriendState, GameHash, InviteCode,
    Keys, Notice, OneTimeCode, OnlineId, PROTOCOL_VERSION, Presence, RelayId, RelayKey,
    RelayPassword, ServerInfo, ServerMsg,
};
use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;
use web_time::Instant;

pub mod address;
pub mod chat;
pub mod chat_media;
pub mod cloud;
pub mod settings;
pub mod synced;
pub mod transport;
pub mod two_factor;
pub mod typing;

pub use address::{AddressError, ServerAddress};
pub use settings::{JoinedRelay, OnlineSettings, RelayEntry, Trust};
use transport::{Connector, Transport, TransportEvent};

pub const AWAY_AFTER: Duration = Duration::from_secs(5 * 60);
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);
const PING_EVERY: Duration = Duration::from_secs(25);
const SILENCE_LIMIT: Duration = Duration::from_secs(70);
const FIRST_RETRY: Duration = Duration::from_secs(1);
const LONGEST_RETRY: Duration = Duration::from_secs(60);
const PAUSED_RETRY: Duration = Duration::from_secs(30);
const WATCHED_PING_EVERY: Duration = Duration::from_secs(3);
const WATCH_FOR: Duration = Duration::from_secs(5);
const ROUND_TRIPS_KEPT: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    Unreachable,
    Lost,
    RelayOff,
    KillSwitch,
    ShuttingDown,
    Disconnected,
    WrongPassword,
    PasswordChanged,
    SignUpsClosed,
    InviteNeeded,
    InviteWrong,
    InviteUsed,
    UnknownAccount,
    AccountExists,
    Banned,
    SignedInElsewhere,
    Protocol { server: u16 },
    BadName,
    TooManyTries,
    Server,
    KeyChanged,
    KeyMismatch,
    CodeNeeded,
    CodeWrong,
    SignedOut,
}

impl Problem {
    fn from_error(code: ErrorCode) -> Self {
        match code {
            ErrorCode::Protocol { server } => Self::Protocol { server },
            ErrorCode::WrongPassword => Self::WrongPassword,
            ErrorCode::SignUpsClosed => Self::SignUpsClosed,
            ErrorCode::InviteNeeded => Self::InviteNeeded,
            ErrorCode::InviteWrong => Self::InviteWrong,
            ErrorCode::InviteUsed => Self::InviteUsed,
            ErrorCode::UnknownAccount => Self::UnknownAccount,
            ErrorCode::AccountExists => Self::AccountExists,
            ErrorCode::Banned => Self::Banned,
            ErrorCode::BadName => Self::BadName,
            ErrorCode::TooManyTries | ErrorCode::RateLimited => Self::TooManyTries,
            ErrorCode::CodeNeeded => Self::CodeNeeded,
            ErrorCode::CodeWrong => Self::CodeWrong,
            ErrorCode::SignedOut => Self::SignedOut,
            _ => Self::Server,
        }
    }

    fn from_notice(notice: Notice) -> Self {
        match notice {
            Notice::RelayOff => Self::RelayOff,
            Notice::KillSwitch => Self::KillSwitch,
            Notice::Disconnected => Self::Disconnected,
            Notice::Banned => Self::Banned,
            Notice::PasswordChanged => Self::PasswordChanged,
            Notice::SignedInElsewhere => Self::SignedInElsewhere,
            Notice::ShuttingDown => Self::ShuttingDown,
            Notice::SignedOut => Self::SignedOut,
        }
    }

    pub fn retries(self) -> bool {
        matches!(
            self,
            Self::Unreachable
                | Self::Lost
                | Self::RelayOff
                | Self::KillSwitch
                | Self::ShuttingDown
                | Self::Disconnected
                | Self::TooManyTries
                | Self::Server
        )
    }

    pub fn needs_password(self) -> bool {
        matches!(self, Self::WrongPassword | Self::PasswordChanged)
    }

    pub fn needs_code(self) -> bool {
        matches!(self, Self::CodeNeeded | Self::CodeWrong)
    }

    pub fn text(self) -> String {
        match self {
            Self::Unreachable => "Can't reach the server".into(),
            Self::Lost => "The connection dropped".into(),
            Self::RelayOff => "The relay is off on this server".into(),
            Self::KillSwitch => "The server's owner paused the relay".into(),
            Self::ShuttingDown => "The server is restarting".into(),
            Self::Disconnected => "The server's owner disconnected you".into(),
            Self::WrongPassword => "Wrong relay password".into(),
            Self::PasswordChanged => "The relay password changed".into(),
            Self::SignUpsClosed => "This server takes no new accounts".into(),
            Self::InviteNeeded => "This server needs an invite code for new accounts".into(),
            Self::InviteWrong => "That invite code doesn't exist".into(),
            Self::InviteUsed => "That invite code was already used".into(),
            Self::UnknownAccount => "This server has no account with that relay ID".into(),
            Self::AccountExists => "An account with this relay ID already exists".into(),
            Self::Banned => "This account is banned on this server".into(),
            Self::SignedInElsewhere => "Citrine is open in another tab or window.".into(),
            Self::Protocol { server } if server > PROTOCOL_VERSION => {
                "The server runs a newer Citrine. Update Citrine to connect.".into()
            }
            Self::Protocol { .. } => "The server runs an older Citrine".into(),
            Self::BadName => "That name isn't allowed".into(),
            Self::TooManyTries => "Too many tries. Wait a minute.".into(),
            Self::Server => "The server had a problem".into(),
            Self::KeyChanged => "This relay's key changed".into(),
            Self::KeyMismatch => "This relay's key doesn't match the one in the address.".into(),
            Self::CodeNeeded => "This relay asks for your two-factor code".into(),
            Self::CodeWrong => "That code is wrong. Try the newest one.".into(),
            Self::SignedOut => "Another device signed this one out".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Connecting,
    Probed,
    SigningIn,
    Connected,
    Retrying { at: Instant, problem: Problem },
    Stopped(Problem),
}

#[derive(Clone)]
enum Plan {
    Probe,
    SignUp {
        relay_id: RelayId,
        name: String,
        invite: Option<InviteCode>,
    },
    SignIn {
        relay_id: RelayId,
    },
}

#[derive(Clone)]
struct Target {
    address: ServerAddress,
    password: String,
    plan: Plan,
    joined: bool,
    trust: Trust,
    code: Option<OneTimeCode>,
}

impl Target {
    fn sign_in_from_now_on(&mut self) {
        if let Plan::SignUp { relay_id, .. } = &self.plan {
            self.plan = Plan::SignIn {
                relay_id: relay_id.clone(),
            };
        }
    }

    fn keys(&self) -> Option<Keys> {
        match &self.plan {
            Plan::Probe => None,
            Plan::SignUp { relay_id, .. } | Plan::SignIn { relay_id } => Some(relay_id.keys()),
        }
    }
}

pub struct Joined {
    pub address: ServerAddress,
    pub relay_id: RelayId,
    pub password: String,
    pub account: Account,
    pub trust: Trust,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Seen {
    Online,
    Away,
    Offline { last_seen: u64 },
}

pub struct Online {
    connector: Connector,
    protocol: u16,
    conn: Option<Box<dyn Transport>>,
    target: Option<Target>,
    keys: Option<Keys>,
    phase: Phase,
    opened_at: Option<Instant>,
    heard_at: Option<Instant>,
    pinged_at: Option<Instant>,
    ping_seq: u64,
    ping_out: Option<(u64, Instant)>,
    round_trips: VecDeque<Duration>,
    watched_until: Option<Instant>,
    failures: u32,
    info: Option<ServerInfo>,
    account: Option<Account>,
    friends: BTreeMap<OnlineId, Friend>,
    playing: BTreeMap<OnlineId, GameHash>,
    inbox: Vec<ServerMsg>,
    joined: Option<Joined>,
    account_changed: bool,
    request_error: Option<ErrorCode>,
    away: bool,
    my_game: Option<GameHash>,
    sent_away: Option<bool>,
    told: BTreeMap<OnlineId, Option<GameHash>>,
    screen: Option<String>,
    screens: u64,
    pictures_refused: bool,
    chat: chat::ChatState,
    cloud: cloud::CloudState,
    relay_key: Option<RelayKey>,
    offered_key: Option<RelayKey>,
    trust_changed: bool,
    setup: Option<Vec<u8>>,
    recovery_codes: Option<Vec<String>>,
    deleted: bool,
    devices: Vec<DeviceInfo>,
    this_device: Option<DeviceId>,
    signed_out: Option<ServerAddress>,
    typing: typing::Typing,
}

impl Default for Online {
    fn default() -> Self {
        Self::new(transport::websocket())
    }
}

impl Online {
    pub fn new(connector: Connector) -> Self {
        Self {
            connector,
            protocol: PROTOCOL_VERSION,
            conn: None,
            target: None,
            keys: None,
            phase: Phase::Idle,
            opened_at: None,
            heard_at: None,
            pinged_at: None,
            ping_seq: 0,
            ping_out: None,
            round_trips: VecDeque::new(),
            watched_until: None,
            failures: 0,
            info: None,
            account: None,
            friends: BTreeMap::new(),
            playing: BTreeMap::new(),
            inbox: Vec::new(),
            joined: None,
            account_changed: false,
            request_error: None,
            away: false,
            my_game: None,
            sent_away: None,
            told: BTreeMap::new(),
            screen: None,
            screens: 0,
            pictures_refused: false,
            chat: chat::ChatState::default(),
            cloud: cloud::CloudState::default(),
            relay_key: None,
            offered_key: None,
            trust_changed: false,
            setup: None,
            recovery_codes: None,
            deleted: false,
            devices: Vec::new(),
            this_device: None,
            signed_out: None,
            typing: typing::Typing::default(),
        }
    }

    pub fn offline() -> Self {
        Self::new(Box::new(|_| Box::new(transport::Unplugged)))
    }

    pub fn probe(&mut self, address: ServerAddress, password: String, trust: Trust, now: Instant) {
        self.start(
            Target {
                address,
                password,
                plan: Plan::Probe,
                joined: false,
                trust,
                code: None,
            },
            now,
        );
    }

    pub fn create_account(&mut self, name: String, now: Instant) {
        self.create_invited_account(name, None, now);
    }

    pub fn create_invited_account(
        &mut self,
        name: String,
        invite: Option<InviteCode>,
        now: Instant,
    ) {
        let Some(target) = self.target.clone() else {
            return;
        };
        let plan = Plan::SignUp {
            relay_id: RelayId::random(),
            name,
            invite,
        };
        self.continue_with(Target { plan, ..target }, now);
    }

    pub fn use_relay_id(&mut self, relay_id: RelayId, code: Option<String>, now: Instant) {
        let Some(target) = self.target.clone() else {
            return;
        };
        let plan = Plan::SignIn { relay_id };
        let code = code.map(OneTimeCode);
        self.continue_with(
            Target {
                plan,
                code,
                ..target
            },
            now,
        );
    }

    pub fn sign_in(&mut self, relay: JoinedRelay, now: Instant) {
        self.start(
            Target {
                address: relay.address,
                password: relay.password,
                plan: Plan::SignIn {
                    relay_id: relay.relay_id,
                },
                joined: true,
                trust: relay.trust,
                code: None,
            },
            now,
        );
    }

    pub fn sign_in_with_code(&mut self, code: String, now: Instant) {
        let Some(target) = self.target.clone() else {
            return;
        };
        let code = Some(OneTimeCode(code));
        self.continue_with(Target { code, ..target }, now);
    }

    pub fn accept_new_key(&mut self, now: Instant) {
        let (Some(key), Some(mut target)) = (self.offered_key.take(), self.target.clone()) else {
            return;
        };
        target.trust.relay_key = Some(key);
        target.trust.expected = None;
        self.trust_changed = true;
        self.continue_with(target, now);
    }

    pub fn retry_now(&mut self, now: Instant) {
        if let Some(target) = self.target.clone()
            && !self.is_connected()
        {
            self.start(target, now);
        }
    }

    pub fn change_password(&mut self, password: String, now: Instant) {
        if let Some(target) = self.target.clone() {
            self.start(Target { password, ..target }, now);
        }
    }

    pub fn sign_out_here(&mut self) {
        if self.phase == Phase::Connected
            && let Some(this) = self.this_device
        {
            self.send(ClientMsg::SignOutDevice(this));
        }
        self.leave();
    }

    pub fn leave(&mut self) {
        self.chat_reset();
        self.cloud_reset();
        self.drop_connection();
        self.target = None;
        self.keys = None;
        self.phase = Phase::Idle;
        self.info = None;
        self.account = None;
        self.friends.clear();
        self.playing.clear();
        self.failures = 0;
        self.joined = None;
        self.request_error = None;
        self.relay_key = None;
        self.offered_key = None;
        self.trust_changed = false;
        self.setup = None;
        self.recovery_codes = None;
        self.deleted = false;
        self.devices.clear();
        self.this_device = None;
    }

    fn start(&mut self, target: Target, now: Instant) {
        self.leave();
        self.target = Some(target);
        self.connect(now);
    }

    fn continue_with(&mut self, target: Target, now: Instant) {
        let probed = self.phase == Phase::Probed && self.conn.is_some();
        self.keys = target.keys();
        self.target = Some(target);
        if probed {
            self.send_plan();
        } else {
            self.drop_connection();
            self.connect(now);
        }
    }

    fn connect(&mut self, now: Instant) {
        let Some(target) = &self.target else {
            return;
        };
        self.keys = target.keys();
        self.conn = Some((self.connector)(&target.address.relay_url()));
        self.phase = Phase::Connecting;
        self.opened_at = Some(now);
        self.heard_at = None;
        self.pinged_at = None;
    }

    fn drop_connection(&mut self) {
        self.conn = None;
        self.ping_out = None;
        self.round_trips.clear();
        self.sent_away = None;
        self.told.clear();
        self.screen = None;
        self.screens = 0;
        self.typing.reset();
    }

    pub fn update(&mut self, now: Instant) {
        self.inbox.clear();
        if let Phase::Retrying { at, .. } = self.phase
            && now >= at
        {
            self.connect(now);
        }
        while let Some(event) = self.conn.as_mut().and_then(|conn| conn.poll()) {
            match event {
                TransportEvent::Opened => {
                    self.heard_at = Some(now);
                    self.pinged_at = Some(now);
                    if let Some(problem) = self.check_key() {
                        self.fail(problem, now);
                        continue;
                    }
                    self.send(ClientMsg::Hello {
                        protocol: self.protocol,
                        app: app_name(),
                    });
                }
                TransportEvent::Message(bytes) => {
                    self.heard_at = Some(now);
                    let arrived = self.conn.as_ref().and_then(|c| c.arrived()).unwrap_or(now);
                    match ServerMsg::decode(&bytes) {
                        Ok(ServerMsg::Pong(n)) => self.pong(n, arrived),
                        Ok(msg) => self.receive(msg, now),
                        Err(_) => {}
                    }
                }
                TransportEvent::Closed | TransportEvent::Failed(_)
                    if self.phase == Phase::Probed =>
                {
                    self.drop_connection();
                }
                TransportEvent::Closed | TransportEvent::Failed(_) => {
                    let problem = match self.heard_at {
                        None => Problem::Unreachable,
                        Some(_) => Problem::Lost,
                    };
                    self.fail(problem, now);
                }
            }
        }
        self.keep_alive(now);
        self.share_status();
        self.chat_tick(now);
        self.cloud_tick(now);
    }

    fn check_key(&mut self) -> Option<Problem> {
        let key = self.conn.as_ref()?.relay_key()?;
        self.relay_key = Some(key);
        let target = self.target.as_mut()?;
        if target
            .trust
            .expected
            .is_some_and(|expected| expected != key.fingerprint())
        {
            return Some(Problem::KeyMismatch);
        }
        match target.trust.relay_key {
            Some(pinned) if pinned != key => {
                self.offered_key = Some(key);
                Some(Problem::KeyChanged)
            }
            Some(_) => None,
            None => {
                target.trust.relay_key = Some(key);
                self.trust_changed = true;
                None
            }
        }
    }

    fn keep_alive(&mut self, now: Instant) {
        if self.conn.is_none() {
            return;
        }
        match self.heard_at {
            None => {
                if self
                    .opened_at
                    .is_some_and(|at| now.duration_since(at) > OPEN_TIMEOUT)
                {
                    self.fail(Problem::Unreachable, now);
                }
            }
            Some(heard) => {
                if now.duration_since(heard) > SILENCE_LIMIT {
                    self.fail(Problem::Lost, now);
                } else {
                    let every = if self.watched_until.is_some_and(|until| now < until) {
                        WATCHED_PING_EVERY
                    } else {
                        PING_EVERY
                    };
                    if self
                        .pinged_at
                        .is_none_or(|at| now.duration_since(at) > every)
                    {
                        self.ping(now);
                    }
                }
            }
        }
    }

    fn ping(&mut self, now: Instant) {
        self.pinged_at = Some(now);
        self.ping_seq += 1;
        let sent = self.conn.as_ref().and_then(|c| c.clock()).unwrap_or(now);
        self.ping_out = Some((self.ping_seq, sent));
        self.send(ClientMsg::Ping(self.ping_seq));
    }

    fn pong(&mut self, n: u64, arrived: Instant) {
        if let Some((seq, at)) = self.ping_out
            && seq == n
        {
            self.ping_out = None;
            if self.round_trips.len() == ROUND_TRIPS_KEPT {
                self.round_trips.pop_front();
            }
            self.round_trips
                .push_back(arrived.saturating_duration_since(at));
        }
    }

    pub fn watch_round_trip(&mut self, now: Instant) {
        let fresh = self.watched_until.is_none_or(|until| now >= until);
        self.watched_until = Some(now + WATCH_FOR);
        if (fresh || self.round_trips.is_empty()) && self.is_connected() && self.ping_out.is_none()
        {
            self.ping(now);
        }
    }

    pub fn round_trip_ms(&self) -> Option<u32> {
        let mut sorted: Vec<Duration> = self.round_trips.iter().copied().collect();
        sorted.sort_unstable();
        sorted
            .get(sorted.len() / 2)
            .map(|d| d.as_millis().try_into().unwrap_or(u32::MAX))
    }

    fn fail(&mut self, problem: Problem, now: Instant) {
        if problem == Problem::SignedOut {
            self.signed_out = self.target.as_ref().map(|t| t.address.clone());
            self.chat_wipe();
        }
        self.drop_connection();
        let joined = self.target.as_ref().is_some_and(|t| t.joined);
        if joined && problem.retries() {
            self.failures += 1;
            let at = now + retry_delay(problem, self.failures);
            self.phase = Phase::Retrying { at, problem };
        } else {
            self.phase = Phase::Stopped(problem);
        }
        for friend in self.friends.values_mut() {
            friend.presence = None;
        }
        self.playing.clear();
    }

    fn receive(&mut self, msg: ServerMsg, now: Instant) {
        match msg {
            ServerMsg::Info(info) => {
                self.info = Some(info);
                self.pictures_refused = false;
                if matches!(self.target.as_ref().map(|t| &t.plan), Some(Plan::Probe)) {
                    self.phase = Phase::Probed;
                } else {
                    self.send_plan();
                }
            }
            ServerMsg::Welcome(account) => {
                self.phase = Phase::Connected;
                self.failures = 0;
                self.sent_away = Some(false);
                if let Some(target) = self.target.as_mut() {
                    target.code = None;
                }
                if let Some(target) = self.target.as_mut()
                    && !target.joined
                {
                    target.joined = true;
                    target.sign_in_from_now_on();
                    if let Plan::SignUp { relay_id, .. } | Plan::SignIn { relay_id } = &target.plan
                    {
                        self.joined = Some(Joined {
                            address: target.address.clone(),
                            relay_id: relay_id.clone(),
                            password: target.password.clone(),
                            account: account.clone(),
                            trust: target.trust.clone(),
                        });
                    }
                }
                self.account = Some(account);
                self.account_changed = true;
            }
            ServerMsg::Account(account) => {
                self.account = Some(account);
                self.account_changed = true;
            }
            ServerMsg::Friends(list) => {
                self.friends = list.into_iter().map(|f| (f.online_id, f)).collect();
                self.chat_changes_due();
                self.playing.clear();
                self.told.clear();
            }
            ServerMsg::Friend(friend) => {
                let id = friend.online_id;
                if friend.state != FriendState::Friend || !is_online(friend.presence) {
                    self.playing.remove(&id);
                    self.told.remove(&id);
                    self.typing.gone(id);
                }
                self.friends.insert(id, friend);
            }
            ServerMsg::FriendGone(id) => {
                self.friends.remove(&id);
                self.playing.remove(&id);
                self.told.remove(&id);
                self.typing.gone(id);
            }
            ServerMsg::Presence { who, presence } => {
                if !is_online(Some(presence)) {
                    self.playing.remove(&who);
                    self.typing.gone(who);
                }
                self.told.remove(&who);
                if let Some(friend) = self.friends.get_mut(&who) {
                    friend.presence = Some(presence);
                }
            }
            ServerMsg::Typing { from, typing } => {
                if self.friends.contains_key(&from) {
                    self.typing.heard(from, typing, now);
                }
            }
            ServerMsg::Error(ErrorCode::BadMessage)
                if self.phase == Phase::Connected && self.typing.refused(now) => {}
            ServerMsg::Playing { from, sealed } => {
                if let (Some(keys), Some(friend)) = (&self.keys, self.friends.get(&from))
                    && let Ok(game) = keys.open_playing(&friend.public_key, &sealed)
                {
                    match game {
                        Some(game) => self.playing.insert(from, game),
                        None => self.playing.remove(&from),
                    };
                }
            }
            ServerMsg::Pong(n) => self.pong(n, now),
            ServerMsg::Error(ErrorCode::AccountExists)
                if self.phase == Phase::SigningIn
                    && matches!(
                        self.target.as_ref().map(|t| &t.plan),
                        Some(Plan::SignUp { .. })
                    ) =>
            {
                if let Some(target) = self.target.as_mut() {
                    target.sign_in_from_now_on();
                }
                self.send_plan();
            }
            ServerMsg::Device(device) => {
                if let Some(target) = self.target.as_mut() {
                    target.trust.device = Some(device);
                    self.trust_changed = true;
                }
            }
            ServerMsg::TwoFactorSetup { secret } => self.setup = Some(secret),
            ServerMsg::RecoveryCodes(codes) => {
                self.setup = None;
                self.recovery_codes = Some(codes);
            }
            ServerMsg::AccountDeleted => self.deleted = true,
            ServerMsg::Devices { list, this } => {
                self.devices = list;
                self.this_device = Some(this);
            }
            ServerMsg::ScreenStored { path } => {
                self.screen = Some(path);
                self.screens += 1;
            }
            ServerMsg::ScreenRefused(ErrorCode::PicturesOff) => {
                self.pictures_refused = true;
                self.screen = None;
            }
            ServerMsg::ScreenRefused(_) => {}
            ServerMsg::Error(code) => match self.phase {
                Phase::Connected => {
                    self.request_error = Some(code);
                    self.inbox.push(ServerMsg::Error(code));
                }
                _ => {
                    if code == ErrorCode::CodeWrong
                        && let Some(target) = self.target.as_mut()
                    {
                        target.code = None;
                    }
                    self.fail(Problem::from_error(code), now)
                }
            },
            ServerMsg::Notice(notice) => self.fail(Problem::from_notice(notice), now),
            other => {
                if let Some(other) = self.chat_receive(other)
                    && let Some(other) = self.cloud_receive(other)
                {
                    self.inbox.push(other);
                }
            }
        }
    }

    fn send_plan(&mut self) {
        let Some(target) = &self.target else {
            return;
        };
        let password = (!target.password.is_empty())
            .then(|| RelayPassword(target.password.clone()))
            .filter(|_| self.info.as_ref().is_none_or(|i| i.password_required));
        let msg = match &target.plan {
            Plan::Probe => return,
            Plan::SignUp {
                relay_id,
                name,
                invite,
            } => {
                let keys = relay_id.keys();
                ClientMsg::SignUp {
                    relay_password: password,
                    auth_key: keys.auth_key().clone(),
                    public_key: keys.public_key(),
                    display_name: name.clone(),
                    invite: *invite,
                    device_name: cloud::device_name(),
                }
            }
            Plan::SignIn { relay_id } => ClientMsg::SignIn {
                relay_password: password,
                auth_key: relay_id.keys().auth_key().clone(),
                device: target.trust.device.clone(),
                code: target.code.clone(),
                device_name: cloud::device_name(),
            },
        };
        self.phase = Phase::SigningIn;
        self.send(msg);
    }

    pub fn send(&mut self, msg: ClientMsg) -> bool {
        let Some(conn) = self.conn.as_mut() else {
            return false;
        };
        match msg.encode() {
            Ok(bytes) => {
                conn.send(bytes);
                true
            }
            Err(_) => false,
        }
    }

    pub fn send_screen(&mut self, png: Vec<u8>) -> bool {
        self.phase == Phase::Connected && self.send(ClientMsg::Screen(png))
    }

    pub fn pictures_off(&self) -> bool {
        self.pictures_refused || self.info.as_ref().is_some_and(|info| !info.pictures)
    }

    pub fn forget_screen(&mut self) {
        self.screen = None;
    }

    pub fn screen(&self) -> Option<(&str, u64)> {
        self.screen.as_deref().map(|path| (path, self.screens))
    }

    pub fn set_activity(&mut self, away: bool, game: Option<GameHash>) {
        self.away = away;
        self.my_game = game;
    }

    fn share_status(&mut self) {
        if self.phase != Phase::Connected {
            return;
        }
        if self.sent_away != Some(self.away) {
            self.sent_away = Some(self.away);
            let away = self.away;
            self.send(ClientMsg::SetStatus { away });
        }
        let Some(keys) = &self.keys else {
            return;
        };
        let mut sealed = Vec::new();
        for friend in self.friends.values() {
            let id = friend.online_id;
            if friend.state != FriendState::Friend || !is_online(friend.presence) {
                continue;
            }
            if self.told.get(&id) == Some(&self.my_game) {
                continue;
            }
            sealed.push((id, keys.seal_playing(&friend.public_key, self.my_game)));
        }
        for (to, sealed) in sealed {
            if self.send(ClientMsg::Playing { to, sealed }) {
                self.told.insert(to, self.my_game);
            }
        }
    }

    pub fn add_friend(&mut self, id: OnlineId) -> bool {
        self.request_error = None;
        self.send(ClientMsg::AddFriend(id))
    }

    pub fn answer(&mut self, from: OnlineId, accept: bool) -> bool {
        self.send(ClientMsg::AnswerFriend { from, accept })
    }

    pub fn remove_friend(&mut self, id: OnlineId) -> bool {
        self.send(ClientMsg::RemoveFriend(id))
    }

    pub fn block(&mut self, id: OnlineId) -> bool {
        self.send(ClientMsg::Block(id))
    }

    pub fn unblock(&mut self, id: OnlineId) -> bool {
        self.send(ClientMsg::Unblock(id))
    }

    pub fn set_display_name(&mut self, name: &str) -> bool {
        self.request_error = None;
        self.send(ClientMsg::SetDisplayName(name.to_owned()))
    }

    pub fn start_two_factor(&mut self) -> bool {
        self.request_error = None;
        self.send(ClientMsg::TwoFactorStart)
    }

    pub fn confirm_two_factor(&mut self, code: String) -> bool {
        self.request_error = None;
        self.send(ClientMsg::TwoFactorConfirm(OneTimeCode(code)))
    }

    pub fn cancel_two_factor(&mut self) {
        self.setup = None;
        self.request_error = None;
    }

    pub fn turn_off_two_factor(&mut self, code: String) -> bool {
        self.request_error = None;
        self.send(ClientMsg::TwoFactorOff(OneTimeCode(code)))
    }

    pub fn new_recovery_codes(&mut self, code: String) -> bool {
        self.request_error = None;
        self.send(ClientMsg::NewRecoveryCodes(OneTimeCode(code)))
    }

    pub fn delete_account(&mut self, code: Option<String>) -> bool {
        self.request_error = None;
        self.send(ClientMsg::DeleteAccount {
            code: code.map(OneTimeCode),
        })
    }

    pub fn two_factor_setup(&self) -> Option<&[u8]> {
        self.setup.as_deref()
    }

    pub fn recovery_codes(&self) -> Option<&[String]> {
        self.recovery_codes.as_deref()
    }

    pub fn forget_recovery_codes(&mut self) {
        self.recovery_codes = None;
    }

    pub fn take_deleted(&mut self) -> bool {
        std::mem::take(&mut self.deleted)
    }

    pub fn take_signed_out(&mut self) -> Option<ServerAddress> {
        self.signed_out.take()
    }

    pub fn devices(&self) -> (&[DeviceInfo], Option<DeviceId>) {
        (&self.devices, self.this_device)
    }

    pub fn sign_out_device(&mut self, id: DeviceId) -> bool {
        self.request_error = None;
        self.this_device != Some(id) && self.send(ClientMsg::SignOutDevice(id))
    }

    pub fn relay_key(&self) -> Option<RelayKey> {
        self.relay_key
    }

    pub fn pinned_key(&self) -> Option<RelayKey> {
        self.target.as_ref()?.trust.relay_key
    }

    pub fn offered_key(&self) -> Option<RelayKey> {
        self.offered_key
    }

    pub fn take_trust(&mut self) -> Option<Trust> {
        let target = self.target.as_ref().filter(|t| t.joined)?;
        std::mem::take(&mut self.trust_changed).then(|| target.trust.clone())
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn is_connected(&self) -> bool {
        self.phase == Phase::Connected
    }

    pub fn protocol(&self) -> u16 {
        self.protocol
    }

    #[cfg(test)]
    pub fn speak(&mut self, protocol: u16) {
        self.protocol = protocol;
    }

    pub fn comes_back(&self) -> bool {
        self.target.is_some() && !matches!(self.phase, Phase::Idle | Phase::Stopped(_))
    }

    pub fn is_active(&self) -> bool {
        self.target.is_some()
    }

    pub fn address(&self) -> Option<&ServerAddress> {
        self.target.as_ref().map(|t| &t.address)
    }

    pub fn info(&self) -> Option<&ServerInfo> {
        self.info.as_ref()
    }

    pub fn account(&self) -> Option<&Account> {
        self.account.as_ref()
    }

    pub fn keys(&self) -> Option<&Keys> {
        self.keys.as_ref()
    }

    pub fn friend(&self, id: OnlineId) -> Option<&Friend> {
        self.friends.get(&id)
    }

    pub fn display_name(&self, id: OnlineId) -> String {
        self.friend(id)
            .map(|f| f.display_name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    pub fn presence(&self, friend: &Friend) -> Option<Seen> {
        self.is_connected().then(|| seen(friend.presence))
    }

    pub fn friend_presence(&self, id: OnlineId) -> Option<Seen> {
        self.friend(id).and_then(|f| self.presence(f))
    }

    pub fn people(&self) -> impl Iterator<Item = &Friend> {
        self.friends.values()
    }

    pub fn friends(&self) -> Vec<&Friend> {
        let mut list: Vec<&Friend> = self
            .friends
            .values()
            .filter(|f| f.state == FriendState::Friend)
            .collect();
        list.sort_by(|a, b| {
            let rank = |f: &Friend| match seen(f.presence) {
                Seen::Online => (0, 0),
                Seen::Away => (1, 0),
                Seen::Offline { last_seen } => (2, u64::MAX - last_seen),
            };
            rank(a).cmp(&rank(b)).then_with(|| {
                a.display_name
                    .to_lowercase()
                    .cmp(&b.display_name.to_lowercase())
            })
        });
        list
    }

    pub fn with_state(&self, state: FriendState) -> Vec<&Friend> {
        self.friends.values().filter(|f| f.state == state).collect()
    }

    pub fn waiting_requests(&self) -> usize {
        if self.phase != Phase::Connected {
            return 0;
        }
        self.friends
            .values()
            .filter(|f| f.state == FriendState::Incoming)
            .count()
    }

    pub fn set_typing_indicators(&mut self, on: bool, now: Instant) {
        if let Some(signal) = self.typing.set_on(on) {
            self.send_typing(signal, now);
        }
    }

    pub fn compose(&mut self, draft: Option<(OnlineId, &str)>, now: Instant) {
        if self.phase != Phase::Connected {
            return;
        }
        for signal in self.typing.compose(draft, now) {
            self.send_typing(signal, now);
        }
    }

    fn send_typing(&mut self, signal: typing::Signal, now: Instant) {
        let (to, typing) = signal.to();
        if self.send(ClientMsg::Typing { to, typing }) {
            self.typing.went_out(now);
        }
    }

    #[cfg(test)]
    pub(crate) fn befriend_for_test(&mut self, friend: Friend) {
        self.friends.insert(friend.online_id, friend);
    }

    pub fn is_typing(&self, friend: OnlineId, now: Instant) -> bool {
        self.is_connected() && self.typing.is_typing(friend, now)
    }

    pub fn playing(&self, id: OnlineId) -> Option<&GameHash> {
        self.playing.get(&id)
    }

    pub fn inbox(&self) -> &[ServerMsg] {
        &self.inbox
    }

    pub fn take_joined(&mut self) -> Option<Joined> {
        self.joined.take()
    }

    pub fn take_account_change(&mut self) -> Option<&Account> {
        if std::mem::take(&mut self.account_changed) {
            self.account.as_ref()
        } else {
            None
        }
    }

    pub fn request_error(&self) -> Option<ErrorCode> {
        self.request_error
    }

    pub fn clear_request_error(&mut self) {
        self.request_error = None;
    }
}

pub fn seen(presence: Option<Presence>) -> Seen {
    match presence {
        Some(Presence::Online { away: false }) => Seen::Online,
        Some(Presence::Online { away: true }) => Seen::Away,
        Some(Presence::Offline { last_seen }) => Seen::Offline { last_seen },
        None => Seen::Offline { last_seen: 0 },
    }
}

pub fn game_title(game: &GameHash) -> Option<&'static str> {
    crate::game_name::known_name(game)
}

fn is_online(presence: Option<Presence>) -> bool {
    matches!(presence, Some(Presence::Online { .. }))
}

fn retry_delay(problem: Problem, failures: u32) -> Duration {
    let base = FIRST_RETRY.saturating_mul(1 << failures.saturating_sub(1).min(6));
    let base = base.min(LONGEST_RETRY);
    let base = match problem {
        Problem::KillSwitch | Problem::RelayOff => base.max(PAUSED_RETRY),
        _ => base,
    };
    let mut jitter = [0u8; 1];
    let _ = getrandom::getrandom(&mut jitter);
    base.mul_f32(0.8 + f32::from(jitter[0]) / 255.0 * 0.4)
}

fn app_name() -> String {
    let platform = if cfg!(target_arch = "wasm32") {
        "web"
    } else {
        std::env::consts::OS
    };
    format!("{} {platform}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests;
