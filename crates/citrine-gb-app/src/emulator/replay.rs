use super::Emulator;
use crate::replay::ReplayPlayer;
use citrine_gb::console::Console;
use citrine_gb::console::Keys;
use citrine_gb::debug::Debug;
use citrine_gb::machine::Machine;

impl Emulator {
    pub fn play_replay(
        &mut self,
        recording: citrine_gb::recording::Recording,
        game: &str,
    ) -> Result<(), crate::replay::ReplayError> {
        let loaded = self.has_game()
            && self
                .seat
                .core()
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
            let held = self.held_buttons();
            let machine = &mut self.seat.core_mut().machine;
            machine.set_keys(machine.keys() - (player.held() - held));
        }
    }

    pub fn start_recording(&mut self) {
        self.recorder.start(&self.seat.core_mut().machine);
    }
}

pub(super) fn feed(replay: &mut Option<ReplayPlayer>, held: Keys, machine: &mut Machine) {
    let Some(player) = replay.as_mut() else {
        return;
    };
    let cycle = Debug::cycles(machine) as u64 * 4;
    for (button, pressed) in player.due(cycle) {
        if pressed {
            machine.set_keys(machine.keys() | button);
        } else if !held.contains(button) {
            machine.set_keys(machine.keys() - button);
        }
    }
    if player.finished() {
        *replay = None;
    }
}
