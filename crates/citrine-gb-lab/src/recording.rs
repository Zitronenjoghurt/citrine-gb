//! File IO for the recording schema shared with the app's recorder (`citrine_gb::recording`).

pub use citrine_gb::recording::{Button, InputEvent, Recording};

use std::path::Path;

/// Loads and sorts by cycle.
pub fn load(path: &Path) -> anyhow::Result<Recording> {
    let json = std::fs::read_to_string(path)?;
    let mut recording = Recording::from_json(&json)?;
    recording.sort();
    Ok(recording)
}

pub fn save(recording: &Recording, path: &Path) -> anyhow::Result<()> {
    std::fs::write(path, recording.to_json()?)?;
    Ok(())
}
