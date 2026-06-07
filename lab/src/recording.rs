//! Input recordings. The schema is defined in `citrine_gb::recording` so the frontend (recorder)
//! and the lab (replayer) share exactly one format; this module adds file IO convenience.

pub use citrine_gb::recording::{Button, InputEvent, Recording};

use std::path::Path;

/// Load a recording from JSON and ensure its events are sorted by cycle.
pub fn load(path: &Path) -> anyhow::Result<Recording> {
    let json = std::fs::read_to_string(path)?;
    let mut recording = Recording::from_json(&json)?;
    recording.sort();
    Ok(recording)
}

/// Write a recording to a JSON file.
pub fn save(recording: &Recording, path: &Path) -> anyhow::Result<()> {
    std::fs::write(path, recording.to_json()?)?;
    Ok(())
}
