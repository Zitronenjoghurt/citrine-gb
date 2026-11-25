use crate::library::{GameProgress, SetInfo};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Console {
    GameBoy,
    GameBoyColor,
    GameBoyAdvance,
    NintendoDs,
}

impl Console {
    pub fn id(self) -> u32 {
        match self {
            Console::GameBoy => 4,
            Console::GameBoyColor => 6,
            Console::GameBoyAdvance => 5,
            Console::NintendoDs => 18,
        }
    }

    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            4 => Some(Console::GameBoy),
            6 => Some(Console::GameBoyColor),
            5 => Some(Console::GameBoyAdvance),
            18 => Some(Console::NintendoDs),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct User {
    pub username: String,
    pub display_name: String,
    pub token: String,
    pub points: u32,
    pub points_hardcore: u32,
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Achievement {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub unlocked: bool,
    pub badge_name: String,
    pub badge_url: Option<String>,
    pub badge_locked_url: Option<String>,
    pub progress: Option<(String, f32)>,
    pub unlock_time: i64,
    pub rarity: f32,
}

impl Achievement {
    pub fn shown_badge_url(&self) -> Option<&str> {
        if self.unlocked {
            self.badge_url.as_deref()
        } else {
            self.badge_locked_url.as_deref()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupKind {
    ActiveChallenge,
    RecentlyUnlocked,
    AlmostThere,
    Locked,
    Unlocked,
    Unsynced,
    Unsupported,
    Unofficial,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub kind: GroupKind,
    pub achievements: Vec<Achievement>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub total: u32,
    pub unlocked: u32,
    pub points_total: u32,
    pub points_unlocked: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreFormat {
    Time,
    Score,
    Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leaderboard {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub format: ScoreFormat,
    pub lower_is_better: bool,
    pub tracking: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub rank: u32,
    pub user: String,
    pub score: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub board: u32,
    pub title: String,
    pub score: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    Top,
    AroundMe,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entries {
    pub board: u32,
    pub fetch: Fetch,
    pub total: u32,
    pub entries: Vec<Entry>,
    pub mine: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scoreboard {
    pub board: u32,
    pub submitted: String,
    pub best: String,
    pub rank: u32,
    pub entries: u32,
    pub top: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameState {
    pub id: u32,
    pub title: String,
    pub hash: String,
    pub badge_url: Option<String>,
    pub summary: Summary,
    pub groups: Vec<Group>,
    pub leaderboards: Vec<Leaderboard>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Load {
    #[default]
    Idle,
    Loading,
    Loaded,
    UnknownGame,
    NoSet,
    Unreachable,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInError {
    WrongPassword,
    TokenExpired,
    Network,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    SignedIn(User),
    SignInFailed {
        error: SignInError,
        message: String,
    },
    GameLoaded,
    Unlocked(Achievement),
    GameCompleted,
    BoardStarted(Leaderboard),
    BoardFailed(Leaderboard),
    BoardSubmitted {
        board: Leaderboard,
        score: String,
    },
    TrackersChanged,
    Scoreboard(Scoreboard),
    Entries(Entries),
    EntriesFailed {
        board: u32,
        fetch: Fetch,
        message: String,
    },
    ServerError(String),
    Disconnected,
    Reconnected,
    ResetNeeded,
    AllProgress {
        console: Console,
        games: Vec<GameProgress>,
    },
    GameList {
        console: Console,
        sets: Vec<SetInfo>,
    },
    FetchFailed {
        console: Console,
        message: String,
    },
}
