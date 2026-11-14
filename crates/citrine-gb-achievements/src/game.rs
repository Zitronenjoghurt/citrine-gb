use crate::library::{GameProgress, SetInfo};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Console {
    GameBoy,
    GameBoyColor,
}

impl Console {
    pub fn id(self) -> u32 {
        match self {
            Console::GameBoy => 4,
            Console::GameBoyColor => 6,
        }
    }

    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            4 => Some(Console::GameBoy),
            6 => Some(Console::GameBoyColor),
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

#[derive(Clone, Debug, PartialEq)]
pub struct GameState {
    pub id: u32,
    pub title: String,
    pub hash: String,
    pub badge_url: Option<String>,
    pub summary: Summary,
    pub groups: Vec<Group>,
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
    NoSet,
    GameLoadFailed(String),
    Unlocked(Achievement),
    GameCompleted,
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
