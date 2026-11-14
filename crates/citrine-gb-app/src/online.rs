use citrine_gb_net::{
    Account, ClientMsg, DeviceId, DeviceInfo, ErrorCode, Friend, FriendState, GameHash, InviteCode,
    Keys, Notice, OneTimeCode, OnlineId, PROTOCOL_VERSION, Presence, RelayId, RelayKey,
    RelayPassword, SAVE_PART_BYTES, ServerInfo, ServerMsg,
};
use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;
use web_time::Instant;

pub mod address;
pub mod chat;
pub mod chat_media;
pub mod cloud;
pub mod gts;
pub mod push;
pub mod settings;
pub mod synced;
#[expect(
    clippy::disallowed_types,
    reason = "the Android lifecycle seals the away status from the Java thread"
)]
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
const ANSWER_WAIT: Duration = Duration::from_secs(20);
const FIRST_RETRY: Duration = Duration::from_secs(1);
const LONGEST_RETRY: Duration = Duration::from_secs(60);
const PAUSED_RETRY: Duration = Duration::from_secs(30);
const WATCHED_PING_EVERY: Duration = Duration::from_secs(3);
const READ_PER_UPDATE: usize = SAVE_PART_BYTES;
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
            Self::UnknownAccount => "No account on this server has that relay ID".into(),
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
    asked_at: Option<Instant>,
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
    push: push::PushState,
    gts: gts::GtsCache,
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
            asked_at: None,
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
            push: push::PushState::default(),
            gts: gts::GtsCache::default(),
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
        self.gts_reset();
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
        self.asked_at = None;
    }

    fn drop_connection(&mut self) {
        self.conn = None;
        self.gts_disconnected();
        self.ping_out = None;
        self.round_trips.clear();
        self.sent_away = None;
        self.push_reset();
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
        let mut unread = READ_PER_UPDATE;
        while unread > 0
            && let Some(event) = self.conn.as_mut().and_then(|conn| conn.poll())
        {
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
                    unread = unread.saturating_sub(bytes.len());
                    self.heard_at = Some(now);
                    self.asked_at = None;
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
                    match &event {
                        TransportEvent::Failed(err) => {
                            log::warn!("the relay line failed: {err}");
                        }
                        _ => log::warn!("the relay closed the line"),
                    }
                    self.fail(problem, now);
                }
            }
        }
        self.keep_alive(now);
        self.share_status();
        self.share_push();
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
                let unanswered = self
                    .asked_at
                    .is_some_and(|at| now.duration_since(at) > ANSWER_WAIT);
                if now.duration_since(heard) > SILENCE_LIMIT && unanswered {
                    log::warn!(
                        "nothing heard from the relay for {} s, dropping the line",
                        now.duration_since(heard).as_secs()
                    );
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
        self.asked_at.get_or_insert(now);
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
                Phase::Connected if code == ErrorCode::GtsOff => {
                    self.gts_refused();
                    self.inbox.push(ServerMsg::Error(code));
                }
                Phase::Connected if code == ErrorCode::DsNetOff => {
                    self.inbox.push(ServerMsg::Error(code));
                }
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
                    && let Some(other) = self.gts_receive(other)
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

    pub(crate) fn send_bytes(&mut self, bytes: Vec<u8>) {
        if let Some(conn) = self.conn.as_mut() {
            conn.send(bytes);
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
        if self.took_away() {
            self.sent_away = Some(true);
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
pub(crate) mod tests {
    use super::*;
    use citrine_gb_net::DeviceKey;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::mpsc::Sender;

    #[derive(Default)]
    pub(crate) struct Wire {
        urls: Vec<String>,
        sent: Vec<ClientMsg>,
        pub(super) side: Option<Sender<ClientMsg>>,
        incoming: VecDeque<TransportEvent>,
        arrived: Option<Instant>,
        key: Option<RelayKey>,
    }

    struct Fake(Rc<RefCell<Wire>>);

    impl Transport for Fake {
        fn send(&mut self, bytes: Vec<u8>) {
            self.0
                .borrow_mut()
                .sent
                .push(ClientMsg::decode(&bytes).unwrap());
        }

        fn poll(&mut self) -> Option<TransportEvent> {
            self.0.borrow_mut().incoming.pop_front()
        }

        fn arrived(&self) -> Option<Instant> {
            self.0.borrow().arrived
        }

        fn relay_key(&self) -> Option<RelayKey> {
            self.0.borrow().key
        }

        fn side_line(&self) -> Option<transport::SideLine> {
            let side = self.0.borrow().side.clone()?;
            Some(Arc::new(move |bytes: Vec<u8>| {
                let _ = side.send(ClientMsg::decode(&bytes).unwrap());
            }))
        }
    }

    fn client() -> (Online, Rc<RefCell<Wire>>) {
        let wire = Rc::new(RefCell::new(Wire::default()));
        let shared = wire.clone();
        let online = Online::new(Box::new(move |url| {
            shared.borrow_mut().urls.push(url.to_owned());
            Box::new(Fake(shared.clone()))
        }));
        (online, wire)
    }

    fn push(wire: &Rc<RefCell<Wire>>, msg: ServerMsg) {
        wire.borrow_mut()
            .incoming
            .push_back(TransportEvent::Message(msg.encode().unwrap()));
    }

    fn open(wire: &Rc<RefCell<Wire>>) {
        wire.borrow_mut().incoming.push_back(TransportEvent::Opened);
    }

    #[cfg(feature = "nds")]
    pub(crate) fn deliver(online: &mut Online, msg: ServerMsg) {
        online.receive(msg, Instant::now());
    }

    pub(crate) fn sent(wire: &Rc<RefCell<Wire>>) -> Vec<ClientMsg> {
        std::mem::take(&mut wire.borrow_mut().sent)
    }

    fn info(sign_ups: bool) -> ServerInfo {
        ServerInfo {
            name: "Lemon".into(),
            version: "0.8.0".into(),
            protocol: PROTOCOL_VERSION,
            password_required: true,
            sign_ups_open: sign_ups,
            invites_required: false,
            quota_bytes: 100 << 20,
            max_message_bytes: 1 << 16,
            max_file_bytes: 1 << 24,
            game_sharing: false,
            pictures: true,
        }
    }

    fn account() -> Account {
        Account {
            online_id: OnlineId::new(0xABCD).unwrap(),
            display_name: "Clara".into(),
            storage_used: 0,
            quota_bytes: 0,
            two_factor: false,
            codes_left: 0,
        }
    }

    fn joined_relay(id: &RelayId) -> JoinedRelay {
        JoinedRelay {
            address: ServerAddress::parse("gb.lemon.industries").unwrap(),
            relay_id: id.clone(),
            password: "pw".into(),
            trust: Trust::default(),
        }
    }

    pub(crate) fn signed_in(id: &RelayId) -> (Online, Rc<RefCell<Wire>>, Instant) {
        let (mut online, wire) = client();
        let now = Instant::now();
        online.sign_in(joined_relay(id), now);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        push(&wire, ServerMsg::Welcome(account()));
        push(&wire, ServerMsg::Friends(vec![]));
        online.update(now);
        assert!(online.is_connected());
        sent(&wire);
        (online, wire, now)
    }

    #[test]
    fn a_probe_reads_the_server_then_creates_the_account() {
        let (mut online, wire) = client();
        let now = Instant::now();
        online.probe(
            ServerAddress::parse("gb.lemon.industries").unwrap(),
            "pw".into(),
            Trust::default(),
            now,
        );
        assert_eq!(online.phase(), Phase::Connecting);
        assert_eq!(wire.borrow().urls, ["wss://gb.lemon.industries/relay"]);

        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        online.update(now);
        assert_eq!(online.phase(), Phase::Probed);
        assert!(
            matches!(&sent(&wire)[..], [ClientMsg::Hello { protocol, .. }] if *protocol == PROTOCOL_VERSION)
        );

        online.create_account("Clara".into(), now);
        let msgs = sent(&wire);
        let [
            ClientMsg::SignUp {
                relay_password,
                auth_key,
                display_name,
                ..
            },
        ] = &msgs[..]
        else {
            panic!("{msgs:?}")
        };
        assert_eq!(relay_password, &Some(RelayPassword("pw".into())));
        assert_eq!(display_name, "Clara");
        assert_eq!(
            wire.borrow().urls.len(),
            1,
            "the probe's connection is reused"
        );

        push(&wire, ServerMsg::Welcome(account()));
        online.update(now);
        let joined = online.take_joined().expect("a join to remember");
        assert_eq!(joined.relay_id.keys().auth_key(), auth_key);
        assert_eq!(joined.account.display_name, "Clara");
        assert!(online.take_joined().is_none());
        assert_eq!(
            online.keys().map(|k| k.public_key()),
            Some(joined.relay_id.keys().public_key()),
            "a new account can seal what it plays"
        );
    }

    #[test]
    fn a_probe_the_server_closes_stays_probed_and_signs_in_on_a_new_connection() {
        let (mut online, wire) = client();
        let now = Instant::now();
        online.probe(
            ServerAddress::parse("gb.lemon.industries").unwrap(),
            "pw".into(),
            Trust::default(),
            now,
        );
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        online.update(now);
        sent(&wire);
        wire.borrow_mut().incoming.push_back(TransportEvent::Closed);
        online.update(now + Duration::from_secs(30));
        assert_eq!(online.phase(), Phase::Probed);

        online.create_account("Clara".into(), now + Duration::from_secs(40));
        assert_eq!(online.phase(), Phase::Connecting);
        assert_eq!(wire.borrow().urls.len(), 2);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        online.update(now + Duration::from_secs(41));
        assert!(
            sent(&wire)
                .iter()
                .any(|m| matches!(m, ClientMsg::SignUp { .. }))
        );
    }

    #[test]
    fn a_wrong_password_stops_and_asks() {
        let (mut online, wire) = client();
        let now = Instant::now();
        online.sign_in(joined_relay(&RelayId::random()), now);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        push(&wire, ServerMsg::Error(ErrorCode::WrongPassword));
        online.update(now);
        assert_eq!(online.phase(), Phase::Stopped(Problem::WrongPassword));
        online.update(now + Duration::from_secs(600));
        assert_eq!(
            wire.borrow().urls.len(),
            1,
            "no retry with a wrong password"
        );

        online.change_password("right".into(), now);
        assert_eq!(wire.borrow().urls.len(), 2);
    }

    #[test]
    fn an_unreachable_server_is_retried_with_growing_waits() {
        let (mut online, wire) = client();
        let mut now = Instant::now();
        online.sign_in(joined_relay(&RelayId::random()), now);
        let mut waits = Vec::new();
        for _ in 0..8 {
            wire.borrow_mut()
                .incoming
                .push_back(TransportEvent::Failed("refused".into()));
            online.update(now);
            let Phase::Retrying { at, problem } = online.phase() else {
                panic!("{:?}", online.phase())
            };
            assert_eq!(problem, Problem::Unreachable);
            waits.push(at.duration_since(now));
            now = at;
            online.update(now);
            assert_eq!(online.phase(), Phase::Connecting);
        }
        assert!(waits[0] < Duration::from_secs(2), "{waits:?}");
        assert!(waits[3] > waits[0] * 4, "{waits:?}");
        assert!(waits[7] <= Duration::from_secs(73), "{waits:?}");
        assert_eq!(wire.borrow().urls.len(), 9);
    }

    #[test]
    fn a_silent_connect_counts_as_unreachable() {
        let (mut online, _wire) = client();
        let now = Instant::now();
        online.sign_in(joined_relay(&RelayId::random()), now);
        online.update(now + Duration::from_secs(5));
        assert_eq!(online.phase(), Phase::Connecting);
        online.update(now + Duration::from_secs(11));
        assert!(matches!(
            online.phase(),
            Phase::Retrying {
                problem: Problem::Unreachable,
                ..
            }
        ));
    }

    #[test]
    fn the_kill_switch_waits_at_least_half_a_minute() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        push(&wire, ServerMsg::Notice(Notice::KillSwitch));
        online.update(now);
        let Phase::Retrying { at, problem } = online.phase() else {
            panic!("{:?}", online.phase())
        };
        assert_eq!(problem, Problem::KillSwitch);
        assert!(at.duration_since(now) >= Duration::from_secs(24));
    }

    #[test]
    fn signed_in_elsewhere_does_not_fight_back() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        push(&wire, ServerMsg::Notice(Notice::SignedInElsewhere));
        online.update(now);
        assert_eq!(online.phase(), Phase::Stopped(Problem::SignedInElsewhere));
    }

    #[test]
    fn an_invite_code_goes_with_the_sign_up_and_a_wrong_one_stops() {
        let (mut online, wire) = client();
        let now = Instant::now();
        online.probe(
            ServerAddress::parse("gb.lemon.industries").unwrap(),
            "pw".into(),
            Trust::default(),
            now,
        );
        open(&wire);
        push(
            &wire,
            ServerMsg::Info(ServerInfo {
                invites_required: true,
                ..info(true)
            }),
        );
        online.update(now);
        sent(&wire);
        let code: InviteCode = "7kq2-m9xd-4htb".parse().unwrap();
        online.create_invited_account("Mia".into(), Some(code), now);
        let msgs = sent(&wire);
        assert!(
            matches!(&msgs[..], [ClientMsg::SignUp { invite: Some(c), .. }] if *c == code),
            "{msgs:?}"
        );
        push(&wire, ServerMsg::Error(ErrorCode::InviteWrong));
        online.update(now);
        assert_eq!(online.phase(), Phase::Stopped(Problem::InviteWrong));
        assert!(!Problem::InviteWrong.retries());
        assert_eq!(
            Problem::from_error(ErrorCode::InviteUsed),
            Problem::InviteUsed
        );
        assert_eq!(
            Problem::from_error(ErrorCode::InviteNeeded),
            Problem::InviteNeeded
        );
    }

    #[test]
    fn a_join_that_fails_does_not_retry() {
        let (mut online, wire) = client();
        let now = Instant::now();
        online.probe(
            ServerAddress::parse("gb.lemon.industries").unwrap(),
            String::new(),
            Trust::default(),
            now,
        );
        wire.borrow_mut()
            .incoming
            .push_back(TransportEvent::Failed("refused".into()));
        online.update(now);
        assert_eq!(online.phase(), Phase::Stopped(Problem::Unreachable));
    }

    #[test]
    fn pings_keep_the_line_open_and_silence_drops_it() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        online.update(now + Duration::from_secs(30));
        assert!(
            sent(&wire)
                .iter()
                .any(|msg| matches!(msg, ClientMsg::Ping(_)))
        );
        online.update(now + Duration::from_secs(71));
        assert!(matches!(
            online.phase(),
            Phase::Retrying {
                problem: Problem::Lost,
                ..
            }
        ));
    }

    #[test]
    fn a_long_pause_between_updates_asks_before_calling_the_line_lost() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        let back = now + Duration::from_secs(120);
        online.update(back);
        assert!(online.is_connected(), "{:?}", online.phase());
        let msgs = sent(&wire);
        let [ClientMsg::Ping(n)] = msgs[..] else {
            panic!("{msgs:?}")
        };
        push(&wire, ServerMsg::Pong(n));
        online.update(back + Duration::from_millis(300));
        assert!(online.is_connected(), "{:?}", online.phase());
        online.update(back + Duration::from_secs(60));
        assert!(online.is_connected(), "{:?}", online.phase());
    }

    #[test]
    fn an_unanswered_ping_after_a_long_pause_drops_the_line() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        let back = now + Duration::from_secs(120);
        online.update(back);
        sent(&wire);
        online.update(back + Duration::from_secs(19));
        assert!(online.is_connected(), "{:?}", online.phase());
        online.update(back + Duration::from_secs(21));
        assert!(matches!(
            online.phase(),
            Phase::Retrying {
                problem: Problem::Lost,
                ..
            }
        ));
    }

    #[test]
    fn hidden_tab_pauses_keep_the_line() {
        let (mut online, wire, start) = signed_in(&RelayId::random());
        let step = Duration::from_millis(100);
        let day = Duration::from_secs(24 * 3600);
        let mut seed: u64 = 0x5eed_83a7;
        let mut roll = |lo: u64, hi: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            lo + (seed >> 33) % (hi - lo + 1)
        };
        let mut now = start;
        let mut next_pause = start + Duration::from_secs(roll(480, 720));
        let (mut pauses, mut drops) = (0, 0);
        while now.duration_since(start) < day {
            if now >= next_pause {
                pauses += 1;
                now += Duration::from_secs(roll(30, 300));
                next_pause = now + Duration::from_secs(roll(480, 720));
            }
            online.update(now);
            for msg in sent(&wire) {
                if let ClientMsg::Ping(n) = msg {
                    push(&wire, ServerMsg::Pong(n));
                }
            }
            if wire.borrow().urls.len() > drops + 1 {
                drops += 1;
                open(&wire);
                push(&wire, ServerMsg::Info(info(true)));
                push(&wire, ServerMsg::Welcome(account()));
                push(&wire, ServerMsg::Friends(vec![]));
            }
            now += step;
        }
        println!("hidden tab: {pauses} pauses in 24 h, {drops} drops");
        assert_eq!(drops, 0);
    }

    fn friend(id: &RelayId, online_id: u64, presence: Option<Presence>) -> Friend {
        Friend {
            online_id: OnlineId::new(online_id).unwrap(),
            display_name: format!("Friend {online_id}"),
            public_key: id.keys().public_key(),
            state: FriendState::Friend,
            presence,
        }
    }

    #[test]
    fn the_game_goes_sealed_to_each_online_friend_once() {
        let me = RelayId::random();
        let (mut online, wire, now) = signed_in(&me);
        let ana = RelayId::random();
        let ben = RelayId::random();
        push(
            &wire,
            ServerMsg::Friends(vec![
                friend(&ana, 1, Some(Presence::Online { away: false })),
                friend(&ben, 2, Some(Presence::Offline { last_seen: 5 })),
            ]),
        );
        let game = [9; 20];
        online.set_activity(false, Some(game));
        online.update(now);
        let msgs = sent(&wire);
        assert_eq!(msgs.len(), 1, "{msgs:?}");
        let ClientMsg::Playing { to, sealed } = &msgs[0] else {
            panic!("{msgs:?}")
        };
        assert_eq!(to.value(), 1);
        assert_eq!(
            ana.keys().open_playing(&me.keys().public_key(), sealed),
            Ok(Some(game))
        );

        online.update(now);
        assert!(sent(&wire).is_empty(), "nothing new to say");

        push(
            &wire,
            ServerMsg::Presence {
                who: OnlineId::new(2).unwrap(),
                presence: Presence::Online { away: false },
            },
        );
        online.update(now);
        let msgs = sent(&wire);
        assert!(
            matches!(&msgs[..], [ClientMsg::Playing { to, .. }] if to.value() == 2),
            "{msgs:?}"
        );

        push(
            &wire,
            ServerMsg::Presence {
                who: OnlineId::new(1).unwrap(),
                presence: Presence::Online { away: false },
            },
        );
        online.update(now);
        let msgs = sent(&wire);
        assert!(
            matches!(&msgs[..], [ClientMsg::Playing { to, .. }] if to.value() == 1),
            "a friend who signs in on another device hears it again: {msgs:?}"
        );

        online.set_activity(true, None);
        online.update(now);
        let msgs = sent(&wire);
        assert_eq!(msgs[0], ClientMsg::SetStatus { away: true });
        assert_eq!(msgs.len(), 3, "{msgs:?}");
    }

    #[test]
    fn a_friends_game_opens_with_their_key_and_clears_when_they_leave() {
        let me = RelayId::random();
        let (mut online, wire, now) = signed_in(&me);
        let ana = RelayId::random();
        let ana_id = OnlineId::new(1).unwrap();
        push(
            &wire,
            ServerMsg::Friends(vec![friend(
                &ana,
                1,
                Some(Presence::Online { away: false }),
            )]),
        );
        let game = [7; 20];
        push(
            &wire,
            ServerMsg::Playing {
                from: ana_id,
                sealed: ana.keys().seal_playing(&me.keys().public_key(), Some(game)),
            },
        );
        online.update(now);
        assert_eq!(online.playing(ana_id), Some(&game));

        let eve = RelayId::random();
        push(
            &wire,
            ServerMsg::Playing {
                from: ana_id,
                sealed: eve
                    .keys()
                    .seal_playing(&me.keys().public_key(), Some([1; 20])),
            },
        );
        online.update(now);
        assert_eq!(
            online.playing(ana_id),
            Some(&game),
            "a forged game is ignored"
        );

        push(
            &wire,
            ServerMsg::Presence {
                who: ana_id,
                presence: Presence::Offline { last_seen: 9 },
            },
        );
        online.update(now);
        assert_eq!(online.playing(ana_id), None);
    }

    #[test]
    fn friends_sort_online_then_away_then_recently_seen() {
        let me = RelayId::random();
        let (mut online, wire, now) = signed_in(&me);
        let key = RelayId::random();
        push(
            &wire,
            ServerMsg::Friends(vec![
                friend(&key, 1, Some(Presence::Offline { last_seen: 10 })),
                friend(&key, 2, Some(Presence::Online { away: true })),
                friend(&key, 3, Some(Presence::Offline { last_seen: 50 })),
                friend(&key, 4, Some(Presence::Online { away: false })),
                Friend {
                    state: FriendState::Incoming,
                    presence: None,
                    ..friend(&key, 5, None)
                },
            ]),
        );
        online.update(now);
        let order: Vec<u64> = online
            .friends()
            .iter()
            .map(|f| f.online_id.value())
            .collect();
        assert_eq!(order, [4, 2, 3, 1]);
        assert_eq!(online.waiting_requests(), 1);
    }

    #[test]
    fn messages_for_chat_and_sessions_reach_the_inbox_for_one_update() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        push(
            &wire,
            ServerMsg::Invite {
                session: 3,
                from: OnlineId::new(1).unwrap(),
                sealed: citrine_gb_net::Sealed {
                    nonce: Default::default(),
                    ciphertext: Vec::new(),
                },
            },
        );
        online.update(now);
        assert_eq!(online.inbox().len(), 1);
        online.update(now);
        assert!(online.inbox().is_empty());
    }

    #[test]
    fn pictures_off_comes_from_the_server_info_or_a_refusal() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        assert!(!online.pictures_off());
        push(
            &wire,
            ServerMsg::ScreenStored {
                path: "/f/ab".into(),
            },
        );
        online.update(now);
        assert_eq!(online.screen(), Some(("/f/ab", 1)));
        push(&wire, ServerMsg::ScreenRefused(ErrorCode::RateLimited));
        online.update(now);
        assert!(!online.pictures_off());
        push(&wire, ServerMsg::ScreenRefused(ErrorCode::PicturesOff));
        online.update(now);
        assert!(online.pictures_off());
        assert_eq!(online.screen(), None);
        assert!(online.is_connected());

        let (mut online, wire) = client();
        online.sign_in(joined_relay(&RelayId::random()), now);
        open(&wire);
        push(
            &wire,
            ServerMsg::Info(ServerInfo {
                pictures: false,
                ..info(true)
            }),
        );
        online.update(now);
        assert!(online.pictures_off());
    }

    #[test]
    fn a_failed_request_while_connected_keeps_the_connection() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        online.add_friend(OnlineId::new(77).unwrap());
        push(&wire, ServerMsg::Error(ErrorCode::UnknownUser));
        online.update(now);
        assert!(online.is_connected());
        assert_eq!(online.request_error(), Some(ErrorCode::UnknownUser));
    }

    fn new_account() -> (Online, Rc<RefCell<Wire>>, Instant, Keys) {
        let (mut online, wire) = client();
        let now = Instant::now();
        online.probe(
            ServerAddress::parse("gb.lemon.industries").unwrap(),
            "pw".into(),
            Trust::default(),
            now,
        );
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        online.update(now);
        online.create_account("Clara".into(), now);
        push(&wire, ServerMsg::Welcome(account()));
        online.update(now);
        let joined = online.take_joined().expect("a join to remember");
        sent(&wire);
        (online, wire, now, joined.relay_id.keys())
    }

    #[test]
    fn a_new_account_signs_in_when_the_connection_comes_back() {
        let (mut online, wire, now, keys) = new_account();
        wire.borrow_mut().incoming.push_back(TransportEvent::Closed);
        online.update(now);
        let Phase::Retrying { at, .. } = online.phase() else {
            panic!("{:?}", online.phase())
        };

        online.update(at);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        online.update(at);
        let msgs = sent(&wire);
        assert!(
            matches!(&msgs[..], [ClientMsg::Hello { .. }, ClientMsg::SignIn { auth_key, .. }] if auth_key == keys.auth_key()),
            "{msgs:?}"
        );

        push(&wire, ServerMsg::Welcome(account()));
        online.update(at);
        assert_eq!(online.phase(), Phase::Connected);
        assert!(online.take_joined().is_none());

        online.retry_now(at);
        online.change_password("new".into(), at);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        online.update(at);
        assert!(
            sent(&wire)
                .iter()
                .all(|msg| !matches!(msg, ClientMsg::SignUp { .. }))
        );
    }

    #[test]
    fn a_sign_up_the_server_already_has_signs_in_instead() {
        let (mut online, wire) = client();
        let now = Instant::now();
        online.probe(
            ServerAddress::parse("gb.lemon.industries").unwrap(),
            "pw".into(),
            Trust::default(),
            now,
        );
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        online.update(now);
        online.create_account("Clara".into(), now);
        let msgs = sent(&wire);
        let Some(ClientMsg::SignUp { auth_key, .. }) = msgs.last() else {
            panic!("{msgs:?}")
        };
        let auth_key = auth_key.clone();

        push(&wire, ServerMsg::Error(ErrorCode::AccountExists));
        online.update(now);
        let msgs = sent(&wire);
        assert!(
            matches!(&msgs[..], [ClientMsg::SignIn { auth_key: again, .. }] if *again == auth_key),
            "{msgs:?}"
        );
        push(&wire, ServerMsg::Welcome(account()));
        online.update(now);
        assert_eq!(online.phase(), Phase::Connected);
        assert!(online.take_joined().is_some());
    }

    #[test]
    fn watching_the_round_trip_pings_every_few_seconds() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        assert_eq!(online.round_trip_ms(), None);
        online.watch_round_trip(now);
        let msgs = sent(&wire);
        let [ClientMsg::Ping(n)] = msgs[..] else {
            panic!("{msgs:?}")
        };
        push(&wire, ServerMsg::Pong(n));
        online.update(now + Duration::from_millis(42));
        assert_eq!(online.round_trip_ms(), Some(42));

        let later = now + Duration::from_millis(3500);
        online.watch_round_trip(later);
        online.update(later);
        assert!(matches!(sent(&wire)[..], [ClientMsg::Ping(_)]));

        let unwatched = later + Duration::from_secs(10);
        online.update(unwatched);
        sent(&wire);
        online.update(unwatched + Duration::from_secs(4));
        assert!(sent(&wire).is_empty(), "no fast pings once nobody looks");

        wire.borrow_mut().incoming.push_back(TransportEvent::Closed);
        online.update(unwatched + Duration::from_secs(5));
        assert_eq!(online.round_trip_ms(), None);
    }

    #[test]
    fn the_round_trip_counts_when_the_pong_arrived_not_the_next_frame() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        online.watch_round_trip(now);
        let msgs = sent(&wire);
        let [ClientMsg::Ping(n)] = msgs[..] else {
            panic!("{msgs:?}")
        };
        push(&wire, ServerMsg::Pong(n));
        wire.borrow_mut().arrived = Some(now + Duration::from_millis(12));
        online.update(now + Duration::from_millis(250));
        assert_eq!(online.round_trip_ms(), Some(12));
    }

    type Stored = (Online, Rc<RefCell<Wire>>, Instant, tempfile::TempDir);

    fn with_storage(relay_quota: u64, quota: u64, used: u64) -> Stored {
        let (mut online, wire) = client();
        let dir = tempfile::tempdir().unwrap();
        online.cloud_attach(crate::storage::SaveStore::at(dir.path().to_path_buf()));
        let now = Instant::now();
        online.sign_in(joined_relay(&RelayId::random()), now);
        open(&wire);
        push(
            &wire,
            ServerMsg::Info(ServerInfo {
                quota_bytes: relay_quota,
                ..info(true)
            }),
        );
        push(
            &wire,
            ServerMsg::Welcome(Account {
                storage_used: used,
                quota_bytes: quota,
                ..account()
            }),
        );
        push(&wire, ServerMsg::Friends(vec![]));
        online.update(now);
        online.set_cloud(Some("clara".into()), true);
        online.update(now);
        push(&wire, ServerMsg::Saves(vec![]));
        online.update(now);
        sent(&wire);
        (online, wire, now, dir)
    }

    fn puts(wire: &Rc<RefCell<Wire>>) -> usize {
        sent(wire)
            .iter()
            .filter(|m| matches!(m, ClientMsg::PutSave { .. }))
            .count()
    }

    #[test]
    fn settings_stay_here_without_an_upload_when_the_cloud_has_no_room() {
        use super::cloud::SyncState;
        let later = Duration::from_secs(4);
        for (relay, quota, used, state) in [
            (0, 0, 0, SyncState::NoStorage),
            (100 << 20, 0, 0, SyncState::NoAccountStorage),
            (100 << 20, 1 << 20, (1 << 20) - 100, SyncState::Full),
        ] {
            let (mut online, wire, now, _dir) = with_storage(relay, quota, used);
            online.account_put("settings", vec![7; 200]);
            online.update(now + later);
            assert_eq!(puts(&wire), 0);
            assert_eq!(online.settings_problem(), Some(state));
        }
    }

    #[test]
    fn a_cloud_upload_with_no_answer_goes_again_after_half_a_minute() {
        let (mut online, wire, now, _dir) = with_storage(100 << 20, 1 << 20, 0);
        online.account_put("settings", vec![7; 200]);
        online.update(now + Duration::from_secs(4));
        assert_eq!(puts(&wire), 1);
        online.update(now + Duration::from_secs(20));
        assert_eq!(puts(&wire), 0);
        online.update(now + Duration::from_secs(36));
        assert_eq!(puts(&wire), 1);
    }

    #[test]
    fn the_share_menu_says_why_files_cant_go() {
        for (relay, quota, used, why) in [
            (0, 0, 0, Some("This relay doesn't keep files")),
            (
                100 << 20,
                0,
                0,
                Some("Your account on this relay has no storage"),
            ),
            (
                100 << 20,
                1 << 20,
                (1 << 20) - 100,
                Some("Your storage is full"),
            ),
            (100 << 20, 1 << 20, 0, None),
        ] {
            let (online, _, _, _dir) = with_storage(relay, quota, used);
            assert_eq!(online.share_blocked(), why);
        }
    }

    const KEY: RelayKey = RelayKey([1; 32]);
    const OTHER_KEY: RelayKey = RelayKey([2; 32]);

    fn trusted(id: &RelayId, key: Option<RelayKey>, device: Option<DeviceKey>) -> JoinedRelay {
        JoinedRelay {
            trust: Trust {
                relay_key: key,
                expected: None,
                device,
            },
            ..joined_relay(id)
        }
    }

    #[test]
    fn the_first_join_pins_the_relay_key_and_keeps_the_device() {
        let (mut online, wire) = client();
        let now = Instant::now();
        wire.borrow_mut().key = Some(KEY);
        online.probe(
            ServerAddress::parse("gb.lemon.industries").unwrap(),
            "pw".into(),
            Trust::default(),
            now,
        );
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        online.update(now);
        assert_eq!(online.phase(), Phase::Probed);
        assert_eq!(online.relay_key(), Some(KEY));
        online.create_account("Clara".into(), now);
        push(&wire, ServerMsg::Device(DeviceKey([7; 32])));
        push(&wire, ServerMsg::Welcome(account()));
        online.update(now);
        let joined = online.take_joined().unwrap();
        assert_eq!(joined.trust.relay_key, Some(KEY));
        assert_eq!(joined.trust.device, Some(DeviceKey([7; 32])));
    }

    #[test]
    fn a_changed_relay_key_stops_before_hello_until_accepted() {
        let (mut online, wire) = client();
        let now = Instant::now();
        let id = RelayId::random();
        wire.borrow_mut().key = Some(OTHER_KEY);
        online.sign_in(trusted(&id, Some(KEY), None), now);
        open(&wire);
        online.update(now);
        assert_eq!(online.phase(), Phase::Stopped(Problem::KeyChanged));
        assert!(
            sent(&wire).is_empty(),
            "nothing leaves before the key matches"
        );
        assert_eq!(online.offered_key(), Some(OTHER_KEY));
        assert!(!Problem::KeyChanged.retries());

        online.accept_new_key(now);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        online.update(now);
        let msgs = sent(&wire);
        assert!(matches!(msgs[0], ClientMsg::Hello { .. }));
        assert!(matches!(msgs[1], ClientMsg::SignIn { .. }));
        let trust = online.take_trust().unwrap();
        assert_eq!(trust.relay_key, Some(OTHER_KEY));
        assert_eq!(online.pinned_key(), Some(OTHER_KEY));
    }

    #[test]
    fn a_known_relay_key_connects_as_before() {
        let (mut online, wire) = client();
        let now = Instant::now();
        let id = RelayId::random();
        wire.borrow_mut().key = Some(KEY);
        online.sign_in(trusted(&id, Some(KEY), None), now);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        push(&wire, ServerMsg::Welcome(account()));
        online.update(now);
        assert!(online.is_connected());
        assert!(online.take_trust().is_none());
    }

    #[test]
    fn an_address_key_that_doesnt_match_stops_the_probe() {
        let (mut online, wire) = client();
        let now = Instant::now();
        wire.borrow_mut().key = Some(OTHER_KEY);
        online.probe(
            ServerAddress::parse("gb.lemon.industries").unwrap(),
            "pw".into(),
            Trust {
                expected: Some(KEY.fingerprint()),
                ..Trust::default()
            },
            now,
        );
        open(&wire);
        online.update(now);
        assert_eq!(online.phase(), Phase::Stopped(Problem::KeyMismatch));
        assert!(sent(&wire).is_empty());
    }

    #[test]
    fn a_new_device_signs_in_with_a_code() {
        let (mut online, wire) = client();
        let now = Instant::now();
        let id = RelayId::random();
        let old = DeviceKey([3; 32]);
        online.sign_in(trusted(&id, None, Some(old.clone())), now);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        online.update(now);
        let msgs = sent(&wire);
        assert!(
            matches!(&msgs[1], ClientMsg::SignIn { device: Some(d), code: None, .. } if *d == old)
        );
        push(&wire, ServerMsg::Error(ErrorCode::CodeNeeded));
        online.update(now);
        assert_eq!(online.phase(), Phase::Stopped(Problem::CodeNeeded));
        assert!(Problem::CodeNeeded.needs_code());

        online.sign_in_with_code("111111".into(), now);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        push(&wire, ServerMsg::Error(ErrorCode::CodeWrong));
        online.update(now);
        assert_eq!(online.phase(), Phase::Stopped(Problem::CodeWrong));
        let msgs = sent(&wire);
        assert!(matches!(&msgs[1], ClientMsg::SignIn { code: Some(c), .. } if c.0 == "111111"));

        online.sign_in_with_code("222222".into(), now);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        push(&wire, ServerMsg::Device(DeviceKey([4; 32])));
        push(&wire, ServerMsg::Welcome(account()));
        online.update(now);
        assert!(online.is_connected());
        let trust = online.take_trust().unwrap();
        assert_eq!(trust.device, Some(DeviceKey([4; 32])));

        sent(&wire);
        wire.borrow_mut().incoming.push_back(TransportEvent::Closed);
        online.update(now);
        let Phase::Retrying { at, .. } = online.phase() else {
            panic!("{:?}", online.phase())
        };
        online.update(at);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        online.update(at);
        let msgs = sent(&wire);
        let sign_in = msgs.iter().find(|m| matches!(m, ClientMsg::SignIn { .. }));
        assert!(
            matches!(sign_in, Some(ClientMsg::SignIn { device: Some(d), code: None, .. }) if *d == DeviceKey([4; 32])),
            "a used code is never sent again"
        );
    }

    #[test]
    fn two_factor_setup_and_codes_follow_the_server() {
        let id = RelayId::random();
        let (mut online, wire, now) = signed_in(&id);
        assert!(online.start_two_factor());
        push(
            &wire,
            ServerMsg::TwoFactorSetup {
                secret: vec![9; 20],
            },
        );
        online.update(now);
        assert_eq!(online.two_factor_setup(), Some(&[9u8; 20][..]));
        online.confirm_two_factor("123456".into());
        push(&wire, ServerMsg::Error(ErrorCode::CodeWrong));
        online.update(now);
        assert!(online.is_connected());
        assert_eq!(online.request_error(), Some(ErrorCode::CodeWrong));
        online.confirm_two_factor("654321".into());
        push(
            &wire,
            ServerMsg::RecoveryCodes(vec!["1111-2222-3333".into()]),
        );
        online.update(now);
        assert_eq!(online.request_error(), None);
        assert!(online.two_factor_setup().is_none());
        assert_eq!(online.recovery_codes().unwrap().len(), 1);
        online.forget_recovery_codes();
        assert!(online.recovery_codes().is_none());
        let msgs = sent(&wire);
        assert!(matches!(msgs[0], ClientMsg::TwoFactorStart));
        assert!(matches!(&msgs[2], ClientMsg::TwoFactorConfirm(c) if c.0 == "654321"));
    }

    #[test]
    fn a_deleted_account_says_so_once() {
        let id = RelayId::random();
        let (mut online, wire, now) = signed_in(&id);
        online.delete_account(Some("1234-5678-9012".into()));
        assert!(matches!(
            &sent(&wire)[..],
            [ClientMsg::DeleteAccount { code: Some(c) }] if c.0 == "1234-5678-9012"
        ));
        push(&wire, ServerMsg::AccountDeleted);
        online.update(now);
        assert!(online.take_deleted());
        assert!(!online.take_deleted());
    }

    fn two_devices(this: DeviceId) -> ServerMsg {
        let device = |id, name: &str| DeviceInfo {
            id,
            name: name.into(),
            added: 1,
            last_seen: 2,
            online: true,
        };
        ServerMsg::Devices {
            list: vec![device(3, "Firefox on Mac"), device(4, "Android phone")],
            this,
        }
    }

    #[test]
    fn the_sign_in_names_this_device() {
        let (mut online, wire) = client();
        let now = Instant::now();
        online.sign_in(joined_relay(&RelayId::random()), now);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        online.update(now);
        let msgs = sent(&wire);
        assert!(
            matches!(&msgs[1], ClientMsg::SignIn { device_name, .. } if *device_name == cloud::device_name())
        );
    }

    #[test]
    fn the_device_list_comes_from_the_relay() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        push(&wire, two_devices(3));
        online.update(now);
        let (list, this) = online.devices();
        assert_eq!(list.len(), 2);
        assert_eq!(this, Some(3));

        assert!(!online.sign_out_device(3), "this device leaves instead");
        assert!(online.sign_out_device(4));
        assert!(matches!(&sent(&wire)[..], [ClientMsg::SignOutDevice(4)]));
    }

    #[test]
    fn leaving_signs_this_device_out_on_the_relay_first() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        push(&wire, two_devices(3));
        online.update(now);
        online.sign_out_here();
        assert!(matches!(&sent(&wire)[..], [ClientMsg::SignOutDevice(3)]));
        assert_eq!(online.phase(), Phase::Idle);
        assert!(online.devices().0.is_empty());
    }

    #[test]
    fn a_device_signed_out_from_another_forgets_the_relay() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        push(&wire, ServerMsg::Notice(Notice::SignedOut));
        online.update(now);
        assert_eq!(online.phase(), Phase::Stopped(Problem::SignedOut));
        assert!(!Problem::SignedOut.retries());
        let address = online.take_signed_out().unwrap();
        assert_eq!(address.key(), "gb.lemon.industries");
        assert!(online.take_signed_out().is_none());
    }

    #[test]
    fn a_signed_out_device_key_is_refused_at_sign_in() {
        let (mut online, wire) = client();
        let now = Instant::now();
        online.sign_in(joined_relay(&RelayId::random()), now);
        open(&wire);
        push(&wire, ServerMsg::Info(info(true)));
        push(&wire, ServerMsg::Error(ErrorCode::SignedOut));
        online.update(now);
        assert_eq!(online.phase(), Phase::Stopped(Problem::SignedOut));
        assert!(online.take_signed_out().is_some());
    }

    #[test]
    fn an_old_relay_that_cannot_read_typing_is_left_alone_quietly() {
        let id = RelayId::random();
        let (mut online, wire, now) = signed_in(&id);
        let ana = OnlineId::new(7).unwrap();
        push(
            &wire,
            ServerMsg::Friend(friend(&id, 7, Some(Presence::Online { away: false }))),
        );
        online.update(now);
        sent(&wire);
        online.compose(Some((ana, "")), now);
        online.compose(Some((ana, "h")), now);
        assert_eq!(
            sent(&wire),
            [ClientMsg::Typing {
                to: ana,
                typing: true
            }]
        );
        push(&wire, ServerMsg::Error(ErrorCode::BadMessage));
        online.update(now);
        assert_eq!(online.request_error(), None);
        let later = now + crate::online::typing::SEND_EVERY * 2;
        online.compose(Some((ana, "hi")), later);
        online.compose(Some((ana, "")), later);
        assert!(sent(&wire).is_empty());
    }

    #[test]
    fn a_typing_friend_shows_only_for_friends_and_clears_when_they_go_offline() {
        let id = RelayId::random();
        let (mut online, wire, now) = signed_in(&id);
        let ana = OnlineId::new(7).unwrap();
        let stranger = OnlineId::new(9).unwrap();
        push(
            &wire,
            ServerMsg::Friend(friend(&id, 7, Some(Presence::Online { away: false }))),
        );
        push(
            &wire,
            ServerMsg::Typing {
                from: stranger,
                typing: true,
            },
        );
        push(
            &wire,
            ServerMsg::Typing {
                from: ana,
                typing: true,
            },
        );
        online.update(now);
        assert!(online.is_typing(ana, now));
        assert!(!online.is_typing(stranger, now));
        push(
            &wire,
            ServerMsg::Presence {
                who: ana,
                presence: Presence::Offline { last_seen: 1 },
            },
        );
        online.update(now);
        assert!(!online.is_typing(ana, now));
    }
}
