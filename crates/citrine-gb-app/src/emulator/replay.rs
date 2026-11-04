use super::Emulator;
use citrine_gb::console::Console;
use citrine_gb::debug::Debug;

impl Emulator {
    pub fn play_replay(
        &mut self,
        recording: citrine_gb::recording::Recording,
        game: &str,
    ) -> Result<(), crate::replay::ReplayError> {
        let loaded = self.has_game()
            && self
                .machine
                .rom_id()
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
            self.machine
                .set_keys(self.machine.keys() - (player.held() - self.held_buttons()));
        }
    }

    pub(super) fn feed_replay(&mut self) {
        let Some(player) = self.replay.as_mut() else {
            return;
        };
        let cycle = Debug::cycles(&self.machine) as u64 * 4;
        for (button, pressed) in player.due(cycle) {
            if pressed {
                self.machine.set_keys(self.machine.keys() | button);
            } else if !self.held.held().contains(button) {
                self.machine.set_keys(self.machine.keys() - button);
            }
        }
        if player.finished() {
            self.replay = None;
        }
    }

    pub fn start_recording(&mut self) {
        self.recorder.start(&self.machine);
    }
}
