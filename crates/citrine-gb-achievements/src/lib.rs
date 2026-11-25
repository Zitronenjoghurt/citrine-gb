#[cfg(not(target_arch = "wasm32"))]
mod client;
mod game;
mod library;

#[cfg(not(target_arch = "wasm32"))]
pub use client::{Client, Memory, NoMemory, Request, RequestId, Response, hash_rom};
pub use game::{
    Achievement, Attempt, Console, Entries, Entry, Event, Fetch, GameState, Group, GroupKind,
    Leaderboard, Load, ScoreFormat, Scoreboard, SignInError, Summary, User,
};
pub use library::{GameProgress, SetInfo};
