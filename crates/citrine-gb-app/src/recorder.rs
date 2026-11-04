//! Records button input during live play into a [`Recording`] that the lab can replay.
//!
//! The recorder timestamps each event with the emulator's **absolute** T-cycle
//! (`gb.debugger.total_cycles`, which resets to 0 on ROM load / soft reset). The lab replays from a
//! fresh load (cycle 0), so absolute cycles line up with the reconstructed state, provided
//! recording starts right after a ROM load / soft reset.

use citrine_gb::console::{Console, Keys};
use citrine_gb::gb::GbModel;
use citrine_gb::recording::{Button, Recording};

#[derive(Default)]
pub struct InputRecorder {
    active: bool,
    data: Option<Recording>,
}

impl InputRecorder {
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn has_data(&self) -> bool {
        self.data.as_ref().is_some_and(|d| !d.events.is_empty())
    }

    pub fn event_count(&self) -> usize {
        self.data.as_ref().map_or(0, |d| d.events.len())
    }

    /// Begin a fresh recording, capturing the ROM identity and model from `gb`.
    pub fn start(&mut self, console: &impl Console) {
        let Ok(model) = GbModel::try_from(console.kind()) else {
            return;
        };
        let sha = console.rom_id().sha256_hex_string();
        self.data = Some(Recording::new(sha, model));
        self.active = true;
    }

    pub fn stop(&mut self) {
        self.active = false;
    }

    /// Record a single-button event. `total_cycles` is the absolute `gb.debugger.total_cycles`
    /// (M-cycles); it is converted to absolute T-cycles for the recording.
    pub fn record(&mut self, total_cycles: u128, state: Keys, pressed: bool) {
        if !self.active {
            return;
        }
        let Some(button) = Button::from_keys(state) else {
            return;
        };
        if let Some(data) = &mut self.data {
            data.push(total_cycles as u64 * 4, button, pressed);
        }
    }

    /// Serialize the current recording to pretty JSON.
    pub fn export_json(&self) -> Option<String> {
        self.data.as_ref().and_then(|d| d.to_json().ok())
    }
}
