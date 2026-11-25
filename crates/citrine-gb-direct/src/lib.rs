#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::{Counts, Direct};
#[cfg(target_arch = "wasm32")]
pub use web::Direct;

pub const CONNECT_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    Offer(String),
    Answer(String),
    Candidate(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Note(u64, Note),
    Up(u64),
    Down(u64),
    Frame(u64, Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stun {
    pub host: String,
    pub port: u16,
}
