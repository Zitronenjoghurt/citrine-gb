#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

mod badges;
pub mod boards;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod net;
pub mod progress;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod secrets;

use crate::app::widgets::game_tile::Progress;
pub use badges::Badges;
use citrine_gb_achievements::{Achievement, GameState, Load, User};
use progress::LibraryProgress;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AchievementSettings {
    pub account: Option<Account>,
    pub in_library: bool,
    pub hardcore: bool,
}

impl Default for AchievementSettings {
    fn default() -> Self {
        Self {
            account: None,
            in_library: true,
            hardcore: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Account {
    pub username: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub token: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    SignedOut,
    SigningIn,
    SignedIn(User),
}

#[derive(Debug, Clone, Default)]
pub struct SignInForm {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    Unlocked(Achievement),
    Completed {
        title: String,
        badge_url: Option<String>,
    },
    Hardcore,
    Board {
        title: String,
        notice: boards::BoardNotice,
    },
}

impl Notice {
    pub fn badge_url(&self) -> Option<&str> {
        match self {
            Notice::Unlocked(achievement) => achievement.badge_url.as_deref(),
            Notice::Completed { badge_url, .. } => badge_url.as_deref(),
            Notice::Hardcore | Notice::Board { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Want {
    SignIn { username: String, password: String },
    SignOut,
    TryAgain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leaving {
    Quit,
    CloseGame,
}

pub const SIGN_UP_URL: &str = "https://retroachievements.org/createaccount.php";

pub struct Achievements {
    pub status: Status,
    pub game: Option<GameState>,
    pub load: Load,
    pub problem: Option<String>,
    pub form: SignInForm,
    pub show_all: bool,
    pub asking_hardcore: bool,
    pub badges: Badges,
    pub boards: boards::Boards,
    pub library: LibraryProgress,
    wants: Vec<Want>,
    #[cfg(not(target_arch = "wasm32"))]
    cache: crate::storage::Folders,
    #[cfg(not(target_arch = "wasm32"))]
    link: Link,
    #[cfg(not(target_arch = "wasm32"))]
    held: Held,
    #[cfg(not(target_arch = "wasm32"))]
    unsent: usize,
    #[cfg(not(target_arch = "wasm32"))]
    pub leaving: Option<Leaving>,
    #[cfg(not(target_arch = "wasm32"))]
    pub quit_anyway: bool,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Held {
    notices: Vec<(Notice, web_time::Instant)>,
    prefetch: Vec<String>,
    game: Option<u32>,
}

#[cfg(not(target_arch = "wasm32"))]
struct Link {
    net: Box<dyn net::Net>,
    secrets: Box<dyn secrets::Secrets>,
    waiting: Vec<(
        citrine_gb_achievements::RequestId,
        std::sync::mpsc::Receiver<citrine_gb_achievements::Response>,
    )>,
    user_agent: Option<String>,
    started: bool,
    by_token: bool,
    retry_at: Option<web_time::Instant>,
    last_idle: Option<web_time::Instant>,
    last_presence: Option<web_time::Instant>,
    library_retry_at: Option<web_time::Instant>,
}

impl Default for Achievements {
    fn default() -> Self {
        Self::with(
            Box::new(crate::covers::net::Offline),
            #[cfg(not(target_arch = "wasm32"))]
            Box::new(net::Offline),
            #[cfg(not(target_arch = "wasm32"))]
            Box::new(secrets::InFile),
            #[cfg(not(target_arch = "wasm32"))]
            crate::storage::Folders::default(),
        )
    }
}

impl Achievements {
    pub fn new(spawner: &citrine_gb_threads::Spawner) -> Self {
        Self::with(
            crate::covers::net::platform(spawner),
            #[cfg(not(target_arch = "wasm32"))]
            net::platform(spawner),
            #[cfg(not(target_arch = "wasm32"))]
            secrets::platform(),
            #[cfg(not(target_arch = "wasm32"))]
            crate::storage::Folders::default(),
        )
    }

    fn with(
        badges: Box<dyn crate::covers::net::Net>,
        #[cfg(not(target_arch = "wasm32"))] net: Box<dyn net::Net>,
        #[cfg(not(target_arch = "wasm32"))] secrets: Box<dyn secrets::Secrets>,
        #[cfg(not(target_arch = "wasm32"))] cache: crate::storage::Folders,
    ) -> Self {
        Self {
            status: Status::SignedOut,
            game: None,
            load: Load::Idle,
            problem: None,
            form: SignInForm::default(),
            show_all: false,
            asking_hardcore: false,
            badges: Badges::new(badges, crate::storage::Folders::default()),
            boards: boards::Boards::default(),
            library: LibraryProgress::default(),
            wants: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            cache,
            #[cfg(not(target_arch = "wasm32"))]
            link: Link {
                net,
                secrets,
                waiting: Vec::new(),
                user_agent: None,
                started: false,
                by_token: false,
                retry_at: None,
                last_idle: None,
                last_presence: None,
                library_retry_at: None,
            },
            #[cfg(not(target_arch = "wasm32"))]
            held: Held::default(),
            #[cfg(not(target_arch = "wasm32"))]
            unsent: 0,
            #[cfg(not(target_arch = "wasm32"))]
            leaving: None,
            #[cfg(not(target_arch = "wasm32"))]
            quit_anyway: false,
        }
    }

    pub fn user(&self) -> Option<&User> {
        match &self.status {
            Status::SignedIn(user) => Some(user),
            _ => None,
        }
    }

    pub fn sign_in(&mut self) {
        let username = self.form.username.trim().to_owned();
        let password = std::mem::take(&mut self.form.password);
        if username.is_empty() || password.is_empty() {
            return;
        }
        self.problem = None;
        self.status = Status::SigningIn;
        self.wants.push(Want::SignIn { username, password });
    }

    pub fn sign_out(&mut self) {
        self.wants.push(Want::SignOut);
    }

    pub fn try_again(&mut self) {
        self.wants.push(Want::TryAgain);
    }

    pub fn hardcore_wanted(settings: &crate::app::ui_state::settings::Settings) -> bool {
        settings.dev_mode && settings.achievements.hardcore
    }

    pub fn progress(&self, settings: &AchievementSettings, hash: Option<&str>) -> Option<Progress> {
        if !settings.in_library || self.user().is_none() {
            return None;
        }
        self.library.of(hash?)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod pump {
    use super::*;
    use crate::app::ui_state::settings::Settings;
    use crate::emulator::Emulator;
    use citrine_gb_achievements::{Event, Response, SignInError};
    use std::sync::mpsc::TryRecvError;
    use web_time::{Duration, Instant};

    const IDLE_EVERY: Duration = Duration::from_secs(1);
    const PRESENCE_EVERY: Duration = Duration::from_secs(1);
    const RETRY_AFTER: Duration = Duration::from_secs(30);
    const BADGE_WAIT: Duration = Duration::from_secs(2);

    impl Achievements {
        pub fn unsent(&self) -> usize {
            self.unsent
        }

        #[cfg(test)]
        pub(crate) fn on_net(net: Box<dyn net::Net>, cache: crate::storage::Folders) -> Self {
            Self::with(
                Box::new(crate::covers::net::Offline),
                net,
                Box::new(secrets::InFile),
                cache,
            )
        }

        #[cfg(test)]
        pub(crate) fn keep_tokens_in(&mut self, secrets: Box<dyn secrets::Secrets>) {
            self.link.secrets = secrets;
        }

        pub fn pump(
            &mut self,
            emulator: &mut Emulator,
            settings: &mut Settings,
            paused: bool,
        ) -> Vec<Notice> {
            let wanted = Self::hardcore_wanted(settings);
            let (notices, reset) = self.pump_client(emulator, settings, paused, wanted);
            if reset {
                emulator.restart();
            }
            let set = !matches!(
                self.load,
                Load::UnknownGame | Load::NoSet | Load::Unreachable | Load::Failed(_)
            );
            emulator.follow_hardcore(wanted && self.status != Status::SignedOut && set);
            emulator.follow_pause_due();
            notices
        }

        fn pump_client(
            &mut self,
            emulator: &mut Emulator,
            settings: &mut Settings,
            paused: bool,
            hardcore: bool,
        ) -> (Vec<Notice>, bool) {
            let now = Instant::now();
            let quiet = settings.achievements.account.is_none()
                && self.status == Status::SignedOut
                && self.wants.is_empty()
                && self.link.waiting.is_empty()
                && !emulator.has_achievements();
            if quiet {
                return (Vec::new(), false);
            }
            emulator
                .with_achievements(|seat| {
                    let (client, mut memory) = seat.parts();
                    let link = &mut self.link;
                    let ua = link.user_agent.get_or_insert_with(|| {
                        client.user_agent(&format!(
                            "Citrine/{} ({})",
                            env!("CARGO_PKG_VERSION"),
                            os_name()
                        ))
                    });
                    let ua = ua.clone();
                    client.set_hardcore(hardcore);
                    let retry = link.retry_at.is_some_and(|at| now >= at);
                    if (!link.started || retry)
                        && let Some(account) = &settings.achievements.account
                    {
                        link.started = true;
                        link.retry_at = None;
                        let token = if account.token.is_empty() {
                            link.secrets.get(&account.username)
                        } else {
                            Some(account.token.clone())
                        };
                        if let Some(token) = token {
                            link.by_token = true;
                            self.status = Status::SigningIn;
                            client.sign_in_with_token(&account.username, &token);
                        } else {
                            settings.achievements.account = None;
                            settings.dirty = true;
                        }
                    }
                    link.started = true;
                    let mut try_again = false;
                    for want in std::mem::take(&mut self.wants) {
                        match want {
                            Want::SignIn { username, password } => {
                                link.by_token = false;
                                link.retry_at = None;
                                client.sign_in_with_password(&username, &password);
                            }
                            Want::TryAgain => try_again = true,
                            Want::SignOut => {
                                client.sign_out();
                                if let Some(account) = &settings.achievements.account {
                                    link.secrets.remove(&account.username);
                                }
                                link.retry_at = None;
                                settings.achievements.account = None;
                                settings.dirty = true;
                                self.status = Status::SignedOut;
                                self.game = None;
                                self.load = Load::Idle;
                                self.problem = None;
                                self.form = SignInForm::default();
                                self.library = LibraryProgress::default();
                                self.boards.follow_game(None);
                                link.library_retry_at = None;
                            }
                        }
                    }
                    let mut answered = Vec::new();
                    link.waiting.retain(|(id, reply)| match reply.try_recv() {
                        Ok(response) => {
                            answered.push((*id, response));
                            false
                        }
                        Err(TryRecvError::Disconnected) => {
                            answered.push((*id, Response::network_error()));
                            false
                        }
                        Err(TryRecvError::Empty) => true,
                    });
                    let replied = !answered.is_empty();
                    for (id, response) in answered {
                        client.complete(id, response, &mut memory);
                    }
                    let mut notices = Vec::new();
                    let mut reset = false;
                    let mut boards_changed = false;
                    let events = client.take_events();
                    let changed = !events.is_empty();
                    for event in events {
                        match event {
                            Event::SignedIn(user) => {
                                if let Some(old) = &settings.achievements.account
                                    && old.username != user.username
                                {
                                    link.secrets.remove(&old.username);
                                }
                                let token = if link.secrets.set(&user.username, &user.token) {
                                    String::new()
                                } else {
                                    user.token.clone()
                                };
                                let account = Some(Account {
                                    username: user.username.clone(),
                                    token,
                                });
                                if settings.achievements.account != account {
                                    settings.achievements.account = account;
                                    settings.dirty = true;
                                }
                                self.problem = None;
                                self.status = Status::SignedIn(user);
                                seat.load_running_game();
                                continue;
                            }
                            Event::SignInFailed { error, message } => {
                                self.status = Status::SignedOut;
                                let by_token = link.by_token;
                                if by_token && error == SignInError::Network {
                                    link.retry_at = Some(now + RETRY_AFTER);
                                } else if by_token {
                                    if let Some(account) = &settings.achievements.account {
                                        link.secrets.remove(&account.username);
                                    }
                                    settings.achievements.account = None;
                                    settings.dirty = true;
                                }
                                self.problem = Some(sign_in_problem(error, &message, by_token));
                            }
                            Event::GameLoaded if hardcore => notices.push(Notice::Hardcore),
                            Event::GameLoaded => {}
                            Event::ResetNeeded => reset = true,
                            Event::Unlocked(achievement) => {
                                notices.push(Notice::Unlocked(achievement))
                            }
                            Event::GameCompleted => {
                                if let Some(game) = seat.client.game() {
                                    notices.push(Notice::Completed {
                                        title: game.title,
                                        badge_url: game.badge_url,
                                    });
                                }
                            }
                            Event::ServerError(message) => {
                                log::warn!("achievements: server error: {message}");
                            }
                            Event::AllProgress { games, .. } => self.library.take_progress(games),
                            Event::GameList { console, sets } => {
                                let today = crate::storage::now_unix();
                                self.library.take_sets(&self.cache, console, sets, today);
                            }
                            Event::FetchFailed { message, .. } => {
                                log::warn!("achievements: library progress didn't load: {message}");
                                self.library.asked = false;
                                link.library_retry_at = Some(now + RETRY_AFTER);
                            }
                            Event::BoardStarted(board) => {
                                boards_changed = true;
                                notices.push(Notice::Board {
                                    title: board.title,
                                    notice: boards::BoardNotice::Started,
                                });
                            }
                            Event::BoardFailed(board) => {
                                boards_changed = true;
                                notices.push(Notice::Board {
                                    title: board.title,
                                    notice: boards::BoardNotice::Failed,
                                });
                            }
                            Event::BoardSubmitted { .. } | Event::TrackersChanged => {
                                boards_changed = true;
                            }
                            Event::Scoreboard(scoreboard) => {
                                boards_changed = true;
                                let me = match &self.status {
                                    Status::SignedIn(user) => boards::me(user),
                                    _ => "",
                                };
                                self.boards.take_scoreboard(&scoreboard, me);
                                let title = self
                                    .boards
                                    .board(scoreboard.board)
                                    .map(|board| board.title.clone())
                                    .unwrap_or_default();
                                notices.push(Notice::Board {
                                    title,
                                    notice: boards::BoardNotice::Submitted {
                                        score: scoreboard.submitted,
                                        rank: scoreboard.rank,
                                        entries: scoreboard.entries,
                                    },
                                });
                            }
                            Event::Entries(entries) => self.boards.take_entries(entries),
                            Event::EntriesFailed { board, message, .. } => {
                                log::warn!("achievements: rankings didn't load: {message}");
                                self.boards.fetch_failed(board);
                            }
                            Event::Disconnected | Event::Reconnected => {}
                        }
                    }
                    let signed_in = matches!(self.status, Status::SignedIn(_));
                    let idle = *seat.client.load() == Load::Idle && seat.console().is_some();
                    if signed_in && (try_again || idle) {
                        seat.load_running_game();
                    }
                    if replied
                        && let Status::SignedIn(user) = &mut self.status
                        && let Some(fresh) = seat.client.user()
                    {
                        *user = fresh;
                    }
                    let (client, mut memory) = seat.parts();
                    let library_due = link.library_retry_at.is_none_or(|at| now >= at);
                    if settings.achievements.in_library
                        && !self.library.asked
                        && library_due
                        && matches!(self.status, Status::SignedIn(_))
                    {
                        self.library.asked = true;
                        link.library_retry_at = None;
                        let today = crate::storage::now_unix();
                        for console in progress::CONSOLES {
                            if !self.library.load_cached(&self.cache, console, today) {
                                client.fetch_sets(console);
                            }
                            client.fetch_progress(console);
                        }
                    }
                    let idle_due = link.last_idle.is_none_or(|at| now - at >= IDLE_EVERY);
                    if (paused || !client.runs_frames()) && idle_due {
                        link.last_idle = Some(now);
                        client.idle(&mut memory);
                    }
                    self.unsent = client.unsent_unlocks();
                    if self.load != *client.load() {
                        self.load = client.load().clone();
                    }
                    if changed || self.game.is_some() != client.has_game() {
                        self.game = client.game();
                        if let Some(game) = &self.game {
                            self.library.saw(game);
                        }
                    }
                    self.boards.follow_game(self.game.as_ref());
                    self.boards.hardcore = client.hardcore();
                    if boards_changed {
                        self.boards.attempts = client.attempts();
                    }
                    if signed_in && self.game.is_some() {
                        for (board, fetch, count) in self.boards.take_wants() {
                            client.fetch_entries(board, fetch, count);
                        }
                    }
                    let presence_due = link
                        .last_presence
                        .is_none_or(|at| now - at >= PRESENCE_EVERY);
                    if self.game.is_some() && presence_due {
                        link.last_presence = Some(now);
                        if let Some(presence) = client.rich_presence() {
                            self.boards.presence = Some(presence);
                        }
                    }
                    for request in client.take_requests() {
                        let id = request.id;
                        link.waiting.push((id, link.net.send(request, &ua)));
                    }
                    (notices, reset)
                })
                .unwrap_or_default()
        }

        pub fn follow(
            &mut self,
            ctx: &egui::Context,
            notices: Vec<Notice>,
        ) -> Vec<(Notice, Option<egui::TextureHandle>)> {
            let now = Instant::now();
            let held = &mut self.held;
            held.notices
                .extend(notices.into_iter().map(|notice| (notice, now)));
            let id = self.game.as_ref().map(|game| game.id);
            if held.game != id {
                held.game = id;
                held.prefetch = self
                    .game
                    .iter()
                    .flat_map(|game| &game.groups)
                    .flat_map(|group| &group.achievements)
                    .filter(|achievement| !achievement.unlocked)
                    .filter_map(|achievement| achievement.badge_url.clone())
                    .collect();
            }
            let badges = &mut self.badges;
            held.prefetch.retain(|url| {
                badges.get(ctx, url);
                !badges.done(url)
            });
            let mut ready = Vec::new();
            held.notices.retain(|(notice, at)| {
                let url = notice.badge_url().unwrap_or_default();
                let badge = badges.get(ctx, url);
                let show = badge.is_some() || badges.done(url) || now - *at >= BADGE_WAIT;
                if show {
                    ready.push((notice.clone(), badge));
                } else {
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
                !show
            });
            ready
        }
    }

    fn sign_in_problem(error: SignInError, message: &str, by_token: bool) -> String {
        match error {
            SignInError::WrongPassword if by_token => {
                "Signed out of RetroAchievements. Sign in again.".to_owned()
            }
            SignInError::TokenExpired => {
                "Signed out of RetroAchievements. Sign in again.".to_owned()
            }
            SignInError::WrongPassword => "Wrong username or password".to_owned(),
            SignInError::Network => "Can't reach RetroAchievements".to_owned(),
            SignInError::Other if message.is_empty() => "Sign-in didn't work".to_owned(),
            SignInError::Other => message.to_owned(),
        }
    }

    fn os_name() -> &'static str {
        match std::env::consts::OS {
            "linux" => "Linux",
            "macos" => "macOS",
            "windows" => "Windows",
            "android" => "Android",
            "ios" => "iOS",
            other => other,
        }
    }
}
