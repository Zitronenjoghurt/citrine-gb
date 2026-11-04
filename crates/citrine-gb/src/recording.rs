//! Input recordings: a ROM identity plus a cycle-timed list of button events.
//!
//! Recordings are produced by the frontend while playing and replayed elsewhere (e.g. the lab) to
//! reproduce a session deterministically. The schema lives here so every consumer agrees on it.
//!
//! Cycles are **T-cycles** (~4.19 MHz on DMG), i.e. `GameBoy::debugger.total_cycles * 4`.

use crate::console::Keys;
use crate::gb::GbModel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Button {
    A,
    B,
    Select,
    Start,
    Right,
    Left,
    Up,
    Down,
}

impl Button {
    pub const ALL: [Button; 8] = [
        Button::A,
        Button::B,
        Button::Select,
        Button::Start,
        Button::Right,
        Button::Left,
        Button::Up,
        Button::Down,
    ];

    pub fn from_keys(keys: Keys) -> Option<Self> {
        Button::ALL.into_iter().find(|b| b.to_keys() == keys)
    }

    pub fn to_keys(self) -> Keys {
        match self {
            Button::A => Keys::A,
            Button::B => Keys::B,
            Button::Select => Keys::SELECT,
            Button::Start => Keys::START,
            Button::Right => Keys::RIGHT,
            Button::Left => Keys::LEFT,
            Button::Up => Keys::UP,
            Button::Down => Keys::DOWN,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct InputEvent {
    /// Absolute T-cycle at which the event occurs.
    pub cycle: u64,
    pub button: Button,
    pub pressed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    /// SHA-256 of the ROM the recording was made against (hex), for sanity-checking on replay.
    pub rom_sha256: String,
    /// Optional path hint to the ROM (relative to a `roms/` dir or absolute).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rom_path: Option<String>,
    pub model: GbModel,
    /// Button events, sorted ascending by `cycle`.
    pub events: Vec<InputEvent>,
    /// Cycle at which the recording ends (informational).
    #[serde(default)]
    pub end_cycle: u64,
}

impl Recording {
    pub fn new(rom_sha256: impl Into<String>, model: GbModel) -> Self {
        Self {
            rom_sha256: rom_sha256.into(),
            rom_path: None,
            model,
            events: Vec::new(),
            end_cycle: 0,
        }
    }

    /// Record a button event at an absolute T-cycle.
    pub fn push(&mut self, cycle: u64, button: Button, pressed: bool) {
        self.events.push(InputEvent {
            cycle,
            button,
            pressed,
        });
        self.end_cycle = self.end_cycle.max(cycle);
    }

    /// Ensure events are sorted by cycle (stable, preserving same-cycle order).
    pub fn sort(&mut self) {
        self.events.sort_by_key(|e| e.cycle);
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}
