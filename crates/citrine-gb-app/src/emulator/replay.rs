use super::Emulator;
use crate::app::ui_state::keybinds::GbButton;
use strum::IntoEnumIterator;

impl Emulator {
    pub fn play_replay(
        &mut self,
        recording: citrine_gb::recording::Recording,
        game: &str,
    ) -> Result<(), crate::replay::ReplayError> {
        let loaded = self.gb.cartridge.has_rom_loaded
            && self
                .gb
                .cartridge
                .header
                .sha256_hex_string()
                .eq_ignore_ascii_case(&recording.rom_sha256);
        if !loaded {
            return Err(crate::replay::ReplayError::OtherGame(game.to_owned()));
        }
        self.stop_replay();
        self.release_all();
        self.restart_on(recording.model)
            .map_err(|_| crate::replay::ReplayError::Unreadable)?;
        self.running = true;
        self.replay = Some(crate::replay::ReplayPlayer::new(recording));
        Ok(())
    }

    pub fn stop_replay(&mut self) {
        if let Some(player) = self.replay.take() {
            for button in GbButton::iter() {
                let joypad = button.joypad();
                if player.held().contains(joypad) && !self.is_held(joypad) {
                    self.gb.release_button(joypad);
                }
            }
        }
    }

    pub(super) fn feed_replay(&mut self) {
        let Some(player) = self.replay.as_mut() else {
            return;
        };
        let cycle = self.gb.debugger.total_cycles as u64 * 4;
        for (button, pressed) in player.due(cycle) {
            if pressed {
                self.gb.press_button(button);
            } else if !self.held.held().contains(button) {
                self.gb.release_button(button);
            }
        }
        if player.finished() {
            self.replay = None;
        }
    }

    pub fn start_recording(&mut self) {
        self.recorder.start(&self.gb);
    }
}
