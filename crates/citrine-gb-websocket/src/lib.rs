#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::{Sender, connect};
#[cfg(target_arch = "wasm32")]
pub use web::{Sender, connect};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Binary(Vec<u8>),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Opened,
    Message(Message),
    Failed(String),
    Closed,
}
